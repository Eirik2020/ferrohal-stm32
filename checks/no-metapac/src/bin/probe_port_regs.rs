//! Compile-fail probe: `PortInstance::regs` moved to a crate-private
//! supertrait (W6b1); no caller outside the crate can reach a port's
//! registers.
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn regs<P: ferrohal_stm32::gpio::PortInstance>(port: &mut P) {
        port.regs();
    }
    let _ = regs;
    loop {}
}
