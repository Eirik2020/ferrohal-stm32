//! STM32H743VI glue: the chip facts the IP modules leave out.
//!
//! The routing facts (RCC bits, vectors, pin alternate functions, DMAMUX
//! request IDs) are generated from stm32-metapac's metadata into
//! `gen/stm32h743vi.rs` by `tools/glue-gen`, cut to the package's bonded pins:
//! the Lucid fits the TFBGA100 (STM32H743VIH6), which DS12117 Table 8 shows
//! bonds the same GPIO set as the LQFP100. The supplies differ: the TFBGA100
//! has no VDDLDO supply (F4 is internally tied to VDD, ES0392 2.2.22) and no
//! VREF+ pin (double-bonded to VDDA, so VREFBUF stays disabled; DS12117
//! note 5).
//! Clock limits and the memory map, shared with the H753, are in
//! `shared/h7.rs`.
//!
//! The TBS Lucid H7 flight controller carries this chip. Its die is the
//! H753's without the crypto block, so the H753ZI Nucleo stands in for it.
//!
//! Requirements: GLUE-001, GLUE-002, GLUE-005, GLUE-006

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma_v1::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{SclPin, SdaPin},
    rcc_h7_rm0433::{Bus, RCC, RccBit},
    sdmmc::{CkPin, CmdPin, D0Pin, D1Pin, D2Pin, D3Pin},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    uart::{Info, RxDma, RxPin, TxDma, TxPin},
    usart_v4::LineRegs,
};

#[path = "shared/macros.rs"]
#[macro_use]
mod macros;

#[path = "shared/h7_macros.rs"]
#[macro_use]
mod family;

/// Which ADC carries the temperature sensor and VREFINT channels.
macro_rules! internal_adc {
    (ADC3) => {
        true
    };
    ($other:ident) => {
        false
    };
}

/// The full-speed USB port's core (high-speed capable, on its internal full-speed PHY), its FIFO RAM in words and
/// its endpoints: facts metapac does not carry.
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = true;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 1024;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 9;

// The routing rows, generated from stm32-metapac's metadata.
#[path = "gen/stm32h743vi.rs"]
mod generated;
pub use generated::*;
#[path = "shared/h7.rs"]
mod h7;

pub use h7::check_dma_buffer;
pub(crate) use h7::{
    CLOCK_LIMITS, SYSTEM_MEMORY, TS_CAL1, TS_CAL2, UID_BASE, VREFIN_CAL, map_system_memory,
};

// The die's errata (ES0392), mounted by each part of the die.
#[path = "shared/errata_es0392.rs"]
mod errata_table;
pub use errata_table::*;

// Timers another timer's trigger output can start.

timer_trigger!(TIM8: TIM1, 0);
timer_trigger!(TIM3: TIM1, 0);

adc_internal!(ADC3: 18, 19);

/// The independent watchdog's token and registers.
pub type WatchdogPeripheral = IWDG1;

pub(crate) fn iwdg_regs() -> pac::iwdg::Iwdg {
    pac::IWDG1
}

/// Freezes the watchdog while a debugger holds the core halted:
/// `DBGMCU_APB4FZR1.IWDG1` (bit 18; IWDG1 sits on APB4, not APB1).
pub(crate) fn set_watchdog_freeze() {
    pac::DBGMCU.apb4fzr1().modify(|w| w.set_iwdg1(true));
}

/// Whether that freeze is set.
pub(crate) fn watchdog_frozen() -> bool {
    pac::DBGMCU.apb4fzr1().read().iwdg1()
}

/// SYSCFG's clock enable, which EXTI's port selection (EXTICR) needs.
pub(crate) const SYSCFG_ENABLE: RccBit = RccBit::new(Bus::Apb4, 1);

// Every DMA1 and DMA2 stream reaches the GPIO ports (RM0433 figure 1).
gpio_dma!(
    1: [0, 1, 2, 3, 4, 5, 6, 7];
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

/// The die revision code, `DBGMCU_IDC.REV_ID` (bits 31:16) on RM0433.
pub(crate) fn revision() -> u16 {
    pac::DBGMCU.idc().read().rev_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uart::{Resource, Signal, Wiring};

    /// A receiver port, as an example board manifest names it.
    type Usart6Example = Wiring<
        USART6,
        Pin<'C', 6, Alternate<7>>,
        Pin<'C', 7, Alternate<7>>,
        Stream<1, 6>,
        Stream<1, 0>,
    >;

    /// An SPI bus, as an example board manifest names it.
    #[test]
    fn spi_wiring_describes_itself() {
        type Spi1Example = crate::spi::Wiring<
            SPI1,
            Pin<'A', 5, Alternate<5>>,
            Pin<'A', 6, Alternate<5>>,
            Pin<'D', 7, Alternate<5>>,
            Stream<1, 5>,
            Stream<1, 4>,
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
                    port: 'D',
                    pin: 7,
                    af: 5
                },
                // DMA1_STREAM5_CH38 and DMA1_STREAM4_CH37 in the manifest.
                Resource::Dma {
                    signal: Signal::Tx,
                    controller: 1,
                    stream: 5,
                    request: 38
                },
                Resource::Dma {
                    signal: Signal::Rx,
                    controller: 1,
                    stream: 4,
                    request: 37
                },
                Resource::Interrupt("DMA1_STREAM4"),
            ]
        );
    }

    /// SPI1 to SPI3 need PLL1Q; without it the bus can't be opened.
    #[test]
    fn spi123_needs_pll1q() {
        use crate::{
            rcc::{AhbPrescaler, ApbPrescaler, Config, Supply, Sysclk, VoltageScale},
            spi::{self, Spi},
            time::Hertz,
        };
        let imu = spi::Config::new(Hertz::mhz(24));
        // HSI at 64 MHz straight through, no PLL.
        let reset = Config {
            hse: None,
            pll1: None,
            sys: Sysclk::Hsi,
            cpu_pre: AhbPrescaler::Div1,
            ahb_pre: AhbPrescaler::Div1,
            apb1_pre: ApbPrescaler::Div1,
            apb2_pre: ApbPrescaler::Div1,
            apb3_pre: ApbPrescaler::Div1,
            apb4_pre: ApbPrescaler::Div1,
            supply: Supply::Ldo,
            voltage_scale: VoltageScale::Scale1,
        }
        .validate()
        .unwrap();
        assert_eq!(reset.pll1_q, None);
        assert_eq!(
            crate::spi::check::<SPI1>(&reset, &imu),
            Err(spi::ConfigError::KernelClock)
        );
        // SPI4 runs from PCLK2: 64 MHz at reset, so 16 MHz.
        assert_eq!(crate::spi::check::<SPI4>(&reset, &imu), Ok(Hertz::mhz(16)));
    }

    #[test]
    fn wiring_describes_itself() {
        assert_eq!(
            Usart6Example::RESOURCES,
            [
                Resource::Peripheral("USART6"),
                Resource::Pin {
                    signal: Signal::Tx,
                    port: 'C',
                    pin: 6,
                    af: 7
                },
                Resource::Pin {
                    signal: Signal::Rx,
                    port: 'C',
                    pin: 7,
                    af: 7
                },
                // The example manifest names these DMA1_STREAM6_CH72 and
                // DMA1_STREAM0_CH71.
                Resource::Dma {
                    signal: Signal::Tx,
                    controller: 1,
                    stream: 6,
                    request: 72
                },
                Resource::Dma {
                    signal: Signal::Rx,
                    controller: 1,
                    stream: 0,
                    request: 71
                },
                Resource::Interrupt("USART6"),
                Resource::Interrupt("DMA1_STREAM0"),
                Resource::Interrupt("DMA1_STREAM6"),
            ]
        );
    }

    /// Every UART and timer-lane route an example board manifest names is one this
    /// glue offers, on the pin and alternate function it names and with its
    /// DMAMUX request. That it compiles is half the check.
    #[test]
    fn example_manifest_routes_exist() {
        use crate::timer::{ChannelPin, ChannelTimer, CompareDma};

        type Af<const P: char, const N: u8, const F: u8> = Pin<P, N, Alternate<F>>;
        fn tx<U: crate::uart::Instance, P: TxPin<U>, D: TxDma<U>>() -> u8 {
            D::REQUEST
        }
        fn rx<U: crate::uart::Instance, P: RxPin<U>, D: RxDma<U>>() -> u8 {
            D::REQUEST
        }
        fn lane<T: ChannelTimer, const C: u8, P: ChannelPin<T, C>, D: CompareDma<T, C>>() -> u8 {
            D::REQUEST
        }

        assert_eq!(rx::<USART6, Af<'C', 7, 7>, Stream<1, 0>>(), 71);
        assert_eq!(tx::<USART6, Af<'C', 6, 7>, Stream<1, 6>>(), 72);
        assert_eq!(rx::<USART3, Af<'D', 9, 7>, Stream<1, 1>>(), 45);
        assert_eq!(tx::<USART3, Af<'D', 8, 7>, Stream<1, 3>>(), 46);
        assert_eq!(rx::<USART1, Af<'A', 10, 7>, Stream<2, 4>>(), 41);
        assert_eq!(rx::<UART8, Af<'E', 0, 8>, Stream<1, 2>>(), 81);
        assert_eq!(lane::<TIM3, 3, Af<'B', 0, 2>, Stream<2, 0>>(), 25);
        assert_eq!(lane::<TIM3, 4, Af<'B', 1, 2>, Stream<2, 1>>(), 26);
        assert_eq!(lane::<TIM5, 1, Af<'A', 0, 2>, Stream<2, 2>>(), 55);
        assert_eq!(lane::<TIM5, 2, Af<'A', 1, 2>, Stream<2, 3>>(), 56);
    }

    #[test]
    fn solve_matches_the_lucid_tree() {
        use crate::rcc::{
            AhbPrescaler, ApbPrescaler, ClockError, Config, Hse, HseMode, Pll, PllSource, Request,
            Supply, Sysclk, VoltageScale,
        };
        use crate::time::Hertz;
        // 8 MHz crystal, 400 MHz core, 100 MHz on Q for SPI1 to SPI3: the
        // tree the Lucid freezes from (8 / 1 * 100 = 800 wide VCO, P 2, Q 8,
        // core undivided, everything else at 2).
        assert_eq!(
            Config::solve(Request {
                hse: Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                },
                sysclk: Hertz::mhz(400),
                q: Some(Hertz::mhz(100)),
            }),
            Ok(Config {
                hse: Some(Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                }),
                pll1: Some(Pll {
                    source: PllSource::Hse,
                    prediv: 1,
                    mul: 100,
                    divp: Some(2),
                    divq: Some(8),
                    divr: None,
                }),
                sys: Sysclk::Pll1P,
                cpu_pre: AhbPrescaler::Div1,
                ahb_pre: AhbPrescaler::Div2,
                apb1_pre: ApbPrescaler::Div2,
                apb2_pre: ApbPrescaler::Div2,
                apb3_pre: ApbPrescaler::Div2,
                apb4_pre: ApbPrescaler::Div2,
                supply: Supply::Ldo,
                voltage_scale: VoltageScale::Scale1,
            })
        );
        // A request the lowest voltage supports runs at it.
        assert_eq!(
            Config::solve(Request {
                hse: Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                },
                sysclk: Hertz::mhz(200),
                q: None,
            })
            .unwrap()
            .voltage_scale,
            VoltageScale::Scale3
        );
        // A request past the fastest scale's core clock: refused.
        assert_eq!(
            Config::solve(Request {
                hse: Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                },
                sysclk: Hertz::mhz(480),
                q: None,
            }),
            Err(ClockError::Cpu)
        );
    }
}
