//! Run E: receive-only UARTs, fed by bit-banged frames from the other end
//! of their links (the TX pins driven as GPIOs):
//!
//! - USART1 on PB7 as the TBS Lucid H7's SBUS input: 100000 8E2, RX
//!   inverted, on the Lucid's DMA2 stream 4, fed inverted 25-byte frames
//!   by PB6 over Z3;
//! - UART7 on PB3 at 115200, fed by PA15 over Z12.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART1`, `DMA2_STREAM4` | USART1's `UartIrq` | 2 |
//! | `UART7`, `DMA1_STREAM3` | UART7's `UartIrq` | 2 |
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
        chip::{UART7, USART1},
        gpio::{Output, Pin},
        uart::{UartInterrupts, UartIrq, UartRx},
    };

    use crate::{
        board,
        tests::{self, Report},
    };

    #[shared]
    struct Shared {
        #[lock_free]
        irq1: UartIrq,
        #[lock_free]
        irq7: UartIrq,
    }

    #[local]
    struct Local {
        usart1: UartRx<USART1>,
        feed1: Pin<'B', 6, Output>,
        uart7: UartRx<UART7>,
        feed7: Pin<'A', 15, Output>,
        vectors1: UartInterrupts,
        vectors7: UartInterrupts,
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
        let (_line1, usart1, irq1) = UartRx::new(
            &mut run.rcc,
            run.usart1,
            &board::SBUS,
            buffers.next().unwrap().rx,
        )
        .unwrap_or_else(|error| defmt::panic!("USART1: {}", error));
        let (_line7, uart7, irq7) = UartRx::new(
            &mut run.rcc,
            run.uart7,
            &board::MSP,
            buffers.next().unwrap().rx,
        )
        .unwrap_or_else(|error| defmt::panic!("UART7: {}", error));
        let vectors1 = irq1.interrupts();
        let vectors7 = irq7.interrupts();
        checks::spawn().ok();
        (
            Shared { irq1, irq7 },
            Local {
                usart1,
                feed1: run.feed1,
                uart7,
                feed7: run.feed7,
                vectors1,
                vectors7,
                led: run.led,
            },
        )
    }

    #[task(binds = USART1, priority = 2, shared = [irq1])]
    fn usart1_line(cx: usart1_line::Context) {
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = DMA2_STREAM4, priority = 2, shared = [irq1])]
    fn usart1_rx(cx: usart1_rx::Context) {
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = UART7, priority = 2, shared = [irq7])]
    fn uart7_line(cx: uart7_line::Context) {
        cx.shared.irq7.on_interrupt();
    }

    #[task(binds = DMA1_STREAM3, priority = 2, shared = [irq7])]
    fn uart7_rx(cx: uart7_rx::Context) {
        cx.shared.irq7.on_interrupt();
    }

    #[task(priority = 1, local = [usart1, feed1, uart7, feed7, vectors1, vectors7, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART1",
            "UART-003 RES-001",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors1),
        );
        report.check_for(
            "UART7",
            "UART-003 RES-001",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors7),
        );
        // The feed sends what the Lucid's receiver would: inverted frames.
        let sbus_feed = board::SBUS.with_inverted_tx(true);
        report.check_for(
            "USART1 receive-only PB7, SBUS 100000 8E2 inverted",
            "UART-007",
            "25-byte frames, one chunk each",
            tests::receive_only(cx.local.usart1, cx.local.feed1, &sbus_feed, 25),
        );
        report.check_for(
            "UART7 receive-only PB3, 115200",
            "UART-007",
            "40-byte frames, one chunk each",
            tests::receive_only(cx.local.uart7, cx.local.feed7, &board::MSP, 40),
        );
        defmt::info!("USART1: health {}", cx.local.usart1.health());
        defmt::info!("UART7: health {}", cx.local.uart7.health());
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
