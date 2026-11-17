//! What the Cortex-M7 chips (the F7 and the H7) share: whether the MPU
//! makes a DMA buffer non-cacheable, for their `check_dma_buffer`. A chip
//! file mounts it as `m7`.
//!
//! Requirements: GLUE-003

/// Whether the MPU makes all of `start..end` non-cacheable: one enabled
/// region holds the whole range, with every subregion it touches enabled
/// and non-cacheable attributes, and no higher-numbered region (which would
/// take precedence) overlaps the range at all. Anything else counts as
/// cacheable, including the MPU being off, since the default memory map
/// caches SRAM.
///
/// The MPU belongs to the firmware. This only reads it, apart from
/// stepping RNR through the regions, which it restores.
pub(crate) fn mpu_non_cacheable(start: usize, end: usize) -> bool {
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
