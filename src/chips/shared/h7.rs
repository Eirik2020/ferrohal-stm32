//! What the RM0433 chips (STM32H742, H743, H753, H750) share beyond their
//! IP versions: clock limits and the DMA's view of memory. Each H7 chip file
//! re-exports these.

use crate::{rcc_h7rm0433::Limits, time::Hertz, uart::ConfigError};

/// Limits both silicon revisions meet. The wide VCO tops out at 836 MHz on
/// revision Y (DS12117 table 39) and 960 MHz on revision V (table 136); the
/// lower one is used. The H743 and H753 share them: the H753 is the H743
/// with the crypto block, on the same die (RM0433).
pub(crate) const CLOCK_LIMITS: Limits = Limits {
    scale1: (Hertz::mhz(400), Hertz::mhz(200), Hertz::mhz(100)),
    scale2: (Hertz::mhz(300), Hertz::mhz(150), Hertz::mhz(75)),
    scale3: (Hertz::mhz(200), Hertz::mhz(100), Hertz::mhz(50)),
    hse_oscillator: (Hertz::mhz(4), Hertz::mhz(48)),
    hse_bypass: (Hertz::mhz(4), Hertz::mhz(50)),
    pll_input: (Hertz::mhz(1), Hertz::mhz(16)),
    vco_medium: (Hertz::mhz(150), Hertz::mhz(420)),
    vco_wide: (Hertz::mhz(192), Hertz::mhz(836)),
};

/// RAM DMA1 and DMA2 can reach (RM0433 figure 1): AXI SRAM, SRAM1 to 3 in
/// D2, and SRAM4 in D3. Not the DTCM or ITCM, which only the core sees.
const DMA_RAM: [(usize, usize); 3] = [
    (0x2400_0000, 512 * 1024),
    (0x3000_0000, 288 * 1024),
    (0x3800_0000, 64 * 1024),
];

/// Whether `buffer` can be handed to a DMA1 or DMA2 stream. `Uart::new`
/// checks every buffer with this; firmware can use it to check its own.
///
/// - It must lie in RAM those controllers reach: not the DTCM or ITCM.
/// - While the data cache is on, it must lie in memory the MPU marks
///   non-cacheable, since the drivers do no cache maintenance: the CPU
///   would read stale received bytes and transmit stale written ones.
pub fn check_dma_buffer(buffer: &[u8]) -> Result<(), ConfigError> {
    if !super::macros::within(buffer, &DMA_RAM) {
        return Err(ConfigError::BufferUnreachable);
    }
    let start = buffer.as_ptr() as usize;
    let end = start.saturating_add(buffer.len());
    if cortex_m::peripheral::SCB::dcache_enabled() && !mpu_non_cacheable(start, end) {
        return Err(ConfigError::BufferCacheable);
    }
    Ok(())
}

/// Whether the MPU makes all of `start..end` non-cacheable: one enabled
/// region holds the whole range, with every subregion it touches enabled
/// and non-cacheable attributes, and no higher-numbered region (which would
/// take precedence) overlaps the range at all. Anything else counts as
/// cacheable, including the MPU being off, since the default memory map
/// caches SRAM.
///
/// The MPU belongs to the firmware. This only reads it, apart from
/// stepping RNR through the regions, which it restores.
fn mpu_non_cacheable(start: usize, end: usize) -> bool {
    // SAFETY: reads, and an RNR write that is undone before returning. The
    // firmware must not be reprogramming the MPU concurrently.
    let mpu = unsafe { &*cortex_m::peripheral::MPU::PTR };
    if mpu.ctrl.read() & 1 == 0 {
        return false;
    }
    let regions = (mpu._type.read() >> 8) & 0xff;
    let saved = mpu.rnr.read();
    let mut covered = false;
    for n in 0..regions {
        // SAFETY: see above.
        unsafe { mpu.rnr.write(n) };
        let rasr = mpu.rasr.read();
        let rbar = mpu.rbar.read();
        if rasr & 1 == 0 {
            continue;
        }
        // Region size 2^(SIZE + 1) bytes; SIZE 31 is the whole 4 GiB.
        let size_field = (rasr >> 1) & 0x1f;
        let size: u64 = 1 << (size_field + 1);
        let base = u64::from(rbar) & !(size - 1);
        let (start, end) = (start as u64, end as u64);
        if end <= base || start >= base + size {
            continue;
        }
        // Higher numbers win: this region now decides.
        let inside = start >= base && end <= base + size;
        covered = inside && subregions_enabled(rasr, base, size, start, end) && non_cacheable(rasr);
    }
    // SAFETY: see above.
    unsafe { mpu.rnr.write(saved) };
    covered
}

/// Whether every subregion of a region that `start..end` touches is
/// enabled. Regions of 256 bytes and up have eight subregions, each
/// disabled by one SRD bit; a disabled one falls through to lower regions.
fn subregions_enabled(rasr: u32, base: u64, size: u64, start: u64, end: u64) -> bool {
    if size < 256 {
        return true;
    }
    let sub = size / 8;
    (0..8).all(|i| {
        let (lo, hi) = (base + i * sub, base + (i + 1) * sub);
        let touched = start < hi && end > lo;
        !touched || rasr & (1 << (8 + i)) == 0
    })
}

/// RASR's TEX, C and B describe a memory type the data cache does not
/// cache: strongly ordered, device, or normal non-cacheable. Reserved
/// encodings count as cacheable.
fn non_cacheable(rasr: u32) -> bool {
    let tex = (rasr >> 19) & 0b111;
    let c = (rasr >> 17) & 1;
    let b = (rasr >> 16) & 1;
    match (tex, c, b) {
        // Strongly ordered, shared device, normal non-cacheable, device.
        (0b000, 0, 0) | (0b000, 0, 1) | (0b001, 0, 0) | (0b010, 0, 0) => true,
        // Cached memory types: inner policy in C and B, 00 non-cacheable.
        (0b100..=0b111, 0, 0) => true,
        _ => false,
    }
}

/// The ROM bootloader's vector table (AN2606).
pub(crate) const SYSTEM_MEMORY: u32 = 0x1FF0_9800;

/// Nothing to map: the H7 boots from an address in the option bytes, and the
/// bootloader runs from where it sits.
pub(crate) fn map_system_memory() {}
