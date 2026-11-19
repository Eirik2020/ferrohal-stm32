//! STM32F407 glue: the chip facts the IP modules leave out, for every package
//! of the line.
//!
//! The F407 is die 0x413, the F405's silicon, with Ethernet, the camera
//! interface and FSMC enabled (none of which FerroHAL drives). The routing
//! facts are generated from stm32-metapac's metadata into `gen/<part>.rs`
//! by `tools/glue-gen`, one file per package; the die's hand
//! facts are `f4/die_0x413.rs`, which the F405 shares.
//!
//! The STM32F4 Discovery (MB997) carries this chip: die 0x413's debugger
//! bench.
//!
//! Requirements: GLUE-001, GLUE-002

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{I2cRxDma, I2cTxDma, SclPin, SdaPin},
    rcc_f4::{Bus, Limits, RCC, RccBit},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    time::Hertz,
    uart::{Info, RxDma, RxPin, TxDma, TxPin},
    usart_v2::Unit,
};

#[path = "../macros.rs"]
#[macro_use]
mod macros;

#[path = "macros.rs"]
#[macro_use]
mod family;

#[path = "die_0x413.rs"]
#[macro_use]
mod die;
pub use die::*;

// The routing rows, generated from stm32-metapac's metadata for the
// part's device and package; a flash-size variant takes its package's.
#[cfg(any(feature = "stm32f407vg", feature = "stm32f407ve"))]
#[path = "../gen/stm32f407vg.rs"]
mod generated;
#[cfg(any(feature = "stm32f407zg", feature = "stm32f407ze"))]
#[path = "../gen/stm32f407zg.rs"]
mod generated;
#[cfg(any(feature = "stm32f407ig", feature = "stm32f407ie"))]
#[path = "../gen/stm32f407ig.rs"]
mod generated;
pub use generated::*;
