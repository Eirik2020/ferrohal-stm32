//! Run A: die 0x413 itself, then each of its own UART routes once.
//!
//! - The die: `DEV_ID` 0x413; the clock tree at 168 MHz from HSE bypass,
//!   voltage scale 1 (PWR_CR bit 14, this die's one VOS bit) with VOSRDY,
//!   five flash wait states; CCM RAM refused for DMA, SRAM accepted.
//! - The routes the F401 does not have (the car rule: `usart_v2` is the
//!   F401's to prove), each opened, echoed, streamed and freed, its parts
//!   handed to the next: USART3 over D1 (TX DMA1 stream 3), UART4 over D1
//!   (stream 4, channel 4), USART3 over D3 (stream 4, channel 7), UART5 over
//!   D2. Between them every DMA route the three have.
//!
//! The routes are polled: no vector is bound, so RTIC leaves every one
//! masked, and each route runs on the service its own calls make (queued
//! frames chained by the next call, the receive stream served by each read:
//! UARTDMA-008). The four routes are four `Uart` types, which no one shared
//! slot could hold; what this run proves is the die's routing glue, which
//! the interrupt path does not change.

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
        chip::check_dma_buffer,
        dma::BufferError,
        uart::{Buffers, Config, Uart, Wiring},
    };

    use crate::{
        board,
        tests::{self, Fail, Loop, Report},
    };

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 256;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunA>,
        buffers: Option<Buffers>,
        sram: &'static mut [u8; 64],
    }

    #[init(local = [
        tx_store: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx_store: [u8; RX_RING] = [0; RX_RING],
        sram_store: [u8; 64] = [0; 64],
    ])]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let run = match board::run_a(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::info!("run A, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {},
            Local {
                run: Some(run),
                buffers: Some(Buffers {
                    tx: cx.local.tx_store,
                    rx: cx.local.rx_store,
                }),
                sram: cx.local.sram_store,
            },
        )
    }

    /// Opens a route's wiring, echoes and streams over it, and frees it to
    /// its parts and buffers.
    macro_rules! route {
        ($report:expr, $rcc:expr, $name:expr, $wiring:expr, $buffers:expr) => {{
            let name: &'static str = $name;
            let mut uart = match Uart::new($rcc, $wiring, &Config::new(ferrohal_stm32::time::Hertz(115_200)), $buffers) {
                Ok(opened) => opened,
                Err(_) => defmt::panic!("{=str}: refused", name),
            };
            let mut link = Loop { name, uart: &mut uart };
            tests::drain(&mut link);
            $report.check_for(
                name,
                "UART-001 UART-002 GLUE-001",
                "echo",
                tests::echo(&mut link, b"die 0x413 route\n"),
            );
            $report.check_for(
                name,
                "UART-002 GLUE-002",
                "stream 4 KiB over its DMA route",
                tests::stream(&mut link, 4 * 1024),
            );
            match uart.free() {
                Ok(parts) => parts,
                Err(_) => defmt::panic!("{}: free refused, a stream did not stop", name),
            }
        }};
    }

    #[task(priority = 1, local = [run, buffers, sram])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let local = cx.local;
        let Some(mut run) = local.run.take() else {
            defmt::panic!("run A's parts were taken")
        };
        let die = "die 0x413";

        let dev_id = stm32_metapac::DBGMCU.idcode().read().dev_id();
        report.check_for(die, "GLUE-001", "DEV_ID is 0x413", expect(0x413, u32::from(dev_id)));
        report.check_for(die, "RCC-001", "freeze gives the solved clocks", if run.rcc.clocks() == board::CLOCKS { Ok(()) } else { Err(Fail::Count { expected: 1, got: 0 }) });
        let vos = stm32_metapac::PWR.cr1().read().vos().to_bits();
        let ready = stm32_metapac::PWR.csr1().read().vosrdy();
        report.check_for(die, "RCC-003", "voltage scale 1 (VOS bit 14 set), VOSRDY", if vos & 0b01 == 0b01 && ready { Ok(()) } else { Err(Fail::Count { expected: 1, got: u32::from(vos) }) });
        let latency = stm32_metapac::FLASH.acr().read().latency().to_bits();
        report.check_for(die, "RCC-001", "five flash wait states at 168 MHz", expect(5, u32::from(latency)));

        // SAFETY: CCM RAM's first 64 bytes, read only by `check_dma_buffer`,
        // which looks at the address alone.
        let ccm: &[u8] = unsafe { core::slice::from_raw_parts(0x1000_0000 as *const u8, 64) };
        report.check_for(die, "GLUE-004", "a buffer in CCM RAM is refused", if check_dma_buffer(ccm) == Err(BufferError::Unreachable) { Ok(()) } else { Err(Fail::Count { expected: 1, got: 0 }) });
        report.check_for(die, "GLUE-004", "a buffer in SRAM is accepted", if check_dma_buffer(&local.sram[..]).is_ok() { Ok(()) } else { Err(Fail::Count { expected: 1, got: 0 }) });

        let Some(buffers) = local.buffers.take() else {
            defmt::panic!("run A's buffers were taken")
        };
        let rcc = &mut run.rcc;

        let (usart3, buffers) = route!(report, rcc, "USART3 PC10->PC11 (DMA1 S3/S1)", run.usart3, buffers);
        let uart4 = Wiring {
            uart: run.uart4,
            tx: usart3.tx.into_alternate::<8>(&mut run.ctrl_c),
            rx: usart3.rx.into_alternate::<8>(&mut run.ctrl_c),
            tx_dma: run.s4,
            rx_dma: run.s2,
        };
        let (uart4, buffers) = route!(report, rcc, "UART4 PC10->PC11 (DMA1 S4/S2)", uart4, buffers);
        let usart3 = Wiring {
            uart: usart3.uart,
            tx: run.pd8,
            rx: run.pd9,
            tx_dma: uart4.tx_dma,
            rx_dma: usart3.rx_dma,
        };
        let (_, buffers) = route!(report, rcc, "USART3 PD8->PD9 (DMA1 S4 ch7/S1)", usart3, buffers);
        let (_, _) = route!(report, rcc, "UART5 PC12->PD2 (DMA1 S7/S0)", run.uart5, buffers);

        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }

    fn expect(expected: u32, got: u32) -> Result<(), Fail> {
        if expected == got { Ok(()) } else { Err(Fail::Count { expected, got }) }
    }
}
