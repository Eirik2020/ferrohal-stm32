//! Die 0x413's hand facts (STM32F405, F407, F415, F417): what metapac does
//! not carry, each citing its manual. Every part cut from this die mounts
//! this module beside its generated routing module (`gen/<part>.rs`).
//!
//! Requirements: GLUE-004

use super::*;

#[path = "errata_es0182.rs"]
mod errata_table;
pub use errata_table::*;

/// ES0182 2.2.1: revision A has no prefetch queue, so `freeze` leaves PRFTEN
/// clear there.
pub(crate) const PREFETCH_ERRATUM: Option<&crate::errata::Erratum> = Some(&ES0182_2_2_1);

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
/// its endpoints: facts metapac does not carry.
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = false;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 320;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 4;

/// SRAM1 (112 KiB) and SRAM2 (16 KiB), contiguous (RM0090 section 2.3).
/// Not the 64 KiB of CCM RAM at 0x1000_0000: only the core reaches it, so a
/// DMA buffer there would never see a byte (RM0090 figure 1).
const DMA_RAM: [(usize, usize); 1] = [(0x2000_0000, 128 * 1024)];

/// Whether `buffer` can be handed to a DMA stream: whether it lies in SRAM1
/// or SRAM2. A Cortex-M4 has no data cache, so that is all. `Uart::new`
/// checks every buffer with this; firmware that places statics in CCM RAM
/// can use it to check its own.
pub fn check_dma_buffer<T>(buffer: &[T]) -> Result<(), crate::dma::BufferError> {
    if macros::within(buffer, &DMA_RAM) {
        Ok(())
    } else {
        Err(crate::dma::BufferError::Unreachable)
    }
}

/// DS8626 and RM0090, at 2.7 V to 3.6 V, in voltage scale 1, which covers
/// 168 MHz on this chip (scale 2 only 144 MHz) and is VOS's reset value.
/// `freeze` writes it rather than trust that. RM0090 gives this chip one VOS
/// bit, 14: 1 is scale 1, 0 scale 2. metapac 21.0.0 shares one PWR block
/// with the F42x, whose VOS is bits 15:14, so it names this chip's scale 1
/// (01) `SCALE3`, and its `SCALE1` (11) would also set bit 15, which isn't
/// VOS here. Hence the bits, not the name.
pub(crate) const CLOCK_LIMITS: Limits = Limits {
    sysclk_max: Hertz::mhz(168),
    hclk_max: Hertz::mhz(168),
    pclk1_max: Hertz::mhz(42),
    pclk2_max: Hertz::mhz(84),
    hse_oscillator: (Hertz::mhz(4), Hertz::mhz(26)),
    hse_bypass: (Hertz::mhz(1), Hertz::mhz(50)),
    pll_input: (Hertz::khz(950), Hertz::khz(2_100)),
    pll_vco: (Hertz::mhz(100), Hertz::mhz(432)),
    flash_step_2v7: Hertz::mhz(30),
    max_wait_states: 5,
    vos: pac::pwr::vals::Vos::from_bits(0b01),
};

/// The independent watchdog's token and registers.
pub type WatchdogPeripheral = IWDG;

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

// Timers another timer's trigger output can start.

timer_trigger!(TIM8: TIM1, 0);
timer_trigger!(TIM3: TIM1, 0);
timer_trigger!(TIM2: TIM3, 2);

adc_internal!(ADC1: 16, 17);

pub(crate) const VREFIN_CAL: *const u16 = 0x1FFF_7A2A as *const u16;
pub(crate) const TS_CAL1: *const u16 = 0x1FFF_7A2C as *const u16;
pub(crate) const TS_CAL2: *const u16 = 0x1FFF_7A2E as *const u16;

/// The unique ID's block (RM0090 39.1, RM0368 25.1).
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
// alone (RM0090 figure 1).
gpio_dma!(
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

/// The die revision code, `DBGMCU_IDCODE.REV_ID` (bits 31:16) on RM0090, as
/// the errata sheet numbers it ([`revision_code`]).
pub(crate) fn revision() -> u16 {
    revision_code(pac::DBGMCU.idcode().read().rev_id())
}

/// What revision A reads in `REV_ID`: the F2's code, with the F2's DEV_ID
/// (ES0182 2.2.2).
const REVISION_A_READS: u16 = 0x2000;
/// Revision A's code in ES0182's revision table.
const REVISION_A: u16 = 0x1000;

/// The sheet's code for a raw `REV_ID`. ES0182 2.2.2: revision A reads 0x2000,
/// which `Erratum::affects` would take for a revision newer than the sheet's
/// newest, and so for one free of revision A's errata (2.2.1's missing
/// prefetch queue among them). No later revision of this die reads 0x2000.
pub(crate) const fn revision_code(raw: u16) -> u16 {
    if raw == REVISION_A_READS {
        REVISION_A
    } else {
        raw
    }
}

#[cfg(test)]
mod revision_tests {
    use super::*;

    #[test]
    fn revision_a_reads_as_revision_a() {
        assert_eq!(revision_code(0x2000), 0x1000);
        assert!(ES0182_2_2_1.affects(revision_code(0x2000)));
    }

    #[test]
    fn every_other_code_is_unchanged() {
        for (code, _) in ERRATA_SHEET.revisions {
            assert_eq!(revision_code(*code), *code);
        }
        assert!(!ES0182_2_2_1.affects(revision_code(0x101F)));
    }
}
