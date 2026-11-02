//! Run L: the polled SPI, no DMA and no interrupts, over S3 and S4.
//!
//! - SPI2 on PB10, PC2, PC3, from PLL1Q.
//! - SPI4 on PE12, PE13, PE14, from PCLK2.
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
    let mut run = match board::run_l(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
    defmt::info!("run L, clocks: {}", board::CLOCKS);
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let cpu = board::CLOCKS.cpu;
    let mut report = Report::new();
    let mut ctrl = run.exti.ctrl;
    let mut sck_a = run.exti.l10.route::<'B'>(&mut ctrl, Edge::Rising);
    let mut sck_b = run.exti.l12.route::<'E'>(&mut ctrl, Edge::Rising);
    match BlockingSpi::new(&mut run.rcc, run.spi2, &board::SPI) {
        Ok(mut spi) => spi_exti::blocking_suite(
            &mut report,
            "SPI2 polled PB10/PC2/PC3",
            &mut spi,
            &board::SPI,
            cpu,
            &mut sck_a,
            &mut ctrl,
        ),
        Err(error) => report.check(
            "SPI2 polled PB10/PC2/PC3",
            "open",
            Err(Fail::SpiConfig(error)),
        ),
    }
    match BlockingSpi::new(&mut run.rcc, run.spi4, &board::SPI) {
        Ok(mut spi) => spi_exti::blocking_suite(
            &mut report,
            "SPI4 polled PE12/PE13/PE14",
            &mut spi,
            &board::SPI,
            cpu,
            &mut sck_b,
            &mut ctrl,
        ),
        Err(error) => report.check(
            "SPI4 polled PE12/PE13/PE14",
            "open",
            Err(Fail::SpiConfig(error)),
        ),
    }
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
