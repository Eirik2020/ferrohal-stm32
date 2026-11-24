//! Run E: receive-only UARTs, fed by bit-banged frames from the other end
//! of their links (the TX pins driven as GPIOs):
//!
//! - USART1 on PB7 as the TBS Lucid H7's SBUS input: 100000 8E2, RX
//!   inverted, on the Lucid's DMA2 channel 4, fed inverted 25-byte frames
//!   by PB6 over Z3;
//! - UART7 on PB3 at 115200, fed by PA15 over Z12.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART1`, `DMA2_STREAM4` | USART1's `on_interrupt` | 2 |
//! | `UART7`, `DMA1_STREAM3` | UART7's `on_interrupt` | 2 |
//! | `SPI1` | RTIC, dispatching the priority 1 `checks` task | 1 |

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use ferrohal_stm32::{
        gpio::{Output, Pin},
        uart::{Uart, Vectors},
    };

    use crate::{
        board,
        tests::{self, Report},
    };

    #[shared]
    struct Shared {
        usart1: Uart<board::Usart1Sbus>,
        uart7: Uart<board::Uart7RxOnly>,
    }

    #[local]
    struct Local {
        feed1: Pin<'B', 6, Output>,
        feed7: Pin<'A', 15, Output>,
        vectors1: Vectors,
        vectors7: Vectors,
        led: board::Led,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the bit-banged frames.
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        board::dma_region(&mut core);
        let mut run = match board::run_e(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run E, clocks: {}", board::CLOCKS);
        let mut buffers = board::dma_buffers().into_iter();
        let usart1 = Uart::new_rx(
            &mut run.rcc,
            run.usart1,
            &board::SBUS,
            buffers.next().unwrap().rx,
        )
        .unwrap_or_else(|error| defmt::panic!("USART1: {}", error));
        let uart7 = Uart::new_rx(
            &mut run.rcc,
            run.uart7,
            &board::MSP,
            buffers.next().unwrap().rx,
        )
        .unwrap_or_else(|error| defmt::panic!("UART7: {}", error));
        let vectors1 = usart1.vectors();
        let vectors7 = uart7.vectors();
        checks::spawn().ok();
        (
            Shared { usart1, uart7 },
            Local {
                feed1: run.feed1,
                feed7: run.feed7,
                vectors1,
                vectors7,
                led: run.led,
            },
        )
    }

    #[task(binds = USART1, priority = 2, shared = [usart1])]
    fn usart1_line(mut cx: usart1_line::Context) {
        cx.shared.usart1.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA2_STREAM4, priority = 2, shared = [usart1])]
    fn usart1_rx(mut cx: usart1_rx::Context) {
        cx.shared.usart1.lock(|uart| tests::serve(uart));
    }

    #[task(binds = UART7, priority = 2, shared = [uart7])]
    fn uart7_line(mut cx: uart7_line::Context) {
        cx.shared.uart7.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM3, priority = 2, shared = [uart7])]
    fn uart7_rx(mut cx: uart7_rx::Context) {
        cx.shared.uart7.lock(|uart| tests::serve(uart));
    }

    #[task(
        priority = 1,
        shared = [usart1, uart7],
        local = [feed1, feed7, vectors1, vectors7, led]
    )]
    async fn checks(cx: checks::Context) {
        let mut usart1 = tests::Locked(cx.shared.usart1);
        let mut uart7 = tests::Locked(cx.shared.uart7);
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART1",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors1),
        );
        report.check_for(
            "UART7",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors7),
        );
        // The feed sends what the Lucid's receiver would: inverted frames.
        let sbus_feed = board::SBUS.with_inverted_tx(true);
        report.check_for(
            "USART1 receive-only PB7, SBUS 100000 8E2 inverted",
            "UART-007",
            "25-byte frames, one chunk each",
            tests::receive_only(&mut usart1, cx.local.feed1, &sbus_feed, 25),
        );
        report.check_for(
            "UART7 receive-only PB3, 115200",
            "UART-007",
            "40-byte frames, one chunk each",
            tests::receive_only(&mut uart7, cx.local.feed7, &board::MSP, 40),
        );
        defmt::info!("USART1: health {}", tests::RxHalf::health(&mut usart1));
        defmt::info!("UART7: health {}", tests::RxHalf::health(&mut uart7));
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
