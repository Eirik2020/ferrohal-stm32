//! Run Q: the waveform generator both ways over one wire, as a bidirectional
//! protocol runs. Needs Z2.
//!
//! TIM3 channel 1 on PC6 and TIM8 channel 2 on PC7 are the two ends of Z2, both
//! active low with pull-ups. One sends while the other releases its pin and reads
//! port C over DMA, then they swap, and the checks recover each sample from what
//! crossed the wire. With the sender's output off, nothing crosses.

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

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [CEC])]
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
        run: Option<board::RunQ>,
        near: &'static mut [u32],
        far: &'static mut [u32],
        near_reads: &'static mut [u16],
        far_reads: &'static mut [u16],
    }

    #[init(local = [
        near_buf: [u32; 20] = [0; 20],
        far_buf: [u32; 20] = [0; 20],
        near_reads_buf: [u16; 1024] = [0; 1024],
        far_reads_buf: [u16; 1024] = [0; 1024],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_q(cx.device) {
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
                near_reads: cx.local.near_reads_buf,
                far_reads: cx.local.far_reads_buf,
            },
        )
    }

    #[task(priority = 1, local = [run, near, far, near_reads, far_reads])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let (Ok(mut tim3), Ok(mut tim8)) = (
            Pwm::new(&mut run.rcc, run.tim3, Hertz::khz(150)),
            Pwm::new(&mut run.rcc, run.tim8, Hertz::khz(150)),
        ) else {
            defmt::panic!("TIM3 or TIM8 PWM")
        };
        let near = tim3.channel::<1, _>(run.pc6).into_two_way_lane::<_, 'C'>(
            run.near_lane,
            core::mem::take(cx.local.near),
            core::mem::take(cx.local.near_reads),
        );
        let far = tim8.channel::<2, _>(run.pc7).into_two_way_lane::<_, 'C'>(
            run.far_lane,
            core::mem::take(cx.local.far),
            core::mem::take(cx.local.far_reads),
        );
        let (Ok(near), Ok(far)) = (near, far) else {
            defmt::panic!("the lanes")
        };
        let mut near = Awg::single(tim3, (near,));
        let mut far = Awg::single(tim8, (far,));
        pwm::awg_two_way_check(
            &mut report,
            "TIM3 CH1 PC6 <-> Z2 <-> TIM8 CH2 PC7",
            &mut near,
            6,
            &mut far,
            7,
            cpu,
        );
        tests::finish(&report, || {});
    }
}
