//! STM32F446 glue: the chip facts the IP modules leave out, for every package
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
/// bidirectional endpoints (RM0390 Table 221).
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = false;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 320;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 6;

// The die's errata (ES0298), and the prefetch erratum `freeze` checks: none
// on this die.
#[path = "errata_es0298.rs"]
mod errata_table;
pub use errata_table::*;
pub(crate) const PREFETCH_ERRATUM: Option<&crate::errata::Erratum> = None;

// The routing rows, generated from stm32-metapac's metadata for the
// part's device and package; a flash-size variant takes its package's.
#[cfg(any(feature = "stm32f446me", feature = "stm32f446mc"))]
#[path = "../gen/stm32f446me.rs"]
mod generated;
#[cfg(any(feature = "stm32f446re", feature = "stm32f446rc"))]
#[path = "../gen/stm32f446re.rs"]
mod generated;
#[cfg(any(feature = "stm32f446ve", feature = "stm32f446vc"))]
#[path = "../gen/stm32f446ve.rs"]
mod generated;
#[cfg(any(feature = "stm32f446ze", feature = "stm32f446zc"))]
#[path = "../gen/stm32f446ze.rs"]
mod generated;
pub use generated::*;

/// The chip's 128 KiB of SRAM: SRAM1's 112 KiB and SRAM2's 16 KiB, contiguous
/// from 0x2000_0000 (RM0390 2.2.2 and 2.2.3), all of it on the DMA's bus
/// matrix (RM0390 Figure 1).
const DMA_RAM: [(usize, usize); 1] = [(0x2000_0000, 128 * 1024)];

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

/// DS10693 and RM0390, at 2.7 V to 3.6 V, in voltage scale 1, which 180 MHz
/// needs on this chip (DS10693 Table 16; RM0390 5.1.3). `freeze` writes it
/// although VOS resets to scale 1 here (RM0390 5.4.1): one path for every F4.
/// HCLK up to 180 MHz with the over-drive on and 168 MHz without it; APB1 up
/// to 45 and APB2 to 90 MHz with it, 42 and 84 without (DS10693 Table 16);
/// the HSE crystal 4 to 26 MHz (Table 39), an external clock 1 to 50 MHz
/// (Table 37); the PLL input 0.95 to 2.1 MHz and its VCO 100 to 432 MHz
/// (Table 43); one wait state per 30 MHz up to 5 at 180 MHz (RM0390
/// Table 5). The over-drive's entry and exit are RM0390 5.1.3's.
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
        erratum: None,
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

// Timers another timer's trigger output can start (RM0390 Table 112).

timer_trigger!(TIM3: TIM1, 0);
timer_trigger!(TIM2: TIM3, 2);

// The temperature sensor on ADC1_IN18, VREFINT on IN17 (RM0390 13.10).
adc_internal!(ADC1: 18, 17);

/// The factory calibration values (DS10693 Tables 81 and 84).
pub(crate) const VREFIN_CAL: *const u16 = 0x1FFF_7A2A as *const u16;
pub(crate) const TS_CAL1: *const u16 = 0x1FFF_7A2C as *const u16;
pub(crate) const TS_CAL2: *const u16 = 0x1FFF_7A2E as *const u16;

/// The unique ID's block (RM0390 34.1).
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
// alone (RM0390 Figure 1).
gpio_dma!(
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

/// The die revision code, `DBGMCU_IDCODE.REV_ID` (bits 31:16) on RM0390.
pub(crate) fn revision() -> u16 {
    pac::DBGMCU.idcode().read().rev_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adc;
    use crate::gpio_v2::Analog;
    use crate::rcc::{ClockError, Config, Hse, HseMode, PllP, Request, Supply};
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
            RxWiring::<USART6, Pin<'C', 7, Alternate<8>>, Stream<2, 1>>::RESOURCES[2],
            Resource::Dma {
                signal: Signal::Rx,
                controller: 2,
                stream: 1,
                request: 5
            }
        );
        // Two compare lanes: TIM3 CH3 on PB0, and a complementary lane,
        // TIM1 CH1N on PA7.
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
            adc::Wiring::<ADC1, Pin<'A', 6, Analog>, Stream<2, 0>>::RESOURCES,
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
    fn solve_reaches_180_mhz_with_the_overdrive() {
        let request = |sysclk, usb| Request {
            hse: Hse {
                freq: Hertz::mhz(8),
                mode: HseMode::Oscillator,
            },
            sysclk,
            usb,
        };
        // 168 MHz and USB at 48 MHz: a 336 MHz VCO, P 2, Q 7, no over-drive.
        let config = Config::solve(request(Hertz::mhz(168), Some(Hertz::mhz(48)))).unwrap();
        assert_eq!(
            config.pll.map(|pll| (pll.divp, pll.divq)),
            Some((PllP::Div2, Some(7)))
        );
        assert!(!config.validate().unwrap().overdrive);
        // 180 MHz: the part's ceiling, over-drive on, APB at 45 and 90 MHz.
        let config = Config::solve(request(Hertz::mhz(180), None)).unwrap();
        let clocks = config.validate().unwrap();
        assert_eq!(clocks.flash_wait_states, 5);
        assert!(clocks.overdrive);
        assert!(clocks.pclk1 <= Hertz::mhz(45) && clocks.pclk2 <= Hertz::mhz(90));
        // Past the chip's system clock: the request itself is refused.
        assert_eq!(
            Config::solve(request(Hertz::mhz(200), None)),
            Err(ClockError::Sysclk)
        );
    }
}
