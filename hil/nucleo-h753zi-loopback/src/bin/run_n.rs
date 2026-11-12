//! Run N: the request for the ROM bootloader, across the resets it causes,
//! with the data cache off and then on. No links. On this die a lone word
//! written to AXI SRAM is lost at reset, so a request that isn't written whole
//! fails here (RM0433 2.4).

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
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_k(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    let cause = run.rcc.reset_cause();
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let mut report = Report::new();
    bootloader::suite(&mut report, "bootloader request", cause, || {
        core.SCB.enable_dcache(&mut core.CPUID)
    });
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
