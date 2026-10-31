//! STM32H753ZI glue: the chip facts the IP modules leave out.
//!
//! Every routing fact here (RCC bits, pin alternate functions, DMAMUX
//! request IDs) was read from stm32-metapac 21.0.0's metadata for this
//! chip. They are hand-written for now, in the trait shapes a generator
//! would emit later. Clock limits and the memory map, shared with the H743,
//! are in `shared/h7.rs`.

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma_v1::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    rcc_h7rm0433::{Bus, RCC, RccBit},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    usart_v4::{self, Info, Line, RxDma, RxPin, TxDma, TxPin},
};

#[path = "shared/macros.rs"]
#[macro_use]
mod macros;
#[path = "shared/h7.rs"]
mod h7;

pub use h7::check_dma_buffer;
pub(crate) use h7::CLOCK_LIMITS;

tokens!(
    USART1, USART2, USART3, UART4, UART5, USART6, UART7, UART8, LPUART1, TIM1, TIM2, TIM3, TIM4,
    TIM5, TIM6, TIM7, TIM8, TIM12, TIM13, TIM14, TIM15, TIM16, TIM17, LPTIM1, LPTIM2, LPTIM3,
    LPTIM4, LPTIM5, HRTIM1, SPI1, SPI2, SPI3, SPI4, SPI5, SPI6, I2C1, I2C2, I2C3, I2C4, SAI1, SAI2,
    SAI3, SAI4, SPDIFRX1, CEC, DAC1, CRS, OPAMP1, OPAMP2, MDIOS, FDCAN1, FDCAN2, FDCANRAM, ADC1,
    ADC2, ADC3, ADC12_COMMON, ADC3_COMMON, ETH, USB_OTG_HS, USB_OTG_FS, DCMI, CRYP, HASH, RNG,
    SDMMC1, SDMMC2, DLYB_SDMMC1, DLYB_SDMMC2, LTDC, WWDG1, IWDG1, MDMA, DMA2D, JPEG, FMC, QUADSPI,
    DLYB_QUADSPI, EXTI, SYSCFG, COMP1, COMP2, VREFBUF, RTC, CRC, BDMA, DMAMUX2, HSEM, DBGMCU, UID,
);

peripherals!(
    "Not here: FLASH and PWR, which RCC's `freeze` owns, and DMAMUX1, whose channels belong to the DMA1 and DMA2 streams.";
    RCC: RCC,
    GPIOA: Port<'A'>,
    GPIOB: Port<'B'>,
    GPIOC: Port<'C'>,
    GPIOD: Port<'D'>,
    GPIOE: Port<'E'>,
    GPIOF: Port<'F'>,
    GPIOG: Port<'G'>,
    GPIOH: Port<'H'>,
    GPIOI: Port<'I'>,
    GPIOJ: Port<'J'>,
    GPIOK: Port<'K'>,
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
    SPI5: SPI5,
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
    CRYP: CRYP,
    HASH: HASH,
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

// GPIO ports, with their AHB4ENR bits. The LQFP144 package bonds out ports
// A to G and PH0, PH1; ports I to K exist as registers only.

macro_rules! ports {
    ($($letter:literal => $regs:ident, $bit:literal;)+) => {
        $(
            impl crate::Sealed for Port<$letter> {}

            impl PortInstance for Port<$letter> {
                fn regs() -> pac::gpio::Gpio {
                    pac::$regs
                }
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
    'F' => GPIOF, 5;
    'G' => GPIOG, 6;
    'H' => GPIOH, 7;
    'I' => GPIOI, 8;
    'J' => GPIOJ, 9;
    'K' => GPIOK, 10;
);

// DMA1 and DMA2, with their AHB1ENR bits. DMAMUX1 channels 0 to 7 feed DMA1
// streams 0 to 7, channels 8 to 15 DMA2's. DMAMUX1 has no clock bit of its
// own; it runs with the controllers.

impl crate::Sealed for Dma<1> {}
impl DmaInstance for Dma<1> {
    fn regs() -> pac::dma::Dma {
        pac::DMA1
    }
    const ENABLE: RccBit = RccBit::new(Bus::Ahb1, 0);
    fn mux() -> pac::dmamux::Dmamux {
        pac::DMAMUX1
    }
    const MUX_BASE: u8 = 0;
}

impl crate::Sealed for Dma<2> {}
impl DmaInstance for Dma<2> {
    fn regs() -> pac::dma::Dma {
        pac::DMA2
    }
    const ENABLE: RccBit = RccBit::new(Bus::Ahb1, 1);
    fn mux() -> pac::dmamux::Dmamux {
        pac::DMAMUX1
    }
    const MUX_BASE: u8 = 8;
}

// U(S)ART instances. `freeze` sets each kernel clock to its bus clock:
// USART1 and USART6 run from PCLK2, the rest from PCLK1.

macro_rules! usarts {
    ($($name:ident => $info:ident, $bus:ident, $bit:literal;)+) => {
        $(
            static $info: Info = Info::new(Line::new(pac::$name), RccBit::new(Bus::$bus, $bit));

            impl usart_v4::Instance for $name {
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
pins!(TxPin<USART6>: ('C', 6, 7), ('G', 14, 7));
pins!(RxPin<USART6>: ('C', 7, 7), ('G', 9, 7));
pins!(TxPin<UART7>: ('A', 15, 11), ('B', 4, 11), ('E', 8, 7), ('F', 7, 7));
pins!(RxPin<UART7>: ('A', 8, 11), ('B', 3, 11), ('E', 7, 7), ('F', 6, 7));
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

/// SYSCFG's clock enable, which EXTI's port selection (EXTICR) needs.
pub(crate) const SYSCFG_ENABLE: RccBit = RccBit::new(Bus::Apb4, 1);

// SPI instances: (bus, enable bit, kernel clock). `freeze` sets SPI1 to SPI3
// to PLL1Q and SPI4 and SPI5 to PCLK2. SPI6 (D3, on the BDMA) is left out.

spis!(
    SPI1 => SPI1_INFO, Apb2, 12, Pll1Q;
    SPI2 => SPI2_INFO, Apb1L, 14, Pll1Q;
    SPI3 => SPI3_INFO, Apb1L, 15, Pll1Q;
    SPI4 => SPI4_INFO, Apb2, 13, Apb2;
    SPI5 => SPI5_INFO, Apb2, 20, Apb2;
);

pins!(SckPin<SPI1>: ('A', 5, 5), ('B', 3, 5), ('G', 11, 5));
pins!(MisoPin<SPI1>: ('A', 6, 5), ('B', 4, 5), ('G', 9, 5));
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
pins!(SckPin<SPI5>: ('F', 7, 5));
pins!(MisoPin<SPI5>: ('F', 8, 5));
pins!(MosiPin<SPI5>: ('F', 9, 5), ('F', 11, 5));

dma!(
    SpiRxDma, SpiTxDma;
    SPI1: rx 37, tx 38;
    SPI2: rx 39, tx 40;
    SPI3: rx 61, tx 62;
    SPI4: rx 83, tx 84;
    SPI5: rx 85, tx 86;
);

// The vector each DMA stream raises.

stream_interrupts!(
    1: [0 => DMA1_STREAM0, 1 => DMA1_STREAM1, 2 => DMA1_STREAM2, 3 => DMA1_STREAM3,
        4 => DMA1_STREAM4, 5 => DMA1_STREAM5, 6 => DMA1_STREAM6, 7 => DMA1_STREAM7];
    2: [0 => DMA2_STREAM0, 1 => DMA2_STREAM1, 2 => DMA2_STREAM2, 3 => DMA2_STREAM3,
        4 => DMA2_STREAM4, 5 => DMA2_STREAM5, 6 => DMA2_STREAM6, 7 => DMA2_STREAM7];
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uart::{Resource, Signal, Wiring};

    /// The TBS Lucid H7's receiver port, as the bench wires it.
    type Usart6Lucid = Wiring<
        USART6,
        Pin<'C', 6, Alternate<7>>,
        Pin<'C', 7, Alternate<7>>,
        Stream<1, 6>,
        Stream<1, 0>,
    >;

    #[test]
    fn wiring_describes_itself() {
        assert_eq!(
            Usart6Lucid::RESOURCES,
            [
                Resource::Peripheral("USART6"),
                Resource::Pin { signal: Signal::Tx, port: 'C', pin: 6, af: 7 },
                Resource::Pin { signal: Signal::Rx, port: 'C', pin: 7, af: 7 },
                // The Lucid's manifest names these DMA1_STREAM6_CH72 and
                // DMA1_STREAM0_CH71.
                Resource::Dma { signal: Signal::Tx, controller: 1, stream: 6, request: 72 },
                Resource::Dma { signal: Signal::Rx, controller: 1, stream: 0, request: 71 },
                Resource::Interrupt("USART6"),
                Resource::Interrupt("DMA1_STREAM0"),
                Resource::Interrupt("DMA1_STREAM6"),
            ]
        );
    }
}
