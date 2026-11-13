//! STM32H743VI glue: the chip facts the IP modules leave out.
//!
//! Every routing fact here (RCC bits, pin alternate functions, DMAMUX
//! request IDs) was read from stm32-metapac 21.0.0's metadata for this
//! chip, whose ports are cut to the package's bonded pins (`BONDED` below):
//! the Lucid fits the TFBGA100 (STM32H743VIH6), which DS12117 Table 8 shows
//! bonds the same GPIO set as the LQFP100. The supplies differ: the TFBGA100
//! has no VDDLDO supply (F4 is internally tied to VDD, ES0392 2.2.22) and no
//! VREF+ pin (double-bonded to VDDA, so VREFBUF stays disabled; DS12117
//! note 5). They are hand-written for now, in the trait shapes a generator
//! would emit later.
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
#[path = "shared/h7.rs"]
mod h7;

pub use h7::check_dma_buffer;
pub(crate) use h7::{
    CLOCK_LIMITS, SYSTEM_MEMORY, TS_CAL1, TS_CAL2, UID_BASE, VREFIN_CAL, map_system_memory,
};

tokens!(
    USART1,
    USART2,
    USART3,
    UART4,
    UART5,
    USART6,
    UART7,
    UART8,
    LPUART1,
    TIM1,
    TIM2,
    TIM3,
    TIM4,
    TIM5,
    TIM6,
    TIM7,
    TIM8,
    TIM12,
    TIM13,
    TIM14,
    TIM15,
    TIM16,
    TIM17,
    LPTIM1,
    LPTIM2,
    LPTIM3,
    LPTIM4,
    LPTIM5,
    HRTIM1,
    SPI1,
    SPI2,
    SPI3,
    SPI4,
    SPI6,
    I2C1,
    I2C2,
    I2C3,
    I2C4,
    SAI1,
    SAI2,
    SAI3,
    SAI4,
    SPDIFRX1,
    SWPMI1,
    DFSDM1,
    CEC,
    DAC1,
    CRS,
    OPAMP1,
    OPAMP2,
    MDIOS,
    FDCAN1,
    FDCAN2,
    FDCANRAM,
    ADC1,
    ADC2,
    ADC3,
    ADC12_COMMON,
    ADC3_COMMON,
    ETH,
    USB_OTG_HS,
    USB_OTG_FS,
    DCMI,
    RNG,
    SDMMC1,
    SDMMC2,
    DLYB_SDMMC1,
    DLYB_SDMMC2,
    LTDC,
    WWDG1,
    IWDG1,
    MDMA,
    DMA2D,
    JPEG,
    FMC,
    QUADSPI,
    DLYB_QUADSPI,
    EXTI,
    SYSCFG,
    COMP1,
    COMP2,
    VREFBUF,
    RTC,
    CRC,
    BDMA,
    DMAMUX2,
    HSEM,
    DBGMCU,
    UID,
);

peripherals!(
    "Not here: FLASH and PWR, which RCC's `freeze` owns, and DMAMUX1, whose channels belong to the DMA1 and DMA2 streams.";
    RCC: RCC,
    GPIOA: Port<'A'>,
    GPIOB: Port<'B'>,
    GPIOC: Port<'C'>,
    GPIOD: Port<'D'>,
    GPIOE: Port<'E'>,
    GPIOH: Port<'H'>,
    DMA1: Dma<1>,
    DMA2: Dma<2>,
    USART1: USART1,
    USART2: USART2,
    USART3: USART3,
    UART4: UART4,
    UART5: UART5,
    USART6: USART6,
    UART7: UART7,
    UART8: UART8,
    LPUART1: LPUART1,
    TIM1: TIM1,
    TIM2: TIM2,
    TIM3: TIM3,
    TIM4: TIM4,
    TIM5: TIM5,
    TIM6: TIM6,
    TIM7: TIM7,
    TIM8: TIM8,
    TIM12: TIM12,
    TIM13: TIM13,
    TIM14: TIM14,
    TIM15: TIM15,
    TIM16: TIM16,
    TIM17: TIM17,
    LPTIM1: LPTIM1,
    LPTIM2: LPTIM2,
    LPTIM3: LPTIM3,
    LPTIM4: LPTIM4,
    LPTIM5: LPTIM5,
    HRTIM1: HRTIM1,
    SPI1: SPI1,
    SPI2: SPI2,
    SPI3: SPI3,
    SPI4: SPI4,
    SPI6: SPI6,
    I2C1: I2C1,
    I2C2: I2C2,
    I2C3: I2C3,
    I2C4: I2C4,
    SAI1: SAI1,
    SAI2: SAI2,
    SAI3: SAI3,
    SAI4: SAI4,
    SPDIFRX1: SPDIFRX1,
    SWPMI1: SWPMI1,
    DFSDM1: DFSDM1,
    CEC: CEC,
    DAC1: DAC1,
    CRS: CRS,
    OPAMP1: OPAMP1,
    OPAMP2: OPAMP2,
    MDIOS: MDIOS,
    FDCAN1: FDCAN1,
    FDCAN2: FDCAN2,
    FDCANRAM: FDCANRAM,
    ADC1: ADC1,
    ADC2: ADC2,
    ADC3: ADC3,
    ADC12_COMMON: ADC12_COMMON,
    ADC3_COMMON: ADC3_COMMON,
    ETH: ETH,
    USB_OTG_HS: USB_OTG_HS,
    USB_OTG_FS: USB_OTG_FS,
    DCMI: DCMI,
    RNG: RNG,
    SDMMC1: SDMMC1,
    SDMMC2: SDMMC2,
    DLYB_SDMMC1: DLYB_SDMMC1,
    DLYB_SDMMC2: DLYB_SDMMC2,
    LTDC: LTDC,
    WWDG1: WWDG1,
    IWDG1: IWDG1,
    MDMA: MDMA,
    DMA2D: DMA2D,
    JPEG: JPEG,
    FMC: FMC,
    QUADSPI: QUADSPI,
    DLYB_QUADSPI: DLYB_QUADSPI,
    EXTI: EXTI,
    SYSCFG: SYSCFG,
    COMP1: COMP1,
    COMP2: COMP2,
    VREFBUF: VREFBUF,
    RTC: RTC,
    CRC: CRC,
    BDMA: BDMA,
    DMAMUX2: DMAMUX2,
    HSEM: HSEM,
    DBGMCU: DBGMCU,
    UID: UID,
);

// GPIO ports, with their AHB4ENR bits. Only the bonded ports are declared:
// both 100-pin packages bond A to E and PH0/PH1 (see `BONDED`); ports F to K
// exist as registers only.

macro_rules! ports {
    ($($letter:literal => $regs:ident, $bit:literal;)+) => {
        $(
            impl crate::Sealed for Port<$letter> {}

            impl crate::gpio::PortRegisters for Port<$letter> {
                fn regs() -> pac::gpio::Gpio {
                    pac::$regs
                }
            }

            impl PortInstance for Port<$letter> {
                const ENABLE: RccBit = RccBit::new(Bus::Ahb4, $bit);
            }
        )+
    };
}

ports!(
    'A' => GPIOA, 0;
    'B' => GPIOB, 1;
    'C' => GPIOC, 2;
    'D' => GPIOD, 3;
    'E' => GPIOE, 4;
    'H' => GPIOH, 7;
);
/// The 100-pin packages' bonded pins, as (port, pin mask): ports A to E and
/// PH0/PH1. DS12117 Table 8 gives the same GPIO set for LQFP100 and the
/// Lucid's TFBGA100 (STM32H743VIH6), so one table serves both.
/// `tools/check_glue.py` reads this: a routing row on any other pin
/// fails, and a bonded pin with no row is reported, not failed.
#[allow(dead_code)] // Read by tools/check_glue.py.
pub(crate) const BONDED: &[(char, u16)] = &[
    ('A', 0xffff),
    ('B', 0xffff),
    ('C', 0xffff),
    ('D', 0xffff),
    ('E', 0xffff),
    ('H', 0x0003), // PH0, PH1
];

// DMA1 and DMA2, with their AHB1ENR bits. DMAMUX1 channels 0 to 7 feed DMA1
// streams 0 to 7, channels 8 to 15 DMA2's. DMAMUX1 has no clock bit of its
// own; it runs with the controllers.

impl crate::Sealed for Dma<1> {}
impl crate::dma::DmaRegisters for Dma<1> {
    fn regs() -> pac::dma::Dma {
        pac::DMA1
    }
    fn mux() -> pac::dmamux::Dmamux {
        pac::DMAMUX1
    }
}
impl DmaInstance for Dma<1> {
    const ENABLE: RccBit = RccBit::new(Bus::Ahb1, 0);
    const MUX_BASE: u8 = 0;
}

impl crate::Sealed for Dma<2> {}
impl crate::dma::DmaRegisters for Dma<2> {
    fn regs() -> pac::dma::Dma {
        pac::DMA2
    }
    fn mux() -> pac::dmamux::Dmamux {
        pac::DMAMUX1
    }
}
impl DmaInstance for Dma<2> {
    const ENABLE: RccBit = RccBit::new(Bus::Ahb1, 1);
    const MUX_BASE: u8 = 8;
}

// U(S)ART instances. `freeze` sets each kernel clock to its bus clock:
// USART1 and USART6 run from PCLK2, the rest from PCLK1.

macro_rules! usarts {
    ($($name:ident => $info:ident, $bus:ident, $bit:literal;)+) => {
        $(
            static $info: Info = Info::new(LineRegs::new(pac::$name), RccBit::new(Bus::$bus, $bit));

            impl crate::uart::Instance for $name {
                const KERNEL: Bus = Bus::$bus;
                const NAME: &'static str = stringify!($name);
                const INTERRUPT: Interrupt = Interrupt::$name;
                const INTERRUPT_NAME: &'static str = stringify!($name);
                fn info() -> &'static Info {
                    &$info
                }
            }
        )+
    };
}

usarts!(
    USART1 => USART1_INFO, Apb2, 4;
    USART2 => USART2_INFO, Apb1L, 17;
    USART3 => USART3_INFO, Apb1L, 18;
    UART4 => UART4_INFO, Apb1L, 19;
    UART5 => UART5_INFO, Apb1L, 20;
    USART6 => USART6_INFO, Apb2, 5;
    UART7 => UART7_INFO, Apb1L, 30;
    UART8 => UART8_INFO, Apb1L, 31;
);

// Pins: (port, pin, alternate function).

pins!(TxPin<USART1>: ('A', 9, 7), ('B', 6, 7), ('B', 14, 4));
pins!(RxPin<USART1>: ('A', 10, 7), ('B', 7, 7), ('B', 15, 4));
pins!(TxPin<USART2>: ('A', 2, 7), ('D', 5, 7));
pins!(RxPin<USART2>: ('A', 3, 7), ('D', 6, 7));
pins!(TxPin<USART3>: ('B', 10, 7), ('C', 10, 7), ('D', 8, 7));
pins!(RxPin<USART3>: ('B', 11, 7), ('C', 11, 7), ('D', 9, 7));
pins!(TxPin<UART4>: ('A', 0, 8), ('A', 12, 6), ('B', 9, 8), ('C', 10, 8), ('D', 1, 8));
pins!(RxPin<UART4>: ('A', 1, 8), ('A', 11, 6), ('B', 8, 8), ('C', 11, 8), ('D', 0, 8));
pins!(TxPin<UART5>: ('B', 6, 14), ('B', 13, 14), ('C', 12, 8));
pins!(RxPin<UART5>: ('B', 5, 14), ('B', 12, 14), ('D', 2, 8));
pins!(TxPin<USART6>: ('C', 6, 7));
pins!(RxPin<USART6>: ('C', 7, 7));
pins!(TxPin<UART7>: ('A', 15, 11), ('B', 4, 11), ('E', 8, 7));
pins!(RxPin<UART7>: ('A', 8, 11), ('B', 3, 11), ('E', 7, 7));
pins!(TxPin<UART8>: ('E', 1, 8));
pins!(RxPin<UART8>: ('E', 0, 8));

// DMA: through DMAMUX1 any DMA1 or DMA2 stream serves any request, so each
// instance pairs with every stream; the type still fixes the request ID.

macro_rules! dma {
    ($rx_trait:ident, $tx_trait:ident; $($usart:ident: rx $rx:literal, tx $tx:literal;)+) => {
        $(
            // SAFETY: the DMAMUX1 request IDs for this instance, from
            // stm32-metapac 21's metadata (RM0433 table 121).
            unsafe impl<const D: u8, const N: u8> $rx_trait<$usart> for Stream<D, N>
            where
                Dma<D>: DmaInstance,
                Stream<D, N>: crate::dma::StreamInterrupt,
            {
                const REQUEST: u8 = $rx;
            }

            // SAFETY: as above.
            unsafe impl<const D: u8, const N: u8> $tx_trait<$usart> for Stream<D, N>
            where
                Dma<D>: DmaInstance,
                Stream<D, N>: crate::dma::StreamInterrupt,
            {
                const REQUEST: u8 = $tx;
            }
        )+
    };
}

dma!(
    RxDma, TxDma;
    USART1: rx 41, tx 42;
    USART2: rx 43, tx 44;
    USART3: rx 45, tx 46;
    UART4: rx 63, tx 64;
    UART5: rx 65, tx 66;
    USART6: rx 71, tx 72;
    UART7: rx 79, tx 80;
    UART8: rx 81, tx 82;
);

// Timers another timer's trigger output can start.

timer_trigger!(TIM8: TIM1, 0);
timer_trigger!(TIM3: TIM1, 0);

// SDMMC1, on its only pins. SDMMC2 isn't offered.

sdmmcs!(
    SDMMC1 => Ahb3, 16;
);
pins!(CkPin<SDMMC1>: ('C', 12, 12));
pins!(CmdPin<SDMMC1>: ('D', 2, 12));
pins!(D0Pin<SDMMC1>: ('C', 8, 12));
pins!(D1Pin<SDMMC1>: ('C', 9, 12));
pins!(D2Pin<SDMMC1>: ('C', 10, 12));
pins!(D3Pin<SDMMC1>: ('C', 11, 12));

// The ADCs: ADC1 for the pins, ADC3 with the internal channels too. ADC2,
// sharing ADC1's clock, isn't offered, nor the _C pads (PC2_C, PC3_C).

adcs!(
    ADC1 => ADC12_COMMON, Ahb1, 5, false;
    ADC3 => ADC3_COMMON, Ahb4, 24, true;
);
adc_internal!(ADC3: 18, 19);
adc_pins!(ADC1: ('A', 0, 16), ('A', 1, 17), ('A', 2, 14), ('A', 3, 15), ('A', 4, 18), ('A', 5, 19), ('A', 6, 3), ('A', 7, 7),
    ('B', 0, 9), ('B', 1, 5), ('C', 0, 10), ('C', 1, 11), ('C', 4, 4), ('C', 5, 8));
adc_pins!(ADC3: ('C', 0, 10), ('C', 1, 11));
adc_dma_mux!(ADC1: mux 9);
adc_dma_mux!(ADC3: mux 115);

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

// SPI instances: (bus, enable bit, kernel clock). `freeze` sets SPI1 to SPI3
// to PLL1Q and SPI4 and SPI5 to PCLK2. SPI6 (D3, on the BDMA) is left out.

spis!(
    SPI1 => SPI1_INFO, Apb2, 12, Pll1Q;
    SPI2 => SPI2_INFO, Apb1L, 14, Pll1Q;
    SPI3 => SPI3_INFO, Apb1L, 15, Pll1Q;
    SPI4 => SPI4_INFO, Apb2, 13, Apb2;
);

pins!(SckPin<SPI1>: ('A', 5, 5), ('B', 3, 5));
pins!(MisoPin<SPI1>: ('A', 6, 5), ('B', 4, 5));
pins!(MosiPin<SPI1>: ('A', 7, 5), ('B', 5, 5), ('D', 7, 5));
pins!(SckPin<SPI2>: ('A', 9, 5), ('A', 12, 5), ('B', 10, 5), ('B', 13, 5), ('D', 3, 5));
pins!(MisoPin<SPI2>: ('B', 14, 5), ('C', 2, 5));
pins!(MosiPin<SPI2>: ('B', 15, 5), ('C', 1, 5), ('C', 3, 5));
pins!(SckPin<SPI3>: ('B', 3, 6), ('C', 10, 6));
pins!(MisoPin<SPI3>: ('B', 4, 6), ('C', 11, 6));
pins!(MosiPin<SPI3>: ('B', 2, 7), ('B', 5, 7), ('C', 12, 6), ('D', 6, 5));
pins!(SckPin<SPI4>: ('E', 2, 5), ('E', 12, 5));
pins!(MisoPin<SPI4>: ('E', 5, 5), ('E', 13, 5));
pins!(MosiPin<SPI4>: ('E', 6, 5), ('E', 14, 5));

dma!(
    SpiRxDma, SpiTxDma;
    SPI1: rx 37, tx 38;
    SPI2: rx 39, tx 40;
    SPI3: rx 61, tx 62;
    SPI4: rx 83, tx 84;
);

// I2C instances: (bus, enable bit). The kernel clock is the bus clock,
// which `freeze` writes the I2C1235 and I2C4 muxes to select. The SMBus
// SMBA pins (PB5, PB12, PA9, PB9's AF11, PD11) are not routed: the
// driver carries I2C alone.

i2cs!(
    I2C1 => I2C1_INFO, Apb1L, 21;
    I2C2 => I2C2_INFO, Apb1L, 22;
    I2C3 => I2C3_INFO, Apb1L, 23;
    I2C4 => I2C4_INFO, Apb4, 7;
);

pins!(SclPin<I2C1>: ('B', 6, 4), ('B', 8, 4));
pins!(SdaPin<I2C1>: ('B', 7, 4), ('B', 9, 4));
pins!(SclPin<I2C2>: ('B', 10, 4));
pins!(SdaPin<I2C2>: ('B', 11, 4));
pins!(SclPin<I2C3>: ('A', 8, 4));
pins!(SdaPin<I2C3>: ('C', 9, 4));
pins!(SclPin<I2C4>: ('B', 6, 6), ('B', 8, 6), ('D', 12, 4));
pins!(SdaPin<I2C4>: ('B', 7, 6), ('B', 9, 6), ('D', 13, 4));

// Timers: (bus, enable bit, update vector, counter bits). Each counts at its
// bus's timer clock.

timers!(
    TIM1 => Apb2, 0, TIM1_UP, 16;
    TIM2 => Apb1L, 0, TIM2, 32;
    TIM3 => Apb1L, 1, TIM3, 16;
    TIM4 => Apb1L, 2, TIM4, 16;
    TIM5 => Apb1L, 3, TIM5, 32;
    TIM6 => Apb1L, 4, TIM6_DAC, 16;
    TIM7 => Apb1L, 5, TIM7, 16;
    TIM8 => Apb2, 1, TIM8_UP_TIM13, 16;
    TIM12 => Apb1L, 6, TIM8_BRK_TIM12, 16;
    TIM13 => Apb1L, 7, TIM8_UP_TIM13, 16;
    TIM14 => Apb1L, 8, TIM8_TRG_COM_TIM14, 16;
    TIM15 => Apb2, 16, TIM15, 16;
    TIM16 => Apb2, 17, TIM16, 16;
    TIM17 => Apb2, 18, TIM17, 16;
);

// Timer channels: (channels, plain or break unit), their pins, and their
// compare (CHx) and update (UP) DMA requests.

channel_timers!(
    TIM1 => 4, break;
    TIM2 => 4, plain;
    TIM3 => 4, plain;
    TIM4 => 4, plain;
    TIM5 => 4, plain;
    TIM8 => 4, break;
    TIM12 => 2, plain;
    TIM13 => 1, plain;
    TIM14 => 1, plain;
    TIM15 => 2, break;
    TIM16 => 1, break;
    TIM17 => 1, break;
);

timer_pins!(TIM1 CH1: ('A', 8, 1), ('E', 9, 1));
timer_pins!(TIM1 CH1N: ('A', 7, 1), ('B', 13, 1), ('E', 8, 1));
timer_pins!(TIM1 CH2: ('A', 9, 1), ('E', 11, 1));
timer_pins!(TIM1 CH2N: ('B', 0, 1), ('B', 14, 1), ('E', 10, 1));
timer_pins!(TIM1 CH3: ('A', 10, 1), ('E', 13, 1));
timer_pins!(TIM1 CH3N: ('B', 1, 1), ('B', 15, 1), ('E', 12, 1));
timer_pins!(TIM1 CH4: ('A', 11, 1), ('E', 14, 1));
timer_pins!(TIM2 CH1: ('A', 0, 1), ('A', 5, 1), ('A', 15, 1));
timer_pins!(TIM2 CH2: ('A', 1, 1), ('B', 3, 1));
timer_pins!(TIM2 CH3: ('A', 2, 1), ('B', 10, 1));
timer_pins!(TIM2 CH4: ('A', 3, 1), ('B', 11, 1));
timer_pins!(TIM3 CH1: ('A', 6, 2), ('B', 4, 2), ('C', 6, 2));
timer_pins!(TIM3 CH2: ('A', 7, 2), ('B', 5, 2), ('C', 7, 2));
timer_pins!(TIM3 CH3: ('B', 0, 2), ('C', 8, 2));
timer_pins!(TIM3 CH4: ('B', 1, 2), ('C', 9, 2));
timer_pins!(TIM4 CH1: ('B', 6, 2), ('D', 12, 2));
timer_pins!(TIM4 CH2: ('B', 7, 2), ('D', 13, 2));
timer_pins!(TIM4 CH3: ('B', 8, 2), ('D', 14, 2));
timer_pins!(TIM4 CH4: ('B', 9, 2), ('D', 15, 2));
timer_pins!(TIM5 CH1: ('A', 0, 2));
timer_pins!(TIM5 CH2: ('A', 1, 2));
timer_pins!(TIM5 CH3: ('A', 2, 2));
timer_pins!(TIM5 CH4: ('A', 3, 2));
timer_pins!(TIM8 CH1: ('C', 6, 3));
timer_pins!(TIM8 CH1N: ('A', 5, 3), ('A', 7, 3));
timer_pins!(TIM8 CH2: ('C', 7, 3));
timer_pins!(TIM8 CH2N: ('B', 0, 3), ('B', 14, 3));
timer_pins!(TIM8 CH3: ('C', 8, 3));
timer_pins!(TIM8 CH3N: ('B', 1, 3), ('B', 15, 3));
timer_pins!(TIM8 CH4: ('C', 9, 3));
timer_pins!(TIM12 CH1: ('B', 14, 2));
timer_pins!(TIM12 CH2: ('B', 15, 2));
timer_pins!(TIM13 CH1: ('A', 6, 9));
timer_pins!(TIM14 CH1: ('A', 7, 9));
timer_pins!(TIM15 CH1: ('A', 2, 4), ('E', 5, 4));
timer_pins!(TIM15 CH1N: ('A', 1, 4), ('E', 4, 4));
timer_pins!(TIM15 CH2: ('A', 3, 4), ('E', 6, 4));
timer_pins!(TIM16 CH1: ('B', 8, 1));
timer_pins!(TIM16 CH1N: ('B', 6, 1));
timer_pins!(TIM17 CH1: ('B', 9, 1));
timer_pins!(TIM17 CH1N: ('B', 7, 1));

timer_dma!(TIM1 CH1: mux 11);
timer_dma!(TIM1 CH2: mux 12);
timer_dma!(TIM1 CH3: mux 13);
timer_dma!(TIM1 CH4: mux 14);
timer_dma!(TIM1 UP: mux 15);
timer_dma!(TIM2 CH1: mux 18);
timer_dma!(TIM2 CH2: mux 19);
timer_dma!(TIM2 CH3: mux 20);
timer_dma!(TIM2 CH4: mux 21);
timer_dma!(TIM2 UP: mux 22);
timer_dma!(TIM3 CH1: mux 23);
timer_dma!(TIM3 CH2: mux 24);
timer_dma!(TIM3 CH3: mux 25);
timer_dma!(TIM3 CH4: mux 26);
timer_dma!(TIM3 UP: mux 27);
timer_dma!(TIM4 CH1: mux 29);
timer_dma!(TIM4 CH2: mux 30);
timer_dma!(TIM4 CH3: mux 31);
timer_dma!(TIM4 UP: mux 32);
timer_dma!(TIM5 CH1: mux 55);
timer_dma!(TIM5 CH2: mux 56);
timer_dma!(TIM5 CH3: mux 57);
timer_dma!(TIM5 CH4: mux 58);
timer_dma!(TIM5 UP: mux 59);
timer_dma!(TIM8 CH1: mux 47);
timer_dma!(TIM8 CH2: mux 48);
timer_dma!(TIM8 CH3: mux 49);
timer_dma!(TIM8 CH4: mux 50);
timer_dma!(TIM8 UP: mux 51);
timer_dma!(TIM15 CH1: mux 105);
timer_dma!(TIM15 UP: mux 106);
timer_dma!(TIM16 CH1: mux 109);
timer_dma!(TIM16 UP: mux 110);
timer_dma!(TIM17 CH1: mux 111);
timer_dma!(TIM17 UP: mux 112);

// Every DMA1 and DMA2 stream reaches the GPIO ports (RM0433 figure 1).
gpio_dma!(
    1: [0, 1, 2, 3, 4, 5, 6, 7];
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

// The full-speed USB port.

usb_fs!(
    USB_OTG_FS => Ahb1, 27, OTG_FS, 1024, 9, hs_core;
);

// The vector each DMA stream raises.

stream_interrupts!(
    1: [0 => DMA1_STREAM0, 1 => DMA1_STREAM1, 2 => DMA1_STREAM2, 3 => DMA1_STREAM3,
        4 => DMA1_STREAM4, 5 => DMA1_STREAM5, 6 => DMA1_STREAM6, 7 => DMA1_STREAM7];
    2: [0 => DMA2_STREAM0, 1 => DMA2_STREAM1, 2 => DMA2_STREAM2, 3 => DMA2_STREAM3,
        4 => DMA2_STREAM4, 5 => DMA2_STREAM5, 6 => DMA2_STREAM6, 7 => DMA2_STREAM7];
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
