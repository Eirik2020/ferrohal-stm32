//! Compile-fail probe: `DmaInstance::mux` moved to a crate-private
//! supertrait (W6b1); no caller outside the crate can reach DMAMUX.
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn mux<D: ferrohal_stm32::dma::DmaInstance>(dma: &D) {
        let _ = dma.mux();
    }
    let _ = mux;
    loop {}
}
