//! Run Y: USART3 into UART4 over Z6, the H7's one cross-wired pair, for the
//! checks a loop cannot make: a receiver set to another frame than its
//! sender. PC10 is USART3's TX and PC11 UART4's RX, both on Z6; USART3
//! takes its RX on PB11 (Z7) and UART4 its TX on PD1 (Z5), and nothing
//! listens on those. Even parity into odd must reach UART4's reader as a
//! parity error [UART-014], and half the baud rate as a framing error
//! [UART-005 USART-002]; then both ends go back to 8N1 and a line must
//! cross again.
//!
//! RTIC owns every interrupt; FerroHAL only enables each UART's and its
//! receive stream's own interrupt bits. Each vector in use, and what serves
//! it:
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART3`, `DMA2_STREAM6`, `DMA2_STREAM7` | USART3 PC10's `UartIrq` | 2 |
//! | `UART4`, `DMA1_STREAM5`, `DMA1_STREAM7` | UART4 PC11's `UartIrq` | 2 |
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
        chip::{UART4, USART3},
        uart::{Config, Uart, UartInterrupts, UartIrq},
    };

    use crate::{
        board,
        tests::{self, Cross, Report},
    };

    const PLAIN: Config = Config::new(115_200);
    const NAME: &str = "USART3 PC10 -> UART4 PC11";

    #[shared]
    struct Shared {
        // Each handle's tasks run at one priority, so none preempts another
        // and the handle needs no lock.
        #[lock_free]
        irq_usart3: UartIrq,
        #[lock_free]
        irq_uart4: UartIrq,
    }

    #[local]
    struct Local {
        usart3: Uart<USART3>,
        vectors_usart3: UartInterrupts,
        uart4: Uart<UART4>,
        vectors_uart4: UartInterrupts,
        led: board::Led,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the waits.
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        // The buffers' block is non-cacheable before anything could cache it.
        board::dma_region(&mut core);

        let mut run = match board::run_y(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run Y, clocks: {}", board::CLOCKS);

        let mut buffers = board::dma_buffers().into_iter();
        let (usart3, irq_usart3) = board::open(
            &mut run.rcc,
            "USART3 PC10",
            run.usart3,
            &PLAIN,
            buffers.next().unwrap(),
        );
        let (uart4, irq_uart4) = board::open(
            &mut run.rcc,
            "UART4 PC11",
            run.uart4,
            &PLAIN,
            buffers.next().unwrap(),
        );
        let vectors_usart3 = irq_usart3.interrupts();
        let vectors_uart4 = irq_uart4.interrupts();
        checks::spawn().ok();
        (
            Shared {
                irq_usart3,
                irq_uart4,
            },
            Local {
                usart3,
                vectors_usart3,
                uart4,
                vectors_uart4,
                led: run.led,
            },
        )
    }

    #[task(binds = USART3, priority = 2, shared = [irq_usart3])]
    fn usart3_line(cx: usart3_line::Context) {
        cx.shared.irq_usart3.on_interrupt();
    }

    #[task(binds = DMA2_STREAM6, priority = 2, shared = [irq_usart3])]
    fn usart3_rx(cx: usart3_rx::Context) {
        cx.shared.irq_usart3.on_interrupt();
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [irq_usart3])]
    fn usart3_tx(cx: usart3_tx::Context) {
        cx.shared.irq_usart3.on_interrupt();
    }

    #[task(binds = UART4, priority = 2, shared = [irq_uart4])]
    fn uart4_line(cx: uart4_line::Context) {
        cx.shared.irq_uart4.on_interrupt();
    }

    #[task(binds = DMA1_STREAM5, priority = 2, shared = [irq_uart4])]
    fn uart4_rx(cx: uart4_rx::Context) {
        cx.shared.irq_uart4.on_interrupt();
    }

    #[task(binds = DMA1_STREAM7, priority = 2, shared = [irq_uart4])]
    fn uart4_tx(cx: uart4_tx::Context) {
        cx.shared.irq_uart4.on_interrupt();
    }

    #[task(priority = 1, local = [usart3, vectors_usart3, uart4, vectors_uart4, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART3 PC10",
            "UART-003",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors_usart3),
        );
        report.check_for(
            "UART4 PC11",
            "UART-003",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors_uart4),
        );
        let mut cross = Cross {
            name: NAME,
            from: cx.local.usart3,
            to: cx.local.uart4,
        };
        report.check_for(
            NAME,
            "UART-002",
            "a line crosses from one UART to the other",
            tests::echo(&mut cross, b"cross\n"),
        );
        report.check_for(
            NAME,
            "UART-014",
            "even into odd reported as parity error",
            tests::parity_mismatch(&mut cross),
        );
        report.check_for(
            NAME,
            "UART-005 USART-002",
            "half baud rate reported as framing error",
            tests::framing_mismatch(&mut cross),
        );
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
