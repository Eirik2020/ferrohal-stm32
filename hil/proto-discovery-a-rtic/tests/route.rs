//! Candidate A with RTIC: run A's route checks on D1 (USART3, PC10 -> PC11)
//! and D2 (UART5, PC12 -> PD2) as embedded-test tests. RTIC's init sets the
//! clocks and hands the parts to a static; idle enters embedded-test, which
//! runs the one test the host asked for after this reset. PROTOTYPE.

#![no_std]
#![no_main]

#[path = "../../stm32f4-discovery/src/board.rs"]
mod board;
#[path = "../../common/tests.rs"]
mod tests;

use core::cell::RefCell;
use critical_section::Mutex;
use defmt_rtt as _;
use ferrohal_stm32::uart::Buffers;

/// What init hands the test embedded-test runs after this reset.
static PARTS: Mutex<RefCell<Option<(board::RunA, Buffers)>>> = Mutex::new(RefCell::new(None));

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic: {}", defmt::Display2Format(info));
    semihosting::process::abort()
}

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use super::{PARTS, board};
    use ferrohal_stm32::uart::Buffers;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {}

    #[init(local = [
        tx_store: [u8; 256] = [0; 256],
        rx_store: [u8; 256] = [0; 256],
    ])]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        #[cfg(feature = "hang-clocks")]
        loop {
            cortex_m::asm::nop();
        }
        #[allow(unreachable_code)]
        let run = match board::run_a(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        let buffers = Buffers { tx: cx.local.tx_store, rx: cx.local.rx_store };
        critical_section::with(|cs| PARTS.borrow(cs).replace(Some((run, buffers))));
        (Shared {}, Local {})
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        // SAFETY: called once, from idle; it never returns.
        unsafe { embedded_test::export::__embedded_test_entry() }
    }
}

#[embedded_test::tests(default_timeout = 3)]
mod suite {
    use super::{PARTS, board, tests};
    use crate::tests::{Fail, Loop};
    use ferrohal_stm32::uart::{Config, Uart};

    fn parts() -> (board::RunA, ferrohal_stm32::uart::Buffers) {
        critical_section::with(|cs| PARTS.borrow(cs).take())
            .unwrap_or_else(|| defmt::panic!("init left no parts"))
    }

    macro_rules! on_route {
        ($field:ident, $name:expr, $check:expr) => {{
            tests::set_cpu_hz(board::CLOCKS.cpu.0);
            let (mut run, buffers) = parts();
            let config = Config::new(ferrohal_stm32::time::Hertz(115_200));
            let Ok(mut uart) = Uart::new(&mut run.rcc, run.$field, &config, buffers) else {
                defmt::panic!("{=str}: refused", $name)
            };
            let mut link = Loop { name: $name, uart: &mut uart };
            tests::drain(&mut link);
            $check(&mut link)
        }};
    }

    /// UART-001
    #[test]
    fn usart3_d1_echo() -> Result<(), Fail> {
        on_route!(usart3, "USART3 D1", |l: &mut dyn tests::Link| tests::echo(l, b"die 0x413 route\n"))
    }

    /// UART-002
    #[test]
    fn usart3_d1_stream() -> Result<(), Fail> {
        on_route!(usart3, "USART3 D1", |l: &mut dyn tests::Link| tests::stream(l, 4 * 1024))
    }

    /// UART-001
    #[test]
    fn uart5_d2_echo() -> Result<(), Fail> {
        #[cfg(feature = "hang-check")]
        loop {
            cortex_m::asm::nop();
        }
        #[allow(unreachable_code)]
        on_route!(uart5, "UART5 D2", |l: &mut dyn tests::Link| tests::echo(l, b"die 0x413 route\n"))
    }

    /// UART-002
    #[test]
    fn uart5_d2_stream() -> Result<(), Fail> {
        on_route!(uart5, "UART5 D2", |l: &mut dyn tests::Link| tests::stream(l, 4 * 1024))
    }
}
