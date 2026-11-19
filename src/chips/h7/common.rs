//! What the RM0433 chips (STM32H742, H743, H753, H750) share beyond their
//! IP versions: clock limits and the DMA's view of memory. Each H7 chip file
//! re-exports these.
//!
//! Requirements: GLUE-003

#[path = "errata_es0392.rs"]
mod errata_table;
pub use errata_table::*;

#[path = "../m7.rs"]
mod m7;

use crate::{errata::Erratum, rcc_h7_rm0433::Limits, time::Hertz};

/// The I2C reload erratum `i2c_v2` checks.
pub(crate) const I2C_RELOAD_ERRATUM: &Erratum = &ES0392_2_19_5;

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
///
/// On revision Y/W parts, ES0392 2.2.10 can corrupt reads of the AXI SRAM
/// block while a controller delays accepting a transfer. The fix,
/// `AXI_TARG7_FN_MOD.READ_ISS_OVERRIDE`, costs read throughput and metapac
/// doesn't carry the register, so DEBT.md holds the decision. The SRAM1 to 4
/// blocks are outside the erratum; `docs/design/h7-errata.md` has the review.
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
pub fn check_dma_buffer<T>(buffer: &[T]) -> Result<(), crate::dma::BufferError> {
    if !super::macros::within(buffer, &DMA_RAM) {
        return Err(crate::dma::BufferError::Unreachable);
    }
    let start = buffer.as_ptr() as usize;
    let end = start.saturating_add(core::mem::size_of_val(buffer));
    if cortex_m::peripheral::SCB::dcache_enabled() && !m7::mpu_non_cacheable(start, end) {
        return Err(crate::dma::BufferError::Cacheable);
    }
    Ok(())
}

/// The ROM bootloader's vector table (AN2606).
pub(crate) const SYSTEM_MEMORY: u32 = 0x1FF0_9800;

/// Nothing to map: the H7 boots from an address in the option bytes, and the
/// bootloader runs from where it sits.
pub(crate) fn map_system_memory() {}

// The ADC's factory calibration (DS12110, DS12117), at 3.3 V and 16 bits.
pub(crate) const VREFIN_CAL: *const u16 = 0x1FF1_E860 as *const u16;
pub(crate) const TS_CAL1: *const u16 = 0x1FF1_E820 as *const u16;
pub(crate) const TS_CAL2: *const u16 = 0x1FF1_E840 as *const u16;

/// The unique ID's block (RM0433 61.1).
pub(crate) const UID_BASE: usize = 0x1FF1_E800;
