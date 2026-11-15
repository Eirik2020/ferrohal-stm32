//! STM32F401RE glue: the chip facts the IP modules leave out.
//!
//! The routing facts (RCC bits, vectors, pin alternate functions, DMA
//! request channels) are generated from stm32-metapac's metadata into
//! `gen/stm32f401re.rs` by `tools/glue-gen`; this file holds the facts metapac
//! does not carry, each citing its manual.
//!
//! Requirements: GLUE-001, GLUE-002

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

// The die's errata (ES0299), and the prefetch erratum `freeze` checks: none
// on this die.
#[path = "shared/errata_es0299.rs"]
mod errata_table;
pub use errata_table::*;
pub(crate) const PREFETCH_ERRATUM: Option<&crate::errata::Erratum> = None;

// The routing rows, generated from stm32-metapac's metadata.
#[path = "gen/stm32f401re.rs"]
mod generated;
pub use generated::*;

/// The chip's 96 KiB of SRAM (RM0368 section 2.3), all of it on the DMA's
/// bus matrix.
const DMA_RAM: [(usize, usize); 1] = [(0x2000_0000, 96 * 1024)];

/// Whether `buffer` can be handed to a DMA stream: whether it lies in SRAM.
/// A Cortex-M4 has no data cache, so that is all. `Uart::new` checks every
/// buffer with this.
pub fn check_dma_buffer(buffer: &[u8]) -> Result<(), crate::dma::BufferError> {
    if macros::within(buffer, &DMA_RAM) {
        Ok(())
    } else {
        Err(crate::dma::BufferError::Unreachable)
    }
}

/// DS10086 and RM0368, at 2.7 V to 3.6 V, in voltage scale 2, which covers
/// 84 MHz on this chip and is VOS's reset value. `freeze` writes it rather
/// than trust that. RM0368's VOS is two bits (15:14): 01 is scale 3, 10
/// scale 2, and 00 and 11 are reserved. 11 is metapac's `SCALE1`, which
/// this chip does not have.
pub(crate) const CLOCK_LIMITS: Limits = Limits {
    sysclk_max: Hertz::mhz(84),
    hclk_max: Hertz::mhz(84),
    pclk1_max: Hertz::mhz(42),
    pclk2_max: Hertz::mhz(84),
    hse_oscillator: (Hertz::mhz(4), Hertz::mhz(26)),
    hse_bypass: (Hertz::mhz(1), Hertz::mhz(50)),
    pll_input: (Hertz::khz(950), Hertz::khz(2_100)),
    pll_vco: (Hertz::mhz(192), Hertz::mhz(432)),
    flash_step_2v7: Hertz::mhz(30),
    max_wait_states: 2,
    vos: pac::pwr::vals::Vos::SCALE2,
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

timer_trigger!(TIM3: TIM1, 0);
timer_trigger!(TIM2: TIM3, 2);

adc_internal!(ADC1: 18, 17);

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
// alone (RM0368 figure 1).
gpio_dma!(
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

/// The die revision code, `DBGMCU_IDCODE.REV_ID` (bits 31:16) on RM0368.
pub(crate) fn revision() -> u16 {
    pac::DBGMCU.idcode().read().rev_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adc::AdcWiring;
    use crate::gpio_v2::Analog;
    use crate::rcc::{
        AhbPrescaler, ApbPrescaler, ClockError, Config, Hse, HseMode, Pll, PllP, PllSource,
        Request, Supply, Sysclk,
    };
    use crate::time::Hertz;
    use crate::timer::{ComplementaryLaneWiring, LaneWiring};
    use crate::uart::{Resource, RxWiring, Signal, Wiring};

    type Usart1Bench = Wiring<
        USART1,
        Pin<'A', 9, Alternate<7>>,
        Pin<'A', 10, Alternate<7>>,
        Stream<2, 7>,
        Stream<2, 2>,
    >;

    #[test]
    fn wiring_describes_itself() {
        assert_eq!(
            Usart1Bench::RESOURCES,
            [
                Resource::Peripheral("USART1"),
                Resource::Pin {
                    signal: Signal::Tx,
                    port: 'A',
                    pin: 9,
                    af: 7
                },
                Resource::Pin {
                    signal: Signal::Rx,
                    port: 'A',
                    pin: 10,
                    af: 7
                },
                Resource::Dma {
                    signal: Signal::Tx,
                    controller: 2,
                    stream: 7,
                    request: 4
                },
                Resource::Dma {
                    signal: Signal::Rx,
                    controller: 2,
                    stream: 2,
                    request: 4
                },
                Resource::Interrupt("USART1"),
                Resource::Interrupt("DMA2_STREAM2"),
                Resource::Interrupt("DMA2_STREAM7"),
            ]
        );
        assert_eq!(
            RxWiring::<USART6, Pin<'A', 12, Alternate<8>>, Stream<2, 1>>::RESOURCES[2],
            Resource::Dma {
                signal: Signal::Rx,
                controller: 2,
                stream: 1,
                request: 5
            }
        );
        // The SpeedyBee's motor lanes: TIM3 CH3 on PB0, and a complementary
        // lane, TIM1 CH1N on PA7.
        assert_eq!(
            LaneWiring::<TIM3, 3, Pin<'B', 0, Alternate<2>>, Stream<1, 7>>::RESOURCES,
            [
                Resource::Peripheral("TIM3"),
                Resource::Pin {
                    signal: Signal::Channel(3),
                    port: 'B',
                    pin: 0,
                    af: 2
                },
                Resource::Dma {
                    signal: Signal::Channel(3),
                    controller: 1,
                    stream: 7,
                    request: 5
                },
            ]
        );
        assert_eq!(
            ComplementaryLaneWiring::<TIM1, 1, Pin<'A', 7, Alternate<1>>, Stream<2, 1>>::RESOURCES,
            [
                Resource::Peripheral("TIM1"),
                Resource::Pin {
                    signal: Signal::Complementary(1),
                    port: 'A',
                    pin: 7,
                    af: 1
                },
                Resource::Dma {
                    signal: Signal::Complementary(1),
                    controller: 2,
                    stream: 1,
                    request: 6
                },
            ]
        );
        // The bench's own ADC input: ADC1 channel 6 on PA6, over stream 0.
        assert_eq!(
            AdcWiring::<ADC1, Pin<'A', 6, Analog>, Stream<2, 0>>::RESOURCES,
            [
                Resource::Peripheral("ADC1"),
                Resource::Analog {
                    signal: Signal::Adc(6),
                    port: 'A',
                    pin: 6,
                },
                Resource::Dma {
                    signal: Signal::Adc(6),
                    controller: 2,
                    stream: 0,
                    request: 0
                },
            ]
        );
    }

    #[test]
    fn solve_matches_the_proven_tree() {
        // 8 MHz crystal, 84 MHz system, 48 MHz USB: the tree the benches
        // freeze from (8 / 8 * 336, P 4, Q 7).
        assert_eq!(
            Config::solve(Request {
                hse: Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                },
                sysclk: Hertz::mhz(84),
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
                    divp: PllP::Div4,
                    divq: Some(7),
                }),
                sys: Sysclk::PllP,
                ahb_pre: AhbPrescaler::Div1,
                apb1_pre: ApbPrescaler::Div2,
                apb2_pre: ApbPrescaler::Div1,
                supply: Supply::V2_7To3_6,
            })
        );
        // Without a USB request, Q is left unset.
        assert_eq!(
            Config::solve(Request {
                hse: Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                },
                sysclk: Hertz::mhz(84),
                usb: None,
            })
            .unwrap()
            .pll
            .unwrap()
            .divq,
            None
        );
        // A USB clock the PLL cannot give exactly: no solution.
        assert_eq!(
            Config::solve(Request {
                hse: Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                },
                sysclk: Hertz::mhz(84),
                usb: Some(Hertz::mhz(47)),
            }),
            Err(ClockError::Unsatisfiable)
        );
        // Past the chip's system clock: the request itself is refused.
        assert_eq!(
            Config::solve(Request {
                hse: Hse {
                    freq: Hertz::mhz(8),
                    mode: HseMode::Oscillator,
                },
                sysclk: Hertz::mhz(120),
                usb: Some(Hertz::mhz(48)),
            }),
            Err(ClockError::Sysclk)
        );
    }
}
