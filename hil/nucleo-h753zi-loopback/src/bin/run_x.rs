//! Run X: the cache and MPU matrix. The ES0392 hazards the rig's shapes
//! could reach, exercised under each memory attribute with both caches on
//! throughout.
//!
//! - The driver's verdicts per shape: a buffer in the write-through region
//!   and in the write-back region is refused (`BufferError::Cacheable`), a buffer
//!   in the non-cacheable DMA block is taken, and with the MPU off a
//!   buffer in plain cached RAM is refused again — the safe-by-default
//!   answer to "MPU off with caches on".
//! - ES0392 2.1.1 (store, linefill to the same set, store, load, in
//!   write-through memory): the loop holds for N rounds under a
//!   write-through region — the load reads the last store — and the same
//!   sequence under write-back runs clean as the control.
//! - ES0392 2.1.5 (clean-invalidate, store, linefill, no barrier): with
//!   the DMB between the maintenance and the store the load always reads
//!   the store; without it the cache-side stale count goes to the log,
//!   because whether that race fires is the silicon's timing. The store
//!   itself reaches SRAM either way, which the check pins after a
//!   clean-invalidate of the line.
//! - With the MPU off the default map governs and SRAM is write-back, so
//!   neither write-through hazard can fire; the control loop runs there.
//!
//! The maintenance op is DCCIMVAC, one of 2.1.5's four triggers; on the
//! clean lines of a write-through region its clean half does nothing.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

use core::ptr::{read_volatile, write_volatile};

use tests::Fail;

/// A write-through region: TEX 000, C 1 (bit 17), B 0.
const WRITE_THROUGH: u32 = 1 << 17;
/// A write-back, write-allocate region: TEX 000, C 1, B 1 (bit 16).
const WRITE_BACK: u32 = (1 << 17) | (1 << 16);
/// 2^(14 + 1) bytes.
const SIZE_32K: u32 = 14 << 1;
const FULL_ACCESS: u32 = 0b011 << 24;
const EXECUTE_NEVER: u32 = 1 << 28;
const ENABLE: u32 = 1;
/// PRIVDEFENA: everything outside the regions keeps the default map.
const DEFAULT_MAP: u32 = 1 << 2;

/// The write-through test block, in free AXI SRAM below the linker's RAM
/// end and far from both .bss and the stack.
const WT_BASE: u32 = 0x2406_0000;
/// The write-back control block.
const WB_BASE: u32 = 0x2406_8000;
/// The region length: eight cache sets' stride, so a same-set linefill
/// sweep stays inside the region.
const REGION_BYTES: u32 = 32 * 1024;
/// The Cortex-M7's data cache line.
const LINE: u32 = 32;
/// Addresses one set apart: the set index is bits 5..12, so a stride of
/// 32 * 128 lands on the same set.
const SET_SPAN: u32 = LINE * 128;

/// (Re)programs the three regions with the MPU off while they are written:
/// region 0 the non-cacheable DMA block, region 1 write-through, region 2
/// write-back. SAFETY: the blocks are untouched RAM and the caches hold no
/// line of them while the attributes change.
unsafe fn set_regions(core: &mut cortex_m::Peripherals) {
    unsafe {
        core.MPU.ctrl.write(0);
        board::dma_region(core);
        core.MPU.rnr.write(1);
        core.MPU.rbar.write(WT_BASE);
        core.MPU
            .rasr
            .write(EXECUTE_NEVER | FULL_ACCESS | WRITE_THROUGH | SIZE_32K | ENABLE);
        core.MPU.rnr.write(2);
        core.MPU.rbar.write(WB_BASE);
        core.MPU
            .rasr
            .write(EXECUTE_NEVER | FULL_ACCESS | WRITE_BACK | SIZE_32K | ENABLE);
        core.MPU.ctrl.write(DEFAULT_MAP | ENABLE);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

/// Turns the MPU off: the default map governs, and its SRAM is write-back.
unsafe fn mpu_off(mpu: &mut cortex_m::peripheral::MPU) {
    unsafe {
        mpu.ctrl.write(0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

/// ES0392 2.1.1's sequence, at `at` in a write-through region: read the
/// line in, store, linefill to another line of the same set, store again,
/// load. True when the load reads the second store.
fn sequence_2_1_1(at: u32) -> bool {
    let p = at as *mut u32;
    unsafe {
        let _first = read_volatile(p);
        write_volatile(p, 0xA5A5_0001);
        // A linefill to a different line that allocates to the same set.
        let _fill = read_volatile((at + SET_SPAN) as *const u32);
        write_volatile(p, 0xA5A5_0002);
        let got = read_volatile(p);
        got == 0xA5A5_0002
    }
}
/// ES0392 2.1.5's sequence, at `at` in a write-through region:
/// clean-invalidate the line, then — with or without the DMB — a store,
/// then a linefill to the same line, then the load. Returns (the load read
/// the store, the store reached SRAM).
unsafe fn sequence_2_1_5(
    scb: &mut cortex_m::peripheral::SCB,
    at: u32,
    barrier: bool,
) -> (bool, bool) {
    unsafe {
        let p = at as *mut u32;
        let _ = read_volatile(p);
        scb.clean_invalidate_dcache_by_address(at as usize, LINE as usize);
        if barrier {
            cortex_m::asm::dmb();
        }
        write_volatile(p, 0x5A5A_0042);
        // Four linefills to the same set evict the line, then the load's
        // miss is the linefill the erratum names.
        for k in 1..=4u32 {
            let _ = read_volatile((at + SET_SPAN * k) as *const u32);
        }
        let load = read_volatile(p);
        // The store's own path to SRAM: clean-invalidate the line and read
        // it back from memory.
        scb.clean_invalidate_dcache_by_address(at as usize, LINE as usize);
        let sram = read_volatile(p);
        (load == 0x5A5A_0042, sram == 0x5A5A_0042)
    }
}

/// The 2.1.1 loop: `rounds` addresses spread over the region, fresh loads
/// only.
fn run_2_1_1(base: u32, rounds: u32) -> Result<(), Fail> {
    for i in 0..rounds {
        // Stay one set-span inside the region for the same-set fill.
        let at = base + (i % (REGION_BYTES / SET_SPAN - 1)) * SET_SPAN;
        if !sequence_2_1_1(at) {
            return Err(Fail::Count {
                expected: i,
                got: i + 1,
            });
        }
    }
    Ok(())
}

/// The 2.1.5 loop with the DMB: every load fresh.
fn run_2_1_5_barriered(
    scb: &mut cortex_m::peripheral::SCB,
    base: u32,
    rounds: u32,
) -> Result<(), Fail> {
    for i in 0..rounds {
        let at = base + (i % (REGION_BYTES / SET_SPAN - 4)) * SET_SPAN;
        let (fresh, in_sram) = unsafe { sequence_2_1_5(scb, at, true) };
        if !fresh || !in_sram {
            return Err(Fail::Count {
                expected: i,
                got: i + 1,
            });
        }
    }
    Ok(())
}

/// The 2.1.5 loop without the DMB. The cache-side stale count is the
/// silicon's timing, not policy, and goes to the log; the store reaching
/// SRAM is the invariant the erratum itself grants.
fn run_2_1_5_free(
    scb: &mut cortex_m::peripheral::SCB,
    base: u32,
    rounds: u32,
) -> Result<u32, Fail> {
    let mut stale = 0;
    for i in 0..rounds {
        let at = base + (i % (REGION_BYTES / SET_SPAN - 4)) * SET_SPAN;
        let (fresh, in_sram) = unsafe { sequence_2_1_5(scb, at, false) };
        if !fresh {
            stale += 1;
        }
        if !in_sram {
            return Err(Fail::Count {
                expected: i,
                got: i + 1,
            });
        }
    }
    Ok(stale)
}

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use cortex_m::peripheral::SCB;
    use ferrohal_stm32::chip::check_dma_buffer;
    use ferrohal_stm32::dma::BufferError;

    use crate::{
        board,
        tests::{self, Fail, Report},
        {WB_BASE, WT_BASE, mpu_off, run_2_1_1, run_2_1_5_barriered, run_2_1_5_free, set_regions},
    };

    const ROUNDS: u32 = 500;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        scb: SCB,
        cpuid: cortex_m::peripheral::CPUID,
        mpu: cortex_m::peripheral::MPU,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        unsafe { set_regions(&mut core) };
        let mut scb = core.SCB;
        let mut cpuid = core.CPUID;
        let mpu = core.MPU;
        board::caches_on(&mut scb, &mut cpuid);
        defmt::info!(
            "run X, clocks: {}, I-cache {=bool}, D-cache {=bool}",
            board::CLOCKS,
            SCB::icache_enabled(),
            SCB::dcache_enabled()
        );
        checks::spawn().ok();
        (Shared {}, Local { scb, cpuid, mpu })
    }

    #[task(priority = 1, local = [scb, cpuid, mpu])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();

        // The driver's verdicts under each attribute.
        report.check_for(
            "cache matrix",
            "GLUE-003",
            "buffer in the write-through region refused",
            tests::expect_refusal(
                check_dma_buffer(unsafe { core::slice::from_raw_parts(WT_BASE as *const u8, 32) }),
                BufferError::Cacheable,
            ),
        );
        report.check_for(
            "cache matrix",
            "GLUE-003",
            "buffer in the write-back region refused",
            tests::expect_refusal(
                check_dma_buffer(unsafe { core::slice::from_raw_parts(WB_BASE as *const u8, 32) }),
                BufferError::Cacheable,
            ),
        );
        report.check_for(
            "cache matrix",
            "GLUE-003",
            "buffer in the non-cacheable DMA block taken",
            check_dma_buffer(unsafe { core::slice::from_raw_parts(0x2407_C000 as *const u8, 32) })
                .map_err(Fail::from),
        );

        // ES0392 2.1.1 under write-through, and the write-back control.
        report.check_for(
            "cache matrix",
            "ES0392 2.1.1",
            "write-through: the load reads the last store, 500 rounds",
            run_2_1_1(WT_BASE, ROUNDS),
        );
        report.check_for(
            "cache matrix",
            "ES0392 2.1.1",
            "write-back control: the same sequence clean",
            run_2_1_1(WB_BASE, ROUNDS),
        );

        // ES0392 2.1.5, with the DMB and without it.
        report.check_for(
            "cache matrix",
            "ES0392 2.1.5",
            "with the DMB: the load always reads the store, 500 rounds",
            run_2_1_5_barriered(cx.local.scb, WT_BASE, ROUNDS),
        );
        match run_2_1_5_free(cx.local.scb, WT_BASE, ROUNDS) {
            Ok(stale) => {
                defmt::info!(
                    "2.1.5 without the DMB: {=u32} stale load(s) in {=u32} rounds; whether the race fires is the silicon's timing",
                    stale,
                    ROUNDS,
                );
                report.check(
                    "cache matrix",
                    "without the DMB: every store still reaches SRAM",
                    Ok(()),
                );
            }
            Err(error) => {
                report.check(
                    "cache matrix",
                    "without the DMB: every store still reaches SRAM",
                    Err(error),
                );
            }
        }

        // The MPU off: the default map governs, its SRAM is write-back, the
        // write-through hazards cannot fire, and the driver refuses.
        unsafe { mpu_off(cx.local.mpu) };
        report.check_for(
            "cache matrix",
            "GLUE-003",
            "MPU off: a buffer in cached RAM refused",
            tests::expect_refusal(
                check_dma_buffer(unsafe { core::slice::from_raw_parts(WT_BASE as *const u8, 32) }),
                BufferError::Cacheable,
            ),
        );
        report.check_for(
            "cache matrix",
            "ES0392 2.1.1",
            "MPU off: the default map's write-back SRAM, the same sequence clean",
            run_2_1_1(WT_BASE, 64),
        );
        // SAFETY: the tokens were taken in init and this run touches no
        // peripheral but the core's, as `reset.rs`'s steal does.
        unsafe { set_regions(&mut cortex_m::Peripherals::steal()) };

        tests::finish(&report, || {});
    }
}
