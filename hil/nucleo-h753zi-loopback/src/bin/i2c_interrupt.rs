//! Run V: the interrupt-driven I2C master on I2C4 (PD12/PD13) against the
//! Pico 1 simulator, wired as run S is, with every byte moved by the event
//! interrupt: I2C4's requests reach the BDMA alone (RM0433 Table 128), which
//! FerroHAL does not drive, so this bus has no DMA wiring. Before the open,
//! the open's refusal with the wiring handed back. The checks are
//! `common/i2c_interrupt.rs`; the procedure is `requirements/benches.md`,
//! "Interrupt-driven I2C on the Nucleo-H753ZI".

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/i2c_interrupt.rs"]
mod i2c_interrupt;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [CEC])]
mod app {
    use ferrohal_stm32::{
        gpio::{Output, Pin},
        i2c::{
            Config, ConfigError, InterruptHealth, InterruptI2c, Outcome, StartError, Transaction,
        },
        time::Hertz,
    };

    use crate::{
        board::{self, I2c4Bus},
        i2c_interrupt::{Bus, Harness, Slot},
        tests::{self, Fail, Report},
    };

    const SPEED: Hertz = Hertz::khz(50);
    const LINK: &str = "I2C4 PD12/PD13 (Pico simulator), interrupt-driven";
    /// What the bus running after an abort verifies: i2c_v2's recovery.
    const RECOVERS: &str = "I2C-011,I2C-V2-003";

    /// The driver in the slot.
    pub struct Irq(InterruptI2c<I2c4Bus>);

    impl Bus for Irq {
        fn start(&mut self, t: Transaction) -> Result<(), (StartError, Transaction)> {
            self.0.start(t).map_err(|r| (r.error(), r.into_parts()))
        }
        fn abort(&mut self) -> Option<Outcome> {
            self.0.abort()
        }
        fn service(&mut self) -> Option<Outcome> {
            self.0.on_interrupt()
        }
        fn bound(&self, write: usize, read: usize) -> u32 {
            self.0.bound_cycles(write, read)
        }
        fn health(&self) -> InterruptHealth {
            self.0.health()
        }
        fn dma_from(&self) -> (Option<usize>, Option<usize>) {
            (None, None)
        }
    }

    #[shared]
    struct Shared {
        slot: Slot<Irq>,
    }

    #[local]
    struct Local {
        led: Pin<'A', 5, Output>,
        report: Report,
        tx: &'static mut [u8],
        rx: &'static mut [u8],
    }

    #[init(local = [
        tx_buf: [u8; 300] = [0; 300],
        rx_buf: [u8; 300] = [0; 300],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        // The Cortex-M7's DWT counts only once unlocked.
        cortex_m::peripheral::DWT::unlock();
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_s(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        defmt::info!("run V, clocks: {}", board::CLOCKS);
        let mut report = Report::new();
        // The open refuses a speed it cannot time, with the wiring back.
        let wiring = match InterruptI2c::new(&mut run.rcc, run.i2c, &Config::new(Hertz::hz(0))) {
            Err(refused) => {
                report.check_for(
                    LINK,
                    "I2C-012",
                    "an open at 0 Hz is refused with the wiring",
                    match refused.error() {
                        ConfigError::Frequency => Ok(()),
                        other => Err(Fail::I2cConfig(other)),
                    },
                );
                refused.into_parts()
            }
            Ok(_) => defmt::panic!("an open at 0 Hz ran"),
        };
        let bus = match InterruptI2c::new(&mut run.rcc, wiring, &Config::new(SPEED)) {
            Ok(bus) => bus,
            Err(refused) => defmt::panic!("the interrupt-fed open refused: {}", refused.error()),
        };
        report.check_for(LINK, "I2C-012", "the wiring handed back opens", Ok(()));
        (
            Shared {
                slot: Slot {
                    bus: Irq(bus),
                    outcome: None,
                },
            },
            Local {
                led: run.led,
                report,
                tx: cx.local.tx_buf,
                rx: cx.local.rx_buf,
            },
        )
    }

    #[idle(shared = [slot], local = [led, report, tx, rx])]
    fn idle(cx: idle::Context) -> ! {
        let report = cx.local.report;
        let tx = core::mem::take(cx.local.tx);
        let rx = core::mem::take(cx.local.rx);
        let mut h = Harness::new(cx.shared.slot, tx, rx);
        h.suite(report, LINK, "irq", "I2C-007,I2C-008,I2C-V2-002", RECOVERS);
        let led = cx.local.led;
        tests::finish(report, || led.set_high());
    }

    #[task(binds = I2C4_EV, priority = 2, shared = [slot])]
    fn ev(mut cx: ev::Context) {
        crate::i2c_interrupt::serve(&mut cx.shared.slot);
    }

    #[task(binds = I2C4_ER, priority = 2, shared = [slot])]
    fn er(mut cx: er::Context) {
        crate::i2c_interrupt::serve(&mut cx.shared.slot);
    }
}
