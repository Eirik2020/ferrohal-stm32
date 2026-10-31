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
//! - One module per metapac IP version (`usart_v2`, `usart_v4`, `dma_v1`,
//!   `dma_v2`, `gpio_v2`, `rcc_f4`, `rcc_h7rm0433`), each behind its own
//!   feature. They speak registers only.
//! - [`chip`]: the glue for the chip feature that is on. It holds the chip
//!   facts (RCC bits, pin alternate functions, DMA request routing, memory
//!   map) and the svd2rust-style [`chip::Peripherals`].
//! - Neutral names: [`uart`], and `rcc`, `gpio`, `dma` for whichever IP
//!   version the chip carries.
//! - [`device`] (feature `rt`): the module RTIC's `device = ...` names.
//!
//! # Ownership
//!
//! `Peripherals::take()` hands out each token once. Shared registers (RCC
//! enable and reset bits, a GPIO port's mode registers) are written only
//! through their single owner (`&mut Rcc`, `&mut gpio::Ctrl`), so they
//! need no critical section.

#![no_std]

#[cfg(not(any(feature = "stm32f401re", feature = "stm32h753zi")))]
compile_error!("enable one chip feature, such as `stm32f401re` or `stm32h753zi`");
#[cfg(all(feature = "stm32f401re", feature = "stm32h753zi"))]
compile_error!("enable only one chip feature");

pub mod time;
pub mod uart;

// The stream DMA: one source, two IP versions (see the module).
#[cfg(feature = "dma_v1")]
#[path = "dma_stream.rs"]
pub mod dma_v1;
#[cfg(feature = "dma_v2")]
#[path = "dma_stream.rs"]
pub mod dma_v2;
#[cfg(feature = "gpio_v2")]
pub mod gpio_v2;
#[cfg(feature = "rcc_f4")]
pub mod rcc_f4;
#[cfg(feature = "rcc_h7rm0433")]
pub mod rcc_h7rm0433;
#[cfg(feature = "usart_v2")]
pub mod usart_v2;
#[cfg(feature = "usart_v4")]
pub mod usart_v4;

#[cfg(feature = "dma_v1")]
pub use dma_v1 as dma;
#[cfg(feature = "dma_v2")]
pub use dma_v2 as dma;
#[cfg(feature = "gpio_v2")]
pub use gpio_v2 as gpio;
#[cfg(feature = "rcc_f4")]
pub use rcc_f4 as rcc;
#[cfg(feature = "rcc_h7rm0433")]
pub use rcc_h7rm0433 as rcc;
#[cfg(feature = "usart_v2")]
use usart_v2 as usart;
#[cfg(feature = "usart_v4")]
use usart_v4 as usart;

#[cfg(feature = "stm32f401re")]
#[path = "chips/stm32f401re.rs"]
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
