//! Nucleo-H753ZI (MB1364): the clock tree, and which pins and DMA streams
//! each UART uses in each test run. This file is chip-specific; a board on
//! another chip has its own. FerroHAL's routing traits check every choice
//! here at compile time: a pin without the UART's function does not build.
//! Through DMAMUX any DMA1 or DMA2 stream can serve any UART; a stream's
//! type carries the request ID.
//!
//! The bench, on the Zio headers (UM2407 tables 18 to 21; this board has no
//! morpho headers fitted). Every link is a UART's TX wired to its own RX:
//!
//! | Link | TX -> RX | Zio sockets |
//! | --- | --- | --- |
//! | Z1 | USART2 PD5 -> PD6 | CN9-6 -> CN9-4 |
//! | Z2 | USART6 PC6 -> PC7 | CN7-1 -> CN7-11 |
//! | Z3 | USART1 PB6 -> PB7 | CN10-14 -> CN10-16 |
//! | Z4 | UART4 PB9 -> PB8 | CN7-4 -> CN7-2 |
//! | Z5 | UART4 PD1 -> PD0 | CN9-27 -> CN9-25 |
//! | Z6 | USART3 or UART4 PC10 -> PC11 | CN8-6 -> CN8-8 |
//! | Z7 | USART3 PB10 -> PB11 | CN10-32 -> CN10-34 |
//! | Z8 | UART5 PC12 -> PD2 | CN8-10 -> CN8-12 |
//! | Z10 | UART7 PF7 -> PF6 | CN9-26 -> CN10-11 |
//! | Z11 | UART7 PE8 -> PE7 | CN10-18 -> CN10-20 |
//! | Z12 | UART7 PA15 -> PB3 | CN7-9 -> CN7-15 |
//!
//! The SPI links run each SPI's MOSI into its MISO (runs F to H):
//!
//! | Link | MOSI -> MISO | Zio sockets |
//! | --- | --- | --- |
//! | S1 | SPI1 PD7 -> PA6 | CN9-2 -> CN7-12 |
//! | S2 | SPI1 or SPI3 PB5 -> PB4 | CN7-14 -> CN7-19 |
//! | S3 | SPI2 PC3 -> PC2 | CN9-5 -> CN9-9 |
//! | S4 | SPI4 PE14 -> PE13 | CN10-8 -> CN10-10 |
//! | S5 | SPI4 PE6 -> PE5 | CN9-20 -> CN9-18 |
//! | S6 | SPI5 PF9 -> PF8 | CN9-28 -> CN9-24 |
//!
//! Every SPI's SCK is also routed to EXTI, which counts its edges. Run F's
//! EXTI checks drive PD5 into PD6 over Z1.
//!
//! UART5's PB13 is not on the Zio header: CN7-5 reaches it through SB7,
//! which is not fitted (it stays off while PB13 serves the Ethernet PHY).
//!
//! Run S talks to no jumper on this board: it drives I2C4 out to the
//! Pico 1 simulator, whose wiring list is `hil/WIRING.md`.
//!
//! Each run starts from reset, so the pins a run does not use stay inputs
//! and the links on them carry nothing. Across the runs every DMA1 and DMA2
//! stream serves a UART at least once.

// Every run binary compiles this file and uses its own run's wiring only.
#![allow(dead_code)]

use core::{
    ptr::addr_of_mut,
    sync::atomic::{AtomicBool, Ordering},
};

use ferrohal_stm32::{
    chip::{
        I2C3, I2C4, Peripherals, SPI1, SPI2, SPI3, SPI4, SPI5, TIM1, TIM2, TIM3, TIM4, TIM5, TIM6,
        TIM7, TIM8, TIM12, TIM13, TIM14, TIM15, TIM16, TIM17, UART4, UART5, UART7, USART1, USART2,
        USART3, USART6,
    },
    dma::Stream,
    exti,
    gpio::{Alternate, Ctrl, Output, Pin, Port, PortInstance, Pull, Speed},
    i2c,
    rcc::{
        AhbPrescaler, ApbPrescaler, ClockError, Clocks, Config, Hse, HseMode, Pll, PllSource, Rcc,
        Supply, Sysclk, VoltageScale,
    },
    spi::{self},
    time::Hertz,
    timer::{Tick, Timebase},
    uart::{self, Parity, RxWiring, StopBits, Wiring},
};

/// The TBS Lucid H7's clock tree (400 MHz core, 200 MHz AXI and AHB, every
/// APB at 100 MHz), from HSI so it works on every board: 64 MHz / 4 = 16
/// MHz reference, * 50 = 800 MHz VCO (wide range), / 2 = 400 MHz.
pub const CLOCK_CONFIG: Config = Config {
    hse: None,
    pll1: Some(Pll {
        source: PllSource::Hsi,
        prediv: 4,
        mul: 50,
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
};

/// The frequencies `CLOCK_CONFIG` gives, worked out and checked while
/// compiling.
/// The IWDG's LSI, measured across a reset: 50 ms at the nominal 32 kHz
/// came back as 50.5 ms, so this board's LSI runs at 31.7 kHz.
pub const LSI_HZ: Hertz = Hertz::hz(31_700);

pub const CLOCKS: Clocks = match CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("CLOCK_CONFIG breaks the H753's limits"),
};

/// The same tree from HSE, as the Lucid runs it from its 8 MHz crystal: for
/// run A. The Nucleo's HSE is the ST-LINK's 8 MHz MCO (UM2407 section
/// 7.5.1), so bypass here. 8 MHz / 1 = 8 MHz reference, * 100 = 800 MHz
/// VCO, then as `CLOCK_CONFIG`. Run A checks the result equals `CLOCKS`.
pub const HSE_CLOCK_CONFIG: Config = Config {
    hse: Some(Hse {
        freq: ferrohal_stm32::time::Hertz::mhz(8),
        mode: HseMode::Bypass,
    }),
    pll1: Some(Pll {
        source: PllSource::Hse,
        prediv: 1,
        mul: 100,
        ..match CLOCK_CONFIG.pll1 {
            Some(pll) => pll,
            None => panic!("CLOCK_CONFIG has a PLL"),
        }
    }),
    ..CLOCK_CONFIG
};

type Af<const P: char, const N: u8, const F: u8> = Pin<P, N, Alternate<F>>;

/// The Lucid's frames, checked against its UARTs while compiling: CRSF to
/// the receiver, SBUS (inverted, receive-only) from the DJI connector, MSP
/// to the DisplayPort.
pub const CRSF: uart::Config = uart::Config::new(420_000);
pub const SBUS: uart::Config = uart::Config::new(100_000)
    .with_parity(Parity::Even)
    .with_stop_bits(StopBits::Two)
    .with_inverted_rx(true);
pub const MSP: uart::Config = uart::Config::new(115_200);
const _: () = assert!(ferrohal_stm32::uart::check::<USART6>(&CLOCKS, &CRSF).is_ok());
const _: () = assert!(ferrohal_stm32::uart::check::<USART1>(&CLOCKS, &SBUS).is_ok());
const _: () = assert!(ferrohal_stm32::uart::check::<USART3>(&CLOCKS, &MSP).is_ok());

// Run A: the Lucid's ports, on its DMA streams. USART6 is on the Lucid's
// pins; USART1 and USART3 are on pins the Zio headers carry.

/// USART6 over Z2: DMA1 stream 6 transmits, stream 0 receives (the Lucid's).
pub type Usart6Lucid = Wiring<USART6, Af<'C', 6, 7>, Af<'C', 7, 7>, Stream<1, 6>, Stream<1, 0>>;
/// USART1 over Z3: DMA2 stream 5 transmits, stream 4 receives (the Lucid's).
pub type Usart1Lucid = Wiring<USART1, Af<'B', 6, 7>, Af<'B', 7, 7>, Stream<2, 5>, Stream<2, 4>>;
/// USART3 over Z7: DMA1 stream 3 transmits, stream 1 receives (the Lucid's).
pub type Usart3Lucid = Wiring<USART3, Af<'B', 10, 7>, Af<'B', 11, 7>, Stream<1, 3>, Stream<1, 1>>;
/// USART2 over Z1, the control: DMA1 stream 4 transmits, stream 2 receives.
pub type Usart2Control = Wiring<USART2, Af<'D', 5, 7>, Af<'D', 6, 7>, Stream<1, 4>, Stream<1, 2>>;

// Run B: the other instances, on the streams run A leaves free.

pub type Uart4B = Wiring<UART4, Af<'B', 9, 8>, Af<'B', 8, 8>, Stream<1, 7>, Stream<1, 5>>;
pub type Uart5B = Wiring<UART5, Af<'C', 12, 8>, Af<'D', 2, 8>, Stream<2, 1>, Stream<2, 0>>;
pub type Uart7B = Wiring<UART7, Af<'F', 7, 7>, Af<'F', 6, 7>, Stream<2, 3>, Stream<2, 2>>;
pub type Usart3B = Wiring<USART3, Af<'C', 10, 7>, Af<'C', 11, 7>, Stream<2, 7>, Stream<2, 6>>;

// Run C: alternate pins, and a UART whose two streams are on different
// controllers.

pub type Uart4C = Wiring<UART4, Af<'D', 1, 8>, Af<'D', 0, 8>, Stream<2, 0>, Stream<2, 7>>;
pub type Uart7C = Wiring<UART7, Af<'E', 8, 7>, Af<'E', 7, 7>, Stream<2, 5>, Stream<1, 5>>;

// Run D: UART4 on the PC10/PC11 link USART3 uses in run B, and UART7 on its
// JTAG pins (the probe uses SWD only).

pub type Uart4D = Wiring<UART4, Af<'C', 10, 8>, Af<'C', 11, 8>, Stream<1, 2>, Stream<2, 1>>;
pub type Uart7D = Wiring<UART7, Af<'A', 15, 11>, Af<'B', 3, 11>, Stream<2, 6>, Stream<1, 3>>;

// Run Y: USART3 into UART4 over Z6, the one link two different UARTs reach:
// PC10 carries USART3's TX, PC11 UART4's RX. USART3 takes its RX on PB11
// (Z7) and UART4 its TX on PD1 (Z5); nothing listens on those here.

pub type Usart3Y = Wiring<USART3, Af<'C', 10, 7>, Af<'B', 11, 7>, Stream<2, 7>, Stream<2, 6>>;
pub type Uart4Y = Wiring<UART4, Af<'D', 1, 8>, Af<'C', 11, 8>, Stream<1, 7>, Stream<1, 5>>;

/// LD1, the green user LED.
pub type Led = Pin<'B', 0, Output>;

// Run E: receive-only UARTs, fed by bit-banged frames from the other end of
// their links: USART1 as the Lucid's SBUS input, inverted; UART7 plain.

/// USART1's RX over Z3, on the Lucid's DMA2 stream 4.
pub type Usart1Sbus = RxWiring<USART1, Af<'B', 7, 7>, Stream<2, 4>>;
/// UART7's RX over Z12; DMA1 stream 3 receives.
pub type Uart7RxOnly = RxWiring<UART7, Af<'B', 3, 11>, Stream<1, 3>>;

pub struct RunE {
    pub rcc: Rcc,
    pub led: Led,
    pub usart1: Usart1Sbus,
    /// PB6, Z3's other end.
    pub feed1: Pin<'B', 6, Output>,
    pub uart7: Uart7RxOnly,
    /// PA15, Z12's other end.
    pub feed7: Pin<'A', 15, Output>,
}

pub fn run_e(p: Peripherals) -> Result<RunE, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    // An inverted line idles low, where `into_push_pull_output` leaves it.
    let feed1 = b.p6.into_push_pull_output(&mut b.ctrl);
    let mut feed7 = a.p15.into_push_pull_output(&mut a.ctrl);
    feed7.set_high();
    let rx1 = rx_pin(b.p7, &mut b.ctrl);
    let rx7 = rx_pin(b.p3, &mut b.ctrl);
    Ok(RunE {
        rcc,
        led,
        usart1: RxWiring {
            uart: p.USART1,
            rx: rx1,
            rx_dma: dma2.s4,
        },
        feed1,
        uart7: RxWiring {
            uart: p.UART7,
            rx: rx7,
            rx_dma: dma1.s3,
        },
        feed7,
    })
}

// Runs F to H: SPI over the S links. Through DMAMUX any stream serves any
// SPI; the runs spread them over both controllers.

/// The SPI links' normal setting: 10 MHz at most, so 6.25 MHz on every SPI
/// (SPI1 to SPI3 from PLL1Q at 100 MHz, SPI4 and SPI5 from PCLK2, / 16).
pub const SPI: spi::Config = spi::Config::new(Hertz::mhz(10));
const _: () = assert!(matches!(
    ferrohal_stm32::spi::check::<SPI1>(&CLOCKS, &SPI),
    Ok(Hertz(6_250_000))
));
const _: () = assert!(matches!(
    ferrohal_stm32::spi::check::<SPI4>(&CLOCKS, &SPI),
    Ok(Hertz(6_250_000))
));

type Af5<const P: char, const N: u8> = Pin<P, N, Alternate<5>>;
type Af6<const P: char, const N: u8> = Pin<P, N, Alternate<6>>;
type Af7<const P: char, const N: u8> = Pin<P, N, Alternate<7>>;

/// SPI1 on the Lucid's IMU pins and streams (DMA1 5 and 4), over S1.
pub type Spi1Lucid =
    spi::Wiring<SPI1, Af5<'A', 5>, Af5<'A', 6>, Af5<'D', 7>, Stream<1, 5>, Stream<1, 4>>;
/// SPI2, SCK on PB10, over S3.
pub type Spi2Pb10 =
    spi::Wiring<SPI2, Af5<'B', 10>, Af5<'C', 2>, Af5<'C', 3>, Stream<2, 0>, Stream<2, 1>>;
/// SPI4 on PE12 to PE14, over S4.
pub type Spi4High =
    spi::Wiring<SPI4, Af5<'E', 12>, Af5<'E', 13>, Af5<'E', 14>, Stream<2, 2>, Stream<2, 3>>;
/// SPI5, over S6.
pub type Spi5 =
    spi::Wiring<SPI5, Af5<'F', 7>, Af5<'F', 8>, Af5<'F', 9>, Stream<2, 4>, Stream<2, 5>>;
/// SPI1 on port B, over S2.
pub type Spi1PortB =
    spi::Wiring<SPI1, Af5<'B', 3>, Af5<'B', 4>, Af5<'B', 5>, Stream<1, 0>, Stream<1, 1>>;
/// SPI4 on PE2, PE5, PE6, over S5.
pub type Spi4Low =
    spi::Wiring<SPI4, Af5<'E', 2>, Af5<'E', 5>, Af5<'E', 6>, Stream<1, 2>, Stream<1, 3>>;
/// SPI3 on port B (MOSI in function 7), over S2.
pub type Spi3PortB =
    spi::Wiring<SPI3, Af6<'B', 3>, Af6<'B', 4>, Af7<'B', 5>, Stream<2, 6>, Stream<2, 7>>;
/// SPI2, SCK on PD3, over S3.
pub type Spi2Pd3 =
    spi::Wiring<SPI2, Af5<'D', 3>, Af5<'C', 2>, Af5<'C', 3>, Stream<1, 6>, Stream<1, 7>>;

pub struct RunF {
    pub rcc: Rcc,
    pub led: Led,
    pub exti: exti::Parts,
    pub spi1: Spi1Lucid,
    pub spi2: Spi2Pb10,
    pub spi4: Spi4High,
    pub spi5: Spi5,
    /// PD5, driving PD6 over Z1 for the EXTI checks.
    pub feed: Pin<'D', 5, Output>,
}

pub struct RunG {
    pub rcc: Rcc,
    pub led: Led,
    pub exti: exti::Parts,
    pub spi1: Spi1PortB,
    pub spi4: Spi4Low,
}

pub struct RunH {
    pub rcc: Rcc,
    pub led: Led,
    pub exti: exti::Parts,
    pub spi3: Spi3PortB,
    pub spi2: Spi2Pd3,
}

pub fn run_f(p: Peripherals) -> Result<RunF, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let mut e = p.GPIOE.split(&mut rcc);
    let mut f = p.GPIOF.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    let mut feed = d.p5.into_push_pull_output(&mut d.ctrl);
    feed.set_low();
    let mut sense = d.p6.into_input(&mut d.ctrl);
    sense.set_pull(&mut d.ctrl, Pull::Down);
    Ok(RunF {
        led,
        exti,
        spi1: spi::Wiring {
            spi: p.SPI1,
            sck: sck(a.p5, &mut a.ctrl),
            miso: miso(a.p6, &mut a.ctrl),
            mosi: sck(d.p7, &mut d.ctrl),
            tx_dma: dma1.s5,
            rx_dma: dma1.s4,
        },
        spi2: spi::Wiring {
            spi: p.SPI2,
            sck: sck(b.p10, &mut b.ctrl),
            miso: miso(c.p2, &mut c.ctrl),
            mosi: sck(c.p3, &mut c.ctrl),
            tx_dma: dma2.s0,
            rx_dma: dma2.s1,
        },
        spi4: spi::Wiring {
            spi: p.SPI4,
            sck: sck(e.p12, &mut e.ctrl),
            miso: miso(e.p13, &mut e.ctrl),
            mosi: sck(e.p14, &mut e.ctrl),
            tx_dma: dma2.s2,
            rx_dma: dma2.s3,
        },
        spi5: spi::Wiring {
            spi: p.SPI5,
            sck: sck(f.p7, &mut f.ctrl),
            miso: miso(f.p8, &mut f.ctrl),
            mosi: sck(f.p9, &mut f.ctrl),
            tx_dma: dma2.s4,
            rx_dma: dma2.s5,
        },
        feed,
        rcc,
    })
}

pub fn run_g(p: Peripherals) -> Result<RunG, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut e = p.GPIOE.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    Ok(RunG {
        led,
        exti,
        spi1: spi::Wiring {
            spi: p.SPI1,
            sck: sck(b.p3, &mut b.ctrl),
            miso: miso(b.p4, &mut b.ctrl),
            mosi: sck(b.p5, &mut b.ctrl),
            tx_dma: dma1.s0,
            rx_dma: dma1.s1,
        },
        spi4: spi::Wiring {
            spi: p.SPI4,
            sck: sck(e.p2, &mut e.ctrl),
            miso: miso(e.p5, &mut e.ctrl),
            mosi: sck(e.p6, &mut e.ctrl),
            tx_dma: dma1.s2,
            rx_dma: dma1.s3,
        },
        rcc,
    })
}

pub fn run_h(p: Peripherals) -> Result<RunH, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    Ok(RunH {
        led,
        exti,
        spi3: spi::Wiring {
            spi: p.SPI3,
            sck: sck(b.p3, &mut b.ctrl),
            miso: miso(b.p4, &mut b.ctrl),
            mosi: sck(b.p5, &mut b.ctrl),
            tx_dma: dma2.s6,
            rx_dma: dma2.s7,
        },
        spi2: spi::Wiring {
            spi: p.SPI2,
            sck: sck(d.p3, &mut d.ctrl),
            miso: miso(c.p2, &mut c.ctrl),
            mosi: sck(c.p3, &mut c.ctrl),
            tx_dma: dma1.s6,
            rx_dma: dma1.s7,
        },
        rcc,
    })
}

// Run I: every timer, against the DWT. No links.

/// The Lucid's control loop rate on TIM4, and its microsecond timebase on
/// TIM2, checked while compiling.
pub const CONTROL_RATE: Hertz = Hertz::khz(8);
const _: () = assert!(Tick::<TIM4>::check(&CLOCKS, CONTROL_RATE).is_ok());
const _: () = assert!(Timebase::<TIM2>::check(&CLOCKS, Hertz::mhz(1)).is_ok());

/// The timers as options, so the checks can take each in turn.
pub struct RunI {
    pub rcc: Rcc,
    pub led: Led,
    pub tim1: Option<TIM1>,
    pub tim2: Option<TIM2>,
    pub tim3: Option<TIM3>,
    pub tim4: Option<TIM4>,
    pub tim5: Option<TIM5>,
    pub tim6: Option<TIM6>,
    pub tim7: Option<TIM7>,
    pub tim8: Option<TIM8>,
    pub tim12: Option<TIM12>,
    pub tim13: Option<TIM13>,
    pub tim14: Option<TIM14>,
    pub tim15: Option<TIM15>,
    pub tim16: Option<TIM16>,
    pub tim17: Option<TIM17>,
}

pub fn run_i(p: Peripherals) -> Result<RunI, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    Ok(RunI {
        rcc,
        led,
        tim1: Some(p.TIM1),
        tim2: Some(p.TIM2),
        tim3: Some(p.TIM3),
        tim4: Some(p.TIM4),
        tim5: Some(p.TIM5),
        tim6: Some(p.TIM6),
        tim7: Some(p.TIM7),
        tim8: Some(p.TIM8),
        tim12: Some(p.TIM12),
        tim13: Some(p.TIM13),
        tim14: Some(p.TIM14),
        tim15: Some(p.TIM15),
        tim16: Some(p.TIM16),
        tim17: Some(p.TIM17),
    })
}

// Run J: DShot-style compare lanes, sampled. No links.

/// The Lucid's motor lanes: TIM3 channels 3 and 4 on PB0 and PB1 (DMA2
/// streams 0 and 1, its streams), TIM5 channels 1 and 4 on PA0 and PA3
/// (DMA2 streams 2 and 3; the Lucid's motor 4 is channel 2 on PA1, which the
/// Nucleo's Ethernet PHY drives). Samplers: TIM8's update request reads port
/// B on DMA1 stream 0, TIM2's port A on DMA1 stream 1. TIM1 drives CH3N on
/// PB15 as plain PWM, for the complementary output.
pub struct RunJ {
    pub rcc: Rcc,
    pub tim3: TIM3,
    pub pb0: Pin<'B', 0, Alternate<2>>,
    pub pb1: Pin<'B', 1, Alternate<2>>,
    pub tim3_lanes: (Stream<2, 0>, Stream<2, 1>),
    pub tim5: TIM5,
    pub pa0: Pin<'A', 0, Alternate<2>>,
    pub pa3: Pin<'A', 3, Alternate<2>>,
    pub tim5_lanes: (Stream<2, 2>, Stream<2, 3>),
    pub tim8: TIM8,
    pub port_b: Stream<1, 0>,
    pub tim2: TIM2,
    pub port_a: Stream<1, 1>,
    pub tim1: TIM1,
    pub pb15: Pin<'B', 15, Alternate<1>>,
}

pub fn run_j(p: Peripherals) -> Result<RunJ, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    Ok(RunJ {
        tim3: p.TIM3,
        pb0: sck(b.p0, &mut b.ctrl),
        pb1: sck(b.p1, &mut b.ctrl),
        tim3_lanes: (dma2.s0, dma2.s1),
        tim5: p.TIM5,
        pa0: sck(a.p0, &mut a.ctrl),
        pa3: sck(a.p3, &mut a.ctrl),
        tim5_lanes: (dma2.s2, dma2.s3),
        tim8: p.TIM8,
        port_b: dma1.s0,
        tim2: p.TIM2,
        port_a: dma1.s1,
        tim1: p.TIM1,
        pb15: sck(b.p15, &mut b.ctrl),
        rcc,
    })
}

// Run Q: the waveform generator both ways over one wire, Z2.

/// TIM3 channel 1 on PC6 and TIM8 channel 2 on PC7, the two ends of Z2, each on its
/// own DMA2 stream, both reading port C. Pull-ups hold the line while neither
/// drives it.
pub struct RunQ {
    pub rcc: Rcc,
    pub tim3: TIM3,
    pub pc6: Pin<'C', 6, Alternate<2>>,
    pub near_lane: Stream<2, 4>,
    pub tim8: TIM8,
    pub pc7: Pin<'C', 7, Alternate<3>>,
    pub far_lane: Stream<2, 5>,
}

pub fn run_q(p: Peripherals) -> Result<RunQ, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut c = p.GPIOC.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let mut pc6: Pin<'C', 6, Alternate<2>> = sck(c.p6, &mut c.ctrl);
    pc6.set_pull(&mut c.ctrl, ferrohal_stm32::gpio::Pull::Up);
    let mut pc7: Pin<'C', 7, Alternate<3>> = sck(c.p7, &mut c.ctrl);
    pc7.set_pull(&mut c.ctrl, ferrohal_stm32::gpio::Pull::Up);
    Ok(RunQ {
        tim3: p.TIM3,
        pc6,
        near_lane: dma2.s4,
        tim8: p.TIM8,
        pc7,
        far_lane: dma2.s5,
        rcc,
    })
}

// Run R: the I2C, I2C3 on PA8 and PC9, pulled up internally, no slave
// fitted. No links: these pins carry no other run's wiring, so the
// bench runs as one script.

pub type I2c3Bus = i2c::Wiring<I2C3, Pin<'A', 8, Alternate<4>>, Pin<'C', 9, Alternate<4>>>;

pub struct RunR {
    pub rcc: Rcc,
    pub led: Pin<'A', 5, Output>,
    pub i2c: I2c3Bus,
}

pub fn run_r(p: Peripherals) -> Result<RunR, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    // SCL and SDA pulled up: with no slave fitted the lines idle high,
    // so a vacant address is answered by the bus itself with a NACK.
    let mut scl: Pin<'A', 8, Alternate<4>> = a.p8.into_alternate_open_drain(&mut a.ctrl);
    scl.set_pull(&mut a.ctrl, Pull::Up);
    let mut sda: Pin<'C', 9, Alternate<4>> = c.p9.into_alternate_open_drain(&mut c.ctrl);
    sda.set_pull(&mut c.ctrl, Pull::Up);
    Ok(RunR {
        rcc,
        led,
        i2c: i2c::Wiring {
            i2c: p.I2C3,
            scl,
            sda,
        },
    })
}

// Run K: the independent watchdog, across the reset it causes. No links.

pub struct RunK {
    pub rcc: Rcc,
    pub led: Led,
    pub iwdg: ferrohal_stm32::chip::IWDG1,
}

pub fn run_k(p: Peripherals) -> Result<RunK, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    Ok(RunK {
        rcc,
        led,
        iwdg: p.IWDG1,
    })
}

// Run L: the polled SPI (no DMA), over S3 and S4.

pub struct RunL {
    pub rcc: Rcc,
    pub led: Led,
    pub exti: exti::Parts,
    pub spi2: spi::BlockingWiring<SPI2, Af5<'B', 10>, Af5<'C', 2>, Af5<'C', 3>>,
    pub spi4: spi::BlockingWiring<SPI4, Af5<'E', 12>, Af5<'E', 13>, Af5<'E', 14>>,
}

pub fn run_l(p: Peripherals) -> Result<RunL, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut e = p.GPIOE.split(&mut rcc);
    let exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    Ok(RunL {
        led,
        exti,
        spi2: spi::BlockingWiring {
            spi: p.SPI2,
            sck: sck(b.p10, &mut b.ctrl),
            miso: miso(c.p2, &mut c.ctrl),
            mosi: sck(c.p3, &mut c.ctrl),
        },
        spi4: spi::BlockingWiring {
            spi: p.SPI4,
            sck: sck(e.p12, &mut e.ctrl),
            miso: miso(e.p13, &mut e.ctrl),
            mosi: sck(e.p14, &mut e.ctrl),
        },
        rcc,
    })
}

// Run M: USB CDC, through the user USB connector (CN13) and a cable to the
// host. PA11 and PA12 reach CN13 through SB21 and SB22 (UM2407 table 14).

// Run O: the ADCs, PD7 driving PA6 over S1 and PF7 driving PF6 over Z10.

pub struct RunO {
    pub rcc: Rcc,
    pub led: Led,
    pub adc1: ferrohal_stm32::chip::ADC1,
    pub analog1: Pin<'A', 6, ferrohal_stm32::gpio::Analog>,
    pub drive1: Pin<'D', 7, Output>,
    pub stream1: Stream<1, 0>,
    pub adc3: ferrohal_stm32::chip::ADC3,
    pub analog3: Pin<'F', 6, ferrohal_stm32::gpio::Analog>,
    pub drive3: Pin<'F', 7, Output>,
    pub stream3: Stream<2, 0>,
}

pub fn run_o(p: Peripherals) -> Result<RunO, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let mut f = p.GPIOF.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    let analog1 = a.p6.into_analog(&mut a.ctrl);
    let drive1 = d.p7.into_push_pull_output(&mut d.ctrl);
    let analog3 = f.p6.into_analog(&mut f.ctrl);
    let drive3 = f.p7.into_push_pull_output(&mut f.ctrl);
    Ok(RunO {
        rcc,
        led,
        adc1: p.ADC1,
        analog1,
        drive1,
        stream1: dma1.s0,
        adc3: p.ADC3,
        analog3,
        drive3,
        stream3: dma2.s0,
    })
}

pub struct RunM {
    pub rcc: Rcc,
    pub led: Led,
    pub usb: ferrohal_stm32::chip::USB_OTG_FS,
    pub dm: Pin<'A', 11, Alternate<10>>,
    pub dp: Pin<'A', 12, Alternate<10>>,
    /// CN13's VBUS, through SB23: high with a cable in.
    pub vbus: Pin<'A', 9>,
    pub tim2: TIM2,
}

pub fn run_m(p: Peripherals) -> Result<RunM, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    let mut dm = a.p11.into_alternate::<10>(&mut a.ctrl);
    dm.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let mut dp = a.p12.into_alternate::<10>(&mut a.ctrl);
    dp.set_speed(&mut a.ctrl, Speed::VeryHigh);
    // No pull: the board drives PA9 through a divider a pull would load.
    let vbus = a.p9.into_input(&mut a.ctrl);
    Ok(RunM {
        rcc,
        led,
        usb: p.USB_OTG_FS,
        dm,
        dp,
        vbus,
        tim2: p.TIM2,
    })
}

/// An SCK or MOSI pin: driven, at high speed for the edges.
fn sck<const P: char, const N: u8, const AF: u8>(
    pin: Pin<P, N>,
    ctrl: &mut Ctrl<P>,
) -> Pin<P, N, Alternate<AF>>
where
    Port<P>: PortInstance,
{
    let mut pin = pin.into_alternate::<AF>(ctrl);
    pin.set_speed(ctrl, Speed::High);
    pin
}

/// A MISO pin, pulled down: without its link it reads zeros, and the check
/// reports a mismatch rather than noise.
fn miso<const P: char, const N: u8, const AF: u8>(
    pin: Pin<P, N>,
    ctrl: &mut Ctrl<P>,
) -> Pin<P, N, Alternate<AF>>
where
    Port<P>: PortInstance,
{
    let mut pin = pin.into_alternate::<AF>(ctrl);
    pin.set_pull(ctrl, Pull::Down);
    pin
}

/// Opens one UART of a run on its buffers, or stops the run saying which.
pub fn open<U, TxP, RxP, TxD, RxD>(
    rcc: &mut Rcc,
    name: &str,
    wiring: Wiring<U, TxP, RxP, TxD, RxD>,
    config: &ferrohal_stm32::uart::Config,
    buffers: DmaBuffers,
) -> (
    ferrohal_stm32::uart::Uart<Wiring<U, TxP, RxP, TxD, RxD>>,
    ferrohal_stm32::uart::UartIrq,
)
where
    U: ferrohal_stm32::uart::Instance,
    TxP: ferrohal_stm32::uart::TxPin<U>,
    RxP: ferrohal_stm32::uart::RxPin<U>,
    TxD: ferrohal_stm32::uart::TxDma<U>,
    RxD: ferrohal_stm32::uart::RxDma<U>,
{
    let buffers = ferrohal_stm32::uart::Buffers {
        tx: buffers.tx,
        rx: buffers.rx,
    };
    match ferrohal_stm32::uart::Uart::new(rcc, wiring, config, buffers) {
        Ok(parts) => parts,
        Err(error) => defmt::panic!("{=str}: {}", name, error),
    }
}

pub struct RunA {
    pub rcc: Rcc,
    pub led: Led,
    pub usart6: Usart6Lucid,
    pub usart1: Usart1Lucid,
    pub usart3: Usart3Lucid,
    pub usart2: Usart2Control,
}

pub struct RunB {
    pub rcc: Rcc,
    pub led: Led,
    pub uart4: Uart4B,
    pub uart5: Uart5B,
    pub uart7: Uart7B,
    pub usart3: Usart3B,
}

// Run W: faults injected on run A's USART6 loopback, same link and streams.

pub struct RunW {
    pub rcc: Rcc,
    pub led: Led,
    pub usart6: Usart6Lucid,
}

pub fn run_w(p: Peripherals) -> Result<RunW, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&HSE_CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    let (tx6, rx6) = pins(c.p6, c.p7, &mut c.ctrl);
    Ok(RunW {
        rcc,
        led,
        usart6: Wiring {
            uart: p.USART6,
            tx: tx6,
            rx: rx6,
            tx_dma: dma1.s6,
            rx_dma: dma1.s0,
        },
    })
}

pub struct RunC {
    pub rcc: Rcc,
    pub led: Led,
    pub uart4: Uart4C,
    pub uart7: Uart7C,
}

pub struct RunD {
    pub rcc: Rcc,
    pub led: Led,
    pub uart4: Uart4D,
    pub uart7: Uart7D,
}

/// A TX and an RX pin of one port, in alternate function `F`. RX is pulled
/// up: without its link it reads an idle line, and the test reports
/// silence rather than noise.
fn pins<const P: char, const T: u8, const R: u8, const F: u8>(
    tx: Pin<P, T>,
    rx: Pin<P, R>,
    ctrl: &mut Ctrl<P>,
) -> (Af<P, T, F>, Af<P, R, F>)
where
    Port<P>: PortInstance,
{
    (tx_pin(tx, ctrl), rx_pin(rx, ctrl))
}

fn tx_pin<const P: char, const N: u8, const F: u8>(
    pin: Pin<P, N>,
    ctrl: &mut Ctrl<P>,
) -> Af<P, N, F>
where
    Port<P>: PortInstance,
{
    let mut pin = pin.into_alternate::<F>(ctrl);
    pin.set_speed(ctrl, Speed::Medium);
    pin
}

fn rx_pin<const P: char, const N: u8, const F: u8>(
    pin: Pin<P, N>,
    ctrl: &mut Ctrl<P>,
) -> Af<P, N, F>
where
    Port<P>: PortInstance,
{
    let mut pin = pin.into_alternate::<F>(ctrl);
    pin.set_pull(ctrl, Pull::Up);
    pin
}

pub fn run_a(p: Peripherals) -> Result<RunA, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&HSE_CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    let (tx6, rx6) = pins(c.p6, c.p7, &mut c.ctrl);
    let (tx1, rx1) = pins(b.p6, b.p7, &mut b.ctrl);
    let (tx3, rx3) = pins(b.p10, b.p11, &mut b.ctrl);
    let (tx2, rx2) = pins(d.p5, d.p6, &mut d.ctrl);
    Ok(RunA {
        rcc,
        led,
        usart6: Wiring {
            uart: p.USART6,
            tx: tx6,
            rx: rx6,
            tx_dma: dma1.s6,
            rx_dma: dma1.s0,
        },
        usart1: Wiring {
            uart: p.USART1,
            tx: tx1,
            rx: rx1,
            tx_dma: dma2.s5,
            rx_dma: dma2.s4,
        },
        usart3: Wiring {
            uart: p.USART3,
            tx: tx3,
            rx: rx3,
            tx_dma: dma1.s3,
            rx_dma: dma1.s1,
        },
        usart2: Wiring {
            uart: p.USART2,
            tx: tx2,
            rx: rx2,
            tx_dma: dma1.s4,
            rx_dma: dma1.s2,
        },
    })
}

pub fn run_b(p: Peripherals) -> Result<RunB, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let mut f = p.GPIOF.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    let (tx4, rx4) = pins(b.p9, b.p8, &mut b.ctrl);
    let tx5 = tx_pin(c.p12, &mut c.ctrl);
    let rx5 = rx_pin(d.p2, &mut d.ctrl);
    let (tx7, rx7) = pins(f.p7, f.p6, &mut f.ctrl);
    let (tx3, rx3) = pins(c.p10, c.p11, &mut c.ctrl);
    Ok(RunB {
        rcc,
        led,
        uart4: Wiring {
            uart: p.UART4,
            tx: tx4,
            rx: rx4,
            tx_dma: dma1.s7,
            rx_dma: dma1.s5,
        },
        uart5: Wiring {
            uart: p.UART5,
            tx: tx5,
            rx: rx5,
            tx_dma: dma2.s1,
            rx_dma: dma2.s0,
        },
        uart7: Wiring {
            uart: p.UART7,
            tx: tx7,
            rx: rx7,
            tx_dma: dma2.s3,
            rx_dma: dma2.s2,
        },
        usart3: Wiring {
            uart: p.USART3,
            tx: tx3,
            rx: rx3,
            tx_dma: dma2.s7,
            rx_dma: dma2.s6,
        },
    })
}

pub fn run_c(p: Peripherals) -> Result<RunC, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let mut e = p.GPIOE.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    let (tx4, rx4) = pins(d.p1, d.p0, &mut d.ctrl);
    let (tx7, rx7) = pins(e.p8, e.p7, &mut e.ctrl);
    Ok(RunC {
        rcc,
        led,
        uart4: Wiring {
            uart: p.UART4,
            tx: tx4,
            rx: rx4,
            tx_dma: dma2.s0,
            rx_dma: dma2.s7,
        },
        uart7: Wiring {
            uart: p.UART7,
            tx: tx7,
            rx: rx7,
            tx_dma: dma2.s5,
            rx_dma: dma1.s5,
        },
    })
}

pub fn run_d(p: Peripherals) -> Result<RunD, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    let (tx4, rx4) = pins(c.p10, c.p11, &mut c.ctrl);
    let tx7 = tx_pin(a.p15, &mut a.ctrl);
    let rx7 = rx_pin(b.p3, &mut b.ctrl);
    Ok(RunD {
        rcc,
        led,
        uart4: Wiring {
            uart: p.UART4,
            tx: tx4,
            rx: rx4,
            tx_dma: dma1.s2,
            rx_dma: dma2.s1,
        },
        uart7: Wiring {
            uart: p.UART7,
            tx: tx7,
            rx: rx7,
            tx_dma: dma2.s6,
            rx_dma: dma1.s3,
        },
    })
}

pub struct RunY {
    pub rcc: Rcc,
    pub led: Led,
    pub usart3: Usart3Y,
    pub uart4: Uart4Y,
}

pub fn run_y(p: Peripherals) -> Result<RunY, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = b.p0.into_push_pull_output(&mut b.ctrl);
    let tx3 = tx_pin(c.p10, &mut c.ctrl);
    let rx4 = rx_pin(c.p11, &mut c.ctrl);
    let rx3 = rx_pin(b.p11, &mut b.ctrl);
    let tx4 = tx_pin(d.p1, &mut d.ctrl);
    Ok(RunY {
        rcc,
        led,
        usart3: Wiring {
            uart: p.USART3,
            tx: tx3,
            rx: rx3,
            tx_dma: dma2.s7,
            rx_dma: dma2.s6,
        },
        uart4: Wiring {
            uart: p.UART4,
            tx: tx4,
            rx: rx4,
            tx_dma: dma1.s7,
            rx_dma: dma1.s5,
        },
    })
}

/// The DMA buffers' block: the top 16 KiB of AXI SRAM, `DMA_NC` in
/// memory.x.
const DMA_NC_BASE: u32 = 0x2407_C000;

/// Makes the DMA buffer block non-cacheable with MPU region 0. The caches
/// stay as they are. The MPU is the firmware's: FerroHAL only checks that
/// its buffers sit in such a block.
pub fn dma_region(core: &mut cortex_m::Peripherals) {
    const ENABLE: u32 = 1;
    // 2^(13 + 1) bytes.
    const SIZE_16K: u32 = 13 << 1;
    // TEX 001, C 0, B 0: normal memory, not cached.
    const NORMAL_NON_CACHEABLE: u32 = 0b001 << 19;
    const SHAREABLE: u32 = 1 << 18;
    const FULL_ACCESS: u32 = 0b011 << 24;
    const EXECUTE_NEVER: u32 = 1 << 28;
    // PRIVDEFENA: everything outside the region keeps the default map.
    const DEFAULT_MAP: u32 = 1 << 2;
    // SAFETY: init runs before anything uses the region, with the MPU off
    // while it is programmed.
    unsafe {
        core.MPU.ctrl.write(0);
        core.MPU.rnr.write(0);
        core.MPU.rbar.write(DMA_NC_BASE);
        core.MPU.rasr.write(
            EXECUTE_NEVER | FULL_ACCESS | NORMAL_NON_CACHEABLE | SHAREABLE | SIZE_16K | ENABLE,
        );
        core.MPU.ctrl.write(DEFAULT_MAP | ENABLE);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

/// Turns the instruction and data caches on.
pub fn caches_on(scb: &mut cortex_m::peripheral::SCB, cpuid: &mut cortex_m::peripheral::CPUID) {
    scb.enable_icache();
    scb.enable_dcache(cpuid);
}

pub const TX_BUFFER: usize = 256;
pub const RX_RING: usize = 256;
/// The most UARTs one run opens.
const UARTS: usize = 4;

#[unsafe(link_section = ".dma_buffers")]
static mut TX: [[u8; TX_BUFFER]; UARTS] = [[0; TX_BUFFER]; UARTS];
#[unsafe(link_section = ".dma_buffers")]
static mut RX: [[u8; RX_RING]; UARTS] = [[0; RX_RING]; UARTS];

/// One UART's buffers, in the non-cacheable block.
pub struct DmaBuffers {
    pub tx: &'static mut [u8],
    pub rx: &'static mut [u8],
}

/// Every UART's buffers. Call once.
pub fn dma_buffers() -> [DmaBuffers; UARTS] {
    static TAKEN: AtomicBool = AtomicBool::new(false);
    if TAKEN.swap(true, Ordering::AcqRel) {
        defmt::panic!("DMA buffers taken twice");
    }
    // SAFETY: the flag makes these the only references. The section is not
    // loaded, so the bytes are cleared here first.
    let (tx, rx) = unsafe { (&mut *addr_of_mut!(TX), &mut *addr_of_mut!(RX)) };
    let mut tx = tx.iter_mut();
    let mut rx = rx.iter_mut();
    core::array::from_fn(|_| {
        let (tx, rx) = match (tx.next(), rx.next()) {
            (Some(tx), Some(rx)) => (tx, rx),
            _ => defmt::unreachable!(),
        };
        tx.fill(0);
        rx.fill(0);
        DmaBuffers {
            tx: &mut tx[..],
            rx: &mut rx[..],
        }
    })
}

// Run S: the I2C master against the Pico 1 simulator, I2C4 on PD12
// (CN10-21) and PD13 (CN10-19), pulled up internally. This is the only
// I2C of the H753ZI a bench can wire — the others' pins are taken by
// the ST-LINK's MCO (PF0), the unfitted morpho headers (PA8) or this
// bench's own UART links (PB6/PB7, PB9/PB8, PB10/PB11); hil/WIRING.md
// has the accounting — and the one whose kernel clock is the APB4
// path, which the run checks against the tree. The simulator answers
// at 0x2A with bus tag 2.

pub type I2c4Bus = i2c::Wiring<I2C4, Pin<'D', 12, Alternate<4>>, Pin<'D', 13, Alternate<4>>>;

pub struct RunS {
    pub rcc: Rcc,
    pub led: Pin<'A', 5, Output>,
    pub i2c: I2c4Bus,
}

pub fn run_s(p: Peripherals) -> Result<RunS, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    // SCL and SDA pulled up here and at the Pico's end: the lines idle
    // high at 50 kHz with no resistor fitted.
    let mut scl: Pin<'D', 12, Alternate<4>> = d.p12.into_alternate_open_drain(&mut d.ctrl);
    scl.set_pull(&mut d.ctrl, Pull::Up);
    let mut sda: Pin<'D', 13, Alternate<4>> = d.p13.into_alternate_open_drain(&mut d.ctrl);
    sda.set_pull(&mut d.ctrl, Pull::Up);
    Ok(RunS {
        rcc,
        led,
        i2c: i2c::Wiring {
            i2c: p.I2C4,
            scl,
            sda,
        },
    })
}

// Run U: the constructor refusals, as the F401's run V. USB is left out: on
// this chip the constructor starts its own 48 MHz source, so nothing on this
// bench can make it refuse.

/// A configuration that must refuse: `Pll1P` as the system clock with no
/// PLL1 configured at all.
pub const NO_PLL1_CONFIG: Config = Config {
    hse: None,
    pll1: None,
    sys: Sysclk::Pll1P,
    cpu_pre: AhbPrescaler::Div1,
    ahb_pre: AhbPrescaler::Div2,
    apb1_pre: ApbPrescaler::Div2,
    apb2_pre: ApbPrescaler::Div2,
    apb3_pre: ApbPrescaler::Div2,
    apb4_pre: ApbPrescaler::Div2,
    supply: Supply::Ldo,
    voltage_scale: VoltageScale::Scale1,
};
const _: () = assert!(matches!(
    NO_PLL1_CONFIG.validate(),
    Err(ClockError::PllNotConfigured)
));

/// USART6 over Z2, DMA1 stream 6 transmitting, stream 1 receiving (stream 0
/// is the TIM8 port-B sampler this run also needs; through DMAMUX any
/// stream serves any UART).
pub type Usart6Z2S1 = Wiring<USART6, Af<'C', 6, 7>, Af<'C', 7, 7>, Stream<1, 6>, Stream<1, 1>>;
/// SPI2 polled over S3, run L's wiring.
pub type Spi2PolledS3 = spi::BlockingWiring<SPI2, Af5<'B', 10>, Af5<'C', 2>, Af5<'C', 3>>;

pub struct RunU {
    pub rcc: Rcc,
    /// USART6 over Z2.
    pub usart6: Usart6Z2S1,
    /// SPI1 over S1.
    pub spi1: Spi1Lucid,
    /// SPI2 polled over S3.
    pub spi2_polled: Spi2PolledS3,
    /// I2C3 on PA8 and PC9, nothing on the bus.
    pub i2c3: I2c3Bus,
    /// ADC1; the scan reads PA1 twice (this chip's ADC has no internal
    /// channels in the driver).
    pub adc1: ferrohal_stm32::chip::ADC1,
    /// PA1 as the scan's analog pin.
    pub analog1: Pin<'A', 1, ferrohal_stm32::gpio::Analog>,
    /// The ADC scan's stream, DMA1 stream 7.
    pub scan_stream: Stream<1, 7>,
    /// TIM3's CH3 on PB0, run J's motor lane, on DMA2 stream 0; TIM5's CH1
    /// on PA0, on DMA2 stream 2; both read back by the TIM8 port-B sampler
    /// on DMA1 stream 0.
    pub tim3: TIM3,
    pub pb0: Pin<'B', 0, Alternate<2>>,
    pub lane3_stream: Stream<2, 0>,
    pub tim5: TIM5,
    pub pa0: Pin<'A', 0, Alternate<2>>,
    pub lane5_stream: Stream<2, 2>,
    pub tim8: TIM8,
    pub sampler_stream: Stream<1, 0>,
    /// TIM15 for the tick, TIM2 for the timebase.
    pub tim15: TIM15,
    pub tim2: TIM2,
    pub iwdg: ferrohal_stm32::chip::IWDG1,
}

pub fn run_u(p: Peripherals) -> Result<(RunU, ClockError), ClockError> {
    // The refusal first: a config naming Pll1P with no PLL1 must hand the
    // RCC token back, and that token freezes the bench's own clocks.
    let (refusal, rcc) = match p.RCC.freeze(&NO_PLL1_CONFIG) {
        Err(refused) => refused.into_inner(),
        Ok(_) => defmt::panic!("NO_PLL1_CONFIG was accepted"),
    };
    let mut rcc = rcc
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let analog1 = a.p1.into_analog(&mut a.ctrl);
    let (tx6, rx6) = pins(c.p6, c.p7, &mut c.ctrl);
    let mut scl: Pin<'A', 8, Alternate<4>> = a.p8.into_alternate_open_drain(&mut a.ctrl);
    scl.set_pull(&mut a.ctrl, Pull::Up);
    let mut sda: Pin<'C', 9, Alternate<4>> = c.p9.into_alternate_open_drain(&mut c.ctrl);
    sda.set_pull(&mut c.ctrl, Pull::Up);
    Ok((
        RunU {
            rcc,
            usart6: Wiring {
                uart: p.USART6,
                tx: tx6,
                rx: rx6,
                tx_dma: dma1.s6,
                rx_dma: dma1.s1,
            },
            spi1: spi::Wiring {
                spi: p.SPI1,
                sck: sck(a.p5, &mut a.ctrl),
                miso: miso(a.p6, &mut a.ctrl),
                mosi: sck(d.p7, &mut d.ctrl),
                tx_dma: dma1.s5,
                rx_dma: dma1.s4,
            },
            spi2_polled: spi::BlockingWiring {
                spi: p.SPI2,
                sck: sck(b.p10, &mut b.ctrl),
                miso: miso(c.p2, &mut c.ctrl),
                mosi: sck(c.p3, &mut c.ctrl),
            },
            i2c3: i2c::Wiring {
                i2c: p.I2C3,
                scl,
                sda,
            },
            adc1: p.ADC1,
            analog1,
            scan_stream: dma1.s7,
            tim3: p.TIM3,
            pb0: sck(b.p0, &mut b.ctrl),
            lane3_stream: dma2.s0,
            tim5: p.TIM5,
            pa0: sck(a.p0, &mut a.ctrl),
            lane5_stream: dma2.s2,
            tim8: p.TIM8,
            sampler_stream: dma1.s0,
            tim15: p.TIM15,
            tim2: p.TIM2,
            iwdg: p.IWDG1,
        },
        refusal,
    ))
}
