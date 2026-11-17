//! Where a peripheral's kernel clock comes from: one type for every driver
//! (API review D2), so a board, the generator and the drivers say it the
//! same way. Re-exported as `crate::rcc::Kernel` from each RCC module.
//!
//! Requirements: RCC-001

use crate::{
    rcc::{Bus, Clocks},
    time::Hertz,
};

/// The clock that times a peripheral.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Kernel {
    /// The clock of the bus it sits on: the U(S)ARTs, the I2Cs, the F4's
    /// SPIs and ADC.
    Bus(Bus),
    /// The timer clock of the bus it sits on: its bus clock, doubled when
    /// the bus is divided (RM0090 7.2, RM0433 8.5.6).
    Timer(Bus),
    /// PLL1's Q output: the H7's SPI1 to SPI3 (SPI123SEL at reset) and its
    /// SDMMC (SDMMCSEL at reset).
    #[cfg(feature = "rcc_h7_rm0433")]
    Pll1Q,
}

impl Kernel {
    /// The frequency, or `None` if its source isn't running.
    pub const fn frequency(self, clocks: &Clocks) -> Option<Hertz> {
        match self {
            Self::Bus(bus) => Some(clocks.bus(bus)),
            Self::Timer(bus) => Some(clocks.timer(bus)),
            #[cfg(feature = "rcc_h7_rm0433")]
            Self::Pll1Q => clocks.pll1_q,
        }
    }

    /// The frequency, 0 if its source isn't running: for a driver whose
    /// divider check refuses a zero clock anyway.
    pub const fn clock(self, clocks: &Clocks) -> Hertz {
        match self.frequency(clocks) {
            Some(clock) => clock,
            None => Hertz(0),
        }
    }
}
