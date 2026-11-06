//! Run J: the independent watchdog, across the reset it causes. No links.
//!
//! The first boot feeds a 50 ms watchdog every 10 ms for 300 ms, then
//! stops; the watchdog must reset the chip. The second boot finds the
//! watchdog as the reset cause and reports. The F401's LSI runs at 17 to
//! 47 kHz (DS10086), so the reset may come 34 to 94 ms after the last feed.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;
#[path = "../../../common/watchdog.rs"]
mod watchdog;

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{chip::Peripherals, iwdg::Watchdog};
use panic_probe as _;

use tests::{Fail, Report};

#[entry]
fn main() -> ! {
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_j(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    let cause = run.rcc.reset_cause();
    defmt::info!("run J, reset cause: {}", cause);
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let mut report = Report::new();
    let name = "IWDG 50 ms";
    if !watchdog::after_reset(&mut report, name, cause, (17, 47)) {
        match Watchdog::start(run.iwdg, watchdog::TIMEOUT_US) {
            Ok(dog) => watchdog::feed_then_starve(&mut report, name, cause, dog, board::CLOCKS.cpu),
            Err(refused) => report.check(
                name,
                "start",
                Err(Fail::Count {
                    expected: 1,
                    got: refused.error() as u32,
                }),
            ),
        }
    }
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
