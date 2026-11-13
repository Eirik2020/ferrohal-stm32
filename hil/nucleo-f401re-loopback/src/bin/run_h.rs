//! Run H: every timer the F401 has, against the DWT cycle counter. No links.
//!
//! Each timer ticks at 1 kHz and must keep that rate to 0.05 %, then stop
//! when paused. The run then frees TIM4 and reopens it at 8 kHz, FerroWasp's
//! control loop rate, and frees TIM2 and TIM5 to reopen them as 1 MHz
//! timebases — each measured again, from the token `free` hands back.
//!
//! First, the voltage scale. Before `freeze`, `init` sets scale 3, as a
//! bootloader might have left it: VOS 0b01 in RM0368's two bits, harmless on
//! the reset clock. `freeze` must replace it, and the run reads back scale 2,
//! VOS 0b10, with VOSRDY set. Scale 2 is also the reset value, so without the
//! lower scale the readback couldn't show `freeze` wrote anything.
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

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SDIO])]
mod app {
    use ferrohal_stm32::{
        chip::{TIM1, TIM2, TIM3, TIM4, TIM5, TIM9, TIM10, TIM11},
        time::Hertz,
        timer::{Tick, Timebase},
    };

    use crate::{
        board,
        tests::{self, Fail, Report},
        timers::{self, TickBench, benched_ticks},
    };

    benched_ticks! {
        /// The timers run H ticks, one at a time. The token stays inside.
        Benched {
            Tim1: TIM1,
            Tim2: TIM2,
            Tim3: TIM3,
            Tim4: TIM4,
            Tim5: TIM5,
            Tim9: TIM9,
            Tim10: TIM10,
            Tim11: TIM11,
        }
    }

    #[shared]
    struct Shared {
        bench: TickBench<Benched>,
    }

    #[local]
    struct Local {
        run: board::RunH,
        vos_before_freeze: u32,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        // The PLL is off after reset, as RM0368 needs for a write to VOS.
        stm32_metapac::RCC.apb1enr().modify(|w| w.set_pwren(true));
        let _ = stm32_metapac::RCC.apb1enr().read();
        stm32_metapac::PWR
            .cr1()
            .modify(|w| w.set_vos(stm32_metapac::pwr::vals::Vos::SCALE3));
        let vos_before_freeze = u32::from(stm32_metapac::PWR.cr1().read().vos().to_bits());
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
            Local {
                run,
                vos_before_freeze,
            },
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

    #[task(priority = 1, shared = [bench], local = [run, vos_before_freeze])]
    async fn checks(mut cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let run = cx.local.run;
        let bench = &mut cx.shared.bench;
        let khz = Hertz::khz(1);

        let before = *cx.local.vos_before_freeze;
        defmt::info!("PWR_CR VOS before freeze {=u32:#04b}", before);
        report.check_for(
            "PWR",
            "RCC-003",
            "scale 3 (VOS 0b01) set before freeze, for it to replace",
            if before == 0b01 {
                Ok(())
            } else {
                Err(Fail::Count {
                    expected: 0b01,
                    got: before,
                })
            },
        );
        // RM0368: PWR_CR VOS is bits 15:14, PWR_CSR VOSRDY bit 14.
        let vos = u32::from(stm32_metapac::PWR.cr1().read().vos().to_bits());
        let ready = stm32_metapac::PWR.csr1().read().vosrdy();
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
            ($name:literal, $variant:ident, $timer:expr, $rate:expr, $ticks:expr) => {
                match $timer.map(|timer| Tick::new(&mut run.rcc, timer, $rate)) {
                    Some(Ok(tick)) => Ok(timers::tick_check(
                        &mut report,
                        $name,
                        bench,
                        Benched::$variant(tick),
                        cpu,
                        $ticks,
                    )),
                    Some(Err(refused)) => {
                        report.check($name, "open", Err(Fail::Timer(refused.error())));
                        Err(())
                    }
                    None => defmt::unreachable!(),
                }
            };
        }
        let _ = tick!("TIM1 tick 1 kHz", Tim1, run.tim1.take(), khz, 50);
        let tim2 = tick!("TIM2 tick 1 kHz", Tim2, run.tim2.take(), khz, 50);
        let _ = tick!("TIM3 tick 1 kHz", Tim3, run.tim3.take(), khz, 50);
        let tim4 = tick!("TIM4 tick 1 kHz", Tim4, run.tim4.take(), khz, 50);
        let tim5 = tick!("TIM5 tick 1 kHz", Tim5, run.tim5.take(), khz, 50);
        let _ = tick!("TIM9 tick 1 kHz", Tim9, run.tim9.take(), khz, 50);
        let _ = tick!("TIM10 tick 1 kHz", Tim10, run.tim10.take(), khz, 50);
        let _ = tick!("TIM11 tick 1 kHz", Tim11, run.tim11.take(), khz, 50);

        // The second use, from the token `free` hands back: nothing is
        // stolen, and no fresh boot is needed. A first use that was refused
        // lost its token with its refusal, so it reports and stops here.
        if let Ok(Benched::Tim4(tick)) = tim4 {
            let token = tick.free();
            match Tick::new(&mut run.rcc, token, board::CONTROL_RATE) {
                Ok(tick) => {
                    let _ = timers::tick_check(
                        &mut report,
                        "TIM4 tick 8 kHz",
                        bench,
                        Benched::Tim4(tick),
                        cpu,
                        400,
                    );
                }
                Err(refused) => {
                    report.check("TIM4 tick 8 kHz", "open", Err(Fail::Timer(refused.error())))
                }
            }
        }
        macro_rules! base {
            ($name:literal, $second:expr, $variant:ident) => {
                if let Ok(Benched::$variant(tick)) = $second {
                    let token = tick.free();
                    match Timebase::new(&mut run.rcc, token, Hertz::mhz(1)) {
                        Ok(base) => timers::timebase_check(&mut report, $name, &base, cpu),
                        Err(refused) => {
                            report.check($name, "open", Err(Fail::Timer(refused.error())))
                        }
                    }
                }
            };
        }
        base!("TIM2 timebase 1 MHz", tim2, Tim2);
        base!("TIM5 timebase 1 MHz", tim5, Tim5);
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
