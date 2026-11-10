//! Compile-fail probe: `Stream::steal` was removed (W6b2); a stream token
//! cannot be conjured from nothing.
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    let steal = ferrohal_stm32::dma::Stream::<1, 1>::steal;
    let _ = steal;
    loop {}
}
