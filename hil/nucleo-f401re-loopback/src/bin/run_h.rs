//! Run H: every timer the F401 has, against the DWT cycle counter. No links.
//!
//! Each timer ticks at 1 kHz and must keep that rate to 0.05 %, then stop
//! when paused. Run U takes the second use of TIM4 (8 kHz, FerroWasp's
//! control loop rate) and of TIM2 and TIM5 (1 MHz timebases) on a fresh
//! boot, so no token is stolen.
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
            ($name:literal, $timer:expr, $rate:expr, $ticks:expr) => {
                match $timer.map(|timer| Tick::new(&mut run.rcc, timer, $rate)) {
                    Some(Ok(tick)) => {
                        timers::tick_check(&mut report, $name, bench, tick, cpu, $ticks)
                    }
                    Some(Err(refused)) => report.check($name, "open", Err(Fail::Timer(refused.error()))),
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
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }
}
