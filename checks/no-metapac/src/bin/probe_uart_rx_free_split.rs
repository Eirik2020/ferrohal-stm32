//! Compile-fail probe: a split UART's receive half cannot free the line
//! while its transmit half lives. `UartRx::free` exists only on a
//! receive-only UART's half and takes its `RxLine`; a full UART's `Line`
//! does not stand in for it (free redesign, rules 1-2).
//!
//! Expects: E0308

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;
use ferrohal_stm32::{
    chip::USART1,
    dma::Stream,
    gpio::{Alternate, Pin},
    uart::{Line, RxWiring, UartRx, Wiring},
};

type Full = Wiring<USART1, (), (), (), ()>;
/// A real receive route, so `free` exists: PA10 on AF7, DMA1 stream 0.
type RxOnly = RxWiring<USART1, Pin<'A', 10, Alternate<7>>, Stream<1, 0>>;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn free_under_tx(line: Line<Full>, rx: UartRx<RxOnly>) {
        let _ = rx.free(line);
    }
    let _ = free_under_tx;
    loop {}
}
