//! Compile-fail probe: `DmaInstance::regs` moved to a crate-private
//! supertrait (W6b1); no caller outside the crate can reach the DMA
//! registers.
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn regs<D: ferrohal_stm32::dma::DmaInstance>(dma: &D) {
        let _ = dma.regs();
    }
    let _ = regs;
    loop {}
}
