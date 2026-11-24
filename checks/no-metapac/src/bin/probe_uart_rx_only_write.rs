//! Compile-fail probe: a receive-only UART has no transmit operations. It is
//! the same `Uart` as a full one, but `write` (and `try_send`, `flush`,
//! `embedded_io::Write`) exist only on a full `Wiring`, so a UART holding no
//! TX pin or channel cannot be asked to send (ADR-011, Q3).
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;
use ferrohal_stm32::{
    chip::USART1,
    dma::Channel,
    gpio::{Alternate, Pin},
    uart::{RxWiring, Uart},
};

/// A real receive route, so the UART exists: PA10 on AF7, DMA1 channel 0.
type RxOnly = RxWiring<USART1, Pin<'A', 10, Alternate<7>>, Channel<1, 0>>;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn send(uart: &mut Uart<RxOnly>) {
        let _ = uart.write(b"x");
    }
    let _ = send;
    loop {}
}
