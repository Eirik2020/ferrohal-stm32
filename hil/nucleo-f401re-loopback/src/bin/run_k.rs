//! Run K: the polled SPI, no DMA and no interrupts, over S4 and S5.
//!
//! - SPI2 on PB13, PC2, PC3: the Foxeer F405 V2's NOR flash pins, whose
//!   DMA streams its UARTs hold, so FerroWasp drives that bus without DMA.
//! - SPI3 on PC10, PC11, PC12.
//!
//! Each SCK pin is routed to EXTI only to read its idle level.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/spi_exti.rs"]
mod spi_exti;
#[path = "../../../common/tests.rs"]
mod tests;

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{chip::Peripherals, exti::Edge, spi::BlockingSpi};
use panic_probe as _;

use tests::{Fail, Report};

#[entry]
fn main() -> ! {
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_k(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
    defmt::info!("run K, clocks: {}", board::CLOCKS);
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let cpu = board::CLOCKS.cpu;
    let mut report = Report::new();
    let mut ctrl = run.exti.ctrl;
    let mut sck_a = run.exti.l13.route::<'B'>(&mut ctrl, Edge::Rising);
    let mut sck_b = run.exti.l10.route::<'C'>(&mut ctrl, Edge::Rising);
    match BlockingSpi::new(&mut run.rcc, run.spi2, &board::SPI) {
        Ok(mut spi) => spi_exti::blocking_suite(
            &mut report,
            "SPI2 polled PB13/PC2/PC3 (Foxeer NOR pins)",
            &mut spi,
            &board::SPI,
            cpu,
            &mut sck_a,
            &mut ctrl,
        ),
        Err(error) => report.check(
            "SPI2 polled PB13/PC2/PC3 (Foxeer NOR pins)",
            "open",
            Err(Fail::SpiConfig(error)),
        ),
    }
    match BlockingSpi::new(&mut run.rcc, run.spi3, &board::SPI) {
        Ok(mut spi) => spi_exti::blocking_suite(
            &mut report,
            "SPI3 polled PC10/PC11/PC12",
            &mut spi,
            &board::SPI,
            cpu,
            &mut sck_b,
            &mut ctrl,
        ),
        Err(error) => report.check(
            "SPI3 polled PC10/PC11/PC12",
            "open",
            Err(Fail::SpiConfig(error)),
        ),
    }
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
