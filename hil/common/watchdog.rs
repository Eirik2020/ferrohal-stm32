//! The watchdog check, shared by the bench firmwares: it spans the reset it
//! causes. State that must survive sits in `.uninit` RAM, which a system
//! reset leaves alone - and every write of it is one whole doubleword,
//! because the H7's SRAM ECC commits a 64-bit word only when it is
//! complete, and a reset drops the last incomplete one (RM0433 2.4).
//!
//! First boot: start a 50 ms watchdog and feed it every 10 ms for 300 ms
//! (no reset may come), then stop feeding and keep writing the time since
//! the last feed. Second boot: the reset cause must be the independent
//! watchdog, and the last time written must fit the LSI's spread and, at
//! the board's measured LSI, 5% of the timeout. Starting must also have
//! frozen the counter for the debugger [IWDG-004].

// Each run binary compiles this file and uses what it needs.
#![allow(dead_code)]

use core::{mem::MaybeUninit, ptr::addr_of_mut};

use cortex_m::peripheral::DWT;
use ferrohal_stm32::{iwdg::Watchdog, reset::ResetCause, time::Hertz};

use crate::tests::{Fail, Report};

pub const TIMEOUT_US: u32 = 50_000;
const MAGIC: u32 = 0x5741_5443;

/// Three doublewords: `[magic: 32 | fed for 300 ms: 32]`,
/// `[microseconds starved]` and `[the starving boot's failure count]`,
/// each written whole.
#[unsafe(link_section = ".uninit.watchdog")]
static mut STATE: MaybeUninit<[u64; 3]> = MaybeUninit::uninit();

fn state() -> *mut u64 {
    // SAFETY: the only access is through these volatile reads and writes,
    // from one context.
    addr_of_mut!(STATE).cast::<u64>()
}

fn read(i: usize) -> u64 {
    // SAFETY: inside STATE; volatile, as a reset may land between writes.
    unsafe { state().add(i).read_volatile() }
}

fn write(i: usize, value: u64) {
    // SAFETY: as above.
    unsafe { state().add(i).write_volatile(value) }
}

fn spin_us(cpu: Hertz, us: u32) {
    cortex_m::asm::delay(cpu.0 / 1_000_000 * us);
}

/// Whether this boot follows the starving: then report on it and return
/// true. Clears the marker either way.
pub fn after_reset(
    report: &mut Report,
    name: &str,
    cause: ResetCause,
    lsi_spread: (u32, u32),
    tight_us: (u32, u32),
) -> bool {
    let claim = read(0);
    let armed = claim >> 32 == u64::from(MAGIC);
    let fed = claim & 0xFFFF_FFFF == 1;
    write(0, 0);
    if !armed {
        return false;
    }
    // The reset restarted RTT; give the host time to find it again.
    cortex_m::asm::delay(200_000_000);
    // The starving boot staked its failures on the third doubleword before
    // its reset, so the exit status counts its checks too.
    report.check(
        name,
        "the starvation boot passed its checks",
        if read(2) as u32 == 0 {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 0,
                got: read(2) as u32,
            })
        },
    );
    report.check(
        name,
        "fed every 10 ms for 300 ms: no reset [IWDG-001]",
        if fed {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 1,
                got: 0,
            })
        },
    );
    report.check(
        name,
        "starved: the reset cause is the watchdog [RESET-001]",
        if cause.independent_watchdog {
            Ok(())
        } else {
            Err(Fail::Level {
                expected_high: true,
            })
        },
    );
    // The timeout at the LSI's slowest and fastest: nominal * 32 / kHz.
    let starved = read(1) as u32;
    let (slowest_khz, fastest_khz) = lsi_spread;
    let (min, max) = (
        TIMEOUT_US * 32 / fastest_khz,
        TIMEOUT_US * 32 / slowest_khz + 2_000,
    );
    report.check(
        name,
        "starved: reset within the LSI's spread of 50 ms [IWDG-002]",
        if (min..=max).contains(&starved) {
            Ok(())
        } else {
            Err(Fail::Time {
                cycles: starved,
                min,
                max,
            })
        },
    );
    report.check(
        name,
        "starved: at the board's LSI, within 5% of 50 ms [IWDG-006]",
        if (tight_us.0..=tight_us.1).contains(&starved) {
            Ok(())
        } else {
            Err(Fail::Time {
                cycles: starved,
                min: tight_us.0,
                max: tight_us.1,
            })
        },
    );
    defmt::info!("{=str}: reset {=u32} us after the last feed", name, starved);
    true
}

/// The first boot: feed, then starve. Returns only if no reset came.
pub fn feed_then_starve(
    report: &mut Report,
    name: &str,
    cause: ResetCause,
    dog: Watchdog,
    cpu: Hertz,
    frozen_before_start: bool,
) {
    report.check(
        name,
        "a fresh start isn't a watchdog reset [RESET-001]",
        if cause.independent_watchdog {
            Err(Fail::Level {
                expected_high: false,
            })
        } else {
            Ok(())
        },
    );
    report.check(
        name,
        "start froze the watchdog for the debugger [IWDG-004]",
        if Watchdog::is_frozen_on_halt() {
            Ok(())
        } else {
            Err(Fail::Level {
                expected_high: true,
            })
        },
    );
    defmt::info!(
        "{=str}: debug freeze before start {=bool}, after {=bool}",
        name,
        frozen_before_start,
        Watchdog::is_frozen_on_halt()
    );
    // Stake this boot's failures before the reset, so the next boot's exit
    // status counts them.
    write(2, u64::from(report.failed));
    // Claim the marker before the first feed: a reset during them reports as
    // one, instead of looking like a fresh start.
    write(0, u64::from(MAGIC) << 32);
    for _ in 0..30 {
        spin_us(cpu, 10_000);
        dog.feed();
    }
    write(0, (u64::from(MAGIC) << 32) | 1);
    write(1, 0);
    let start = DWT::cycle_count();
    let per_us = cpu.0 / 1_000_000;
    loop {
        let us = DWT::cycle_count().wrapping_sub(start) / per_us;
        write(1, u64::from(us));
        if us > 1_000_000 {
            break;
        }
    }
    write(0, 0);
    report.check(
        name,
        "starved: the watchdog resets the chip [IWDG-002]",
        Err(Fail::Count {
            expected: 1,
            got: 0,
        }),
    );
}
