//! Run F: SPI1 on the TBS Lucid H7's IMU pins and streams, the other SPIs
//! on their first pins, and EXTI.
//!
//! - SPI1: PA5, PA6, PD7 (the Lucid's), DMA1 streams 5 and 4, over S1.
//! - SPI2: PB10, PC2, PC3, DMA2 streams 0 and 1, over S3.
//! - SPI4: PE12, PE13, PE14, DMA2 streams 2 and 3, over S4.
//! - SPI5: PF7, PF8, PF9, DMA2 streams 4 and 5, over S6.
//! - EXTI line 6 on PD6, driven by PD5 over Z1.
//!
//! Each SPI's SCK is routed to its EXTI line too, to count its edges.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `DMA1_STREAM4`, `DMA2_STREAM1`, `DMA2_STREAM3`, `DMA2_STREAM5` | the open `Spi`'s receive stream | 2 |
//! | `EXTI9_5` (lines 5, 6, 7), `EXTI15_10` (lines 10, 12) | EXTI edge counting | 2 |
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
        gpio::{Output, Pin},
        spi::{Buffers, Spi},
    };

    use crate::{
        board,
        spi_exti::{self, Bus},
        tests::{self, Report},
    };

    #[shared]
    struct Shared {
        bus1: Bus<board::Spi1Lucid>,
        bus2: Bus<board::Spi2Pb10>,
        bus4: Bus<board::Spi4High>,
        bus5: Bus<board::Spi5>,
    }

    #[local]
    struct Local {
        ctrl: Ctrl,
        led: board::Led,
        spi1: Option<Spi<board::Spi1Lucid>>,
        spi2: Option<Spi<board::Spi2Pb10>>,
        spi4: Option<Spi<board::Spi4High>>,
        spi5: Option<Spi<board::Spi5>>,
        sck1: Exti<'A', 5>,
        sck2: Exti<'B', 10>,
        sck4: Exti<'E', 12>,
        sck5: Exti<'F', 7>,
        sense: Exti<'D', 6>,
        feed: Pin<'D', 5, Output>,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the timing checks. The caches stay off, as
        // on the Lucid.
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_f(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run F, clocks: {}", board::CLOCKS);
        let mut buffers = board::dma_buffers().into_iter();
        let b = buffers.next().unwrap_or_else(|| defmt::unreachable!());
        let spi1 = Spi::new(
            &mut run.rcc,
            run.spi1,
            &board::SPI,
            Buffers { tx: b.tx, rx: b.rx },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI1: {}", error));
        let b = buffers.next().unwrap_or_else(|| defmt::unreachable!());
        let spi2 = Spi::new(
            &mut run.rcc,
            run.spi2,
            &board::SPI,
            Buffers { tx: b.tx, rx: b.rx },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI2: {}", error));
        let b = buffers.next().unwrap_or_else(|| defmt::unreachable!());
        let spi4 = Spi::new(
            &mut run.rcc,
            run.spi4,
            &board::SPI,
            Buffers { tx: b.tx, rx: b.rx },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI4: {}", error));
        let b = buffers.next().unwrap_or_else(|| defmt::unreachable!());
        let spi5 = Spi::new(
            &mut run.rcc,
            run.spi5,
            &board::SPI,
            Buffers { tx: b.tx, rx: b.rx },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI5: {}", error));
        let mut ctrl = run.exti.ctrl;
        let mut sck1 = run.exti.l5.route::<'A'>(&mut ctrl, Edge::Rising);
        let mut sck2 = run.exti.l10.route::<'B'>(&mut ctrl, Edge::Rising);
        let mut sck4 = run.exti.l12.route::<'E'>(&mut ctrl, Edge::Rising);
        let mut sck5 = run.exti.l7.route::<'F'>(&mut ctrl, Edge::Rising);
        let mut sense = run.exti.l6.route::<'D'>(&mut ctrl, Edge::Rising);
        // Masked until a check counts them.
        sck1.unlisten(&mut ctrl);
        sck2.unlisten(&mut ctrl);
        sck4.unlisten(&mut ctrl);
        sck5.unlisten(&mut ctrl);
        sense.unlisten(&mut ctrl);
        checks::spawn().ok();
        (
            Shared {
                bus1: Bus::new(),
                bus2: Bus::new(),
                bus4: Bus::new(),
                bus5: Bus::new(),
            },
            Local {
                ctrl,
                led: run.led,
                spi1: Some(spi1),
                spi2: Some(spi2),
                spi4: Some(spi4),
                spi5: Some(spi5),
                sck1,
                sck2,
                sck4,
                sck5,
                sense,
                feed: run.feed,
            },
        )
    }

    #[task(binds = DMA1_STREAM4, priority = 2, shared = [bus1])]
    fn spi1_rx(mut cx: spi1_rx::Context) {
        cx.shared.bus1.lock(Bus::on_dma);
    }

    #[task(binds = DMA2_STREAM1, priority = 2, shared = [bus2])]
    fn spi2_rx(mut cx: spi2_rx::Context) {
        cx.shared.bus2.lock(Bus::on_dma);
    }

    #[task(binds = DMA2_STREAM3, priority = 2, shared = [bus4])]
    fn spi4_rx(mut cx: spi4_rx::Context) {
        cx.shared.bus4.lock(Bus::on_dma);
    }

    #[task(binds = DMA2_STREAM5, priority = 2, shared = [bus5])]
    fn spi5_rx(mut cx: spi5_rx::Context) {
        cx.shared.bus5.lock(Bus::on_dma);
    }

    #[task(binds = EXTI9_5, priority = 2, shared = [bus1, bus2, bus4, bus5])]
    fn exti_5_9(mut cx: exti_5_9::Context) {
        cx.shared.bus1.lock(Bus::on_exti);
        cx.shared.bus2.lock(Bus::on_exti);
        cx.shared.bus4.lock(Bus::on_exti);
        cx.shared.bus5.lock(Bus::on_exti);
    }

    #[task(binds = EXTI15_10, priority = 2, shared = [bus1, bus2, bus4, bus5])]
    fn exti_10_15(mut cx: exti_10_15::Context) {
        cx.shared.bus1.lock(Bus::on_exti);
        cx.shared.bus2.lock(Bus::on_exti);
        cx.shared.bus4.lock(Bus::on_exti);
        cx.shared.bus5.lock(Bus::on_exti);
    }

    #[task(priority = 1, shared = [bus1, bus2, bus4, bus5], local = [ctrl, led, spi1, spi2, spi4, spi5, sck1, sck2, sck4, sck5, sense, feed])]
    async fn checks(mut cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let l = cx.local;
        let bus1 = &mut cx.shared.bus1;
        let bus2 = &mut cx.shared.bus2;
        let bus4 = &mut cx.shared.bus4;
        let bus5 = &mut cx.shared.bus5;

        bus1.lock(|b| b.spi = l.spi1.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI1 PA5/PA6/PD7 (Lucid), DMA1 S5/S4",
            bus1,
            &board::SPI,
            cpu,
            Interrupt::DMA1_STREAM4,
            l.sck1,
            l.ctrl,
        );
        bus2.lock(|b| b.spi = l.spi2.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI2 PB10/PC2/PC3, DMA2 S0/S1",
            bus2,
            &board::SPI,
            cpu,
            Interrupt::DMA2_STREAM1,
            l.sck2,
            l.ctrl,
        );
        bus4.lock(|b| b.spi = l.spi4.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI4 PE12/PE13/PE14, DMA2 S2/S3",
            bus4,
            &board::SPI,
            cpu,
            Interrupt::DMA2_STREAM3,
            l.sck4,
            l.ctrl,
        );
        bus5.lock(|b| b.spi = l.spi5.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI5 PF7/PF8/PF9, DMA2 S4/S5",
            bus5,
            &board::SPI,
            cpu,
            Interrupt::DMA2_STREAM5,
            l.sck5,
            l.ctrl,
        );
        bus5.lock(|b| b.spi = None);
        spi_exti::exti_suite(
            &mut report,
            "EXTI6 PD6 <- PD5 (Z1)",
            bus5,
            l.feed,
            l.sense,
            l.ctrl,
        );
        let led = l.led;
        tests::finish(&report, || led.set_high());
    }
}
