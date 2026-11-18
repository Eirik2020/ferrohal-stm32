//! STM32F429 glue: the chip facts the IP modules leave out, for every package
//! of the line.
//!
//! The F429 is die 0x419, the F427's silicon with LTDC enabled (FerroHAL
//! drives neither LTDC nor the FMC). The routing facts are generated from
//! stm32-metapac's metadata into `gen/<part>.rs` by `tools/glue-gen`, one
//! file per package; the die's hand facts are `f4/die_0x419.rs`, which
//! the F427 shares.
//!
//! No board carries it on the rig: the line is unproven.
//!
//! Requirements: GLUE-001, GLUE-002

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{SclPin, SdaPin},
    rcc_f4::{Bus, Limits, Overdrive, RCC, RccBit},
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

#[path = "die_0x419.rs"]
#[macro_use]
mod die;
pub use die::*;

// The routing rows, generated from stm32-metapac's metadata for the
// part's device and package; a flash-size variant takes its package's.
#[cfg(any(
    feature = "stm32f429vi",
    feature = "stm32f429vg",
    feature = "stm32f429ve"
))]
#[path = "../gen/stm32f429vi.rs"]
mod generated;
#[cfg(any(
    feature = "stm32f429zi",
    feature = "stm32f429zg",
    feature = "stm32f429ze"
))]
#[path = "../gen/stm32f429zi.rs"]
mod generated;
#[cfg(any(
    feature = "stm32f429ii",
    feature = "stm32f429ig",
    feature = "stm32f429ie"
))]
#[path = "../gen/stm32f429ii.rs"]
mod generated;
#[cfg(any(feature = "stm32f429ai", feature = "stm32f429ag"))]
#[path = "../gen/stm32f429ai.rs"]
mod generated;
#[cfg(any(
    feature = "stm32f429bi",
    feature = "stm32f429bg",
    feature = "stm32f429be"
))]
#[path = "../gen/stm32f429bi.rs"]
mod generated;
#[cfg(any(
    feature = "stm32f429ni",
    feature = "stm32f429ng",
    feature = "stm32f429ne"
))]
#[path = "../gen/stm32f429ni.rs"]
mod generated;
pub use generated::*;
