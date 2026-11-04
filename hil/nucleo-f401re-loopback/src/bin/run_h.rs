//! Run H: every timer the F401 has, against the DWT cycle counter. No links.
//!
//! Each timer ticks at 1 kHz (TIM4 also at 8 kHz, FerroWasp's control loop
//! rate) and must keep that rate to 0.05 %, then stop when paused. TIM2 and
//! TIM5, the 32-bit timers, also run as 1 MHz timebases.
//!
//! First, the voltage scale `freeze` wrote is read back: scale 2, VOS 0b10
//! in RM0368's two bits, with VOSRDY set.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `TIM1_UP_TIM10`, `TIM2`, `TIM3`, `TIM4`, `TIM5`, `TIM1_BRK_TIM9`, `TIM1_TRG_COM_TIM11` | the running `Tick` | 2 |
//! | `SDIO` | RTIC, dispatching the priority 1 `checks` task | 1 |

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

/// PWR_CR and PWR_CSR (RM0368): the voltage scale, VOS (bits 15:14), and
/// its ready flag, VOSRDY (bit 14).
const PWR_CR: *const u32 = 0x4000_7000 as *const u32;
const PWR_CSR: *const u32 = 0x4000_7004 as *const u32;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SDIO])]
mod app {
    use core::ptr::read_volatile;

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
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run H, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {
                bench: TickBench::new(),
            },
            Local { run },
        )
    }

    #[task(binds = TIM1_UP_TIM10, priority = 2, shared = [bench])]
    fn tim1_up_tim10(mut cx: tim1_up_tim10::Context) {
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

    #[task(binds = TIM1_BRK_TIM9, priority = 2, shared = [bench])]
    fn tim1_brk_tim9(mut cx: tim1_brk_tim9::Context) {
        cx.shared.bench.lock(TickBench::on_interrupt);
    }

    #[task(binds = TIM1_TRG_COM_TIM11, priority = 2, shared = [bench])]
    fn tim1_trg_com_tim11(mut cx: tim1_trg_com_tim11::Context) {
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

        // SAFETY: reads of PWR's control and status registers.
        let (pwr_cr, pwr_csr) =
            unsafe { (read_volatile(crate::PWR_CR), read_volatile(crate::PWR_CSR)) };
        let vos = (pwr_cr >> 14) & 0b11;
        let ready = pwr_csr & (1 << 14) != 0;
        defmt::info!("PWR_CR VOS {=u32:#04b}, PWR_CSR VOSRDY {=bool}", vos, ready);
        report.check_for(
            "PWR",
            "RCC-003",
            "voltage scale 2 (VOS 0b10) and VOSRDY set",
            if vos != 0b10 {
                Err(Fail::Count {
                    expected: 0b10,
                    got: vos,
                })
            } else if !ready {
                Err(Fail::Level {
                    expected_high: true,
                })
            } else {
                Ok(())
            },
        );

        macro_rules! tick {
            ($name:literal, $timer:expr, $rate:expr, $ticks:expr) => {
                match $timer.map(|timer| Tick::new(&mut run.rcc, timer, $rate)) {
                    Some(Ok(tick)) => {
                        timers::tick_check(&mut report, $name, bench, tick, cpu, $ticks)
                    }
                    Some(Err(error)) => report.check($name, "open", Err(Fail::Timer(error))),
                    None => defmt::unreachable!(),
                }
            };
        }
        tick!("TIM1 tick 1 kHz", run.tim1.take(), khz, 50);
        tick!("TIM2 tick 1 kHz", run.tim2.take(), khz, 50);
        tick!("TIM3 tick 1 kHz", run.tim3.take(), khz, 50);
        tick!("TIM4 tick 1 kHz", run.tim4.take(), khz, 50);
        tick!("TIM5 tick 1 kHz", run.tim5.take(), khz, 50);
        tick!("TIM9 tick 1 kHz", run.tim9.take(), khz, 50);
        tick!("TIM10 tick 1 kHz", run.tim10.take(), khz, 50);
        tick!("TIM11 tick 1 kHz", run.tim11.take(), khz, 50);
        // Second uses: each timer's first owner above has been paused and
        // dropped, so a stolen token is the only one in use.
        // SAFETY: as above.
        let tim4 = unsafe { ferrohal_stm32::chip::TIM4::steal() };
        tick!("TIM4 tick 8 kHz", Some(tim4), board::CONTROL_RATE, 400);
        // SAFETY: as above.
        let (tim2, tim5) = unsafe {
            (
                ferrohal_stm32::chip::TIM2::steal(),
                ferrohal_stm32::chip::TIM5::steal(),
            )
        };
        for (name, base) in [
            (
                "TIM2 timebase 1 MHz",
                Timebase::new(&mut run.rcc, tim2, Hertz::mhz(1)),
            ),
            (
                "TIM5 timebase 1 MHz",
                Timebase::new(&mut run.rcc, tim5, Hertz::mhz(1)),
            ),
        ] {
            match base {
                Ok(base) => timers::timebase_check(&mut report, name, &base, cpu),
                Err(error) => report.check(name, "open", Err(Fail::Timer(error))),
            }
        }
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
