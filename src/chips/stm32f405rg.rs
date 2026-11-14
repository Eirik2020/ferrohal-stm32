//! STM32F405RG glue: the chip facts the IP modules leave out.
//!
//! The routing facts (RCC bits, vectors, pin alternate functions, DMA
//! request channels), cut to the LQFP64's bonded pins, are generated from
//! stm32-metapac's metadata into `gen/stm32f405rg.rs` by `tools/glue-gen`;
//! this file holds the facts metapac does not carry, each citing its manual.
//!
//! The Foxeer F405 V2 flight controller carries this chip; there is no
//! Nucleo for it, so its hardware tests run on that board.
//!
//! Requirements: GLUE-001, GLUE-002, GLUE-004, GLUE-005, GLUE-006

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma_v2::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{SclPin, SdaPin},
    rcc_f4::{Bus, Limits, RCC, RccBit},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    time::Hertz,
    uart::{Info, RxDma, RxPin, TxDma, TxPin},
    usart_v2::LineRegs,
};

#[path = "shared/macros.rs"]
#[macro_use]
mod macros;

#[path = "shared/f4_macros.rs"]
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
/// its endpoints: facts metapac does not carry.
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = false;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 320;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 4;

// The routing rows, generated from stm32-metapac's metadata.
#[path = "gen/stm32f405rg.rs"]
mod generated;
pub use generated::*;

/// SRAM1 (112 KiB) and SRAM2 (16 KiB), contiguous (RM0090 section 2.3).
/// Not the 64 KiB of CCM RAM at 0x1000_0000: only the core reaches it, so a
/// DMA buffer there would never see a byte (RM0090 figure 1).
const DMA_RAM: [(usize, usize); 1] = [(0x2000_0000, 128 * 1024)];

/// Whether `buffer` can be handed to a DMA stream: whether it lies in SRAM1
/// or SRAM2. A Cortex-M4 has no data cache, so that is all. `Uart::new`
/// checks every buffer with this; firmware that places statics in CCM RAM
/// can use it to check its own.
pub fn check_dma_buffer(buffer: &[u8]) -> Result<(), crate::dma::BufferError> {
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

/// The die revision code, `DBGMCU_IDCODE.REV_ID` (bits 31:16) on RM0090.
pub(crate) fn revision() -> u16 {
    pac::DBGMCU.idcode().read().rev_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uart::{Resource, Signal, Wiring};

    /// A receiver port, as an example board manifest names it.
    type Usart2Example = Wiring<
        USART2,
        Pin<'A', 2, Alternate<7>>,
        Pin<'A', 3, Alternate<7>>,
        Stream<1, 6>,
        Stream<1, 5>,
    >;

    #[test]
    fn wiring_describes_itself() {
        assert_eq!(
            Usart2Example::RESOURCES,
            [
                Resource::Peripheral("USART2"),
                Resource::Pin {
                    signal: Signal::Tx,
                    port: 'A',
                    pin: 2,
                    af: 7
                },
                Resource::Pin {
                    signal: Signal::Rx,
                    port: 'A',
                    pin: 3,
                    af: 7
                },
                // The example manifest names these DMA1_STREAM6_CH4 and
                // DMA1_STREAM5_CH4.
                Resource::Dma {
                    signal: Signal::Tx,
                    controller: 1,
                    stream: 6,
                    request: 4
                },
                Resource::Dma {
                    signal: Signal::Rx,
                    controller: 1,
                    stream: 5,
                    request: 4
                },
                Resource::Interrupt("USART2"),
                Resource::Interrupt("DMA1_STREAM5"),
                Resource::Interrupt("DMA1_STREAM6"),
            ]
        );
    }

    /// Every UART and timer-lane route an example board manifest names is one this
    /// glue offers, on the pin and alternate function it names and with its
    /// DMA channel. That it compiles is half the check.
    #[test]
    fn example_manifest_routes_exist() {
        use crate::{
            timer::{ChannelPin, ChannelTimer, CompareDma, ComplementaryPin},
            uart::Instance,
        };

        type Af<const P: char, const N: u8, const F: u8> = Pin<P, N, Alternate<F>>;
        fn tx<U: Instance, P: TxPin<U>, D: TxDma<U>>() -> u8 {
            D::REQUEST
        }
        fn rx<U: Instance, P: RxPin<U>, D: RxDma<U>>() -> u8 {
            D::REQUEST
        }
        fn lane<T: ChannelTimer, const C: u8, P: ChannelPin<T, C>, D: CompareDma<T, C>>() -> u8 {
            D::REQUEST
        }
        fn lane_n<T: ChannelTimer, const C: u8, P: ComplementaryPin<T, C>, D: CompareDma<T, C>>()
        -> u8 {
            D::REQUEST
        }

        assert_eq!(rx::<USART2, Af<'A', 3, 7>, Stream<1, 5>>(), 4);
        assert_eq!(tx::<USART2, Af<'A', 2, 7>, Stream<1, 6>>(), 4);
        assert_eq!(rx::<USART3, Af<'C', 11, 7>, Stream<1, 1>>(), 4);
        assert_eq!(tx::<USART3, Af<'C', 10, 7>, Stream<1, 3>>(), 4);
        assert_eq!(rx::<UART4, Af<'A', 1, 8>, Stream<1, 2>>(), 4);
        assert_eq!(tx::<UART4, Af<'A', 0, 8>, Stream<1, 4>>(), 4);
        assert_eq!(rx::<USART1, Af<'A', 10, 7>, Stream<2, 5>>(), 4);
        assert_eq!(lane::<TIM1, 1, Af<'A', 8, 1>, Stream<2, 1>>(), 6);
        assert_eq!(lane::<TIM8, 4, Af<'C', 9, 3>, Stream<2, 7>>(), 7);
        assert_eq!(lane::<TIM8, 3, Af<'C', 8, 3>, Stream<2, 2>>(), 0);
        assert_eq!(lane_n::<TIM1, 3, Af<'B', 15, 1>, Stream<2, 6>>(), 6);
    }

    /// An SPI bus, as an example board manifest names it.
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
        assert_eq!(
            Spi1Example::RESOURCES,
            [
                Resource::Peripheral("SPI1"),
                Resource::Pin {
                    signal: Signal::Sck,
                    port: 'A',
                    pin: 5,
                    af: 5
                },
                Resource::Pin {
                    signal: Signal::Miso,
                    port: 'A',
                    pin: 6,
                    af: 5
                },
                Resource::Pin {
                    signal: Signal::Mosi,
                    port: 'A',
                    pin: 7,
                    af: 5
                },
                // DMA2_STREAM3_CH3 and DMA2_STREAM0_CH3 in the manifest.
                Resource::Dma {
                    signal: Signal::Tx,
                    controller: 2,
                    stream: 3,
                    request: 3
                },
                Resource::Dma {
                    signal: Signal::Rx,
                    controller: 2,
                    stream: 0,
                    request: 3
                },
                Resource::Interrupt("DMA2_STREAM0"),
            ]
        );
    }

    #[test]
    fn ccm_ram_is_refused() {
        // SAFETY: never dereferenced; only the address range is checked.
        let ccm = unsafe { core::slice::from_raw_parts(0x1000_0000 as *const u8, 64) };
        assert_eq!(
            check_dma_buffer(ccm),
            Err(crate::dma::BufferError::Unreachable)
        );
        // SAFETY: as above.
        let sram2_end = unsafe { core::slice::from_raw_parts(0x2001_ff00 as *const u8, 256) };
        assert_eq!(check_dma_buffer(sram2_end), Ok(()));
        // SAFETY: as above.
        let past = unsafe { core::slice::from_raw_parts(0x2001_ff00 as *const u8, 257) };
        assert_eq!(
            check_dma_buffer(past),
            Err(crate::dma::BufferError::Unreachable)
        );
    }

    #[test]
    fn uart4_and_uart5_refuse_fractional_stop_bits() {
        use crate::uart::{self, StopBits, Uart};
        let clocks = crate::rcc_f4::Config::hsi().validate().unwrap();
        let half = uart::Config::new(115_200).with_stop_bits(StopBits::Half);
        let one_and_half = half.with_stop_bits(StopBits::OneAndHalf);
        let two = half.with_stop_bits(StopBits::Two);
        assert_eq!(
            crate::uart::check::<UART4>(&clocks, &half),
            Err(crate::uart::ConfigError::Unsupported)
        );
        assert_eq!(
            crate::uart::check::<UART5>(&clocks, &one_and_half),
            Err(crate::uart::ConfigError::Unsupported)
        );
        assert_eq!(crate::uart::check::<UART4>(&clocks, &two), Ok(()));
        assert_eq!(crate::uart::check::<USART3>(&clocks, &half), Ok(()));
    }

    #[test]
    fn foxeer_clocks_validate() {
        use crate::rcc_f4::{
            AhbPrescaler, ApbPrescaler, Config, Hse, HseMode, Pll, PllP, PllSource, Supply, Sysclk,
        };
        // 8 MHz crystal to 168 MHz, with 48 MHz on PLLQ for USB.
        let config = Config {
            hse: Some(Hse {
                freq: Hertz::mhz(8),
                mode: HseMode::Oscillator,
            }),
            pll: Some(Pll {
                source: PllSource::Hse,
                prediv: 8,
                mul: 336,
                divp: PllP::Div2,
                divq: Some(7),
            }),
            sys: Sysclk::PllP,
            ahb_pre: AhbPrescaler::Div1,
            apb1_pre: ApbPrescaler::Div4,
            apb2_pre: ApbPrescaler::Div2,
            supply: Supply::V2_7To3_6,
        };
        let clocks = config.validate().unwrap();
        assert_eq!(clocks.sysclk, Hertz::mhz(168));
        assert_eq!(
            (clocks.pclk1, clocks.pclk2),
            (Hertz::mhz(42), Hertz::mhz(84))
        );
        assert_eq!(
            (clocks.timclk1, clocks.timclk2),
            (Hertz::mhz(84), Hertz::mhz(168))
        );
        assert_eq!(clocks.pll_q, Some(Hertz::mhz(48)));
        assert_eq!(clocks.flash_wait_states, 5);
        // APB1 at 84 MHz is over its limit.
        let fast_apb1 = Config {
            apb1_pre: ApbPrescaler::Div2,
            ..config
        };
        assert!(fast_apb1.validate().is_err());
    }

    #[test]
    fn solve_matches_the_foxeer_tree() {
        use crate::rcc_f4::{
            AhbPrescaler, ApbPrescaler, ClockError, Config, Hse, HseMode, Pll, PllP, PllSource,
            Request, Supply, Sysclk,
        };
        // 8 MHz crystal, 168 MHz system, 48 MHz USB: the tree the Foxeer
        // freezes from (8 / 8 * 336, P 2, Q 7).
        assert_eq!(
            Config::solve(Request {
                hse: Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                },
                sysclk: Hertz::mhz(168),
                usb: Some(Hertz::mhz(48)),
            }),
            Ok(Config {
                hse: Some(Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                }),
                pll: Some(Pll {
                    source: PllSource::Hse,
                    prediv: 8,
                    mul: 336,
                    divp: PllP::Div2,
                    divq: Some(7),
                }),
                sys: Sysclk::PllP,
                ahb_pre: AhbPrescaler::Div1,
                apb1_pre: ApbPrescaler::Div4,
                apb2_pre: ApbPrescaler::Div2,
                supply: Supply::V2_7To3_6,
            })
        );
        // A USB clock the PLL cannot give exactly: no solution.
        assert_eq!(
            Config::solve(Request {
                hse: Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                },
                sysclk: Hertz::mhz(168),
                usb: Some(Hertz::mhz(47)),
            }),
            Err(ClockError::Unsatisfiable)
        );
    }
}
