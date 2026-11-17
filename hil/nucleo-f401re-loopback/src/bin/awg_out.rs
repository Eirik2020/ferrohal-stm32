//! Run O: the waveform generator out over a wire. Needs J3.
//!
//! TIM4 channel 1 plays on PB6. TIM1 reads port C, where J3 brings it to PC7, and
//! the check recovers each sample from what crossed the wire.

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

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SDIO])]
mod app {
    use ferrohal_stm32::{
        time::Hertz,
        timer::{Awg, Pwm, Sampler},
    };

    use crate::{
        board, pwm,
        tests::{self, Report},
    };

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunO>,
        lane: &'static mut [u32],
        samples: &'static mut [u16],
    }

    #[init(local = [
        lane_buf: [u32; 20] = [0; 20],
        samples_buf: [u16; 1024] = [0; 1024],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_o(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {},
            Local {
                run: Some(run),
                lane: cx.local.lane_buf,
                samples: cx.local.samples_buf,
            },
        )
    }

    #[task(priority = 1, local = [run, lane, samples])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let Ok(pwm) = Pwm::new(&mut run.rcc, run.tim4, Hertz::khz(150)) else {
            defmt::panic!("TIM4 PWM")
        };
        let lane = ferrohal_stm32::timer::LaneWiring::<_, 1, _, _>::new(run.pb6, run.lane).assemble(&pwm, core::mem::take(cx.local.lane));
        let sampler = Sampler::new::<'C'>(
            &mut run.rcc,
            run.tim1,
            run.sampler,
            Hertz::mhz(6),
            core::mem::take(cx.local.samples),
        );
        let (Ok(lane), Ok(mut sampler)) = (lane, sampler) else {
            defmt::panic!("the TIM4 lane or the TIM1 sampler")
        };
        let mut awg = Awg::single(pwm, (lane,));
        pwm::awg_wire_out_check(
            &mut report,
            "TIM4 CH1 PB6 -> J3 -> PC7",
            &mut awg,
            &mut sampler,
            7,
            cpu,
        );
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
