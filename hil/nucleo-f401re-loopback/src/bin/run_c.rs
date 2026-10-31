//! Run C: USART6 over J2 (PA11 to PA12) on DMA2 stream 7, its other
//! transmit stream, which USART1 holds in runs A and B. The full check
//! list.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART6`, `DMA2_STREAM1`, `DMA2_STREAM7` | USART6's `UartIrq` | 2 |
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
        device::Interrupt,
        uart::{Buffers, Config, Uart, UartInterrupts, UartIrq},
    };

    use crate::{
        board,
        tests::{self, Loop, Report},
    };

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 256;

    #[shared]
    struct Shared {
        #[lock_free]
        irq6: UartIrq,
    }

    #[local]
    struct Local {
        usart6: Uart,
        vectors6: UartInterrupts,
        led: board::Led,
    }

    #[init(local = [
        tx6: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx6: [u8; RX_RING] = [0; RX_RING],
    ])]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        // The cycle counter, for the wait-bound check.
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let mut run = match board::run_c(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run C, clocks: {}", board::CLOCKS);

        let (usart6, irq6) = Uart::new(
            &mut run.rcc,
            run.usart6,
            &Config::new(115_200),
            Buffers {
                tx: cx.local.tx6,
                rx: cx.local.rx6,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("USART6: {}", error));

        let vectors6 = irq6.interrupts();

        checks::spawn().ok();
        (
            Shared { irq6 },
            Local {
                usart6,
                vectors6,
                led: run.led,
            },
        )
    }

    #[task(binds = USART6, priority = 2, shared = [irq6])]
    fn usart6_line(cx: usart6_line::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(binds = DMA2_STREAM1, priority = 2, shared = [irq6])]
    fn usart6_rx(cx: usart6_rx::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [irq6])]
    fn usart6_tx(cx: usart6_tx::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(priority = 1, local = [usart6, vectors6, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check(
            "USART6",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors6),
        );
        tests::full_suite(
            &mut report,
            &mut Loop {
                name: "USART6 PA11->PA12, TX on stream 7",
                uart: cx.local.usart6,
            },
            &Config::new(115_200),
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA2_STREAM1),
            &mut tests::nvic_mask_all([
                Interrupt::USART6,
                Interrupt::DMA2_STREAM1,
                Interrupt::DMA2_STREAM7,
            ]),
            false,
        );
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
