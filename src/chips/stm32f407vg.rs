//! STM32F407VG glue: the chip facts the IP modules leave out.
//!
//! The F407 is die 0x413, the F405's silicon, with Ethernet, the camera
//! interface and FSMC enabled (none of which FerroHAL drives) and bonded in
//! the LQFP100. The routing facts are generated from stm32-metapac's
//! metadata into `gen/stm32f407vg.rs` by `tools/glue-gen`; the die's hand
//! facts are `shared/die_0x413.rs`, which the F405 shares.
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
    dma_v2::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{SclPin, SdaPin},
    rcc_f4::{Bus, Limits, RCC, RccBit},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    time::Hertz,
    uart::{Info, RxDma, RxPin, TxDma, TxPin},
    usart_v2::LineRegs,
};

#[path = "shared/macros.rs"]
#[macro_use]
mod macros;

#[path = "shared/f4_macros.rs"]
#[macro_use]
mod family;

#[path = "shared/die_0x413.rs"]
#[macro_use]
mod die;
pub use die::*;

// The routing rows, generated from stm32-metapac's metadata.
#[path = "gen/stm32f407vg.rs"]
mod generated;
pub use generated::*;
