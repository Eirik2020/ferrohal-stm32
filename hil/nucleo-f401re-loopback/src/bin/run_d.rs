//! Run D: USART6 receive-only on PA12, at SBUS's 100000 8E2, fed over J2
//! by PA11 driven as a GPIO and bit-banging 25-byte frames. The F401 has no
//! RX inversion (the Foxeer F405 inverts SBUS in hardware); `board.rs`
//! checks that refusal while compiling.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART6`, `DMA2_STREAM1` | USART6's `UartIrq` | 2 |
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
        uart::{UartInterrupts, UartIrq, UartRx},
    };

    use crate::{
        board,
        tests::{self, Report},
    };

    const RX_RING: usize = 256;

    #[shared]
    struct Shared {
        #[lock_free]
        irq6: UartIrq,
    }

    #[local]
    struct Local {
        usart6: UartRx,
        feed: Pin<'A', 11, Output>,
        vectors6: UartInterrupts,
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
        let (usart6, irq6) =
            UartRx::new(&mut run.rcc, run.usart6, &board::SBUS_PLAIN, cx.local.rx6)
                .unwrap_or_else(|error| defmt::panic!("USART6: {}", error));
        let vectors6 = irq6.interrupts();
        checks::spawn().ok();
        (
            Shared { irq6 },
            Local {
                usart6,
                feed: run.feed,
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

    #[task(priority = 1, local = [usart6, feed, vectors6, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check(
            "USART6",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors6),
        );
        report.check(
            "USART6 receive-only PA12, 100000 8E2",
            "25-byte frames, one chunk each",
            tests::receive_only(cx.local.usart6, cx.local.feed, &board::SBUS_PLAIN, 25),
        );
        defmt::info!("USART6: health {}", cx.local.usart6.health());
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
