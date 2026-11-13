//! The SPI master every chip offers, independent of the hardware underneath.
//!
//! This module is chip-neutral. It defines what an SPI driver takes and
//! reports; each IP version (`spi_v2` on F4, `spi_v4` on H7) supplies the
//! register operations under one driver of the same shape:
//!
//! - **init**: `Spi::new(&mut rcc, wiring, &config, buffers)`. The
//!   [`Wiring`] comes from the board file and names the instance, its three
//!   pins and two DMA streams; the routing traits reject a wrong
//!   combination at compile time. Chip select is not the driver's: it is a
//!   GPIO output the firmware drives around each transfer. A refusal is a
//!   [`Refused`]: the reason, with the wiring and buffers handed back
//!   (`BlockingSpi::new`: the wiring).
//! - **transfer**: `start(tx)` begins one full-duplex DMA transfer and
//!   returns; `on_interrupt()` (or `poll()`) reports its completion, after
//!   which `received()` holds the bytes clocked in. `abort()` recovers a
//!   transfer that never finishes.
//! - **blocking**: `embedded_hal::spi::SpiBus`, each operation bounded.
//! - **without DMA**: [`BlockingSpi`], a polled `SpiBus` for a bus whose
//!   DMA routes are taken, from a [`BlockingWiring`] of the instance and
//!   its pins alone.
//! - **interrupts**: the receive stream's vector only. FerroHAL never
//!   touches the NVIC.
//!
//! Requirements: SPI-001, SPI-002, SPI-003, SPI-004, SPI-005, SPI-007, SPI-009

#[cfg(any(feature = "spi_v2", feature = "spi_v4"))]
mod instance;
mod polled;
#[cfg(any(feature = "dma_v1", feature = "dma_v2"))]
pub(crate) mod stream_dma;

#[cfg(any(feature = "spi_v2", feature = "spi_v4"))]
pub use instance::{Info, Instance, MisoPin, MosiPin, SckPin, SpiParts, SpiRxDma, SpiTxDma, check};
#[cfg(any(feature = "spi_v2", feature = "spi_v4"))]
pub use polled::BlockingSpi;
#[cfg(any(feature = "dma_v1", feature = "dma_v2"))]
pub use stream_dma::Spi;

pub use embedded_hal::spi::{MODE_0, MODE_1, MODE_2, MODE_3, Mode, Phase, Polarity};

pub use crate::resource::{IrqError, Refused, Resource, Signal};
pub use crate::time::CycleClock;
use crate::time::Hertz;

/// Which bit of each byte goes first on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BitOrder {
    MsbFirst,
    LsbFirst,
}

/// The bus settings. Frames are 8 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// The fastest SCK allowed. The driver picks the fastest prescaler
    /// (the kernel clock over 2, 4, ... 256) at or below it; `Spi::new` and
    /// `check` report the result.
    pub frequency: Hertz,
    pub mode: Mode,
    pub bit_order: BitOrder,
}

#[cfg(feature = "defmt")]
impl defmt::Format for Config {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(
            f,
            "Config {{ frequency: {}, cpol: {}, cpha: {}, bit_order: {} }}",
            self.frequency,
            self.mode.polarity == Polarity::IdleHigh,
            self.mode.phase == Phase::CaptureOnSecondTransition,
            self.bit_order
        )
    }
}

impl Config {
    /// Mode 0, most significant bit first.
    pub const fn new(frequency: Hertz) -> Self {
        Self {
            frequency,
            mode: MODE_0,
            bit_order: BitOrder::MsbFirst,
        }
    }

    pub const fn with_mode(self, mode: Mode) -> Self {
        Self { mode, ..self }
    }

    pub const fn with_bit_order(self, bit_order: BitOrder) -> Self {
        Self { bit_order, ..self }
    }

    /// The SCK an SPI clocked at `kernel_clock` would run this at, or why
    /// it can't. A `const fn`, so a board can check its buses while
    /// compiling; `Spi::check` finds the kernel clock for an instance.
    pub const fn check(&self, kernel_clock: Hertz) -> Result<Hertz, ConfigError> {
        match self.divider(kernel_clock) {
            Ok(code) => Ok(Hertz(kernel_clock.0 >> (code + 1))),
            Err(error) => Err(error),
        }
    }

    /// The prescaler code both IP versions use (BR, MBR): SCK is the kernel
    /// clock over 2^(code + 1).
    pub(crate) const fn divider(&self, kernel_clock: Hertz) -> Result<u8, ConfigError> {
        if self.frequency.0 == 0 {
            return Err(ConfigError::Frequency);
        }
        let mut code = 0;
        while code < 8 {
            if kernel_clock.0 >> (code + 1) <= self.frequency.0 {
                return Ok(code);
            }
            code += 1;
        }
        Err(ConfigError::Frequency)
    }
}

/// Where an SPI instance's kernel clock comes from. Chip facts: the glue
/// sets it per instance, and `freeze` sets the muxes to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Kernel {
    /// The clock of the bus the SPI sits on.
    Bus(crate::rcc::Bus),
    /// PLL1's Q output (the H7's SPI1 to SPI3, SPI123SEL at reset).
    #[cfg(feature = "rcc_h7_rm0433")]
    Pll1Q,
}

impl Kernel {
    /// The frequency, or `None` if its source isn't running.
    pub const fn frequency(self, clocks: &crate::rcc::Clocks) -> Option<Hertz> {
        match self {
            Self::Bus(bus) => Some(clocks.bus(bus)),
            #[cfg(feature = "rcc_h7_rm0433")]
            Self::Pll1Q => clocks.pll1_q,
        }
    }
}

/// What the board file hands to `Spi::new`: one SPI instance, its three
/// pins and its two DMA streams. The driver's trait bounds check that they
/// belong together; this struct only carries them.
pub struct Wiring<U, Sck, Miso, Mosi, TxDma, RxDma> {
    pub spi: U,
    pub sck: Sck,
    pub miso: Miso,
    pub mosi: Mosi,
    pub tx_dma: TxDma,
    pub rx_dma: RxDma,
}

/// What the board file hands to `BlockingSpi::new`: one SPI instance and
/// its three pins, no DMA streams.
pub struct BlockingWiring<U, Sck, Miso, Mosi> {
    pub spi: U,
    pub sck: Sck,
    pub miso: Miso,
    pub mosi: Mosi,
}

/// The memory the driver's DMA uses, owned by the driver from `new` on.
/// A transfer is at most as long as the shorter of the two.
pub struct Buffers {
    /// `start` copies the bytes to send into this.
    pub tx: &'static mut [u8],
    /// The bytes clocked in land here; `received` lends them out.
    pub rx: &'static mut [u8],
}

/// The interrupt vector an SPI needs bound, from `Spi::interrupts`: its
/// receive stream's, whose handler calls `Spi::on_interrupt`.
#[cfg(any(feature = "spi_v2", feature = "spi_v4"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SpiInterrupts {
    pub rx_dma: crate::chip::Interrupt,
}

#[cfg(any(feature = "spi_v2", feature = "spi_v4"))]
impl SpiInterrupts {
    pub fn iter(&self) -> impl Iterator<Item = crate::chip::Interrupt> {
        [self.rx_dma].into_iter()
    }

    /// Checks the NVIC, reading only: the vector is enabled. That the tasks
    /// sharing the `Spi` with its handler lock it is for the firmware to
    /// keep.
    pub fn verify(&self) -> Result<(), IrqError> {
        crate::resource::verify_vectors(self.iter())
    }
}

/// Why `Spi::new`, `set_config` or `check` refused. `Spi::new` returns it
/// in a [`Refused`], with what it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError {
    /// Even the kernel clock over 256 is faster than `frequency`, or
    /// `frequency` is 0.
    Frequency,
    /// The instance's kernel clock source isn't running (on the H7, SPI1
    /// to SPI3 need PLL1's Q output).
    KernelClock,
    /// A transfer is running; settings change only between transfers.
    Busy,
    /// A DMA stream was left running by an earlier owner and did not stop.
    DmaBusy,
    /// The transmit buffer is empty or longer than one DMA transfer.
    TxBuffer,
    /// The receive buffer is empty or longer than one DMA transfer.
    RxBuffer,
    /// A buffer cannot serve DMA: out of the controller's reach, or cacheable.
    Buffer(crate::dma::BufferError),
}

/// Why a transfer failed or was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// A transfer is already running.
    Busy,
    /// Nothing to send.
    Empty,
    /// Longer than the driver's buffers.
    TooLarge,
    /// A DMA stream reported a transfer or direct-mode error. The transfer
    /// was stopped and the bus reset; the next one can start.
    Dma,
    /// A received byte was overwritten before DMA took it.
    Overrun,
    /// Another master drove NSS low (`MODF`); the SPI dropped out of master
    /// mode and was put back.
    ModeFault,
    /// The transfer didn't finish within its bound. It was stopped and the
    /// bus reset.
    Timeout,
}

impl embedded_hal::spi::Error for Error {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        use embedded_hal::spi::ErrorKind;
        match self {
            Self::Overrun => ErrorKind::Overrun,
            Self::ModeFault => ErrorKind::ModeFault,
            _ => ErrorKind::Other,
        }
    }
}

/// Counts since `new`, for health monitoring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Health {
    /// Transfers that completed.
    pub transfers: u32,
    pub dma_errors: u32,
    pub overruns: u32,
    pub mode_faults: u32,
    /// Blocking transfers that ran past their bound.
    pub timeouts: u32,
    /// Running transfers stopped by `abort` or an error.
    pub aborts: u32,
}

impl From<crate::dma::BufferError> for ConfigError {
    fn from(error: crate::dma::BufferError) -> Self {
        Self::Buffer(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divider_picks_the_fastest_sck_at_or_below() {
        let kernel = Hertz::mhz(84);
        assert_eq!(
            Config::new(Hertz::mhz(42)).check(kernel),
            Ok(Hertz::mhz(42))
        );
        assert_eq!(
            Config::new(Hertz::mhz(30)).check(kernel),
            Ok(Hertz::mhz(21))
        );
        assert_eq!(
            Config::new(Hertz::mhz(100)).check(kernel),
            Ok(Hertz::mhz(42))
        );
        assert_eq!(
            Config::new(Hertz::khz(400)).check(kernel),
            Ok(Hertz(328_125))
        );
        assert_eq!(
            Config::new(Hertz::khz(300)).check(kernel),
            Err(ConfigError::Frequency)
        );
        assert_eq!(
            Config::new(Hertz(0)).check(kernel),
            Err(ConfigError::Frequency)
        );
        // The Lucid's IMU: at most 24 MHz from PLL1Q at 100 MHz.
        assert_eq!(
            Config::new(Hertz::mhz(24)).check(Hertz::mhz(100)),
            Ok(Hertz(12_500_000))
        );
    }
}
