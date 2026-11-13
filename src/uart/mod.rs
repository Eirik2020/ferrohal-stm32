//! The UART every chip offers, independent of the hardware underneath.
//!
//! This module is chip-neutral. It defines what a UART driver takes and
//! reports; each IP version (`usart_v2` on F4, `usart_v4` on H7, later
//! others) supplies a driver with the same shape:
//!
//! - **init**: `Uart::new(&mut rcc, wiring, &config, buffers)`. The
//!   [`Wiring`] comes from the board file and names the instance, pins and
//!   DMA streams; the IP's routing traits reject a wrong combination at
//!   compile time. A refusal is a [`Refused`]: the reason, with the wiring
//!   and buffers handed back.
//! - **read and write**: `embedded_io::{Read, Write, ReadReady, WriteReady}`
//!   on the driver and on its two halves.
//! - **interrupts**: a `UartIrq` handle whose `on_interrupt()` the
//!   scheduler's handlers call. FerroHAL never touches the NVIC.
//!
//! The driver for the enabled chip is re-exported here, so firmware names
//! `ferrohal_stm32::uart::Uart` whatever the IP version.
//!
//! Requirements: UART-001, UART-002, UART-003, UART-004, UART-005, UART-007, UART-008, UART-009, UART-010, UART-012, UART-013, UART-014, UART-015

use core::fmt;

#[cfg(any(feature = "dma_v1", feature = "dma_v2"))]
mod ring;
#[cfg(any(feature = "dma_v1", feature = "dma_v2"))]
pub(crate) mod stream_dma;
#[cfg(any(feature = "dma_v1", feature = "dma_v2"))]
mod tx_ring;

#[cfg(any(feature = "usart_v2", feature = "usart_v4"))]
pub use crate::usart::{Instance, Line, RxLine, Uart, UartIrq, UartRx, UartTx};

/// Frame settings. Data is always 8 bits; with parity on, the parity bit
/// is a ninth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    pub baudrate: u32,
    pub parity: Parity,
    pub stop_bits: StopBits,
    /// The RX line idles low and its levels are swapped, as SBUS sends
    /// them. Only some IP versions can do this; the others refuse it with
    /// [`ConfigError::Unsupported`].
    pub invert_rx: bool,
    /// The TX line likewise.
    pub invert_tx: bool,
}

impl Config {
    /// `baudrate`, 8N1, not inverted.
    pub const fn new(baudrate: u32) -> Self {
        Self {
            baudrate,
            parity: Parity::None,
            stop_bits: StopBits::One,
            invert_rx: false,
            invert_tx: false,
        }
    }

    pub const fn with_parity(self, parity: Parity) -> Self {
        Self { parity, ..self }
    }

    pub const fn with_stop_bits(self, stop_bits: StopBits) -> Self {
        Self { stop_bits, ..self }
    }

    pub const fn with_inverted_rx(self, invert_rx: bool) -> Self {
        Self { invert_rx, ..self }
    }

    pub const fn with_inverted_tx(self, invert_tx: bool) -> Self {
        Self { invert_tx, ..self }
    }

    /// Whether a UART clocked at `kernel_clock` can take this frame: the
    /// baud rate within 1 %, and only the features its IP version has. A
    /// `const fn`, so a board can check its frames while compiling, as it
    /// checks its clock tree; `Uart::check` finds the kernel clock for an
    /// instance, and adds its own limits (such as the F4's UART4 and UART5,
    /// which have no 0.5 or 1.5 stop bits).
    pub const fn check(&self, kernel_clock: crate::time::Hertz) -> Result<(), ConfigError> {
        match self.divider(kernel_clock, true) {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// BRR for oversampling by 16 with no prescaler: the kernel clock over
    /// the baud rate, in 12.4 fixed point, which is the plain quotient
    /// rounded.
    #[cfg(any(feature = "usart_v2", feature = "usart_v4"))]
    pub(crate) const fn divider(
        &self,
        kernel_clock: crate::time::Hertz,
        fractional_stop: bool,
    ) -> Result<u16, ConfigError> {
        if (self.invert_rx || self.invert_tx) && !crate::usart::INVERSION {
            return Err(ConfigError::Unsupported);
        }
        if !fractional_stop && matches!(self.stop_bits, StopBits::Half | StopBits::OneAndHalf) {
            return Err(ConfigError::Unsupported);
        }
        if self.baudrate == 0 {
            return Err(ConfigError::BaudRate);
        }
        let clock = kernel_clock.0 as u64;
        let baud = self.baudrate as u64;
        let brr = (clock + baud / 2) / baud;
        if brr < 16 || brr > 0xffff {
            return Err(ConfigError::BaudRate);
        }
        // Within 1 %: |clock / brr - baud| <= baud / 100.
        let error = clock.abs_diff(brr * baud);
        if error * 100 > brr * baud {
            return Err(ConfigError::BaudRate);
        }
        Ok(brr as u16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Parity {
    None,
    Even,
    Odd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum StopBits {
    Half,
    One,
    OneAndHalf,
    Two,
}

/// What the board file hands to `Uart::new`: one UART instance, its pins
/// and its DMA streams. The IP driver's trait bounds check that they belong
/// together; this struct only carries them.
pub struct Wiring<U, Tx, Rx, TxDma, RxDma> {
    pub uart: U,
    pub tx: Tx,
    pub rx: Rx,
    pub tx_dma: TxDma,
    pub rx_dma: RxDma,
}

/// What the board file hands to `UartRx::new` for a receive-only UART: the
/// instance, its RX pin and its receive DMA stream.
pub struct RxWiring<U, Rx, RxDma> {
    pub uart: U,
    pub rx: Rx,
    pub rx_dma: RxDma,
}

pub use crate::resource::{IrqError, Refused, Resource, Signal};

/// The interrupt vectors a UART needs bound to its `UartIrq`, all at one
/// priority above every task that uses the UART. From
/// `UartIrq::interrupts`.
#[cfg(any(feature = "usart_v2", feature = "usart_v4"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct UartInterrupts {
    pub line: crate::chip::Interrupt,
    pub rx_dma: crate::chip::Interrupt,
    /// `None` for a receive-only UART.
    pub tx_dma: Option<crate::chip::Interrupt>,
}

#[cfg(any(feature = "usart_v2", feature = "usart_v4"))]
impl UartInterrupts {
    /// Every vector, in order: line, receive, transmit.
    pub fn iter(&self) -> impl Iterator<Item = crate::chip::Interrupt> {
        [Some(self.line), Some(self.rx_dma), self.tx_dma]
            .into_iter()
            .flatten()
    }

    /// Checks the NVIC, reading only: every vector enabled, all at one
    /// priority. FerroHAL never sets NVIC state; this tells the firmware
    /// whether its scheduler set it as the driver needs. That the tasks
    /// using the UART run below this priority is for the firmware to keep.
    pub fn verify(&self) -> Result<(), IrqError> {
        crate::resource::verify_vectors(self.iter())
    }
}

/// The memory the driver's DMA uses, owned by the driver from `new` on.
pub struct Buffers {
    /// `write` copies into this and transmits from it. 1 to 65 535 bytes.
    pub tx: &'static mut [u8],
    /// The receive ring, filled continuously by DMA. An even length from 2
    /// to 65 534 bytes. `read` must keep up within one ring's worth of
    /// bytes, and so must the receive DMA interrupt, which must also never
    /// be preempted by the code that reads.
    pub rx: &'static mut [u8],
}

pub use crate::time::CycleClock;

/// One chunk of received bytes, as [`UartRx::read_chunk`] copies it out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Chunk {
    /// Bytes copied into the caller's buffer.
    pub len: usize,
    pub end: ChunkEnd,
    /// The receiver's generation once this chunk was taken. It advances at
    /// every gap, so two chunks of one generation have no loss between them.
    pub generation: u32,
}

/// Why a chunk ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ChunkEnd {
    /// The line went idle: the sender paused, typically at a frame's end.
    Idle,
    /// The caller's buffer filled before the line went idle.
    Full,
    /// Bytes were lost or damaged here, and the generation advanced. A line
    /// error ends the chunk it struck: that chunk may hold the damaged byte,
    /// so treat its content as suspect. Overruns and DMA errors end an
    /// empty chunk.
    Gap(Error),
}

/// How often each fault has struck since the UART was opened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Health {
    pub overrun: u32,
    pub framing: u32,
    pub parity: u32,
    pub noise: u32,
    pub dma_transfer: u32,
    pub dma_overrun: u32,
    /// `read` fell more than a ring behind.
    pub buffer_overrun: u32,
    /// Times the receive DMA was restarted after an error.
    pub restarts: u32,
    /// Idle or error marks the interrupt could not queue. A lost idle mark
    /// merges two chunks; a lost error mark is still reported, without its
    /// position.
    pub marks_lost: u32,
}

/// Why `UartTx::try_send` did not queue a frame. Nothing was queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SendError {
    /// No room for the whole frame now; retry once frames have gone.
    Full,
    /// Longer than the transmit buffer: it can never fit.
    TooLarge,
    /// A transmit DMA stream did not stop or start; transmission is off.
    Halted,
}

/// How the transmit half has fared since the UART was opened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct TxHealth {
    /// Frames that left the wire.
    pub sent: u32,
    /// Frames lost to a DMA error.
    pub dma_errors: u32,
    /// Times the queue did not drain within its bound and was dropped.
    pub timeouts: u32,
}

/// Why `Uart::new` or `set_config` refused a configuration. Apart from
/// `DmaBusy`, nothing has been written to hardware when this is returned.
/// `Uart::new` and `UartRx::new` return it in a [`Refused`], with what they
/// took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError {
    /// A DMA stream was left running by an earlier owner and did not stop.
    DmaBusy,
    /// The baud rate cannot be reached from this UART's clock within 1 %.
    BaudRate,
    /// The frame asks for something this IP version cannot do, such as
    /// inversion on `usart_v2`.
    Unsupported,
    /// The transmit buffer is empty or longer than one DMA transfer.
    TxBuffer,
    /// The receive ring is shorter than 2 bytes, odd, or longer than one
    /// DMA transfer.
    RxBuffer,
    /// A buffer lies in memory the DMA controller cannot reach, such as
    /// the H7's DTCM.
    BufferUnreachable,
    /// The data cache is on and a buffer is not in memory the MPU marks
    /// non-cacheable. The driver does no cache maintenance, so it refuses
    /// rather than read stale bytes.
    BufferCacheable,
}

/// A fault on the link. Each one marks a gap or corruption in the byte
/// stream; reception and transmission carry on after it unless it says
/// otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// The UART received a byte before the previous one was taken.
    Overrun,
    /// A stop bit was missing: wrong baud rate, a break, or a broken line.
    Framing,
    Parity,
    /// The UART sampled noise in a bit.
    Noise,
    /// `read` fell more than one ring behind; the unread bytes are gone.
    BufferOverrun,
    /// The DMA stream hit a bus error and stopped. The driver restarted
    /// reception; anything in flight is lost.
    DmaTransfer,
    /// The DMA stream could not keep up with the UART.
    DmaOverrun,
    /// A transfer did not finish within its counted bound. The driver
    /// stopped it.
    Timeout,
    /// A DMA stream did not stop when told to. The driver no longer uses
    /// that direction: it fails closed rather than reuse a running stream.
    Halted,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Overrun => "UART overrun",
            Self::Framing => "framing error",
            Self::Parity => "parity error",
            Self::Noise => "noise on the line",
            Self::BufferOverrun => "receive ring overrun",
            Self::DmaTransfer => "DMA transfer error",
            Self::DmaOverrun => "DMA overrun",
            Self::Timeout => "transfer timed out",
            Self::Halted => "DMA stream did not stop",
        })
    }
}

impl core::error::Error for Error {}

impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        use embedded_io::ErrorKind;
        match self {
            Self::Framing | Self::Parity | Self::Noise => ErrorKind::InvalidData,
            Self::Timeout => ErrorKind::TimedOut,
            Self::Overrun
            | Self::BufferOverrun
            | Self::DmaTransfer
            | Self::DmaOverrun
            | Self::Halted => ErrorKind::Other,
        }
    }
}
