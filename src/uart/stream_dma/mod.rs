//! The UART driver on a stream DMA (`dma_v1`, `dma_v2`), for any USART IP
//! version. The IP module (`usart_v2`, `usart_v4`) supplies `Unit`, its
//! register operations; everything here is the same on every chip.
//!
//! # One owner
//!
//! A [`Uart`] holds everything: the instance token, the pins, the stream
//! tokens, both buffers and every piece of state, as plain fields. Every
//! operation takes it `&mut`, [`Uart::on_interrupt`] included. A task and a
//! handler that both use it share it under the scheduler's lock (RTIC's
//! `#[shared]` and `lock`); there is no other concurrency control, and no
//! critical section (ADR-011).
//!
//! # How it moves bytes
//!
//! - **Transmit** ([`tx`]): `try_send` and `write` copy a frame into the
//!   transmit buffer and queue it; one DMA transfer per frame, chained by
//!   [`Uart::on_interrupt`] from the transmit stream's vector, or by the next
//!   call that touches the queue when that vector is unbound.
//! - **Receive** ([`rx`]): one DMA stream runs for ever in circular mode,
//!   filling the receive ring. A read serves the stream's half and complete
//!   flags itself, then copies out what has arrived; the flags still
//!   pending are counted with the served ones, so a service up to most of a
//!   lap late loses nothing.
//! - **Errors**: line errors (overrun, framing, parity, noise) raise the
//!   USART interrupt, and DMA errors the stream's. The service records each
//!   one; reads report them in turn, and reception carries on. A DMA bus
//!   error stops the stream; the next read reports it and restarts the ring.
//!
//! # Interrupts
//!
//! FerroHAL enables the peripheral's own interrupt bits (USART: idle,
//! parity, errors, and transmission complete while the end of the last
//! queued frame is awaited; receive stream: half, complete, errors; transmit stream:
//! complete, errors) and nothing else. The scheduler binds the vectors in
//! [`Uart::vectors`] and calls [`Uart::on_interrupt`] from each.
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
//! arrives; [`Uart::read_within`] is the bounded form.
//!
//! # Memory
//!
//! Both buffers must be memory the chosen DMA controller can reach. On a
//! chip with a data cache, while it is on, they must also be memory the MPU
//! marks non-cacheable: this driver does no cache maintenance. `Uart::new`
//! checks both with `chip::check_dma_buffer`.
//!
//! # DMA engines
//!
//! The engine reaches DMA only through the erased stream's start, stop,
//! flags, clear, remaining count and interrupt enables. A channel DMA
//! (`bdma_v1` behind DMAMUX) slots in by offering the same operations
//! (ADR-011, forward compatibility).
//!
//! Requirements: UART-002, UART-003, UART-004, UART-005, UART-006, UART-007, UART-008, UART-009, UART-010, UART-011, UART-012, UART-013, UART-014, UART-015, UART-020, UARTDMA-002, UARTDMA-008, UARTDMA-009, API-002, API-003, UARTDMA-011, UARTDMA-012, RESOURCE-002, UART-022, UART-023

mod io;
mod rx;
mod tx;

use crate::{
    chip::Interrupt,
    dma::{StopTimeout, Stream, StreamInstance, StreamInterrupt},
    gpio::AltPin,
    rcc::{Clocks, Rcc, RccBit},
    resource::{Service, Vectors},
    time::{CycleClock, Hertz},
    uart::{Buffers, Config, ConfigError, Error, Refused, Resource, RxWiring, Signal, Wiring},
    usart_ip::Unit,
    wait::{Deadline, wait},
};

use rx::Rx;
use tx::Tx;

/// Bits per frame at most: start, 8 data, parity, 2 stop.
const MAX_FRAME_BITS: u32 = 12;
/// A pass that only polls a register (TC, SBK) counts as one cycle. These
/// waits end within a frame or two unless the hardware is stuck, so a
/// generous bound costs nothing in normal running.
const REGISTER_PASS_CYCLES: u32 = 1;

/// Per-instance facts: the register wrapper. The glue layer holds one static
/// `Info` per USART.
pub type Info = crate::resource::Info<Unit>;

// SAFETY: `Unit` is an address; every access through it is by the one
// `Uart` that holds the instance token.
unsafe impl Sync for Info {}

pub(crate) const ERR_DMA_TRANSFER: u8 = 1 << 0;
pub(crate) const ERR_OVERRUN: u8 = 1 << 1;
pub(crate) const ERR_FRAMING: u8 = 1 << 2;
pub(crate) const ERR_PARITY: u8 = 1 << 3;
pub(crate) const ERR_NOISE: u8 = 1 << 4;
pub(crate) const ERR_DMA_OVERRUN: u8 = 1 << 5;

/// A USART this chip has.
pub trait Instance: crate::Sealed + 'static {
    /// Its kernel clock: the bus it sits on.
    const KERNEL: crate::rcc::Kernel;
    /// Its name in the reference manual.
    const NAME: &'static str;
    /// Its RCC enable (and reset) bit.
    const ENABLE: RccBit;
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

/// What a UART was opened from, by role: the instance, the pins and the
/// streams the [`Uart`] keeps, and [`Uart::free`] hands back. Implemented by
/// [`Wiring`] and [`RxWiring`] only, so a UART's type names the board's
/// whole route.
pub trait Parts: crate::Sealed {
    type Instance: Instance;
    type TxPin;
    type RxPin;
    type TxDma;
    type RxDma;
}

impl<U, TxP, RxP, TxD, RxD> crate::Sealed for Wiring<U, TxP, RxP, TxD, RxD> {}
impl<U: Instance, TxP, RxP, TxD, RxD> Parts for Wiring<U, TxP, RxP, TxD, RxD> {
    type Instance = U;
    type TxPin = TxP;
    type RxPin = RxP;
    type TxDma = TxD;
    type RxDma = RxD;
}

impl<U, RxP, RxD> crate::Sealed for RxWiring<U, RxP, RxD> {}
/// A receive-only UART has no transmit side: its transmit parts are `()`.
impl<U: Instance, RxP, RxD> Parts for RxWiring<U, RxP, RxD> {
    type Instance = U;
    type TxPin = ();
    type RxPin = RxP;
    type TxDma = ();
    type RxDma = RxD;
}

/// Whether instance `U` can take `config` under `clocks`; see
/// [`Config::check`]. A `const fn`, so a board can assert it while
/// compiling: `const _: () = assert!(uart::check::<USART6>(&CLOCKS,
/// &CRSF).is_ok());`.
pub const fn check<U: Instance>(clocks: &Clocks, config: &Config) -> Result<(), ConfigError> {
    match config.divider(U::KERNEL.clock(clocks), U::FRACTIONAL_STOP_BITS) {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}

/// What a [`Uart::new`] refusal hands back: the reason, and the wiring and
/// buffers it took.
pub type UartRefused<W> = Refused<ConfigError, (W, Buffers)>;

/// What a [`Uart::new_rx`] refusal hands back: the reason, and the wiring
/// and ring it took.
pub type UartRxRefused<W> = Refused<ConfigError, (W, &'static mut [u8])>;

/// What one [`Uart::on_interrupt`] found. Each field is a reason to wake a
/// task; none needs one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Events {
    /// The line went idle: a frame has ended, and
    /// [`read_chunk`](Uart::read_chunk) has a chunk to give.
    pub idle: bool,
    /// The receive stream crossed half or the end of the ring: a reader of a
    /// continuous stream should read before another half fills.
    pub filled: bool,
    /// A transmit frame left the wire, or failed: there is room to queue
    /// another.
    pub sent: bool,
    /// A line or DMA fault was recorded; the next read reports it.
    pub fault: bool,
    /// The transmit queue emptied and the USART's transmission complete
    /// (TC) followed: the last stop bit of the last frame has left the
    /// wire. Reported once per emptying, however many frames came before.
    pub tx_idle: bool,
}

impl Events {
    /// Whether anything happened.
    pub fn any(&self) -> bool {
        self.idle || self.filled || self.sent || self.fault || self.tx_idle
    }
}

/// A UART: both directions on a full [`Wiring`], receive only on an
/// [`RxWiring`]. The one owner of everything it was opened from: the
/// instance token (a second `Uart` for the same instance cannot be built),
/// the pins, the stream tokens and the buffers, all handed back by
/// [`free`](Self::free).
///
/// Implements `embedded_io`'s `Read` and `ReadReady`, and on a full wiring
/// `Write` and `WriteReady`. Firmware that reads in one task and writes in
/// another shares the `Uart` between them, and with its handler, under the
/// scheduler's lock.
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
///   such as [`read_chunk`](Self::read_chunk), needs a timeout fallback on
///   v2, such as a timer, not IDLE alone.
pub struct Uart<W: Parts> {
    unit: Unit,
    /// The bus clock the baud rate divides.
    kernel_clock: Hertz,
    /// The instance's `FRACTIONAL_STOP_BITS`.
    fractional_stop: bool,
    cpu: Hertz,
    /// CPU cycles one frame lasts at the current baud rate.
    frame_cycles: u32,
    clock: Option<&'static dyn CycleClock>,
    rx: Rx,
    /// `None` on a receive-only UART.
    tx: Option<Tx>,
    vectors: Vectors,
    token: W::Instance,
    rx_pin: W::RxPin,
    rx_stream: W::RxDma,
    tx_pin: W::TxPin,
    tx_stream: W::TxDma,
}

// SAFETY: the only owner of its buffers, its streams and its USART; moving
// it to another context moves that ownership. Its tokens move with it.
unsafe impl<W: Parts> Send for Uart<W>
where
    W::Instance: Send,
    W::TxPin: Send,
    W::RxPin: Send,
    W::TxDma: Send,
    W::RxDma: Send,
{
}

impl<U, TxP, RxP, TxD, RxD> Uart<Wiring<U, TxP, RxP, TxD, RxD>>
where
    U: Instance,
    TxP: TxPin<U>,
    RxP: RxPin<U>,
    TxD: TxDma<U>,
    RxD: RxDma<U>,
{
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
    /// On success the UART keeps every part; [`free`](Self::free) returns
    /// the wiring and the buffers whole, so the same parts open again.
    #[allow(clippy::type_complexity)]
    pub fn new(
        rcc: &mut Rcc,
        wiring: Wiring<U, TxP, RxP, TxD, RxD>,
        config: &Config,
        mut buffers: Buffers,
    ) -> Result<Self, UartRefused<Wiring<U, TxP, RxP, TxD, RxD>>> {
        // After `Ok` the driver is the only user of the streams and of the
        // buffers' memory: it keeps raw pointers into `buffers`, so nothing
        // here may touch `buffers` again once the open succeeds.
        match Self::open(rcc, wiring, config, &mut buffers) {
            Ok(uart) => Ok(uart),
            Err((error, wiring)) => Err(Refused::new(error, (wiring, buffers))),
        }
    }

    #[allow(clippy::type_complexity)]
    fn open(
        rcc: &mut Rcc,
        wiring: Wiring<U, TxP, RxP, TxD, RxD>,
        config: &Config,
        buffers: &mut Buffers,
    ) -> Result<Self, (ConfigError, Wiring<U, TxP, RxP, TxD, RxD>)> {
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
            Err(error) => return Err((error, wiring)),
        };
        let unit = U::info().unit();
        rcc.open(U::ENABLE);
        let mut tx = Tx::new(
            crate::dma::claim(&wiring.tx_dma),
            TxD::REQUEST,
            buffers.tx,
            checked.tx_len,
        );
        let rx = Rx::new(
            crate::dma::claim(&wiring.rx_dma),
            RxD::REQUEST,
            buffers.rx,
            checked.rx_len,
        );
        // Streams left running by an earlier owner must stop before they
        // are pointed at new memory.
        if tx.reset_stream().is_err() {
            return Err((ConfigError::DmaBusy, wiring));
        }
        let vectors = Vectors::one(U::INTERRUPT)
            .with(<RxD as StreamInterrupt>::INTERRUPT)
            .with(<TxD as StreamInterrupt>::INTERRUPT);
        let Wiring {
            uart,
            tx: tx_pin,
            rx: rx_pin,
            tx_dma,
            rx_dma,
        } = wiring;
        let mut opened = Self {
            unit,
            kernel_clock: U::KERNEL.clock(&clocks),
            fractional_stop: U::FRACTIONAL_STOP_BITS,
            cpu: clocks.cpu,
            frame_cycles: checked.frame_cycles,
            clock: None,
            rx,
            tx: Some(tx),
            vectors,
            token: uart,
            rx_pin,
            rx_stream: rx_dma,
            tx_pin,
            tx_stream: tx_dma,
        };
        if opened.start(config, checked.brr).is_err() {
            return Err((ConfigError::DmaBusy, opened.into_wiring()));
        }
        Ok(opened)
    }

    /// The wiring again, from the parts this UART holds.
    fn into_wiring(self) -> Wiring<U, TxP, RxP, TxD, RxD> {
        Wiring {
            uart: self.token,
            tx: self.tx_pin,
            rx: self.rx_pin,
            tx_dma: self.tx_stream,
            rx_dma: self.rx_stream,
        }
    }

    /// Stops the line and both streams, and hands back what
    /// [`new`](Self::new) took: the wiring (the instance, both pins, both
    /// stream tokens) and the buffers. What a `Drop` would do, made
    /// explicit so the parts can move out. The transmit queue is dropped.
    /// A reopen runs `Uart::new` again on the same parts.
    ///
    /// A stream that does not stop in time could still write the buffers,
    /// so they are not handed out: the refusal returns the UART, closed and
    /// both directions halted (sends and reads report `Halted`), still owning
    /// them. `free` may be tried again; it succeeds once both streams have
    /// stopped.
    // The refusal hands back the UART itself, which still owns buffers a
    // stream may write: it cannot be boxed without an allocator, and a
    // caller holds one either way.
    #[allow(clippy::type_complexity, clippy::result_large_err)]
    pub fn free(
        mut self,
    ) -> Result<(Wiring<U, TxP, RxP, TxD, RxD>, Buffers), Refused<StopTimeout, Self>> {
        let stopped = self.close();
        if stopped.is_err() {
            return Err(Refused::new(StopTimeout, self));
        }
        let (tx, tx_len) = match self.tx.as_ref() {
            Some(tx) => tx.buffer(),
            None => (core::ptr::null_mut(), 0),
        };
        let (rx, rx_len) = self.rx.buffer();
        // SAFETY: both were the caller's `'static` buffers, of these lengths
        // (`check_open` refuses longer ones), and the driver was their only
        // user since `new`; both streams have stopped (checked above), so
        // nothing else will touch them again. A full UART always has `tx`.
        let buffers = unsafe {
            Buffers {
                tx: if tx.is_null() {
                    &mut []
                } else {
                    core::slice::from_raw_parts_mut(tx, tx_len)
                },
                rx: core::slice::from_raw_parts_mut(rx, rx_len),
            }
        };
        Ok((self.into_wiring(), buffers))
    }

    /// Changes the frame settings. Waits for transmission to finish first;
    /// the receive ring keeps running, but a byte arriving during the change
    /// is lost.
    pub fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        let brr = config.divider(self.kernel_clock, self.fractional_stop)?;
        // A failed flush has already stopped the stream or halted the
        // transmit side, and the next write reports it.
        let _ = self.flush();
        self.write_frame(config, brr, true);
        Ok(())
    }

    /// Sends a break: the line held low for a frame and more.
    pub fn send_break(&mut self) -> Result<(), Error> {
        self.flush()?;
        let unit = self.unit;
        unit.request_break();
        let deadline = Deadline::new(self.clock, self.budget(2), REGISTER_PASS_CYCLES);
        if wait(deadline, || !unit.break_pending()) {
            Ok(())
        } else {
            Err(Error::Timeout)
        }
    }
}

impl<U, RxP, RxD> Uart<RxWiring<U, RxP, RxD>>
where
    U: Instance,
    RxP: RxPin<U>,
    RxD: RxDma<U>,
{
    /// Opens a receive-only UART: no TX pin, no transmit stream, the
    /// transmitter left off. The same `Uart` as a full one, without the
    /// transmit operations.
    ///
    /// A refusal hands back `wiring` and `ring` with the reason, as
    /// [`Uart::new`] does. A `DmaBusy` leaves the hardware as that
    /// describes for the receive stream, with no transmit stream.
    pub fn new_rx(
        rcc: &mut Rcc,
        wiring: RxWiring<U, RxP, RxD>,
        config: &Config,
        ring: &'static mut [u8],
    ) -> Result<Self, UartRxRefused<RxWiring<U, RxP, RxD>>> {
        // After `Ok` nothing here may touch `ring` again.
        match Self::open_rx(rcc, wiring, config, ring) {
            Ok(uart) => Ok(uart),
            Err((error, wiring)) => Err(Refused::new(error, (wiring, ring))),
        }
    }

    fn open_rx(
        rcc: &mut Rcc,
        wiring: RxWiring<U, RxP, RxD>,
        config: &Config,
        ring: &mut [u8],
    ) -> Result<Self, (ConfigError, RxWiring<U, RxP, RxD>)> {
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
            Err(error) => return Err((error, wiring)),
        };
        let unit = U::info().unit();
        rcc.open(U::ENABLE);
        let rx = Rx::new(
            crate::dma::claim(&wiring.rx_dma),
            RxD::REQUEST,
            ring,
            checked.rx_len,
        );
        let vectors = Vectors::one(U::INTERRUPT).with(<RxD as StreamInterrupt>::INTERRUPT);
        let RxWiring {
            uart,
            rx: rx_pin,
            rx_dma,
        } = wiring;
        let mut opened = Self {
            unit,
            kernel_clock: U::KERNEL.clock(&clocks),
            fractional_stop: U::FRACTIONAL_STOP_BITS,
            cpu: clocks.cpu,
            frame_cycles: checked.frame_cycles,
            clock: None,
            rx,
            tx: None,
            vectors,
            token: uart,
            rx_pin,
            rx_stream: rx_dma,
            tx_pin: (),
            tx_stream: (),
        };
        if opened.start(config, checked.brr).is_err() {
            return Err((ConfigError::DmaBusy, opened.into_wiring()));
        }
        Ok(opened)
    }

    fn into_wiring(self) -> RxWiring<U, RxP, RxD> {
        RxWiring {
            uart: self.token,
            rx: self.rx_pin,
            rx_dma: self.rx_stream,
        }
    }

    /// Stops the receive stream and closes the line, and hands back what
    /// [`new_rx`](Self::new_rx) took: the wiring (the instance, the pin, the
    /// stream token) and the ring, as [`Uart::free`] does for a full UART.
    // The refusal hands back the UART itself, which still owns buffers a
    // stream may write: it cannot be boxed without an allocator, and a
    // caller holds one either way.
    #[allow(clippy::type_complexity, clippy::result_large_err)]
    pub fn free(
        mut self,
    ) -> Result<(RxWiring<U, RxP, RxD>, &'static mut [u8]), Refused<StopTimeout, Self>> {
        if self.close().is_err() {
            return Err(Refused::new(StopTimeout, self));
        }
        let (ring, len) = self.rx.buffer();
        // SAFETY: the caller's `'static` ring, of this length, whose only
        // user since `new_rx` was the driver; the stream has stopped
        // (checked above).
        let ring = unsafe { core::slice::from_raw_parts_mut(ring, len) };
        Ok((self.into_wiring(), ring))
    }

    /// Changes the frame settings. The receive ring keeps running, but a
    /// byte arriving during the change is lost.
    pub fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        let brr = config.divider(self.kernel_clock, self.fractional_stop)?;
        check_rx_only(config)?;
        self.write_frame(config, brr, false);
        Ok(())
    }
}

impl<W: Parts> Uart<W> {
    /// Writes the frame, starts the ring, then turns the receiver (and the
    /// transmitter, with a transmit side) on: the receiver last, so no byte
    /// arrives before the ring is ready.
    fn start(&mut self, config: &Config, brr: u16) -> Result<(), Error> {
        self.unit.write_frame(config, brr);
        self.rx.start_ring(self.unit)?;
        self.unit.enable(self.tx.is_some());
        Ok(())
    }

    fn write_frame(&mut self, config: &Config, brr: u16, tx: bool) {
        self.unit.disable();
        self.unit.write_frame(config, brr);
        self.unit.enable(tx);
        self.frame_cycles = frame_cycles(self.cpu, config.baudrate);
    }

    /// Stops both streams and the line, and drops the transmit queue: what
    /// `free` runs. Both directions are halted if a stream does not stop.
    fn close(&mut self) -> Result<(), StopTimeout> {
        let tx_stopped = match self.tx.as_mut() {
            Some(tx) => tx.halt(),
            None => Ok(()),
        };
        let rx_stopped = self.rx.halt();
        self.unit.set_event_enables(false);
        // TC stays set once the line is quiet: with TCIE left on after a
        // drain the caller never served, the USART's vector would fire for
        // ever once nothing serves it.
        self.unit.set_tc_interrupt(false);
        self.unit.disable();
        tx_stopped.and(rx_stopped)
    }

    /// Serves the USART, the receive stream and the transmit stream: records
    /// where the line went idle and where faults struck, counts the ring's
    /// half-laps, finishes the frame that went and starts the next. Call it
    /// from every vector in [`vectors`](Self::vectors), or from a task to
    /// poll; it is safe with nothing pending.
    pub fn on_interrupt(&mut self) -> Events {
        let mut events = Events::default();
        let (filled, dma_fault) = self.rx.serve_stream();
        events.filled = filled;
        let (idle, line_fault) = self.rx.serve_line(self.unit);
        events.idle = idle;
        events.fault = dma_fault || line_fault;
        if let Some(tx) = self.tx.as_mut() {
            let (sent, fault) = tx.service(self.unit);
            events.sent = sent;
            events.fault |= fault;
            events.tx_idle = tx.take_idle();
        }
        events
    }

    /// The vectors to bind to [`on_interrupt`](Self::on_interrupt): the
    /// USART's, the receive stream's and, with a transmit side, the transmit
    /// stream's. Binding the transmit stream's is optional: queued frames
    /// also move on whenever the UART is called.
    pub fn vectors(&self) -> Vectors {
        self.vectors
    }

    /// Stops the line raising its own interrupt: clears the idle, parity,
    /// error and transmission-complete enables. The vectors stay enabled in the NVIC, so
    /// [`Vectors::verify`] still passes, and an event that arrives while
    /// masked stays pending in the USART and is reported after
    /// [`unmask`](Self::unmask).
    pub fn mask(&mut self) {
        self.unit.set_event_enables(false);
        if let Some(tx) = self.tx.as_mut() {
            tx.set_masked(self.unit, true);
        }
    }

    /// Restores the idle, parity and error enables, and the
    /// transmission-complete enable if the last frame's end is awaited.
    pub fn unmask(&mut self) {
        self.unit.set_event_enables(true);
        if let Some(tx) = self.tx.as_mut() {
            tx.set_masked(self.unit, false);
        }
    }

    /// Measures every wait with `clock` instead of counting passes, so a
    /// bound lasts the time asked. `None` goes back to counting.
    pub fn set_clock(&mut self, clock: Option<&'static dyn CycleClock>) {
        self.clock = clock;
    }

    /// CPU cycles that cover `bytes` frames, twice over.
    fn budget(&self, bytes: u16) -> u32 {
        (u32::from(bytes) + 2)
            .saturating_mul(self.frame_cycles)
            .saturating_mul(2)
    }
}

impl<W: Parts> Service for Uart<W> {
    type Event = Events;

    fn on_interrupt(&mut self) -> Events {
        Uart::on_interrupt(self)
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
    kernel: crate::rcc::Kernel,
    fractional_stop: bool,
    config: &Config,
    tx: Option<&[u8]>,
    rx: &[u8],
) -> Result<Checked, ConfigError> {
    let brr = config.divider(kernel.clock(&clocks), fractional_stop)?;
    if tx.is_none() {
        check_rx_only(config)?;
    }
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

/// What a receive-only UART refuses beyond [`Config::divider`]: half-duplex
/// and swap, which both need the TX pin it does not hold.
fn check_rx_only(config: &Config) -> Result<(), ConfigError> {
    if config.half_duplex || config.swap {
        Err(ConfigError::Unsupported)
    } else {
        Ok(())
    }
}

/// CPU cycles per frame, the unit of every counted wait.
fn frame_cycles(cpu: Hertz, baudrate: Hertz) -> u32 {
    (cpu.0 / baudrate.0.max(1))
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
        Resource::pin::<TxP>(Signal::Tx),
        Resource::pin::<RxP>(Signal::Rx),
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
        Resource::Interrupt(<Stream<RD, RN> as StreamInterrupt>::INTERRUPT_NAME),
        Resource::Interrupt(<Stream<TD, TN> as StreamInterrupt>::INTERRUPT_NAME),
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
        Resource::pin::<RxP>(Signal::Rx),
        Resource::Dma {
            signal: Signal::Rx,
            controller: RD,
            stream: RN,
            request: <Stream<RD, RN> as RxDma<U>>::REQUEST,
        },
        Resource::Interrupt(U::INTERRUPT_NAME),
        Resource::Interrupt(<Stream<RD, RN> as StreamInterrupt>::INTERRUPT_NAME),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_receive_only_uart_refuses_what_needs_a_tx_pin() {
        let frame = Config::new(Hertz(115_200));
        assert_eq!(check_rx_only(&frame), Ok(()));
        assert_eq!(
            check_rx_only(&frame.with_half_duplex(true)),
            Err(ConfigError::Unsupported)
        );
        assert_eq!(
            check_rx_only(&frame.with_swap(true)),
            Err(ConfigError::Unsupported)
        );
    }
}
