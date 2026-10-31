//! FerroHAL's UART DMA loopback test on a Nucleo-H753ZI, under RTIC 2.
//!
//! Fit a jumper cap across CN9 pins 4 and 6 (PD6, USART2 RX, and PD5,
//! USART2 TX), then `cargo run --release`. Results come over RTT; the run
//! exits through semihosting with status 0 if every check passed, and LD1
//! lights.
//!
//! RTIC owns every interrupt; FerroHAL only enables USART2's and DMA1
//! stream 0's own interrupt bits. Each vector in use, and what serves it:
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART2` | `usart2`: line errors, through `UartIrq` | 2 |
//! | `DMA1_STREAM0` | `dma1_stream0`: ring half-laps and DMA errors | 2 |
//! | `SPI1` | RTIC, dispatching the priority 1 `loopback` task | 1 |

#![no_std]
#![no_main]

mod board;
#[path = "../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use ferrohal_stm32::{
        chip::check_dma_buffer,
        device::Interrupt,
        gpio::{Output, Pin},
        uart::{Buffers, Config, ConfigError, Uart, UartIrq},
    };

    use crate::{
        board,
        tests::{self, Loop, Report},
    };

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 64;

    #[shared]
    struct Shared {
        // Both interrupt tasks run at one priority, so neither preempts the
        // other and the handle needs no lock.
        #[lock_free]
        irq: UartIrq,
    }

    #[local]
    struct Local {
        uart: Uart,
        led: Pin<'B', 0, Output>,
    }

    #[init]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        // The cycle counter, for the wait-bound check.
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let mut board = match board::init(cx.device) {
            Ok(board) => board,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        // `freeze` must land on the tree worked out while compiling.
        defmt::assert_eq!(board.rcc.clocks(), board::CLOCKS);
        defmt::info!("clocks: {}", board::CLOCKS);
        board::caches(&mut cx.core);
        defmt::info!(
            "I-cache {=bool}, D-cache {=bool}",
            cortex_m::peripheral::SCB::icache_enabled(),
            cortex_m::peripheral::SCB::dcache_enabled()
        );

        let config = Config::new(115_200);
        let (tx, rx) = board::dma_buffers();
        defmt::assert_eq!(tx.len(), TX_BUFFER);
        defmt::assert_eq!(rx.len(), RX_RING);
        let buffers = Buffers { tx, rx };
        let (uart, irq) = match Uart::new(&mut board.rcc, board.loopback_uart, &config, buffers) {
            Ok(parts) => parts,
            Err(error) => defmt::panic!("uart: {}", error),
        };
        defmt::info!("USART2 up: {}", config);

        loopback::spawn().ok();
        (
            Shared { irq },
            Local {
                uart,
                led: board.led,
            },
        )
    }

    #[task(binds = USART2, priority = 2, shared = [irq])]
    fn usart2(cx: usart2::Context) {
        cx.shared.irq.on_interrupt();
    }

    #[task(binds = DMA1_STREAM0, priority = 2, shared = [irq])]
    fn dma1_stream0(cx: dma1_stream0::Context) {
        cx.shared.irq.on_interrupt();
    }

    #[task(priority = 1, local = [uart, led])]
    async fn loopback(cx: loopback::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();

        // `Uart::new` has accepted the non-cacheable buffers; these must
        // be refused. The stack is ordinary, cached AXI SRAM.
        let cached = core::hint::black_box([0u8; 32]);
        report.check(
            "D-cache on",
            "buffer in cached RAM refused",
            tests::expect_refusal(check_dma_buffer(&cached), ConfigError::BufferCacheable),
        );
        // SAFETY: never read; only its address is checked.
        let dtcm = unsafe { core::slice::from_raw_parts(0x2000_0000 as *const u8, 32) };
        report.check(
            "D-cache on",
            "buffer in DTCM refused",
            tests::expect_refusal(check_dma_buffer(dtcm), ConfigError::BufferUnreachable),
        );

        tests::full_suite(
            &mut report,
            &mut Loop {
                name: "USART2 PD5->PD6",
                uart: cx.local.uart,
            },
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::DMA1_STREAM0),
        );
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
