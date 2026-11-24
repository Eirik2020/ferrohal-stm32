//! Run A: every UART the F401 has, each on its own, then all three at once.
//!
//! - USART1 over J1 (PA9 to PA10): the full check list.
//! - USART6 over J2 (PA11 to PA12): the full check list.
//! - USART2 through the ST-LINK, with `hil/vcp_echo.py` sending bytes back:
//!   echo, stream and ring overrun. Breaks and baud changes do not survive
//!   the ST-LINK bridge, so they are left out.
//! - All three streaming at once, on DMA1 and DMA2 together.
//!
//! The clocks come from HSE (the ST-LINK's 8 MHz MCO, in bypass); the
//! other runs use HSI for the same frequencies. USART2's host echo checks
//! the result against the host's own clock.
//!
//! RTIC owns every interrupt; FerroHAL only enables the UARTs' and their
//! streams' own interrupt bits. Each UART is one shared resource, served by
//! its handlers and used by the `checks` task under RTIC's lock, one call at
//! a time (`tests::Locked`; `docs/design/uart-ownership.md`). Each vector in
//! use, and what serves it:
//!
//! | Vector | Serves | Priority |
//! | --- | --- | --- |
//! | `USART1`, `DMA2_STREAM2`, `DMA2_STREAM7` | USART1's `on_interrupt` | 2 |
//! | `USART6`, `DMA2_STREAM1`, `DMA2_STREAM6` | USART6's `on_interrupt` | 2 |
//! | `USART2`, `DMA1_STREAM5`, `DMA1_STREAM6` | USART2's `on_interrupt` | 2 |
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
        chip::Interrupt,
        uart::{Buffers, Config, Uart, Vectors},
    };

    use crate::{
        board,
        tests::{self, HostEcho, Locked, Loop, Report},
    };

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 256;

    #[shared]
    struct Shared {
        usart1: Uart<board::Usart1Loop>,
        usart6: Uart<board::Usart6Loop>,
        usart2: Uart<board::Usart2Vcp>,
    }

    #[local]
    struct Local {
        vectors1: Vectors,
        vectors6: Vectors,
        vectors2: Vectors,
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

        let config = Config::new(ferrohal_stm32::time::Hertz(115_200));
        let local = cx.local;
        let usart1 = Uart::new(
            &mut run.rcc,
            run.usart1,
            &config,
            Buffers {
                tx: local.tx1,
                rx: local.rx1,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("USART1: {}", error));
        let usart6 = Uart::new(
            &mut run.rcc,
            run.usart6,
            &config,
            Buffers {
                tx: local.tx6,
                rx: local.rx6,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("USART6: {}", error));
        let usart2 = Uart::new(
            &mut run.rcc,
            run.usart2,
            &config,
            Buffers {
                tx: local.tx2,
                rx: local.rx2,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("USART2: {}", error));

        let vectors1 = usart1.vectors();
        let vectors6 = usart6.vectors();
        let vectors2 = usart2.vectors();

        checks::spawn().ok();
        (
            Shared {
                usart1,
                usart6,
                usart2,
            },
            Local {
                vectors1,
                vectors6,
                vectors2,
                led: run.led,
            },
        )
    }

    #[task(binds = USART1, priority = 2, shared = [usart1])]
    fn usart1_line(mut cx: usart1_line::Context) {
        cx.shared.usart1.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA2_STREAM2, priority = 2, shared = [usart1])]
    fn usart1_rx(mut cx: usart1_rx::Context) {
        cx.shared.usart1.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [usart1])]
    fn usart1_tx(mut cx: usart1_tx::Context) {
        cx.shared.usart1.lock(|uart| tests::serve(uart));
    }

    #[task(binds = USART6, priority = 2, shared = [usart6])]
    fn usart6_line(mut cx: usart6_line::Context) {
        cx.shared.usart6.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA2_STREAM1, priority = 2, shared = [usart6])]
    fn usart6_rx(mut cx: usart6_rx::Context) {
        cx.shared.usart6.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA2_STREAM6, priority = 2, shared = [usart6])]
    fn usart6_tx(mut cx: usart6_tx::Context) {
        cx.shared.usart6.lock(|uart| tests::serve(uart));
    }

    #[task(binds = USART2, priority = 2, shared = [usart2])]
    fn usart2_line(mut cx: usart2_line::Context) {
        cx.shared.usart2.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM5, priority = 2, shared = [usart2])]
    fn usart2_rx(mut cx: usart2_rx::Context) {
        cx.shared.usart2.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM6, priority = 2, shared = [usart2])]
    fn usart2_tx(mut cx: usart2_tx::Context) {
        cx.shared.usart2.lock(|uart| tests::serve(uart));
    }

    #[task(
        priority = 1,
        shared = [usart1, usart6, usart2],
        local = [vectors1, vectors6, vectors2, led]
    )]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART1",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors1),
        );
        report.check_for(
            "USART6",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors6),
        );
        report.check_for(
            "USART2",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors2),
        );
        let mut usart1 = Locked(cx.shared.usart1);
        let mut usart6 = Locked(cx.shared.usart6);
        let mut usart2 = Locked(cx.shared.usart2);

        let mut loop1 = Loop {
            name: "USART1 PA9->PA10",
            uart: &mut usart1,
        };
        tests::full_suite(
            &mut report,
            &mut loop1,
            &Config::new(ferrohal_stm32::time::Hertz(115_200)),
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA2_STREAM2),
            &mut tests::nvic_mask_all([
                Interrupt::USART1,
                Interrupt::DMA2_STREAM2,
                Interrupt::DMA2_STREAM7,
            ]),
            false,
        );

        report.check_for(
            "USART1 PA9->PA10",
            "UART-020",
            "a masked line keeps its events out until unmasking",
            tests::line_mask(&mut loop1),
        );
        report.check_for(
            "USART1 PA9->PA10",
            "UART-DMA-002",
            "a handler held off by the task's lock runs within half a ring",
            tests::lock_delay(
                &mut loop1.uart.0,
                Interrupt::USART1,
                board::CLOCKS.cpu.0,
                115_200,
                RX_RING,
                TX_BUFFER,
            ),
        );
        tests::drain(&mut loop1);

        let mut loop6 = Loop {
            name: "USART6 PA11->PA12",
            uart: &mut usart6,
        };
        tests::full_suite(
            &mut report,
            &mut loop6,
            &Config::new(ferrohal_stm32::time::Hertz(115_200)),
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA2_STREAM1),
            &mut tests::nvic_mask_all([
                Interrupt::USART6,
                Interrupt::DMA2_STREAM1,
                Interrupt::DMA2_STREAM6,
            ]),
            false,
        );

        let mut host = HostEcho {
            name: "USART2 via host",
            uart: &mut usart2,
        };
        let name = host.name;
        tests::drain(&mut host);
        report.check_for(
            name,
            "UART-001 UART-002 UART-016",
            "echo",
            tests::echo(&mut host, b"hello host\n"),
        );
        report.check_for(
            name,
            "UART-002",
            "stream 8 KiB",
            tests::stream(&mut host, 8 * 1024),
        );
        report.check_for(
            name,
            "UART-009",
            "ring overrun reported",
            tests::ring_overrun(&mut host, RX_RING),
        );

        report.check_for(
            "USART1 + USART6 + USART2",
            "UART-002",
            "8 KiB each, all at once",
            tests::streams(&mut [&mut loop1, &mut loop6, &mut host], 8 * 1024),
        );

        report.check_for(
            "USART1 + USART6 + USART2",
            "RESOURCE-002",
            "every handler serves its UART through Service",
            tests::served(),
        );

        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
