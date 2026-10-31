//! STM32H753ZI glue: the chip facts the IP modules leave out.
//!
//! Every routing fact here (RCC bits, pin alternate functions, DMAMUX
//! request IDs) was read from stm32-metapac 21.0.0's metadata for this
//! chip; clock limits and the memory map from DS12117 and RM0433. They are
//! hand-written for now, in the trait shapes a generator would emit later.

use core::sync::atomic::{AtomicBool, Ordering};

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma_v1::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    rcc_h7rm0433::{Bus, Limits, RCC, RccBit},
    time::Hertz,
    uart::ConfigError,
    usart_v4::{self, Info, Line, RxDma, RxPin, TxDma, TxPin},
};

/// Limits both silicon revisions meet. The wide VCO tops out at 836 MHz on
/// revision Y (DS12117 table 39) and 960 MHz on revision V (table 136); the
/// lower one is used.
pub(crate) const CLOCK_LIMITS: Limits = Limits {
    scale1: (Hertz::mhz(400), Hertz::mhz(200), Hertz::mhz(100)),
    scale2: (Hertz::mhz(300), Hertz::mhz(150), Hertz::mhz(75)),
    scale3: (Hertz::mhz(200), Hertz::mhz(100), Hertz::mhz(50)),
    hse_oscillator: (Hertz::mhz(4), Hertz::mhz(48)),
    hse_bypass: (Hertz::mhz(4), Hertz::mhz(50)),
    pll_input: (Hertz::mhz(1), Hertz::mhz(16)),
    vco_medium: (Hertz::mhz(150), Hertz::mhz(420)),
    vco_wide: (Hertz::mhz(192), Hertz::mhz(836)),
};

/// RAM DMA1 and DMA2 can reach (RM0433 figure 1): AXI SRAM, SRAM1 to 3 in
/// D2, and SRAM4 in D3. Not the DTCM or ITCM, which only the core sees.
const DMA_RAM: [(usize, usize); 3] = [
    (0x2400_0000, 512 * 1024),
    (0x3000_0000, 288 * 1024),
    (0x3800_0000, 64 * 1024),
];

/// Whether `buffer` can be handed to a DMA1 or DMA2 stream. `Uart::new`
/// checks every buffer with this; firmware can use it to check its own.
///
/// - It must lie in RAM those controllers reach: not the DTCM or ITCM.
/// - While the data cache is on, it must lie in memory the MPU marks
///   non-cacheable, since the drivers do no cache maintenance: the CPU
///   would read stale received bytes and transmit stale written ones.
pub fn check_dma_buffer(buffer: &[u8]) -> Result<(), ConfigError> {
    let start = buffer.as_ptr() as usize;
    let end = start.saturating_add(buffer.len());
    if !DMA_RAM
        .iter()
        .any(|&(base, len)| start >= base && end <= base + len)
    {
        return Err(ConfigError::BufferUnreachable);
    }
    if cortex_m::peripheral::SCB::dcache_enabled() && !mpu_non_cacheable(start, end) {
        return Err(ConfigError::BufferCacheable);
    }
    Ok(())
}

/// Whether the MPU makes all of `start..end` non-cacheable: one enabled
/// region holds the whole range, with every subregion it touches enabled
/// and non-cacheable attributes, and no higher-numbered region (which would
/// take precedence) overlaps the range at all. Anything else counts as
/// cacheable, including the MPU being off, since the default memory map
/// caches SRAM.
///
/// The MPU belongs to the firmware. This only reads it, apart from
/// stepping RNR through the regions, which it restores.
fn mpu_non_cacheable(start: usize, end: usize) -> bool {
    // SAFETY: reads, and an RNR write that is undone before returning. The
    // firmware must not be reprogramming the MPU concurrently.
    let mpu = unsafe { &*cortex_m::peripheral::MPU::PTR };
    if mpu.ctrl.read() & 1 == 0 {
        return false;
    }
    let regions = (mpu._type.read() >> 8) & 0xff;
    let saved = mpu.rnr.read();
    let mut covered = false;
    for n in 0..regions {
        // SAFETY: see above.
        unsafe { mpu.rnr.write(n) };
        let rasr = mpu.rasr.read();
        let rbar = mpu.rbar.read();
        if rasr & 1 == 0 {
            continue;
        }
        // Region size 2^(SIZE + 1) bytes; SIZE 31 is the whole 4 GiB.
        let size_field = (rasr >> 1) & 0x1f;
        let size: u64 = 1 << (size_field + 1);
        let base = u64::from(rbar) & !(size - 1);
        let (start, end) = (start as u64, end as u64);
        if end <= base || start >= base + size {
            continue;
        }
        // Higher numbers win: this region now decides.
        let inside = start >= base && end <= base + size;
        covered = inside && subregions_enabled(rasr, base, size, start, end) && non_cacheable(rasr);
    }
    // SAFETY: see above.
    unsafe { mpu.rnr.write(saved) };
    covered
}

/// Whether every subregion of a region that `start..end` touches is
/// enabled. Regions of 256 bytes and up have eight subregions, each
/// disabled by one SRD bit; a disabled one falls through to lower regions.
fn subregions_enabled(rasr: u32, base: u64, size: u64, start: u64, end: u64) -> bool {
    if size < 256 {
        return true;
    }
    let sub = size / 8;
    (0..8).all(|i| {
        let (lo, hi) = (base + i * sub, base + (i + 1) * sub);
        let touched = start < hi && end > lo;
        !touched || rasr & (1 << (8 + i)) == 0
    })
}

/// RASR's TEX, C and B describe a memory type the data cache does not
/// cache: strongly ordered, device, or normal non-cacheable. Reserved
/// encodings count as cacheable.
fn non_cacheable(rasr: u32) -> bool {
    let tex = (rasr >> 19) & 0b111;
    let c = (rasr >> 17) & 1;
    let b = (rasr >> 16) & 1;
    match (tex, c, b) {
        // Strongly ordered, shared device, normal non-cacheable, device.
        (0b000, 0, 0) | (0b000, 0, 1) | (0b001, 0, 0) | (0b010, 0, 0) => true,
        // Cached memory types: inner policy in C and B, 00 non-cacheable.
        (0b100..=0b111, 0, 0) => true,
        _ => false,
    }
}

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
    USART1, USART2, USART3, UART4, UART5, USART6, UART7, UART8, LPUART1, TIM1, TIM2, TIM3, TIM4,
    TIM5, TIM6, TIM7, TIM8, TIM12, TIM13, TIM14, TIM15, TIM16, TIM17, LPTIM1, LPTIM2, LPTIM3,
    LPTIM4, LPTIM5, HRTIM1, SPI1, SPI2, SPI3, SPI4, SPI5, SPI6, I2C1, I2C2, I2C3, I2C4, SAI1, SAI2,
    SAI3, SAI4, SPDIFRX1, CEC, DAC1, CRS, OPAMP1, OPAMP2, MDIOS, FDCAN1, FDCAN2, FDCANRAM, ADC1,
    ADC2, ADC3, ADC12_COMMON, ADC3_COMMON, ETH, USB_OTG_HS, USB_OTG_FS, DCMI, CRYP, HASH, RNG,
    SDMMC1, SDMMC2, DLYB_SDMMC1, DLYB_SDMMC2, LTDC, WWDG1, IWDG1, MDMA, DMA2D, JPEG, FMC, QUADSPI,
    DLYB_QUADSPI, EXTI, SYSCFG, COMP1, COMP2, VREFBUF, RTC, CRC, BDMA, DMAMUX2, HSEM, DBGMCU, UID,
);

macro_rules! peripherals {
    ($($name:ident: $ty:ty),+ $(,)?) => {
        /// Every peripheral of the chip, once. svd2rust's model: take this
        /// once, then move each token to the driver or task that owns it.
        ///
        /// Not here: FLASH and PWR, which RCC's `freeze` owns, and DMAMUX1,
        /// whose channels belong to the DMA1 and DMA2 streams.
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

macro_rules! pins {
    ($trait:ident<$usart:ident>: $(($port:literal, $pin:literal, $af:literal)),+ $(,)?) => {
        $(impl $trait<$usart> for Pin<$port, $pin, Alternate<$af>> {})+
    };
}

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
    ($($usart:ident: rx $rx:literal, tx $tx:literal;)+) => {
        $(
            // SAFETY: the DMAMUX1 request IDs for this instance, from
            // stm32-metapac 21's metadata (RM0433 table 121).
            unsafe impl<const D: u8, const N: u8> RxDma<$usart> for Stream<D, N>
            where
                Dma<D>: DmaInstance,
                Stream<D, N>: crate::dma::StreamInterrupt,
            {
                const REQUEST: u8 = $rx;
            }

            // SAFETY: as above.
            unsafe impl<const D: u8, const N: u8> TxDma<$usart> for Stream<D, N>
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
    USART1: rx 41, tx 42;
    USART2: rx 43, tx 44;
    USART3: rx 45, tx 46;
    UART4: rx 63, tx 64;
    UART5: rx 65, tx 66;
    USART6: rx 71, tx 72;
    UART7: rx 79, tx 80;
    UART8: rx 81, tx 82;
);

// The vector each DMA stream raises.

macro_rules! stream_interrupts {
    ($($dma:literal: [$($stream:literal => $vector:ident),+];)+) => {
        $($(
            impl crate::dma::StreamInterrupt for crate::dma::Stream<$dma, $stream> {
                const INTERRUPT: Interrupt = Interrupt::$vector;
                const NAME: &'static str = stringify!($vector);
            }
        )+)+
    };
}

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
                Resource::Uart("USART6"),
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
