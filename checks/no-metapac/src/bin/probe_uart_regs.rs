//! Compile-fail probe: the UART's `regs` was removed (W6b1), and the one
//! owner of ADR-011 adds none back: a `Uart` is not a path to the USART
//! registers.
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;
use ferrohal_stm32::uart::{Parts, Uart};

#[cortex_m_rt::entry]
fn main() -> ! {
    fn regs<W: Parts>(uart: &Uart<W>) {
        let _ = uart.regs();
    }
    let _ = regs;
    loop {}
}
