//! Die 0x419's hand facts (STM32F427, F429, F437, F439): what metapac does
//! not carry, each citing its manual. Every part cut from this die mounts
//! this module beside its generated routing module (`gen/<part>.rs`).
//!
//! Requirements: GLUE-004

use super::*;

#[path = "errata_es0206.rs"]
mod errata_table;
pub use errata_table::*;

/// The prefetch erratum `freeze` checks: none on this die.
pub(crate) const PREFETCH_ERRATUM: Option<&crate::errata::Erratum> = None;

/// Which ADC carries the temperature sensor and VREFINT channels.
macro_rules! internal_adc {
    (ADC1) => {
        true
    };
    ($other:ident) => {
        false
    };
}

/// The full-speed USB port's core (full-speed only), its FIFO RAM in words and
/// its endpoints: facts metapac does not carry. 1.25 KiB of FIFO RAM (RM0090
/// 34.2), four bidirectional endpoints as on die 0x413.
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = false;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 320;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 4;

/// SRAM1 (112 KiB), SRAM2 (16 KiB) and SRAM3 (64 KiB), contiguous from
/// 0x2000_0000 (RM0090 2.3). Not the 64 KiB of CCM RAM at 0x1000_0000: only
/// the core reaches it, so a DMA buffer there would never see a byte (RM0090
/// Figure 2).
const DMA_RAM: [(usize, usize); 1] = [(0x2000_0000, 192 * 1024)];

/// Whether `buffer` can be handed to a DMA channel: whether it lies in SRAM1,
/// SRAM2 or SRAM3. A Cortex-M4 has no data cache, so that is all.
/// `Uart::new` checks every buffer with this; firmware that places statics
/// in CCM RAM can use it to check its own.
pub fn check_dma_buffer<T>(buffer: &[T]) -> Result<(), crate::dma::BufferError> {
    if macros::within(buffer, &DMA_RAM) {
        Ok(())
    } else {
        Err(crate::dma::BufferError::Unreachable)
    }
}

/// DS9405 and RM0090, at 2.7 V to 3.6 V, in voltage scale 1 (VOS 11), which
/// 168 MHz and over needs (DS9405 Table 17). HCLK up to 180 MHz with the
/// over-drive on and 168 MHz without it; APB1 up to 45 and APB2 to 90 MHz
/// with it, 42 and 84 without (Table 17); the HSE crystal 4 to 26 MHz
/// (Table 39), an external clock 1 to 50 MHz (Table 37); the PLL input 0.95
/// to 2.1 MHz and its VCO 100 to 432 MHz (Table 43); one wait state per
/// 30 MHz up to 5 at 180 MHz (RM0090 Table 12). Revisions A and Y have no
/// over-drive (ES0206 2.2.12), so `freeze` refuses it there.
pub(crate) const CLOCK_LIMITS: Limits = Limits {
    sysclk_max: Hertz::mhz(180),
    hclk_max: Hertz::mhz(180),
    pclk1_max: Hertz::mhz(42),
    pclk2_max: Hertz::mhz(84),
    hse_oscillator: (Hertz::mhz(4), Hertz::mhz(26)),
    hse_bypass: (Hertz::mhz(1), Hertz::mhz(50)),
    pll_input: (Hertz::khz(950), Hertz::khz(2_100)),
    pll_vco: (Hertz::mhz(100), Hertz::mhz(432)),
    flash_step_2v7: Hertz::mhz(30),
    max_wait_states: 5,
    vos: pac::pwr::vals::Vos::SCALE1,
    overdrive: Some(Overdrive {
        above: Hertz::mhz(168),
        pclk1_max: Hertz::mhz(45),
        pclk2_max: Hertz::mhz(90),
        erratum: Some(&ES0206_2_2_12),
    }),
};

pub(crate) fn iwdg_regs() -> pac::iwdg::Iwdg {
    pac::IWDG
}

/// Freezes the watchdog while a debugger holds the core halted:
/// `DBGMCU_APB1FZ.IWDG` (bit 12; the freeze lives in the APB1 freeze
/// register, not `DBGMCU_CR`).
pub(crate) fn set_watchdog_freeze() {
    pac::DBGMCU.apb1fzr().modify(|w| w.set_iwdg(true));
}

/// Whether that freeze is set.
pub(crate) fn watchdog_frozen() -> bool {
    pac::DBGMCU.apb1fzr().read().iwdg()
}

// Timers another timer's trigger output can start (RM0090 Tables 94 and 98).

timer_trigger!(TIM8: TIM1, 0);
timer_trigger!(TIM3: TIM1, 0);
timer_trigger!(TIM2: TIM3, 2);

// The temperature sensor on ADC1_IN18, which it shares with VBAT, and
// VREFINT on IN17 (RM0090 13.10: IN16 is the F40x's).
adc_internal!(ADC1: 18, 17);

/// The factory calibration values (DS9405 Tables 82 and 85).
pub(crate) const VREFIN_CAL: *const u16 = 0x1FFF_7A2A as *const u16;
pub(crate) const TS_CAL1: *const u16 = 0x1FFF_7A2C as *const u16;
pub(crate) const TS_CAL2: *const u16 = 0x1FFF_7A2E as *const u16;

/// The unique ID's block (RM0090 39.1).
pub(crate) const UID_BASE: usize = 0x1FFF_7A10;

/// The ROM bootloader's vector table (AN2606).
pub(crate) const SYSTEM_MEMORY: u32 = 0x1FFF_0000;

/// Maps system memory at address 0, as the BOOT0 pin would for the bootloader.
pub(crate) fn map_system_memory() {
    pac::RCC.apb2enr().modify(|w| w.set_syscfgen(true));
    let _ = pac::RCC.apb2enr().read();
    pac::SYSCFG.memrm().modify(|w| w.set_mem_mode(1));
}

/// SYSCFG's clock enable, which EXTI's port selection (EXTICR) needs.
pub(crate) const SYSCFG_ENABLE: RccBit = RccBit::new(Bus::Apb2, 14);

// Only DMA2's peripheral port reaches the GPIO ports; DMA1's reaches APB1
// alone (RM0090 Figure 2).
gpio_dma!(
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

/// The die revision code, `DBGMCU_IDCODE.REV_ID` (bits 31:16) on RM0090.
pub(crate) fn revision() -> u16 {
    pac::DBGMCU.idcode().read().rev_id()
}
