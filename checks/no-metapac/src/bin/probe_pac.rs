//! Compile-fail probe: metapac must not be re-exported at the crate root
//! (`ferrohal_stm32::pac`).
//!
//! Expects: E0432

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

use ferrohal_stm32::pac;

#[cortex_m_rt::entry]
fn main() -> ! {
    loop {}
}
