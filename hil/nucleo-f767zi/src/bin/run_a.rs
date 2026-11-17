//! Run A: the clock tree and every timer, against the DWT cycle counter. No
//! links.
//!
//! First the regulator. Before `freeze`, `init` sets voltage scale 3 (VOS
//! 0b01), as a bootloader might have left it, harmless on the reset clock with
//! the PLL off. `freeze` must replace it with scale 1 (VOS 0b11) and see
//! VOSRDY [RCC-003], and, since the tree runs APB1 at 54 MHz and APB2 at 108,
//! past their normal-mode limits, put the regulator in over-drive: ODEN and
//! ODSWEN set, ODRDY and ODSWRDY reported [RCC-F7-001].
//!
//! Then each timer ticks at 1 kHz and must keep that rate to 0.05 %, then stop
//! when paused. The run then frees TIM4 and reopens it at 8 kHz, and frees
//! TIM2 and TIM5 to reopen them as 1 MHz timebases, each measured again, from
//! the token `free` hands back.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `TIM1_UP_TIM10`, `TIM2` to `TIM5`, `TIM1_BRK_TIM9`, `TIM1_TRG_COM_TIM11`, `TIM8_UP_TIM13`, `TIM8_BRK_TIM12`, `TIM8_TRG_COM_TIM14` | the running `Tick` | 2 |
//! | `SPDIF_RX` | RTIC, dispatching the priority 1 `checks` task | 1 |

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

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPDIF_RX])]
mod app {
    use ferrohal_stm32::{
        chip::{TIM1, TIM2, TIM3, TIM4, TIM5, TIM8, TIM9, TIM10, TIM11, TIM12, TIM13, TIM14},
        time::Hertz,
        timer::{Tick, Timebase},
    };

    use crate::{
        board,
        tests::{self, Fail, Report},
        timers::{self, TickBench, benched_ticks},
    };

    benched_ticks! {
        /// The timers run A ticks, one at a time. The token stays inside.
        Benched {
            Tim1: TIM1,
            Tim2: TIM2,
            Tim3: TIM3,
            Tim4: TIM4,
            Tim5: TIM5,
            Tim8: TIM8,
            Tim9: TIM9,
            Tim10: TIM10,
            Tim11: TIM11,
            Tim12: TIM12,
            Tim13: TIM13,
            Tim14: TIM14,
        }
    }

    #[shared]
    struct Shared {
        bench: TickBench<Benched>,
    }

    #[local]
    struct Local {
        run: board::RunA,
        vos_before_freeze: u32,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        // The PLL is off after reset, as RM0410 4.1.6 needs for a write to VOS.
        stm32_metapac::RCC.apb1enr().modify(|w| w.set_pwren(true));
        let _ = stm32_metapac::RCC.apb1enr().read();
        stm32_metapac::PWR
            .cr1()
            .modify(|w| w.set_vos(stm32_metapac::pwr::vals::Vos::SCALE3));
        let vos_before_freeze = u32::from(stm32_metapac::PWR.cr1().read().vos().to_bits());
        let run = match board::run_a(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run A, clocks: {}", board::CLOCKS);
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
        // RM0410 4.4.1 and 4.4.2: PWR_CR1 VOS bits 15:14, ODEN 16, ODSWEN 17;
        // PWR_CSR1 VOSRDY 14, ODRDY 16, ODSWRDY 17.
        let cr1 = stm32_metapac::PWR.cr1().read();
        let csr1 = stm32_metapac::PWR.csr1().read();
        let vos = u32::from(cr1.vos().to_bits());
        defmt::info!(
            "PWR_CR1 VOS {=u32:#04b} ODEN {=bool} ODSWEN {=bool}; PWR_CSR1 VOSRDY {=bool} ODRDY {=bool} ODSWRDY {=bool}",
            vos,
            cr1.oden(),
            cr1.odswen(),
            csr1.vosrdy(),
            csr1.odrdy(),
            csr1.odswrdy()
        );
        report.check_for(
            "PWR",
            "RCC-003",
            "voltage scale 1 (VOS 0b11) and VOSRDY set",
            if vos != 0b11 {
                Err(Fail::Count {
                    expected: 0b11,
                    got: vos,
                })
            } else if !csr1.vosrdy() {
                Err(Fail::Level {
                    expected_high: true,
                })
            } else {
                Ok(())
            },
        );
        report.check_for(
            "PWR",
            "RCC-F7-001",
            "over-drive on (ODEN, ODSWEN) and ready (ODRDY, ODSWRDY)",
            if cr1.oden() && cr1.odswen() && csr1.odrdy() && csr1.odswrdy() {
                Ok(())
            } else {
                Err(Fail::Level {
                    expected_high: true,
                })
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
        let _ = tick!("TIM8 tick 1 kHz", Tim8, run.tim8.take(), khz, 50);
        let _ = tick!("TIM9 tick 1 kHz", Tim9, run.tim9.take(), khz, 50);
        let _ = tick!("TIM10 tick 1 kHz", Tim10, run.tim10.take(), khz, 50);
        let _ = tick!("TIM11 tick 1 kHz", Tim11, run.tim11.take(), khz, 50);
        let _ = tick!("TIM12 tick 1 kHz", Tim12, run.tim12.take(), khz, 50);
        let _ = tick!("TIM13 tick 1 kHz", Tim13, run.tim13.take(), khz, 50);
        let _ = tick!("TIM14 tick 1 kHz", Tim14, run.tim14.take(), khz, 50);

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
