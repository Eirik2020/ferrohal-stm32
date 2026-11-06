//! Run R: the SpeedyBee F405 V5's four motor lanes on two timers, one start.
//!
//! TIM3 CH3/CH4 on PB0/PB1 and TIM2 CH3/CH2 on PB10/PB3 (DMA1 streams 7, 2,
//! 1 and 6) run at the DShot600 rate, read back by the TIM1/DMA2 port-B
//! sampler. The board's TIM2 CH4 pin, PB11, is not bonded on the F401's
//! LQFP64 (DS10086's pin table), so the bench's second TIM2 lane rides CH2 on
//! PB3 over TIM2 CH4's own DMA route, stream 6 channel 3; the board's PB11
//! lane is glue-checked for the F405 and proven on the board itself. TIM2
//! starts first in software beside TIM3 (the Lucid's arrangement), then from
//! TIM3's trigger output (the Foxeer's arrangement, over the SpeedyBee's
//! timers' TIM3-to-TIM2 route), then every lane active low, as bidirectional
//! DShot idles the line. No links: a pin's input register reads its own
//! waveform, and the sampler reaches every motor pin of port B over DMA2,
//! which serves only GPIO and memory here, so ES0299 2.2.11 can't strike. A
//! reply on one of these lanes is read by the sampler, not by the lane's own
//! stream: DMA1's AHB peripheral port is not connected to the bus matrix
//! (RM0090).
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
        chip::{TIM2, TIM3},
        time::Hertz,
        timer::{CompareLane, Pwm, Sampler},
    };

    use crate::{
        board, pwm,
        tests::{self, Fail, Report},
    };

    /// 16 bits per frame, most significant first.
    const BITS: usize = 16;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunR>,
        lane3: &'static mut [u32],
        lane4: &'static mut [u32],
        lane23: &'static mut [u32],
        lane22: &'static mut [u32],
        samples: &'static mut [u16],
    }

    #[init(local = [
        lane3_buf: [u32; 20] = [0; 20],
        lane4_buf: [u32; 20] = [0; 20],
        lane23_buf: [u32; 20] = [0; 20],
        lane22_buf: [u32; 20] = [0; 20],
        samples_buf: [u16; 1024] = [0; 1024],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_r(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run R, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {},
            Local {
                run: Some(run),
                lane3: cx.local.lane3_buf,
                lane4: cx.local.lane4_buf,
                lane23: cx.local.lane23_buf,
                lane22: cx.local.lane22_buf,
                samples: cx.local.samples_buf,
            },
        )
    }

    /// Arms the four lanes with their patterns, stages both timers, starts
    /// them — both in software, or TIM3 alone when `linked` (TIM2 starts from
    /// its trigger) — and decodes every lane's pin from the sampler.
    #[allow(clippy::too_many_arguments)]
    fn four_lanes(
        report: &mut Report,
        name: &str,
        pwm3: &mut Pwm<TIM3>,
        pwm2: &mut Pwm<TIM2>,
        lane3: &mut CompareLane<TIM3, 3>,
        lane4: &mut CompareLane<TIM3, 4>,
        lane23: &mut CompareLane<TIM2, 3>,
        lane22: &mut CompareLane<TIM2, 2>,
        patterns: (u16, u16, u16, u16),
        sampler: &mut Sampler,
        linked: bool,
        active_low: bool,
        cpu: Hertz,
    ) {
        let period = pwm3.period();
        let per_bit16 = (u64::from(sampler.rate().0) * 16 / u64::from(pwm3.frequency().0)) as u32;
        let count = (per_bit16 as usize * (BITS + 8)) / 16;
        let result = (|| -> Result<(), Fail> {
            lane3
                .arm(&pwm::duties(patterns.0, period))
                .map_err(|_| Fail::Count { expected: BITS as u32, got: 0 })?;
            lane4
                .arm(&pwm::duties(patterns.1, period))
                .map_err(|_| Fail::Count { expected: BITS as u32, got: 0 })?;
            lane23
                .arm(&pwm::duties(patterns.2, period))
                .map_err(|_| Fail::Count { expected: BITS as u32, got: 0 })?;
            lane22
                .arm(&pwm::duties(patterns.3, period))
                .map_err(|_| Fail::Count { expected: BITS as u32, got: 0 })?;
            sampler.start(count).map_err(|_| Fail::Count {
                expected: count as u32,
                got: 0,
            })?;
            pwm3.stage(0b1100);
            pwm2.stage(0b0110);
            pwm3.start();
            if !linked {
                pwm2.start();
            }
            let budget = cpu.0 / 100;
            let done3 = pwm::wait(budget, || lane3.poll());
            let done4 = pwm::wait(budget, || lane4.poll());
            let done23 = pwm::wait(budget, || lane23.poll());
            let done22 = pwm::wait(budget, || lane22.poll());
            let sampled = pwm::wait(budget, || sampler.poll());
            pwm3.stop();
            pwm2.stop();
            pwm3.close_requests();
            pwm2.close_requests();
            let finished = [done3, done4, done23, done22]
                .iter()
                .filter(|done| matches!(done, Some(Ok(()))))
                .count();
            if finished != 4 {
                lane3.abort();
                lane4.abort();
                lane23.abort();
                lane22.abort();
                return Err(Fail::Count {
                    expected: 4,
                    got: finished as u32,
                });
            }
            match sampled {
                Some(Ok(_)) => Ok(()),
                _ => Err(Fail::Count {
                    expected: count as u32,
                    got: 0,
                }),
            }
        })();
        let finished_ok = result.is_ok();
        report.check(name, "all four lanes and the sampler finish [PWM-003]", result);
        if !finished_ok {
            sampler.stop();
            return;
        }
        report.check(
            name,
            "the TIM3 CH3 lane's 16 bits, one period apart [PWM-002]",
            pwm::decode(sampler.samples(), 0, patterns.0, per_bit16, active_low),
        );
        report.check(
            name,
            "the TIM3 CH4 lane's 16 bits, one period apart [PWM-002]",
            pwm::decode(sampler.samples(), 1, patterns.1, per_bit16, active_low),
        );
        report.check(
            name,
            "the TIM2 CH3 lane's 16 bits, one period apart [PWM-002]",
            pwm::decode(sampler.samples(), 10, patterns.2, per_bit16, active_low),
        );
        report.check(
            name,
            "the TIM2 CH2 lane's 16 bits, one period apart [PWM-002]",
            pwm::decode(sampler.samples(), 3, patterns.3, per_bit16, active_low),
        );
    }

    #[task(priority = 1, local = [run, lane3, lane4, lane23, lane22, samples])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let (Ok(mut pwm3), Ok(mut pwm2)) = (
            Pwm::new(&mut run.rcc, run.tim3, Hertz::khz(600)),
            Pwm::new(&mut run.rcc, run.tim2, Hertz::khz(600)),
        ) else {
            defmt::panic!("motor timers")
        };
        let (Ok(mut lane3), Ok(mut lane4), Ok(mut lane23), Ok(mut lane22)) = (
            pwm3
                .channel::<3, _>(run.ch3)
                .into_lane(run.lane3, core::mem::take(cx.local.lane3)),
            pwm3
                .channel::<4, _>(run.ch4)
                .into_lane(run.lane4, core::mem::take(cx.local.lane4)),
            pwm2
                .channel::<3, _>(run.ch23)
                .into_lane(run.lane23, core::mem::take(cx.local.lane23)),
            pwm2
                .channel::<2, _>(run.ch22)
                .into_lane(run.lane22, core::mem::take(cx.local.lane22)),
        ) else {
            defmt::panic!("motor lanes")
        };
        let Ok(mut sampler) = Sampler::new::<_, _, 'B'>(
            &mut run.rcc,
            run.tim1,
            run.sampler,
            Hertz::mhz(1),
            core::mem::take(cx.local.samples),
        ) else {
            defmt::panic!("the TIM1 sampler")
        };
        if let Err(error) = sampler.set_rate(Hertz(pwm3.frequency().0 * 12)) {
            report.check(
                "run R",
                "the sampler at 12 times the bit rate [PWM-003]",
                Err(Fail::Timer(error)),
            );
        } else {
            defmt::info!(
                "run R: four lanes at {}, sampled at {}",
                pwm3.frequency(),
                sampler.rate()
            );
            // Independent first: TIM2 still starts in software, so both start
            // modes come from the same fresh timers.
            four_lanes(
                &mut report,
                "independent start: TIM3 CH3/CH4 PB0/PB1, TIM2 CH3/CH4 PB10/PB11",
                &mut pwm3,
                &mut pwm2,
                &mut lane3,
                &mut lane4,
                &mut lane23,
                &mut lane22,
                (0x3c96, 0xe14d, 0x7b31, 0x84ce),
                &mut sampler,
                false,
                false,
                cpu,
            );
            // The trigger route, as the suite proves it: the slave sits still
            // until the master starts, then begins with it, 100 starts over.
            pwm3.lead();
            pwm2.follow(&pwm3);
            pwm::trigger_suite(&mut report, "TIM3 starts TIM2", &mut pwm3, &mut pwm2, 32);
            // Linked: the four lanes from one edge, the master's start
            // bringing the slave's in through its trigger.
            four_lanes(
                &mut report,
                "linked start: TIM3 CH3/CH4 PB0/PB1, TIM2 CH3/CH4 PB10/PB11",
                &mut pwm3,
                &mut pwm2,
                &mut lane3,
                &mut lane4,
                &mut lane23,
                &mut lane22,
                (0x5b29, 0xa5c3, 0xc37b, 0x31d9),
                &mut sampler,
                true,
                false,
                cpu,
            );
            // Active low, as bidirectional DShot idles the line: the same
            // four lanes, their pulses pulling low, decoded inverted.
            pwm3.set_active_low::<3>(true);
            pwm3.set_active_low::<4>(true);
            pwm2.set_active_low::<3>(true);
            pwm2.set_active_low::<2>(true);
            four_lanes(
                &mut report,
                "active low: TIM3 CH3/CH4 PB0/PB1, TIM2 CH3/CH4 PB10/PB11",
                &mut pwm3,
                &mut pwm2,
                &mut lane3,
                &mut lane4,
                &mut lane23,
                &mut lane22,
                (0x3c96, 0xe14d, 0x7b31, 0x84ce),
                &mut sampler,
                true,
                true,
                cpu,
            );
        }
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}