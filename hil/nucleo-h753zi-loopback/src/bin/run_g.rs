//! Run G: SPI1 and SPI4 on their other pins.
//!
//! - SPI1: PB3, PB4, PB5, DMA1 streams 0 and 1, over S2.
//! - SPI4: PE2, PE5, PE6, DMA1 streams 2 and 3, over S5.
//!
//! Each SPI's SCK is routed to its EXTI line too, to count its edges.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `DMA1_STREAM1`, `DMA1_STREAM3` | the open `Spi`'s receive stream | 2 |
//! | `EXTI3` (line 3), `EXTI2` (line 2) | EXTI edge counting | 2 |
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
        chip::{SPI1, SPI4},
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
        bus1: Bus<SPI1>,
        bus4: Bus<SPI4>,
    }

    #[local]
    struct Local {
        ctrl: Ctrl,
        led: board::Led,
        spi1: Option<Spi<SPI1>>,
        spi4: Option<Spi<SPI4>>,
        sck1: Exti<'B', 3>,
        sck4: Exti<'E', 2>,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the timing checks. The caches stay off, as
        // on the Lucid.
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_g(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run G, clocks: {}", board::CLOCKS);
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
        let spi4 = Spi::new(
            &mut run.rcc,
            run.spi4,
            &board::SPI,
            Buffers { tx: b.tx, rx: b.rx },
        )
        .unwrap_or_else(|error| defmt::panic!("SPI4: {}", error));
        let mut ctrl = run.exti.ctrl;
        let mut sck1 = run.exti.l3.route::<'B'>(&mut ctrl, Edge::Rising);
        let mut sck4 = run.exti.l2.route::<'E'>(&mut ctrl, Edge::Rising);
        // Masked until a check counts them.
        sck1.unlisten(&mut ctrl);
        sck4.unlisten(&mut ctrl);
        checks::spawn().ok();
        (
            Shared {
                bus1: Bus::new(),
                bus4: Bus::new(),
            },
            Local {
                ctrl,
                led: run.led,
                spi1: Some(spi1),
                spi4: Some(spi4),
                sck1,
                sck4,
            },
        )
    }

    #[task(binds = DMA1_STREAM1, priority = 2, shared = [bus1])]
    fn spi1_rx(mut cx: spi1_rx::Context) {
        cx.shared.bus1.lock(Bus::on_dma);
    }

    #[task(binds = DMA1_STREAM3, priority = 2, shared = [bus4])]
    fn spi4_rx(mut cx: spi4_rx::Context) {
        cx.shared.bus4.lock(Bus::on_dma);
    }

    #[task(binds = EXTI3, priority = 2, shared = [bus1])]
    fn exti_3(mut cx: exti_3::Context) {
        cx.shared.bus1.lock(Bus::on_exti);
    }

    #[task(binds = EXTI2, priority = 2, shared = [bus4])]
    fn exti_2(mut cx: exti_2::Context) {
        cx.shared.bus4.lock(Bus::on_exti);
    }

    #[task(priority = 1, shared = [bus1, bus4], local = [ctrl, led, spi1, spi4, sck1, sck4])]
    async fn checks(mut cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let l = cx.local;
        let bus1 = &mut cx.shared.bus1;
        let bus4 = &mut cx.shared.bus4;

        bus1.lock(|b| b.spi = l.spi1.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI1 PB3/PB4/PB5, DMA1 S0/S1",
            bus1,
            &board::SPI,
            cpu,
            Interrupt::DMA1_STREAM1,
            l.sck1,
            l.ctrl,
        );
        bus4.lock(|b| b.spi = l.spi4.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI4 PE2/PE5/PE6, DMA1 S2/S3",
            bus4,
            &board::SPI,
            cpu,
            Interrupt::DMA1_STREAM3,
            l.sck4,
            l.ctrl,
        );
        bus4.lock(|b| b.spi = None);
        let led = l.led;
        tests::finish(&report, || led.set_high());
    }
}
