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
//! Then the four lanes as one generator turn to read port B through one
//! reader paced by TIM1's update on DMA2 stream 5 (`docs/design/lanes.md`): every lane on
//! the port shares that reader, only the last lane raises its end-of-transfer
//! interrupt, and the read runs at the pacer's rate.
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
        chip::{TIM1, TIM2, TIM3},
        time::Hertz,
        timer::{CompareLane, Pwm, Sampler},
    };
    use stm32_metapac as pac;

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
        reads: [&'static mut [u16]; 4],
    }

    #[init(local = [
        lane3_buf: [u32; 20] = [0; 20],
        lane4_buf: [u32; 20] = [0; 20],
        lane23_buf: [u32; 20] = [0; 20],
        lane22_buf: [u32; 20] = [0; 20],
        samples_buf: [u16; 1024] = [0; 1024],
        read3_buf: [u16; 64] = [0; 64],
        read4_buf: [u16; 64] = [0; 64],
        read23_buf: [u16; 64] = [0; 64],
        read22_buf: [u16; 64] = [0; 64],
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
                reads: [
                    cx.local.read3_buf,
                    cx.local.read4_buf,
                    cx.local.read23_buf,
                    cx.local.read22_buf,
                ],
            },
        )
    }

    /// Arms the four lanes with their patterns, stages both timers, starts
    /// them — both in software, or TIM3 alone when `linked` (TIM2 starts from
    /// its trigger) — and decodes every lane's pin from the sampler.
    #[allow(clippy::too_many_arguments)]
    fn four_lanes<W3, W4, W23, W22, S1>(
        report: &mut Report,
        name: &str,
        pwm3: &mut Pwm<TIM3>,
        pwm2: &mut Pwm<TIM2>,
        lane3: &mut CompareLane<TIM3, 3, W3>,
        lane4: &mut CompareLane<TIM3, 4, W4>,
        lane23: &mut CompareLane<TIM2, 3, W23>,
        lane22: &mut CompareLane<TIM2, 2, W22>,
        patterns: (u16, u16, u16, u16),
        sampler: &mut Sampler<TIM1, S1>,
        linked: bool,
        active_low: bool,
        cpu: Hertz,
    ) {
        let period = pwm3.period();
        let per_bit16 = (u64::from(sampler.rate().0) * 16 / u64::from(pwm3.rate().0)) as u32;
        let count = (per_bit16 as usize * (BITS + 8)) / 16;
        let result = (|| -> Result<(), Fail> {
            crate::pwm::arm(lane3, pwm3, &pwm::duties(patterns.0, period))
                .map_err(|_| Fail::Count {
                    expected: BITS as u32,
                    got: 0,
                })?;
            crate::pwm::arm(lane4, pwm3, &pwm::duties(patterns.1, period))
                .map_err(|_| Fail::Count {
                    expected: BITS as u32,
                    got: 0,
                })?;
            crate::pwm::arm(lane23, pwm2, &pwm::duties(patterns.2, period))
                .map_err(|_| Fail::Count {
                    expected: BITS as u32,
                    got: 0,
                })?;
            crate::pwm::arm(lane22, pwm2, &pwm::duties(patterns.3, period))
                .map_err(|_| Fail::Count {
                    expected: BITS as u32,
                    got: 0,
                })?;
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
            let done3 = pwm::wait(budget, || lane3.on_interrupt());
            let done4 = pwm::wait(budget, || lane4.on_interrupt());
            let done23 = pwm::wait(budget, || lane23.on_interrupt());
            let done22 = pwm::wait(budget, || lane22.on_interrupt());
            let sampled = pwm::wait(budget, || sampler.on_interrupt());
            pwm3.stop();
            pwm2.stop();
            pwm3.close_requests();
            pwm2.close_requests();
            let finished = [done3, done4, done23, done22]
                .iter()
                .filter(|done| matches!(done, Some(Ok(()))))
                .count();
            if finished != 4 {
                lane3.abort(pwm3);
                lane4.abort(pwm3);
                lane23.abort(pwm2);
                lane22.abort(pwm2);
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
        report.check(
            name,
            "all four lanes and the sampler finish [PWM-003]",
            result,
        );
        if !finished_ok {
            sampler.abort();
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

    /// A pass, or a count of what was wrong.
    fn holds(ok: bool) -> Result<(), Fail> {
        if ok {
            Ok(())
        } else {
            Err(Fail::Count { expected: 1, got: 0 })
        }
    }

    #[task(priority = 1, local = [run, lane3, lane4, lane23, lane22, samples, reads])]
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
        // Paced lanes: they send on their own DMA1 streams, which cannot
        // reach GPIO (RM0090 figure 33), and read port B only through a
        // reader paced by another timer.
        let [r3, r4, r23, r22] = core::mem::replace(cx.local.reads, [&mut [], &mut [], &mut [], &mut []]);
        let (Ok(mut lane3), Ok(mut lane4), Ok(mut lane23), Ok(mut lane22)) = (
            ferrohal_stm32::timer::LaneWiring::<_, 3, _, _>::new(run.ch3, run.lane3)
                .assemble_paced::<'B'>(&pwm3).map(|mut lane| {
            crate::pwm::lend_frames(core::mem::take(cx.local.lane3));
            let _ = lane.lend_samples(r3);
            lane
        }),
            ferrohal_stm32::timer::LaneWiring::<_, 4, _, _>::new(run.ch4, run.lane4)
                .assemble_paced::<'B'>(&pwm3).map(|mut lane| {
            crate::pwm::lend_frames(core::mem::take(cx.local.lane4));
            let _ = lane.lend_samples(r4);
            lane
        }),
            ferrohal_stm32::timer::LaneWiring::<_, 3, _, _>::new(run.ch23, run.lane23)
                .assemble_paced::<'B'>(&pwm2).map(|mut lane| {
            crate::pwm::lend_frames(core::mem::take(cx.local.lane23));
            let _ = lane.lend_samples(r23);
            lane
        }),
            ferrohal_stm32::timer::LaneWiring::<_, 2, _, _>::new(run.ch22, run.lane22)
                .assemble_paced::<'B'>(&pwm2).map(|mut lane| {
            crate::pwm::lend_frames(core::mem::take(cx.local.lane22));
            let _ = lane.lend_samples(r22);
            lane
        }),
        ) else {
            defmt::panic!("motor lanes")
        };
        let Ok(mut sampler) = Sampler::new::<'B'>(
            &mut run.rcc,
            run.tim1,
            run.sampler,
            Hertz::mhz(1),
            core::mem::take(cx.local.samples),
        ) else {
            defmt::panic!("the TIM1 sampler")
        };
        if let Err(error) = sampler.set_rate(Hertz(pwm3.rate().0 * 12)) {
            report.check(
                "run R",
                "the sampler at 12 times the bit rate [PWM-003]",
                Err(Fail::Timer(error)),
            );
        } else {
            defmt::info!(
                "run R: four lanes at {}, sampled at {}",
                pwm3.rate(),
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
            // The four lanes as one generator, TIM3 starting TIM2 through its
            // trigger: every lane of both finishes.
            match ferrohal_stm32::timer::Awg::linked(pwm3, pwm2, (lane3, lane4), (lane23, lane22)) {
                Ok(mut awg) => {
                    pwm::awg_pair_check(&mut report, "TIM3 starts TIM2, linked", &mut awg, cpu);
                    // TIM1 leaves the sampler to pace the lanes' one reader.
                    let paced = match sampler.free() {
                        Ok((tim1, stream, _)) => Pwm::new(&mut run.rcc, tim1, Hertz::mhz(4))
                            .map(|pacer| (pacer, stream))
                            .map_err(|refused| Fail::Timer(refused.error())),
                        Err(_) => Err(Fail::Count { expected: 1, got: 0 }),
                    };
                    let name = "four lanes on TIM3 and TIM2, port B, paced by TIM1";
                    match paced {
                        Ok((pacer, stream)) => {
                            let registered = awg.read_ahead_paced::<_, _, 'B'>(&pacer, stream);
                            pacer.start();
                            report.check(name, "a reader paced by TIM1 registers [AWG-017]", holds(registered.is_ok()));
                            let rate = awg.prepare_listen(pacer.rate(), 64);
                            report.check(name, "the paced reader reads at its pacer's rate [AWG-017]",
                                holds(rate.ok() == Some(pacer.rate())));
                            report.check(name, "every lane on port B shares the one reader [AWG-014]",
                                holds((0..4).all(|lane| awg.reader_of(lane) == 0)));
                            let period = awg.period();
                            let frames = [
                                [period / 4, period / 2, period / 4],
                                [period / 2, period / 4, period / 2],
                                [period / 4, period / 4, period / 2],
                                [period / 2, period / 2, period / 4],
                            ];
                            let loaded = (0..4).all(|lane| crate::pwm::load(&mut awg, lane, &frames[lane]).is_ok());
                            awg.listen_on_end();
                            let played = loaded && awg.play().is_ok();
                            // Only the last lane (TIM2 CH2, DMA1 stream 6)
                            // raises its end: streams 7, 2 and 1 are quiet.
                            let tcie = [7usize, 2, 1, 6].map(|n| pac::DMA1.st(n).cr().read().tcie());
                            report.check(name, "only the last lane raises its end-of-transfer interrupt [AWG-020]",
                                holds(played && tcie == [false, false, false, true]));
                            let budget = cpu.0 / 100;
                            let ended = played && matches!(pwm::wait(budget, || awg.on_interrupt()), Some(Ok(_)));
                            let read = ended && matches!(pwm::wait(budget, || awg.on_interrupt()), Some(Ok(_)));
                            let first = awg.samples(0);
                            let shared = read
                                && first.len() == 64
                                && (1..4).all(|lane| awg.samples(lane).as_ptr() == first.as_ptr()
                                    && awg.samples(lane).len() == first.len());
                            report.check(name, "every lane's frame ends, and every lane's samples are the one reader's 64 port words [AWG-014 AWG-020]",
                                holds(shared));
                            let _ = awg.speak();
                            pacer.stop();
                        }
                        Err(fail) => report.check(name, "the TIM1 pacer [AWG-017]", Err(fail)),
                    }
                }
                Err(refused) => report.check(
                    "TIM3 starts TIM2, linked",
                    "AWG: every lane on both timers finishes [AWG-005]",
                    Err(Fail::Timer(refused.error())),
                ),
            }
        }
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
