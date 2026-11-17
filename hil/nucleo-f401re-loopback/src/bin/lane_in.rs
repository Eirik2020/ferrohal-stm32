//! Run P: a lane reading a waveform on its own pin, in over a wire. Needs S1.
//!
//! TIM3 channel 1 plays active low on PA6, into S1. TIM1's channel 1, complementary
//! on PA7, the other end, releases its pin and reads port A over DMA, and the check
//! recovers each sample. This is the receive side a bidirectional protocol needs.

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
        timer::{Awg, Pwm},
    };

    use crate::{
        board, pwm,
        tests::{self, Report},
    };

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunP>,
        near: &'static mut [u32],
        far: &'static mut [u32],
        reads: &'static mut [u16],
    }

    #[init(local = [
        near_buf: [u32; 20] = [0; 20],
        far_buf: [u32; 20] = [0; 20],
        reads_buf: [u16; 1024] = [0; 1024],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_p(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {},
            Local {
                run: Some(run),
                near: cx.local.near_buf,
                far: cx.local.far_buf,
                reads: cx.local.reads_buf,
            },
        )
    }

    #[task(priority = 1, local = [run, near, far, reads])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let (Ok(tim1), Ok(tim3)) = (
            Pwm::new(&mut run.rcc, run.tim1, Hertz::khz(150)),
            Pwm::new(&mut run.rcc, run.tim3, Hertz::khz(150)),
        ) else {
            defmt::panic!("TIM1 or TIM3 PWM")
        };
        let near = ferrohal_stm32::timer::ComplementaryLaneWiring::<_, 1, _, _>::new(run.pa7, run.near_lane).assemble_two_way::<'A'>(&tim1, core::mem::take(cx.local.near), core::mem::take(cx.local.reads));
        let far = ferrohal_stm32::timer::LaneWiring::<_, 1, _, _>::new(run.pa6, run.far_lane).assemble(&tim3, core::mem::take(cx.local.far));
        let (Ok(near), Ok(far)) = (near, far) else {
            defmt::panic!("the lanes")
        };
        let mut near = Awg::single(tim1, (near,));
        let mut far = Awg::single(tim3, (far,));
        pwm::awg_wire_in_check(
            &mut report,
            "TIM3 CH1 PA6 -> S1 -> TIM1 CH1N PA7",
            &mut near,
            7,
            &mut far,
            cpu,
        );
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
