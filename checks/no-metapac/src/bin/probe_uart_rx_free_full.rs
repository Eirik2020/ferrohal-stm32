//! Compile-fail probe: a receive-only UART frees to its own `RxWiring`, never
//! to a full `Wiring`: it cannot hand back a TX pin and channel it never held
//! (free redesign, rules 1-2; ADR-011).
//!
//! Expects: E0308

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;
use ferrohal_stm32::{
    chip::USART1,
    dma::Channel,
    gpio::{Alternate, Pin},
    uart::{RxWiring, Uart, Wiring},
};

type Rx = Pin<'A', 10, Alternate<7>>;
type RxOnly = RxWiring<USART1, Rx, Channel<1, 0>>;
type Full = Wiring<USART1, Pin<'A', 9, Alternate<7>>, Rx, Channel<1, 1>, Channel<1, 0>>;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn free_into_full(uart: Uart<RxOnly>) -> Option<Full> {
        uart.free().ok().map(|(wiring, _)| wiring)
    }
    let _ = free_into_full;
    loop {}
}
