//! STM32F413 glue: the chip facts the IP modules leave out, for every package
//! of the line.
//!
//! The routing facts (RCC bits, vectors, pin alternate functions, DMA
//! request channels) are generated from stm32-metapac's metadata into
//! `gen/<part>.rs` by `tools/glue-gen`, one file per package; this file holds the facts metapac
//! does not carry, each citing its manual.
//!
//! Requirements: GLUE-001, GLUE-002

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{I2cRxDma, I2cTxDma, SclPin, SdaPin},
    rcc_f4::{Bus, Limits, RCC, RccBit},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    time::Hertz,
    uart::{Info, RxDma, RxPin, TxDma, TxPin},
    usart_v2::Unit,
};

#[path = "../macros.rs"]
#[macro_use]
mod macros;

#[path = "macros.rs"]
#[macro_use]
mod family;

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
/// its endpoints: facts metapac does not carry. 1.25 KiB and six
/// bidirectional endpoints (RM0430 Table 224).
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = false;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 320;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 6;

// The die's errata (ES0372), and the prefetch erratum `freeze` checks: none
// on this die.
#[path = "errata_es0372.rs"]
mod errata_table;
pub use errata_table::*;
pub(crate) const PREFETCH_ERRATUM: Option<&crate::errata::Erratum> = None;

// The routing rows, generated from stm32-metapac's metadata for the
// part's device and package; a flash-size variant takes its package's.
#[cfg(any(feature = "stm32f413ch", feature = "stm32f413cg"))]
#[path = "../gen/stm32f413ch.rs"]
mod generated;
#[cfg(any(feature = "stm32f413mh", feature = "stm32f413mg"))]
#[path = "../gen/stm32f413mh.rs"]
mod generated;
#[cfg(any(feature = "stm32f413rh", feature = "stm32f413rg"))]
#[path = "../gen/stm32f413rh.rs"]
mod generated;
#[cfg(any(feature = "stm32f413vh", feature = "stm32f413vg"))]
#[path = "../gen/stm32f413vh.rs"]
mod generated;
#[cfg(any(feature = "stm32f413zh", feature = "stm32f413zg"))]
#[path = "../gen/stm32f413zh.rs"]
mod generated;
pub use generated::*;

/// The chip's 320 KiB of SRAM: SRAM1's 256 KiB and SRAM2's 64 KiB, contiguous
/// from 0x2000_0000, and reached by every AHB master (RM0430 2.3).
const DMA_RAM: [(usize, usize); 1] = [(0x2000_0000, 320 * 1024)];

/// Whether `buffer` can be handed to a DMA stream: whether it lies in SRAM.
/// A Cortex-M4 has no data cache, so that is all. `Uart::new` checks every
/// buffer with this.
pub fn check_dma_buffer<T>(buffer: &[T]) -> Result<(), crate::dma::BufferError> {
    if macros::within(buffer, &DMA_RAM) {
        Ok(())
    } else {
        Err(crate::dma::BufferError::Unreachable)
    }
}

/// DS11581 and RM0430, at 2.7 V to 3.6 V, in voltage scale 1, which 100 MHz
/// needs on this chip (DS11581 Table 17). `freeze` writes it: VOS resets to
/// scale 2. HCLK and APB2 up to 100 MHz, APB1 up to 50 MHz (Table 17); the
/// HSE crystal 4 to 26 MHz (Table 43), an external clock 1 to 50 MHz
/// (Table 41); the PLL input 0.95 to 2.1 MHz and its VCO 100 to 432 MHz
/// (Table 47); one wait state per 25 MHz up to 3 at 100 MHz (RM0430
/// Table 6), not the 30 MHz of the other F4s.
pub(crate) const CLOCK_LIMITS: Limits = Limits {
    sysclk_max: Hertz::mhz(100),
    hclk_max: Hertz::mhz(100),
    pclk1_max: Hertz::mhz(50),
    pclk2_max: Hertz::mhz(100),
    hse_oscillator: (Hertz::mhz(4), Hertz::mhz(26)),
    hse_bypass: (Hertz::mhz(1), Hertz::mhz(50)),
    pll_input: (Hertz::khz(950), Hertz::khz(2_100)),
    pll_vco: (Hertz::mhz(100), Hertz::mhz(432)),
    flash_step_2v7: Hertz::mhz(25),
    max_wait_states: 3,
    vos: pac::pwr::vals::Vos::SCALE1,
    overdrive: None,
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

// Timers another timer's trigger output can start (RM0430 Tables 102 and
// 106).

timer_trigger!(TIM8: TIM1, 0);
timer_trigger!(TIM3: TIM1, 0);
timer_trigger!(TIM2: TIM3, 2);

// The temperature sensor on ADC1_IN18, VREFINT on IN17 (RM0430 13.9).
adc_internal!(ADC1: 18, 17);

/// The factory calibration values (DS11581 Tables 82 and 85).
pub(crate) const VREFIN_CAL: *const u16 = 0x1FFF_7A2A as *const u16;
pub(crate) const TS_CAL1: *const u16 = 0x1FFF_7A2C as *const u16;
pub(crate) const TS_CAL2: *const u16 = 0x1FFF_7A2E as *const u16;

/// The unique ID's block (RM0430 35.1).
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
// alone (RM0430 Figure 1).
gpio_dma!(
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

/// The die revision code, `DBGMCU_IDCODE.REV_ID` (bits 31:16) on RM0430.
pub(crate) fn revision() -> u16 {
    pac::DBGMCU.idcode().read().rev_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rcc::{ClockError, Config, Hse, HseMode, Request};
    use crate::time::Hertz;

    #[test]
    fn solve_reaches_100_mhz_and_refuses_more() {
        let request = |sysclk| Request {
            hse: Hse {
                freq: Hertz::mhz(8),
                mode: HseMode::Oscillator,
            },
            sysclk,
            usb: None,
        };
        let clocks = Config::solve(request(Hertz::mhz(100)))
            .unwrap()
            .validate()
            .unwrap();
        assert_eq!(clocks.flash_wait_states, 3);
        // 75 MHz is two wait states here (RM0430 Table 6), where a 30 MHz
        // step would allow 76-90 MHz on two.
        let clocks = Config::solve(request(Hertz::mhz(76)))
            .unwrap()
            .validate()
            .unwrap();
        assert_eq!(clocks.flash_wait_states, 3);
        assert_eq!(
            Config::solve(request(Hertz::mhz(120))),
            Err(ClockError::Sysclk)
        );
    }

    #[test]
    fn all_320_kib_of_sram_is_reachable() {
        // SAFETY: never dereferenced; only the address range is checked.
        let end = unsafe { core::slice::from_raw_parts(0x2004_ff00 as *const u8, 256) };
        assert!(check_dma_buffer(end).is_ok());
        // SAFETY: as above.
        let past = unsafe { core::slice::from_raw_parts(0x2004_ff00 as *const u8, 257) };
        assert!(check_dma_buffer(past).is_err());
    }
}
