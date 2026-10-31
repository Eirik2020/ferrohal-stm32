//! STM32F401RE glue: the chip facts the IP modules leave out.
//!
//! Every routing fact here (RCC bits, pin alternate functions, DMA request
//! channels) was read from stm32-metapac 21.0.0's metadata for this chip.
//! They are hand-written for now, in the trait shapes a generator would
//! emit from that metadata later.

use core::sync::atomic::{AtomicBool, Ordering};

use stm32_metapac as pac;

use crate::{
    dma_v2::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    rcc_f4::{Bus, Limits, RCC, RccBit},
    time::Hertz,
    uart::ConfigError,
    usart_v2::{self, Info, Line, RxDma, RxPin, TxDma, TxPin},
};

/// Whether `buffer` can be handed to a DMA stream: always, here. Every byte
/// of this chip's SRAM is on the DMA's bus matrix, and a Cortex-M4 has no
/// data cache. `Uart::new` checks every buffer with this.
pub fn check_dma_buffer(_buffer: &[u8]) -> Result<(), ConfigError> {
    Ok(())
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

macro_rules! tokens {
    ($($name:ident),+ $(,)?) => {
        $(
            /// A peripheral token with no driver yet. Zero-sized.
            #[allow(non_camel_case_types)]
            pub struct $name {
                _private: (),
            }

            impl $name {
                const fn new() -> Self {
                    Self { _private: () }
                }

                /// A second token for this peripheral.
                ///
                /// # Safety
                ///
                /// Nothing else may use the peripheral while this token is
                /// in use.
                pub unsafe fn steal() -> Self {
                    Self::new()
                }
            }

            impl crate::Sealed for $name {}
        )+
    };
}

tokens!(
    USART1, USART2, USART6, TIM1, TIM2, TIM3, TIM4, TIM5, TIM9, TIM10, TIM11, SPI1, SPI2, SPI3,
    I2C1, I2C2, I2C3, ADC1, ADC1_COMMON, SDIO, SYSCFG, EXTI, PWR, CRC, RTC, IWDG, WWDG,
    USB_OTG_FS, DBGMCU, UID,
);

macro_rules! peripherals {
    ($($name:ident: $ty:ty),+ $(,)?) => {
        /// Every peripheral of the chip, once. svd2rust's model: take this
        /// once, then move each token to the driver or task that owns it.
        ///
        /// FLASH is not here: RCC's `freeze` owns its latency register.
        #[allow(non_snake_case)]
        pub struct Peripherals {
            $(pub $name: $ty,)+
        }

        impl Peripherals {
            /// The tokens, the first time. `None` after that.
            pub fn take() -> Option<Self> {
                if TAKEN.swap(true, Ordering::AcqRel) {
                    None
                } else {
                    // SAFETY: the flag says this is the first time.
                    Some(unsafe { Self::steal() })
                }
            }

            /// The tokens, unconditionally. RTIC calls this when its app
            /// sets `peripherals = true`.
            ///
            /// # Safety
            ///
            /// No token from an earlier `take` or `steal` may still be in
            /// use.
            pub unsafe fn steal() -> Self {
                TAKEN.store(true, Ordering::Release);
                Self {
                    $($name: <$ty>::new(),)+
                }
            }
        }
    };
}

static TAKEN: AtomicBool = AtomicBool::new(false);

peripherals!(
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

macro_rules! pins {
    ($trait:ident<$usart:ident>: $(($port:literal, $pin:literal, $af:literal)),+ $(,)?) => {
        $(impl $trait<$usart> for Pin<$port, $pin, Alternate<$af>> {})+
    };
}

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
