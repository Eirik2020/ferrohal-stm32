//! Why the chip last reset, as `Rcc::reset_cause` reads it, and restarting
//! into the ROM bootloader.
//!
//! The bootloader wants the chip as reset left it, so getting there takes a
//! reset: [`reboot_to_bootloader`] leaves a request in RAM and resets, and
//! the next boot, first thing in `main`, checks [`bootloader_requested`] and
//! calls [`enter_bootloader`]. Before the clocks are set up, no `Rcc` exists
//! yet: [`peek_cause`] reads the reset flags without one.

use core::{mem::MaybeUninit, ptr::addr_of_mut};

use cortex_m::peripheral::SCB;

const REQUESTED: u32 = 0x4446_5521;

/// `.uninit` is RAM a reset leaves alone, as long as the write reached it.
/// A cache line, so cleaning it touches nothing else.
#[repr(align(32))]
struct Request(#[allow(dead_code)] MaybeUninit<[u32; 8]>);

#[unsafe(link_section = ".uninit.ferrohal_bootloader_request")]
static mut REQUEST: Request = Request(MaybeUninit::uninit());

fn words() -> *mut u32 {
    addr_of_mut!(REQUEST).cast::<u32>()
}

/// Resets the chip into its ROM bootloader, by way of [`bootloader_requested`]
/// on the next boot. The caller has checked that nothing is flying and that
/// storage is idle.
pub fn reboot_to_bootloader() -> ! {
    let words = words();
    // SAFETY: inside REQUEST, volatile, and nothing else writes it. Stolen
    // SCB: the reset follows, so no owner sees the cache clean, which changes
    // no memory contents anyway.
    unsafe {
        // The H7's RAM has ECC per 64 bits, and a reset loses a partial write
        // to them still waiting for the rest (RM0433 2.4): the bench measured
        // a lone word lost, with or without a barrier or a read back. So the
        // request fills its doubleword, and a second one follows.
        words.write_volatile(REQUESTED);
        words.add(1).write_volatile(!REQUESTED);
        words.add(2).write_volatile(0);
        words.add(3).write_volatile(0);
        if SCB::dcache_enabled() {
            cortex_m::Peripherals::steal()
                .SCB
                .clean_dcache_by_address(words as usize, 32);
        }
    }
    cortex_m::asm::dsb();
    SCB::sys_reset()
}

/// Whether the last reset came from [`reboot_to_bootloader`]; forgets the
/// request, so it answers yes once. Call it first in `main`, with the data
/// cache still off.
pub fn bootloader_requested() -> bool {
    let words = words();
    // SAFETY: inside REQUEST, volatile. Read as integers: after a power-on
    // the words hold whatever RAM held, which two matching words rule out.
    unsafe {
        let requested = words.read_volatile() == REQUESTED
            && words.add(1).read_volatile() == !REQUESTED;
        words.write_volatile(0);
        words.add(1).write_volatile(0);
        requested
    }
}

/// The reset flags, without clearing them and without an `Rcc`: for a
/// decision made before `freeze`. `Rcc::reset_cause` still reports them later.
pub fn peek_cause() -> ResetCause {
    crate::rcc::read_reset_cause()
}

/// Jumps to the ROM bootloader, clearing the reset flags so a stale one can't
/// bring a later boot back here.
///
/// # Safety
///
/// Call it first in `main`, with the chip as reset left it: no clock, cache,
/// interrupt or peripheral set up. The jump abandons everything in RAM.
pub unsafe fn enter_bootloader() -> ! {
    crate::rcc::clear_reset_flags();
    crate::chip::map_system_memory();
    // SAFETY: as the caller promises, and SYSTEM_MEMORY holds the bootloader's
    // stack pointer and reset vector (AN2606).
    unsafe {
        (*SCB::PTR).vtor.write(crate::chip::SYSTEM_MEMORY);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        cortex_m::asm::bootload(crate::chip::SYSTEM_MEMORY as *const u32)
    }
}

/// The reset flags RCC latched, chip-neutral. More than one can be set: a
/// power-on reset also sets the pin flag, for one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ResetCause {
    pub power_on: bool,
    pub pin: bool,
    pub brownout: bool,
    pub software: bool,
    pub independent_watchdog: bool,
    pub window_watchdog: bool,
    pub low_power: bool,
}
