//! The UART every chip offers, independent of the hardware underneath.
//!
//! This module is chip-neutral. It defines what a UART driver takes and
//! reports; each IP version (`usart_v2` on F4, `usart_v4` on H7, later
//! others) supplies a driver with the same shape:
//!
//! - **init**: `Uart::new(&mut rcc, wiring, &config, buffers)`. The
//!   [`Wiring`] comes from the board file and names the instance, pins and
//!   DMA streams; the IP's routing traits reject a wrong combination at
//!   compile time.
//! - **read and write**: `embedded_io::{Read, Write, ReadReady, WriteReady}`
//!   on the driver and on its two halves.
//! - **interrupts**: a `UartIrq` handle whose `on_interrupt()` the
//!   scheduler's handlers call. FerroHAL never touches the NVIC.
//!
//! The driver for the enabled chip is re-exported here, so firmware names
//! `ferrohal_stm32::uart::Uart` whatever the IP version.

use core::fmt;

#[cfg(any(feature = "dma_v1", feature = "dma_v2"))]
pub(crate) mod stream_dma;

#[cfg(any(feature = "usart_v2", feature = "usart_v4"))]
pub use crate::usart::{Uart, UartIrq, UartRx, UartTx};

/// Frame settings. Data is always 8 bits; with parity on, the parity bit
/// is a ninth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    pub baudrate: u32,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

impl Config {
    /// `baudrate`, 8N1.
    pub const fn new(baudrate: u32) -> Self {
        Self {
            baudrate,
            parity: Parity::None,
            stop_bits: StopBits::One,
        }
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

/// Why `Uart::new` or `set_config` refused a configuration. Apart from
/// `DmaBusy`, nothing has been written to hardware when this is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError {
    /// A DMA stream was left running by an earlier owner and did not stop.
    DmaBusy,
    /// The baud rate cannot be reached from this UART's clock within 1 %.
    BaudRate,
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
