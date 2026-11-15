//! Compile-fail probe: a port splits into the pins its package bonds only
//! (the generated `Split` parts). The H743VI's LQFP100 bonds PH0 and PH1 of
//! port H, so PH2 cannot be named.
//!
//! Expects: E0609

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn unbonded(p: ferrohal_stm32::chip::Peripherals, rcc: &mut ferrohal_stm32::rcc::Rcc) {
        let h = p.GPIOH.split(rcc);
        let _ = h.p2;
    }
    let _ = unbonded;
    loop {}
}
