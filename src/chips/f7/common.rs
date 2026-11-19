//! What the F72x, F74x and F76x dies (RM0431, RM0385, RM0410) share beyond
//! their IP versions: the clock limits, the DMA's view of memory, the
//! bootloader, the watchdog's debug freeze and the revision code. Each F7
//! chip file mounts it and supplies its own DTCM and SRAM extents.
//!
//! Requirements: GLUE-003

use stm32_metapac as pac;

use crate::{
    rcc_f7::{Bus, Limits, Overdrive, RccBit},
    time::Hertz,
};

#[path = "../m7.rs"]
mod m7;

/// DS11853 (F722), DS10916 (F745) and DS11532 (F767), at 2.7 V to 3.6 V in
/// voltage scale 1, which 216 MHz needs; `freeze` writes it (VOS 11,
/// metapac's `SCALE1`). HCLK up to 216 MHz with the over-drive and 180
/// without it; APB1 up to 54 MHz and APB2 up to 108 with it, 45 and 90
/// without (DS11853 Table 16, DS10916 Table 18, DS11532 Table 17). The HSE
/// crystal 4 to 26 MHz (Table 40, 40 and 43), an external clock 1 to 50 MHz
/// (Table 38, 38 and 41); the PLL input 0.95 to 2.1 MHz and its VCO 100 to
/// 432 MHz (Table 44, 44 and 47). One wait state per 30 MHz, up to 7 at 216
/// MHz (RM0431 and RM0385 Table 5, RM0410 Table 7).
pub(crate) const CLOCK_LIMITS: Limits = Limits {
    sysclk_max: Hertz::mhz(216),
    hclk_max: Hertz::mhz(216),
    pclk1_max: Hertz::mhz(45),
    pclk2_max: Hertz::mhz(90),
    hse_oscillator: (Hertz::mhz(4), Hertz::mhz(26)),
    hse_bypass: (Hertz::mhz(1), Hertz::mhz(50)),
    pll_input: (Hertz::khz(950), Hertz::khz(2_100)),
    pll_vco: (Hertz::mhz(100), Hertz::mhz(432)),
    flash_step_2v7: Hertz::mhz(30),
    max_wait_states: 7,
    vos: pac::pwr::vals::Vos::SCALE1,
    overdrive: Some(Overdrive {
        above: Hertz::mhz(180),
        pclk1_max: Hertz::mhz(54),
        pclk2_max: Hertz::mhz(108),
        erratum: None,
    }),
};

/// Whether `buffer` can be handed to a DMA stream. `dtcm` is the chip's
/// DTCM and `sram` its SRAM1 and SRAM2, which sit together above it;
/// `dtcm_erratum`, if any, is the die's erratum against DMA reads of the
/// DTCM.
///
/// - It must lie in RAM the streams reach: the DTCM, through the core's AHBS
///   port (RM0431, RM0385 and RM0410 2.1.6, AN4667 1.3.3), or SRAM1 and
///   SRAM2. Not the ITCM, which AHBS does not reach.
/// - The DTCM is never cached, but on revision A of die 0x451 a stream reads
///   zeros from it while the core sleeps (ES0334 2.2.4), and a driver cannot
///   know whether the firmware sleeps during a transfer: there it is
///   refused, and the buffer goes in SRAM1 or SRAM2.
/// - In SRAM1 and SRAM2, while the data cache is on, it must lie in memory
///   the MPU marks non-cacheable, since the drivers do no cache maintenance:
///   the CPU would read stale received bytes and transmit stale written ones.
pub(crate) fn check_dma_buffer<T>(
    buffer: &[T],
    dtcm: (usize, usize),
    sram: (usize, usize),
    dtcm_erratum: Option<&crate::errata::Erratum>,
) -> Result<(), crate::dma::BufferError> {
    if super::macros::within(buffer, &[dtcm]) {
        return match dtcm_erratum {
            Some(e) if e.affects(revision()) => Err(crate::dma::BufferError::Unreachable),
            _ => Ok(()),
        };
    }
    if !super::macros::within(buffer, &[sram]) {
        return Err(crate::dma::BufferError::Unreachable);
    }
    let start = buffer.as_ptr() as usize;
    let end = start.saturating_add(core::mem::size_of_val(buffer));
    if cortex_m::peripheral::SCB::dcache_enabled() && !m7::mpu_non_cacheable(start, end) {
        return Err(crate::dma::BufferError::Cacheable);
    }
    Ok(())
}

/// The ROM bootloader's vector table (AN2606 Tables 98, 100 and 104).
pub(crate) const SYSTEM_MEMORY: u32 = 0x1FF0_0000;

/// Nothing to map: the F7 boots from an address in the option bytes
/// (BOOT_ADD0 and BOOT_ADD1), and SYSCFG_MEMRMP has no remap of system
/// memory to address 0; the bootloader runs from where it sits.
pub(crate) fn map_system_memory() {}

pub(crate) fn iwdg_regs() -> pac::iwdg::Iwdg {
    pac::IWDG
}

/// Freezes the watchdog while a debugger holds the core halted:
/// `DBGMCU_APB1_FZ.DBG_IWDG_STOP` (bit 12; RM0431 34.16.5, RM0385 40.16.5,
/// RM0410 44.16.5).
pub(crate) fn set_watchdog_freeze() {
    pac::DBGMCU.apb1fzr().modify(|w| w.set_iwdg(true));
}

/// Whether that freeze is set.
pub(crate) fn watchdog_frozen() -> bool {
    pac::DBGMCU.apb1fzr().read().iwdg()
}

/// SYSCFG's clock enable, which EXTI's port selection (EXTICR) needs.
pub(crate) const SYSCFG_ENABLE: RccBit = RccBit::new(Bus::Apb2, 14);

/// The die revision code, `DBGMCU_IDCODE.REV_ID` (bits 31:16).
pub(crate) fn revision() -> u16 {
    pac::DBGMCU.idcode().read().rev_id()
}
