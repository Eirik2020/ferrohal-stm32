//! Run B: USART1 and USART6 wired to each other, each on its other pins and
//! other receive stream.
//!
//! - USART1 TX (PB6) over J3 to USART6 RX (PC7), and USART6 TX (PC6) over
//!   J4 to USART1 RX (PB7): echo, stream and break, each way.
//! - Both directions streaming at once.
//! - Deliberate mismatches, each way: half the baud rate must give a
//!   framing error, even parity into odd a parity error. The receiving
//!   USART is not transmitting, so ES0299 2.10.2 cannot hide the parity
//!   error.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART1`, `DMA2_STREAM5`, `DMA2_STREAM7` | USART1's `UartIrq` | 2 |
//! | `USART6`, `DMA2_STREAM2`, `DMA2_STREAM6` | USART6's `UartIrq` | 2 |
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
    use ferrohal_stm32::uart::{Buffers, Config, Uart, UartIrq};

    use crate::{
        board,
        tests::{self, Cross, Pair, Report},
    };

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 256;

    #[shared]
    struct Shared {
        #[lock_free]
        irq1: UartIrq,
        #[lock_free]
        irq6: UartIrq,
    }

    #[local]
    struct Local {
        usart1: Uart,
        usart6: Uart,
        led: board::Led,
    }

    #[init(local = [
        tx1: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx1: [u8; RX_RING] = [0; RX_RING],
        tx6: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx6: [u8; RX_RING] = [0; RX_RING],
    ])]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        // The cycle counter, for the wait-bound check.
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let mut run = match board::run_b(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run B, clocks: {}", board::CLOCKS);

        let config = Config::new(115_200);
        let local = cx.local;
        let (usart1, irq1) = Uart::new(
            &mut run.rcc,
            run.usart1,
            &config,
            Buffers {
                tx: local.tx1,
                rx: local.rx1,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("USART1: {}", error));
        let (usart6, irq6) = Uart::new(
            &mut run.rcc,
            run.usart6,
            &config,
            Buffers {
                tx: local.tx6,
                rx: local.rx6,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("USART6: {}", error));

        checks::spawn().ok();
        (
            Shared { irq1, irq6 },
            Local {
                usart1,
                usart6,
                led: run.led,
            },
        )
    }

    #[task(binds = USART1, priority = 2, shared = [irq1])]
    fn usart1_line(cx: usart1_line::Context) {
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = DMA2_STREAM5, priority = 2, shared = [irq1])]
    fn usart1_rx(cx: usart1_rx::Context) {
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [irq1])]
    fn usart1_tx(cx: usart1_tx::Context) {
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = USART6, priority = 2, shared = [irq6])]
    fn usart6_line(cx: usart6_line::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(binds = DMA2_STREAM2, priority = 2, shared = [irq6])]
    fn usart6_rx(cx: usart6_rx::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(binds = DMA2_STREAM6, priority = 2, shared = [irq6])]
    fn usart6_tx(cx: usart6_tx::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(priority = 1, local = [usart1, usart6, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let (usart1, usart6) = (cx.local.usart1, cx.local.usart6);

        for forward in [true, false] {
            let mut cross = if forward {
                Cross {
                    name: "USART1 PB6->USART6 PC7",
                    from: &mut *usart1,
                    to: &mut *usart6,
                }
            } else {
                Cross {
                    name: "USART6 PC6->USART1 PB7",
                    from: &mut *usart6,
                    to: &mut *usart1,
                }
            };
            let name = cross.name;
            tests::drain(&mut cross);
            report.check(name, "echo", tests::echo(&mut cross, b"hello across\n"));
            report.check(name, "stream 8 KiB", tests::stream(&mut cross, 8 * 1024));
            report.check(name, "break reported", tests::line_break(&mut cross));
            report.check(
                name,
                "half baud rate reported as framing error",
                tests::framing_mismatch(&mut cross),
            );
            report.check(
                name,
                "even into odd reported as parity error",
                tests::parity_mismatch(&mut cross),
            );
        }

        let (tx1, rx1) = usart1.halves();
        let (tx6, rx6) = usart6.halves();
        report.check(
            "USART1 <-> USART6",
            "8 KiB each way, both at once",
            tests::streams(
                &mut [
                    &mut Pair {
                        name: "USART1->USART6",
                        tx: tx1,
                        rx: rx6,
                    },
                    &mut Pair {
                        name: "USART6->USART1",
                        tx: tx6,
                        rx: rx1,
                    },
                ],
                8 * 1024,
            ),
        );

        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
