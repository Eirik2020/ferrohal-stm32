//! The SPI master on a stream DMA (`dma_v1`, `dma_v2`), for any SPI IP
//! version. The IP module (`spi_v2`, `spi_v4`) supplies `Unit`, its
//! register operations; everything here is the same on every chip.
//!
//! # How a transfer runs
//!
//! `start` copies the bytes to send into the transmit buffer, arms the
//! receive stream, then the transmit stream, then lets the SPI request
//! (the order both reference manuals give). Every byte sent clocks one in,
//! so the receive stream completes last: its complete interrupt, served by
//! [`Spi::on_interrupt`], ends the transfer. The transmit stream raises no
//! interrupt; its error flags are read at the same time.
//!
//! One owner: the firmware shares the `Spi` between the task that starts
//! transfers and the receive stream's handler (an RTIC lock, or one task at
//! the stream's priority). FerroHAL takes no lock.
//!
//! # Waits
//!
//! The blocking forms (`transfer`, `SpiBus`) wait for completion within a
//! bound: twice the transfer's time on the wire, plus a fixed margin, in
//! CPU cycles, measured with a [`CycleClock`] when one is set, otherwise
//! counted as passes of at least `PASS_CYCLES`. A transfer that runs past
//! it is aborted and reported as [`Error::Timeout`].
//!
//! # Memory
//!
//! Both buffers must be memory the chosen DMA controller can reach, and on
//! a chip with a data cache, memory the MPU marks non-cacheable. `Spi::new`
//! checks both with `chip::check_dma_buffer`.
//!
//! Requirements: SPI-002, SPI-003, SPI-004, SPI-005, SPI-006, SPI-007, SPI-008

use core::sync::atomic::{Ordering, fence};

use crate::{
    chip::Interrupt,
    dma::{
        Channel, Direction, FifoMode, Flags, Interrupts, Priority, Stream, StreamInstance,
        StreamInterrupt, Transfer, WordSize,
    },
    gpio::AltPin,
    rcc::{Rcc, RccBit},
    resource::{Resource, Signal},
    spi::{Buffers, Config, ConfigError, Error, Health, Kernel, SpiInterrupts, Wiring},
    spi_ip::Unit,
    time::{CycleClock, Hertz},
    wait::Deadline,
};

/// Spins allowed for a DMA stream to report itself stopped.
const STOP_SPINS: u32 = 10_000;
/// The fewest CPU cycles one pass of the completion wait can take, used
/// when no [`CycleClock`] is set: the fastest pass measured with the DWT
/// (`hil` "a polling pass takes no less than the driver's floor"), halved.
/// H753 at 400 MHz with caches on, 233 cycles; the F401 at 84 MHz takes
/// 324 and the H753 with caches off about 690, so their counted waits run
/// up to about 3 and 6 times the time asked. A `CycleClock` makes every
/// wait exact.
const PASS_CYCLES: u32 = 112;
/// Added to every blocking bound, in CPU cycles: the start-up and the
/// interrupt latency a transfer can see before its first SCK edge.
const MARGIN_CYCLES: u32 = 20_000;

/// Per-instance facts. Chip facts: the glue layer holds one static `Info`
/// per SPI.
pub struct Info {
    unit: Unit,
    enable: RccBit,
}

// SAFETY: `unit` is an address.
unsafe impl Sync for Info {}

impl Info {
    pub(crate) const fn new(unit: Unit, enable: RccBit) -> Self {
        Self { unit, enable }
    }

    pub(crate) fn unit(&self) -> Unit {
        self.unit
    }

    pub(crate) fn enable(&self) -> RccBit {
        self.enable
    }
}

/// An SPI instance this chip has. Chip facts: the glue layer implements it.
pub trait Instance: crate::Sealed + 'static {
    /// Where its kernel clock comes from.
    const KERNEL: Kernel;
    /// Its name in the reference manual.
    const NAME: &'static str;
    #[doc(hidden)]
    fn info() -> &'static Info;
}

/// A pin, in the right alternate function, that carries `U`'s SCK.
pub trait SckPin<U: Instance>: crate::Sealed {}
/// A pin, in the right alternate function, that carries `U`'s MISO.
pub trait MisoPin<U: Instance>: crate::Sealed {}
/// A pin, in the right alternate function, that carries `U`'s MOSI.
pub trait MosiPin<U: Instance>: crate::Sealed {}

/// A DMA stream that serves `U`'s transmit requests.
///
/// # Safety
///
/// `REQUEST` must be the request that routes `U`'s TX to this stream.
pub unsafe trait SpiTxDma<U: Instance>: StreamInstance + StreamInterrupt {
    const REQUEST: u8;
}

/// A DMA stream that serves `U`'s receive requests.
///
/// # Safety
///
/// As [`SpiTxDma`], for RX.
pub unsafe trait SpiRxDma<U: Instance>: StreamInstance + StreamInterrupt {
    const REQUEST: u8;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Running(u16),
    Done(u16),
}

/// An SPI master, full duplex over two DMA streams.
pub struct Spi {
    info: &'static Info,
    tx: Channel,
    rx: Channel,
    tx_request: u8,
    rx_request: u8,
    tx_buf: &'static mut [u8],
    rx_buf: &'static mut [u8],
    /// The longest transfer: the shorter buffer.
    cap: u16,
    state: State,
    kernel_clock: Hertz,
    cpu: Hertz,
    frequency: Hertz,
    /// CPU cycles per SCK period, rounded up.
    sck_cycles: u32,
    clock: Option<&'static dyn CycleClock>,
    vector: Interrupt,
    health: Health,
}

impl Spi {
    /// Opens `wiring` with `config`: enables and resets the SPI, claims
    /// both streams and configures the bus as a master. Chip select is the
    /// firmware's GPIO. Returns the SCK in use through
    /// [`frequency`](Self::frequency).
    pub fn new<U, Sck, Miso, Mosi, TxD, RxD>(
        rcc: &mut Rcc,
        wiring: Wiring<U, Sck, Miso, Mosi, TxD, RxD>,
        config: &Config,
        buffers: Buffers,
    ) -> Result<Self, ConfigError>
    where
        U: Instance,
        Sck: SckPin<U>,
        Miso: MisoPin<U>,
        Mosi: MosiPin<U>,
        TxD: SpiTxDma<U>,
        RxD: SpiRxDma<U>,
    {
        let info = U::info();
        let clocks = rcc.clocks();
        let kernel_clock = U::KERNEL
            .frequency(&clocks)
            .ok_or(ConfigError::KernelClock)?;
        let code = config.divider(kernel_clock)?;
        let tx_len = u16::try_from(buffers.tx.len())
            .ok()
            .filter(|&len| len > 0)
            .ok_or(ConfigError::TxBuffer)?;
        let rx_len = u16::try_from(buffers.rx.len())
            .ok()
            .filter(|&len| len > 0)
            .ok_or(ConfigError::RxBuffer)?;
        crate::chip::check_dma_buffer(buffers.tx)?;
        crate::chip::check_dma_buffer(buffers.rx)?;

        rcc.enable(info.enable);
        rcc.reset(info.enable);
        let mut tx = wiring.tx_dma.into_channel();
        let mut rx = wiring.rx_dma.into_channel();
        // Streams left running by an earlier owner must stop before they
        // are pointed at new memory.
        tx.stop(STOP_SPINS).map_err(|_| ConfigError::DmaBusy)?;
        rx.stop(STOP_SPINS).map_err(|_| ConfigError::DmaBusy)?;
        tx.clear_flags(Flags::ALL);
        rx.clear_flags(Flags::ALL);
        info.unit.configure(code, config);

        let frequency = Hertz(kernel_clock.0 >> (code + 1));
        Ok(Self {
            info,
            tx,
            rx,
            tx_request: TxD::REQUEST,
            rx_request: RxD::REQUEST,
            tx_buf: buffers.tx,
            rx_buf: buffers.rx,
            cap: tx_len.min(rx_len),
            state: State::Idle,
            kernel_clock,
            cpu: clocks.cpu,
            frequency,
            sck_cycles: clocks.cpu.0.div_ceil(frequency.0.max(1)),
            clock: None,
            vector: RxD::INTERRUPT,
            health: Health::default(),
        })
    }

    /// The SCK `config` would give on `U` under `clocks`. A `const fn`, so
    /// a board can assert its buses while compiling.
    pub const fn check<U: Instance>(
        clocks: &crate::rcc::Clocks,
        config: &Config,
    ) -> Result<Hertz, ConfigError> {
        match U::KERNEL.frequency(clocks) {
            Some(kernel_clock) => config.check(kernel_clock),
            None => Err(ConfigError::KernelClock),
        }
    }

    /// Changes the bus settings between transfers. Returns the new SCK.
    pub fn set_config(&mut self, config: &Config) -> Result<Hertz, ConfigError> {
        if matches!(self.state, State::Running(_)) {
            return Err(ConfigError::Busy);
        }
        let code = config.divider(self.kernel_clock)?;
        self.info.unit.configure(code, config);
        self.frequency = Hertz(self.kernel_clock.0 >> (code + 1));
        self.sck_cycles = self.cpu.0.div_ceil(self.frequency.0.max(1));
        Ok(self.frequency)
    }

    /// The SCK in use.
    pub fn frequency(&self) -> Hertz {
        self.frequency
    }

    /// The longest transfer: the shorter of the two buffers.
    pub fn capacity(&self) -> usize {
        usize::from(self.cap)
    }

    /// Measures blocking waits with `clock` instead of counting passes.
    pub fn set_clock(&mut self, clock: Option<&'static dyn CycleClock>) {
        self.clock = clock;
    }

    /// The vector to bind to [`on_interrupt`](Self::on_interrupt).
    pub fn interrupts(&self) -> SpiInterrupts {
        SpiInterrupts {
            rx_dma: self.vector,
        }
    }

    pub fn health(&self) -> Health {
        self.health
    }

    pub fn is_busy(&self) -> bool {
        matches!(self.state, State::Running(_))
    }

    /// Starts one full-duplex transfer of `tx.len()` bytes and returns; the
    /// receive stream's interrupt (or [`poll`](Self::poll)) ends it.
    pub fn start(&mut self, tx: &[u8]) -> Result<(), Error> {
        if self.is_busy() {
            return Err(Error::Busy);
        }
        let len = u16::try_from(tx.len()).map_err(|_| Error::TooLarge)?;
        if len == 0 {
            return Err(Error::Empty);
        }
        if len > self.cap {
            return Err(Error::TooLarge);
        }
        if let Some(buf) = self.tx_buf.get_mut(..tx.len()) {
            buf.copy_from_slice(tx);
        }
        self.begin(len)
    }

    fn begin(&mut self, len: u16) -> Result<(), Error> {
        let unit = self.info.unit;
        self.tx.clear_flags(Flags::ALL);
        self.rx.clear_flags(Flags::ALL);
        unit.enable_rx_dma(len);
        let rx = Transfer {
            request: self.rx_request,
            #[cfg(feature = "dma_v1")]
            bufferable: false,
            direction: Direction::PeripheralToMemory,
            peripheral: unit.rx_address(),
            memory: self.rx_buf.as_mut_ptr() as u32,
            double_buffer: None,
            len,
            size: WordSize::Byte,
            memory_increment: true,
            circular: false,
            priority: Priority::VeryHigh,
            fifo: FifoMode::Direct,
            interrupts: Interrupts {
                complete: true,
                error: true,
                ..Interrupts::default()
            },
        };
        let tx = Transfer {
            request: self.tx_request,
            direction: Direction::MemoryToPeripheral,
            peripheral: unit.tx_address(),
            memory: self.tx_buf.as_ptr() as u32,
            priority: Priority::High,
            interrupts: Interrupts::default(),
            ..rx
        };
        // SAFETY: both buffers are the driver's own for its whole life, at
        // least `len` long (`len <= cap`), and nothing else touches them
        // until the transfer ends; the addresses are this SPI's data
        // registers, which the requests select.
        let started = unsafe { self.rx.start(&rx).and_then(|()| self.tx.start(&tx)) };
        if started.is_err() {
            self.reset_bus();
            self.health.dma_errors = self.health.dma_errors.saturating_add(1);
            return Err(Error::Dma);
        }
        unit.go();
        self.state = State::Running(len);
        Ok(())
    }

    /// Serves the receive stream's interrupt: `Some` once the running
    /// transfer has ended, with its length or why it failed. `None` if
    /// nothing has ended (or nothing was running).
    pub fn on_interrupt(&mut self) -> Option<Result<usize, Error>> {
        let State::Running(len) = self.state else {
            self.rx.clear_flags(Flags::ALL);
            return None;
        };
        let rx = self.rx.flags();
        let tx = self.tx.flags();
        // A FIFO error in direct mode is advisory; the others stop a stream.
        if rx.transfer_error || rx.direct_mode_error || tx.transfer_error || tx.direct_mode_error {
            self.fail();
            self.health.dma_errors = self.health.dma_errors.saturating_add(1);
            return Some(Err(Error::Dma));
        }
        if !rx.complete {
            return None;
        }
        let unit = self.info.unit;
        // The last byte is in; the SPI finishes its frame within an SCK or
        // two. Counted: a register read per pass, at least a cycle each.
        let mut deadline = Deadline::new(None, self.sck_cycles.saturating_mul(32), 1);
        while !unit.done() {
            if deadline.expired() {
                self.fail();
                self.health.timeouts = self.health.timeouts.saturating_add(1);
                return Some(Err(Error::Timeout));
            }
        }
        if Unit::HOLD_AFTER_END {
            // ES0392 2.22.6: disabling right after EOT can cut the last SCK
            // pulse short at a slow SCK. Hold one SCK period first.
            cortex_m::asm::delay(self.sck_cycles);
        }
        let (overrun, mode_fault) = unit.take_errors();
        unit.end();
        self.rx.clear_flags(Flags::ALL);
        self.tx.clear_flags(Flags::ALL);
        // Nothing the stream wrote may be read before this.
        fence(Ordering::SeqCst);
        if mode_fault {
            self.state = State::Idle;
            self.health.mode_faults = self.health.mode_faults.saturating_add(1);
            self.reset_bus();
            return Some(Err(Error::ModeFault));
        }
        if overrun {
            self.state = State::Idle;
            self.health.overruns = self.health.overruns.saturating_add(1);
            return Some(Err(Error::Overrun));
        }
        self.state = State::Done(len);
        self.health.transfers = self.health.transfers.saturating_add(1);
        Some(Ok(usize::from(len)))
    }

    /// [`on_interrupt`](Self::on_interrupt), for firmware that polls
    /// instead of binding the vector.
    pub fn poll(&mut self) -> Option<Result<usize, Error>> {
        self.on_interrupt()
    }

    /// The bytes the last completed transfer clocked in; empty while one is
    /// running or after a failure.
    pub fn received(&self) -> &[u8] {
        match self.state {
            State::Done(len) => self.rx_buf.get(..usize::from(len)).unwrap_or(&[]),
            _ => &[],
        }
    }

    /// Stops a running transfer, if any, and leaves the bus idle and ready.
    /// The watchdog's recovery: safe to call at any time.
    pub fn abort(&mut self) {
        if self.is_busy() {
            self.health.aborts = self.health.aborts.saturating_add(1);
        }
        self.reset_bus();
    }

    /// One transfer, waited for: sends `tx` and returns what came back.
    pub fn transfer(&mut self, tx: &[u8]) -> Result<&[u8], Error> {
        self.start(tx)?;
        self.wait_done(tx.len())?;
        Ok(self.received())
    }

    /// Waits for the running transfer of `len` bytes, polling.
    fn wait_done(&mut self, len: usize) -> Result<(), Error> {
        let bits = u32::try_from(len).unwrap_or(u32::MAX).saturating_mul(8);
        let cycles = bits
            .saturating_mul(self.sck_cycles)
            .saturating_mul(2)
            .saturating_add(MARGIN_CYCLES);
        let mut deadline = Deadline::new(self.clock, cycles, PASS_CYCLES);
        loop {
            if let Some(result) = self.on_interrupt() {
                return result.map(|_| ());
            }
            if deadline.expired() {
                if let Some(result) = self.on_interrupt() {
                    return result.map(|_| ());
                }
                self.fail();
                self.health.timeouts = self.health.timeouts.saturating_add(1);
                return Err(Error::Timeout);
            }
        }
    }

    /// Stops a failed transfer and counts it as aborted.
    fn fail(&mut self) {
        self.health.aborts = self.health.aborts.saturating_add(1);
        self.reset_bus();
    }

    /// Both streams stopped, their flags cleared, the SPI idle.
    fn reset_bus(&mut self) {
        // A stream that won't stop leaves nothing better to do than carry
        // on: the next start reports it as `Error::Dma`.
        let _ = self.tx.stop(STOP_SPINS);
        let _ = self.rx.stop(STOP_SPINS);
        // Two frames in flight at most; a register read per spin, at least
        // a cycle each, so this bound covers them twice over.
        self.info
            .unit
            .reset_state(self.sck_cycles.saturating_mul(32));
        self.tx.clear_flags(Flags::ALL);
        self.rx.clear_flags(Flags::ALL);
        fence(Ordering::SeqCst);
        self.state = State::Idle;
    }

    /// Runs `len` bytes through the buffers for the blocking forms: `fill`
    /// writes what to send; the result is what came back.
    fn blocking(&mut self, len: usize, fill: impl FnOnce(&mut [u8])) -> Result<&[u8], Error> {
        if self.is_busy() {
            return Err(Error::Busy);
        }
        let n = u16::try_from(len).map_err(|_| Error::TooLarge)?;
        if n == 0 {
            return Ok(&[]);
        }
        if n > self.cap {
            return Err(Error::TooLarge);
        }
        if let Some(buf) = self.tx_buf.get_mut(..len) {
            fill(buf);
        }
        self.begin(n)?;
        self.wait_done(len)?;
        Ok(self.received())
    }

    /// The split for the blocking forms. `cap` is at least 1 by
    /// construction; the `max` lets the compiler see it, so `chunks` has no
    /// panic path.
    fn chunk(&self) -> usize {
        usize::from(self.cap).max(1)
    }
}

/// Copies as much of `src` as fits into `dst`.
fn copy(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len());
    if let (Some(dst), Some(src)) = (dst.get_mut(..n), src.get(..n)) {
        dst.copy_from_slice(src);
    }
}

impl embedded_hal::spi::ErrorType for Spi {
    type Error = Error;
}

/// Bounded: every operation is split into transfers of at most
/// [`capacity`](Spi::capacity) bytes, each waited for within its bound.
/// The bytes sent while reading are 0x00.
impl embedded_hal::spi::SpiBus<u8> for Spi {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let chunk = self.chunk();
        for part in words.chunks_mut(chunk) {
            let rx = self.blocking(part.len(), |tx| tx.fill(0))?;
            copy(part, rx);
        }
        Ok(())
    }

    fn write(&mut self, words: &[u8]) -> Result<(), Error> {
        let chunk = self.chunk();
        for part in words.chunks(chunk) {
            self.blocking(part.len(), |tx| copy(tx, part))?;
        }
        Ok(())
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        let chunk = self.chunk();
        let total = read.len().max(write.len());
        let mut at = 0;
        while at < total {
            let len = chunk.min(total - at);
            let out = write.get(at..).unwrap_or(&[]);
            let rx = self.blocking(len, |tx| {
                tx.fill(0);
                copy(tx, out);
            })?;
            copy(read.get_mut(at..).unwrap_or(&mut []), rx);
            at += len;
        }
        Ok(())
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let chunk = self.chunk();
        for part in words.chunks_mut(chunk) {
            let rx = self.blocking(part.len(), |tx| copy(tx, part))?;
            copy(part, rx);
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        // Every blocking operation has finished by the time it returns; a
        // transfer started with `start` is the interrupt's to end.
        if self.is_busy() {
            Err(Error::Busy)
        } else {
            Ok(())
        }
    }
}

impl<U, Sck, Miso, Mosi, const TD: u8, const TN: u8, const RD: u8, const RN: u8>
    Wiring<U, Sck, Miso, Mosi, Stream<TD, TN>, Stream<RD, RN>>
where
    U: Instance,
    Sck: SckPin<U> + AltPin,
    Miso: MisoPin<U> + AltPin,
    Mosi: MosiPin<U> + AltPin,
    Stream<TD, TN>: SpiTxDma<U>,
    Stream<RD, RN>: SpiRxDma<U>,
{
    /// Everything this wiring uses, as data: the instance, its three pins
    /// with their alternate functions, both streams with their requests,
    /// and the receive stream's vector.
    pub const RESOURCES: [Resource; 7] = [
        Resource::Peripheral(U::NAME),
        Resource::Pin {
            signal: Signal::Sck,
            port: Sck::PORT,
            pin: Sck::PIN,
            af: Sck::AF,
        },
        Resource::Pin {
            signal: Signal::Miso,
            port: Miso::PORT,
            pin: Miso::PIN,
            af: Miso::AF,
        },
        Resource::Pin {
            signal: Signal::Mosi,
            port: Mosi::PORT,
            pin: Mosi::PIN,
            af: Mosi::AF,
        },
        Resource::Dma {
            signal: Signal::Tx,
            controller: TD,
            stream: TN,
            request: <Stream<TD, TN> as SpiTxDma<U>>::REQUEST,
        },
        Resource::Dma {
            signal: Signal::Rx,
            controller: RD,
            stream: RN,
            request: <Stream<RD, RN> as SpiRxDma<U>>::REQUEST,
        },
        Resource::Interrupt(<Stream<RD, RN> as StreamInterrupt>::NAME),
    ];
}
