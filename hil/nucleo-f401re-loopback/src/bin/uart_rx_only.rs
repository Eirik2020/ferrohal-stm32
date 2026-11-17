//! Run D: USART6 receive-only on PA12, at SBUS's 100000 8E2, fed over J2
//! by PA11 driven as a GPIO and bit-banging 25-byte frames. The F401 has no
//! RX inversion (the Foxeer F405 inverts SBUS in hardware); `board.rs`
//! checks that refusal while compiling.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART6`, `DMA2_STREAM1` | USART6's `on_interrupt` | 2 |
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

    const RX_RING: usize = 256;

    #[shared]
    struct Shared {
        usart6: Uart<board::Usart6RxOnly>,
    }

    #[local]
    struct Local {
        feed: Pin<'A', 11, Output>,
        vectors6: Vectors,
        led: board::Led,
    }

    #[init(local = [rx6: [u8; RX_RING] = [0; RX_RING]])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        // The cycle counter, for the bit-banged frames.
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_d(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run D, clocks: {}", board::CLOCKS);
        let usart6 = Uart::new_rx(&mut run.rcc, run.usart6, &board::SBUS_PLAIN, cx.local.rx6)
            .unwrap_or_else(|error| defmt::panic!("USART6: {}", error));
        let vectors6 = usart6.vectors();
        checks::spawn().ok();
        (
            Shared { usart6 },
            Local {
                feed: run.feed,
                vectors6,
                led: run.led,
            },
        )
    }

    #[task(binds = USART6, priority = 2, shared = [usart6])]
    fn usart6_line(mut cx: usart6_line::Context) {
        cx.shared.usart6.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA2_STREAM1, priority = 2, shared = [usart6])]
    fn usart6_rx(mut cx: usart6_rx::Context) {
        cx.shared.usart6.lock(|uart| tests::serve(uart));
    }

    #[task(priority = 1, shared = [usart6], local = [feed, vectors6, led])]
    async fn checks(cx: checks::Context) {
        let mut usart6 = tests::Locked(cx.shared.usart6);
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART6",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors6),
        );
        report.check_for(
            "USART6 receive-only PA12, 100000 8E2",
            "UART-007",
            "25-byte frames, one chunk each",
            tests::receive_only(&mut usart6, cx.local.feed, &board::SBUS_PLAIN, 25),
        );
        defmt::info!("USART6: health {}", tests::RxHalf::health(&mut usart6));
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
