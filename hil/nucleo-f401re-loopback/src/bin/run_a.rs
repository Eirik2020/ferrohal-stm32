//! Run A: every UART the F401 has, each on its own, then all three at once.
//!
//! - USART1 over J1 (PA9 to PA10): the full check list.
//! - USART6 over J2 (PA11 to PA12): the full check list.
//! - USART2 through the ST-LINK, with `hil/vcp_echo.py` sending bytes back:
//!   echo, stream and ring overrun. Breaks and baud changes do not survive
//!   the ST-LINK bridge, so they are left out.
//! - All three streaming at once, on DMA1 and DMA2 together.
//!
//! RTIC owns every interrupt; FerroHAL only enables the UARTs' and their
//! receive streams' own interrupt bits. Each vector in use, and what serves
//! it:
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART1`, `DMA2_STREAM2` | USART1's `UartIrq` | 2 |
//! | `USART6`, `DMA2_STREAM1` | USART6's `UartIrq` | 2 |
//! | `USART2`, `DMA1_STREAM5` | USART2's `UartIrq` | 2 |
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
        uart::{Buffers, Config, Uart, UartIrq},
    };

    use crate::{
        board,
        tests::{self, HostEcho, Loop, Report},
    };

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 64;

    #[shared]
    struct Shared {
        // Each handle's two tasks run at one priority, so neither preempts
        // the other and the handle needs no lock.
        #[lock_free]
        irq1: UartIrq,
        #[lock_free]
        irq6: UartIrq,
        #[lock_free]
        irq2: UartIrq,
    }

    #[local]
    struct Local {
        usart1: Uart,
        usart6: Uart,
        usart2: Uart,
        led: board::Led,
    }

    #[init(local = [
        tx1: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx1: [u8; RX_RING] = [0; RX_RING],
        tx6: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx6: [u8; RX_RING] = [0; RX_RING],
        tx2: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx2: [u8; RX_RING] = [0; RX_RING],
    ])]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        // The cycle counter, for the wait-bound check.
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let mut run = match board::run_a(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run A, clocks: {}", board::CLOCKS);

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
        let (usart2, irq2) = Uart::new(
            &mut run.rcc,
            run.usart2,
            &config,
            Buffers {
                tx: local.tx2,
                rx: local.rx2,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("USART2: {}", error));

        checks::spawn().ok();
        (
            Shared { irq1, irq6, irq2 },
            Local {
                usart1,
                usart6,
                usart2,
                led: run.led,
            },
        )
    }

    #[task(binds = USART1, priority = 2, shared = [irq1])]
    fn usart1_line(cx: usart1_line::Context) {
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = DMA2_STREAM2, priority = 2, shared = [irq1])]
    fn usart1_rx(cx: usart1_rx::Context) {
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = USART6, priority = 2, shared = [irq6])]
    fn usart6_line(cx: usart6_line::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(binds = DMA2_STREAM1, priority = 2, shared = [irq6])]
    fn usart6_rx(cx: usart6_rx::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(binds = USART2, priority = 2, shared = [irq2])]
    fn usart2_line(cx: usart2_line::Context) {
        cx.shared.irq2.on_interrupt();
    }

    #[task(binds = DMA1_STREAM5, priority = 2, shared = [irq2])]
    fn usart2_rx(cx: usart2_rx::Context) {
        cx.shared.irq2.on_interrupt();
    }

    #[task(priority = 1, local = [usart1, usart6, usart2, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let (usart1, usart6, usart2) = (cx.local.usart1, cx.local.usart6, cx.local.usart2);

        let mut loop1 = Loop {
            name: "USART1 PA9->PA10",
            uart: usart1,
        };
        tests::full_suite(
            &mut report,
            &mut loop1,
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA2_STREAM2),
        );

        let mut loop6 = Loop {
            name: "USART6 PA11->PA12",
            uart: usart6,
        };
        tests::full_suite(
            &mut report,
            &mut loop6,
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA2_STREAM1),
        );

        let mut host = HostEcho {
            name: "USART2 via host",
            uart: usart2,
        };
        let name = host.name;
        tests::drain(&mut host);
        report.check(name, "echo", tests::echo(&mut host, b"hello host\n"));
        report.check(name, "stream 8 KiB", tests::stream(&mut host, 8 * 1024));
        report.check(
            name,
            "ring overrun reported",
            tests::ring_overrun(&mut host, RX_RING),
        );

        report.check(
            "USART1 + USART6 + USART2",
            "8 KiB each, all at once",
            tests::streams(&mut [&mut loop1, &mut loop6, &mut host], 8 * 1024),
        );

        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
