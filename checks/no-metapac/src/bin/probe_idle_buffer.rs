//! Compile-fail probe: `Channel::set_idle_buffer` was removed (W6b2); the
//! idle buffer is none of an outside caller's business.
//!
//! Expects: E0599

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn idle(channel: &mut ferrohal_stm32::dma::Channel, buffer: &'static [u8]) {
        let _ = channel.set_idle_buffer(buffer);
    }
    let _ = idle;
    loop {}
}
