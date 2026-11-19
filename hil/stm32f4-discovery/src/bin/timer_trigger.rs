//! Run F: die 0x413's timer trigger routes (RM0090 table 98), each as the
//! glue states it: TIM8 and TIM3 from TIM1 (ITR0), TIM2 from TIM3 (ITR2) and
//! TIM5 from TIM3 (ITR1). For each, the slave must wait while its master is
//! stopped, then start with it, 100 starts over, within 32 counts (GLUE-010,
//! with PWM-004 and PWM-005's checks). The routes are die 0x413's alone, so
//! they run on the F407 here. Counters only, no pins, no links; everything
//! is polled.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/pwm.rs"]
mod pwm;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use ferrohal_stm32::{time::Hertz, timer::Pwm};

    use crate::{
        board, pwm,
        tests::{self, Fail, Report},
    };

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunF>,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_f(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run F, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (Shared {}, Local { run: Some(run) })
    }

    #[task(priority = 1, local = [run])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let rate = Hertz::khz(600);
        let timers = (
            Pwm::new(&mut run.rcc, run.tim1, rate),
            Pwm::new(&mut run.rcc, run.tim2, rate),
            Pwm::new(&mut run.rcc, run.tim3, rate),
            Pwm::new(&mut run.rcc, run.tim5, rate),
            Pwm::new(&mut run.rcc, run.tim8, rate),
        );
        let (Ok(mut tim1), Ok(mut tim2), Ok(mut tim3), Ok(mut tim5), Ok(mut tim8)) = timers else {
            report.check("die 0x413 trigger routes", "every timer at 600 kHz [GLUE-010]",
                Err(Fail::Count { expected: 5, got: 0 }));
            tests::finish(&report, || {})
        };
        pwm::trigger_suite(&mut report, "TIM1 starts TIM8 [GLUE-010]", &mut tim1, &mut tim8, 32);
        pwm::trigger_suite(&mut report, "TIM1 starts TIM3 [GLUE-010]", &mut tim1, &mut tim3, 32);
        pwm::trigger_suite(&mut report, "TIM3 starts TIM2 [GLUE-010]", &mut tim3, &mut tim2, 32);
        pwm::trigger_suite(&mut report, "TIM3 starts TIM5 [GLUE-010]", &mut tim3, &mut tim5, 32);
        tests::finish(&report, || {});
    }
}
