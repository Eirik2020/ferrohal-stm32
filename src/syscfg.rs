//! The system configuration controller, split into the parts its users
//! own. SYSCFG holds unrelated controls side by side: EXTI's port selection
//! (EXTICR) on every chip here, the F4's memory remap, the H7's I/O
//! compensation cell. One driver taking the whole token would lock the
//! others out, so [`SYSCFG::split`] turns on its clock and hands out one part
//! per user. Today that is [`Exticr`] alone, for [`exti`](crate::exti); a
//! part for the remap or the compensation cell comes with the driver that
//! needs it.
//!
//! Requirements: EXTI-001

use crate::{
    chip::{SYSCFG, SYSCFG_ENABLE},
    rcc::Rcc,
};

/// EXTI's port selection registers (EXTICR): which port drives each of the
/// sixteen GPIO lines. [`EXTI::split`](crate::chip::EXTI) takes it.
pub struct Exticr {
    _private: (),
}

/// The split SYSCFG.
pub struct Parts {
    pub exticr: Exticr,
}

impl SYSCFG {
    /// Turns on SYSCFG's clock and hands out its parts.
    pub fn split(self, rcc: &mut Rcc) -> Parts {
        rcc.enable(SYSCFG_ENABLE);
        Parts {
            exticr: Exticr { _private: () },
        }
    }
}
