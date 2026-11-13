//! Run E: SPI and EXTI.
//!
//! - SPI1 on the Foxeer F405's IMU pins and streams (PA5, PA6, PA7; DMA2
//!   streams 3 and 0), over S1.
//! - SPI3 on port C (PC10, PC11, PC12; DMA1 streams 7 and 2), over S5.
//! - SPI2 on port B (PB13, PB14, PB15; DMA1 streams 4 and 3), over S3.
//! - EXTI line 12 on PA12, driven by PA11 over J2.
//!
//! Each SPI's SCK is routed to its EXTI line too, to count its edges. LD2
//! shares PA5 with SPI1's SCK, so it flickers and does not report.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `DMA2_STREAM0`, `DMA1_STREAM2`, `DMA1_STREAM3` | the open `Spi`'s receive stream | 2 |
//! | `EXTI9_5` (line 5), `EXTI15_10` (lines 10, 12, 13) | EXTI edge counting | 2 |
//! | `SDIO` | RTIC, dispatching the priority 1 `checks` task | 1 |

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

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SDIO])]
mod app {
    use ferrohal_stm32::{
        chip::{SPI1, SPI2, SPI3},
        device::Interrupt,
        exti::{Ctrl, Edge, Exti},
        gpio::{Output, Pin},
        spi::{Buffers, Spi},
    };

    use crate::{
        board,
        spi_exti::{self, Bus},
        tests::{self, Report},
    };

    const BUF: usize = 256;

    #[shared]
    struct Shared {
        bus1: Bus<SPI1>,
        bus3: Bus<SPI3>,
        bus2: Bus<SPI2>,
    }

    #[local]
    struct Local {
        ctrl: Ctrl,
        spi1: Option<Spi<SPI1>>,
        spi3: Option<Spi<SPI3>>,
        spi2: Option<Spi<SPI2>>,
        sck1: Exti<'A', 5>,
        sck3: Exti<'C', 10>,
        sck2: Exti<'B', 13>,
        sense: Exti<'A', 12>,
        feed: Pin<'A', 11, Output>,
    }

    #[init(local = [
        tx1: [u8; BUF] = [0; BUF],
        rx1: [u8; BUF] = [0; BUF],
        tx3: [u8; BUF] = [0; BUF],
        rx3: [u8; BUF] = [0; BUF],
        tx2: [u8; BUF] = [0; BUF],
        rx2: [u8; BUF] = [0; BUF],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the timing checks.
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_e(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run E, clocks: {}", board::CLOCKS);
        let l = cx.local;
        let spi1 = Spi::new(
            &mut run.rcc,
            run.spi1,
            &board::SPI,
            Buffers {
                tx: l.tx1,
                rx: l.rx1,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI1: {}", error));
        let spi3 = Spi::new(
            &mut run.rcc,
            run.spi3,
            &board::SPI,
            Buffers {
                tx: l.tx3,
                rx: l.rx3,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI3: {}", error));
        let spi2 = Spi::new(
            &mut run.rcc,
            run.spi2,
            &board::SPI,
            Buffers {
                tx: l.tx2,
                rx: l.rx2,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI2: {}", error));
        let mut ctrl = run.exti.ctrl;
        let mut sck1 = run.exti.l5.route::<'A'>(&mut ctrl, Edge::Rising);
        let mut sck3 = run.exti.l10.route::<'C'>(&mut ctrl, Edge::Rising);
        let mut sck2 = run.exti.l13.route::<'B'>(&mut ctrl, Edge::Rising);
        let mut sense = run.exti.l12.route::<'A'>(&mut ctrl, Edge::Rising);
        // Masked until a check counts them.
        sck1.unlisten(&mut ctrl);
        sck3.unlisten(&mut ctrl);
        sck2.unlisten(&mut ctrl);
        sense.unlisten(&mut ctrl);
        checks::spawn().ok();
        (
            Shared {
                bus1: Bus::new(),
                bus3: Bus::new(),
                bus2: Bus::new(),
            },
            Local {
                ctrl,
                spi1: Some(spi1),
                spi3: Some(spi3),
                spi2: Some(spi2),
                sck1,
                sck3,
                sck2,
                sense,
                feed: run.feed,
            },
        )
    }

    #[task(binds = DMA2_STREAM0, priority = 2, shared = [bus1])]
    fn spi1_rx(mut cx: spi1_rx::Context) {
        cx.shared.bus1.lock(Bus::on_dma);
    }

    #[task(binds = DMA1_STREAM2, priority = 2, shared = [bus3])]
    fn spi3_rx(mut cx: spi3_rx::Context) {
        cx.shared.bus3.lock(Bus::on_dma);
    }

    #[task(binds = DMA1_STREAM3, priority = 2, shared = [bus2])]
    fn spi2_rx(mut cx: spi2_rx::Context) {
        cx.shared.bus2.lock(Bus::on_dma);
    }

    #[task(binds = EXTI9_5, priority = 2, shared = [bus1, bus3, bus2])]
    fn exti_5_9(mut cx: exti_5_9::Context) {
        cx.shared.bus1.lock(Bus::on_exti);
        cx.shared.bus3.lock(Bus::on_exti);
        cx.shared.bus2.lock(Bus::on_exti);
    }

    #[task(binds = EXTI15_10, priority = 2, shared = [bus1, bus3, bus2])]
    fn exti_10_15(mut cx: exti_10_15::Context) {
        cx.shared.bus1.lock(Bus::on_exti);
        cx.shared.bus3.lock(Bus::on_exti);
        cx.shared.bus2.lock(Bus::on_exti);
    }

    #[task(priority = 1, shared = [bus1, bus3, bus2], local = [ctrl, spi1, spi3, spi2, sck1, sck3, sck2, sense, feed])]
    async fn checks(mut cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let l = cx.local;
        let bus1 = &mut cx.shared.bus1;
        let bus3 = &mut cx.shared.bus3;
        let bus2 = &mut cx.shared.bus2;

        bus1.lock(|b| b.spi = l.spi1.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI1 PA5/PA6/PA7, DMA2 S3/S0",
            bus1,
            &board::SPI,
            cpu,
            Interrupt::DMA2_STREAM0,
            l.sck1,
            l.ctrl,
        );
        bus3.lock(|b| b.spi = l.spi3.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI3 PC10/PC11/PC12, DMA1 S7/S2",
            bus3,
            &board::SPI,
            cpu,
            Interrupt::DMA1_STREAM2,
            l.sck3,
            l.ctrl,
        );
        bus2.lock(|b| b.spi = l.spi2.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI2 PB13/PB14/PB15, DMA1 S4/S3",
            bus2,
            &board::SPI,
            cpu,
            Interrupt::DMA1_STREAM3,
            l.sck2,
            l.ctrl,
        );
        bus2.lock(|b| b.spi = None);
        spi_exti::exti_suite(
            &mut report,
            "EXTI12 PA12 <- PA11 (J2)",
            bus2,
            l.feed,
            l.sense,
            l.ctrl,
        );
        tests::finish(&report, || {});
    }
}
