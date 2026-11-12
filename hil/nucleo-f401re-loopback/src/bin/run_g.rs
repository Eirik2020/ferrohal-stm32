//! Run G: SPI3 on port B (PB3, PB4, PB5; DMA1 streams 5 and 0), over S2,
//! the pins SPI1 has in run F.
//!
//! SCK is routed to EXTI line 3 too, to count its edges.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `DMA1_STREAM0` | the open `Spi`'s receive stream | 2 |
//! | `EXTI3` (line 3) | EXTI edge counting | 2 |
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
        chip::SPI3,
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
        bus: Bus<SPI3>,
    }

    #[local]
    struct Local {
        ctrl: Ctrl,
        spi3: Option<Spi<SPI3>>,
        sck3: Exti<'B', 3>,
        led: board::Led,
    }

    #[init(local = [
        tx3: [u8; BUF] = [0; BUF],
        rx3: [u8; BUF] = [0; BUF],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the timing checks.
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_g(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run G, clocks: {}", board::CLOCKS);
        let l = cx.local;
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
        let mut ctrl = run.exti.ctrl;
        let mut sck3 = run.exti.l3.route::<'B'>(&mut ctrl, Edge::Rising);
        // Masked until a check counts them.
        sck3.unlisten(&mut ctrl);
        checks::spawn().ok();
        (
            Shared { bus: Bus::new() },
            Local {
                ctrl,
                spi3: Some(spi3),
                sck3,
                led: run.led,
            },
        )
    }

    #[task(binds = DMA1_STREAM0, priority = 2, shared = [bus])]
    fn spi3_rx(mut cx: spi3_rx::Context) {
        cx.shared.bus.lock(Bus::on_dma);
    }

    #[task(binds = EXTI3, priority = 2, shared = [bus])]
    fn exti_3(mut cx: exti_3::Context) {
        cx.shared.bus.lock(Bus::on_exti);
    }

    #[task(priority = 1, shared = [bus], local = [ctrl, spi3, sck3, led])]
    async fn checks(mut cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let l = cx.local;
        let bus = &mut cx.shared.bus;

        bus.lock(|b| b.spi = l.spi3.take());
        spi_exti::spi_suite(
            &mut report,
            "SPI3 PB3/PB4/PB5, DMA1 S5/S0",
            bus,
            &board::SPI,
            cpu,
            Interrupt::DMA1_STREAM0,
            l.sck3,
            l.ctrl,
        );
        bus.lock(|b| b.spi = None);
        let led = l.led;
        tests::finish(&report, || led.set_high());
    }
}
