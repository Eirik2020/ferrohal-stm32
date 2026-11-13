//! Compile-fail probe: the UART halves carry their instance, so one UART's
//! line does not rejoin another's halves (free redesign, rules 1-2).
//!
//! Expects: E0308

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;
use ferrohal_stm32::{
    chip::{UART7, USART1},
    uart::{Line, Uart, UartRx, UartTx},
};

#[cortex_m_rt::entry]
fn main() -> ! {
    fn cross(line: Line<USART1>, tx: UartTx<UART7>, rx: UartRx<UART7>) -> Uart<USART1> {
        Uart::join(line, tx, rx)
    }
    let _ = cross;
    loop {}
}
