//! STM32F767 glue: the chip facts the IP modules leave out, for every
//! package of every line cut from die 0x451 (RM0410): the F765, F767, F768,
//! F769, F777, F778 and F779. They differ in blocks FerroHAL does not drive
//! (Ethernet, JPEG, LTDC, DSI, CRYP and HASH) and in the pins a package
//! bonds; both are in each package's generated glue, and the F769's and
//! F779's DSI pinouts are packages of their own.
//!
//! The routing facts (RCC bits, vectors, pin alternate functions, DMA
//! request channels) are generated from stm32-metapac's metadata by
//! `tools/glue-gen`, one file per package, cut to its bonded pins; this file
//! holds the facts metapac does not carry, each citing its manual. What the
//! F7 dies share (clock limits, the DMA's view of memory, the bootloader) is
//! in `f7/common.rs`.
//!
//! The Nucleo-F767ZI is the F7's proof board; until it runs the benches,
//! every F7 part is unproven.
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
    dma::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{SclPin, SdaPin},
    rcc_f7::{Bus, RCC, RccBit},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    uart::{Info, RxDma, RxPin, TxDma, TxPin},
    usart_v4::Unit,
};

#[path = "../macros.rs"]
#[macro_use]
mod macros;

// The F4's macro shapes: fixed-request stream DMA, bus-named RCC registers. Its
// `usarts!` takes each instance's kind: UART4/5/7/8 have no smartcard mode
// (RM0410 Table 226), and 0.5 and 1.5 stop bits are smartcard settings (RM0410
// 34.5.2), so the driver refuses them there, as on the F4.
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
/// and its endpoints (RM0410 Table 295: 1.25 KiB, which it rounds to 1.2,
/// six bidirectional endpoints): facts metapac does not carry.
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = false;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 320;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 6;

// The die's errata (ES0334), and the prefetch erratum `freeze` checks: none on
// this die.
#[path = "errata_es0334.rs"]
mod errata_table;
pub use errata_table::*;
pub(crate) const PREFETCH_ERRATUM: Option<&crate::errata::Erratum> = None;
/// The I2C reload erratum `i2c_v2` checks.
pub(crate) const I2C_RELOAD_ERRATUM: &crate::errata::Erratum = &ES0334_2_13_5;

// The routing rows, generated from stm32-metapac's metadata for the
// part's device and package; a flash-size variant takes its package's.
#[cfg(any(feature = "stm32f765vi", feature = "stm32f765vg"))]
#[path = "../gen/stm32f765vi.rs"]
mod generated;
#[cfg(any(feature = "stm32f765zi", feature = "stm32f765zg"))]
#[path = "../gen/stm32f765zi.rs"]
mod generated;
#[cfg(any(feature = "stm32f765ii", feature = "stm32f765ig"))]
#[path = "../gen/stm32f765ii.rs"]
mod generated;
#[cfg(any(feature = "stm32f765bi", feature = "stm32f765bg"))]
#[path = "../gen/stm32f765bi.rs"]
mod generated;
#[cfg(any(feature = "stm32f765ni", feature = "stm32f765ng"))]
#[path = "../gen/stm32f765ni.rs"]
mod generated;
#[cfg(any(feature = "stm32f767vi", feature = "stm32f767vg"))]
#[path = "../gen/stm32f767vi.rs"]
mod generated;
#[cfg(any(feature = "stm32f767zi", feature = "stm32f767zg"))]
#[path = "../gen/stm32f767zi.rs"]
mod generated;
#[cfg(any(feature = "stm32f767ii", feature = "stm32f767ig"))]
#[path = "../gen/stm32f767ii.rs"]
mod generated;
#[cfg(any(feature = "stm32f767bi", feature = "stm32f767bg"))]
#[path = "../gen/stm32f767bi.rs"]
mod generated;
#[cfg(any(feature = "stm32f767ni", feature = "stm32f767ng"))]
#[path = "../gen/stm32f767ni.rs"]
mod generated;
#[cfg(feature = "stm32f768ai")]
#[path = "../gen/stm32f768ai.rs"]
mod generated;
#[cfg(any(feature = "stm32f769ai", feature = "stm32f769ag"))]
#[path = "../gen/stm32f769ai.rs"]
mod generated;
#[cfg(any(feature = "stm32f769bi", feature = "stm32f769bg"))]
#[path = "../gen/stm32f769bi.rs"]
mod generated;
#[cfg(any(feature = "stm32f769ii", feature = "stm32f769ig"))]
#[path = "../gen/stm32f769ii.rs"]
mod generated;
#[cfg(any(feature = "stm32f769ni", feature = "stm32f769ng"))]
#[path = "../gen/stm32f769ni.rs"]
mod generated;
#[cfg(feature = "stm32f777vi")]
#[path = "../gen/stm32f777vi.rs"]
mod generated;
#[cfg(feature = "stm32f777zi")]
#[path = "../gen/stm32f777zi.rs"]
mod generated;
#[cfg(feature = "stm32f777ii")]
#[path = "../gen/stm32f777ii.rs"]
mod generated;
#[cfg(feature = "stm32f777bi")]
#[path = "../gen/stm32f777bi.rs"]
mod generated;
#[cfg(feature = "stm32f777ni")]
#[path = "../gen/stm32f777ni.rs"]
mod generated;
#[cfg(feature = "stm32f778ai")]
#[path = "../gen/stm32f778ai.rs"]
mod generated;
#[cfg(feature = "stm32f779ai")]
#[path = "../gen/stm32f779ai.rs"]
mod generated;
#[cfg(feature = "stm32f779bi")]
#[path = "../gen/stm32f779bi.rs"]
mod generated;
#[cfg(feature = "stm32f779ii")]
#[path = "../gen/stm32f779ii.rs"]
mod generated;
#[cfg(feature = "stm32f779ni")]
#[path = "../gen/stm32f779ni.rs"]
mod generated;
pub use generated::*;

#[path = "common.rs"]
mod f7;
pub(crate) use f7::{
    CLOCK_LIMITS, SYSCFG_ENABLE, SYSTEM_MEMORY, iwdg_regs, map_system_memory, revision,
    set_watchdog_freeze, watchdog_frozen,
};

/// The DTCM: 128 KiB at the bottom of the SRAM space (RM0410 2.2.2).
const DTCM: (usize, usize) = (0x2000_0000, 128 * 1024);

/// SRAM1 and SRAM2, together: 368 KiB of SRAM1 at 0x2002_0000 and 16 KiB of
/// SRAM2 at 0x2007_C000 (RM0410 2.2.2).
const SRAM: (usize, usize) = (0x2002_0000, 384 * 1024);

/// Whether `buffer` can be handed to a DMA stream: in the DTCM, except on
/// revision A, or in SRAM1 and SRAM2 and, with the data cache on,
/// non-cacheable (see `f7/common.rs`). Every DMA driver checks its buffers
/// with this.
pub fn check_dma_buffer<T>(buffer: &[T]) -> Result<(), crate::dma::BufferError> {
    f7::check_dma_buffer(buffer, DTCM, SRAM, Some(&ES0334_2_2_4))
}

// Timers another timer's trigger output can start (RM0410 Table 183).

timer_trigger!(TIM3: TIM1, 0);
timer_trigger!(TIM2: TIM3, 2);

// The temperature sensor on ADC1_IN18, which it shares with VBAT (VBATE
// stays clear), and VREFINT on IN17 (RM0410 15.10).
adc_internal!(ADC1: 18, 17);

/// The factory calibration values (DS11532 Tables 79 and 82).
pub(crate) const VREFIN_CAL: *const u16 = 0x1FF0_F44A as *const u16;
pub(crate) const TS_CAL1: *const u16 = 0x1FF0_F44C as *const u16;
pub(crate) const TS_CAL2: *const u16 = 0x1FF0_F44E as *const u16;

/// The unique ID's block (RM0410 45.1).
pub(crate) const UID_BASE: usize = 0x1FF0_F420;

// Only DMA2's peripheral port reaches the GPIO ports; DMA1's is not on the
// bus matrix (RM0410 8.3.2).
gpio_dma!(
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rcc::{Config, Hse, HseMode, Request};
    use crate::time::Hertz;

    #[test]
    fn solve_gives_216_mhz_and_usb_in_overdrive() {
        let clocks = Config::solve(Request {
            hse: Hse {
                freq: Hertz::mhz(8),
                mode: HseMode::Oscillator,
            },
            sysclk: Hertz::mhz(216),
            usb: Some(Hertz::mhz(48)),
        })
        .unwrap()
        .validate()
        .unwrap();
        assert!(clocks.overdrive);
        assert_eq!(clocks.pll_q, Some(Hertz::mhz(48)));
        assert_eq!(clocks.flash_wait_states, 7);
    }

    #[test]
    fn the_dtcm_is_128_kib_and_sram2_ends_the_block() {
        // SRAM1 starts where the DTCM's 128 KiB end; SRAM2's 16 KiB end the
        // block at 0x2008_0000.
        assert_eq!(SRAM.0, DTCM.0 + DTCM.1);
        assert_eq!(SRAM.0 + SRAM.1, 0x2008_0000);
    }

    #[test]
    fn only_revision_a_has_the_dtcm_erratum() {
        assert!(ES0334_2_2_4.affects(0x1000));
        assert!(!ES0334_2_2_4.affects(0x1001));
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
