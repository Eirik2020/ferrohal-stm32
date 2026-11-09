//! The svd2rust-shaped device module, for RTIC's `device = ...`.
//!
//! RTIC needs the `Interrupt` enum (also as `interrupt`), `NVIC_PRIO_BITS`
//! and `Peripherals::steal()`; the last exists only with FerroHAL's `rtic`
//! feature (safety policy, exception 1). The vector table comes from
//! stm32-metapac's `rt` feature, which `rtic` turns on. With
//! `peripherals = true`, `init`'s `cx.device` is FerroHAL's [`Peripherals`].

pub use stm32_metapac::{Interrupt, NVIC_PRIO_BITS, interrupt};

pub use crate::chip::Peripherals;
