//! Run C: the ROM bootloader reached (RESET-004). The first boot asks for it
//! and resets; the next boot's `enter_bootloader_if_requested` jumps to the
//! ROM before any clock is touched, as AN2606 expects. `hil/run-discovery.sh`
//! then checks that `dfu-util` lists this chip's ROM on CN5, by the serial the
//! ROM derives from the UID, and resets the chip out of it.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::reset;
use panic_probe as _;

#[entry]
fn main() -> ! {
    reset::enter_bootloader_if_requested();
    defmt::info!("run C: asking for the ROM bootloader; the host looks for its DFU device");
    // A second at the reset clock (HSI, 16 MHz), for the line to reach the host.
    cortex_m::asm::delay(16_000_000);
    reset::reboot_to_bootloader()
}
