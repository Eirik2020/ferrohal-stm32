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

use core::{
    ptr,
    sync::atomic::{AtomicU8, AtomicU32, Ordering, fence},
};

use crate::{
    dma::{
        Channel, Direction, FifoMode, FlagHandle, Flags, Interrupts, Priority, StreamInstance,
        Transfer, WordSize,
    },
    rcc::{Rcc, RccBit},
    time::Hertz,
    uart::{Buffers, Config, ConfigError, Error, Wiring},
    usart::{Line, Regs},
};

/// Spins allowed for a DMA stream to report itself stopped. A stopping
/// stream finishes at most one word, so this is generous.
const STOP_SPINS: u32 = 10_000;
/// Bits per frame at most: start, 8 data, parity, 2 stop.
const MAX_FRAME_BITS: u32 = 12;
/// The fewest CPU cycles one pass of a wait loop takes: an empty
/// `try_read` (the `read_within` loop), a busy `poll_idle` (the transmit
/// wait). Measured with the DWT cycle counter (`hil` `measure_polls`) and
/// halved: F401 at 84 MHz 370 and 181 cycles, H753 at 400 MHz with caches
/// on 264 and 110. A faster pass would make a bound run short; the HIL
/// check `wait_bounds` catches that on every board.
const RX_PASS_CYCLES: u32 = 128;
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
    line: Line,
    enable: RccBit,
    state: State,
}

// SAFETY: `line` is an address, and `State` is atomics.
unsafe impl Sync for Info {}

impl Info {
    pub(crate) const fn new(line: Line, enable: RccBit) -> Self {
        Self {
            line,
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
    /// Error bits recorded and not yet reported.
    errors: AtomicU8,
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
            errors: AtomicU8::new(0),
        }
    }
}

/// A USART this chip has.
pub trait Instance: crate::Sealed + 'static {
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
pub unsafe trait TxDma<U: Instance>: StreamInstance {
    const REQUEST: u8;
}

/// A DMA stream that serves `U`'s receive requests.
///
/// # Safety
///
/// As [`TxDma`], for `U`'s RX request.
pub unsafe trait RxDma<U: Instance>: StreamInstance {
    const REQUEST: u8;
}

/// A UART, both directions. Implements `embedded_io`'s `Read`, `Write`,
/// `ReadReady` and `WriteReady`; [`split`](Self::split) gives two halves
/// that can live in different tasks.
pub struct Uart {
    tx: UartTx,
    rx: UartRx,
    kernel_clock: Hertz,
    cpu: Hertz,
}

/// The transmit half.
pub struct UartTx {
    info: &'static Info,
    dma: Channel,
    request: u8,
    buf: *mut u8,
    cap: u16,
    /// Bytes in the transfer now running, while `busy`.
    in_flight: u16,
    busy: bool,
    /// The stream did not stop; transmission is off for good.
    halted: bool,
    frame_cycles: u32,
}

/// The receive half.
pub struct UartRx {
    info: &'static Info,
    dma: Channel,
    request: u8,
    ring: *mut u8,
    len: u16,
    /// Bytes read since the ring last (re)started, wrapping.
    read_count: u32,
    /// Where the next byte to read sits in the ring.
    read_index: u16,
    /// `State::halves` when the ring last (re)started.
    base: u32,
    /// The stream did not stop; reception is off for good.
    halted: bool,
    frame_cycles: u32,
}

/// The interrupt side: call [`on_interrupt`](Self::on_interrupt) from the
/// USART's vector and the receive stream's vector.
pub struct UartIrq {
    info: &'static Info,
    rx_dma: FlagHandle,
}

// SAFETY: each half is the only owner of its buffer, its stream and the
// registers it writes; moving it to another context moves that ownership.
unsafe impl Send for UartTx {}
unsafe impl Send for UartRx {}

impl Uart {
    /// Configures the USART and both DMA streams, and starts reception.
    ///
    /// `wiring` comes from the board file; its pins must already be in
    /// their alternate function, which their types guarantee. Every check
    /// that can fail on the configuration runs before any register is
    /// written.
    pub fn new<U, TxP, RxP, TxD, RxD>(
        rcc: &mut Rcc,
        wiring: Wiring<U, TxP, RxP, TxD, RxD>,
        config: &Config,
        buffers: Buffers,
    ) -> Result<(Self, UartIrq), ConfigError>
    where
        U: Instance,
        TxP: TxPin<U>,
        RxP: RxPin<U>,
        TxD: TxDma<U>,
        RxD: RxDma<U>,
    {
        let info = U::info();
        let clocks = rcc.clocks();
        let kernel_clock = clocks.bus(info.enable.bus);
        let brr = baud_divider(kernel_clock, config.baudrate)?;
        let tx_len = u16::try_from(buffers.tx.len())
            .ok()
            .filter(|&len| len > 0)
            .ok_or(ConfigError::TxBuffer)?;
        let rx_len = u16::try_from(buffers.rx.len())
            .ok()
            .filter(|&len| len >= 2 && len % 2 == 0)
            .ok_or(ConfigError::RxBuffer)?;
        crate::chip::check_dma_buffer(buffers.tx)?;
        crate::chip::check_dma_buffer(buffers.rx)?;
        let frame_cycles = frame_cycles(clocks.cpu, config.baudrate);

        rcc.enable(info.enable);
        rcc.reset(info.enable);
        info.state.halves.store(0, Ordering::Relaxed);
        info.state.errors.store(0, Ordering::Relaxed);

        let mut tx = UartTx {
            info,
            dma: wiring.tx_dma.into_channel(),
            request: TxD::REQUEST,
            buf: buffers.tx.as_mut_ptr(),
            cap: tx_len,
            in_flight: 0,
            busy: false,
            halted: false,
            frame_cycles,
        };
        let mut rx = UartRx {
            info,
            dma: wiring.rx_dma.into_channel(),
            request: RxD::REQUEST,
            ring: buffers.rx.as_mut_ptr(),
            len: rx_len,
            read_count: 0,
            read_index: 0,
            base: 0,
            halted: false,
            frame_cycles,
        };

        // Streams left running by an earlier owner must stop before they
        // are pointed at new memory.
        tx.stop_stream().map_err(|_| ConfigError::DmaBusy)?;
        info.line.write_frame(config, brr);
        rx.start_ring().map_err(|_| ConfigError::DmaBusy)?;
        // The receiver last, so no byte arrives before the ring is ready.
        info.line.enable();

        let irq = UartIrq {
            info,
            rx_dma: rx.dma.flag_handle(),
        };
        Ok((
            Self {
                tx,
                rx,
                kernel_clock,
                cpu: clocks.cpu,
            },
            irq,
        ))
    }

    /// Changes the frame settings. Waits for transmission to finish first;
    /// the receive ring keeps running, but a byte arriving during the change
    /// is lost.
    pub fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        let brr = baud_divider(self.kernel_clock, config.baudrate)?;
        // A failed flush has already stopped the stream or halted the
        // transmit side, and the next write reports it.
        let _ = self.tx.flush();
        let line = self.tx.info.line;
        line.disable();
        line.write_frame(config, brr);
        line.enable();
        let cycles = frame_cycles(self.cpu, config.baudrate);
        self.tx.frame_cycles = cycles;
        self.rx.frame_cycles = cycles;
        Ok(())
    }

    pub fn split(self) -> (UartTx, UartRx) {
        (self.tx, self.rx)
    }

    /// Both halves at once, borrowed: [`split`](Self::split) without giving
    /// up `set_config`.
    pub fn halves(&mut self) -> (&mut UartTx, &mut UartRx) {
        (&mut self.tx, &mut self.rx)
    }

    pub fn tx(&mut self) -> &mut UartTx {
        &mut self.tx
    }

    pub fn rx(&mut self) -> &mut UartRx {
        &mut self.rx
    }

    /// The raw USART registers. FerroHAL's guarantees end here.
    ///
    /// # Safety
    ///
    /// The caller takes over every invariant this driver keeps.
    pub unsafe fn regs(&self) -> Regs {
        self.tx.info.line.regs()
    }
}

impl UartTx {
    /// Starts sending as much of `data` as fits the transmit buffer, waiting
    /// first for the previous transfer. Returns how many bytes it took.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        if self.halted {
            return Err(Error::Halted);
        }
        if data.is_empty() {
            return Ok(0);
        }
        self.wait_idle()?;
        self.start(data)
    }

    /// As [`write`](Self::write), but returns `Ok(0)` instead of waiting
    /// when a transfer is still running.
    pub fn try_write(&mut self, data: &[u8]) -> Result<usize, Error> {
        if self.halted {
            return Err(Error::Halted);
        }
        if data.is_empty() || !self.poll_idle()? {
            return Ok(0);
        }
        self.start(data)
    }

    /// Waits until every byte written has left the wire.
    pub fn flush(&mut self) -> Result<(), Error> {
        if self.halted {
            return Err(Error::Halted);
        }
        self.wait_idle()?;
        let line = self.info.line;
        if wait(self.budget(1, REGISTER_PASS_CYCLES), || line.tc()) {
            Ok(())
        } else {
            Err(Error::Timeout)
        }
    }

    /// Whether a write would start at once.
    pub fn is_idle(&mut self) -> Result<bool, Error> {
        if self.halted {
            return Err(Error::Halted);
        }
        self.poll_idle()
    }

    /// Sends a break: the line held low for a frame and more.
    pub fn send_break(&mut self) -> Result<(), Error> {
        self.flush()?;
        let line = self.info.line;
        line.request_break();
        if wait(self.budget(2, REGISTER_PASS_CYCLES), || !line.break_pending()) {
            Ok(())
        } else {
            Err(Error::Timeout)
        }
    }

    /// Passes of a loop costing at least `pass_cycles` each that cover
    /// `bytes` frames, twice over.
    fn budget(&self, bytes: u16, pass_cycles: u32) -> u32 {
        (u32::from(bytes) + 2)
            .saturating_mul(self.frame_cycles)
            .saturating_mul(2)
            / pass_cycles
    }

    fn start(&mut self, data: &[u8]) -> Result<usize, Error> {
        let len = data.len().min(usize::from(self.cap));
        // SAFETY: `buf` is this half's own buffer of `cap` bytes, and the
        // stream is stopped, so nothing else reads or writes it.
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), self.buf, len) };
        self.info.line.clear_tc();
        let transfer = Transfer {
            request: self.request,
            #[cfg(feature = "dma_v1")]
            bufferable: true,
            direction: Direction::MemoryToPeripheral,
            peripheral: self.info.line.tx_address(),
            memory: self.buf as u32,
            double_buffer: None,
            len: len as u16,
            size: WordSize::Byte,
            memory_increment: true,
            circular: false,
            priority: Priority::Medium,
            fifo: FifoMode::Direct,
            interrupts: Interrupts::default(),
        };
        // SAFETY: the buffer is this half's, `len` fits it, and it is not
        // touched again until `poll_idle` has seen the stream finish or
        // `stop_stream` has stopped it. The routing trait vouched for the
        // request.
        unsafe { self.dma.start(&transfer) }.map_err(|_| Error::Halted)?;
        self.busy = true;
        self.in_flight = len as u16;
        Ok(len)
    }

    /// Whether the last transfer has finished, reporting how it ended.
    fn poll_idle(&mut self) -> Result<bool, Error> {
        if !self.busy {
            return Ok(true);
        }
        let flags = self.dma.flags();
        if flags.transfer_error || flags.direct_mode_error {
            self.busy = false;
            self.stop_stream()?;
            return Err(if flags.transfer_error {
                Error::DmaTransfer
            } else {
                Error::DmaOverrun
            });
        }
        if flags.fifo_error {
            // Advisory in direct mode: the transfer carries on.
            self.dma.clear_flags(Flags {
                fifo_error: true,
                ..Flags::default()
            });
        }
        if flags.complete && !self.dma.is_running() {
            self.dma.clear_flags(Flags::ALL);
            self.busy = false;
            return Ok(true);
        }
        Ok(false)
    }

    fn wait_idle(&mut self) -> Result<(), Error> {
        for _ in 0..self.budget(self.in_flight, TX_PASS_CYCLES) {
            if self.poll_idle()? {
                return Ok(());
            }
        }
        if self.poll_idle()? {
            return Ok(());
        }
        self.busy = false;
        self.stop_stream()?;
        Err(Error::Timeout)
    }

    fn stop_stream(&mut self) -> Result<(), Error> {
        if self.dma.stop(STOP_SPINS).is_err() {
            self.halted = true;
            return Err(Error::Halted);
        }
        self.dma.clear_flags(Flags::ALL);
        Ok(())
    }
}

impl UartRx {
    /// Copies out what has arrived, up to `buf.len()` bytes, without
    /// waiting. `Ok(0)` means nothing yet.
    pub fn try_read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        let available = self.available()?;
        let len = buf.len().min(usize::from(available));
        if len == 0 {
            return Ok(0);
        }
        let mut index = usize::from(self.read_index);
        for byte in &mut buf[..len] {
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
        let (written, position) = self.write_count();
        if written.wrapping_sub(self.read_count) > u32::from(self.len) {
            self.resync(written, position);
            return Err(Error::BufferOverrun);
        }
        self.read_count = self.read_count.wrapping_add(len as u32);
        self.read_index = index as u16;
        Ok(len)
    }

    /// As [`try_read`](Self::try_read), but waits for at least one byte for
    /// as long as `frames` frames take at the configured baud rate.
    /// `Err(Timeout)` if none came.
    pub fn read_within(&mut self, buf: &mut [u8], frames: u32) -> Result<usize, Error> {
        let passes = frames.saturating_mul(self.frame_cycles) / RX_PASS_CYCLES;
        for _ in 0..passes {
            let len = self.try_read(buf)?;
            if len > 0 || buf.is_empty() {
                return Ok(len);
            }
        }
        match self.try_read(buf)? {
            0 if !buf.is_empty() => Err(Error::Timeout),
            len => Ok(len),
        }
    }

    /// Bytes waiting to be read.
    pub fn available(&mut self) -> Result<u16, Error> {
        if self.halted {
            return Err(Error::Halted);
        }
        if let Some(error) = self.take_error() {
            if !self.dma.is_running() {
                self.start_ring()?;
            }
            return Err(error);
        }
        if !self.dma.is_running() {
            // Stopped by an error whose interrupt has not run yet. Restart
            // now; the interrupt will find the flags already cleared.
            self.start_ring()?;
            return Err(Error::DmaTransfer);
        }
        let (written, position) = self.write_count();
        let available = written.wrapping_sub(self.read_count);
        if available > u32::from(self.len) {
            self.resync(written, position);
            return Err(Error::BufferOverrun);
        }
        Ok(available as u16)
    }

    /// The oldest error recorded and not yet reported.
    fn take_error(&mut self) -> Option<Error> {
        let errors = &self.info.state.errors;
        let pending = errors.load(Ordering::Acquire);
        let (bit, error) = ERRORS.iter().find(|(bit, _)| pending & bit != 0)?;
        errors.fetch_and(!bit, Ordering::AcqRel);
        Some(*error)
    }

    /// Bytes DMA has written since the ring (re)started, wrapping, and its
    /// position in the ring.
    ///
    /// NDTR gives the position within a lap; the half-lap count gives the
    /// lap. That count is what the interrupt has served plus the half and
    /// complete flags still pending, read between two loads of the served
    /// count so the interrupt cannot have run in between. A crossing after
    /// the flags were read shows in the position: an even count with the
    /// position in the second half means the half-transfer flag is now
    /// set, an odd count with the position in the first half means the
    /// complete flag is.
    ///
    /// The flags are sticky, so this holds while the interrupt is less than
    /// a full lap late. It also assumes the reader never preempts
    /// [`UartIrq`], which clears the flags before it adds to the count.
    fn write_count(&self) -> (u32, u16) {
        let halves = &self.info.state.halves;
        let len = u32::from(self.len);
        let mut tries = 0;
        let (count, pending, remaining) = loop {
            let before = halves.load(Ordering::Acquire);
            let flags = self.dma.flags();
            let remaining = u32::from(self.dma.remaining());
            let after = halves.load(Ordering::Acquire);
            tries += 1;
            if before == after || tries == COUNT_TRIES {
                let pending = u32::from(flags.half) + u32::from(flags.complete);
                break (after, pending, remaining);
            }
        };
        // Nothing read from the ring may be read before the count.
        fence(Ordering::SeqCst);
        let position = len - remaining.min(len);
        let halves = count.wrapping_sub(self.base).wrapping_add(pending);
        let lap = if halves % 2 == 1 && position < len / 2 {
            halves.wrapping_add(1) / 2
        } else {
            halves / 2
        };
        // `len` is even, so `lap * len` stays continuous when `halves` wraps.
        let written = lap.wrapping_mul(len).wrapping_add(position);
        let index = if position == len { 0 } else { position as u16 };
        (written, index)
    }

    /// Drops everything unread and carries on from DMA's position.
    fn resync(&mut self, written: u32, index: u16) {
        self.read_count = written;
        self.read_index = index;
    }

    /// Starts, or restarts, the ring from its first byte.
    fn start_ring(&mut self) -> Result<(), Error> {
        if self.dma.stop(STOP_SPINS).is_err() {
            self.halted = true;
            return Err(Error::Halted);
        }
        self.dma.clear_flags(Flags::ALL);
        self.base = self.info.state.halves.load(Ordering::Acquire);
        self.read_count = 0;
        self.read_index = 0;
        let transfer = Transfer {
            request: self.request,
            #[cfg(feature = "dma_v1")]
            bufferable: true,
            direction: Direction::PeripheralToMemory,
            peripheral: self.info.line.rx_address(),
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

impl UartIrq {
    /// Serves both the USART's and the receive stream's interrupt. Safe to
    /// call from either vector, or when neither is pending.
    pub fn on_interrupt(&mut self) {
        self.on_uart();
        self.on_rx_dma();
    }

    /// The USART vector: records line errors and clears them.
    pub fn on_uart(&mut self) {
        let bits = self.info.line.take_errors();
        if bits != 0 {
            self.info.state.errors.fetch_or(bits, Ordering::Release);
        }
    }

    /// The receive stream's vector: counts half-laps and records errors.
    pub fn on_rx_dma(&mut self) {
        let flags = self.rx_dma.flags();
        if !flags.any() {
            return;
        }
        self.rx_dma.clear(flags);
        // Both set means the interrupt ran late by up to half a lap; the
        // two crossings still count once each.
        let steps = u32::from(flags.half) + u32::from(flags.complete);
        if steps > 0 {
            self.info.state.halves.fetch_add(steps, Ordering::Release);
        }
        let mut bits = 0;
        if flags.transfer_error {
            bits |= ERR_DMA_TRANSFER;
        }
        if flags.direct_mode_error || flags.fifo_error {
            bits |= ERR_DMA_OVERRUN;
        }
        if bits != 0 {
            self.info.state.errors.fetch_or(bits, Ordering::Release);
        }
    }
}

/// BRR for oversampling by 16 with no prescaler: the kernel clock over the
/// baud rate, in 12.4 fixed point, which is the plain quotient rounded.
fn baud_divider(kernel_clock: Hertz, baudrate: u32) -> Result<u16, ConfigError> {
    if baudrate == 0 {
        return Err(ConfigError::BaudRate);
    }
    let clock = u64::from(kernel_clock.0);
    let baud = u64::from(baudrate);
    let brr = (clock + baud / 2) / baud;
    if !(16..=0xffff).contains(&brr) {
        return Err(ConfigError::BaudRate);
    }
    // Within 1 %: |clock / brr - baud| <= baud / 100.
    let error = clock.abs_diff(brr * baud);
    if error * 100 > brr * baud {
        return Err(ConfigError::BaudRate);
    }
    Ok(brr as u16)
}

/// CPU cycles per frame, the unit of every counted wait.
fn frame_cycles(cpu: Hertz, baudrate: u32) -> u32 {
    (cpu.0 / baudrate.max(1)).saturating_mul(MAX_FRAME_BITS).max(1)
}

/// Spins until `done` or `spins` run out.
fn wait(spins: u32, mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..spins {
        if done() {
            return true;
        }
    }
    done()
}

impl embedded_io::ErrorType for Uart {
    type Error = Error;
}

impl embedded_io::ErrorType for UartTx {
    type Error = Error;
}

impl embedded_io::ErrorType for UartRx {
    type Error = Error;
}

impl embedded_io::Write for UartTx {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        UartTx::write(self, buf)
    }

    fn flush(&mut self) -> Result<(), Error> {
        UartTx::flush(self)
    }
}

impl embedded_io::WriteReady for UartTx {
    fn write_ready(&mut self) -> Result<bool, Error> {
        self.is_idle()
    }
}

impl embedded_io::Read for UartRx {
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

impl embedded_io::ReadReady for UartRx {
    fn read_ready(&mut self) -> Result<bool, Error> {
        Ok(self.available()? > 0)
    }
}

impl embedded_io::Write for Uart {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.tx.write(buf)
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.tx.flush()
    }
}

impl embedded_io::WriteReady for Uart {
    fn write_ready(&mut self) -> Result<bool, Error> {
        self.tx.is_idle()
    }
}

impl embedded_io::Read for Uart {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        embedded_io::Read::read(&mut self.rx, buf)
    }
}

impl embedded_io::ReadReady for Uart {
    fn read_ready(&mut self) -> Result<bool, Error> {
        embedded_io::ReadReady::read_ready(&mut self.rx)
    }
}
