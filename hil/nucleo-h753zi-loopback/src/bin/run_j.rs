//! Run J: the TBS Lucid H7's DShot lanes, read back by pin samplers. No
//! links.
//!
//! TIM3 (PB0, PB1 on DMA2 streams 0 and 1: the Lucid's motors 1 and 2) and
//! TIM5 (PA0, PA3 on streams 2 and 3) run at the DShot150, 300 and 600 bit
//! rates, each timer's two lanes sending different 16-bit patterns from the
//! same update. TIM8's update request has DMA1 stream 0 copy port B's input
//! register into memory about 12 times a bit, TIM2's has stream 1 copy port
//! A's, and the checks decode every pin's bits and timing. TIM1 then drives
//! its complementary output CH3N (PB15) at a quarter duty, which the port B
//! sampler measures.
//!
//! Everything is polled: no vectors are bound. The caches stay off, as on
//! the Lucid.

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
        run: Option<board::RunJ>,
        lanes: Option<&'static mut [[u32; 20]; 4]>,
        port_b: &'static mut [u16],
        port_a: &'static mut [u16],
    }

    #[init(local = [
        lane_bufs: [[u32; 20]; 4] = [[0; 20]; 4],
        port_b_buf: [u16; 1024] = [0; 1024],
        port_a_buf: [u16; 1024] = [0; 1024],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_j(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run J, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {},
            Local {
                run: Some(run),
                lanes: Some(cx.local.lane_bufs),
                port_b: cx.local.port_b_buf,
                port_a: cx.local.port_a_buf,
            },
        )
    }

    #[task(priority = 1, local = [run, lanes, port_b, port_a])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let Some(lanes) = cx.local.lanes.take() else {
            defmt::unreachable!()
        };
        let [b0, b1, a0, a3] = lanes.each_mut();
        let sampler_b = Sampler::new::<_, _, 'B'>(
            &mut run.rcc,
            run.tim8,
            run.port_b,
            Hertz::mhz(1),
            core::mem::take(cx.local.port_b),
        );
        let sampler_a = Sampler::new::<_, _, 'A'>(
            &mut run.rcc,
            run.tim2,
            run.port_a,
            Hertz::mhz(1),
            core::mem::take(cx.local.port_a),
        );
        let (Ok(mut sampler_b), Ok(mut sampler_a)) = (sampler_b, sampler_a) else {
            defmt::panic!("samplers")
        };

        let Ok(mut tim3) = Pwm::new(&mut run.rcc, run.tim3, Hertz::khz(150)) else {
            defmt::panic!("TIM3 PWM")
        };
        let ch3 = tim3
            .channel::<3, _>(run.pb0)
            .into_lane(run.tim3_lanes.0, b0);
        let ch4 = tim3
            .channel::<4, _>(run.pb1)
            .into_lane(run.tim3_lanes.1, b1);
        let (Ok(mut ch3), Ok(mut ch4)) = (ch3, ch4) else {
            defmt::panic!("TIM3 lanes")
        };
        let Ok(mut tim5) = Pwm::new(&mut run.rcc, run.tim5, Hertz::khz(150)) else {
            defmt::panic!("TIM5 PWM")
        };
        let ch1 = tim5
            .channel::<1, _>(run.pa0)
            .into_lane(run.tim5_lanes.0, a0);
        let ch4_5 = tim5
            .channel::<4, _>(run.pa3)
            .into_lane(run.tim5_lanes.1, a3);
        let (Ok(mut ch1), Ok(mut ch4_5)) = (ch1, ch4_5) else {
            defmt::panic!("TIM5 lanes")
        };

        for rate in [150, 300, 600] {
            let name = match rate {
                150 => "TIM3 CH3/CH4 PB0/PB1 (Lucid), DShot150 rate",
                300 => "TIM3 CH3/CH4 PB0/PB1 (Lucid), DShot300 rate",
                _ => "TIM3 CH3/CH4 PB0/PB1 (Lucid), DShot600 rate",
            };
            let rates = tim3
                .set_frequency(Hertz::khz(rate))
                .and_then(|bit| sampler_b.set_rate(Hertz(bit.0 * 12)));
            match rates {
                Ok(_) => pwm::lane_pair_check(
                    &mut report,
                    name,
                    &mut tim3,
                    (&mut ch3, &mut ch4),
                    (0, 1),
                    (0xa5c3, 0x5b29),
                    &mut sampler_b,
                    cpu,
                ),
                Err(error) => report.check(
                    name,
                    "timer and sampler at the rate [PWM-003]",
                    Err(Fail::Timer(error)),
                ),
            }
            let name = match rate {
                150 => "TIM5 CH1/CH4 PA0/PA3, DShot150 rate",
                300 => "TIM5 CH1/CH4 PA0/PA3, DShot300 rate",
                _ => "TIM5 CH1/CH4 PA0/PA3, DShot600 rate",
            };
            let rates = tim5
                .set_frequency(Hertz::khz(rate))
                .and_then(|bit| sampler_a.set_rate(Hertz(bit.0 * 12)));
            match rates {
                Ok(_) => pwm::lane_pair_check(
                    &mut report,
                    name,
                    &mut tim5,
                    (&mut ch1, &mut ch4_5),
                    (0, 3),
                    (0x3c5a, 0xe817),
                    &mut sampler_a,
                    cpu,
                ),
                Err(error) => report.check(
                    name,
                    "timer and sampler at the rate [PWM-003]",
                    Err(Fail::Timer(error)),
                ),
            }
        }

        // TIM1's complementary output at a quarter duty, 100 kHz.
        let name = "TIM1 CH3N PB15, 100 kHz, 1/4 duty";
        // The last rates above are still set: DShot600's, with the samplers at 12 times.
        let mut awg3 = Awg::single(tim3, (ch3, ch4));
        pwm::awg_check(
            &mut report,
            "TIM3 CH3/CH4 PB0/PB1 (Lucid)",
            &mut awg3,
            (0, 1),
            &mut sampler_b,
            cpu,
        );
        let mut awg5 = Awg::single(tim5, (ch1, ch4_5));
        pwm::awg_check(
            &mut report,
            "TIM5 CH1/CH4 PA0/PA3",
            &mut awg5,
            (0, 3),
            &mut sampler_a,
            cpu,
        );

        match Pwm::new(&mut run.rcc, run.tim1, Hertz::khz(100)) {
            Ok(mut tim1) => {
                let mut ch = tim1.complementary::<3, _>(run.pb15);
                ch.set_duty(tim1.period() / 4);
                tim1.start();
                let _ = sampler_b.set_rate(Hertz::mhz(2));
                pwm::duty_check(&mut report, name, &mut sampler_b, 15, 4, cpu);
                tim1.set_output::<3>(false);
                tim1.stop();
            }
            Err(error) => report.check(name, "timer at the rate", Err(Fail::Timer(error))),
        }
        tests::finish(&report, || {});
    }
}
