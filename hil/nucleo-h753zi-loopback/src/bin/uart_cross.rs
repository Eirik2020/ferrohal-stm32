//! Run Y: USART3 into UART4 over Z6, the H7's one cross-wired pair, for the
//! checks a loop cannot make: a receiver set to another frame than its
//! sender. PC10 is USART3's TX and PC11 UART4's RX, both on Z6; USART3
//! takes its RX on PB11 (Z7) and UART4 its TX on PD1 (Z5), and nothing
//! listens on those. Even parity into odd must reach UART4's reader as a
//! parity error [UART-014], and half the baud rate as a framing error
//! [UART-005 UART-017]; then both ends go back to 8N1 and a line must
//! cross again. In half-duplex USART3's receiver listens on PC10, so a
//! frame it sends reaches UART4 and its own ring; in full duplex its ring,
//! on the unwired PB11, stays silent [UART-021]. Swapped, USART3 sends on
//! PB11, so UART4 must hear nothing [USART-V4-002].
//!
//! RTIC owns every interrupt; FerroHAL only enables each UART's and its
//! streams' own interrupt bits. Each UART is one shared resource, served by
//! its handlers and used by the `checks` task under RTIC's lock, one call at
//! a time (`tests::Locked`; `docs/design/uart-ownership.md`). Each vector in
//! use, and what serves it:
//!
//! | Vector | Serves | Priority |
//! | --- | --- | --- |
//! | `USART3`, `DMA2_STREAM6`, `DMA2_STREAM7` | USART3 PC10's `on_interrupt` | 2 |
//! | `UART4`, `DMA1_STREAM5`, `DMA1_STREAM7` | UART4 PC11's `on_interrupt` | 2 |
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
    use ferrohal_stm32::uart::{Config, Uart, Vectors};

    use crate::{
        board,
        tests::{self, Cross, Locked, Report},
    };

    const PLAIN: Config = Config::new(ferrohal_stm32::time::Hertz(115_200));
    const NAME: &str = "USART3 PC10 -> UART4 PC11";

    #[shared]
    struct Shared {
        usart3: Uart<board::Usart3Y>,
        uart4: Uart<board::Uart4Y>,
    }

    #[local]
    struct Local {
        vectors_usart3: Vectors,
        vectors_uart4: Vectors,
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
        let usart3 = board::open(
            &mut run.rcc,
            "USART3 PC10",
            run.usart3,
            &PLAIN,
            buffers.next().unwrap(),
        );
        let uart4 = board::open(
            &mut run.rcc,
            "UART4 PC11",
            run.uart4,
            &PLAIN,
            buffers.next().unwrap(),
        );
        let vectors_usart3 = usart3.vectors();
        let vectors_uart4 = uart4.vectors();
        checks::spawn().ok();
        (
            Shared { usart3, uart4 },
            Local {
                vectors_usart3,
                vectors_uart4,
                led: run.led,
            },
        )
    }

    #[task(binds = USART3, priority = 2, shared = [usart3])]
    fn usart3_line(mut cx: usart3_line::Context) {
        cx.shared.usart3.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA2_STREAM6, priority = 2, shared = [usart3])]
    fn usart3_rx(mut cx: usart3_rx::Context) {
        cx.shared.usart3.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [usart3])]
    fn usart3_tx(mut cx: usart3_tx::Context) {
        cx.shared.usart3.lock(|uart| tests::serve(uart));
    }

    #[task(binds = UART4, priority = 2, shared = [uart4])]
    fn uart4_line(mut cx: uart4_line::Context) {
        cx.shared.uart4.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM5, priority = 2, shared = [uart4])]
    fn uart4_rx(mut cx: uart4_rx::Context) {
        cx.shared.uart4.lock(|uart| tests::serve(uart));
    }

    #[task(binds = DMA1_STREAM7, priority = 2, shared = [uart4])]
    fn uart4_tx(mut cx: uart4_tx::Context) {
        cx.shared.uart4.lock(|uart| tests::serve(uart));
    }

    #[task(
        priority = 1,
        shared = [usart3, uart4],
        local = [vectors_usart3, vectors_uart4, led]
    )]
    async fn checks(cx: checks::Context) {
        let mut usart3 = Locked(cx.shared.usart3);
        let mut uart4 = Locked(cx.shared.uart4);
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART3 PC10",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors_usart3),
        );
        report.check_for(
            "UART4 PC11",
            "UART-003 RESOURCE-001",
            "vectors enabled",
            tests::vectors_verified(cx.local.vectors_uart4),
        );
        let mut cross = Cross {
            name: NAME,
            from: &mut usart3,
            to: &mut uart4,
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
            "UART-005 UART-017",
            "half baud rate reported as framing error",
            tests::framing_mismatch(&mut cross),
        );
        report.check_for(
            NAME,
            "UART-021",
            "half-duplex USART3: a frame out reaches UART4 and USART3's own ring; in full duplex its ring stays silent",
            tests::half_duplex_one_way(
                &mut cross,
                &Config::new(ferrohal_stm32::time::Hertz(115_200)),
            ),
        );
        report.check_for(
            NAME,
            "USART-V4-002",
            "a swapped USART3 sends on PB11, so UART4 hears nothing",
            tests::swap_moves_tx(&mut cross),
        );
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
