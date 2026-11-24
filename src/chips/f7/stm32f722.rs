//! STM32F722 glue: the chip facts the IP modules leave out, for every package
//! of the line.
//!
//! The routing facts (RCC bits, vectors, pin alternate functions, DMA
//! request channels) are generated from stm32-metapac's metadata into
//! `gen/<part>.rs` by `tools/glue-gen`, one file per package, cut to its
//! bonded pins; this file holds the facts metapac does not carry, each citing its manual. What
//! the F7 dies share (clock limits, the DMA's view of memory, the bootloader)
//! is in `f7/common.rs`.
//!
//! The chip's U(S)ARTs are usart_v3, which the usart_v4 module serves
//! (ADR-011).
//!
//! Requirements: GLUE-001, GLUE-002

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a driver reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma::{Channel, Dma, DmaInstance},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{I2cRxDma, I2cTxDma, SclPin, SdaPin},
    rcc_f7::{Bus, RCC, RccBit},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    uart::{Info, RxDma, RxPin, TxDma, TxPin},
    usart_v4::Unit,
};

#[path = "../macros.rs"]
#[macro_use]
mod macros;

// The F4's macro shapes: fixed-request DMA, bus-named RCC registers. Its
// `usarts!` takes each instance's kind: UART4/5/7/8 have no smartcard mode
// (RM0431 Table 164), and 0.5 and 1.5 stop bits are smartcard settings (RM0431
// 27.5.2), so the driver refuses them there, as on the F4.
#[path = "../f4/macros.rs"]
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

/// The full-speed USB port's core (full-speed only), its FIFO RAM in words
/// and its endpoints (RM0431 Table 226: 1.25 KiB, six bidirectional
/// endpoints): facts metapac does not carry.
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = false;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 320;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 6;

// The die's errata (ES0360), and the prefetch erratum `freeze` checks: none on
// this die.
#[path = "errata_es0360.rs"]
mod errata_table;
pub use errata_table::*;
pub(crate) const PREFETCH_ERRATUM: Option<&crate::errata::Erratum> = None;
/// The I2C reload erratum `i2c_v2` checks.
pub(crate) const I2C_RELOAD_ERRATUM: &crate::errata::Erratum = &ES0360_2_9_4;

// The routing rows, generated from stm32-metapac's metadata for the
// part's device and package; a flash-size variant takes its package's.
#[cfg(any(feature = "stm32f722re", feature = "stm32f722rc"))]
#[path = "../gen/stm32f722re.rs"]
mod generated;
#[cfg(any(feature = "stm32f722ve", feature = "stm32f722vc"))]
#[path = "../gen/stm32f722ve.rs"]
mod generated;
#[cfg(any(feature = "stm32f722ze", feature = "stm32f722zc"))]
#[path = "../gen/stm32f722ze.rs"]
mod generated;
#[cfg(any(feature = "stm32f722ie", feature = "stm32f722ic"))]
#[path = "../gen/stm32f722ie.rs"]
mod generated;
pub use generated::*;

#[path = "common.rs"]
mod f7;
pub(crate) use f7::{
    CLOCK_LIMITS, SYSCFG_ENABLE, SYSTEM_MEMORY, iwdg_regs, map_system_memory, revision,
    set_watchdog_freeze, watchdog_frozen,
};

/// The DTCM: 64 KiB at the bottom of the SRAM space (RM0431 1.7).
const DTCM: (usize, usize) = (0x2000_0000, 64 * 1024);

/// SRAM1 and SRAM2, together: 176 KiB of SRAM1 at
/// 0x2001_0000 and 16 KiB of SRAM2 at 0x2003_C000 (RM0431 1.7).
const SRAM: (usize, usize) = (0x2001_0000, 192 * 1024);

/// Whether `buffer` can be handed to a DMA channel: in the DTCM, or in SRAM1
/// and SRAM2 and, with the data cache on, non-cacheable (see
/// `f7/common.rs`). Every DMA driver checks its buffers with this.
pub fn check_dma_buffer<T>(buffer: &[T]) -> Result<(), crate::dma::BufferError> {
    f7::check_dma_buffer(buffer, DTCM, SRAM, None)
}

// Timers another timer's trigger output can start (RM0431 Table 121).

timer_trigger!(TIM3: TIM1, 0);
timer_trigger!(TIM2: TIM3, 2);

// The temperature sensor on ADC1_IN18, which it shares with VBAT (VBATE
// stays clear), and VREFINT on IN17 (RM0431 14.10).
adc_internal!(ADC1: 18, 17);

/// The factory calibration values (DS11853 Tables 74 and 77).
pub(crate) const VREFIN_CAL: *const u16 = 0x1FF0_7A2A as *const u16;
pub(crate) const TS_CAL1: *const u16 = 0x1FF0_7A2C as *const u16;
pub(crate) const TS_CAL2: *const u16 = 0x1FF0_7A2E as *const u16;

/// The unique ID's block (RM0431 35.1).
pub(crate) const UID_BASE: usize = 0x1FF0_7A10;

// Only DMA2's peripheral port reaches the GPIO ports; DMA1's is not on the
// bus matrix (RM0431 8.3.2).
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
            Channel<2, 3>,
            Channel<2, 0>,
        >;
        assert_eq!(Spi1Example::RESOURCES[0], Resource::Peripheral("SPI1"));
        assert!(Spi1Example::RESOURCES.contains(&Resource::Dma {
            signal: Signal::Tx,
            controller: 2,
            channel: 3,
            request: 3
        }));
        assert!(Spi1Example::RESOURCES.contains(&Resource::Dma {
            signal: Signal::Rx,
            controller: 2,
            channel: 0,
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
        assert_eq!(SRAM.0 + SRAM.1, 0x2004_0000);
    }

    #[test]
    fn uart4_and_uart5_refuse_fractional_stop_bits() {
        use crate::uart::{self, StopBits};
        let clocks = Config::hsi().validate().unwrap();
        let half = uart::Config::new(Hertz(115_200)).with_stop_bits(StopBits::Half);
        let one_and_half = half.with_stop_bits(StopBits::OneAndHalf);
        let two = half.with_stop_bits(StopBits::Two);
        assert_eq!(
            uart::check::<UART4>(&clocks, &half),
            Err(uart::ConfigError::Unsupported)
        );
        assert_eq!(
            uart::check::<UART5>(&clocks, &one_and_half),
            Err(uart::ConfigError::Unsupported)
        );
        assert_eq!(uart::check::<UART4>(&clocks, &two), Ok(()));
        assert_eq!(uart::check::<USART3>(&clocks, &half), Ok(()));
    }

    #[test]
    fn usart_v3_takes_half_duplex_swap_and_inversion() {
        let clocks = Config::hsi().validate().unwrap();
        let frame = crate::uart::Config::new(Hertz(115_200));
        for frame in [
            frame.with_half_duplex(true),
            frame.with_swap(true),
            frame.with_inverted_rx(true).with_inverted_tx(true),
        ] {
            assert_eq!(crate::uart::check::<USART1>(&clocks, &frame), Ok(()));
        }
    }
}
