//! Run L: the request for the ROM bootloader, across the resets it causes.
//! No links. The F401 has no data cache, so that step repeats the first.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/bootloader.rs"]
mod bootloader;
#[path = "../../../common/tests.rs"]
mod tests;

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::chip::Peripherals;
use panic_probe as _;

use tests::Report;

#[entry]
fn main() -> ! {
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_j(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    let cause = run.rcc.reset_cause();
    tests::set_cpu_hz(board::CLOCKS.sysclk.0);
    let mut report = Report::new();
    bootloader::suite(&mut report, "bootloader request", cause, || {});
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
