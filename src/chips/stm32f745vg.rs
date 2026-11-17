//! STM32F745 glue: the chip facts the IP modules leave out, for every package
//! of the line.
//!
//! The routing facts (RCC bits, vectors, pin alternate functions, DMA
//! request channels) are generated from stm32-metapac's metadata into
//! `gen/<part>.rs` by `tools/glue-gen`, one file per package, cut to its
//! bonded pins; this file holds the facts metapac does not carry, each citing its manual. What
//! the F7 dies share (clock limits, the DMA's view of memory, the bootloader)
//! is in `shared/f7.rs`.
//!
//! The chip's U(S)ARTs are usart_v3, which the UART redesign serves
//! (ADR-011): until then this chip has no UART driver, and its glue no
//! UART rows.
//!
//! Requirements: GLUE-001, GLUE-002

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a driver reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma_v2::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{SclPin, SdaPin},
    rcc_f7::{Bus, RCC, RccBit},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
};

#[path = "shared/macros.rs"]
#[macro_use]
mod macros;

// The F4's macro shapes: fixed-request stream DMA, bus-named RCC registers.
// Its `usarts!` stays unused until the F7 has a UART driver.
#[path = "shared/f4_macros.rs"]
#[macro_use]
#[allow(unused_macros)]
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

/// The full-speed USB port's core (full-speed only), its FIFO RAM in words
/// and its endpoints (RM0385 Table 263: 1.25 KiB, six bidirectional
/// endpoints): facts metapac does not carry.
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = false;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 320;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 6;

// The die's errata (ES0290), and the prefetch erratum `freeze` checks: none on
// this die.
#[path = "shared/errata_es0290.rs"]
mod errata_table;
pub use errata_table::*;
pub(crate) const PREFETCH_ERRATUM: Option<&crate::errata::Erratum> = None;
/// The I2C reload erratum `i2c_v2` checks.
pub(crate) const I2C_RELOAD_ERRATUM: &crate::errata::Erratum = &ES0290_2_9_4;

// The routing rows, generated from stm32-metapac's metadata for the
// part's device and package; a flash-size variant takes its package's.
#[cfg(any(feature = "stm32f745vg", feature = "stm32f745ve"))]
#[path = "gen/stm32f745vg.rs"]
mod generated;
#[cfg(any(feature = "stm32f745zg", feature = "stm32f745ze"))]
#[path = "gen/stm32f745zg.rs"]
mod generated;
#[cfg(any(feature = "stm32f745ig", feature = "stm32f745ie"))]
#[path = "gen/stm32f745ig.rs"]
mod generated;
pub use generated::*;

#[path = "shared/f7.rs"]
mod f7;
pub(crate) use f7::{
    CLOCK_LIMITS, SYSCFG_ENABLE, SYSTEM_MEMORY, iwdg_regs, map_system_memory, revision,
    set_watchdog_freeze, watchdog_frozen,
};

/// The DTCM: 64 KiB at the bottom of the SRAM space (RM0385 2.3).
const DTCM: (usize, usize) = (0x2000_0000, 64 * 1024);

/// SRAM1 and SRAM2, together: 240 KiB of SRAM1 at
/// 0x2001_0000 and 16 KiB of SRAM2 at 0x2004_C000 (RM0385 2.3).
const SRAM: (usize, usize) = (0x2001_0000, 256 * 1024);

/// Whether `buffer` can be handed to a DMA stream: in the DTCM, or in SRAM1
/// and SRAM2 and, with the data cache on, non-cacheable (see
/// `shared/f7.rs`). Every DMA driver checks its buffers with this.
pub fn check_dma_buffer<T>(buffer: &[T]) -> Result<(), crate::dma::BufferError> {
    f7::check_dma_buffer(buffer, DTCM, SRAM, None)
}

// Timers another timer's trigger output can start (RM0385 Table 154).

timer_trigger!(TIM3: TIM1, 0);
timer_trigger!(TIM2: TIM3, 2);

// The temperature sensor on ADC1_IN18, which it shares with VBAT (VBATE
// stays clear), and VREFINT on IN17 (RM0385 15.10).
adc_internal!(ADC1: 18, 17);

/// The factory calibration values (DS10916 Tables 70 and 73).
pub(crate) const VREFIN_CAL: *const u16 = 0x1FF0_F44A as *const u16;
pub(crate) const TS_CAL1: *const u16 = 0x1FF0_F44C as *const u16;
pub(crate) const TS_CAL2: *const u16 = 0x1FF0_F44E as *const u16;

/// The unique ID's block (RM0385 41.1).
pub(crate) const UID_BASE: usize = 0x1FF0_F420;

// Only DMA2's peripheral port reaches the GPIO ports; DMA1's is not on the
// bus matrix (RM0385 8.3.2).
gpio_dma!(
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rcc::{ClockError, Config, Hse, HseMode, PllP, Request};
    use crate::resource::{Resource, Signal};
    use crate::time::Hertz;

    #[test]
    fn spi_wiring_describes_itself() {
        type Spi1Example = crate::spi::Wiring<
            SPI1,
            Pin<'A', 5, Alternate<5>>,
            Pin<'A', 6, Alternate<5>>,
            Pin<'A', 7, Alternate<5>>,
            Stream<2, 3>,
            Stream<2, 0>,
        >;
        assert_eq!(Spi1Example::RESOURCES[0], Resource::Peripheral("SPI1"));
        assert!(Spi1Example::RESOURCES.contains(&Resource::Dma {
            signal: Signal::Tx,
            controller: 2,
            stream: 3,
            request: 3
        }));
        assert!(Spi1Example::RESOURCES.contains(&Resource::Dma {
            signal: Signal::Rx,
            controller: 2,
            stream: 0,
            request: 3
        }));
    }

    fn request(sysclk: Hertz, usb: Option<Hertz>) -> Request {
        Request {
            hse: Hse {
                freq: Hertz::mhz(8),
                mode: HseMode::Oscillator,
            },
            sysclk,
            usb,
        }
    }

    #[test]
    fn solve_gives_216_mhz_and_usb_in_overdrive() {
        // 8 MHz crystal: a 1 MHz PLL input, a 432 MHz VCO, P 2 and Q 9.
        let config = Config::solve(request(Hertz::mhz(216), Some(Hertz::mhz(48)))).unwrap();
        assert_eq!(
            config.pll.map(|pll| (pll.divp, pll.divq)),
            Some((PllP::Div2, Some(9)))
        );
        let clocks = config.validate().unwrap();
        assert!(clocks.overdrive);
        // The over-drive's APB limits: 54 and 108 MHz.
        assert_eq!(
            (clocks.pclk1, clocks.pclk2),
            (Hertz::mhz(54), Hertz::mhz(108))
        );
        assert_eq!(clocks.flash_wait_states, 7);
    }

    #[test]
    fn at_180_mhz_the_overdrive_stays_off_and_the_apbs_drop() {
        let clocks = Config::solve(request(Hertz::mhz(180), None))
            .unwrap()
            .validate()
            .unwrap();
        assert!(!clocks.overdrive);
        assert_eq!(
            (clocks.pclk1, clocks.pclk2),
            (Hertz::mhz(45), Hertz::mhz(90))
        );
        assert_eq!(clocks.flash_wait_states, 5);
    }

    #[test]
    fn solve_refuses_more_than_216_mhz() {
        assert_eq!(
            Config::solve(request(Hertz::mhz(217), None)),
            Err(ClockError::Sysclk)
        );
    }

    #[test]
    fn sram_runs_from_the_dtcm_to_sram2s_end() {
        // SRAM1 starts where the DTCM's 64 KiB end; SRAM2's 16 KiB end the
        // block.
        assert_eq!(SRAM.0, 0x2000_0000 + 64 * 1024);
        assert_eq!(SRAM.0 + SRAM.1, 0x2005_0000);
    }
}
