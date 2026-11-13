//! Run K: the independent watchdog (IWDG1), across the resets it causes. No
//! links.
//!
//! Boot 1 feeds a 50 ms watchdog every 10 ms for 300 ms, then stops; the
//! watchdog must reset the chip. Boot 2 finds the watchdog as the reset
//! cause and reports. The H7's LSI runs at 29.4 to 33.6 kHz (DS12110), so
//! the reset may come 47 to 55 ms after the last feed.
//!
//! Boot 2 then starts a 100 ms watchdog whose last 30 ms are its window,
//! and feeds it five times 85 ms after each reload, inside the window: the
//! chip must keep running [IWDG-007]. It then feeds at once, with the
//! counter just reloaded and above the window, and that feed must reset
//! the chip [IWDG-005]. Boot 3 reports it. At the board's 31.7 kHz the
//! window opens about 71 ms after a reload and closes at about 101 ms.
//!
//! The window phase's progress survives the resets in one doubleword of
//! `.uninit` RAM, tagged with a magic value: the H7 keeps only whole
//! doublewords across a reset (RM0433 2.4), and an untagged word holding
//! whatever RAM held would read as a phase. Boot 2's failures ride along
//! in the same doubleword, and boot 1's in the shared watchdog marker
//! (boot 2 folds them in), so the exit status counts every boot.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;
#[path = "../../../common/watchdog.rs"]
mod watchdog;

use core::{mem::MaybeUninit, ptr::addr_of_mut};

use cortex_m::peripheral::DWT;
use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{chip::Peripherals, iwdg::Watchdog, reset::ResetCause};
use panic_probe as _;

use tests::{Fail, Report};

/// The windowed run: 100 ms timeout, the last 30 ms feedable.
const WINDOW_US: u32 = 100_000;
const OPEN_US: u32 = 30_000;
/// Feeds inside the window, and how long after each reload they come.
const IN_WINDOW_FEEDS: u32 = 5;
const IN_WINDOW_MS: u32 = 85;
/// How long after the last feed the early one comes. A key written while
/// the previous one is still crossing into the LSI domain counts as the
/// same reload (measured: an immediate second feed did not reset), so the
/// early feed waits until the counter has surely reloaded and is still far
/// above the window.
const EARLY_AFTER_MS: u32 = 10;
/// Further feeds inside the window, so the host reads the report before the
/// early feed's reset.
const DRAIN_FEEDS: u32 = 3;

/// The window phase's progress: about to start, feeding inside the window,
/// feeding early, or failed and waiting for the watchdog to end the run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Starting,
    InWindow,
    Early,
    Failed,
}

impl Phase {
    const fn code(self) -> u64 {
        match self {
            Phase::Starting => 1,
            Phase::InWindow => 2,
            Phase::Early => 3,
            Phase::Failed => 4,
        }
    }
}

/// `[magic: 32 | failures carried: 24 | phase: 8]`.
const MAGIC: u64 = 0x5749_4E44;

#[repr(align(8))]
struct Slot(#[allow(dead_code)] MaybeUninit<u64>);

#[unsafe(link_section = ".uninit.window")]
static mut WINDOW: Slot = Slot(MaybeUninit::uninit());

fn marker() -> *mut u64 {
    // SAFETY: the only access is through these volatile reads and writes,
    // from one context.
    addr_of_mut!(WINDOW).cast::<u64>()
}

/// The phase a previous boot left, and the failures it carried; `None`
/// unless the doubleword holds the magic.
fn read_marker() -> Option<(Phase, u32)> {
    // SAFETY: inside WINDOW; volatile, as a reset may land between writes.
    let value = unsafe { marker().read_volatile() };
    if value >> 32 != MAGIC {
        return None;
    }
    let failures = ((value >> 8) & 0xFF_FFFF) as u32;
    let phase = match value & 0xFF {
        1 => Phase::Starting,
        2 => Phase::InWindow,
        3 => Phase::Early,
        4 => Phase::Failed,
        _ => return None,
    };
    Some((phase, failures))
}

fn write_marker(phase: Phase, failures: u32) {
    let value = (MAGIC << 32) | (u64::from(failures & 0xFF_FFFF) << 8) | phase.code();
    // SAFETY: as above; one doubleword, written whole.
    unsafe { marker().write_volatile(value) }
}

fn clear_marker() {
    // SAFETY: as above.
    unsafe { marker().write_volatile(0) }
}

/// Waits on the cycle counter: `asm::delay` can run short on the M7, which
/// would move a feed out of the window.
fn wait_ms(ms: u32) {
    let start = DWT::cycle_count();
    let cycles = board::CLOCKS.cpu.0 / 1_000 * ms;
    while DWT::cycle_count().wrapping_sub(start) < cycles {}
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
    let window = "IWDG1 100 ms, window 30 ms";

    // Boot 3: the window phase's reset, reported first, so that this boot
    // never runs the starvation again. Only the watchdog ends that phase;
    // a marker found after any other reset is an aborted run's, and boot 1
    // starts afresh.
    let left = read_marker();
    clear_marker();
    if let (Some((phase, carried)), true) = (left, cause.independent_watchdog) {
        // The reset restarted RTT; give the host time to find it again.
        wait_ms(200);
        report_window(&mut report, window, phase, cause);
        report.check(
            window,
            "the window phase's earlier boot passed its checks",
            if carried == 0 {
                Ok(())
            } else {
                Err(Fail::Count {
                    expected: 0,
                    got: carried,
                })
            },
        );
        let led = &mut run.led;
        tests::finish(&report, || led.set_high());
    }

    let name = "IWDG1 50 ms";
    if !watchdog::after_reset(&mut report, name, cause, (29, 34), (47_500, 52_500)) {
        // Boot 1: the starvation run.
        let frozen = Watchdog::is_frozen_on_halt();
        match Watchdog::start_with_lsi(run.iwdg, watchdog::TIMEOUT_US, board::LSI_HZ) {
            Ok(dog) => {
                watchdog::feed_then_starve(&mut report, name, cause, dog, board::CLOCKS.cpu, frozen)
            }
            Err(refused) => report.check(
                name,
                "start",
                Err(Fail::Count {
                    expected: 1,
                    got: refused.error() as u32,
                }),
            ),
        }
    } else {
        // Boot 2: the window phase, which ends in the reset boot 3 reports.
        window_phase(&mut report, window, run.iwdg);
    }

    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}

/// Starts the windowed watchdog, feeds it inside the window, then early.
/// Never returns: the early feed resets the chip, or the failure does.
fn window_phase(
    report: &mut Report,
    window: &str,
    iwdg: ferrohal_stm32::chip::WatchdogPeripheral,
) -> ! {
    write_marker(Phase::Starting, report.failed);
    let dog = match Watchdog::start_windowed(iwdg, WINDOW_US, OPEN_US) {
        Ok(dog) => dog,
        Err(refused) => {
            report.check(
                window,
                "start_windowed",
                Err(Fail::Count {
                    expected: 1,
                    got: refused.error() as u32,
                }),
            );
            fail_out(report);
        }
    };
    write_marker(Phase::InWindow, report.failed);
    for _ in 0..IN_WINDOW_FEEDS {
        wait_ms(IN_WINDOW_MS);
        dog.feed();
    }
    report.check(
        window,
        "fed 5 times 85 ms after each reload, inside the window: no reset [IWDG-007]",
        Ok(()),
    );
    // Keep feeding inside the window while the host drains RTT: the early
    // feed's reset restarts the buffer, and that line was lost without this.
    for _ in 0..DRAIN_FEEDS {
        wait_ms(IN_WINDOW_MS);
        dog.feed();
    }
    wait_ms(EARLY_AFTER_MS);
    write_marker(Phase::Early, report.failed);
    // Early: the counter reloaded 10 ms ago and is far above the window.
    dog.feed();
    // Still running: the window let an early feed through. Wait as long as
    // the reset could take to arrive before calling it a failure.
    wait_ms(5);
    report.check(
        window,
        "a feed before the window opens resets the chip [IWDG-005]",
        Err(Fail::Count {
            expected: 1,
            got: 0,
        }),
    );
    fail_out(report);
}

/// Ends a failed window phase. The watchdog may still be running, and a
/// semihosting exit would leave it to reset into a fresh run K, so the
/// failures go to boot 3 in the marker and the watchdog ends this boot.
fn fail_out(report: &Report) -> ! {
    write_marker(Phase::Failed, report.failed);
    // Longer than the 100 ms timeout at the LSI's slowest (DS12110).
    wait_ms(200);
    // Not running after all (a refused start): end here.
    clear_marker();
    tests::finish(report, || {})
}

/// Boot 3's verdict on where the window phase's reset struck.
fn report_window(report: &mut Report, window: &str, phase: Phase, cause: ResetCause) {
    match phase {
        Phase::Starting => report.check(
            window,
            "start_windowed returned without a reset [IWDG-007]",
            Err(Fail::Level {
                expected_high: true,
            }),
        ),
        Phase::InWindow => report.check(
            window,
            "fed inside the window: no reset [IWDG-007]",
            Err(Fail::Count {
                expected: 0,
                got: 1,
            }),
        ),
        Phase::Early => report.check(
            window,
            "a feed before the window opens resets the chip [IWDG-005]",
            if cause.independent_watchdog {
                Ok(())
            } else {
                Err(Fail::Level {
                    expected_high: true,
                })
            },
        ),
        // Boot 2 reported its failure; the carried count fails the run.
        Phase::Failed => {}
    }
}
