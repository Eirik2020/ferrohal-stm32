//! Compile-fail probe: `Channel::start(&Transfer)` is `pub(crate)` (W6b2);
//! the public path takes a configuration, not a hand-made `Transfer`.
//!
//! Expects: E0624

#![no_std]
#![no_main]

use ferrohal_no_metapac as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    fn start(channel: &mut ferrohal_stm32::dma::Channel, transfer: &ferrohal_stm32::dma::Transfer) {
        let _ = channel.start(transfer);
    }
    let _ = start;
    loop {}
}
