//! STM32F427 glue: the chip facts the IP modules leave out, for every package
//! of the line.
//!
//! The F427 is die 0x419, which the F429 shares with LTDC added (FerroHAL
//! drives neither LTDC nor the FMC). The routing facts are generated from
//! stm32-metapac's metadata into `gen/<part>.rs` by `tools/glue-gen`, one
//! file per package; the die's hand facts are `f4/die_0x419.rs`.
//!
//! No board carries it on the rig: the line is unproven.
//!
//! Requirements: GLUE-001, GLUE-002

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{SclPin, SdaPin},
    rcc_f4::{Bus, Limits, Overdrive, RCC, RccBit},
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

#[path = "die_0x419.rs"]
#[macro_use]
mod die;
pub use die::*;

// The routing rows, generated from stm32-metapac's metadata for the
// part's device and package; a flash-size variant takes its package's.
#[cfg(any(feature = "stm32f427vi", feature = "stm32f427vg"))]
#[path = "../gen/stm32f427vi.rs"]
mod generated;
#[cfg(any(feature = "stm32f427zi", feature = "stm32f427zg"))]
#[path = "../gen/stm32f427zi.rs"]
mod generated;
#[cfg(any(feature = "stm32f427ii", feature = "stm32f427ig"))]
#[path = "../gen/stm32f427ii.rs"]
mod generated;
#[cfg(any(feature = "stm32f427ai", feature = "stm32f427ag"))]
#[path = "../gen/stm32f427ai.rs"]
mod generated;
pub use generated::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rcc::{ClockError, Config, Hse, HseMode, Request};
    use crate::time::Hertz;

    fn request(sysclk: Hertz) -> Request {
        Request {
            hse: Hse {
                freq: Hertz::mhz(8),
                mode: HseMode::Oscillator,
            },
            sysclk,
            usb: None,
        }
    }

    #[test]
    fn solve_reaches_180_mhz_with_the_overdrive() {
        let clocks = Config::solve(request(Hertz::mhz(180)))
            .unwrap()
            .validate()
            .unwrap();
        assert!(clocks.overdrive);
        assert!(clocks.pclk1 <= Hertz::mhz(45) && clocks.pclk2 <= Hertz::mhz(90));
        assert_eq!(clocks.flash_wait_states, 5);
        assert_eq!(
            Config::solve(request(Hertz::mhz(200))),
            Err(ClockError::Sysclk)
        );
    }

    #[test]
    fn revisions_a_and_y_have_no_overdrive() {
        // ES0206 2.2.12: freeze refuses a tree that needs it there, and
        // only there.
        for (revision, withheld) in [
            (0x1000, true),
            (0x1003, true),
            (0x1007, false),
            (0x2001, false),
            (0x2003, false),
        ] {
            assert_eq!(CLOCK_LIMITS.overdrive_withheld(true, revision), withheld);
            assert!(!CLOCK_LIMITS.overdrive_withheld(false, revision));
        }
    }

    #[test]
    fn sram3_is_reachable_and_ccm_ram_is_not() {
        // SAFETY: never dereferenced; only the address range is checked.
        let sram3 = unsafe { core::slice::from_raw_parts(0x2002_0000 as *const u8, 64 * 1024) };
        assert!(check_dma_buffer(sram3).is_ok());
        // SAFETY: as above.
        let past = unsafe { core::slice::from_raw_parts(0x2002_ff00 as *const u8, 257) };
        assert!(check_dma_buffer(past).is_err());
        // SAFETY: as above.
        let ccm = unsafe { core::slice::from_raw_parts(0x1000_0000 as *const u8, 16) };
        assert!(check_dma_buffer(ccm).is_err());
    }
}
