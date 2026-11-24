//! Candidate B, an RTIC bench that uses links: run A's route checks on D1
//! (USART3, PC10 -> PC11) and D2 (UART5, PC12 -> PD2), polled as in run A,
//! each check a stage the host can pick. A route whose checks are all left
//! out is not opened; its buffers pass to the next.

#![no_std]
#![no_main]

#[path = "../../../stm32f4-discovery/src/board.rs"]
mod board;
#[path = "../harness.rs"]
mod harness;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use super::harness::{self, Selection, check, stage};
    use crate::{
        board,
        tests::{self, Loop},
    };
    use ferrohal_stm32::uart::{Buffers, Config, Uart};

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 256;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunA>,
        buffers: Option<Buffers>,
        select: Selection,
    }

    #[init(local = [
        tx_store: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx_store: [u8; RX_RING] = [0; RX_RING],
    ])]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let select = Selection::read();
        let run = stage("clocks", 2, || {
            #[cfg(feature = "hang-clocks")]
            harness::hang();
            match board::run_a(cx.device) {
                Ok(run) => run,
                Err(error) => defmt::panic!("clocks: {}", error),
            }
        });
        checks::spawn().ok();
        let buffers = Buffers { tx: cx.local.tx_store, rx: cx.local.rx_store };
        (Shared {}, Local { run: Some(run), buffers: Some(buffers), select })
    }

    /// Opens a route, runs its picked checks, and frees it.
    macro_rules! route {
        ($select:expr, $rcc:expr, $echo:expr, $stream:expr, $hang:expr, $wiring:expr, $buffers:expr) => {{
            let (echo, stream): (&'static str, &'static str) = ($echo, $stream);
            if $select.wants(echo) || $select.wants(stream) {
                let config = Config::new(ferrohal_stm32::time::Hertz(115_200));
                let mut uart = match stage("open", 1, || Uart::new($rcc, $wiring, &config, $buffers)) {
                    Ok(opened) => opened,
                    Err(_) => defmt::panic!("{=str}: refused", echo),
                };
                let mut link = Loop { name: echo, uart: &mut uart };
                tests::drain(&mut link);
                check($select, echo, 1, || {
                    if $hang {
                        harness::hang();
                    }
                    tests::echo(&mut link, b"die 0x413 route\n")
                });
                check($select, stream, 2, || tests::stream(&mut link, 4 * 1024));
                match uart.free() {
                    Ok((_, buffers)) => buffers,
                    Err(_) => defmt::panic!("{=str}: free refused", echo),
                }
            } else {
                $buffers
            }
        }};
    }

    #[task(priority = 1, local = [run, buffers, select])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let select = cx.local.select;
        let Some(mut run) = cx.local.run.take() else { defmt::panic!("parts taken") };
        let Some(buffers) = cx.local.buffers.take() else { defmt::panic!("buffers taken") };
        let rcc = &mut run.rcc;
        let buffers = route!(select, rcc, "UART-001.usart3-d1-echo",
            "UART-002.usart3-d1-stream", false, run.usart3, buffers);
        let _ = route!(select, rcc, "UART-001.uart5-d2-echo", "UART-002.uart5-d2-stream",
            cfg!(feature = "hang-check"), run.uart5, buffers);
        harness::finish(board::CLOCKS.cpu.0)
    }
}
