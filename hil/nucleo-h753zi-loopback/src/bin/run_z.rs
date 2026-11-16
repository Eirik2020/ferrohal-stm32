//! Run Z: the ROM bootloader reached (RESET-004) on the H7. The first boot
//! asks for it and resets; the next boot's `enter_bootloader_if_requested`
//! jumps to the ROM before any clock is touched. `hil/run-h753.sh` then checks
//! that `dfu-util` lists the ROM on CN13 by the serial the ROM derives from
//! the UID, and erases the chip so the ROM does not stay on the bus. The
//! ROM's bootloader is 0x91 here (measured), which no longer locks PWR_CR3
//! (AN2606), so the next run needs no power cycle.

#![no_std]
#![no_main]

// For its identity block only; the run uses no board part.
#[path = "../board.rs"]
mod board;

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::reset;
use panic_probe as _;

#[entry]
fn main() -> ! {
    reset::enter_bootloader_if_requested();
    defmt::info!("run Z: asking for the ROM bootloader; the host looks for its DFU device");
    // A second at the reset clock (HSI, 64 MHz), for the line to reach the host.
    cortex_m::asm::delay(64_000_000);
    reset::reboot_to_bootloader()
}
