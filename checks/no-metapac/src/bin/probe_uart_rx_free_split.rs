//! Compile-fail probe: a split UART's receive half cannot free the line
//! while its transmit half lives. `UartRx::free` takes a receive-only
//! UART's `RxLine`, not a full UART's `Line` (free redesign, rules 1-2).
//!
//! Expects: E0308

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;
use ferrohal_stm32::{
    chip::USART1,
    uart::{Line, UartRx, UartTx},
};

#[cortex_m_rt::entry]
fn main() -> ! {
    fn free_under_tx(line: Line<USART1>, tx: UartTx<USART1>, rx: UartRx<USART1>) -> USART1 {
        let token = rx.free(line);
        let _ = tx;
        token
    }
    let _ = free_under_tx;
    loop {}
}
