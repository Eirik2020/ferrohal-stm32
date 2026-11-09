//! Run U: each timer's second use, from a fresh boot. No links.
//!
//! Run H drops each timer's first owner once it has measured it, then needs a
//! second token for TIM4 at 8 kHz and for TIM2 and TIM5 as 1 MHz timebases.
//! This run gets those tokens from the board's own boot instead: nothing is
//! stolen, and the same three checks run on a clean `Peripherals`.

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

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SDIO])]
mod app {
    use ferrohal_stm32::{
        time::Hertz,
        timer::{Tick, Timebase},
    };

    use crate::{
        board,
        tests::{self, Fail, Report},
        timers::{self, TickBench},
    };

    #[shared]
    struct Shared {
        bench: TickBench,
    }

    #[local]
    struct Local {
        run: board::RunH,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_h(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::info!("run U, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {
                bench: TickBench::new(),
            },
            Local { run },
        )
    }

    #[task(binds = TIM4, priority = 2, shared = [bench])]
    fn tim4(mut cx: tim4::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(priority = 1, shared = [bench], local = [run])]
    async fn checks(mut cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let run = cx.local.run;
        let bench = &mut cx.shared.bench;

        let tick = run
            .tim4
            .take()
            .map(|timer| Tick::new(&mut run.rcc, timer, board::CONTROL_RATE));
        match tick {
            Some(Ok(tick)) => {
                timers::tick_check(&mut report, "TIM4 tick 8 kHz", bench, tick, cpu, 400)
            }
            Some(Err(refused)) => report.check(
                "TIM4 tick 8 kHz",
                "open",
                Err(Fail::Timer(refused.error())),
            ),
            None => defmt::unreachable!(),
        }
        macro_rules! base {
            ($name:literal, $timer:expr) => {
                match $timer.map(|timer| Timebase::new(&mut run.rcc, timer, Hertz::mhz(1))) {
                    Some(Ok(base)) => timers::timebase_check(&mut report, $name, &base, cpu),
                    Some(Err(refused)) => {
                        report.check($name, "open", Err(Fail::Timer(refused.error())))
                    }
                    None => defmt::unreachable!(),
                }
            };
        }
        base!("TIM2 timebase 1 MHz", run.tim2.take());
        base!("TIM5 timebase 1 MHz", run.tim5.take());
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
