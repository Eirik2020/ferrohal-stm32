//! Compile-fail probe: `reset::enter_bootloader` is `pub(crate)` (W6b2);
//! the public path is `reboot_to_bootloader()`, which checks the request.
//!
//! Expects: E0603

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    let bootloader = ferrohal_stm32::reset::enter_bootloader;
    let _ = bootloader;
    loop {}
}
