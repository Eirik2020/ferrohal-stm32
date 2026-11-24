//! What the SPI drivers share: the instance and its facts, the routing
//! traits the glue implements, and the parts a handle keeps. Both drivers,
//! [`Spi`](super::Spi) over DMA and [`BlockingSpi`](super::BlockingSpi)
//! polled, take their bounds from here (API review D4).
//!
//! Requirements: SPI-001, SPI-005, SPI-011

use crate::{
    dma::{ChannelInstance, ChannelInterrupt},
    rcc::Kernel,
    rcc::{Clocks, RccBit},
    spi::{BlockingWiring, Config, ConfigError, Wiring},
    spi_ip::Unit,
    time::Hertz,
};

/// Per-instance facts. Chip facts: the glue layer holds one static `Info`
/// per SPI.
pub type Info = crate::resource::Info<Unit>;

// SAFETY: a `Unit` is an address.
unsafe impl Sync for Info {}

/// An SPI instance this chip has. Chip facts: the glue layer implements it.
pub trait Instance: crate::Sealed + 'static {
    /// Where its kernel clock comes from.
    const KERNEL: Kernel;
    /// Its name in the reference manual.
    const NAME: &'static str;
    /// Its RCC enable (and reset) bit.
    const ENABLE: RccBit;
    #[doc(hidden)]
    fn info() -> &'static Info;
}

/// A pin, in the right alternate function, that carries `U`'s SCK.
pub trait SckPin<U: Instance>: crate::Sealed {}
/// A pin, in the right alternate function, that carries `U`'s MISO.
pub trait MisoPin<U: Instance>: crate::Sealed {}
/// A pin, in the right alternate function, that carries `U`'s MOSI.
pub trait MosiPin<U: Instance>: crate::Sealed {}

/// A DMA channel that serves `U`'s transmit requests.
///
/// # Safety
///
/// `REQUEST` must be the request that routes `U`'s TX to this channel.
pub unsafe trait SpiTxDma<U: Instance>: ChannelInstance + ChannelInterrupt {
    const REQUEST: u8;
}

/// A DMA channel that serves `U`'s receive requests.
///
/// # Safety
///
/// As [`SpiTxDma`], for RX.
pub unsafe trait SpiRxDma<U: Instance>: ChannelInstance + ChannelInterrupt {
    const REQUEST: u8;
}

/// What an SPI was opened from: the instance its handle names, for its name
/// and kernel clock. Implemented by [`Wiring`] and [`BlockingWiring`] only,
/// so a handle's type names the board's whole route.
pub trait Parts: crate::Sealed {
    type Instance: Instance;
}

impl<U, Sck, Miso, Mosi, TxD, RxD> crate::Sealed for Wiring<U, Sck, Miso, Mosi, TxD, RxD> {}
impl<U: Instance, Sck, Miso, Mosi, TxD, RxD> Parts for Wiring<U, Sck, Miso, Mosi, TxD, RxD> {
    type Instance = U;
}

impl<U, Sck, Miso, Mosi> crate::Sealed for BlockingWiring<U, Sck, Miso, Mosi> {}
impl<U: Instance, Sck, Miso, Mosi> Parts for BlockingWiring<U, Sck, Miso, Mosi> {
    type Instance = U;
}

/// The SCK side both SPI engines keep (API review U6): the kernel clock it is
/// divided from, the SCK in use, and the CPU cycles one SCK period takes.
#[derive(Clone, Copy)]
pub(crate) struct SckClock {
    kernel_clock: Hertz,
    cpu: Hertz,
    frequency: Hertz,
    /// CPU cycles per SCK period, rounded up.
    cycles: u32,
}

impl SckClock {
    /// Configures `unit` for `config` from `kernel_clock`, or refuses.
    pub(crate) fn configure(
        unit: Unit,
        kernel_clock: Hertz,
        cpu: Hertz,
        config: &Config,
    ) -> Result<Self, ConfigError> {
        let code = config.divider(kernel_clock)?;
        unit.configure(code, config);
        let frequency = Hertz(kernel_clock.0 >> (code + 1));
        Ok(Self {
            kernel_clock,
            cpu,
            frequency,
            cycles: cpu.0.div_ceil(frequency.0.max(1)),
        })
    }

    /// The same kernel clock, reconfigured for `config`.
    pub(crate) fn reconfigure(
        &mut self,
        unit: Unit,
        config: &Config,
    ) -> Result<Hertz, ConfigError> {
        *self = Self::configure(unit, self.kernel_clock, self.cpu, config)?;
        Ok(self.frequency)
    }

    pub(crate) fn frequency(&self) -> Hertz {
        self.frequency
    }

    pub(crate) fn kernel_clock(&self) -> Hertz {
        self.kernel_clock
    }

    /// CPU cycles per SCK period, rounded up.
    pub(crate) fn cycles(&self) -> u32 {
        self.cycles
    }

    /// ES0392 2.22.6: disabling right after EOT can cut the last SCK pulse
    /// short at a slow SCK, so an IP version that needs it holds one SCK
    /// period first.
    pub(crate) fn hold_after_end(&self) {
        if Unit::HOLD_AFTER_END {
            cortex_m::asm::delay(self.cycles);
        }
    }
}

/// The SCK `config` would give on instance `U` under `clocks`. A
/// `const fn`, so a board can assert its buses while compiling:
/// `const _: () = assert!(spi::check::<SPI1>(&CLOCKS, &BUS).is_ok());`.
pub const fn check<U: Instance>(clocks: &Clocks, config: &Config) -> Result<Hertz, ConfigError> {
    match U::KERNEL.frequency(clocks) {
        Some(kernel_clock) => config.check(kernel_clock),
        None => Err(ConfigError::KernelClock),
    }
}
