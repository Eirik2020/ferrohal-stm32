//! Run F: SPI1 and SPI2 on their other pins.
//!
//! - SPI1 on port B (PB3, PB4, PB5; DMA2 streams 5 and 2), over S2.
//! - SPI2 with SCK on PB10 and MISO, MOSI on PC2, PC3 (DMA1 streams 4 and
//!   3), over S4.
//!
//! Each SPI's SCK is routed to its EXTI line too, to count its edges.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `DMA2_STREAM2`, `DMA1_STREAM3` | the open `Spi`'s receive stream | 2 |
//! | `EXTI3` (line 3), `EXTI15_10` (line 10) | EXTI edge counting | 2 |
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
    }

    #[local]
    struct Local {
        ctrl: Ctrl,
        spi1: Option<Spi<board::Spi1PortB>>,
        spi2: Option<Spi<board::Spi2PortC>>,
        sck1: Exti<'B', 3>,
        sck2: Exti<'B', 10>,
    }

    #[init(local = [
        tx1: [u8; BUF] = [0; BUF],
        rx1: [u8; BUF] = [0; BUF],
        tx2: [u8; BUF] = [0; BUF],
        rx2: [u8; BUF] = [0; BUF],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the timing checks.
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
        let mut ctrl = run.exti.ctrl;
        let mut sck1 = run.exti.l3.route::<'B'>(&mut ctrl, Edge::Rising);
        let mut sck2 = run.exti.l10.route::<'B'>(&mut ctrl, Edge::Rising);
        // Masked until a check counts them.
        sck1.unlisten(&mut ctrl);
        sck2.unlisten(&mut ctrl);
        checks::spawn().ok();
        (
            Shared {
                bus1: Bus::new(),
                bus2: Bus::new(),
            },
            Local {
                ctrl,
                spi1: Some(spi1),
                spi2: Some(spi2),
                sck1,
                sck2,
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

    #[task(binds = EXTI3, priority = 2, shared = [bus1, bus2])]
    fn exti_3(mut cx: exti_3::Context) {
        cx.shared.bus1.lock(Bus::on_exti);
        cx.shared.bus2.lock(Bus::on_exti);
    }

    #[task(binds = EXTI15_10, priority = 2, shared = [bus1, bus2])]
    fn exti_10_15(mut cx: exti_10_15::Context) {
        cx.shared.bus1.lock(Bus::on_exti);
        cx.shared.bus2.lock(Bus::on_exti);
    }

    #[task(priority = 1, shared = [bus1, bus2], local = [ctrl, spi1, spi2, sck1, sck2])]
    async fn checks(mut cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let l = cx.local;
        let bus1 = &mut cx.shared.bus1;
        let bus2 = &mut cx.shared.bus2;

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
        tests::finish(&report, || {});
    }
}
