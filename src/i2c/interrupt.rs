//! The interrupt-driven I2C master (ADR-013): `start` returns at once, the
//! I2C's event and error vectors carry the transaction, and its bytes move
//! by DMA or from the event interrupt, on memory the caller lends.
//!
//! - **transaction**: one 7-bit address, a write phase, a read phase, or a
//!   write then a read joined by a repeated start. Each phase is a lent
//!   `&'static mut` buffer and a length of 1 to [`MAX_PHASE`] bytes.
//! - **data path**: per phase, by DMA when the wiring is a [`DmaWiring`]
//!   and the phase is at least the IP version's threshold long, otherwise
//!   from the event interrupt. The thresholds are the bench's crossovers
//!   (ADR-013): a phase shorter than them costs fewer CPU cycles served by
//!   interrupt than arming a stream does.
//! - **end**: [`Service::on_interrupt`](crate::Service) serves the I2C's
//!   event and error vectors and, on `i2c_v1`, the receive stream's; it
//!   reports `Some(Outcome)` once the transaction has ended, with the lent
//!   buffers handed back. No path panics.
//! - **bound**: a slave may stretch SCL without limit, and the driver has
//!   no timer. The caller bounds a transaction with its own timer, with
//!   [`InterruptI2c::bound_cycles`] as the blocking driver's bound, and
//!   calls [`InterruptI2c::abort`] when it passes: the transaction ends
//!   with [`Fault::Aborted`], the buffers come back and the bus is put back
//!   so the next start runs.
//! - **memory**: every DMA phase's buffer is checked on each `start` with
//!   `chip::check_dma_buffer`; a buffer the stream cannot use is refused
//!   with the transaction handed back.
//!
//! Requirements: I2C-007, I2C-008, I2C-009, I2C-010, I2C-011, I2C-012, I2C-013

use core::sync::atomic::{Ordering, fence};

use super::{ConfigError, Error, I2cRxDma, I2cTxDma, Instance, Parts, SclPin, SdaPin, Wiring};
use crate::{
    dma::{AnyStream, Direction, Flags, Interrupts, Priority, Transfer, WordSize},
    gpio::AltPin,
    i2c_ip::Unit,
    rcc::Rcc,
    resource::{Refused, Resource, Signal},
    time::Hertz,
};

/// The longest phase: NBYTES is eight bits on `i2c_v2`, and reload mode is
/// never used (ES0392 2.19.5). The same cap on `i2c_v1`, for one rule.
pub const MAX_PHASE: usize = 255;

/// What the board file hands to [`InterruptI2c::new_dma`]: the instance, its
/// two pins and a stream for each direction. The routing traits check that
/// they belong together; this struct only carries them.
pub struct DmaWiring<U, Scl, Sda, TxD, RxD> {
    pub i2c: U,
    pub scl: Scl,
    pub sda: Sda,
    pub tx_dma: TxD,
    pub rx_dma: RxD,
}

impl<U, Scl, Sda, TxD, RxD> crate::Sealed for DmaWiring<U, Scl, Sda, TxD, RxD> {}
impl<U: Instance, Scl, Sda, TxD, RxD> Parts for DmaWiring<U, Scl, Sda, TxD, RxD> {
    type Instance = U;
}

impl<U, Scl, Sda, const TD: u8, const TN: u8, const RD: u8, const RN: u8>
    DmaWiring<U, Scl, Sda, crate::dma::Stream<TD, TN>, crate::dma::Stream<RD, RN>>
where
    U: Instance,
    Scl: SclPin<U> + AltPin,
    Sda: SdaPin<U> + AltPin,
    crate::dma::Stream<TD, TN>: I2cTxDma<U>,
    crate::dma::Stream<RD, RN>: I2cRxDma<U>,
{
    /// Everything this wiring uses, as data: the instance, its two pins with
    /// their alternate functions, and both streams with their requests.
    pub const RESOURCES: [Resource; 5] = [
        Resource::Peripheral(U::NAME),
        Resource::pin::<Scl>(Signal::Scl),
        Resource::pin::<Sda>(Signal::Sda),
        Resource::Dma {
            signal: Signal::Tx,
            controller: TD,
            stream: TN,
            request: <crate::dma::Stream<TD, TN> as I2cTxDma<U>>::REQUEST,
        },
        Resource::Dma {
            signal: Signal::Rx,
            controller: RD,
            stream: RN,
            request: <crate::dma::Stream<RD, RN> as I2cRxDma<U>>::REQUEST,
        },
    ];
}

/// Why [`InterruptI2c::new_dma`] refused: the reasons the blocking driver
/// has, and a stream an earlier owner left running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum OpenError {
    /// As [`I2c::new`](super::I2c::new).
    Config(ConfigError),
    /// A stream did not report itself stopped in time. Its EN is cleared,
    /// so it may still stop later; by then the I2C has been enabled in RCC.
    DmaBusy,
}

/// What [`InterruptI2c::new_dma`] hands back when it refuses: the reason,
/// and the wiring it took.
pub type DmaRefused<U, Scl, Sda, TxD, RxD> = Refused<OpenError, DmaWiring<U, Scl, Sda, TxD, RxD>>;

/// One lent buffer and how much of it a phase uses.
pub struct Part {
    pub buf: &'static mut [u8],
    pub len: usize,
}

/// One transaction to one 7-bit address.
pub struct Transaction {
    pub address: u8,
    pub write: Option<Part>,
    pub read: Option<Part>,
}

/// Why a transaction ended badly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Fault {
    /// What the I2C reported. The flags were cleared and the bus put back.
    I2c(Error),
    /// A stream failed, or did not move every byte.
    Dma,
    /// [`InterruptI2c::abort`] ended it.
    Aborted,
}

/// A transaction's end: its buffers, back, and what happened.
pub struct Outcome {
    pub transaction: Transaction,
    pub result: Result<(), Fault>,
}

/// Why `start` refused a transaction. The transaction is handed back with
/// it, and nothing was written to the bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum StartError {
    /// A transaction is running.
    Busy,
    /// The bus did not come back idle from the last stop within its bound.
    /// The peripheral was put back; the next start may run.
    BusBusy,
    /// No phase has a byte, or a length is past its buffer.
    Empty,
    /// A phase is longer than [`MAX_PHASE`], or the address is past 7 bits.
    TooLarge,
    /// A DMA phase's buffer is memory the stream can't use.
    Buffer(crate::dma::BufferError),
}

/// Counts since `new`, for health monitoring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct InterruptHealth {
    /// Transactions that completed.
    pub transactions: u32,
    /// Transactions no device acknowledged.
    pub nacks: u32,
    /// Transactions ended by arbitration lost or an overrun.
    pub faults: u32,
    /// Transactions [`InterruptI2c::abort`] ended.
    pub aborts: u32,
    /// Bus errors (BERR), cleared and counted while the transaction went
    /// on, as the errata sheets ask.
    pub bus_errors: u32,
    /// Transactions a stream failed.
    pub dma_errors: u32,
    /// Starts refused because the bus stayed busy.
    pub busy: u32,
    /// Phases whose bytes moved by DMA, and by the event interrupt.
    pub dma_phases: u32,
    pub irq_phases: u32,
}

/// What the IP's state machine is working on.
pub(crate) struct Cursor<'a> {
    pub address: u8,
    pub read: bool,
    pub len: u16,
    pub pos: u16,
    pub dma: bool,
    /// The last phase of the transaction: its end is the stop.
    pub last: bool,
    /// The address was acknowledged (EV6 on `i2c_v1`).
    pub addressed: bool,
    pub buf: &'a mut [u8],
}

/// What one service of the IP found.
pub(crate) enum Step {
    Pending,
    /// A write phase is out and a read follows: start it (a repeated start).
    PhaseDone,
    /// The transaction's stop is armed or out.
    Finished,
    Fail(Error),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Running,
}

/// Register reads `start` allows for the last stop to clear (CR1.STOP on
/// `i2c_v1`, BUSY on both): a stop is one bit time on the wire, and at
/// 1 kHz SCL and one register read a spin this covers it many times over.
const IDLE_SPINS: u32 = 200_000;

/// The interrupt-driven I2C master. It keeps the board's wiring, and
/// [`free`](InterruptI2c::free) hands it back.
pub struct InterruptI2c<W: Parts> {
    unit: Unit,
    tx: Option<(AnyStream, u8)>,
    rx: Option<(AnyStream, u8)>,
    frequency: Hertz,
    kernel_clock: Hertz,
    /// CPU cycles per SCL bit, rounded up.
    bit_cycles: u32,
    state: State,
    job: Option<Transaction>,
    read_phase: bool,
    dma: bool,
    pos: u16,
    addressed: bool,
    health: InterruptHealth,
    wiring: W,
}

/// The timing for `config` on `U`, the SCL it gives, the kernel clock and
/// CPU cycles a bit: every refusal before the hardware is touched.
fn timing<U: Instance>(
    rcc: &Rcc,
    config: &super::Config,
) -> Result<(crate::i2c_ip::Timing, Hertz, u32), ConfigError> {
    let clocks = rcc.clocks();
    let kernel_clock = U::KERNEL.clock(&clocks);
    config.check(kernel_clock)?;
    let timing = Unit::timing(kernel_clock, config.frequency)?;
    let bit_cycles = clocks.cpu.0.div_ceil(timing.frequency().0.max(1));
    Ok((timing, kernel_clock, bit_cycles))
}

impl<U, Scl, Sda> InterruptI2c<Wiring<U, Scl, Sda>>
where
    U: Instance,
    Scl: SclPin<U> + AltPin,
    Sda: SdaPin<U> + AltPin,
{
    /// Opens `wiring` with `config`, every byte moved from the event
    /// interrupt: for an I2C whose streams are taken. A refusal hands the
    /// wiring back with the reason, before anything is written to hardware.
    pub fn new(
        rcc: &mut Rcc,
        wiring: Wiring<U, Scl, Sda>,
        config: &super::Config,
    ) -> Result<Self, Refused<ConfigError, Wiring<U, Scl, Sda>>> {
        let (timing, kernel_clock, bit_cycles) = match timing::<U>(rcc, config) {
            Ok(t) => t,
            Err(error) => return Err(Refused::new(error, wiring)),
        };
        rcc.open(U::ENABLE);
        let unit = U::info().unit();
        unit.apply(&timing);
        unit.irq_enable();
        Ok(Self::with(
            unit,
            None,
            None,
            timing.frequency(),
            kernel_clock,
            bit_cycles,
            wiring,
        ))
    }

    /// Closes the I2C and hands back the wiring. A transaction still running
    /// is abandoned: PE drops with it in flight.
    pub fn free(self) -> Wiring<U, Scl, Sda> {
        self.unit.irq_quiet();
        self.unit.disable();
        self.wiring
    }
}

impl<U, Scl, Sda, TxD, RxD> InterruptI2c<DmaWiring<U, Scl, Sda, TxD, RxD>>
where
    U: Instance,
    Scl: SclPin<U> + AltPin,
    Sda: SdaPin<U> + AltPin,
    TxD: I2cTxDma<U>,
    RxD: I2cRxDma<U>,
{
    /// Opens `wiring` with `config`, phases from the IP version's threshold
    /// up moved by DMA. A refusal hands the wiring back with the reason;
    /// every reason but [`OpenError::DmaBusy`] comes before anything is
    /// written to hardware.
    pub fn new_dma(
        rcc: &mut Rcc,
        wiring: DmaWiring<U, Scl, Sda, TxD, RxD>,
        config: &super::Config,
    ) -> Result<Self, DmaRefused<U, Scl, Sda, TxD, RxD>> {
        let (timing, kernel_clock, bit_cycles) = match timing::<U>(rcc, config) {
            Ok(t) => t,
            Err(error) => return Err(Refused::new(OpenError::Config(error), wiring)),
        };
        let mut tx = crate::dma::claim(&wiring.tx_dma);
        let mut rx = crate::dma::claim(&wiring.rx_dma);
        // Streams left running by an earlier owner must stop before they
        // are pointed at new memory.
        if tx.stop(crate::dma::STOP_SPINS).is_err() || rx.stop(crate::dma::STOP_SPINS).is_err() {
            return Err(Refused::new(OpenError::DmaBusy, wiring));
        }
        tx.clear_flags(Flags::ALL);
        rx.clear_flags(Flags::ALL);
        rcc.open(U::ENABLE);
        let unit = U::info().unit();
        unit.apply(&timing);
        unit.irq_enable();
        Ok(Self::with(
            unit,
            Some((tx, TxD::REQUEST)),
            Some((rx, RxD::REQUEST)),
            timing.frequency(),
            kernel_clock,
            bit_cycles,
            wiring,
        ))
    }

    /// Stops both streams, closes the I2C and hands back the wiring. A
    /// transaction still running is abandoned. A stream that does not stop
    /// in time could still use a lent buffer, so the refusal returns the
    /// driver, still holding it; `free` may be tried again.
    #[allow(clippy::type_complexity)]
    pub fn free(
        mut self,
    ) -> Result<DmaWiring<U, Scl, Sda, TxD, RxD>, Refused<crate::dma::StopTimeout, Self>> {
        self.unit.irq_quiet();
        let mut stopped = true;
        for (stream, _) in self.tx.iter_mut().chain(self.rx.iter_mut()) {
            if stream.stop(crate::dma::STOP_SPINS).is_err() {
                stopped = false;
            }
            stream.clear_flags(Flags::ALL);
        }
        self.unit.disable();
        if !stopped {
            return Err(Refused::new(crate::dma::StopTimeout, self));
        }
        Ok(self.wiring)
    }
}

impl<W: Parts> InterruptI2c<W> {
    fn with(
        unit: Unit,
        tx: Option<(AnyStream, u8)>,
        rx: Option<(AnyStream, u8)>,
        frequency: Hertz,
        kernel_clock: Hertz,
        bit_cycles: u32,
        wiring: W,
    ) -> Self {
        Self {
            unit,
            tx,
            rx,
            frequency,
            kernel_clock,
            bit_cycles,
            state: State::Idle,
            job: None,
            read_phase: false,
            dma: false,
            pos: 0,
            addressed: false,
            health: InterruptHealth::default(),
            wiring,
        }
    }

    /// The shortest write phase a DMA wiring moves by DMA; shorter ones
    /// move from the event interrupt.
    pub const DMA_FROM_WRITE: usize = Unit::DMA_FROM_WRITE;
    /// The shortest read phase a DMA wiring moves by DMA.
    pub const DMA_FROM_READ: usize = Unit::DMA_FROM_READ;

    /// The SCL in use.
    pub fn frequency(&self) -> Hertz {
        self.frequency
    }

    /// The kernel clock SCL is timed from.
    pub fn kernel_clock(&self) -> Hertz {
        self.kernel_clock
    }

    /// The instance's name in the reference manual.
    pub fn name(&self) -> &'static str {
        <W::Instance as Instance>::NAME
    }

    pub fn health(&self) -> InterruptHealth {
        self.health
    }

    /// A transaction is running.
    pub fn is_busy(&self) -> bool {
        self.state == State::Running
    }

    /// The bound the blocking driver puts on a transaction of `write` then
    /// `read` bytes: twice its wire time plus the margin, in CPU cycles.
    pub fn bound_cycles(&self, write: usize, read: usize) -> u32 {
        let mut bits = 0u32;
        if write > 0 {
            bits = bits.saturating_add(super::wire_bits(write));
        }
        if read > 0 {
            bits = bits.saturating_add(super::wire_bits(read));
        }
        bits.saturating_mul(self.bit_cycles)
            .saturating_mul(2)
            .saturating_add(crate::wait::MARGIN_CYCLES)
    }

    /// Whether a phase of `len` bytes in this direction moves by DMA.
    fn uses_dma(&self, read: bool, len: usize) -> bool {
        let (wired, from) = if read {
            (self.rx.is_some(), Self::DMA_FROM_READ)
        } else {
            (self.tx.is_some(), Self::DMA_FROM_WRITE)
        };
        wired && len >= from
    }

    /// Starts `transaction` and returns; [`on_interrupt`](Self::on_interrupt)
    /// reports its end. A refusal hands the transaction back with nothing on
    /// the bus.
    pub fn start(
        &mut self,
        transaction: Transaction,
    ) -> Result<(), Refused<StartError, Transaction>> {
        if let Err(error) = self.check(&transaction) {
            return Err(Refused::new(error, transaction));
        }
        // The last transaction's stop must be off the wire.
        let mut spins = 0;
        while !self.unit.irq_idle() {
            spins += 1;
            if spins >= IDLE_SPINS {
                self.health.busy = self.health.busy.saturating_add(1);
                // A stop that never got out: the peripheral put back, so
                // the next start may run.
                self.unit.recover();
                return Err(Refused::new(StartError::BusBusy, transaction));
            }
        }
        let first_read = transaction.write.is_none();
        self.job = Some(transaction);
        self.state = State::Running;
        self.unit.irq_clear();
        if self.begin(first_read).is_err() {
            // A stream that would not start: nothing went on the wire.
            self.unit.irq_quiet();
            return match self.finish(Err(Fault::Dma)) {
                Some(outcome) => Err(Refused::new(
                    StartError::Buffer(crate::dma::BufferError::Unreachable),
                    outcome.transaction,
                )),
                None => Ok(()),
            };
        }
        Ok(())
    }

    fn check(&self, t: &Transaction) -> Result<(), StartError> {
        if self.state == State::Running {
            return Err(StartError::Busy);
        }
        if t.address > 0x7F {
            return Err(StartError::TooLarge);
        }
        let mut any = false;
        for (part, read) in [(&t.write, false), (&t.read, true)] {
            if let Some(part) = part {
                if part.len == 0 || part.len > part.buf.len() {
                    return Err(StartError::Empty);
                }
                if part.len > MAX_PHASE {
                    return Err(StartError::TooLarge);
                }
                if self.uses_dma(read, part.len)
                    && let Some(used) = part.buf.get(..part.len)
                {
                    crate::chip::check_dma_buffer(used).map_err(StartError::Buffer)?;
                }
                any = true;
            }
        }
        if any { Ok(()) } else { Err(StartError::Empty) }
    }

    /// Arms a phase: its stream if it uses one, then the IP's start.
    fn begin(&mut self, read: bool) -> Result<(), ()> {
        let Some(job) = self.job.as_mut() else {
            return Err(());
        };
        let restart = read && job.write.is_some();
        let last = read || job.read.is_none();
        let address = job.address;
        let Some(part) = (if read {
            job.read.as_mut()
        } else {
            job.write.as_mut()
        }) else {
            return Err(());
        };
        let len = part.len;
        let ptr = part.buf.as_mut_ptr() as u32;
        let dma = self.uses_dma(read, len);
        self.read_phase = read;
        self.dma = dma;
        self.pos = 0;
        self.addressed = false;
        if dma {
            self.health.dma_phases = self.health.dma_phases.saturating_add(1);
            let data = self.unit.irq_data_address(read);
            let (stream, request) = if read {
                self.rx.as_mut()
            } else {
                self.tx.as_mut()
            }
            .ok_or(())?;
            stream.clear_flags(Flags::ALL);
            let interrupts = if read && Unit::READ_ENDS_ON_STREAM {
                Interrupts {
                    complete: true,
                    half: false,
                    error: true,
                    fifo_error: false,
                }
            } else {
                Interrupts::default()
            };
            let transfer = Transfer::one_shot(
                *request,
                if read {
                    Direction::PeripheralToMemory
                } else {
                    Direction::MemoryToPeripheral
                },
                data,
                ptr,
                len as u16,
                WordSize::Byte,
                Priority::High,
            )
            .with_interrupts(interrupts);
            // SAFETY: the buffer is the lent transaction's, held in `job`
            // until the stream has stopped (`finish` stops it before the
            // job is handed back, and keeps the job if it won't stop), at
            // least `len` long and checked reachable by `check_dma_buffer`
            // (`check`); the address is this I2C's data register, which the
            // request selects.
            unsafe { stream.start(&transfer) }.map_err(|_| ())?;
        } else {
            self.health.irq_phases = self.health.irq_phases.saturating_add(1);
        }
        let cursor = Cursor {
            address,
            read,
            len: len as u16,
            pos: 0,
            dma,
            last,
            addressed: false,
            buf: &mut [],
        };
        self.unit.irq_begin(&cursor, restart);
        Ok(())
    }

    /// Ends the running transaction: the I2C quiet, both streams stopped,
    /// the buffers handed back. A stream that won't stop keeps its buffer:
    /// `None`, and the driver stays busy-free but holds the job.
    fn finish(&mut self, result: Result<(), Fault>) -> Option<Outcome> {
        self.unit.irq_quiet();
        let mut stopped = true;
        for (stream, _) in self.tx.iter_mut().chain(self.rx.iter_mut()) {
            if stream.stop(crate::dma::STOP_SPINS).is_err() {
                stopped = false;
            }
            stream.clear_flags(Flags::ALL);
        }
        fence(Ordering::SeqCst);
        let result = if stopped { result } else { Err(Fault::Dma) };
        let h = &mut self.health;
        match result {
            Ok(()) => h.transactions = h.transactions.saturating_add(1),
            Err(Fault::I2c(Error::Nack)) => h.nacks = h.nacks.saturating_add(1),
            Err(Fault::Dma) => h.dma_errors = h.dma_errors.saturating_add(1),
            Err(Fault::Aborted) => h.aborts = h.aborts.saturating_add(1),
            Err(Fault::I2c(_)) => h.faults = h.faults.saturating_add(1),
        }
        self.state = State::Idle;
        if !stopped {
            return None;
        }
        let transaction = self.job.take()?;
        Some(Outcome {
            transaction,
            result,
        })
    }

    /// Ends a running transaction, the caller's bound having passed, and
    /// puts the bus back: [`Fault::Aborted`], with the buffers. `None` if
    /// nothing ran.
    pub fn abort(&mut self) -> Option<Outcome> {
        if self.state != State::Running {
            return None;
        }
        let outcome = self.finish(Err(Fault::Aborted));
        self.unit.irq_abort();
        outcome
    }

    /// Serves the I2C's vectors (and, on `i2c_v1`, the receive stream's):
    /// `Some` once the transaction has ended.
    pub fn on_interrupt(&mut self) -> Option<Outcome> {
        if self.state != State::Running {
            self.unit.irq_quiet();
            return None;
        }
        let dma_failed = self
            .tx
            .iter()
            .chain(self.rx.iter())
            .any(|(s, _)| s.flags().failed());
        if dma_failed {
            self.unit.irq_fail();
            return self.finish(Err(Fault::Dma));
        }
        if self.read_phase
            && self.dma
            && Unit::READ_ENDS_ON_STREAM
            && self.rx.as_ref().is_some_and(|(s, _)| s.flags().complete)
        {
            self.unit.irq_read_dma_done();
            return self.finish(Ok(()));
        }
        let read = self.read_phase;
        let last = read || self.job.as_ref().is_none_or(|j| j.read.is_none());
        let (step, pos, addressed, bus_errors) = {
            let Some(job) = self.job.as_mut() else {
                return self.finish(Err(Fault::Dma));
            };
            let address = job.address;
            let Some(part) = (if read {
                job.read.as_mut()
            } else {
                job.write.as_mut()
            }) else {
                return self.finish(Err(Fault::Dma));
            };
            let mut cursor = Cursor {
                address,
                read,
                len: part.len as u16,
                pos: self.pos,
                dma: self.dma,
                last,
                addressed: self.addressed,
                buf: &mut part.buf[..],
            };
            let mut bus_errors = 0;
            let step = self.unit.irq_service(&mut cursor, &mut bus_errors);
            (step, cursor.pos, cursor.addressed, bus_errors)
        };
        self.pos = pos;
        self.addressed = addressed;
        self.health.bus_errors = self.health.bus_errors.saturating_add(bus_errors);
        match step {
            Step::Pending => None,
            Step::Fail(error) => {
                self.unit.irq_fail();
                self.finish(Err(Fault::I2c(error)))
            }
            Step::PhaseDone => {
                if self.dma && !self.stream_drained(false) {
                    self.unit.irq_fail();
                    return self.finish(Err(Fault::Dma));
                }
                if let Some((stream, _)) = self.tx.as_mut() {
                    let _ = stream.stop(crate::dma::STOP_SPINS);
                    stream.clear_flags(Flags::ALL);
                }
                if self.job.as_ref().is_none_or(|j| j.read.is_none()) {
                    self.unit.irq_stop();
                    return self.finish(Ok(()));
                }
                if self.begin(true).is_err() {
                    self.unit.irq_fail();
                    return self.finish(Err(Fault::Dma));
                }
                None
            }
            Step::Finished => {
                if self.dma && !self.stream_drained(read) {
                    return self.finish(Err(Fault::Dma));
                }
                self.finish(Ok(()))
            }
        }
    }

    fn stream_drained(&self, read: bool) -> bool {
        let route = if read {
            self.rx.as_ref()
        } else {
            self.tx.as_ref()
        };
        route.is_some_and(|(s, _)| s.remaining() == 0)
    }
}

impl<W: Parts> crate::resource::Service for InterruptI2c<W> {
    type Event = Option<Outcome>;

    fn on_interrupt(&mut self) -> Self::Event {
        InterruptI2c::on_interrupt(self)
    }
}
