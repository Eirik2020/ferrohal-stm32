//! Run E: UART4 over link F5 (PC10 to PC11), the full check list, and USART3
//! through the ST-LINK's virtual COM port, with `hil/vcp_echo.py` sending bytes
//! back: echo, stream and ring overrun (breaks and baud changes do not survive
//! the bridge). Then both streaming at once.
//!
//! Each UART's line vector and its two streams' vectors run its
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
        tests::{self, HostEcho, Locked, Loop, Report},
    };

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 256;

    #[shared]
    struct Shared {
        uart4: Uart<board::Uart4PortC>,
        usart3: Uart<board::Usart3Vcp>,
    }

    #[local]
    struct Local {
        vectors_uart4: Vectors,
        vectors_usart3: Vectors,
        led: board::Led,
    }

    #[init(local = [
        tx_uart4: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx_uart4: [u8; RX_RING] = [0; RX_RING],
        tx_usart3: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx_usart3: [u8; RX_RING] = [0; RX_RING],
    ])]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        // The cycle counter, for the wait-bound check; the F7's Cortex-M7
        // ignores DWT writes until it is unlocked.
        cortex_m::peripheral::DWT::unlock();
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let mut run = match board::run_e(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run E, clocks: {}", board::CLOCKS);

        let config = Config::new(Hertz(115_200));
        let local = cx.local;
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
        let usart3 = Uart::new(
            &mut run.rcc,
            run.usart3,
            &config,
            Buffers {
                tx: local.tx_usart3,
                rx: local.rx_usart3,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("usart3: {}", error));
        let vectors_usart3 = usart3.vectors();

        checks::spawn().ok();
        (
            Shared { uart4, usart3 },
            Local {
                vectors_uart4,
                vectors_usart3,
                led: run.led,
            },
        )
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

    #[task(binds = USART3, priority = 2, shared = [usart3])]
    fn usart3_line(mut cx: usart3_line::Context) {
        cx.shared.usart3.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM1, priority = 2, shared = [usart3])]
    fn usart3_rx(mut cx: usart3_rx::Context) {
        cx.shared.usart3.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM3, priority = 2, shared = [usart3])]
    fn usart3_tx(mut cx: usart3_tx::Context) {
        cx.shared.usart3.lock(|uart| tests::serve(uart));
    }

    #[task(priority = 1, shared = [uart4, usart3], local = [vectors_uart4, vectors_usart3, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "UART4 PC10->PC11",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors_uart4),
        );
        report.check_for(
            "USART3 via host",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors_usart3),
        );
        let mut uart4 = Locked(cx.shared.uart4);
        let mut usart3 = Locked(cx.shared.usart3);

        let mut loop_uart4 = Loop {
            name: "UART4 PC10->PC11",
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

        let mut host = HostEcho {
            name: "USART3 via host",
            uart: &mut usart3,
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
            "UART4 + USART3",
            "UART-002",
            "8 KiB each, all at once",
            tests::streams(&mut [&mut loop_uart4, &mut host], 8 * 1024),
        );
        report.check_for(
            "UART4 + USART3",
            "RESOURCE-002",
            "every handler serves its UART through Service",
            tests::served(),
        );
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
