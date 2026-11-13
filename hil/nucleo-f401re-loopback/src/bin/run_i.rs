//! Run I: DShot-style compare lanes, read back by a pin sampler. No links.
//!
//! TIM3 runs at the DShot150, 300 and 600 bit rates with lanes on channels
//! 3 and 4 (PB0, PB1; DMA1 streams 7 and 2) sending different 16-bit
//! patterns from the same update. TIM1's update request has DMA2 stream 5
//! copy port B's input register into memory about 12 times a bit, and the
//! checks decode both pins' bits and timing from it.
//!
//! Everything is polled: no vectors are bound.

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
        tests::{self, Fail, Report},
    };

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunI>,
        lane3: &'static mut [u32],
        lane4: &'static mut [u32],
        samples: &'static mut [u16],
    }

    #[init(local = [
        lane3_buf: [u32; 20] = [0; 20],
        lane4_buf: [u32; 20] = [0; 20],
        samples_buf: [u16; 1024] = [0; 1024],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_i(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run I, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {},
            Local {
                run: Some(run),
                lane3: cx.local.lane3_buf,
                lane4: cx.local.lane4_buf,
                samples: cx.local.samples_buf,
            },
        )
    }

    #[task(priority = 1, local = [run, lane3, lane4, samples])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let Ok(mut pwm) = Pwm::new(&mut run.rcc, run.tim3, Hertz::khz(150)) else {
            defmt::panic!("TIM3 PWM")
        };
        let lane3 = pwm
            .channel::<3, _>(run.ch3)
            .into_lane(run.lane3, core::mem::take(cx.local.lane3));
        let lane4 = pwm
            .channel::<4, _>(run.ch4)
            .into_lane(run.lane4, core::mem::take(cx.local.lane4));
        let sampler = Sampler::new::<_, 'B'>(
            &mut run.rcc,
            run.tim1,
            run.sampler,
            Hertz::mhz(1),
            core::mem::take(cx.local.samples),
        );
        let (Ok(mut lane3), Ok(mut lane4), Ok(mut sampler)) = (lane3, lane4, sampler) else {
            defmt::panic!("TIM3 lanes or the TIM1 sampler")
        };
        for (rate, name) in [
            (150, "TIM3 CH3/CH4 PB0/PB1, DShot150 rate"),
            (300, "TIM3 CH3/CH4 PB0/PB1, DShot300 rate"),
            (600, "TIM3 CH3/CH4 PB0/PB1, DShot600 rate"),
        ] {
            let rates = pwm
                .set_frequency(Hertz::khz(rate))
                .and_then(|bit| sampler.set_rate(Hertz(bit.0 * 12)));
            if let Err(error) = rates {
                report.check(
                    name,
                    "timer and sampler at the rate [PWM-003]",
                    Err(Fail::Timer(error)),
                );
                continue;
            }
            defmt::info!(
                "{=str}: bit rate {}, sampling at {}",
                name,
                pwm.frequency(),
                sampler.rate()
            );
            pwm::lane_pair_check(
                &mut report,
                name,
                &mut pwm,
                (&mut lane3, &mut lane4),
                (0, 1),
                (0xa5c3, 0x5b29),
                &mut sampler,
                cpu,
            );
        }
        // The last rate above is still set: DShot600's, with the sampler at 12 times.
        let mut awg = Awg::single(pwm, (lane3, lane4));
        pwm::awg_check(
            &mut report,
            "TIM3 CH3/CH4 PB0/PB1",
            &mut awg,
            (0, 1),
            &mut sampler,
            cpu,
        );
        pwm::awg_free_check(
            &mut report,
            "TIM3 CH3/CH4 PB0/PB1, freed",
            awg,
            (0, 1),
            &mut sampler,
            &mut run.rcc,
            cpu,
        );
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
