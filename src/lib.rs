//! FerroHAL: a scheduler-agnostic STM32 HAL on stm32-metapac.
//!
//! FerroHAL drives peripherals; the scheduler owns execution. It never
//! defines an interrupt handler, sets an NVIC priority or enables a vector:
//! drivers enable their peripheral's own interrupt bits and expose service
//! methods the scheduler's handlers call. There is no async here and no
//! time source; every wait is counted.
//!
//! # Layout
//!
//! - One module per metapac IP version (`usart_v2`, `usart_v4`, `spi_v2`,
//!   `spi_v4`, `i2c_v1`, `i2c_v2`, `dma_v1`, `dma_v2`, `exti_v1`,
//!   `exti_h7`, `timer_v1`, `timer_v3`, `iwdg_v1`, `iwdg_v2`, `gpio_v2`,
//!   `rcc_f4`, `rcc_h7rm0433`), each behind its own feature. They speak
//!   registers only.
//! - [`chip`]: the glue for the chip feature that is on. It holds the chip
//!   facts (RCC bits, pin alternate functions, DMA request routing, memory
//!   map) and the svd2rust-style [`chip::Peripherals`].
//! - Neutral names: [`uart`], [`spi`], [`i2c`] and `usb`, and `exti`,
//!   `timer`, `iwdg`, `rcc`, `gpio`, `dma` for whichever IP version the
//!   chip carries.
//! - [`device`] (feature `rt`): the module RTIC's `device = ...` names.
//!
//! # Ownership
//!
//! `Peripherals::take()` hands out each token once. Shared registers (RCC
//! enable and reset bits, a GPIO port's mode registers) are written only
//! through their single owner (`&mut Rcc`, `&mut gpio::Ctrl`), so they
//! need no critical section.

#![no_std]

#[cfg(not(any(
    feature = "stm32f401re",
    feature = "stm32f405rg",
    feature = "stm32h743vi",
    feature = "stm32h753zi"
)))]
compile_error!("enable one chip feature, such as `stm32f401re` or `stm32h753zi`");
#[cfg(any(
    all(feature = "stm32f401re", feature = "stm32f405rg"),
    all(feature = "stm32f401re", feature = "stm32h743vi"),
    all(feature = "stm32f401re", feature = "stm32h753zi"),
    all(feature = "stm32f405rg", feature = "stm32h743vi"),
    all(feature = "stm32f405rg", feature = "stm32h753zi"),
    all(feature = "stm32h743vi", feature = "stm32h753zi"),
))]
compile_error!("enable only one chip feature");

pub mod adc;
pub mod i2c;
pub mod reset;
pub mod resource;
pub mod signature;
pub mod spi;
pub mod time;
pub mod uart;
#[cfg(feature = "usb")]
pub mod usb;
mod wait;

// EXTI: one source, two IP versions (see the module).
#[cfg(feature = "exti_h7")]
#[path = "exti.rs"]
pub mod exti_h7;
#[cfg(feature = "exti_v1")]
#[path = "exti.rs"]
pub mod exti_v1;

// The independent watchdog: one source, two IP versions (see the module).
#[cfg(feature = "iwdg_v1")]
#[path = "iwdg.rs"]
pub mod iwdg_v1;
#[cfg(feature = "iwdg_v2")]
#[path = "iwdg.rs"]
pub mod iwdg_v2;

// Timers: one source, two IP versions (see the module).
#[cfg(feature = "timer_v1")]
#[path = "timer.rs"]
pub mod timer_v1;
#[cfg(feature = "timer_v3")]
#[path = "timer.rs"]
pub mod timer_v3;

// The stream DMA: one source, two IP versions (see the module).
#[cfg(feature = "adc_v2")]
pub mod adc_v2;
#[cfg(feature = "adc_v4")]
pub mod adc_v4;
#[cfg(feature = "dma_v1")]
#[path = "dma_stream.rs"]
pub mod dma_v1;
#[cfg(feature = "dma_v2")]
#[path = "dma_stream.rs"]
pub mod dma_v2;
#[cfg(feature = "gpio_v2")]
pub mod gpio_v2;
#[cfg(feature = "i2c_v1")]
pub mod i2c_v1;
#[cfg(feature = "i2c_v2")]
pub mod i2c_v2;
#[cfg(feature = "rcc_f4")]
pub mod rcc_f4;
#[cfg(feature = "rcc_h7rm0433")]
pub mod rcc_h7rm0433;
#[cfg(feature = "sdmmc_v2")]
pub mod sdmmc_v2;
#[cfg(feature = "spi_v2")]
pub mod spi_v2;
#[cfg(feature = "spi_v4")]
pub mod spi_v4;
#[cfg(feature = "usart_v2")]
pub mod usart_v2;
#[cfg(feature = "usart_v4")]
pub mod usart_v4;

#[cfg(feature = "adc_v2")]
use adc_v2 as adc_ip;
#[cfg(feature = "adc_v4")]
use adc_v4 as adc_ip;
#[cfg(feature = "dma_v1")]
pub use dma_v1 as dma;
#[cfg(feature = "dma_v2")]
pub use dma_v2 as dma;
#[cfg(feature = "exti_h7")]
pub use exti_h7 as exti;
#[cfg(feature = "exti_v1")]
pub use exti_v1 as exti;
#[cfg(feature = "gpio_v2")]
pub use gpio_v2 as gpio;
#[cfg(feature = "i2c_v1")]
use i2c_v1 as i2c_ip;
#[cfg(feature = "i2c_v2")]
use i2c_v2 as i2c_ip;
#[cfg(feature = "iwdg_v1")]
pub use iwdg_v1 as iwdg;
#[cfg(feature = "iwdg_v2")]
pub use iwdg_v2 as iwdg;
#[cfg(feature = "rcc_f4")]
pub use rcc_f4 as rcc;
#[cfg(feature = "rcc_h7rm0433")]
pub use rcc_h7rm0433 as rcc;
#[cfg(feature = "sdmmc_v2")]
pub use sdmmc_v2 as sdmmc;
#[cfg(feature = "spi_v2")]
use spi_v2 as spi_ip;
#[cfg(feature = "spi_v4")]
use spi_v4 as spi_ip;
#[cfg(feature = "timer_v1")]
pub use timer_v1 as timer;
#[cfg(feature = "timer_v3")]
pub use timer_v3 as timer;
#[cfg(feature = "usart_v2")]
use usart_v2 as usart;
#[cfg(feature = "usart_v4")]
use usart_v4 as usart;

#[cfg(feature = "stm32f401re")]
#[path = "chips/stm32f401re.rs"]
pub mod chip;
#[cfg(feature = "stm32f405rg")]
#[path = "chips/stm32f405rg.rs"]
pub mod chip;
#[cfg(feature = "stm32h743vi")]
#[path = "chips/stm32h743vi.rs"]
pub mod chip;
#[cfg(feature = "stm32h753zi")]
#[path = "chips/stm32h753zi.rs"]
pub mod chip;

#[cfg(feature = "rt")]
pub mod device;

mod sealed {
    pub trait Sealed {}
}
pub(crate) use sealed::Sealed;
