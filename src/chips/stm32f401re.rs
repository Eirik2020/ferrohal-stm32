//! STM32F401RE glue: the chip facts the IP modules leave out.
//!
//! Every routing fact here (RCC bits, pin alternate functions, DMA request
//! channels) was read from stm32-metapac 21.0.0's metadata for this chip.
//! They are hand-written for now, in the trait shapes a generator would
//! emit from that metadata later.

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma_v2::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    rcc_f4::{Bus, Limits, RCC, RccBit},
    time::Hertz,
    uart::ConfigError,
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    usart_v2::{self, Info, Line, RxDma, RxPin, TxDma, TxPin},
};

#[path = "shared/macros.rs"]
#[macro_use]
mod macros;

/// The chip's 96 KiB of SRAM (RM0368 section 2.3), all of it on the DMA's
/// bus matrix.
const DMA_RAM: [(usize, usize); 1] = [(0x2000_0000, 96 * 1024)];

/// Whether `buffer` can be handed to a DMA stream: whether it lies in SRAM.
/// A Cortex-M4 has no data cache, so that is all. `Uart::new` checks every
/// buffer with this.
pub fn check_dma_buffer(buffer: &[u8]) -> Result<(), ConfigError> {
    if macros::within(buffer, &DMA_RAM) {
        Ok(())
    } else {
        Err(ConfigError::BufferUnreachable)
    }
}

/// DS10086 and RM0368, at 2.7 V to 3.6 V. VOS keeps its reset value
/// (scale 2), which covers 84 MHz on this chip.
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
};

tokens!(
    USART1, USART2, USART6, TIM1, TIM2, TIM3, TIM4, TIM5, TIM9, TIM10, TIM11, SPI1, SPI2, SPI3,
    I2C1, I2C2, I2C3, ADC1, ADC1_COMMON, SDIO, SYSCFG, EXTI, PWR, CRC, RTC, IWDG, WWDG,
    USB_OTG_FS, DBGMCU, UID,
);

peripherals!(
    "FLASH is not here: RCC's `freeze` owns its latency register.";
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
    USART6: USART6,
    TIM1: TIM1,
    TIM2: TIM2,
    TIM3: TIM3,
    TIM4: TIM4,
    TIM5: TIM5,
    TIM9: TIM9,
    TIM10: TIM10,
    TIM11: TIM11,
    SPI1: SPI1,
    SPI2: SPI2,
    SPI3: SPI3,
    I2C1: I2C1,
    I2C2: I2C2,
    I2C3: I2C3,
    ADC1: ADC1,
    ADC1_COMMON: ADC1_COMMON,
    SDIO: SDIO,
    SYSCFG: SYSCFG,
    EXTI: EXTI,
    PWR: PWR,
    CRC: CRC,
    RTC: RTC,
    IWDG: IWDG,
    WWDG: WWDG,
    USB_OTG_FS: USB_OTG_FS,
    DBGMCU: DBGMCU,
    UID: UID,
);

// GPIO ports and DMA controllers, with their AHB1ENR bits.

macro_rules! ports {
    ($($letter:literal => $regs:ident, $bit:literal;)+) => {
        $(
            impl crate::Sealed for Port<$letter> {}

            impl PortInstance for Port<$letter> {
                fn regs() -> pac::gpio::Gpio {
                    pac::$regs
                }
                const ENABLE: RccBit = RccBit::new(Bus::Ahb1, $bit);
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

impl crate::Sealed for Dma<1> {}
impl DmaInstance for Dma<1> {
    fn regs() -> pac::dma::Dma {
        pac::DMA1
    }
    const ENABLE: RccBit = RccBit::new(Bus::Ahb1, 21);
}

impl crate::Sealed for Dma<2> {}
impl DmaInstance for Dma<2> {
    fn regs() -> pac::dma::Dma {
        pac::DMA2
    }
    const ENABLE: RccBit = RccBit::new(Bus::Ahb1, 22);
}

// USART instances. The bus is also the kernel clock: USART1 and USART6 run
// from PCLK2, USART2 from PCLK1.

macro_rules! usarts {
    ($($name:ident => $info:ident, $bus:ident, $bit:literal;)+) => {
        $(
            static $info: Info = Info::new(Line::new(pac::$name), RccBit::new(Bus::$bus, $bit));

            impl usart_v2::Instance for $name {
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
    USART2 => USART2_INFO, Apb1, 17;
    USART6 => USART6_INFO, Apb2, 5;
);

// Pins: (port, pin, alternate function).

pins!(TxPin<USART1>: ('A', 9, 7), ('B', 6, 7));
pins!(RxPin<USART1>: ('A', 10, 7), ('B', 7, 7));
pins!(TxPin<USART2>: ('A', 2, 7));
pins!(RxPin<USART2>: ('A', 3, 7));
pins!(TxPin<USART6>: ('A', 11, 8), ('C', 6, 8));
pins!(RxPin<USART6>: ('A', 12, 8), ('C', 7, 8));

// DMA: (controller, stream, request channel).

macro_rules! dma {
    ($trait:ident<$usart:ident>: $(($dma:literal, $stream:literal, $request:literal)),+ $(,)?) => {
        $(
            // SAFETY: RM0368 table 28 and 29 (DMA1 and DMA2 request
            // mapping), matching stm32-metapac 21's metadata.
            unsafe impl $trait<$usart> for Stream<$dma, $stream> {
                const REQUEST: u8 = $request;
            }
        )+
    };
}

dma!(TxDma<USART1>: (2, 7, 4));
dma!(RxDma<USART1>: (2, 2, 4), (2, 5, 4));
dma!(TxDma<USART2>: (1, 6, 4));
dma!(RxDma<USART2>: (1, 5, 4));
dma!(TxDma<USART6>: (2, 6, 5), (2, 7, 5));
dma!(RxDma<USART6>: (2, 1, 5), (2, 2, 5));

// SPI instances: (bus, enable bit, kernel clock). The kernel clock is the
// bus clock.

spis!(
    SPI1 => SPI1_INFO, Apb2, 12, Apb2;
    SPI2 => SPI2_INFO, Apb1, 14, Apb1;
    SPI3 => SPI3_INFO, Apb1, 15, Apb1;
);

pins!(SckPin<SPI1>: ('A', 5, 5), ('B', 3, 5));
pins!(MisoPin<SPI1>: ('A', 6, 5), ('B', 4, 5));
pins!(MosiPin<SPI1>: ('A', 7, 5), ('B', 5, 5));
pins!(SckPin<SPI2>: ('B', 10, 5), ('B', 13, 5));
pins!(MisoPin<SPI2>: ('B', 14, 5), ('C', 2, 5));
pins!(MosiPin<SPI2>: ('B', 15, 5), ('C', 3, 5));
pins!(SckPin<SPI3>: ('B', 3, 6), ('C', 10, 6));
pins!(MisoPin<SPI3>: ('B', 4, 6), ('C', 11, 6));
pins!(MosiPin<SPI3>: ('B', 5, 6), ('C', 12, 6));

dma!(SpiTxDma<SPI1>: (2, 3, 3), (2, 5, 3));
dma!(SpiRxDma<SPI1>: (2, 0, 3), (2, 2, 3));
dma!(SpiTxDma<SPI2>: (1, 4, 0));
dma!(SpiRxDma<SPI2>: (1, 3, 0));
dma!(SpiTxDma<SPI3>: (1, 5, 0), (1, 7, 0));
dma!(SpiRxDma<SPI3>: (1, 0, 0), (1, 2, 0));

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
                Resource::Pin { signal: Signal::Tx, port: 'A', pin: 9, af: 7 },
                Resource::Pin { signal: Signal::Rx, port: 'A', pin: 10, af: 7 },
                Resource::Dma { signal: Signal::Tx, controller: 2, stream: 7, request: 4 },
                Resource::Dma { signal: Signal::Rx, controller: 2, stream: 2, request: 4 },
                Resource::Interrupt("USART1"),
                Resource::Interrupt("DMA2_STREAM2"),
                Resource::Interrupt("DMA2_STREAM7"),
            ]
        );
        assert_eq!(
            RxWiring::<USART6, Pin<'A', 12, Alternate<8>>, Stream<2, 1>>::RESOURCES[2],
            Resource::Dma { signal: Signal::Rx, controller: 2, stream: 1, request: 5 }
        );
    }
}
