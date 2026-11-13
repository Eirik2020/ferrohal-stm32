//! The bootloader request, across the resets it causes: it must survive a
//! software reset, with the data cache off and on, and answer yes once. The
//! jump itself runs on the flight controllers, whose bootloader the host sees
//! over USB. Results wait in `.uninit` RAM for the last boot: RTT restarts.

// Each run binary compiles this file and uses what it needs.
#![allow(dead_code)]

use core::{mem::MaybeUninit, ptr::addr_of_mut};

use ferrohal_stm32::reset::{self, ResetCause};

use crate::tests::{Fail, Report};

const MAGIC: u32 = 0x424F_4F54;

/// A doubleword each, written whole: [magic, stage], then the results.
#[repr(align(8))]
struct State(#[allow(dead_code)] MaybeUninit<[[u32; 2]; 6]>);

#[unsafe(link_section = ".uninit.bootloader_bench")]
static mut STATE: State = State(MaybeUninit::uninit());

fn state() -> *mut u32 {
    addr_of_mut!(STATE).cast::<u32>()
}

const FRESH: usize = 1;
const COLD: usize = 2;
const AGAIN: usize = 3;
const CACHED: usize = 4;

fn read(i: usize) -> u32 {
    // SAFETY: inside STATE, read as integers, volatile, from one context.
    unsafe { state().add(2 * i).read_volatile() }
}

fn write(i: usize, value: u32) {
    // SAFETY: as above.
    unsafe {
        state().add(2 * i).write_volatile(value);
        state().add(2 * i + 1).write_volatile(value);
    }
}

fn stage() -> u32 {
    // SAFETY: as above.
    unsafe {
        if state().read_volatile() == MAGIC {
            state().add(1).read_volatile()
        } else {
            0
        }
    }
}

fn set_stage(stage: u32) {
    // SAFETY: as above.
    unsafe {
        state().write_volatile(MAGIC);
        state().add(1).write_volatile(stage);
    }
}

fn expect(report: &mut Report, name: &str, what: &str, ok: bool) {
    let result = if ok {
        Ok(())
    } else {
        Err(Fail::Count {
            expected: 1,
            got: 0,
        })
    };
    report.check(name, what, result);
}

/// One step per boot. Returns only on the last boot, having reported.
/// `enable_cache` turns the data cache on, where the chip has one.
pub fn suite(report: &mut Report, name: &str, cause: ResetCause, enable_cache: impl FnOnce()) {
    // Before anything else writes RAM: this is what a real boot would do.
    let requested = reset::bootloader_requested();
    let again = reset::bootloader_requested();
    match stage() {
        0 => {
            for i in FRESH..=CACHED {
                write(i, 0);
            }
            write(FRESH, u32::from(!requested));
            set_stage(1);
            reset::reboot_to_bootloader()
        }
        1 => {
            write(COLD, u32::from(requested));
            write(AGAIN, u32::from(!again));
            set_stage(2);
            enable_cache();
            reset::reboot_to_bootloader()
        }
        _ => {
            write(CACHED, u32::from(requested));
            set_stage(0);
        }
    }
    // The reset restarted RTT; give the host time to find it again.
    cortex_m::asm::delay(200_000_000);
    expect(
        report,
        name,
        "a fresh boot finds no request [RESET-003]",
        read(FRESH) == 1,
    );
    expect(
        report,
        name,
        "the request survives the reset [RESET-002]",
        read(COLD) == 1,
    );
    expect(
        report,
        name,
        "it answers yes only once [RESET-003]",
        read(AGAIN) == 1,
    );
    expect(
        report,
        name,
        "it survives with the data cache on [RESET-002]",
        read(CACHED) == 1,
    );
    expect(
        report,
        name,
        "the reset cause is software [RESET-001]",
        cause.software,
    );
}
