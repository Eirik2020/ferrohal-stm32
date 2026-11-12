//! Compile-fail probe: the UART line's `regs` was removed (W6b1); a `Line`
//! handle is not a path to the USART registers.
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn regs<U: ferrohal_stm32::uart::Instance>(line: &ferrohal_stm32::uart::Line<U>) {
        let _ = line.regs();
    }
    let _ = regs;
    loop {}
}
