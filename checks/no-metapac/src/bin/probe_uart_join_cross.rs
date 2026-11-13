//! Compile-fail probe: the UART handles are typed by their wiring, so one
//! UART's line does not rejoin another's halves (free redesign, rules 1-2).
//!
//! Expects: E0308

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;
use ferrohal_stm32::{
    chip::{UART7, USART1},
    uart::{Line, Uart, UartRx, UartTx, Wiring},
};

type One = Wiring<USART1, (), (), (), ()>;
type Other = Wiring<UART7, (), (), (), ()>;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn cross(line: Line<One>, tx: UartTx<Other>, rx: UartRx<Other>) -> Uart<One> {
        Uart::join(line, tx, rx)
    }
    let _ = cross;
    loop {}
}
