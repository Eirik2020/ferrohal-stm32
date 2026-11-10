//! Run I: every timer the H753ZI has, against the DWT cycle counter. No
//! links.
//!
//! Each timer ticks at 1 kHz and must keep that rate to 0.05 %, then stop
//! when paused. Run T takes the second use of TIM4 (8 kHz, the Lucid's
//! control loop rate) and of TIM2 and TIM5 (1 MHz timebases; TIM2 is the
//! Lucid's) on a fresh boot, so no token is stolen.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `TIM1_UP`, `TIM2` to `TIM5`, `TIM6_DAC`, `TIM7`, `TIM8_UP_TIM13`, `TIM8_BRK_TIM12`, `TIM8_TRG_COM_TIM14`, `TIM15` to `TIM17` | the running `Tick` | 2 |
//! | `CEC` | RTIC, dispatching the priority 1 `checks` task | 1 |

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;
#[path = "../../../common/timers.rs"]
mod timers;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [CEC])]
mod app {
    use ferrohal_stm32::{
        chip::{
            TIM1, TIM12, TIM13, TIM14, TIM15, TIM16, TIM17, TIM2, TIM3, TIM4, TIM5, TIM6, TIM7,
            TIM8,
        },
        time::Hertz,
        timer::Tick,
    };

    use crate::{
        board,
        tests::{self, Fail, Report},
        timers::{self, TickBench, benched_ticks},
    };

    benched_ticks! {
        /// The timers run I ticks, one at a time. The token stays inside.
        Benched {
            Tim1: TIM1,
            Tim2: TIM2,
            Tim3: TIM3,
            Tim4: TIM4,
            Tim5: TIM5,
            Tim6: TIM6,
            Tim7: TIM7,
            Tim8: TIM8,
            Tim12: TIM12,
            Tim13: TIM13,
            Tim14: TIM14,
            Tim15: TIM15,
            Tim16: TIM16,
            Tim17: TIM17,
        }
    }

    #[shared]
    struct Shared {
        bench: TickBench<Benched>,
    }

    #[local]
    struct Local {
        run: board::RunI,
    }

    #[init]
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
            Shared {
                bench: TickBench::new(),
            },
            Local { run },
        )
    }

    #[task(binds = TIM1_UP, priority = 2, shared = [bench])]
    fn tim1_up(mut cx: tim1_up::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM2, priority = 2, shared = [bench])]
    fn tim2(mut cx: tim2::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM3, priority = 2, shared = [bench])]
    fn tim3(mut cx: tim3::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM4, priority = 2, shared = [bench])]
    fn tim4(mut cx: tim4::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM5, priority = 2, shared = [bench])]
    fn tim5(mut cx: tim5::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM6_DAC, priority = 2, shared = [bench])]
    fn tim6_dac(mut cx: tim6_dac::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM7, priority = 2, shared = [bench])]
    fn tim7(mut cx: tim7::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM8_UP_TIM13, priority = 2, shared = [bench])]
    fn tim8_up_tim13(mut cx: tim8_up_tim13::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM8_BRK_TIM12, priority = 2, shared = [bench])]
    fn tim8_brk_tim12(mut cx: tim8_brk_tim12::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM8_TRG_COM_TIM14, priority = 2, shared = [bench])]
    fn tim8_trg_com_tim14(mut cx: tim8_trg_com_tim14::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM15, priority = 2, shared = [bench])]
    fn tim15(mut cx: tim15::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM16, priority = 2, shared = [bench])]
    fn tim16(mut cx: tim16::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM17, priority = 2, shared = [bench])]
    fn tim17(mut cx: tim17::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(priority = 1, shared = [bench], local = [run])]
    async fn checks(mut cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let run = cx.local.run;
        let bench = &mut cx.shared.bench;
        let khz = Hertz::khz(1);

        macro_rules! tick {
            ($name:literal, $variant:ident, $timer:expr, $rate:expr, $ticks:expr) => {
                match $timer.map(|timer| Tick::new(&mut run.rcc, timer, $rate)) {
                    Some(Ok(tick)) => {
                        timers::tick_check(&mut report, $name, bench, Benched::$variant(tick), cpu, $ticks)
                    }
                    Some(Err(refused)) => report.check($name, "open", Err(Fail::Timer(refused.error()))),
                    None => defmt::unreachable!(),
                }
            };
        }
        tick!("TIM1 tick 1 kHz", Tim1, run.tim1.take(), khz, 50);
        tick!("TIM2 tick 1 kHz", Tim2, run.tim2.take(), khz, 50);
        tick!("TIM3 tick 1 kHz", Tim3, run.tim3.take(), khz, 50);
        tick!("TIM4 tick 1 kHz", Tim4, run.tim4.take(), khz, 50);
        tick!("TIM5 tick 1 kHz", Tim5, run.tim5.take(), khz, 50);
        tick!("TIM6 tick 1 kHz", Tim6, run.tim6.take(), khz, 50);
        tick!("TIM7 tick 1 kHz", Tim7, run.tim7.take(), khz, 50);
        tick!("TIM8 tick 1 kHz", Tim8, run.tim8.take(), khz, 50);
        tick!("TIM12 tick 1 kHz", Tim12, run.tim12.take(), khz, 50);
        tick!("TIM13 tick 1 kHz", Tim13, run.tim13.take(), khz, 50);
        tick!("TIM14 tick 1 kHz", Tim14, run.tim14.take(), khz, 50);
        tick!("TIM15 tick 1 kHz", Tim15, run.tim15.take(), khz, 50);
        tick!("TIM16 tick 1 kHz", Tim16, run.tim16.take(), khz, 50);
        tick!("TIM17 tick 1 kHz", Tim17, run.tim17.take(), khz, 50);
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
