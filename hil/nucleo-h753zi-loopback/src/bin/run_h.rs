//! Run H: SPI3, and SPI2 with SCK on PD3.
//!
//! - SPI3: PB3, PB4, PB5 (MOSI in function 7), DMA2 streams 6 and 7, over
//!   S2: the pins SPI1 has in run G.
//! - SPI2: PD3, PC2, PC3, DMA1 streams 6 and 7, over S3.
//!
//! Both SCKs are line 3, on ports B and D: the line is routed to PB3 for
//! SPI3, then released and routed to PD3 for SPI2.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `DMA2_STREAM7`, `DMA1_STREAM7` | the open `Spi`'s receive stream | 2 |
//! | `EXTI3` (line 3) | EXTI edge counting | 2 |
//! | `CEC` | RTIC, dispatching the priority 1 `checks` task | 1 |

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/spi_exti.rs"]
mod spi_exti;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [CEC])]
mod app {
    use ferrohal_stm32::{
        device::Interrupt,
        exti::{Ctrl, Edge, Exti},
        spi::{Buffers, Spi},
    };

    use crate::{
        board,
        spi_exti::{self, Bus},
        tests::{self, Report},
    };

    #[shared]
    struct Shared {
        bus3: Bus<board::Spi3PortB>,
        bus2: Bus<board::Spi2Pd3>,
    }

    #[local]
    struct Local {
        ctrl: Ctrl,
        led: board::Led,
        spi3: Option<Spi<board::Spi3PortB>>,
        spi2: Option<Spi<board::Spi2Pd3>>,
        sck3: Option<Exti<'B', 3>>,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the timing checks. The caches stay off, as
        // on the Lucid.
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_h(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run H, clocks: {}", board::CLOCKS);
        let mut buffers = board::dma_buffers().into_iter();
        let b = buffers.next().unwrap_or_else(|| defmt::unreachable!());
        let spi3 = Spi::new(
            &mut run.rcc,
            run.spi3,
            &board::SPI,
            Buffers { tx: b.tx, rx: b.rx },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI3: {}", error));
        let b = buffers.next().unwrap_or_else(|| defmt::unreachable!());
        let spi2 = Spi::new(
            &mut run.rcc,
            run.spi2,
            &board::SPI,
            Buffers { tx: b.tx, rx: b.rx },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI2: {}", error));
        let mut ctrl = run.exti.ctrl;
        let mut sck3 = run.exti.l3.route::<'B'>(&mut ctrl, Edge::Rising);
        // Masked until a check counts them.
        sck3.unlisten(&mut ctrl);
        checks::spawn().ok();
        (
            Shared {
                bus3: Bus::new(),
                bus2: Bus::new(),
            },
            Local {
                ctrl,
                led: run.led,
                spi3: Some(spi3),
                spi2: Some(spi2),
                sck3: Some(sck3),
            },
        )
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [bus3])]
    fn spi3_rx(mut cx: spi3_rx::Context) {
        cx.shared.bus3.lock(Bus::on_dma);
    }

    #[task(binds = DMA1_STREAM7, priority = 2, shared = [bus2])]
    fn spi2_rx(mut cx: spi2_rx::Context) {
        cx.shared.bus2.lock(Bus::on_dma);
    }

    #[task(binds = EXTI3, priority = 2, shared = [bus3, bus2])]
    fn exti_3(mut cx: exti_3::Context) {
        cx.shared.bus3.lock(Bus::on_exti);
        cx.shared.bus2.lock(Bus::on_exti);
    }

    #[task(priority = 1, shared = [bus3, bus2], local = [ctrl, led, spi3, spi2, sck3])]
    async fn checks(mut cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let l = cx.local;
        let bus3 = &mut cx.shared.bus3;
        let bus2 = &mut cx.shared.bus2;

        let Some(mut sck3) = l.sck3.take() else {
            defmt::unreachable!()
        };
        bus3.lock(|b| b.spi = l.spi3.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI3 PB3/PB4/PB5, DMA2 S6/S7",
            bus3,
            &board::SPI,
            cpu,
            Interrupt::DMA2_STREAM7,
            &mut sck3,
            l.ctrl,
        );
        // Line 3 moves from PB3 to PD3.
        let mut sck2 = sck3.release(l.ctrl).route::<'D'>(l.ctrl, Edge::Rising);
        bus2.lock(|b| b.spi = l.spi2.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI2 PD3/PC2/PC3, DMA1 S6/S7",
            bus2,
            &board::SPI,
            cpu,
            Interrupt::DMA1_STREAM7,
            &mut sck2,
            l.ctrl,
        );
        bus2.lock(|b| b.spi = None);
        let led = l.led;
        tests::finish(&report, || led.set_high());
    }
}
