//! The reset and clock controller, independent of its IP version: what
//! every RCC version shares (the kernel clock and the oscillator, prescaler and
//! enable-bit types), then the chip's own version's items, re-exported here
//! so a driver or a board names `rcc::Config`, `rcc::Rcc` and the rest on
//! every chip. Each version (`rcc_f4`, `rcc_h7_rm0433`) keeps its clock
//! tree, its register encodings and its chip limits.
//!
//! Requirements: RCC-001

pub(crate) mod kernel;
#[macro_use]
pub(crate) mod shared;

#[cfg(feature = "rcc_f4")]
pub use crate::rcc_f4::*;
#[cfg(feature = "rcc_h7_rm0433")]
pub use crate::rcc_h7_rm0433::*;
