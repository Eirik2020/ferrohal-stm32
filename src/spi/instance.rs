//! What the SPI drivers share: the instance and its facts, the routing
//! traits the glue implements, and the parts a handle keeps. Both drivers,
//! [`Spi`](super::Spi) over DMA and [`BlockingSpi`](super::BlockingSpi)
//! polled, take their bounds from here (API review D4).

use crate::{
    dma::{StreamInstance, StreamInterrupt},
    rcc::Kernel,
    rcc::{Clocks, RccBit},
    spi::{BlockingWiring, Config, ConfigError, Wiring},
    spi_ip::Unit,
    time::Hertz,
};

/// Per-instance facts. Chip facts: the glue layer holds one static `Info`
/// per SPI.
pub struct Info {
    unit: Unit,
}

// SAFETY: `unit` is an address.
unsafe impl Sync for Info {}

impl Info {
    pub(crate) const fn new(unit: Unit) -> Self {
        Self { unit }
    }

    pub(crate) fn unit(&self) -> Unit {
        self.unit
    }
}

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

/// What an SPI was opened from: the instance its handle names, for its name
/// and kernel clock. Implemented by [`Wiring`] and [`BlockingWiring`] only,
/// so a handle's type names the board's whole route.
pub trait SpiParts: crate::Sealed {
    type Instance: Instance;
}

impl<U, Sck, Miso, Mosi, TxD, RxD> crate::Sealed for Wiring<U, Sck, Miso, Mosi, TxD, RxD> {}
impl<U: Instance, Sck, Miso, Mosi, TxD, RxD> SpiParts for Wiring<U, Sck, Miso, Mosi, TxD, RxD> {
    type Instance = U;
}

impl<U, Sck, Miso, Mosi> crate::Sealed for BlockingWiring<U, Sck, Miso, Mosi> {}
impl<U: Instance, Sck, Miso, Mosi> SpiParts for BlockingWiring<U, Sck, Miso, Mosi> {
    type Instance = U;
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
