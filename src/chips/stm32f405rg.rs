//! STM32F405RG glue: the chip facts the IP modules leave out.
//!
//! Every routing fact here (RCC bits, pin alternate functions, DMA request
//! channels) was read from stm32-metapac 21.0.0's metadata for this chip,
//! whose ports are cut to the LQFP64 package (`BONDED` below). They are
//! hand-written for now, in the trait shapes a generator would emit from
//! that metadata later.
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
    uart::ConfigError,
    usart_v2::{self, Info, LineRegs, RxDma, RxPin, TxDma, TxPin},
};

#[path = "shared/macros.rs"]
#[macro_use]
mod macros;

/// SRAM1 (112 KiB) and SRAM2 (16 KiB), contiguous (RM0090 section 2.3).
/// Not the 64 KiB of CCM RAM at 0x1000_0000: only the core reaches it, so a
/// DMA buffer there would never see a byte (RM0090 figure 1).
const DMA_RAM: [(usize, usize); 1] = [(0x2000_0000, 128 * 1024)];

/// Whether `buffer` can be handed to a DMA stream: whether it lies in SRAM1
/// or SRAM2. A Cortex-M4 has no data cache, so that is all. `Uart::new`
/// checks every buffer with this; firmware that places statics in CCM RAM
/// can use it to check its own.
pub fn check_dma_buffer(buffer: &[u8]) -> Result<(), ConfigError> {
    if macros::within(buffer, &DMA_RAM) {
        Ok(())
    } else {
        Err(ConfigError::BufferUnreachable)
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

tokens!(
    USART1,
    USART2,
    USART3,
    UART4,
    UART5,
    USART6,
    TIM1,
    TIM2,
    TIM3,
    TIM4,
    TIM5,
    TIM6,
    TIM7,
    TIM8,
    TIM9,
    TIM10,
    TIM11,
    TIM12,
    TIM13,
    TIM14,
    SPI1,
    SPI2,
    SPI3,
    I2C1,
    I2C2,
    I2C3,
    ADC1,
    ADC2,
    ADC3,
    ADC123_COMMON,
    CAN1,
    CAN2,
    DAC1,
    RNG,
    SDIO,
    SYSCFG,
    EXTI,
    CRC,
    RTC,
    IWDG,
    WWDG,
    USB_OTG_FS,
    USB_OTG_HS,
    DBGMCU,
    UID,
);

peripherals!(
    "Not here: FLASH and PWR, which RCC's `freeze` owns for the flash latency and the voltage scale.";
    RCC: RCC,
    GPIOA: Port<'A'>,
    GPIOB: Port<'B'>,
    GPIOC: Port<'C'>,
    GPIOD: Port<'D'>,
    GPIOH: Port<'H'>,
    DMA1: Dma<1>,
    DMA2: Dma<2>,
    USART1: USART1,
    USART2: USART2,
    USART3: USART3,
    UART4: UART4,
    UART5: UART5,
    USART6: USART6,
    TIM1: TIM1,
    TIM2: TIM2,
    TIM3: TIM3,
    TIM4: TIM4,
    TIM5: TIM5,
    TIM6: TIM6,
    TIM7: TIM7,
    TIM8: TIM8,
    TIM9: TIM9,
    TIM10: TIM10,
    TIM11: TIM11,
    TIM12: TIM12,
    TIM13: TIM13,
    TIM14: TIM14,
    SPI1: SPI1,
    SPI2: SPI2,
    SPI3: SPI3,
    I2C1: I2C1,
    I2C2: I2C2,
    I2C3: I2C3,
    ADC1: ADC1,
    ADC2: ADC2,
    ADC3: ADC3,
    ADC123_COMMON: ADC123_COMMON,
    CAN1: CAN1,
    CAN2: CAN2,
    DAC1: DAC1,
    RNG: RNG,
    SDIO: SDIO,
    SYSCFG: SYSCFG,
    EXTI: EXTI,
    CRC: CRC,
    RTC: RTC,
    IWDG: IWDG,
    WWDG: WWDG,
    USB_OTG_FS: USB_OTG_FS,
    USB_OTG_HS: USB_OTG_HS,
    DBGMCU: DBGMCU,
    UID: UID,
);

// GPIO ports and DMA controllers, with their AHB1ENR bits. Only the
// bonded ports are declared: the LQFP64 bonds A to C, PD2 and PH0/PH1 (see
// `BONDED`); ports E to I exist as registers only.

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
    'H' => GPIOH, 7;
);

/// The LQFP64 package's bonded pins, as (port, pin mask): ports A to C, PD2
/// and PH0/PH1. `tools/check_glue.py` reads this: a routing row on any other
/// pin fails, and a bonded pin with no row is reported, not failed.
#[allow(dead_code)] // Read by tools/check_glue.py.
pub(crate) const BONDED: &[(char, u16)] = &[
    ('A', 0xffff),
    ('B', 0xffff),
    ('C', 0xffff),
    ('D', 0x0004), // PD2
    ('H', 0x0003), // PH0, PH1
];

impl crate::Sealed for Dma<1> {}
impl crate::dma::DmaRegisters for Dma<1> {
    fn regs() -> pac::dma::Dma {
        pac::DMA1
    }
}
impl DmaInstance for Dma<1> {
    const ENABLE: RccBit = RccBit::new(Bus::Ahb1, 21);
}

impl crate::Sealed for Dma<2> {}
impl crate::dma::DmaRegisters for Dma<2> {
    fn regs() -> pac::dma::Dma {
        pac::DMA2
    }
}
impl DmaInstance for Dma<2> {
    const ENABLE: RccBit = RccBit::new(Bus::Ahb1, 22);
}

// U(S)ART instances. The bus is also the kernel clock: USART1 and USART6 run
// from PCLK2, the rest from PCLK1.

macro_rules! usarts {
    ($($kind:ident $name:ident => $info:ident, $bus:ident, $bit:literal;)+) => {
        $(
            static $info: Info = Info::new(LineRegs::new(pac::$name), RccBit::new(Bus::$bus, $bit));

            impl usart_v2::Instance for $name {
                const KERNEL: Bus = Bus::$bus;
                const NAME: &'static str = stringify!($name);
                const INTERRUPT: Interrupt = Interrupt::$name;
                const INTERRUPT_NAME: &'static str = stringify!($name);
                const FRACTIONAL_STOP_BITS: bool = fractional_stop!($kind);
                fn info() -> &'static Info {
                    &$info
                }
            }
        )+
    };
}

// A `uart` row is an asynchronous-only UART: the same registers, without
// the synchronous clock, smartcard and flow control, and without 0.5 and 1.5
// stop bits (RM0090, USART_CR2 STOP).
macro_rules! fractional_stop {
    (usart) => {
        true
    };
    (uart) => {
        false
    };
}

usarts!(
    usart USART1 => USART1_INFO, Apb2, 4;
    usart USART2 => USART2_INFO, Apb1, 17;
    usart USART3 => USART3_INFO, Apb1, 18;
    uart UART4 => UART4_INFO, Apb1, 19;
    uart UART5 => UART5_INFO, Apb1, 20;
    usart USART6 => USART6_INFO, Apb2, 5;
);

// Pins: (port, pin, alternate function). PC10 and PC11 serve USART3 (AF7)
// or UART4 (AF8).

pins!(TxPin<USART1>: ('A', 9, 7), ('B', 6, 7));
pins!(RxPin<USART1>: ('A', 10, 7), ('B', 7, 7));
pins!(TxPin<USART2>: ('A', 2, 7));
pins!(RxPin<USART2>: ('A', 3, 7));
pins!(TxPin<USART3>: ('B', 10, 7), ('C', 10, 7));
pins!(RxPin<USART3>: ('B', 11, 7), ('C', 11, 7));
pins!(TxPin<UART4>: ('A', 0, 8), ('C', 10, 8));
pins!(RxPin<UART4>: ('A', 1, 8), ('C', 11, 8));
pins!(TxPin<UART5>: ('C', 12, 8));
pins!(RxPin<UART5>: ('D', 2, 8));
pins!(TxPin<USART6>: ('C', 6, 8));
pins!(RxPin<USART6>: ('C', 7, 8));

// DMA: (controller, stream, request channel).

macro_rules! dma {
    ($trait:ident<$usart:ident>: $(($dma:literal, $stream:literal, $request:literal)),+ $(,)?) => {
        $(
            // SAFETY: RM0090 tables 42 and 43 (DMA1 and DMA2 request
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
dma!(TxDma<USART3>: (1, 3, 4), (1, 4, 7));
dma!(RxDma<USART3>: (1, 1, 4));
dma!(TxDma<UART4>: (1, 4, 4));
dma!(RxDma<UART4>: (1, 2, 4));
dma!(TxDma<UART5>: (1, 7, 4));
dma!(RxDma<UART5>: (1, 0, 4));
dma!(TxDma<USART6>: (2, 6, 5), (2, 7, 5));
dma!(RxDma<USART6>: (2, 1, 5), (2, 2, 5));

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

// The ADC. ADC2 and ADC3 aren't offered. The calibration values are the
// datasheets' (DS8626 and DS10086), taken at 3.3 V and 12 bits.

adcs!(
    ADC1 => ADC123_COMMON, Apb2, 8, true;
);
adc_internal!(ADC1: 16, 17);
adc_pins!(ADC1: ('A', 0, 0), ('A', 1, 1), ('A', 2, 2), ('A', 3, 3), ('A', 4, 4), ('A', 5, 5), ('A', 6, 6), ('A', 7, 7),
    ('B', 0, 8), ('B', 1, 9), ('C', 0, 10), ('C', 1, 11), ('C', 2, 12), ('C', 3, 13), ('C', 4, 14), ('C', 5, 15));
adc_dma!(ADC1: (2, 0, 0), (2, 4, 0));

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

// I2C instances: (bus, enable bit). The kernel clock is PCLK1, the bus
// clock, with no mux to set. The SMBus SMBA pins (PB5, PB12, PA9) are not
// routed: the driver carries I2C alone.

i2cs!(
    I2C1 => I2C1_INFO, Apb1, 21;
    I2C2 => I2C2_INFO, Apb1, 22;
    I2C3 => I2C3_INFO, Apb1, 23;
);

pins!(SclPin<I2C1>: ('B', 6, 4), ('B', 8, 4));
pins!(SdaPin<I2C1>: ('B', 7, 4), ('B', 9, 4));
pins!(SclPin<I2C2>: ('B', 10, 4));
pins!(SdaPin<I2C2>: ('B', 11, 4));
pins!(SclPin<I2C3>: ('A', 8, 4));
pins!(SdaPin<I2C3>: ('C', 9, 4));

// Timers: (bus, enable bit, update vector, counter bits). Each counts at its
// bus's timer clock.

timers!(
    TIM1 => Apb2, 0, TIM1_UP_TIM10, 16;
    TIM2 => Apb1, 0, TIM2, 32;
    TIM3 => Apb1, 1, TIM3, 16;
    TIM4 => Apb1, 2, TIM4, 16;
    TIM5 => Apb1, 3, TIM5, 32;
    TIM6 => Apb1, 4, TIM6_DAC, 16;
    TIM7 => Apb1, 5, TIM7, 16;
    TIM8 => Apb2, 1, TIM8_UP_TIM13, 16;
    TIM9 => Apb2, 16, TIM1_BRK_TIM9, 16;
    TIM10 => Apb2, 17, TIM1_UP_TIM10, 16;
    TIM11 => Apb2, 18, TIM1_TRG_COM_TIM11, 16;
    TIM12 => Apb1, 6, TIM8_BRK_TIM12, 16;
    TIM13 => Apb1, 7, TIM8_UP_TIM13, 16;
    TIM14 => Apb1, 8, TIM8_TRG_COM_TIM14, 16;
);

// Timer channels: (channels, plain or break unit), their pins, and their
// compare (CHx) and update (UP) DMA requests. Where a stream serves a signal
// on two channels, the one dedicated to that signal is kept (TIM1 CH3 on
// DMA2 stream 6: channel 6, not channel 0, which TIM1 CH1 to CH3 share).

channel_timers!(
    TIM1 => 4, break;
    TIM2 => 4, plain;
    TIM3 => 4, plain;
    TIM4 => 4, plain;
    TIM5 => 4, plain;
    TIM8 => 4, break;
    TIM9 => 2, plain;
    TIM10 => 1, plain;
    TIM11 => 1, plain;
    TIM12 => 2, plain;
    TIM13 => 1, plain;
    TIM14 => 1, plain;
);

timer_pins!(TIM1 CH1: ('A', 8, 1));
timer_pins!(TIM1 CH1N: ('A', 7, 1), ('B', 13, 1));
timer_pins!(TIM1 CH2: ('A', 9, 1));
timer_pins!(TIM1 CH2N: ('B', 0, 1), ('B', 14, 1));
timer_pins!(TIM1 CH3: ('A', 10, 1));
timer_pins!(TIM1 CH3N: ('B', 1, 1), ('B', 15, 1));
timer_pins!(TIM1 CH4: ('A', 11, 1));
timer_pins!(TIM2 CH1: ('A', 0, 1), ('A', 5, 1), ('A', 15, 1));
timer_pins!(TIM2 CH2: ('A', 1, 1), ('B', 3, 1));
timer_pins!(TIM2 CH3: ('A', 2, 1), ('B', 10, 1));
timer_pins!(TIM2 CH4: ('A', 3, 1), ('B', 11, 1));
timer_pins!(TIM3 CH1: ('A', 6, 2), ('B', 4, 2), ('C', 6, 2));
timer_pins!(TIM3 CH2: ('A', 7, 2), ('B', 5, 2), ('C', 7, 2));
timer_pins!(TIM3 CH3: ('B', 0, 2), ('C', 8, 2));
timer_pins!(TIM3 CH4: ('B', 1, 2), ('C', 9, 2));
timer_pins!(TIM4 CH1: ('B', 6, 2));
timer_pins!(TIM4 CH2: ('B', 7, 2));
timer_pins!(TIM4 CH3: ('B', 8, 2));
timer_pins!(TIM4 CH4: ('B', 9, 2));
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
timer_pins!(TIM9 CH1: ('A', 2, 3));
timer_pins!(TIM9 CH2: ('A', 3, 3));
timer_pins!(TIM10 CH1: ('B', 8, 3));
timer_pins!(TIM11 CH1: ('B', 9, 3));
timer_pins!(TIM12 CH1: ('B', 14, 9));
timer_pins!(TIM12 CH2: ('B', 15, 9));
timer_pins!(TIM13 CH1: ('A', 6, 9));
timer_pins!(TIM14 CH1: ('A', 7, 9));

timer_dma!(TIM1 CH1: (2, 1, 6), (2, 3, 6), (2, 6, 0));
timer_dma!(TIM1 CH2: (2, 2, 6), (2, 6, 0));
timer_dma!(TIM1 CH3: (2, 6, 6));
timer_dma!(TIM1 CH4: (2, 4, 6));
timer_dma!(TIM1 UP: (2, 5, 6));
timer_dma!(TIM2 CH1: (1, 5, 3));
timer_dma!(TIM2 CH2: (1, 6, 3));
timer_dma!(TIM2 CH3: (1, 1, 3));
timer_dma!(TIM2 CH4: (1, 6, 3), (1, 7, 3));
timer_dma!(TIM2 UP: (1, 1, 3), (1, 7, 3));
timer_dma!(TIM3 CH1: (1, 4, 5));
timer_dma!(TIM3 CH2: (1, 5, 5));
timer_dma!(TIM3 CH3: (1, 7, 5));
timer_dma!(TIM3 CH4: (1, 2, 5));
timer_dma!(TIM3 UP: (1, 2, 5));
timer_dma!(TIM4 CH1: (1, 0, 2));
timer_dma!(TIM4 CH2: (1, 3, 2));
timer_dma!(TIM4 CH3: (1, 7, 2));
timer_dma!(TIM4 UP: (1, 6, 2));
timer_dma!(TIM5 CH1: (1, 2, 6));
timer_dma!(TIM5 CH2: (1, 4, 6));
timer_dma!(TIM5 CH3: (1, 0, 6));
timer_dma!(TIM5 CH4: (1, 1, 6), (1, 3, 6));
timer_dma!(TIM5 UP: (1, 0, 6), (1, 6, 6));
timer_dma!(TIM8 CH1: (2, 2, 7));
timer_dma!(TIM8 CH2: (2, 2, 0), (2, 3, 7));
timer_dma!(TIM8 CH3: (2, 2, 0), (2, 4, 7));
timer_dma!(TIM8 CH4: (2, 7, 7));
timer_dma!(TIM8 UP: (2, 1, 7));

// Only DMA2's peripheral port reaches the GPIO ports; DMA1's reaches APB1
// alone (RM0090 figure 1).
gpio_dma!(
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

// The full-speed USB port.

usb_fs!(
    USB_OTG_FS => Ahb2, 7, OTG_FS, 320, 4, fs_core;
);

// The vector each DMA stream raises.

stream_interrupts!(
    1: [0 => DMA1_STREAM0, 1 => DMA1_STREAM1, 2 => DMA1_STREAM2, 3 => DMA1_STREAM3,
        4 => DMA1_STREAM4, 5 => DMA1_STREAM5, 6 => DMA1_STREAM6, 7 => DMA1_STREAM7];
    2: [0 => DMA2_STREAM0, 1 => DMA2_STREAM1, 2 => DMA2_STREAM2, 3 => DMA2_STREAM3,
        4 => DMA2_STREAM4, 5 => DMA2_STREAM5, 6 => DMA2_STREAM6, 7 => DMA2_STREAM7];
);

/// The die revision code, `DBGMCU_IDCODE.REV_ID` (bits 31:16) on RM0090.
pub(crate) fn revision() -> u16 {
    pac::DBGMCU.idcode().read().rev_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uart::{Resource, Signal, Wiring};

    /// The Foxeer F405 V2's receiver port, as its manifest claims it.
    type Usart2Foxeer = Wiring<
        USART2,
        Pin<'A', 2, Alternate<7>>,
        Pin<'A', 3, Alternate<7>>,
        Stream<1, 6>,
        Stream<1, 5>,
    >;

    #[test]
    fn wiring_describes_itself() {
        assert_eq!(
            Usart2Foxeer::RESOURCES,
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
                // The manifest names these DMA1_STREAM6_CH4 and
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

    /// Every UART and motor route the Foxeer's manifest claims is one this
    /// glue offers, on the pin and alternate function it names and with its
    /// DMA channel. That it compiles is half the check.
    #[test]
    fn foxeer_manifest_routes_exist() {
        use crate::{
            timer::{ChannelPin, ChannelTimer, CompareDma, ComplementaryPin},
            usart_v2::Instance,
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

    /// The Foxeer's IMU bus, as its manifest claims it.
    #[test]
    fn imu_spi_describes_itself() {
        type Spi1Foxeer = crate::spi::Wiring<
            SPI1,
            Pin<'A', 5, Alternate<5>>,
            Pin<'A', 6, Alternate<5>>,
            Pin<'A', 7, Alternate<5>>,
            Stream<2, 3>,
            Stream<2, 0>,
        >;
        assert_eq!(
            Spi1Foxeer::RESOURCES,
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
        assert_eq!(check_dma_buffer(ccm), Err(ConfigError::BufferUnreachable));
        // SAFETY: as above.
        let sram2_end = unsafe { core::slice::from_raw_parts(0x2001_ff00 as *const u8, 256) };
        assert_eq!(check_dma_buffer(sram2_end), Ok(()));
        // SAFETY: as above.
        let past = unsafe { core::slice::from_raw_parts(0x2001_ff00 as *const u8, 257) };
        assert_eq!(check_dma_buffer(past), Err(ConfigError::BufferUnreachable));
    }

    #[test]
    fn uart4_and_uart5_refuse_fractional_stop_bits() {
        use crate::uart::{self, StopBits, Uart};
        let clocks = crate::rcc_f4::Config::hsi().validate().unwrap();
        let half = uart::Config::new(115_200).with_stop_bits(StopBits::Half);
        let one_and_half = half.with_stop_bits(StopBits::OneAndHalf);
        let two = half.with_stop_bits(StopBits::Two);
        assert_eq!(
            Uart::<UART4>::check(&clocks, &half),
            Err(ConfigError::Unsupported)
        );
        assert_eq!(
            Uart::<UART5>::check(&clocks, &one_and_half),
            Err(ConfigError::Unsupported)
        );
        assert_eq!(Uart::<UART4>::check(&clocks, &two), Ok(()));
        assert_eq!(Uart::<USART3>::check(&clocks, &half), Ok(()));
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
