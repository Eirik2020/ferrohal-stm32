//! Compile-fail probe: `AnyStream::regs` was removed (W6b1); a DMA channel
//! handle is not a path to the stream registers.
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn regs(channel: &ferrohal_stm32::dma::AnyStream) {
        let _ = channel.regs();
    }
    let _ = regs;
    loop {}
}
