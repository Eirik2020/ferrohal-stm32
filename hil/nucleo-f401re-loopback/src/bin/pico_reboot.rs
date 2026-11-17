//! Run T: asks the Pico simulator to reboot into its USB bootloader, the
//! request that spares the next flash a finger on the BOOTSEL button.
//! `hil/pico-i2c-sim.sh flash` runs it when the RPI-RP2 drive is not
//! mounted: one write to the map's BOOT register over I2C3, and the drive
//! appears a moment later. There is nothing else to check here — the
//! live suite covers the bus; this run exists so the host can reach the
//! bootloader without a person.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use cortex_m_rt::entry;
use defmt_rtt as _;
use embedded_hal::i2c::I2c as HalI2c;
use ferrohal_stm32::{
    chip::Peripherals,
    i2c::{Config, I2c},
    time::Hertz,
};
use panic_probe as _;

use tests::{Fail, Report};

/// The simulator's address and BOOT register, as `hil/pico-i2c-sim`'s
/// map spells them.
const SIM: u8 = 0x2A;
const REG_BOOT: u8 = 0x3D;

#[entry]
fn main() -> ! {
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_s(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let mut report = Report::new();
    let link = "I2C3 PA8/PC9 (Pico simulator)";
    match I2c::new(&mut run.rcc, run.i2c, &Config::new(Hertz::khz(50))) {
        Ok(mut bus) => {
            bus.set_clock(Some(&tests::DWT_CLOCK));
            report.check_for(
                link,
                "I2C-006",
                "the simulator is asked for its bootloader",
                match HalI2c::write(&mut bus, SIM, &[REG_BOOT, 1]) {
                    Ok(()) => Ok(()),
                    Err(error) => Err(Fail::I2c(error)),
                },
            );
        }
        Err(refused) => report.check(
            link,
            "the simulator is asked for its bootloader",
            Err(Fail::I2cConfig(refused.error())),
        ),
    }
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
