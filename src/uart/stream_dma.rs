//! The UART driver on a stream DMA (`dma_v1`, `dma_v2`), for any USART IP
//! version. The IP module (`usart_v2`, `usart_v4`) supplies `Line`, its
//! register operations; everything here is the same on every chip.
//!
//! # How it moves bytes
//!
//! - **Transmit**: `write` copies into the driver's transmit buffer and
//!   starts one DMA transfer from it; it returns as soon as the transfer
//!   is running. The next `write`, or `flush`, waits for it. Nothing needs
//!   an interrupt: the stream's flags are polled.
//! - **Receive**: one DMA stream runs for ever in circular mode, filling the
//!   receive ring. `read` copies out what has arrived. The stream's
//!   half and complete interrupts, served by [`UartIrq`], count the ring's
//!   half-laps, which is how `read` tells whether it has fallen a whole lap
//!   behind. `read` also counts the flags the interrupt has not served yet,
//!   so the interrupt may run up to a full lap late.
//! - **Errors**: line errors (overrun, framing, parity, noise) raise the
//!   USART interrupt, and DMA errors the stream's. [`UartIrq`] records each
//!   one; `read` reports them in turn, and reception carries on. A DMA bus
//!   error stops the stream; the next `read` reports it and restarts the
//!   ring.
//!
//! # Interrupts
//!
//! FerroHAL enables the peripheral's own interrupt bits (USART: errors;
//! receive stream: half, complete, errors) and nothing else. The scheduler
//! binds the USART vector and the receive stream's vector, and calls
//! [`UartIrq::on_interrupt`] from both. The transmit stream's vector is not
//! used.
//!
//! # Waits
//!
//! Every wait is a counted loop. Its bound starts as the frame time at the
//! configured baud rate, in CPU cycles, and becomes a number of passes by
//! dividing by the fewest cycles one pass of that loop takes
//! (`RX_PASS_CYCLES`, `TX_PASS_CYCLES`, or one cycle for a bare register
//! poll). Since no pass is faster than that, no bound is shorter than the
//! time the hardware needs; the HIL check `wait_bounds` measures that this
//! holds, and how far over it runs. The exception is
//! `embedded_io::Read::read`, whose contract is to block until a byte
//! arrives; [`UartRx::read_within`] is the bounded form.
//!
//! # Memory
//!
//! Both buffers must be memory the chosen DMA controller can reach. On a
//! chip with a data cache, while it is on, they must also be memory the MPU
//! marks non-cacheable: this driver does no cache maintenance. `Uart::new`
//! checks both with `chip::check_dma_buffer`.
//!
//! Requirements: UART-002, UART-003, UART-004, UART-005, UART-006, UART-007, UART-008, UART-009, UART-010, UART-011, UART-012, UART-013, UART-014, UART-015, UARTDMA-002, UARTDMA-008, UARTDMA-009, API-002

use core::{
    marker::PhantomData,
    ptr,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering, fence},
};

use crate::{
    chip::Interrupt,
    dma::{
        Channel, Direction, FifoMode, FlagHandle, Flags, Interrupts, Priority, Stream,
        StreamInstance, StreamInterrupt, Transfer, WordSize,
    },
    gpio::AltPin,
    rcc::{Bus, Clocks, Rcc, RccBit},
    time::Hertz,
    uart::{
        Buffers, Chunk, ChunkEnd, Config, ConfigError, Error, Health, Refused, Resource, RxWiring,
        SendError, Signal, TxHealth, UartInterrupts, Wiring,
        ring::{self, Mark, MarkKind, MarkQueue, Take},
        tx_ring::{self, FRAMES, Frame, FrameQueue},
    },
    usart::LineRegs,
    wait::{Deadline, wait},
};

use crate::time::CycleClock;

/// Spins allowed for a DMA stream to report itself stopped. A stopping
/// stream finishes at most one word, so this is generous.
const STOP_SPINS: u32 = 10_000;
/// Bits per frame at most: start, 8 data, parity, 2 stop.
const MAX_FRAME_BITS: u32 = 12;
/// The fewest CPU cycles one pass of a wait loop takes, used when no
/// [`CycleClock`] is set: an empty `try_read` (the `read_within` loop), a
/// busy `poll_idle` (the transmit wait). The fastest pass measured with the
/// DWT cycle counter (`hil` `wait_bounds`), halved: H753 at 400 MHz with
/// caches on, 474 and 108 cycles. Slower configurations wait longer than
/// asked (F401 at 84 MHz: 679 and 184; H753 with caches off: about 1500
/// and 390, so up to about 8 times the time asked); a caller clock makes
/// every wait exact. A faster pass would make a bound run short; the HIL
/// check `wait_bounds` catches that on every board.
const RX_PASS_CYCLES: u32 = 224;
const TX_PASS_CYCLES: u32 = 48;
/// A pass that only polls a register (TC, SBK) counts as one cycle. These
/// waits end within a frame or two unless the hardware is stuck, so a
/// generous bound costs nothing in normal running.
const REGISTER_PASS_CYCLES: u32 = 1;
/// Tries to read NDTR between two equal half-lap counts before using the
/// last one. Each retry means an interrupt landed in a three-read window.
const COUNT_TRIES: u32 = 8;

/// Per-instance facts and state. Chip facts: the glue layer holds one
/// static `Info` per USART.
pub struct Info {
    regs: LineRegs,
    enable: RccBit,
    state: State,
}

// SAFETY: `port` is an address, and `State` is atomics.
unsafe impl Sync for Info {}

impl Info {
    pub(crate) const fn new(regs: LineRegs, enable: RccBit) -> Self {
        Self {
            regs,
            enable,
            state: State::new(),
        }
    }
}

/// What the interrupt side tells the reader. Atomics only, so neither side
/// takes a lock.
struct State {
    /// Half-laps of the receive ring the interrupt has seen. Written only
    /// by the interrupt side.
    halves: AtomicU32,
    /// `halves` when the ring last (re)started. Written only by the reader,
    /// with the stream stopped.
    base: AtomicU32,
    /// The ring's run, advanced at every restart; marks carry it.
    epoch: AtomicU32,
    /// The ring's length. Set once when the UART opens.
    len: AtomicU32,
    /// Where the line went idle and where line errors struck.
    marks: MarkQueue,
    /// Errors with no position to report them at: DMA errors, and line
    /// errors when `marks` was full.
    errors: AtomicU8,
    counts: Counts,
    /// CPU cycles one frame lasts at the current baud rate.
    frame_cycles: AtomicU32,
    tx: TxState,
}

/// The transmit queue, shared by `UartTx` and `UartIrq`.
struct TxState {
    frames: FrameQueue,
    /// The transmit half is servicing the queue; `UartIrq` only masks the
    /// stream's interrupts and sets `deferred`.
    servicing: AtomicBool,
    /// `UartIrq` masked the stream while the transmit half was servicing:
    /// the half services again before it lets go.
    deferred: AtomicBool,
    /// The frame at the queue's head is in flight.
    running: AtomicBool,
    /// The stream did not stop or start; transmission is off for good.
    halted: AtomicBool,
    errors: AtomicU8,
    sent: AtomicU32,
    dma_errors: AtomicU32,
}

impl TxState {
    const fn new() -> Self {
        Self {
            frames: FrameQueue::new(),
            servicing: AtomicBool::new(false),
            deferred: AtomicBool::new(false),
            running: AtomicBool::new(false),
            halted: AtomicBool::new(false),
            errors: AtomicU8::new(0),
            sent: AtomicU32::new(0),
            dma_errors: AtomicU32::new(0),
        }
    }

    fn reset(&self) {
        self.frames.clear();
        self.servicing.store(false, Ordering::Relaxed);
        self.deferred.store(false, Ordering::Relaxed);
        self.running.store(false, Ordering::Relaxed);
        self.halted.store(false, Ordering::Relaxed);
        self.errors.store(0, Ordering::Relaxed);
        self.sent.store(0, Ordering::Relaxed);
        self.dma_errors.store(0, Ordering::Relaxed);
    }
}

/// Fault counts kept by the interrupt side.
struct Counts {
    overrun: AtomicU32,
    framing: AtomicU32,
    parity: AtomicU32,
    noise: AtomicU32,
    dma_transfer: AtomicU32,
    dma_overrun: AtomicU32,
    marks_lost: AtomicU32,
}

impl Counts {
    const fn new() -> Self {
        Self {
            overrun: AtomicU32::new(0),
            framing: AtomicU32::new(0),
            parity: AtomicU32::new(0),
            noise: AtomicU32::new(0),
            dma_transfer: AtomicU32::new(0),
            dma_overrun: AtomicU32::new(0),
            marks_lost: AtomicU32::new(0),
        }
    }

    fn reset(&self) {
        for count in [
            &self.overrun,
            &self.framing,
            &self.parity,
            &self.noise,
            &self.dma_transfer,
            &self.dma_overrun,
            &self.marks_lost,
        ] {
            count.store(0, Ordering::Relaxed);
        }
    }
}

pub(crate) const ERR_DMA_TRANSFER: u8 = 1 << 0;
pub(crate) const ERR_OVERRUN: u8 = 1 << 1;
pub(crate) const ERR_FRAMING: u8 = 1 << 2;
pub(crate) const ERR_PARITY: u8 = 1 << 3;
pub(crate) const ERR_NOISE: u8 = 1 << 4;
pub(crate) const ERR_DMA_OVERRUN: u8 = 1 << 5;

/// Reporting order: the error that restarts reception comes first.
const ERRORS: [(u8, Error); 6] = [
    (ERR_DMA_TRANSFER, Error::DmaTransfer),
    (ERR_OVERRUN, Error::Overrun),
    (ERR_FRAMING, Error::Framing),
    (ERR_PARITY, Error::Parity),
    (ERR_NOISE, Error::Noise),
    (ERR_DMA_OVERRUN, Error::DmaOverrun),
];

impl State {
    const fn new() -> Self {
        Self {
            halves: AtomicU32::new(0),
            base: AtomicU32::new(0),
            epoch: AtomicU32::new(0),
            len: AtomicU32::new(0),
            marks: MarkQueue::new(),
            errors: AtomicU8::new(0),
            counts: Counts::new(),
            frame_cycles: AtomicU32::new(0),
            tx: TxState::new(),
        }
    }
}

/// A USART this chip has.
pub trait Instance: crate::Sealed + 'static {
    /// The bus whose clock is this USART's kernel clock.
    const KERNEL: Bus;
    /// Its name in the reference manual.
    const NAME: &'static str;
    /// Its interrupt vector.
    const INTERRUPT: Interrupt;
    /// The vector's name.
    const INTERRUPT_NAME: &'static str;
    /// Whether 0.5 and 1.5 stop bits are available. Not on the F4's UART4
    /// and UART5 (RM0090, USART_CR2 STOP): frames asking for them are refused
    /// with [`ConfigError::Unsupported`].
    const FRACTIONAL_STOP_BITS: bool = true;
    #[doc(hidden)]
    fn info() -> &'static Info;
}

/// A pin, in the right alternate function, that carries `U`'s TX.
pub trait TxPin<U: Instance>: crate::Sealed {}
/// A pin, in the right alternate function, that carries `U`'s RX.
pub trait RxPin<U: Instance>: crate::Sealed {}

/// A DMA stream that serves `U`'s transmit requests.
///
/// # Safety
///
/// `REQUEST` must be the request that routes `U`'s TX to this stream
/// (`dma_v2`: its CHSEL channel; `dma_v1`: `U`'s DMAMUX request ID): the
/// stream writes `U`'s data register.
pub unsafe trait TxDma<U: Instance>: StreamInstance + StreamInterrupt {
    const REQUEST: u8;
}

/// A DMA stream that serves `U`'s receive requests.
///
/// # Safety
///
/// As [`TxDma`], for `U`'s RX request.
pub unsafe trait RxDma<U: Instance>: StreamInstance + StreamInterrupt {
    const REQUEST: u8;
}

/// A UART, both directions, and the keeper of its instance token: a second
/// `Uart` for the same instance cannot be built, and [`free`](Self::free)
/// hands the token back. Implements `embedded_io`'s `Read`, `Write`,
/// `ReadReady` and `WriteReady`; [`split`](Self::split) gives three handles
/// that can live in different tasks, and [`join`](Self::join) puts them
/// back together.
///
/// # Limits on `usart_v2` (F4)
///
/// - **One byte may be lost per reported line error.** Clearing overrun,
///   framing, parity or noise takes an SR read then a DR read. If DMA has
///   not yet taken the byte in DR, that read consumes it. `usart_v4` clears
///   through ICR and has no such loss.
/// - **Parity detection is reliable only while the receiving USART is not
///   transmitting** (ES0299 2.10.2, ES0182 2.11.2). An SR read followed by
///   a DR write also clears PE, and the transmit DMA writes DR, so a
///   parity error can go unreported in full duplex.
/// - **IDLE can be missed when the sender's clock runs faster than the
///   receiver's** (ES0299 2.10.1, ES0182 2.11.1). A read that ends at idle,
///   such as [`UartRx::read_chunk`], needs a timeout fallback on v2, such
///   as a timer, not IDLE alone.
pub struct Uart<U> {
    line: Line<U>,
    tx: UartTx<U>,
    rx: UartRx<U>,
}

/// The line's control registers: the frame, the interrupt enables and a
/// break. Exactly one exists per open UART: [`Uart::split`] hands it out,
/// and [`Uart`] keeps it while the UART is whole. It carries the instance
/// token: [`Uart::join`] takes it from here. A receive-only UART has an
/// [`RxLine`] instead, so this line cannot free a receive half while its
/// transmit half lives.
///
/// A frame change and a break borrow the transmit half, so neither can run
/// beside the other, beside a mask, or between two contexts.
pub struct Line<U> {
    info: &'static Info,
    /// The bus clock the baud rate divides.
    kernel_clock: Hertz,
    /// The instance's `FRACTIONAL_STOP_BITS`.
    fractional_stop: bool,
    cpu: Hertz,
    token: U,
}

impl<U> Line<U> {
    /// Changes the frame settings. Waits for transmission to finish first;
    /// the receive ring keeps running, but a byte arriving during the change
    /// is lost.
    pub fn set_config(&mut self, tx: &mut UartTx<U>, config: &Config) -> Result<(), ConfigError> {
        let brr = config.divider(self.kernel_clock, self.fractional_stop)?;
        // A failed flush has already stopped the stream or halted the
        // transmit side, and the next write reports it.
        let _ = tx.flush();
        let port = self.info.regs;
        port.disable();
        port.write_frame(config, brr);
        port.enable(true);
        self.info
            .state
            .frame_cycles
            .store(frame_cycles(self.cpu, config.baudrate), Ordering::Relaxed);
        Ok(())
    }

    /// Sends a break: the line held low for a frame and more.
    pub fn send_break(&mut self, tx: &mut UartTx<U>) -> Result<(), Error> {
        tx.flush()?;
        let port = self.info.regs;
        port.request_break();
        let deadline = Deadline::new(tx.clock, tx.budget(2), REGISTER_PASS_CYCLES);
        if wait(deadline, || !port.break_pending()) {
            Ok(())
        } else {
            Err(Error::Timeout)
        }
    }

    /// Stops the line raising its own interrupt: clears the idle, parity and
    /// error enables. The vectors stay enabled in the NVIC, so
    /// [`UartIrq::verify`] still passes, and an event that arrives while
    /// masked stays pending in the USART and is reported after
    /// [`unmask`](Self::unmask).
    pub fn mask(&mut self) {
        self.info.regs.set_event_enables(false);
    }

    /// Restores the idle, parity and error enables.
    pub fn unmask(&mut self) {
        self.info.regs.set_event_enables(true);
    }
}

/// A receive-only UART's line: what [`UartRx::new`] hands out beside the
/// receive half, and what [`UartRx::free`] takes back. It carries the
/// instance token. It is not a [`Line`]: there is no transmit half, so no
/// frame change or break, and a full UART's line cannot stand in for it
/// to free a receive half while the transmit half lives.
pub struct RxLine<U> {
    line: Line<U>,
}

impl<U> RxLine<U> {
    /// Stops the line raising its own interrupt; see [`Line::mask`].
    pub fn mask(&mut self) {
        self.line.mask();
    }

    /// Restores the idle, parity and error enables.
    pub fn unmask(&mut self) {
        self.line.unmask();
    }
}

/// The transmit half: a queue of whole frames in the transmit buffer. It
/// belongs to instance `U` by type, so it rejoins, configures or breaks
/// only its own line; the token itself rides with the line.
pub struct UartTx<U> {
    info: &'static Info,
    dma: Channel,
    request: u8,
    buf: *mut u8,
    cap: u16,
    /// Where the last queued frame ends. Written only by this half.
    write: u16,
    timeouts: u32,
    clock: Option<&'static dyn CycleClock>,
    instance: PhantomData<U>,
}

/// The receive half. It belongs to instance `U` by type, so it rejoins or
/// frees only its own line; the token itself rides with the line.
pub struct UartRx<U> {
    info: &'static Info,
    dma: Channel,
    request: u8,
    ring: *mut u8,
    len: u16,
    /// Bytes read since the ring last (re)started, wrapping.
    read_count: u32,
    /// Where the next byte to read sits in the ring.
    read_index: u16,
    /// Gaps in the byte stream so far.
    generation: u32,
    buffer_overruns: u32,
    restarts: u32,
    /// The stream did not stop; reception is off for good.
    halted: bool,
    clock: Option<&'static dyn CycleClock>,
    instance: PhantomData<U>,
}

/// The interrupt side: call [`on_interrupt`](Self::on_interrupt) from the
/// USART's vector, the receive stream's vector and, to chain transmit
/// frames from the interrupt, the transmit stream's vector. All three must
/// run at one priority, above every task that uses the UART.
/// A share of the open UART, made beside it and never given back: it
/// reaches the streams only under the service protocol, and a
/// [`free`](Uart::free) leaves it holding an inert handle (the USART
/// disabled, the streams stopped, the transmit queue empty).
pub struct UartIrq {
    info: &'static Info,
    rx_dma: FlagHandle,
    /// `None` for a receive-only UART.
    tx: Option<TxIrq>,
    vectors: UartInterrupts,
}

/// The transmit stream, shared with [`UartTx`] under `service_tx`'s
/// protocol, and what starting a frame needs.
struct TxIrq {
    dma: Channel,
    buf: *mut u8,
    request: u8,
}

// SAFETY: the only owner of its buffer and its transmit stream; moving it
// to another context moves that ownership. `UartIrq` shares both with it
// only under `service_tx`'s protocol. `U` is a marker; no token is held.
unsafe impl<U> Send for UartTx<U> {}
// SAFETY: the only owner of its buffer and its receive stream; moving it to
// another context moves that ownership. `U` is a marker; no token is held.
unsafe impl<U> Send for UartRx<U> {}
// SAFETY: shares the transmit stream and buffer with `UartTx` only under
// `service_tx`'s protocol, and writes no register the halves own.
unsafe impl Send for UartIrq {}

impl<U: Instance> Uart<U> {
    /// Configures the USART and both DMA streams, and starts reception.
    ///
    /// `wiring` comes from the board file; its pins must already be in
    /// their alternate function, which their types guarantee. Every check
    /// that can fail on the configuration runs before any register is
    /// written.
    ///
    /// A refusal hands back `wiring` and `buffers` with the reason, to
    /// retry or use elsewhere. Every reason but [`ConfigError::DmaBusy`]
    /// leaves the hardware untouched.
    ///
    /// `DmaBusy` means a stream an earlier owner left running did not
    /// report itself stopped in time. Its EN is cleared, so it may still
    /// stop later. Neither stream was ever pointed at `buffers`, so they
    /// are safe to reuse. By then the USART has been enabled and reset in
    /// RCC, and:
    /// - if the transmit stream is the one, nothing else was touched: the
    ///   USART's registers are at their reset values, and the receive
    ///   stream is as the earlier owner left it;
    /// - if the receive stream is the one, the transmit stream has
    ///   stopped, and the frame is written: UE, DMAT, DMAR and EIE set, PCE
    ///   and PEIE too with parity, and RE and TE off.
    ///
    /// On success the instance token moves into the line handle; the pin
    /// and stream tokens end here. [`free`](Self::free) returns the
    /// instance token, so the same instance can open again.
    #[allow(clippy::type_complexity)]
    pub fn new<TxP, RxP, TxD, RxD>(
        rcc: &mut Rcc,
        wiring: Wiring<U, TxP, RxP, TxD, RxD>,
        config: &Config,
        mut buffers: Buffers,
    ) -> Result<(Self, UartIrq), Refused<ConfigError, (Wiring<U, TxP, RxP, TxD, RxD>, Buffers)>>
    where
        TxP: TxPin<U>,
        RxP: RxPin<U>,
        TxD: TxDma<U>,
        RxD: RxDma<U>,
    {
        // Taken apart so a refusal can put the wiring back together. After
        // `Ok` the driver is the only user of the streams and of the
        // buffers' memory: it keeps raw pointers into `buffers`, so nothing
        // here may touch `buffers` again once the open succeeds.
        let Wiring {
            uart,
            tx,
            rx,
            tx_dma,
            rx_dma,
        } = wiring;
        match Self::open(rcc, uart, &tx_dma, &rx_dma, config, &mut buffers) {
            Ok(opened) => Ok(opened),
            Err((error, uart)) => Err(Refused::new(
                error,
                (
                    Wiring {
                        uart,
                        tx,
                        rx,
                        tx_dma,
                        rx_dma,
                    },
                    buffers,
                ),
            )),
        }
    }

    fn open<TxD, RxD>(
        rcc: &mut Rcc,
        uart: U,
        tx_dma: &TxD,
        rx_dma: &RxD,
        config: &Config,
        buffers: &mut Buffers,
    ) -> Result<(Self, UartIrq), (ConfigError, U)>
    where
        TxD: TxDma<U>,
        RxD: RxDma<U>,
    {
        let clocks = rcc.clocks();
        let checked = match check_open(
            clocks,
            U::KERNEL,
            U::FRACTIONAL_STOP_BITS,
            config,
            Some(buffers.tx),
            buffers.rx,
        ) {
            Ok(checked) => checked,
            Err(error) => return Err((error, uart)),
        };
        let info = U::info();
        let kernel_clock = clocks.bus(U::KERNEL);
        reset(rcc, info, checked.rx_len);
        info.state
            .frame_cycles
            .store(checked.frame_cycles, Ordering::Relaxed);
        let mut tx = UartTx {
            info,
            dma: crate::dma::claim_channel(tx_dma),
            request: TxD::REQUEST,
            buf: buffers.tx.as_mut_ptr(),
            cap: checked.tx_len,
            write: 0,
            timeouts: 0,
            clock: None,
            instance: PhantomData,
        };
        let rx = UartRx::<U>::assemble(
            info,
            crate::dma::claim_channel(rx_dma),
            RxD::REQUEST,
            buffers.rx,
        );

        // Streams left running by an earlier owner must stop before they
        // are pointed at new memory.
        if tx.dma.stop(STOP_SPINS).is_err() {
            return Err((ConfigError::DmaBusy, uart));
        }
        tx.dma.clear_flags(Flags::ALL);
        let tx_irq = TxIrq {
            // SAFETY: shared with `tx` under `service_tx`'s protocol.
            dma: unsafe { tx.dma.alias() },
            buf: tx.buf,
            request: tx.request,
        };
        let vectors = UartInterrupts {
            line: U::INTERRUPT,
            rx_dma: RxD::INTERRUPT,
            tx_dma: Some(TxD::INTERRUPT),
        };
        let (rx, irq) = match rx.start(config, checked.brr, true, Some(tx_irq), vectors) {
            Ok(opened) => opened,
            Err((error, _rx)) => return Err((error, uart)),
        };
        Ok((
            Self {
                line: Line {
                    info,
                    kernel_clock,
                    fractional_stop: U::FRACTIONAL_STOP_BITS,
                    cpu: clocks.cpu,
                    token: uart,
                },
                tx,
                rx,
            },
            irq,
        ))
    }

    /// Whether instance `U` can take `config` under `clocks`; see
    /// [`Config::check`]. A `const fn`, so a board can assert it while
    /// compiling: `const _: () = assert!(Uart::<USART6>::check(&CLOCKS,
    /// &CRSF).is_ok());`.
    pub const fn check(clocks: &Clocks, config: &Config) -> Result<(), ConfigError> {
        match config.divider(clocks.bus(U::KERNEL), U::FRACTIONAL_STOP_BITS) {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Changes the frame settings. Waits for transmission to finish first;
    /// the receive ring keeps running, but a byte arriving during the change
    /// is lost.
    pub fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        self.line.set_config(&mut self.tx, config)
    }

    /// Sends a break: the line held low for a frame and more.
    pub fn send_break(&mut self) -> Result<(), Error> {
        self.line.send_break(&mut self.tx)
    }

    /// Measures every wait of both halves with `clock` instead of counting
    /// passes, so a bound lasts the time asked. `None` goes back to counting.
    pub fn set_clock(&mut self, clock: Option<&'static dyn CycleClock>) {
        self.tx.clock = clock;
        self.rx.clock = clock;
    }

    /// The line and both data halves, each able to live in its own task.
    /// The instance token rides with the line.
    pub fn split(self) -> (Line<U>, UartTx<U>, UartRx<U>) {
        (self.line, self.tx, self.rx)
    }

    /// Puts a [`split`](Self::split) UART back together, and `free` with
    /// it: the three handles are the proof that the UART is whole again,
    /// because nothing outside this module can make one. The token comes
    /// back out of the line. All three carry the instance, so halves of
    /// another UART do not compile here.
    pub fn join(line: Line<U>, tx: UartTx<U>, rx: UartRx<U>) -> Self {
        Self { line, tx, rx }
    }

    /// Stops the line and both streams, and hands the instance token back:
    /// what a `Drop` would do, made explicit so the token can move out.
    /// The transmit queue is dropped; the buffers' memory and the DMA
    /// channels end with the halves. The [`UartIrq`] keeps its vectors but
    /// finds a closed line: the USART is disabled with its event enables
    /// cleared, and the transmit side is marked halted, so a service that
    /// races this starts nothing. A reopen runs `Uart::new` again, from
    /// the same token and fresh stream tokens.
    ///
    /// A `Line` or `UartTx` split off earlier must be
    /// [`join`](Self::join)ed back first: this takes the whole UART, and
    /// the token rides in the line.
    pub fn free(self) -> U {
        let Self {
            line,
            mut tx,
            mut rx,
        } = self;
        tx.halt();
        rx.halt();
        line.info.regs.set_event_enables(false);
        line.info.regs.disable();
        line.token
    }

    /// The line and both halves at once, borrowed: [`split`](Self::split)
    /// without giving the UART up.
    pub fn halves(&mut self) -> (&mut Line<U>, &mut UartTx<U>, &mut UartRx<U>) {
        (&mut self.line, &mut self.tx, &mut self.rx)
    }

    /// The line's control handle: the frame, the interrupt enables and a
    /// break.
    pub fn line(&mut self) -> &mut Line<U> {
        &mut self.line
    }

    pub fn tx(&mut self) -> &mut UartTx<U> {
        &mut self.tx
    }

    pub fn rx(&mut self) -> &mut UartRx<U> {
        &mut self.rx
    }
}

impl<U> UartTx<U> {
    /// Queues `frame` whole, or not at all, and starts it if the line is
    /// free. Frames go out in order, back to back.
    ///
    /// With the transmit stream's vector bound to [`UartIrq::on_interrupt`],
    /// each finished frame starts the next from the interrupt. Without it,
    /// queued frames move on only when this half is called: `try_send`,
    /// [`poll`](Self::poll), `write` or `flush`.
    pub fn try_send(&mut self, frame: &[u8]) -> Result<(), SendError> {
        if self.halted() {
            return Err(SendError::Halted);
        }
        if frame.is_empty() {
            return Ok(());
        }
        let len = u16::try_from(frame.len())
            .ok()
            .filter(|&len| len <= self.cap)
            .ok_or(SendError::TooLarge)?;
        self.poll();
        let tx = &self.info.state.tx;
        if tx.frames.len() >= FRAMES {
            return Err(SendError::Full);
        }
        let oldest = tx.frames.peek().map(|frame| frame.offset);
        let offset = tx_ring::place(self.write, oldest, self.cap, len).ok_or(SendError::Full)?;
        // SAFETY: `place` found `offset..offset + len` inside the buffer and
        // outside every queued frame, so neither DMA nor the service reads
        // it until the frame is pushed below.
        unsafe {
            ptr::copy_nonoverlapping(
                frame.as_ptr(),
                self.buf.add(usize::from(offset)),
                frame.len(),
            )
        };
        // The bytes land before the frame is visible to the service.
        fence(Ordering::SeqCst);
        tx.frames.push(Frame { offset, len });
        self.write = offset + len;
        self.poll();
        Ok(())
    }

    /// Finishes the frame in flight if it is done and starts the next. Only
    /// needed with the transmit stream's vector unbound.
    pub fn poll(&mut self) {
        let tx = &self.info.state.tx;
        loop {
            // Every write to the stream happens with `servicing` set: an
            // interrupt then only masks the stream (and says so), never
            // starts a transfer between this half's read and write of CR.
            tx.servicing.store(true, Ordering::SeqCst);
            tx.deferred.store(false, Ordering::SeqCst);
            self.dma.set_interrupts(Interrupts::default());
            service_tx(self.info, &mut self.dma, self.buf, self.request);
            // A completion not yet seen raises the interrupt once unmasked.
            self.dma.set_interrupts(TX_INTERRUPTS);
            tx.servicing.store(false, Ordering::SeqCst);
            // An interrupt that came meanwhile masked the stream again and
            // left its work here: service once more.
            if !tx.deferred.load(Ordering::SeqCst) {
                return;
            }
        }
    }

    /// Starts sending as much of `data` as fits as one frame, waiting first
    /// for room. Returns how many bytes it took.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        self.take_error()?;
        if data.is_empty() {
            return Ok(0);
        }
        let mut deadline = Deadline::new(self.clock, self.budget(self.cap), TX_PASS_CYCLES);
        loop {
            let tx = &self.info.state.tx;
            let oldest = tx.frames.peek().map(|frame| frame.offset);
            let room = if tx.frames.len() < FRAMES {
                tx_ring::largest(self.write, oldest, self.cap)
            } else {
                0
            };
            let len = data.len().min(usize::from(room));
            if len > 0 {
                return match self.try_send(&data[..len]) {
                    Ok(()) => Ok(len),
                    Err(SendError::Halted) => Err(Error::Halted),
                    // Room was just measured, and only this half fills it.
                    Err(SendError::Full | SendError::TooLarge) => Err(Error::Halted),
                };
            }
            self.poll();
            self.take_error()?;
            if deadline.expired() {
                return self.time_out();
            }
        }
    }

    /// Waits until every queued frame has left the wire.
    pub fn flush(&mut self) -> Result<(), Error> {
        let mut deadline = Deadline::new(self.clock, self.budget(self.cap), TX_PASS_CYCLES);
        loop {
            self.poll();
            self.take_error()?;
            if self.info.state.tx.frames.len() == 0 {
                break;
            }
            if deadline.expired() {
                return self.time_out();
            }
        }
        let port = self.info.regs;
        let deadline = Deadline::new(self.clock, self.budget(1), REGISTER_PASS_CYCLES);
        if wait(deadline, || port.tc()) {
            Ok(())
        } else {
            Err(Error::Timeout)
        }
    }

    /// Whether every queued frame has gone.
    pub fn is_idle(&mut self) -> Result<bool, Error> {
        self.poll();
        self.take_error()?;
        Ok(self.info.state.tx.frames.len() == 0)
    }

    /// Frames queued, the one in flight included.
    pub fn queued(&self) -> usize {
        self.info.state.tx.frames.len()
    }

    /// Frames sent, DMA errors and timeouts since the UART was opened.
    pub fn health(&self) -> TxHealth {
        let tx = &self.info.state.tx;
        TxHealth {
            sent: tx.sent.load(Ordering::Relaxed),
            dma_errors: tx.dma_errors.load(Ordering::Relaxed),
            timeouts: self.timeouts,
        }
    }

    /// Measures this half's waits with `clock`; see [`Uart::set_clock`].
    pub fn set_clock(&mut self, clock: Option<&'static dyn CycleClock>) {
        self.clock = clock;
    }

    /// CPU cycles that cover `bytes` frames, twice over.
    fn budget(&self, bytes: u16) -> u32 {
        (u32::from(bytes) + 2)
            .saturating_mul(self.info.state.frame_cycles.load(Ordering::Relaxed))
            .saturating_mul(2)
    }

    fn halted(&self) -> bool {
        self.info.state.tx.halted.load(Ordering::Acquire)
    }

    /// The oldest transmit error not yet reported.
    fn take_error(&mut self) -> Result<(), Error> {
        if self.halted() {
            return Err(Error::Halted);
        }
        let errors = &self.info.state.tx.errors;
        let pending = errors.load(Ordering::Acquire);
        for (bit, error) in [
            (ERR_DMA_TRANSFER, Error::DmaTransfer),
            (ERR_DMA_OVERRUN, Error::DmaOverrun),
        ] {
            if pending & bit != 0 {
                errors.fetch_and(!bit, Ordering::AcqRel);
                return Err(error);
            }
        }
        Ok(())
    }

    /// Stops the transmit stream and drops the queue, and marks
    /// transmission off for good so a racing service cannot start a frame.
    /// Private: the sequence `free` runs, ordered like `time_out` plus the
    /// halt, so the interrupt side finds nothing to do.
    fn halt(&mut self) {
        let tx = &self.info.state.tx;
        tx.servicing.store(true, Ordering::SeqCst);
        tx.halted.store(true, Ordering::Release);
        self.dma.set_interrupts(Interrupts::default());
        let _ = self.dma.stop(STOP_SPINS);
        self.dma.clear_flags(Flags::ALL);
        tx.frames.clear();
        tx.running.store(false, Ordering::Release);
        tx.servicing.store(false, Ordering::SeqCst);
    }

    /// A frame did not finish in time: stop the stream, drop the queue.
    fn time_out<T>(&mut self) -> Result<T, Error> {
        self.timeouts = self.timeouts.wrapping_add(1);
        let tx = &self.info.state.tx;
        tx.servicing.store(true, Ordering::SeqCst);
        self.dma.set_interrupts(Interrupts::default());
        let stopped = self.dma.stop(STOP_SPINS).is_ok();
        self.dma.clear_flags(Flags::ALL);
        tx.frames.clear();
        tx.running.store(false, Ordering::Release);
        if stopped {
            self.dma.set_interrupts(TX_INTERRUPTS);
        } else {
            tx.halted.store(true, Ordering::Release);
        }
        tx.servicing.store(false, Ordering::SeqCst);
        if stopped {
            Err(Error::Timeout)
        } else {
            Err(Error::Halted)
        }
    }
}

/// The transmit stream's interrupts: a frame done, or a DMA error. A FIFO
/// error is advisory in direct mode and only cleared.
const TX_INTERRUPTS: Interrupts = Interrupts {
    complete: true,
    half: false,
    error: true,
    fifo_error: false,
};

/// Finishes the frame in flight if it is done, and starts the next.
///
/// Called by the transmit half (with `servicing` set) or by [`UartIrq`]
/// (only while `servicing` is clear), never both at once: the interrupt
/// does nothing but mask the stream while `servicing` is set, and is never
/// preempted by the task.
fn service_tx(info: &Info, dma: &mut Channel, buf: *mut u8, request: u8) {
    let tx = &info.state.tx;
    if tx.running.load(Ordering::Acquire) {
        let flags = dma.flags();
        if flags.transfer_error || flags.direct_mode_error {
            if dma.stop(STOP_SPINS).is_err() {
                tx.halted.store(true, Ordering::Release);
            }
            dma.clear_flags(Flags::ALL);
            tx.frames.pop();
            tx.running.store(false, Ordering::Release);
            tx.dma_errors.fetch_add(1, Ordering::Relaxed);
            let bit = if flags.transfer_error {
                ERR_DMA_TRANSFER
            } else {
                ERR_DMA_OVERRUN
            };
            tx.errors.fetch_or(bit, Ordering::Release);
        } else if flags.complete && !dma.is_running() {
            dma.clear_flags(Flags::ALL);
            tx.frames.pop();
            tx.running.store(false, Ordering::Release);
            tx.sent.fetch_add(1, Ordering::Relaxed);
        } else {
            if flags.fifo_error {
                dma.clear_flags(Flags {
                    fifo_error: true,
                    ..Flags::default()
                });
            }
            return;
        }
    }
    if tx.halted.load(Ordering::Acquire) {
        return;
    }
    let Some(frame) = tx.frames.peek() else {
        return;
    };
    info.regs.clear_tc();
    let transfer = Transfer {
        request,
        #[cfg(feature = "dma_v1")]
        bufferable: true,
        direction: Direction::MemoryToPeripheral,
        peripheral: info.regs.tx_address(),
        // SAFETY (of the address): the frame lies inside the buffer.
        memory: buf.wrapping_add(usize::from(frame.offset)) as u32,
        double_buffer: None,
        len: frame.len,
        size: WordSize::Byte,
        memory_increment: true,
        circular: false,
        priority: Priority::Medium,
        fifo: FifoMode::Direct,
        interrupts: TX_INTERRUPTS,
    };
    // SAFETY: the frame's bytes are in the transmit buffer, written before
    // the frame was queued and untouched until it is popped, after the
    // stream has finished with it. The routing trait vouched for the
    // request.
    match unsafe { dma.start(&transfer) } {
        Ok(()) => tx.running.store(true, Ordering::Release),
        Err(_) => {
            // Still enabled: a stop that never took. Fail closed.
            tx.halted.store(true, Ordering::Release);
        }
    }
}

impl<U> UartRx<U> {
    /// Opens a receive-only UART: no TX pin, no transmit stream, the
    /// transmitter left off. The same receive half a full UART has, and a
    /// `UartIrq` to call from the USART's vector and the receive stream's.
    ///
    /// A refusal hands back `wiring` and `ring` with the reason, as
    /// [`Uart::new`] does. A `DmaBusy` leaves the hardware as that
    /// describes for the receive stream, with no transmit stream.
    ///
    /// On success the instance token moves into the line handle;
    /// [`free`](Self::free) returns it.
    #[allow(clippy::type_complexity)]
    pub fn new<RxP, RxD>(
        rcc: &mut Rcc,
        wiring: RxWiring<U, RxP, RxD>,
        config: &Config,
        ring: &'static mut [u8],
    ) -> Result<
        (RxLine<U>, Self, UartIrq),
        Refused<ConfigError, (RxWiring<U, RxP, RxD>, &'static mut [u8])>,
    >
    where
        U: Instance,
        RxP: RxPin<U>,
        RxD: RxDma<U>,
    {
        // Taken apart so a refusal can put the wiring back together; after
        // `Ok` nothing here may touch `ring` again.
        let RxWiring { uart, rx, rx_dma } = wiring;
        match Self::open(rcc, uart, &rx_dma, config, ring) {
            Ok(opened) => Ok(opened),
            Err((error, uart)) => Err(Refused::new(error, (RxWiring { uart, rx, rx_dma }, ring))),
        }
    }

    fn open<RxD>(
        rcc: &mut Rcc,
        uart: U,
        rx_dma: &RxD,
        config: &Config,
        ring: &mut [u8],
    ) -> Result<(RxLine<U>, Self, UartIrq), (ConfigError, U)>
    where
        U: Instance,
        RxD: RxDma<U>,
    {
        let clocks = rcc.clocks();
        let checked = match check_open(
            clocks,
            U::KERNEL,
            U::FRACTIONAL_STOP_BITS,
            config,
            None,
            ring,
        ) {
            Ok(checked) => checked,
            Err(error) => return Err((error, uart)),
        };
        let info = U::info();
        let kernel_clock = clocks.bus(U::KERNEL);
        reset(rcc, info, checked.rx_len);
        info.state
            .frame_cycles
            .store(checked.frame_cycles, Ordering::Relaxed);
        let rx = Self::assemble(info, crate::dma::claim_channel(rx_dma), RxD::REQUEST, ring);
        let vectors = UartInterrupts {
            line: U::INTERRUPT,
            rx_dma: RxD::INTERRUPT,
            tx_dma: None,
        };
        let (rx, irq) = match rx.start(config, checked.brr, false, None, vectors) {
            Ok(opened) => opened,
            Err((error, _rx)) => return Err((error, uart)),
        };
        Ok((
            RxLine {
                line: Line {
                    info,
                    kernel_clock,
                    fractional_stop: U::FRACTIONAL_STOP_BITS,
                    cpu: clocks.cpu,
                    token: uart,
                },
            },
            rx,
            irq,
        ))
    }

    /// Stops the receive stream and closes the line, and hands the
    /// instance token back: what a `Drop` would do, made explicit so the
    /// token can move out. Takes the [`RxLine`] back, because the token
    /// rides in it and leaves only when the handles that can touch the
    /// hardware are gone; the `UartIrq` stays, inert (see [`Uart::free`]).
    /// Only a receive-only UART has an `RxLine`, so a split UART's receive
    /// half cannot free the line under a live transmit half; it rejoins
    /// with [`Uart::join`] instead. A reopen runs `UartRx::new` again, from
    /// the same token and a fresh stream token.
    pub fn free(mut self, line: RxLine<U>) -> U {
        let RxLine { line } = line;
        self.halt();
        line.info.regs.set_event_enables(false);
        line.info.regs.disable();
        line.token
    }

    /// Keeps the ring's address and length only: the caller's `'static`
    /// reference ends once the UART is open, leaving the driver the ring's
    /// only user.
    fn assemble(info: &'static Info, dma: Channel, request: u8, ring: &mut [u8]) -> Self {
        Self {
            info,
            dma,
            request,
            len: ring.len() as u16,
            ring: ring.as_mut_ptr(),
            read_count: 0,
            read_index: 0,
            generation: 0,
            buffer_overruns: 0,
            // `start_ring` counts the first start too.
            restarts: 0,
            halted: false,
            clock: None,
            instance: PhantomData,
        }
    }

    /// Writes the frame, starts the ring, then turns the receiver (and the
    /// transmitter, `tx`) on: the receiver last, so no byte arrives before
    /// the ring is ready. On a `DmaBusy` the half comes back untouched.
    fn start(
        mut self,
        config: &Config,
        brr: u16,
        tx: bool,
        tx_irq: Option<TxIrq>,
        vectors: UartInterrupts,
    ) -> Result<(Self, UartIrq), (ConfigError, Self)> {
        self.info.regs.write_frame(config, brr);
        if self.start_ring().is_err() {
            return Err((ConfigError::DmaBusy, self));
        }
        self.info.regs.enable(tx);
        let irq = UartIrq {
            info: self.info,
            rx_dma: self.dma.flag_handle(),
            tx: tx_irq,
            vectors,
        };
        Ok((self, irq))
    }

    /// Stops the receive stream and clears its flags. Private: the
    /// sequence `free` runs.
    fn halt(&mut self) {
        let _ = self.dma.stop(STOP_SPINS);
        self.dma.clear_flags(Flags::ALL);
    }

    /// Copies out what has arrived, up to `buf.len()` bytes, without
    /// waiting. `Ok(0)` means nothing yet.
    ///
    /// Chunk boundaries do not matter here, but errors keep their place: the
    /// bytes received before a line error are returned first, and the error
    /// on the next call.
    pub fn try_read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        let written = self.check_ring()?;
        let epoch = self.info.state.epoch.load(Ordering::Acquire);
        let marks = &self.info.state.marks;
        loop {
            let room = buf.len().min(usize::from(self.len)) as u32;
            match ring::plan(self.read_count, written.0, room, marks.peek(), epoch) {
                Take::Drop => marks.pop(),
                Take::ToMark { len: 0, kind } => {
                    marks.pop();
                    if let Some(error) = line_error(kind) {
                        self.gap();
                        return Err(error);
                    }
                }
                Take::ToMark { len, .. } | Take::Full { len } => {
                    // The mark, if any, stays: an error is reported once the
                    // bytes before it are out. `plan` keeps `len` within
                    // `room`; the clamp lets the optimiser see it too.
                    let len = (len as usize).min(buf.len());
                    return self.copy_out(&mut buf[..len]);
                }
                Take::Wait => {
                    let available = written.0.wrapping_sub(self.read_count).min(room);
                    let available = (available as usize).min(buf.len());
                    return self.copy_out(&mut buf[..available]);
                }
            }
        }
    }

    /// The next chunk, copied into `buf`, if one is complete.
    ///
    /// A chunk ends where the line went idle, where `buf` is full, or at a
    /// gap: a line error, a ring overrun or a DMA error, each of which also
    /// advances [`generation`](Self::generation). `Ok(None)` means the bytes
    /// since the last chunk are still arriving. `Err` only for
    /// [`Error::Halted`].
    pub fn read_chunk(&mut self, buf: &mut [u8]) -> Result<Option<Chunk>, Error> {
        let written = match self.check_ring() {
            Ok(written) => written,
            Err(Error::Halted) => return Err(Error::Halted),
            Err(error) => {
                return Ok(Some(Chunk {
                    len: 0,
                    end: ChunkEnd::Gap(error),
                    generation: self.generation,
                }));
            }
        };
        let epoch = self.info.state.epoch.load(Ordering::Acquire);
        let marks = &self.info.state.marks;
        loop {
            let room = buf.len().min(usize::from(self.len)) as u32;
            match ring::plan(self.read_count, written.0, room, marks.peek(), epoch) {
                Take::Drop => marks.pop(),
                Take::ToMark { len, kind } => {
                    // `plan` keeps `len` within `room`; the clamp lets the
                    // optimiser see it too.
                    let len = (len as usize).min(buf.len());
                    let len = self.copy_out_exact(&mut buf[..len])?;
                    marks.pop();
                    if len.is_none() {
                        // Lapped during the copy: `copy_out_exact` resynced.
                        return Ok(Some(self.gap_chunk(Error::BufferOverrun)));
                    }
                    match line_error(kind) {
                        Some(error) => {
                            self.gap();
                            return Ok(Some(Chunk {
                                len: len.unwrap_or(0),
                                end: ChunkEnd::Gap(error),
                                generation: self.generation,
                            }));
                        }
                        // An idle line right after a boundary: nothing new.
                        None if len == Some(0) => {}
                        None => {
                            return Ok(Some(Chunk {
                                len: len.unwrap_or(0),
                                end: ChunkEnd::Idle,
                                generation: self.generation,
                            }));
                        }
                    }
                }
                Take::Full { len } => {
                    let len = (len as usize).min(buf.len());
                    return match self.copy_out_exact(&mut buf[..len])? {
                        Some(len) => Ok(Some(Chunk {
                            len,
                            end: ChunkEnd::Full,
                            generation: self.generation,
                        })),
                        None => Ok(Some(self.gap_chunk(Error::BufferOverrun))),
                    };
                }
                Take::Wait => return Ok(None),
            }
        }
    }

    /// As [`try_read`](Self::try_read), but waits for at least one byte for
    /// as long as `frames` frames take at the configured baud rate.
    /// `Err(Timeout)` if none came.
    pub fn read_within(&mut self, buf: &mut [u8], frames: u32) -> Result<usize, Error> {
        let cycles = frames.saturating_mul(self.info.state.frame_cycles.load(Ordering::Relaxed));
        let mut deadline = Deadline::new(self.clock, cycles, RX_PASS_CYCLES);
        loop {
            let len = self.try_read(buf)?;
            if len > 0 || buf.is_empty() {
                return Ok(len);
            }
            if deadline.expired() {
                break;
            }
        }
        match self.try_read(buf)? {
            0 if !buf.is_empty() => Err(Error::Timeout),
            len => Ok(len),
        }
    }

    /// Measures this half's waits with `clock`; see [`Uart::set_clock`].
    pub fn set_clock(&mut self, clock: Option<&'static dyn CycleClock>) {
        self.clock = clock;
    }

    /// Bytes waiting to be read, chunk boundaries or not.
    pub fn available(&mut self) -> Result<u16, Error> {
        let (written, _) = self.check_ring()?;
        Ok(written.wrapping_sub(self.read_count) as u16)
    }

    /// Advances at every gap in the byte stream: each line error, ring
    /// overrun or DMA error, whether reported by `read_chunk` or by
    /// `try_read`. Two chunks with the same generation have no loss between
    /// them.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Counts of every fault since the UART was opened.
    pub fn health(&self) -> Health {
        let counts = &self.info.state.counts;
        let load = |count: &AtomicU32| count.load(Ordering::Relaxed);
        Health {
            overrun: load(&counts.overrun),
            framing: load(&counts.framing),
            parity: load(&counts.parity),
            noise: load(&counts.noise),
            dma_transfer: load(&counts.dma_transfer),
            dma_overrun: load(&counts.dma_overrun),
            buffer_overrun: self.buffer_overruns,
            // The first start is not a restart.
            restarts: self.restarts.saturating_sub(1),
            marks_lost: load(&counts.marks_lost),
        }
    }

    /// Where DMA has written to, after handling whatever stopped or lapped
    /// the ring: errors whose position was lost, a stopped stream, a ring
    /// overrun. Each of those is a gap.
    fn check_ring(&mut self) -> Result<(u32, u16), Error> {
        if self.halted {
            return Err(Error::Halted);
        }
        if let Some(error) = self.take_error() {
            if !self.dma.is_running() {
                self.start_ring()?;
            }
            self.gap();
            return Err(error);
        }
        if !self.dma.is_running() {
            // Stopped by an error whose interrupt has not run yet. Restart
            // now; the interrupt will find the flags already cleared.
            self.start_ring()?;
            self.gap();
            return Err(Error::DmaTransfer);
        }
        let (written, index) = count_written(&self.info.state, &self.dma.flag_handle());
        if written.wrapping_sub(self.read_count) > u32::from(self.len) {
            self.resync(written, index);
            self.buffer_overruns = self.buffer_overruns.wrapping_add(1);
            self.gap();
            return Err(Error::BufferOverrun);
        }
        Ok((written, index))
    }

    /// Copies `buf.len()` bytes, already known to have arrived, out of the
    /// ring. `Err(BufferOverrun)` if DMA lapped the reader meanwhile.
    fn copy_out(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        match self.copy_out_exact(buf)? {
            Some(len) => Ok(len),
            None => {
                self.gap();
                Err(Error::BufferOverrun)
            }
        }
    }

    /// As `copy_out`, but `Ok(None)` for an overrun during the copy.
    fn copy_out_exact(&mut self, buf: &mut [u8]) -> Result<Option<usize>, Error> {
        let len = buf.len();
        let mut index = usize::from(self.read_index);
        for byte in buf.iter_mut() {
            // SAFETY: `index` is inside the ring, which this half owns; DMA
            // may be writing elsewhere in it, so each byte is read once,
            // volatile, after the count that said it had arrived.
            *byte = unsafe { ptr::read_volatile(self.ring.add(index)) };
            index += 1;
            if index == usize::from(self.len) {
                index = 0;
            }
        }
        // Had DMA lapped the reader while it copied, the bytes are suspect.
        let (written, position) = count_written(&self.info.state, &self.dma.flag_handle());
        if written.wrapping_sub(self.read_count) > u32::from(self.len) {
            self.resync(written, position);
            self.buffer_overruns = self.buffer_overruns.wrapping_add(1);
            return Ok(None);
        }
        self.read_count = self.read_count.wrapping_add(len as u32);
        self.read_index = index as u16;
        Ok(Some(len))
    }

    fn gap(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn gap_chunk(&mut self, error: Error) -> Chunk {
        self.gap();
        Chunk {
            len: 0,
            end: ChunkEnd::Gap(error),
            generation: self.generation,
        }
    }

    /// The oldest error recorded without a position (DMA errors, and line
    /// errors when the mark queue was full) and not yet reported.
    fn take_error(&mut self) -> Option<Error> {
        let errors = &self.info.state.errors;
        let pending = errors.load(Ordering::Acquire);
        let (bit, error) = ERRORS.iter().find(|(bit, _)| pending & bit != 0)?;
        errors.fetch_and(!bit, Ordering::AcqRel);
        Some(*error)
    }

    /// Drops everything unread and carries on from DMA's position.
    fn resync(&mut self, written: u32, index: u16) {
        self.read_count = written;
        self.read_index = index;
    }

    /// Starts, or restarts, the ring from its first byte. The ring's run
    /// (epoch) advances after the base is set, so the interrupt never tags
    /// a mark of the new run with a position from the old one.
    fn start_ring(&mut self) -> Result<(), Error> {
        if self.dma.stop(STOP_SPINS).is_err() {
            self.halted = true;
            return Err(Error::Halted);
        }
        self.dma.clear_flags(Flags::ALL);
        let state = &self.info.state;
        state
            .base
            .store(state.halves.load(Ordering::Acquire), Ordering::Release);
        state.epoch.fetch_add(1, Ordering::AcqRel);
        state.marks.clear();
        self.read_count = 0;
        self.read_index = 0;
        self.restarts = self.restarts.wrapping_add(1);
        let transfer = Transfer {
            request: self.request,
            #[cfg(feature = "dma_v1")]
            bufferable: true,
            direction: Direction::PeripheralToMemory,
            peripheral: self.info.regs.rx_address(),
            memory: self.ring as u32,
            double_buffer: None,
            len: self.len,
            size: WordSize::Byte,
            memory_increment: true,
            circular: true,
            priority: Priority::High,
            fifo: FifoMode::Direct,
            interrupts: Interrupts {
                complete: true,
                half: true,
                error: true,
                fifo_error: true,
            },
        };
        // SAFETY: the ring is this half's for good, `len` is its length, and
        // the CPU only ever reads it. The routing trait vouched for the
        // request.
        unsafe { self.dma.start(&transfer) }.map_err(|_| Error::Halted)
    }
}

/// The error a mark records, if it is not an idle line.
fn line_error(kind: MarkKind) -> Option<Error> {
    match kind {
        MarkKind::Idle => None,
        MarkKind::Overrun => Some(Error::Overrun),
        MarkKind::Framing => Some(Error::Framing),
        MarkKind::Parity => Some(Error::Parity),
        MarkKind::Noise => Some(Error::Noise),
    }
}

/// Bytes DMA has written since the ring (re)started, wrapping, and its
/// position in the ring. Both sides use it: the reader, and the interrupt
/// when it records a mark.
///
/// The half-lap count is what the interrupt has served plus the half and
/// complete flags still pending, read between two loads of the served count
/// so the interrupt cannot have run in between. The flags are sticky, so
/// this holds while the interrupt is less than a full lap late. It also
/// assumes the reader never preempts [`UartIrq`], which clears the flags
/// before it adds to the count.
fn count_written(state: &State, rx_dma: &FlagHandle) -> (u32, u16) {
    let len = state.len.load(Ordering::Relaxed);
    let mut tries = 0;
    let (count, pending, remaining) = loop {
        let before = state.halves.load(Ordering::Acquire);
        let flags = rx_dma.flags();
        let remaining = u32::from(rx_dma.remaining());
        let after = state.halves.load(Ordering::Acquire);
        tries += 1;
        if before == after || tries == COUNT_TRIES {
            let pending = u32::from(flags.half) + u32::from(flags.complete);
            break (after, pending, remaining);
        }
    };
    // Nothing read from the ring may be read before the count.
    fence(Ordering::SeqCst);
    let halves = count
        .wrapping_sub(state.base.load(Ordering::Acquire))
        .wrapping_add(pending);
    ring::written(halves, remaining, len)
}

impl UartIrq {
    /// The vectors to bind to this handle; see [`UartInterrupts::verify`].
    pub fn interrupts(&self) -> UartInterrupts {
        self.vectors
    }

    /// Serves both the USART's and the receive stream's interrupt. Safe to
    /// call from either vector, or when neither is pending.
    pub fn on_interrupt(&mut self) {
        self.on_rx_dma();
        self.on_uart();
        self.on_tx_dma();
    }

    /// The transmit stream's vector: finishes the frame that went and
    /// starts the next.
    pub fn on_tx_dma(&mut self) {
        let Some(irq) = self.tx.as_mut() else {
            return;
        };
        // The task is servicing: it finishes the job and unmasks after. A
        // transfer it started may have unmasked the stream already, and a
        // completion pending with the stream unmasked would fire again on
        // every return, so mask here before leaving it to the task.
        let tx = &self.info.state.tx;
        if tx.servicing.load(Ordering::SeqCst) {
            irq.dma.set_interrupts(Interrupts::default());
            tx.deferred.store(true, Ordering::SeqCst);
            return;
        }
        service_tx(self.info, &mut irq.dma, irq.buf, irq.request);
    }

    /// The USART vector: marks where the line went idle and where line
    /// errors struck, and clears them.
    pub fn on_uart(&mut self) {
        let (bits, idle) = self.info.regs.take_events();
        if bits == 0 && !idle {
            return;
        }
        let state = &self.info.state;
        let epoch = state.epoch.load(Ordering::Acquire);
        let (at, _) = count_written(state, &self.rx_dma);
        if state.epoch.load(Ordering::Acquire) != epoch {
            // The ring restarted meanwhile; the position means nothing.
            return;
        }
        for (bit, kind, count) in [
            (ERR_OVERRUN, MarkKind::Overrun, &state.counts.overrun),
            (ERR_FRAMING, MarkKind::Framing, &state.counts.framing),
            (ERR_PARITY, MarkKind::Parity, &state.counts.parity),
            (ERR_NOISE, MarkKind::Noise, &state.counts.noise),
        ] {
            if bits & bit == 0 {
                continue;
            }
            count.fetch_add(1, Ordering::Relaxed);
            if !state.marks.push(Mark { at, kind, epoch }) {
                // No room: report it without its position rather than lose it.
                state.counts.marks_lost.fetch_add(1, Ordering::Relaxed);
                state.errors.fetch_or(bit, Ordering::Release);
            }
        }
        // A lost idle mark only merges two chunks; nothing else is lost.
        if idle
            && !state.marks.push(Mark {
                at,
                kind: MarkKind::Idle,
                epoch,
            })
        {
            state.counts.marks_lost.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The receive stream's vector: counts half-laps and records errors.
    pub fn on_rx_dma(&mut self) {
        let flags = self.rx_dma.flags();
        if !flags.any() {
            return;
        }
        self.rx_dma.clear(flags);
        let state = &self.info.state;
        // Both set means the interrupt ran late by up to half a lap; the
        // two crossings still count once each.
        let steps = u32::from(flags.half) + u32::from(flags.complete);
        if steps > 0 {
            state.halves.fetch_add(steps, Ordering::Release);
        }
        let mut bits = 0;
        if flags.transfer_error {
            bits |= ERR_DMA_TRANSFER;
            state.counts.dma_transfer.fetch_add(1, Ordering::Relaxed);
        }
        if flags.direct_mode_error || flags.fifo_error {
            bits |= ERR_DMA_OVERRUN;
            state.counts.dma_overrun.fetch_add(1, Ordering::Relaxed);
        }
        if bits != 0 {
            state.errors.fetch_or(bits, Ordering::Release);
        }
    }
}

/// The receive ring's length, checked: even, 2 to 65 534 bytes, in memory
/// the DMA reaches.
fn ring_len(ring: &[u8]) -> Result<u16, ConfigError> {
    let len = u16::try_from(ring.len())
        .ok()
        .filter(|&len| len >= 2 && len % 2 == 0)
        .ok_or(ConfigError::RxBuffer)?;
    crate::chip::check_dma_buffer(ring)?;
    Ok(len)
}

/// What an open computes before any token is consumed: the divider, the
/// buffers' checks and the wait arithmetic. `tx` is `None` for a
/// receive-only UART. Split out of the opens so a refusal can hand the
/// instance token back before anything has moved.
struct Checked {
    brr: u16,
    tx_len: u16,
    rx_len: u16,
    frame_cycles: u32,
}

fn check_open(
    clocks: Clocks,
    kernel: Bus,
    fractional_stop: bool,
    config: &Config,
    tx: Option<&[u8]>,
    rx: &[u8],
) -> Result<Checked, ConfigError> {
    let brr = config.divider(clocks.bus(kernel), fractional_stop)?;
    let tx_len = match tx {
        Some(tx) => {
            let len = u16::try_from(tx.len())
                .ok()
                .filter(|&len| len > 0)
                .ok_or(ConfigError::TxBuffer)?;
            crate::chip::check_dma_buffer(tx)?;
            len
        }
        None => 0,
    };
    let rx_len = ring_len(rx)?;
    let frame_cycles = frame_cycles(clocks.cpu, config.baudrate);
    Ok(Checked {
        brr,
        tx_len,
        rx_len,
        frame_cycles,
    })
}

/// Turns the USART's clock on, resets it, and resets its shared state.
fn reset(rcc: &mut Rcc, info: &Info, rx_len: u16) {
    rcc.enable(info.enable);
    rcc.reset(info.enable);
    let state = &info.state;
    state.halves.store(0, Ordering::Relaxed);
    state.base.store(0, Ordering::Relaxed);
    state.len.store(u32::from(rx_len), Ordering::Relaxed);
    state.marks.clear();
    state.errors.store(0, Ordering::Relaxed);
    state.counts.reset();
    state.tx.reset();
}

/// CPU cycles per frame, the unit of every counted wait.
fn frame_cycles(cpu: Hertz, baudrate: u32) -> u32 {
    (cpu.0 / baudrate.max(1))
        .saturating_mul(MAX_FRAME_BITS)
        .max(1)
}

impl<U, TxP, RxP, const TD: u8, const TN: u8, const RD: u8, const RN: u8>
    Wiring<U, TxP, RxP, Stream<TD, TN>, Stream<RD, RN>>
where
    U: Instance,
    TxP: TxPin<U> + AltPin,
    RxP: RxPin<U> + AltPin,
    Stream<TD, TN>: TxDma<U>,
    Stream<RD, RN>: RxDma<U>,
{
    /// Everything this wiring uses, as data: the instance, both pins with
    /// their alternate functions, both streams with their requests, and the
    /// three vectors to bind (line, receive, transmit).
    pub const RESOURCES: [Resource; 8] = [
        Resource::Peripheral(U::NAME),
        Resource::Pin {
            signal: Signal::Tx,
            port: TxP::PORT,
            pin: TxP::PIN,
            af: TxP::AF,
        },
        Resource::Pin {
            signal: Signal::Rx,
            port: RxP::PORT,
            pin: RxP::PIN,
            af: RxP::AF,
        },
        Resource::Dma {
            signal: Signal::Tx,
            controller: TD,
            stream: TN,
            request: <Stream<TD, TN> as TxDma<U>>::REQUEST,
        },
        Resource::Dma {
            signal: Signal::Rx,
            controller: RD,
            stream: RN,
            request: <Stream<RD, RN> as RxDma<U>>::REQUEST,
        },
        Resource::Interrupt(U::INTERRUPT_NAME),
        Resource::Interrupt(<Stream<RD, RN> as StreamInterrupt>::NAME),
        Resource::Interrupt(<Stream<TD, TN> as StreamInterrupt>::NAME),
    ];
}

impl<U, RxP, const RD: u8, const RN: u8> RxWiring<U, RxP, Stream<RD, RN>>
where
    U: Instance,
    RxP: RxPin<U> + AltPin,
    Stream<RD, RN>: RxDma<U>,
{
    /// Everything this receive-only wiring uses, as data.
    pub const RESOURCES: [Resource; 5] = [
        Resource::Peripheral(U::NAME),
        Resource::Pin {
            signal: Signal::Rx,
            port: RxP::PORT,
            pin: RxP::PIN,
            af: RxP::AF,
        },
        Resource::Dma {
            signal: Signal::Rx,
            controller: RD,
            stream: RN,
            request: <Stream<RD, RN> as RxDma<U>>::REQUEST,
        },
        Resource::Interrupt(U::INTERRUPT_NAME),
        Resource::Interrupt(<Stream<RD, RN> as StreamInterrupt>::NAME),
    ];
}

impl<U> embedded_io::ErrorType for Uart<U> {
    type Error = Error;
}

impl<U> embedded_io::ErrorType for UartTx<U> {
    type Error = Error;
}

impl<U> embedded_io::ErrorType for UartRx<U> {
    type Error = Error;
}

impl<U> embedded_io::Write for UartTx<U> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        UartTx::write(self, buf)
    }

    fn flush(&mut self) -> Result<(), Error> {
        UartTx::flush(self)
    }
}

impl<U> embedded_io::WriteReady for UartTx<U> {
    fn write_ready(&mut self) -> Result<bool, Error> {
        self.poll();
        self.take_error()?;
        let tx = &self.info.state.tx;
        let oldest = tx.frames.peek().map(|frame| frame.offset);
        Ok(tx.frames.len() < FRAMES && tx_ring::largest(self.write, oldest, self.cap) > 0)
    }
}

impl<U> embedded_io::Read for UartRx<U> {
    /// Blocks until at least one byte has arrived, as the trait requires.
    /// [`UartRx::read_within`] is the bounded form.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        loop {
            let len = self.try_read(buf)?;
            if len > 0 || buf.is_empty() {
                return Ok(len);
            }
        }
    }
}

impl<U> embedded_io::ReadReady for UartRx<U> {
    fn read_ready(&mut self) -> Result<bool, Error> {
        Ok(self.available()? > 0)
    }
}

impl<U> embedded_io::Write for Uart<U> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.tx.write(buf)
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.tx.flush()
    }
}

impl<U> embedded_io::WriteReady for Uart<U> {
    fn write_ready(&mut self) -> Result<bool, Error> {
        embedded_io::WriteReady::write_ready(&mut self.tx)
    }
}

impl<U: Instance> embedded_io::Read for Uart<U> {
    /// Blocks until at least one byte has arrived, as the trait requires.
    /// [`UartRx::read_within`], through the [`halves`](Uart::halves), is the
    /// bounded form.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        embedded_io::Read::read(&mut self.rx, buf)
    }
}

impl<U: Instance> embedded_io::ReadReady for Uart<U> {
    fn read_ready(&mut self) -> Result<bool, Error> {
        embedded_io::ReadReady::read_ready(&mut self.rx)
    }
}
