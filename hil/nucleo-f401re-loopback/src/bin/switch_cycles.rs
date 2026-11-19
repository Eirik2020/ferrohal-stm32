//! Run X: the prepared switch, built with `profile-listen`.
//!
//! Run R's four lanes (TIM3 CH3/CH4 on PB0/PB1, TIM2 CH3/CH2 on PB10/PB3,
//! DMA1 streams 7, 2, 1 and 6) as one generator, TIM3 starting TIM2 through
//! its trigger, and one reader of port B paced by TIM1 on DMA2 stream 5, as
//! run R ends. Two listens are prepared, the second with another count: every
//! exchange must read the second's count at the rate it returned, and the
//! switch's cycle count (`timer::profile`, the listen whole) must stay within
//! an eighth of its least over every exchange, since a prepared listen only
//! stores what was computed ahead (`docs/design/lanes.md`). No links: the
//! reader reads the lanes' own pins.
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
        timer::{Awg, LaneWiring, Pwm, profile},
    };

    use crate::{
        board, pwm,
        tests::{self, Fail, Report},
    };

    /// Exchanges whose switch is timed.
    const EXCHANGES: u32 = 200;
    /// The count prepared first, then the one prepared last.
    const FIRST: usize = 32;
    const LAST: usize = 64;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunR>,
        lanes: [&'static mut [u32]; 4],
        reads: [&'static mut [u16]; 4],
    }

    #[init(local = [
        lane3_buf: [u32; 20] = [0; 20],
        lane4_buf: [u32; 20] = [0; 20],
        lane23_buf: [u32; 20] = [0; 20],
        lane22_buf: [u32; 20] = [0; 20],
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
        defmt::info!("run X, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {},
            Local {
                run: Some(run),
                lanes: [
                    cx.local.lane3_buf,
                    cx.local.lane4_buf,
                    cx.local.lane23_buf,
                    cx.local.lane22_buf,
                ],
                reads: [
                    cx.local.read3_buf,
                    cx.local.read4_buf,
                    cx.local.read23_buf,
                    cx.local.read22_buf,
                ],
            },
        )
    }

    /// A pass, or a count of what was wrong.
    fn holds(ok: bool) -> Result<(), Fail> {
        if ok {
            Ok(())
        } else {
            Err(Fail::Count { expected: 1, got: 0 })
        }
    }

    #[task(priority = 1, local = [run, lanes, reads])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let name = "four lanes on TIM3 and TIM2, port B, paced by TIM1";
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let (Ok(pwm3), Ok(pwm2)) = (
            Pwm::new(&mut run.rcc, run.tim3, Hertz::khz(600)),
            Pwm::new(&mut run.rcc, run.tim2, Hertz::khz(600)),
        ) else {
            defmt::panic!("motor timers")
        };
        let Ok(pacer) = Pwm::new(&mut run.rcc, run.tim1, Hertz::mhz(4)) else {
            defmt::panic!("the TIM1 pacer")
        };
        let [l3, l4, l23, l22] = core::mem::replace(cx.local.lanes, [&mut [], &mut [], &mut [], &mut []]);
        let [r3, r4, r23, r22] = core::mem::replace(cx.local.reads, [&mut [], &mut [], &mut [], &mut []]);
        let (Ok(lane3), Ok(lane4), Ok(lane23), Ok(lane22)) = (
            LaneWiring::<_, 3, _, _>::new(run.ch3, run.lane3).assemble_paced::<'B'>(&pwm3, l3, r3),
            LaneWiring::<_, 4, _, _>::new(run.ch4, run.lane4).assemble_paced::<'B'>(&pwm3, l4, r4),
            LaneWiring::<_, 3, _, _>::new(run.ch23, run.lane23).assemble_paced::<'B'>(&pwm2, l23, r23),
            LaneWiring::<_, 2, _, _>::new(run.ch22, run.lane22).assemble_paced::<'B'>(&pwm2, l22, r22),
        ) else {
            defmt::panic!("motor lanes")
        };
        pwm3.lead();

        let mut pwm2 = pwm2;
        pwm2.follow(&pwm3);
        let mut awg = match Awg::linked(pwm3, pwm2, (lane3, lane4), (lane23, lane22)) {
            Ok(awg) => awg,
            Err(_) => defmt::panic!("the generator"),
        };
        let registered = awg.read_ahead_paced::<_, _, 'B'>(&pacer, run.sampler);
        pacer.start();
        report.check(name, "a reader paced by TIM1 registers [AWG-017]", holds(registered.is_ok()));
        let first = awg.prepare_listen(pacer.rate(), FIRST);
        let last = awg.prepare_listen(pacer.rate(), LAST);
        report.check(
            name,
            "the last prepare_listen is the one prepared, at the rate it returned [AWG-013]",
            holds(first.is_ok() && matches!((last, awg.prepared()),
                (Ok(rate), Some((_, LAST, at))) if at == rate)),
        );
        let period = awg.period();
        let frames = [
            [period / 4, period / 2, period / 4],
            [period / 2, period / 4, period / 2],
            [period / 4, period / 4, period / 2],
            [period / 2, period / 2, period / 4],
        ];
        profile::clear();
        let budget = cpu.0 / 100;
        let mut whole = 0;
        for _ in 0..EXCHANGES {
            let loaded = (0..4).all(|lane| awg.load(lane, &frames[lane]).is_ok());
            awg.listen_on_end();
            let played = loaded && awg.play().is_ok();
            // The switch runs inside the call that sees the last lane end,
            // so the read's end is the first event a call reports that began
            // already listening; the second timer's lanes may end in a call
            // of their own before that.
            let read = played
                && matches!(
                    pwm::wait(budget, || {
                        let listening = awg.is_listening();
                        match awg.on_interrupt() {
                            Some(Ok(_)) if listening => Some(Ok(())),
                            Some(Err(error)) => Some(Err(error)),
                            _ => None,
                        }
                    }),
                    Some(Ok(()))
                );
            if read && (0..4).all(|lane| awg.samples(lane).len() == LAST) {
                whole += 1;
            }
            let _ = awg.speak();
        }
        report.check(
            name,
            "every exchange reads the last prepared count [AWG-013]",
            if whole == EXCHANGES {
                Ok(())
            } else {
                Err(Fail::Count { expected: EXCHANGES, got: whole })
            },
        );
        let (least, mean, most, count) = profile::STAGES[0].get();
        defmt::info!(
            "listen whole: least {} cycles, mean {}, most {}, over {}",
            least,
            mean,
            most,
            count
        );
        report.check(
            name,
            "every switch takes the same cycles, within an eighth of the least [AWG-013]",
            holds(count == EXCHANGES && most - least <= least / 8),
        );
        pacer.stop();
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
