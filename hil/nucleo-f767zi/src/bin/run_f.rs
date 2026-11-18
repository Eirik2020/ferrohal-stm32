//! Run F: SPI1, SPI2, SPI4 and SPI5 over links S1 to S4, each through spi_v3's
//! FIFO with byte-wide data register access.
//!
//! - SPI1 on port B (PB3, PB4, PB5; DMA2 streams 5 and 2), over S1.
//! - SPI2 with SCK on PB10, MISO, MOSI on PC2, PC3 (DMA1 streams 4 and 3), over S2.
//! - SPI4 on port E (PE2, PE5, PE6; DMA2 streams 1 and 0), over S3.
//! - SPI5 on port F (PF7, PF8, PF9; DMA2 streams 4 and 3), over S4.
//!
//! Each SPI's SCK is routed to its EXTI line too, to count its edges; each line
//! has a vector of its own here, so each handler serves only its SPI. The
//! instruction cache is on: from flash at seven wait states without it, the edge
//! handlers missed edges of the APB2 SPIs' slowest clock (measured 2026-11-18).
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `DMA2_STREAM2`, `DMA1_STREAM3`, `DMA2_STREAM0`, `DMA2_STREAM3` | the open `Spi`'s receive stream | 2 |
//! | `EXTI3`, `EXTI15_10`, `EXTI2`, `EXTI9_5` | EXTI edge counting | 2 |
//! | `SPDIF_RX` | RTIC, dispatching the priority 1 `checks` task | 1 |

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

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPDIF_RX])]
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

    const BUF: usize = 256;

    #[shared]
    struct Shared {
        bus1: Bus<board::Spi1PortB>,
        bus2: Bus<board::Spi2PortC>,
        bus4: Bus<board::Spi4PortE>,
        bus5: Bus<board::Spi5PortF>,
    }

    #[local]
    struct Local {
        ctrl: Ctrl,
        spi1: Option<Spi<board::Spi1PortB>>,
        spi2: Option<Spi<board::Spi2PortC>>,
        spi4: Option<Spi<board::Spi4PortE>>,
        spi5: Option<Spi<board::Spi5PortF>>,
        sck1: Exti<'B', 3>,
        sck2: Exti<'B', 10>,
        sck4: Exti<'E', 2>,
        sck5: Exti<'F', 7>,
    }

    #[init(local = [
        tx1: [u8; BUF] = [0; BUF],
        rx1: [u8; BUF] = [0; BUF],
        tx2: [u8; BUF] = [0; BUF],
        rx2: [u8; BUF] = [0; BUF],
        tx4: [u8; BUF] = [0; BUF],
        rx4: [u8; BUF] = [0; BUF],
        tx5: [u8; BUF] = [0; BUF],
        rx5: [u8; BUF] = [0; BUF],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the timing checks; the F7's Cortex-M7 ignores
        // DWT writes until it is unlocked.
        cortex_m::peripheral::DWT::unlock();
        core.SCB.enable_icache();
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_f(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run F, clocks: {}", board::CLOCKS);
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
        let spi4 = Spi::new(
            &mut run.rcc,
            run.spi4,
            &board::SPI,
            Buffers {
                tx: l.tx4,
                rx: l.rx4,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI4: {}", error));
        let spi5 = Spi::new(
            &mut run.rcc,
            run.spi5,
            &board::SPI,
            Buffers {
                tx: l.tx5,
                rx: l.rx5,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI5: {}", error));
        let mut ctrl = run.exti.ctrl;
        let mut sck1 = run.exti.l3.route::<'B'>(&mut ctrl, Edge::Rising);
        let mut sck2 = run.exti.l10.route::<'B'>(&mut ctrl, Edge::Rising);
        let mut sck4 = run.exti.l2.route::<'E'>(&mut ctrl, Edge::Rising);
        let mut sck5 = run.exti.l7.route::<'F'>(&mut ctrl, Edge::Rising);
        // Masked until a check counts them.
        sck1.mask(&mut ctrl);
        sck2.mask(&mut ctrl);
        sck4.mask(&mut ctrl);
        sck5.mask(&mut ctrl);
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
                spi1: Some(spi1),
                spi2: Some(spi2),
                spi4: Some(spi4),
                spi5: Some(spi5),
                sck1,
                sck2,
                sck4,
                sck5,
            },
        )
    }

    #[task(binds = DMA2_STREAM2, priority = 2, shared = [bus1])]
    fn spi1_rx(mut cx: spi1_rx::Context) {
        cx.shared.bus1.lock(Bus::on_dma);
    }

    #[task(binds = DMA1_STREAM3, priority = 2, shared = [bus2])]
    fn spi2_rx(mut cx: spi2_rx::Context) {
        cx.shared.bus2.lock(Bus::on_dma);
    }

    #[task(binds = DMA2_STREAM0, priority = 2, shared = [bus4])]
    fn spi4_rx(mut cx: spi4_rx::Context) {
        cx.shared.bus4.lock(Bus::on_dma);
    }

    #[task(binds = DMA2_STREAM3, priority = 2, shared = [bus5])]
    fn spi5_rx(mut cx: spi5_rx::Context) {
        cx.shared.bus5.lock(Bus::on_dma);
    }

    #[task(binds = EXTI3, priority = 2, shared = [bus1])]
    fn exti_3(mut cx: exti_3::Context) {
        cx.shared.bus1.lock(Bus::on_exti);
    }

    #[task(binds = EXTI15_10, priority = 2, shared = [bus2])]
    fn exti_10_15(mut cx: exti_10_15::Context) {
        cx.shared.bus2.lock(Bus::on_exti);
    }

    #[task(binds = EXTI2, priority = 2, shared = [bus4])]
    fn exti_2(mut cx: exti_2::Context) {
        cx.shared.bus4.lock(Bus::on_exti);
    }

    #[task(binds = EXTI9_5, priority = 2, shared = [bus5])]
    fn exti_5_9(mut cx: exti_5_9::Context) {
        cx.shared.bus5.lock(Bus::on_exti);
    }

    #[task(
        priority = 1,
        shared = [bus1, bus2, bus4, bus5],
        local = [ctrl, spi1, spi2, spi4, spi5, sck1, sck2, sck4, sck5]
    )]
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
            "SPI1 PB3/PB4/PB5, DMA2 S5/S2",
            bus1,
            &board::SPI,
            cpu,
            Interrupt::DMA2_STREAM2,
            l.sck1,
            l.ctrl,
        );
        bus2.lock(|b| b.spi = l.spi2.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI2 PB10/PC2/PC3, DMA1 S4/S3",
            bus2,
            &board::SPI,
            cpu,
            Interrupt::DMA1_STREAM3,
            l.sck2,
            l.ctrl,
        );
        bus2.lock(|b| b.spi = None);
        bus4.lock(|b| b.spi = l.spi4.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI4 PE2/PE5/PE6, DMA2 S1/S0",
            bus4,
            &board::SPI,
            cpu,
            Interrupt::DMA2_STREAM0,
            l.sck4,
            l.ctrl,
        );
        bus4.lock(|b| b.spi = None);
        bus5.lock(|b| b.spi = l.spi5.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI5 PF7/PF8/PF9, DMA2 S4/S3",
            bus5,
            &board::SPI,
            cpu,
            Interrupt::DMA2_STREAM3,
            l.sck5,
            l.ctrl,
        );
        bus5.lock(|b| b.spi = None);
        tests::finish(&report, || {});
    }
}
