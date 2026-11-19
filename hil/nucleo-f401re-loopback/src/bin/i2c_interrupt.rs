//! Run X: the interrupt-driven I2C master on I2C3 (PA8/PC9)
//! against the Pico 1 simulator, wired as run S is, first with every byte
//! moved by the event interrupt, then from a DMA wiring of DMA1 stream 4
//! (TX) and stream 2 (RX), channel 3 (RM0368 Table 28). Before each open,
//! the open's refusal with the wiring handed back. The checks are
//! `common/i2c_interrupt.rs`; the procedure is `requirements/benches.md`,
//! "Interrupt-driven I2C on the Nucleo-F401RE".

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

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SDIO])]
mod app {
    use ferrohal_stm32::{
        chip::I2C3,
        dma::Stream,
        gpio::{Alternate, OpenDrain, Pin},
        i2c::{
            Config, ConfigError, DmaWiring, InterruptHealth, InterruptI2c, OpenError, Outcome,
            StartError, Transaction,
        },
        rcc::Rcc,
        time::Hertz,
    };

    use crate::{
        board::{self, I2c3Bus},
        i2c_interrupt::{Bus, Harness, Slot},
        tests::{self, Fail, Report},
    };

    const SPEED: Hertz = Hertz::khz(50);
    const LINK: &str = "I2C3 PA8/PC9 (Pico simulator), interrupt-driven";
    /// What the bus running after an abort verifies: i2c_v1's recovery.
    const RECOVERS: &str = "I2C-011,I2C-V1-003";

    type Scl = Pin<'A', 8, Alternate<4, OpenDrain>>;
    type Sda = Pin<'C', 9, Alternate<4, OpenDrain>>;
    type IrqBus = InterruptI2c<I2c3Bus>;
    type DmaBus = InterruptI2c<DmaWiring<I2C3, Scl, Sda, Stream<1, 4>, Stream<1, 2>>>;

    /// The driver in the slot: interrupt-fed, or from the DMA wiring.
    pub enum Mode {
        Irq(IrqBus),
        Dma(DmaBus),
        Closed,
    }

    impl Bus for Mode {
        fn start(&mut self, t: Transaction) -> Result<(), (StartError, Transaction)> {
            match self {
                Mode::Irq(b) => b.start(t).map_err(|r| (r.error(), r.into_parts())),
                Mode::Dma(b) => b.start(t).map_err(|r| (r.error(), r.into_parts())),
                Mode::Closed => Err((StartError::Busy, t)),
            }
        }
        fn abort(&mut self) -> Option<Outcome> {
            match self {
                Mode::Irq(b) => b.abort(),
                Mode::Dma(b) => b.abort(),
                Mode::Closed => None,
            }
        }
        fn service(&mut self) -> Option<Outcome> {
            match self {
                Mode::Irq(b) => b.on_interrupt(),
                Mode::Dma(b) => b.on_interrupt(),
                Mode::Closed => None,
            }
        }
        fn bound(&self, write: usize, read: usize) -> u32 {
            match self {
                Mode::Irq(b) => b.bound_cycles(write, read),
                Mode::Dma(b) => b.bound_cycles(write, read),
                Mode::Closed => 0,
            }
        }
        fn health(&self) -> InterruptHealth {
            match self {
                Mode::Irq(b) => b.health(),
                Mode::Dma(b) => b.health(),
                Mode::Closed => InterruptHealth::default(),
            }
        }
        fn dma_from(&self) -> (Option<usize>, Option<usize>) {
            match self {
                Mode::Dma(_) => (Some(DmaBus::DMA_FROM_WRITE), Some(DmaBus::DMA_FROM_READ)),
                _ => (None, None),
            }
        }
    }

    #[shared]
    struct Shared {
        slot: Slot<Mode>,
    }

    #[local]
    struct Local {
        led: board::Led,
        report: Report,
        rcc: Rcc,
        streams: Option<(Stream<1, 4>, Stream<1, 2>)>,
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
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_x(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        defmt::info!("run X, clocks: {}", board::CLOCKS);
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
                    bus: Mode::Irq(bus),
                    outcome: None,
                },
            },
            Local {
                led: run.led,
                report,
                rcc: run.rcc,
                streams: Some((run.tx, run.rx)),
                tx: cx.local.tx_buf,
                rx: cx.local.rx_buf,
            },
        )
    }

    #[idle(shared = [slot], local = [led, report, rcc, streams, tx, rx])]
    fn idle(cx: idle::Context) -> ! {
        let report = cx.local.report;
        let tx = core::mem::take(cx.local.tx);
        let rx = core::mem::take(cx.local.rx);
        let mut h = Harness::new(cx.shared.slot, tx, rx);
        h.suite(report, LINK, "irq", "I2C-007,I2C-008", RECOVERS);
        // The DMA wiring: the interrupt-fed driver's wiring and both streams.
        let rcc = cx.local.rcc;
        let streams = cx.local.streams.take();
        let opened = h.m.lock(|s| {
            let Mode::Irq(bus) = core::mem::replace(&mut s.bus, Mode::Closed) else {
                return Err("the interrupt-fed driver was not in the slot");
            };
            let wiring = bus.free();
            let Some((tx_dma, rx_dma)) = streams else {
                return Err("the streams were taken");
            };
            let dma = DmaWiring {
                i2c: wiring.i2c,
                scl: wiring.scl,
                sda: wiring.sda,
                tx_dma,
                rx_dma,
            };
            let dma = match InterruptI2c::new_dma(rcc, dma, &Config::new(Hertz::mhz(2))) {
                Err(refused) => {
                    if refused.error() != OpenError::Config(ConfigError::Frequency) {
                        return Err("the DMA open refused otherwise than for the speed");
                    }
                    refused.into_parts()
                }
                Ok(_) => return Err("a DMA open at 2 MHz ran"),
            };
            match InterruptI2c::new_dma(rcc, dma, &Config::new(SPEED)) {
                Ok(bus) => {
                    s.bus = Mode::Dma(bus);
                    Ok(())
                }
                Err(_) => Err("the DMA open refused"),
            }
        });
        report.check_for(
            LINK,
            "I2C-012",
            "a DMA open at 2 MHz is refused with the wiring, which opens",
            opened.map_err(Fail::Unexpected),
        );
        if opened.is_ok() {
            h.suite(report, LINK, "dma", "I2C-007,I2C-008,I2C-V1-002", RECOVERS);
        }
        let led = cx.local.led;
        tests::finish(report, || led.set_high());
    }

    #[task(binds = I2C3_EV, priority = 2, shared = [slot])]
    fn ev(mut cx: ev::Context) {
        crate::i2c_interrupt::serve(&mut cx.shared.slot);
    }

    #[task(binds = I2C3_ER, priority = 2, shared = [slot])]
    fn er(mut cx: er::Context) {
        crate::i2c_interrupt::serve(&mut cx.shared.slot);
    }

    #[task(binds = DMA1_STREAM2, priority = 2, shared = [slot])]
    fn rx_stream(mut cx: rx_stream::Context) {
        crate::i2c_interrupt::serve(&mut cx.shared.slot);
    }
}
