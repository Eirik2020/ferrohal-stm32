//! Run D: five UARTs over links F1, F2, F3, F4 and F6, each the full check
//! list, then all five streaming at once on DMA1 and DMA2.
//!
//! RTIC owns every interrupt; each UART is one shared resource, served by its
//! handlers and used by the `checks` task under RTIC's lock (`tests::Locked`).
//! Every UART's line vector and its two streams' vectors run its
//! `on_interrupt` at priority 2; `SPDIF_RX` dispatches `checks` at 1.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPDIF_RX])]
mod app {
    use ferrohal_stm32::{
        chip::Interrupt,
        time::Hertz,
        uart::{Buffers, Config, Uart, Vectors},
    };

    use crate::{
        board,
        tests::{self, Locked, Loop, Report},
    };

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 256;

    #[shared]
    struct Shared {
        usart2: Uart<board::Usart2Loop>,
        usart6: Uart<board::Usart6Loop>,
        uart4: Uart<board::Uart4Loop>,
        uart5: Uart<board::Uart5Loop>,
        uart7: Uart<board::Uart7Loop>,
    }

    #[local]
    struct Local {
        vectors_usart2: Vectors,
        vectors_usart6: Vectors,
        vectors_uart4: Vectors,
        vectors_uart5: Vectors,
        vectors_uart7: Vectors,
        led: board::Led,
    }

    #[init(local = [
        tx_usart2: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx_usart2: [u8; RX_RING] = [0; RX_RING],
        tx_usart6: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx_usart6: [u8; RX_RING] = [0; RX_RING],
        tx_uart4: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx_uart4: [u8; RX_RING] = [0; RX_RING],
        tx_uart5: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx_uart5: [u8; RX_RING] = [0; RX_RING],
        tx_uart7: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx_uart7: [u8; RX_RING] = [0; RX_RING],
    ])]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        // The cycle counter, for the wait-bound check; the F7's Cortex-M7
        // ignores DWT writes until it is unlocked.
        cortex_m::peripheral::DWT::unlock();
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let mut run = match board::run_d(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run D, clocks: {}", board::CLOCKS);

        let config = Config::new(Hertz(115_200));
        let local = cx.local;
        let usart2 = Uart::new(
            &mut run.rcc,
            run.usart2,
            &config,
            Buffers {
                tx: local.tx_usart2,
                rx: local.rx_usart2,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("usart2: {}", error));
        let vectors_usart2 = usart2.vectors();
        let usart6 = Uart::new(
            &mut run.rcc,
            run.usart6,
            &config,
            Buffers {
                tx: local.tx_usart6,
                rx: local.rx_usart6,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("usart6: {}", error));
        let vectors_usart6 = usart6.vectors();
        let uart4 = Uart::new(
            &mut run.rcc,
            run.uart4,
            &config,
            Buffers {
                tx: local.tx_uart4,
                rx: local.rx_uart4,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("uart4: {}", error));
        let vectors_uart4 = uart4.vectors();
        let uart5 = Uart::new(
            &mut run.rcc,
            run.uart5,
            &config,
            Buffers {
                tx: local.tx_uart5,
                rx: local.rx_uart5,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("uart5: {}", error));
        let vectors_uart5 = uart5.vectors();
        let uart7 = Uart::new(
            &mut run.rcc,
            run.uart7,
            &config,
            Buffers {
                tx: local.tx_uart7,
                rx: local.rx_uart7,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("uart7: {}", error));
        let vectors_uart7 = uart7.vectors();

        checks::spawn().ok();
        (
            Shared {
                usart2,
                usart6,
                uart4,
                uart5,
                uart7,
            },
            Local {
                vectors_usart2,
                vectors_usart6,
                vectors_uart4,
                vectors_uart5,
                vectors_uart7,
                led: run.led,
            },
        )
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

    #[task(binds = UART4, priority = 2, shared = [uart4])]
    fn uart4_line(mut cx: uart4_line::Context) {
        cx.shared.uart4.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM2, priority = 2, shared = [uart4])]
    fn uart4_rx(mut cx: uart4_rx::Context) {
        cx.shared.uart4.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM4, priority = 2, shared = [uart4])]
    fn uart4_tx(mut cx: uart4_tx::Context) {
        cx.shared.uart4.lock(|uart| tests::serve(uart));
    }

    #[task(binds = UART5, priority = 2, shared = [uart5])]
    fn uart5_line(mut cx: uart5_line::Context) {
        cx.shared.uart5.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM0, priority = 2, shared = [uart5])]
    fn uart5_rx(mut cx: uart5_rx::Context) {
        cx.shared.uart5.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM7, priority = 2, shared = [uart5])]
    fn uart5_tx(mut cx: uart5_tx::Context) {
        cx.shared.uart5.lock(|uart| tests::serve(uart));
    }

    #[task(binds = UART7, priority = 2, shared = [uart7])]
    fn uart7_line(mut cx: uart7_line::Context) {
        cx.shared.uart7.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM3, priority = 2, shared = [uart7])]
    fn uart7_rx(mut cx: uart7_rx::Context) {
        cx.shared.uart7.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM1, priority = 2, shared = [uart7])]
    fn uart7_tx(mut cx: uart7_tx::Context) {
        cx.shared.uart7.lock(|uart| tests::serve(uart));
    }

    #[task(priority = 1, shared = [usart2, usart6, uart4, uart5, uart7], local = [vectors_usart2, vectors_usart6, vectors_uart4, vectors_uart5, vectors_uart7, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART2 PD5->PD6",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors_usart2),
        );
        report.check_for(
            "USART6 PG14->PG9",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors_usart6),
        );
        report.check_for(
            "UART4 PD1->PD0",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors_uart4),
        );
        report.check_for(
            "UART5 PC12->PD2",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors_uart5),
        );
        report.check_for(
            "UART7 PE8->PE7",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors_uart7),
        );
        let mut usart2 = Locked(cx.shared.usart2);
        let mut usart6 = Locked(cx.shared.usart6);
        let mut uart4 = Locked(cx.shared.uart4);
        let mut uart5 = Locked(cx.shared.uart5);
        let mut uart7 = Locked(cx.shared.uart7);

        let mut loop_usart2 = Loop {
            name: "USART2 PD5->PD6",
            uart: &mut usart2,
        };
        tests::full_suite(
            &mut report,
            &mut loop_usart2,
            &Config::new(Hertz(115_200)),
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA1_STREAM5),
            &mut tests::nvic_mask_all([
                Interrupt::USART2,
                Interrupt::DMA1_STREAM5,
                Interrupt::DMA1_STREAM6,
            ]),
            true,
        );

        let mut loop_usart6 = Loop {
            name: "USART6 PG14->PG9",
            uart: &mut usart6,
        };
        tests::full_suite(
            &mut report,
            &mut loop_usart6,
            &Config::new(Hertz(115_200)),
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA2_STREAM1),
            &mut tests::nvic_mask_all([
                Interrupt::USART6,
                Interrupt::DMA2_STREAM1,
                Interrupt::DMA2_STREAM6,
            ]),
            true,
        );

        let mut loop_uart4 = Loop {
            name: "UART4 PD1->PD0",
            uart: &mut uart4,
        };
        tests::full_suite(
            &mut report,
            &mut loop_uart4,
            &Config::new(Hertz(115_200)),
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA1_STREAM2),
            &mut tests::nvic_mask_all([
                Interrupt::UART4,
                Interrupt::DMA1_STREAM2,
                Interrupt::DMA1_STREAM4,
            ]),
            true,
        );

        let mut loop_uart5 = Loop {
            name: "UART5 PC12->PD2",
            uart: &mut uart5,
        };
        tests::full_suite(
            &mut report,
            &mut loop_uart5,
            &Config::new(Hertz(115_200)),
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA1_STREAM0),
            &mut tests::nvic_mask_all([
                Interrupt::UART5,
                Interrupt::DMA1_STREAM0,
                Interrupt::DMA1_STREAM7,
            ]),
            true,
        );

        let mut loop_uart7 = Loop {
            name: "UART7 PE8->PE7",
            uart: &mut uart7,
        };
        tests::full_suite(
            &mut report,
            &mut loop_uart7,
            &Config::new(Hertz(115_200)),
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA1_STREAM3),
            &mut tests::nvic_mask_all([
                Interrupt::UART7,
                Interrupt::DMA1_STREAM3,
                Interrupt::DMA1_STREAM1,
            ]),
            true,
        );

        report.check_for(
            "USART2 + USART6 + UART4 + UART5 + UART7",
            "UART-002",
            "8 KiB each, all at once",
            tests::streams(
                &mut [
                    &mut loop_usart2,
                    &mut loop_usart6,
                    &mut loop_uart4,
                    &mut loop_uart5,
                    &mut loop_uart7,
                ],
                8 * 1024,
            ),
        );
        report.check_for(
            "USART2 + USART6 + UART4 + UART5 + UART7",
            "RESOURCE-002",
            "every handler serves its UART through Service",
            tests::served(),
        );
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
