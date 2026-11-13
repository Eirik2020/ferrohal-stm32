//! Run N: TIM1 starting TIM3 from its trigger output, as the Foxeer's motor
//! timers start. No links: no output is enabled.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/pwm.rs"]
mod pwm;
#[path = "../../../common/tests.rs"]
mod tests;

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{chip::Peripherals, time::Hertz, timer::Pwm};
use panic_probe as _;

use tests::Report;

#[entry]
fn main() -> ! {
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let (tim_m, tim_s) = (p.TIM1, p.TIM3);
    let mut rcc = match p.RCC.freeze(&board::CLOCK_CONFIG) {
        Ok(rcc) => rcc,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    tests::set_cpu_hz(board::CLOCKS.sysclk.0);
    let (Ok(mut master), Ok(mut slave)) = (
        Pwm::new(&mut rcc, tim_m, Hertz::khz(2)),
        Pwm::new(&mut rcc, tim_s, Hertz::khz(2)),
    ) else {
        defmt::panic!("timers")
    };
    let mut report = Report::new();
    pwm::trigger_suite(&mut report, "TIM1 starts TIM3", &mut master, &mut slave, 32);
    tests::finish(&report, || {});
}
