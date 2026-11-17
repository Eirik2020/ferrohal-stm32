//! What both RCC modules (`rcc_f4`, `rcc_h7_rm0433`) share word for word:
//! the oscillator and prescaler choices, a peripheral's enable and reset
//! bit, and the enable and reset writes over a family's bus registers
//! ([`bus_registers!`]). Each RCC module re-exports the types as
//! `crate::rcc::*` and keeps the register encodings, which metapac types
//! per family. API review U2, S7 and U9 (2026-11-17).
//!
//! Requirements: RCC-001

use crate::{rcc::Bus, time::Hertz};

/// The high-speed external clock: its frequency and how it is driven.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Hse {
    pub freq: Hertz,
    pub mode: HseMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum HseMode {
    /// A crystal or resonator on OSC_IN and OSC_OUT.
    Oscillator,
    /// An external clock on OSC_IN, such as an ST-LINK's MCO.
    Bypass,
}

/// The PLL's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PllSource {
    Hsi,
    Hse,
}

/// An AHB (or, on the H7, core) clock divider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AhbPrescaler {
    Div1,
    Div2,
    Div4,
    Div8,
    Div16,
    Div64,
    Div128,
    Div256,
    Div512,
}

impl AhbPrescaler {
    pub(crate) const fn divisor(self) -> u32 {
        match self {
            Self::Div1 => 1,
            Self::Div2 => 2,
            Self::Div4 => 4,
            Self::Div8 => 8,
            Self::Div16 => 16,
            Self::Div64 => 64,
            Self::Div128 => 128,
            Self::Div256 => 256,
            Self::Div512 => 512,
        }
    }
}

/// An APB clock divider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ApbPrescaler {
    Div1,
    Div2,
    Div4,
    Div8,
    Div16,
}

impl ApbPrescaler {
    pub(crate) const fn divisor(self) -> u32 {
        match self {
            Self::Div1 => 1,
            Self::Div2 => 2,
            Self::Div4 => 4,
            Self::Div8 => 8,
            Self::Div16 => 16,
        }
    }
}

/// A peripheral's bit in its bus's enable and reset registers (the same
/// position in both). Chip facts: the glue layer supplies them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RccBit {
    pub(crate) bus: Bus,
    pub(crate) bit: u8,
}

impl RccBit {
    pub(crate) const fn new(bus: Bus, bit: u8) -> Self {
        Self { bus, bit }
    }
}

/// `Rcc::enable`, `reset` and `open` over a family's buses:
/// `Bus => enable register, reset register;` per bus, expanded in the RCC
/// module, where `Rcc`, `Bus`, `RccBit` and `RCC_REGS` resolve.
macro_rules! bus_registers {
    ($($bus:ident => $enr:ident, $rstr:ident;)+) => {
        impl Rcc {
            /// Turns a peripheral's clock on, then reads the register back so
            /// the write has landed before the peripheral is touched (the
            /// delay RM0368 asks for: ES0299 2.2.7, ES0182 2.2.13).
            pub(crate) fn enable(&mut self, bit: RccBit) {
                let mask = 1 << bit.bit;
                match bit.bus {
                    $(Bus::$bus => {
                        RCC_REGS.$enr().modify(|w| w.0 |= mask);
                        let _ = RCC_REGS.$enr().read();
                    })+
                }
            }

            /// Pulses a peripheral's reset, returning its registers to reset
            /// values.
            pub(crate) fn reset(&mut self, bit: RccBit) {
                let mask = 1 << bit.bit;
                match bit.bus {
                    $(Bus::$bus => {
                        RCC_REGS.$rstr().modify(|w| w.0 |= mask);
                        RCC_REGS.$rstr().modify(|w| w.0 &= !mask);
                    })+
                }
            }

            /// What a driver's constructor does first: the clock on, then a
            /// reset, so the peripheral starts from reset values whatever ran
            /// before.
            pub(crate) fn open(&mut self, bit: RccBit) {
                self.enable(bit);
                self.reset(bit);
            }
        }
    };
}
