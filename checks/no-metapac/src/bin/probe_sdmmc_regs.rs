//! Compile-fail probe: SDMMC's `regs` sits on the crate-private `Registers`
//! supertrait (W6b1); the public `Instance` trait names no register path.
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn regs<S: ferrohal_stm32::sdmmc::Instance>(sdmmc: &S) {
        let _ = sdmmc.regs();
    }
    let _ = regs;
    loop {}
}
