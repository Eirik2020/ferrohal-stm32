//! Run K: the independent watchdog (IWDG1), across the reset it causes. No
//! links.
//!
//! The first boot feeds a 50 ms watchdog every 10 ms for 300 ms, then
//! stops; the watchdog must reset the chip. The second boot finds the
//! watchdog as the reset cause and reports. The H7's LSI runs at 29.4 to
//! 33.6 kHz (DS12110), so the reset may come 47 to 55 ms after the last
//! feed.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;
#[path = "../../../common/watchdog.rs"]
mod watchdog;

use core::{mem::MaybeUninit, ptr::addr_of_mut};

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{chip::Peripherals, iwdg::Watchdog};
use panic_probe as _;

use tests::{Fail, Report};

/// The window phase's marker values: arming, then started and fed.
const WINDOW_STARTING: u32 = 1;
const WINDOW_STARTED: u32 = 2;
/// The windowed run: 100 ms timeout, the last 30 ms feedable.
const WINDOW_US: u32 = 100_000;
const OPEN_US: u32 = 30_000;

#[unsafe(link_section = ".uninit.window")]
static mut WINDOW: MaybeUninit<u32> = MaybeUninit::uninit();

fn marker() -> *mut u32 {
    // SAFETY: the only access is through these volatile reads and writes,
    // from one context.
    addr_of_mut!(WINDOW).cast::<u32>()
}

fn read_marker() -> u32 {
    // SAFETY: inside WINDOW; volatile, as a reset may land between writes.
    unsafe { marker().read_volatile() }
}

fn write_marker(value: u32) {
    // SAFETY: as above.
    unsafe { marker().write_volatile(value) }
}

fn spin_ms(ms: u32) {
    cortex_m::asm::delay(board::CLOCKS.cpu.0 / 1_000 * ms);
}

#[entry]
fn main() -> ! {
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_k(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    let cause = run.rcc.reset_cause();
    defmt::info!("run K, reset cause: {}", cause);
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let mut report = Report::new();
    // The token passes to whichever phase runs: the starvation run on a
    // fresh boot, the windowed one after its report.
    let mut iwdg = Some(run.iwdg);
    let name = "IWDG1 50 ms";
    if !watchdog::after_reset(&mut report, name, cause, (29, 34), (47_500, 52_500)) {
        let frozen = Watchdog::is_frozen_on_halt();
        let token = iwdg.take().unwrap_or_else(|| defmt::unreachable!());
        match Watchdog::start_with_lsi(token, watchdog::TIMEOUT_US, board::LSI_HZ) {
            Ok(dog) => watchdog::feed_then_starve(
                &mut report,
                name,
                cause,
                dog,
                board::CLOCKS.cpu,
                frozen,
            ),
            Err(refused) => report.check(
                name,
                "start",
                Err(Fail::Count {
                    expected: 1,
                    got: refused.error() as u32,
                }),
            ),
        }
    }

    // The window phase: boot 2 arms it and feeds early; the reset that
    // follows is boot 3, which reports it.
    let window = "IWDG1 100 ms, window 30 ms";
    match read_marker() {
        0 => {
            write_marker(WINDOW_STARTING);
            let token = iwdg.take().unwrap_or_else(|| defmt::unreachable!());
            match Watchdog::start_windowed(token, WINDOW_US, OPEN_US) {
                Ok(dog) => {
                    // Started and about to feed: a reset now is the feed's,
                    // not the start's.
                    write_marker(WINDOW_STARTED);
                    // Early: the counter is at its reload, above the window.
                    dog.feed();
                    // Still running: the window let an early feed through.
                    // Keep feeding so the failure is reported, not a later
                    // timeout.
                    for _ in 0..15 {
                        spin_ms(10);
                        dog.feed();
                    }
                    report.check(
                        window,
                        "a feed before the window opens resets the chip [IWDG-005]",
                        Err(Fail::Count {
                            expected: 1,
                            got: 0,
                        }),
                    );
                    write_marker(0);
                }
                Err(refused) => {
                    report.check(
                        window,
                        "start_windowed",
                        Err(Fail::Count {
                            expected: 1,
                            got: refused.error() as u32,
                        }),
                    );
                    write_marker(0);
                }
            }
        }
        WINDOW_STARTING => {
            report.check(
                window,
                "start_windowed returned before the feed [IWDG-005]",
                Err(Fail::Level {
                    expected_high: true,
                }),
            );
            write_marker(0);
        }
        _ => {
            report.check(
                window,
                "a feed before the window opens resets the chip [IWDG-005]",
                if cause.independent_watchdog {
                    Ok(())
                } else {
                    Err(Fail::Level {
                        expected_high: true,
                    })
                },
            );
            write_marker(0);
        }
    }

    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
