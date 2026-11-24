//! Run D: `timer_v1`'s complementary output, which the F401 cannot sample.
//! TIM1 drives CH1N on PE8 at a quarter duty, 100 kHz; TIM8's update request
//! has DMA2 stream 1 copy port E's input register at 2 MHz. Checks: the
//! complementary output carries the duty set on the channel (TIMER-PWM-008), and a
//! one-pulse period on it (TIMER-PWM-007). No links; everything is polled.

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
    use ferrohal_stm32::{
        time::Hertz,
        timer::{Pwm, Sampler},
    };

    use crate::{
        board, pwm,
        tests::{self, Fail, Report},
    };

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunD>,
        port_e: &'static mut [u16],
    }

    #[init(local = [port_e_buf: [u16; 1024] = [0; 1024]])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_d(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run D, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {},
            Local {
                run: Some(run),
                port_e: cx.local.port_e_buf,
            },
        )
    }

    #[task(priority = 1, local = [run, port_e])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let Ok(mut sampler) = Sampler::new::<'E'>(
            &mut run.rcc,
            run.tim8,
            run.port_e,
            Hertz::mhz(2),
            core::mem::take(cx.local.port_e),
        ) else {
            defmt::panic!("TIM8 sampler on port E")
        };

        let name = "TIM1 CH1N PE8, 100 kHz, 1/4 duty";
        match Pwm::new(&mut run.rcc, run.tim1, Hertz::khz(100)) {
            Ok(mut tim1) => {
                let mut ch = tim1.complementary::<1, _>(run.pe8);
                ch.set_duty(tim1.period() / 4);
                tim1.start();
                pwm::complementary_duty_check(&mut report, name, &mut sampler, 8, 4, cpu);
                pwm::one_pulse_check(&mut report, name, &mut tim1, 8, 4, &mut sampler, cpu);
                tim1.set_output::<1>(false);
                tim1.stop();
            }
            Err(refused) => {
                report.check(name, "timer at the rate", Err(Fail::Timer(refused.error())))
            }
        }
        tests::finish(&report, || {});
    }
}
