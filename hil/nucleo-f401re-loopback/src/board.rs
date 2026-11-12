//! Nucleo-F401RE (MB1136): the clock tree, and which pins and DMA streams
//! each UART uses in each test run. This file is chip-specific; a board on
//! another chip has its own. FerroHAL's routing traits check every choice
//! here at compile time: a pin without the UART's function, or a stream
//! that cannot serve its request, does not build.
//!
//! The bench (UM1724 tables 16 and 29; no solder bridge on any of these):
//!
//! | Link | From | To |
//! | --- | --- | --- |
//! | J1 | CN5-1 (D8, PA9) | CN9-3 (D2, PA10) |
//! | J2 | CN10-14 (PA11) | CN10-12 (PA12) |
//! | J3 | CN5-3 (D10, PB6) | CN5-2 (D9, PC7) |
//! | J4 | CN10-4 (PC6) | CN7-21 (PB7) |
//! | S1 | CN10-15 (PA7) | CN10-13 (PA6) |
//! | S2 | CN10-29 (PB5) | CN10-27 (PB4) |
//! | S3 | CN10-26 (PB15) | CN10-28 (PB14) |
//! | S4 | CN7-37 (PC3) | CN7-35 (PC2) |
//! | S5 | CN7-3 (PC12) | CN7-2 (PC11) |
//!
//! J1 to J4 are UART links. S1 to S5 run each SPI's MOSI into its MISO
//! (runs E to G); every SPI's SCK is also routed to EXTI, which counts its
//! edges. Run E's EXTI checks drive PA11 into PA12 over J2.
//!
//! Run S talks to no jumper on this board: it drives I2C3 out to the
//! Pico 1 simulator, whose wiring list is `hil/WIRING.md`.
//!
//! USART2 (PA2, PA3) needs no link: SB13 and SB14 wire it to the ST-LINK's
//! virtual COM port, where `hil/vcp_echo.py` sends every byte back.
//!
//! Each run starts from reset, so the pins a run does not use stay inputs
//! and the links on them carry nothing.

// Every run binary compiles this file and uses its own run's wiring only.
#![allow(dead_code)]

use ferrohal_stm32::{
    chip::{
        I2C1, I2C3, Peripherals, SPI1, SPI2, SPI3, TIM1, TIM2, TIM3, TIM4, TIM5, TIM9, TIM10,
        TIM11, USART1, USART2, USART6,
    },
    dma::Stream,
    exti,
    gpio::{Alternate, Ctrl, Input, Output, Parts, Pin, Port, PortInstance, Pull, Speed},
    i2c,
    rcc::{
        AhbPrescaler, ApbPrescaler, ClockError, Clocks, Config, Hse, HseMode, Pll, PllP, PllSource,
        Rcc, Supply, Sysclk,
    },
    spi::{self, Spi},
    time::Hertz,
    timer::{Tick, Timebase},
    uart::{self, ConfigError, Parity, RxWiring, StopBits, Uart, Wiring},
};

/// 84 MHz, the F401's ceiling, from HSI so it works on every board:
/// 16 MHz / 16 * 336 = 336 MHz VCO, / 4 = 84 MHz, / 7 = 48 MHz.
pub const CLOCK_CONFIG: Config = Config {
    hse: None,
    pll: Some(Pll {
        source: PllSource::Hsi,
        prediv: 16,
        mul: 336,
        divp: PllP::Div4,
        divq: Some(7),
    }),
    sys: Sysclk::PllP,
    ahb_pre: AhbPrescaler::Div1,
    // APB1 tops out at 42 MHz.
    apb1_pre: ApbPrescaler::Div2,
    apb2_pre: ApbPrescaler::Div1,
    supply: Supply::V2_7To3_6,
};

/// The frequencies `CLOCK_CONFIG` gives, worked out and checked while
/// compiling.
/// The IWDG's LSI, measured across a reset: 50 ms at the nominal 32 kHz
/// came back as 53.3 ms, so this board's LSI runs at 30.0 kHz.
pub const LSI_HZ: Hertz = Hertz::hz(30_000);

pub const CLOCKS: Clocks = match CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("CLOCK_CONFIG breaks the F401's limits"),
};

/// The same frequencies from HSE, for run A: the ST-LINK's 8 MHz MCO, which
/// MB1136 C-02 and later boards wire to PH0 (UM1724 section 7.9.1), in
/// bypass. 8 MHz / 8 * 336 = 336 MHz VCO, then as `CLOCK_CONFIG`. Run A
/// checks the result equals `CLOCKS`.
pub const HSE_CLOCK_CONFIG: Config = Config {
    hse: Some(Hse {
        freq: ferrohal_stm32::time::Hertz::mhz(8),
        mode: HseMode::Bypass,
    }),
    pll: Some(Pll {
        source: PllSource::Hse,
        prediv: 8,
        ..match CLOCK_CONFIG.pll {
            Some(pll) => pll,
            None => panic!("CLOCK_CONFIG has a PLL"),
        }
    }),
    ..CLOCK_CONFIG
};

/// Run D's frame: SBUS's 100000 8E2. The F401 has no inversion, so the
/// Foxeer F405 inverts SBUS in hardware; this checks the refusal while
/// compiling.
pub const SBUS_PLAIN: uart::Config = uart::Config::new(100_000)
    .with_parity(Parity::Even)
    .with_stop_bits(StopBits::Two);
const _: () = assert!(Uart::<USART6>::check(&CLOCKS, &SBUS_PLAIN).is_ok());
const _: () = assert!(matches!(
    Uart::<USART6>::check(&CLOCKS, &SBUS_PLAIN.with_inverted_rx(true)),
    Err(ConfigError::Unsupported)
));

/// USART1 and USART2 use alternate function 7, USART6 function 8.
type Af7<const P: char, const N: u8> = Pin<P, N, Alternate<7>>;
type Af8<const P: char, const N: u8> = Pin<P, N, Alternate<8>>;

// Run A: every UART on its own, all at once.

/// USART1 over J1. DMA2 stream 7 transmits, stream 2 receives.
pub type Usart1Loop = Wiring<USART1, Af7<'A', 9>, Af7<'A', 10>, Stream<2, 7>, Stream<2, 2>>;
/// USART6 over J2. DMA2 stream 6 transmits, stream 1 receives.
pub type Usart6Loop = Wiring<USART6, Af8<'A', 11>, Af8<'A', 12>, Stream<2, 6>, Stream<2, 1>>;
/// USART2 through the ST-LINK. DMA1 stream 6 transmits, stream 5 receives.
pub type Usart2Vcp = Wiring<USART2, Af7<'A', 2>, Af7<'A', 3>, Stream<1, 6>, Stream<1, 5>>;

// Run B: USART1 and USART6 wired to each other.

/// USART1 on its other pins: TX over J3 to USART6, RX over J4 from it.
/// DMA2 stream 7 transmits, stream 5 receives.
pub type Usart1Cross = Wiring<USART1, Af7<'B', 6>, Af7<'B', 7>, Stream<2, 7>, Stream<2, 5>>;
/// USART6 on its other pins: TX over J4 to USART1, RX over J3 from it.
/// DMA2 stream 6 transmits, stream 2 receives.
pub type Usart6Cross = Wiring<USART6, Af8<'C', 6>, Af8<'C', 7>, Stream<2, 6>, Stream<2, 2>>;

// Run C: USART6's other transmit stream, which USART1 holds in runs A and B.

/// USART6 over J2. DMA2 stream 7 transmits, stream 1 receives.
pub type Usart6LoopS7 = Wiring<USART6, Af8<'A', 11>, Af8<'A', 12>, Stream<2, 7>, Stream<2, 1>>;

/// LD2, the green user LED, on PA5.
pub type Led = Pin<'A', 5, Output>;

pub struct RunA {
    pub rcc: Rcc,
    pub led: Led,
    pub usart1: Usart1Loop,
    pub usart6: Usart6Loop,
    pub usart2: Usart2Vcp,
}

pub struct RunB {
    pub rcc: Rcc,
    pub led: Led,
    pub usart1: Usart1Cross,
    pub usart6: Usart6Cross,
}

// Run W: faults injected on run B's cross-wired pair, same link and same
// streams.

pub struct RunW {
    pub rcc: Rcc,
    pub led: Led,
    pub usart1: Usart1Cross,
    pub usart6: Usart6Cross,
}

pub fn run_w(p: Peripherals) -> Result<RunW, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut gpioa = p.GPIOA.split(&mut rcc);
    let gpiob = p.GPIOB.split(&mut rcc);
    let gpioc = p.GPIOC.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = gpioa.p5.into_push_pull_output(&mut gpioa.ctrl);
    let Parts {
        ctrl: mut ctrl_b,
        p6: pb6,
        p7: pb7,
        ..
    } = gpiob;
    let Parts {
        ctrl: mut ctrl_c,
        p6: pc6,
        p7: pc7,
        ..
    } = gpioc;
    let (tx1, rx1) = uart_pins(pb6, pb7, &mut ctrl_b);
    let (tx6, rx6) = uart_pins(pc6, pc7, &mut ctrl_c);
    Ok(RunW {
        rcc,
        led,
        usart1: Wiring {
            uart: p.USART1,
            tx: tx1,
            rx: rx1,
            tx_dma: dma2.s7,
            rx_dma: dma2.s5,
        },
        usart6: Wiring {
            uart: p.USART6,
            tx: tx6,
            rx: rx6,
            tx_dma: dma2.s6,
            rx_dma: dma2.s2,
        },
    })
}

// Run D: USART6 receive-only on PA12, fed over J2 by PA11 as a GPIO.

/// USART6's RX over J2; DMA2 stream 1 receives.
pub type Usart6RxOnly = RxWiring<USART6, Af8<'A', 12>, Stream<2, 1>>;

pub struct RunD {
    pub rcc: Rcc,
    pub led: Led,
    pub usart6: Usart6RxOnly,
    /// PA11, J2's other end, bit-banging frames at USART6.
    pub feed: Pin<'A', 11, Output>,
}

pub fn run_d(p: Peripherals) -> Result<RunD, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut gpioa = p.GPIOA.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = gpioa.p5.into_push_pull_output(&mut gpioa.ctrl);
    let mut feed = gpioa.p11.into_push_pull_output(&mut gpioa.ctrl);
    // A plain line idles high.
    feed.set_high();
    let mut rx = gpioa.p12.into_alternate::<8>(&mut gpioa.ctrl);
    rx.set_pull(&mut gpioa.ctrl, Pull::Up);
    Ok(RunD {
        rcc,
        led,
        usart6: RxWiring {
            uart: p.USART6,
            rx,
            rx_dma: dma2.s1,
        },
        feed,
    })
}

pub struct RunC {
    pub rcc: Rcc,
    pub led: Led,
    pub usart6: Usart6LoopS7,
}

pub fn run_a(p: Peripherals) -> Result<RunA, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&HSE_CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut gpioa = p.GPIOA.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = gpioa.p5.into_push_pull_output(&mut gpioa.ctrl);
    let (tx1, rx1) = uart_pins(gpioa.p9, gpioa.p10, &mut gpioa.ctrl);
    let (tx6, rx6) = uart_pins(gpioa.p11, gpioa.p12, &mut gpioa.ctrl);
    let (tx2, rx2) = uart_pins(gpioa.p2, gpioa.p3, &mut gpioa.ctrl);
    Ok(RunA {
        rcc,
        led,
        usart1: Wiring {
            uart: p.USART1,
            tx: tx1,
            rx: rx1,
            tx_dma: dma2.s7,
            rx_dma: dma2.s2,
        },
        usart6: Wiring {
            uart: p.USART6,
            tx: tx6,
            rx: rx6,
            tx_dma: dma2.s6,
            rx_dma: dma2.s1,
        },
        usart2: Wiring {
            uart: p.USART2,
            tx: tx2,
            rx: rx2,
            tx_dma: dma1.s6,
            rx_dma: dma1.s5,
        },
    })
}

pub fn run_b(p: Peripherals) -> Result<RunB, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut gpioa = p.GPIOA.split(&mut rcc);
    let gpiob = p.GPIOB.split(&mut rcc);
    let gpioc = p.GPIOC.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = gpioa.p5.into_push_pull_output(&mut gpioa.ctrl);
    let Parts {
        ctrl: mut ctrl_b,
        p6: pb6,
        p7: pb7,
        ..
    } = gpiob;
    let Parts {
        ctrl: mut ctrl_c,
        p6: pc6,
        p7: pc7,
        ..
    } = gpioc;
    let (tx1, rx1) = uart_pins(pb6, pb7, &mut ctrl_b);
    let (tx6, rx6) = uart_pins(pc6, pc7, &mut ctrl_c);
    Ok(RunB {
        rcc,
        led,
        usart1: Wiring {
            uart: p.USART1,
            tx: tx1,
            rx: rx1,
            tx_dma: dma2.s7,
            rx_dma: dma2.s5,
        },
        usart6: Wiring {
            uart: p.USART6,
            tx: tx6,
            rx: rx6,
            tx_dma: dma2.s6,
            rx_dma: dma2.s2,
        },
    })
}

pub fn run_c(p: Peripherals) -> Result<RunC, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut gpioa = p.GPIOA.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = gpioa.p5.into_push_pull_output(&mut gpioa.ctrl);
    let (tx6, rx6) = uart_pins(gpioa.p11, gpioa.p12, &mut gpioa.ctrl);
    Ok(RunC {
        rcc,
        led,
        usart6: Wiring {
            uart: p.USART6,
            tx: tx6,
            rx: rx6,
            tx_dma: dma2.s7,
            rx_dma: dma2.s1,
        },
    })
}

// Runs E to G: SPI over the S links, every pin and DMA route RM0368 lists
// for SPI1 to SPI3, each instance and stream once per run.

/// The SPI links' normal setting: 10 MHz at most, so 5.25 MHz on every SPI
/// (SPI1 from PCLK2 / 16, SPI2 and SPI3 from PCLK1 / 8).
pub const SPI: spi::Config = spi::Config::new(Hertz::mhz(10));
const _: () = assert!(matches!(
    Spi::<SPI1>::check(&CLOCKS, &SPI),
    Ok(Hertz(5_250_000))
));
const _: () = assert!(matches!(
    Spi::<SPI2>::check(&CLOCKS, &SPI),
    Ok(Hertz(5_250_000))
));

type Af5<const P: char, const N: u8> = Pin<P, N, Alternate<5>>;
type Af6<const P: char, const N: u8> = Pin<P, N, Alternate<6>>;

/// SPI1 on the Foxeer F405's IMU pins and streams, over S1. SCK is LD2.
pub type Spi1Foxeer =
    spi::Wiring<SPI1, Af5<'A', 5>, Af5<'A', 6>, Af5<'A', 7>, Stream<2, 3>, Stream<2, 0>>;
/// SPI3 on port C, over S5.
pub type Spi3PortC =
    spi::Wiring<SPI3, Af6<'C', 10>, Af6<'C', 11>, Af6<'C', 12>, Stream<1, 7>, Stream<1, 2>>;
/// SPI2 on port B, over S3.
pub type Spi2PortB =
    spi::Wiring<SPI2, Af5<'B', 13>, Af5<'B', 14>, Af5<'B', 15>, Stream<1, 4>, Stream<1, 3>>;
/// SPI1 on port B, over S2.
pub type Spi1PortB =
    spi::Wiring<SPI1, Af5<'B', 3>, Af5<'B', 4>, Af5<'B', 5>, Stream<2, 5>, Stream<2, 2>>;
/// SPI2 with SCK on PB10, MISO and MOSI on port C, over S4.
pub type Spi2PortC =
    spi::Wiring<SPI2, Af5<'B', 10>, Af5<'C', 2>, Af5<'C', 3>, Stream<1, 4>, Stream<1, 3>>;
/// SPI3 on port B, over S2.
pub type Spi3PortB =
    spi::Wiring<SPI3, Af6<'B', 3>, Af6<'B', 4>, Af6<'B', 5>, Stream<1, 5>, Stream<1, 0>>;

pub struct RunE {
    pub rcc: Rcc,
    pub exti: exti::Parts,
    pub spi1: Spi1Foxeer,
    pub spi3: Spi3PortC,
    pub spi2: Spi2PortB,
    /// PA11, driving PA12 over J2 for the EXTI checks.
    pub feed: Pin<'A', 11, Output>,
}

pub struct RunF {
    pub rcc: Rcc,
    pub exti: exti::Parts,
    pub spi1: Spi1PortB,
    pub spi2: Spi2PortC,
}

pub struct RunG {
    pub rcc: Rcc,
    pub exti: exti::Parts,
    pub spi3: Spi3PortB,
    /// Free in run G: SPI1 isn't on PA5 here.
    pub led: Led,
}

pub fn run_e(p: Peripherals) -> Result<RunE, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let mut feed = a.p11.into_push_pull_output(&mut a.ctrl);
    feed.set_low();
    let mut sense = a.p12.into_input(&mut a.ctrl);
    sense.set_pull(&mut a.ctrl, Pull::Down);
    Ok(RunE {
        exti,
        spi1: spi::Wiring {
            spi: p.SPI1,
            sck: sck(a.p5, &mut a.ctrl),
            miso: miso(a.p6, &mut a.ctrl),
            mosi: mosi(a.p7, &mut a.ctrl),
            tx_dma: dma2.s3,
            rx_dma: dma2.s0,
        },
        spi3: spi::Wiring {
            spi: p.SPI3,
            sck: sck(c.p10, &mut c.ctrl),
            miso: miso(c.p11, &mut c.ctrl),
            mosi: mosi(c.p12, &mut c.ctrl),
            tx_dma: dma1.s7,
            rx_dma: dma1.s2,
        },
        spi2: spi::Wiring {
            spi: p.SPI2,
            sck: sck(b.p13, &mut b.ctrl),
            miso: miso(b.p14, &mut b.ctrl),
            mosi: mosi(b.p15, &mut b.ctrl),
            tx_dma: dma1.s4,
            rx_dma: dma1.s3,
        },
        feed,
        rcc,
    })
}

pub fn run_f(p: Peripherals) -> Result<RunF, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    Ok(RunF {
        exti,
        spi1: spi::Wiring {
            spi: p.SPI1,
            sck: sck(b.p3, &mut b.ctrl),
            miso: miso(b.p4, &mut b.ctrl),
            mosi: mosi(b.p5, &mut b.ctrl),
            tx_dma: dma2.s5,
            rx_dma: dma2.s2,
        },
        spi2: spi::Wiring {
            spi: p.SPI2,
            sck: sck(b.p10, &mut b.ctrl),
            miso: miso(c.p2, &mut c.ctrl),
            mosi: mosi(c.p3, &mut c.ctrl),
            tx_dma: dma1.s4,
            rx_dma: dma1.s3,
        },
        rcc,
    })
}

pub fn run_g(p: Peripherals) -> Result<RunG, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    Ok(RunG {
        exti,
        spi3: spi::Wiring {
            spi: p.SPI3,
            sck: sck(b.p3, &mut b.ctrl),
            miso: miso(b.p4, &mut b.ctrl),
            mosi: mosi(b.p5, &mut b.ctrl),
            tx_dma: dma1.s5,
            rx_dma: dma1.s0,
        },
        led,
        rcc,
    })
}

// Run H: every timer, against the DWT. No links.

/// FerroWasp's control loop rate, and its microsecond timebase, checked
/// while compiling on the timers FerroWasp's boards use for them.
pub const CONTROL_RATE: Hertz = Hertz::khz(8);
const _: () = assert!(Tick::<TIM4>::check(&CLOCKS, CONTROL_RATE).is_ok());
const _: () = assert!(Timebase::<TIM2>::check(&CLOCKS, Hertz::mhz(1)).is_ok());

/// The timers as options, so the checks can take each in turn.
pub struct RunH {
    pub rcc: Rcc,
    pub led: Led,
    pub tim1: Option<TIM1>,
    pub tim2: Option<TIM2>,
    pub tim3: Option<TIM3>,
    pub tim4: Option<TIM4>,
    pub tim5: Option<TIM5>,
    pub tim9: Option<TIM9>,
    pub tim10: Option<TIM10>,
    pub tim11: Option<TIM11>,
}

pub fn run_h(p: Peripherals) -> Result<RunH, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    Ok(RunH {
        rcc,
        led,
        tim1: Some(p.TIM1),
        tim2: Some(p.TIM2),
        tim3: Some(p.TIM3),
        tim4: Some(p.TIM4),
        tim5: Some(p.TIM5),
        tim9: Some(p.TIM9),
        tim10: Some(p.TIM10),
        tim11: Some(p.TIM11),
    })
}

// Run I: DShot-style compare lanes, sampled. No links.

/// The lanes: TIM3 channels 3 and 4 on PB0 and PB1 (DMA1 streams 7 and 2),
/// an APB1 timer on DMA1. The sampler: TIM1's update request on DMA2
/// stream 5, reading port B. DMA2 then serves only GPIO and memory, never
/// APB2, so ES0299 2.2.11 can't strike.
pub struct RunI {
    pub rcc: Rcc,
    pub led: Led,
    pub tim3: TIM3,
    pub ch3: Pin<'B', 0, Alternate<2>>,
    pub ch4: Pin<'B', 1, Alternate<2>>,
    pub lane3: Stream<1, 7>,
    pub lane4: Stream<1, 2>,
    pub tim1: TIM1,
    pub sampler: Stream<2, 5>,
}

pub fn run_i(p: Peripherals) -> Result<RunI, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    Ok(RunI {
        led,
        tim3: p.TIM3,
        ch3: sck(b.p0, &mut b.ctrl),
        ch4: sck(b.p1, &mut b.ctrl),
        lane3: dma1.s7,
        lane4: dma1.s2,
        tim1: p.TIM1,
        sampler: dma2.s5,
        rcc,
    })
}

// Run R: the SpeedyBee F405 V5's four motor lanes on two timers, one start.

/// The lanes: TIM3 channels 3 and 4 on PB0 and PB1, TIM2 channel 3 on PB10
/// and channel 2 on PB3 (DMA1 streams 7, 2, 1 and 6) — the SpeedyBee F405
/// V5's motor arrangement as far as this package carries it. The board's
/// TIM2 CH4 pin, PB11, is not bonded on the F401's LQFP64 (DS10086's pin
/// table; metapac has no PB11), so the bench's second TIM2 lane rides CH2 on
/// PB3 over TIM2 CH4's own DMA route, stream 6 channel 3; the board's PB11
/// lane is glue-checked for the F405 and proven on the board itself. The
/// sampler: TIM1's update request on DMA2 stream 5, reading port B, so DMA2
/// serves only GPIO and memory and ES0299 2.2.11 can't strike. RM0090: DMA1's
/// AHB peripheral port is not connected to the bus matrix, so a reply on one
/// of these lanes is read by the sampler, not by the lane's own stream — the
/// shape the board's bidirectional DShot will use.
pub struct RunR {
    pub rcc: Rcc,
    pub led: Led,
    pub tim3: TIM3,
    pub tim2: TIM2,
    pub ch3: Pin<'B', 0, Alternate<2>>,
    pub ch4: Pin<'B', 1, Alternate<2>>,
    pub ch23: Pin<'B', 10, Alternate<1>>,
    pub ch22: Pin<'B', 3, Alternate<1>>,
    pub lane3: Stream<1, 7>,
    pub lane4: Stream<1, 2>,
    pub lane23: Stream<1, 1>,
    pub lane22: Stream<1, 6>,
    pub tim1: TIM1,
    pub sampler: Stream<2, 5>,
}

pub fn run_r(p: Peripherals) -> Result<RunR, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    Ok(RunR {
        led,
        tim3: p.TIM3,
        tim2: p.TIM2,
        ch3: sck(b.p0, &mut b.ctrl),
        ch4: sck(b.p1, &mut b.ctrl),
        ch23: sck(b.p10, &mut b.ctrl),
        ch22: sck(b.p3, &mut b.ctrl),
        lane3: dma1.s7,
        lane4: dma1.s2,
        lane23: dma1.s1,
        lane22: dma1.s6,
        tim1: p.TIM1,
        sampler: dma2.s5,
        rcc,
    })
}

// Run O: the waveform generator out over J3, read at the far end.

/// TIM4 channel 1 on PB6, on DMA1 stream 0, plays into J3. TIM1's update request
/// on DMA2 stream 5 reads port C, where J3 reaches PC7, left an input
/// and pulled up: with the sender's output off a released wire reads
/// high, so "nothing crossed" is a level the checks can see.
pub struct RunO {
    pub rcc: Rcc,
    pub led: Led,
    pub tim4: TIM4,
    pub pb6: Pin<'B', 6, Alternate<2>>,
    pub lane: Stream<1, 0>,
    pub tim1: TIM1,
    pub sampler: Stream<2, 5>,
    pub pc7: Pin<'C', 7, Input>,
}

pub fn run_o(p: Peripherals) -> Result<RunO, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    // PC7, J3's far end, stays an input, pulled up; the pin is held so
    // the pull is not an accident of boot.
    let mut c = p.GPIOC.split(&mut rcc);
    let mut pc7 = c.p7.into_input(&mut c.ctrl);
    pc7.set_pull(&mut c.ctrl, Pull::Up);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    Ok(RunO {
        led,
        tim4: p.TIM4,
        pb6: sck(b.p6, &mut b.ctrl),
        lane: dma1.s0,
        tim1: p.TIM1,
        sampler: dma2.s5,
        pc7,
        rcc,
    })
}

// Run P: the waveform generator in over S1, read by a lane on its own pin.

/// TIM1's channel 1, complementary output on PA7, listens on DMA2 stream 1, the
/// only DMA here that reaches GPIO. TIM3 channel 1 on PA6, on DMA1 stream 4, plays
/// into the other end of S1. A pull-up holds the line while neither drives it.
pub struct RunP {
    pub rcc: Rcc,
    pub led: Led,
    pub tim1: TIM1,
    pub pa7: Pin<'A', 7, Alternate<1>>,
    pub near_lane: Stream<2, 1>,
    pub tim3: TIM3,
    pub pa6: Pin<'A', 6, Alternate<2>>,
    pub far_lane: Stream<1, 4>,
}

pub fn run_p(p: Peripherals) -> Result<RunP, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    let mut pa7: Pin<'A', 7, Alternate<1>> = sck(a.p7, &mut a.ctrl);
    pa7.set_pull(&mut a.ctrl, ferrohal_stm32::gpio::Pull::Up);
    Ok(RunP {
        led,
        tim1: p.TIM1,
        pa7,
        near_lane: dma2.s1,
        tim3: p.TIM3,
        pa6: sck(a.p6, &mut a.ctrl),
        far_lane: dma1.s4,
        rcc,
    })
}

// Run J: the independent watchdog, across the reset it causes. No links.

pub struct RunJ {
    pub rcc: Rcc,
    pub led: Led,
    pub iwdg: ferrohal_stm32::chip::IWDG,
}

pub fn run_j(p: Peripherals) -> Result<RunJ, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    Ok(RunJ {
        rcc,
        led,
        iwdg: p.IWDG,
    })
}

// Run K: the polled SPI (no DMA), over S4 and S5.

/// SPI2 on the Foxeer F405 V2's NOR flash pins (PB13, PC2, PC3), over S4,
/// at the Foxeer's 10 MHz in mode 0: 5.25 MHz here.
pub type Spi2FoxeerNor = spi::BlockingWiring<SPI2, Af5<'B', 13>, Af5<'C', 2>, Af5<'C', 3>>;
/// SPI3 on port C, over S5.
pub type Spi3Blocking = spi::BlockingWiring<SPI3, Af6<'C', 10>, Af6<'C', 11>, Af6<'C', 12>>;

// Run M: the ADC, PA7 driving PA6 over S1.

pub struct RunM {
    pub rcc: Rcc,
    pub led: Led,
    pub adc: ferrohal_stm32::chip::ADC1,
    pub analog: Pin<'A', 6, ferrohal_stm32::gpio::Analog>,
    pub drive: Pin<'A', 7, Output>,
    pub stream: Stream<2, 0>,
}

pub fn run_m(p: Peripherals) -> Result<RunM, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    let analog = a.p6.into_analog(&mut a.ctrl);
    let drive = a.p7.into_push_pull_output(&mut a.ctrl);
    Ok(RunM {
        rcc,
        led,
        adc: p.ADC1,
        analog,
        drive,
        stream: dma2.s0,
    })
}

pub struct RunK {
    pub rcc: Rcc,
    pub led: Led,
    pub exti: exti::Parts,
    pub spi2: Spi2FoxeerNor,
    pub spi3: Spi3Blocking,
}

pub fn run_k(p: Peripherals) -> Result<RunK, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    Ok(RunK {
        led,
        exti,
        spi2: spi::BlockingWiring {
            spi: p.SPI2,
            sck: sck(b.p13, &mut b.ctrl),
            miso: miso(c.p2, &mut c.ctrl),
            mosi: mosi(c.p3, &mut c.ctrl),
        },
        spi3: spi::BlockingWiring {
            spi: p.SPI3,
            sck: sck(c.p10, &mut c.ctrl),
            miso: miso(c.p11, &mut c.ctrl),
            mosi: mosi(c.p12, &mut c.ctrl),
        },
        rcc,
    })
}

// Run Q: the I2C, I2C1 on the Arduino connector's SCL and SDA (PB8, PB9),
// pulled up internally, no slave fitted. No links.

pub type I2c1Bus = i2c::Wiring<I2C1, Pin<'B', 8, Alternate<4>>, Pin<'B', 9, Alternate<4>>>;

pub struct RunQ {
    pub rcc: Rcc,
    pub led: Led,
    pub i2c: I2c1Bus,
}

pub fn run_q(p: Peripherals) -> Result<RunQ, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    // SCL and SDA pulled up: with no slave fitted the lines idle high,
    // so a vacant address is answered by the bus itself with a NACK.
    let mut scl: Pin<'B', 8, Alternate<4>> = b.p8.into_alternate_open_drain(&mut b.ctrl);
    scl.set_pull(&mut b.ctrl, Pull::Up);
    let mut sda: Pin<'B', 9, Alternate<4>> = b.p9.into_alternate_open_drain(&mut b.ctrl);
    sda.set_pull(&mut b.ctrl, Pull::Up);
    Ok(RunQ {
        rcc,
        led,
        i2c: i2c::Wiring {
            i2c: p.I2C1,
            scl,
            sda,
        },
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

fn mosi<const P: char, const N: u8, const AF: u8>(
    pin: Pin<P, N>,
    ctrl: &mut Ctrl<P>,
) -> Pin<P, N, Alternate<AF>>
where
    Port<P>: PortInstance,
{
    sck(pin, ctrl)
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

/// A TX and an RX pin of one port, in alternate function `AF`. RX is pulled
/// up: without its link it reads an idle line, and the test reports
/// silence rather than noise.
fn uart_pins<const P: char, const T: u8, const R: u8, const AF: u8>(
    tx: Pin<P, T>,
    rx: Pin<P, R>,
    ctrl: &mut Ctrl<P>,
) -> (Pin<P, T, Alternate<AF>>, Pin<P, R, Alternate<AF>>)
where
    Port<P>: PortInstance,
{
    let mut tx = tx.into_alternate::<AF>(ctrl);
    tx.set_speed(ctrl, Speed::Medium);
    let mut rx = rx.into_alternate::<AF>(ctrl);
    rx.set_pull(ctrl, Pull::Up);
    (tx, rx)
}

// Run S: the I2C master against the Pico 1 simulator, I2C3 on PA8
// (CN9-8, D7) and PC9 (CN10-1), pulled up internally. The simulator —
// hil/pico-i2c-sim, wired per hil/WIRING.md — answers at 0x2A; that
// list also explains why I2C3 is the F401's only benchable second I2C.
// No jumper between the Nucleo's own pins: the link runs to the Pico,
// and the NACK-only run stays on I2C1 untouched.

pub type I2c3Bus = i2c::Wiring<I2C3, Pin<'A', 8, Alternate<4>>, Pin<'C', 9, Alternate<4>>>;

pub struct RunS {
    pub rcc: Rcc,
    pub led: Led,
    pub i2c: I2c3Bus,
}

pub fn run_s(p: Peripherals) -> Result<RunS, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    // SCL and SDA pulled up here and at the Pico's end: the lines idle
    // high at 50 kHz with no resistor fitted.
    let mut scl: Pin<'A', 8, Alternate<4>> = a.p8.into_alternate_open_drain(&mut a.ctrl);
    scl.set_pull(&mut a.ctrl, Pull::Up);
    let mut sda: Pin<'C', 9, Alternate<4>> = c.p9.into_alternate_open_drain(&mut c.ctrl);
    sda.set_pull(&mut c.ctrl, Pull::Up);
    Ok(RunS {
        rcc,
        led,
        i2c: i2c::Wiring {
            i2c: p.I2C3,
            scl,
            sda,
        },
    })
}

// Run V: every constructor refuses on hardware and hands its parts back.

/// The clocks run V freezes: the bench's own, except PLLQ gives 56 MHz
/// instead of 48, so the USB constructor must refuse (an F4's only USB
/// clock source is a 48 MHz PLLQ). Nothing else on this bench uses PLLQ.
pub const REFUSAL_CLOCK_CONFIG: Config = Config {
    hse: None,
    pll: Some(Pll {
        source: PllSource::Hsi,
        prediv: 16,
        mul: 336,
        divp: PllP::Div4,
        divq: Some(6),
    }),
    sys: Sysclk::PllP,
    ahb_pre: AhbPrescaler::Div1,
    apb1_pre: ApbPrescaler::Div2,
    apb2_pre: ApbPrescaler::Div1,
    supply: Supply::V2_7To3_6,
};
pub const REFUSAL_CLOCKS: Clocks = match REFUSAL_CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("REFUSAL_CLOCK_CONFIG breaks the F401's limits"),
};

/// A configuration that must refuse: `PllP` as the system clock with no
/// PLL configured at all.
pub const NO_PLL_CONFIG: Config = Config {
    hse: None,
    pll: None,
    sys: Sysclk::PllP,
    ahb_pre: AhbPrescaler::Div1,
    apb1_pre: ApbPrescaler::Div2,
    apb2_pre: ApbPrescaler::Div1,
    supply: Supply::V2_7To3_6,
};
const _: () = assert!(matches!(
    NO_PLL_CONFIG.validate(),
    Err(ClockError::PllNotConfigured)
));
const _: () = {
    match REFUSAL_CLOCKS.pll_q {
        Some(Hertz(56_000_000)) => {}
        _ => panic!("REFUSAL_CLOCK_CONFIG must give 56 MHz on PLLQ"),
    }
};

/// USART1 over J1, DMA2 stream 7 transmitting, stream 2 receiving.
pub type Usart1LoopS2 = Wiring<USART1, Af7<'A', 9>, Af7<'A', 10>, Stream<2, 7>, Stream<2, 2>>;
/// SPI3 on port C over S5, DMA1 stream 5 transmitting, stream 2 receiving:
/// DMA2's streams are the UART's, the ADC scan's and the sampler's.
pub type Spi3PortCRefusal =
    spi::Wiring<SPI3, Af6<'C', 10>, Af6<'C', 11>, Af6<'C', 12>, Stream<1, 5>, Stream<1, 2>>;

/// I2C3 to the Pico simulator's link, or to nothing: pulled up either way,
/// a vacant address is NACKed by the bus itself.
pub type I2c3Link = i2c::Wiring<I2C3, Pin<'A', 8, Alternate<4>>, Pin<'C', 9, Alternate<4>>>;

/// SPI2 polled, over S3.
pub type Spi2Polled = spi::BlockingWiring<SPI2, Af5<'B', 13>, Af5<'B', 14>, Af5<'B', 15>>;

pub struct RunV {
    pub rcc: Rcc,
    pub led: Led,
    /// The USB token and its two pins, as the driver wants them wired.
    pub usb: (
        ferrohal_stm32::chip::USB_OTG_FS,
        Pin<'A', 11, Alternate<10>>,
        Pin<'A', 12, Alternate<10>>,
    ),
    /// USART1 over J1, DMA2 streams 7 and 2.
    pub usart1: Usart1LoopS2,
    /// SPI3 over S5, DMA1 streams 5 and 2.
    pub spi3: Spi3PortCRefusal,
    /// The ADC scan's stream, DMA2 stream 0.
    pub scan_stream: Stream<2, 0>,
    /// SPI2 polled over S3.
    pub spi2_polled: Spi2Polled,
    /// I2C3 on PA8 and PC9.
    pub i2c3: I2c3Link,
    /// ADC1; the scan uses the internal channels, so no pin is taken.
    pub adc1: ferrohal_stm32::chip::ADC1,
    /// TIM3's CH3 on PB0, and the same channel again on PC8: the refused
    /// lane construction uses PC8's channel with PB0's buffer.
    pub tim3: TIM3,
    pub ch3: Pin<'B', 0, Alternate<2>>,
    /// TIM3 CH3's stream, DMA1 stream 7.
    pub lane3_stream: Stream<1, 7>,
    /// TIM4's CH1 on PB6, run O's waveform pin, on DMA1 stream 0; TIM2's
    /// CH3 on PB10, run R's, on DMA1 stream 1; both read back by the TIM1
    /// sampler on DMA2 stream 5.
    pub tim4: TIM4,
    pub pb6: Pin<'B', 6, Alternate<2>>,
    pub lane4_stream: Stream<1, 0>,
    pub tim2: TIM2,
    pub pb10: Pin<'B', 10, Alternate<1>>,
    pub lane2_stream: Stream<1, 1>,
    pub tim1: TIM1,
    pub sampler_stream: Stream<2, 5>,
    /// TIM9 for the tick, TIM5 for the timebase.
    pub tim9: TIM9,
    pub tim5: TIM5,
    pub iwdg: ferrohal_stm32::chip::IWDG,
    /// Port A's control, for re-purposing the USB pins the refusal hands back.
    pub ctrl_a: Ctrl<'A'>,
}

pub fn run_v(p: Peripherals) -> Result<(RunV, ClockError), ClockError> {
    // The refusal first: a config naming PllP with no PLL must hand the RCC
    // token back, and that token freezes the run's own clocks.
    let (refusal, rcc) = match p.RCC.freeze(&NO_PLL_CONFIG) {
        Err(refused) => refused.into_inner(),
        Ok(_) => defmt::panic!("NO_PLL_CONFIG was accepted"),
    };
    let mut rcc = rcc
        .freeze(&REFUSAL_CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = a.p5.into_push_pull_output(&mut a.ctrl);
    let (tx1, rx1) = uart_pins(a.p9, a.p10, &mut a.ctrl);
    let mut scl: Pin<'A', 8, Alternate<4>> = a.p8.into_alternate_open_drain(&mut a.ctrl);
    scl.set_pull(&mut a.ctrl, Pull::Up);
    let mut sda: Pin<'C', 9, Alternate<4>> = c.p9.into_alternate_open_drain(&mut c.ctrl);
    sda.set_pull(&mut c.ctrl, Pull::Up);
    // The DShot600-rate lanes need fast edges, as the `sck` helper sets.
    let mut ch3: Pin<'B', 0, Alternate<2>> = b.p0.into_alternate(&mut b.ctrl);
    ch3.set_speed(&mut b.ctrl, Speed::High);
    let mut pb6: Pin<'B', 6, Alternate<2>> = b.p6.into_alternate(&mut b.ctrl);
    pb6.set_speed(&mut b.ctrl, Speed::High);
    let mut pb10: Pin<'B', 10, Alternate<1>> = b.p10.into_alternate(&mut b.ctrl);
    pb10.set_speed(&mut b.ctrl, Speed::High);
    Ok((
        RunV {
            rcc,
            led,
            usb: (
                p.USB_OTG_FS,
                a.p11.into_alternate(&mut a.ctrl),
                a.p12.into_alternate(&mut a.ctrl),
            ),
            usart1: Wiring {
                uart: p.USART1,
                tx: tx1,
                rx: rx1,
                tx_dma: dma2.s7,
                rx_dma: dma2.s2,
            },
            spi3: spi::Wiring {
                spi: p.SPI3,
                sck: sck(c.p10, &mut c.ctrl),
                miso: miso(c.p11, &mut c.ctrl),
                mosi: mosi(c.p12, &mut c.ctrl),
                tx_dma: dma1.s5,
                rx_dma: dma1.s2,
            },
            scan_stream: dma2.s0,
            spi2_polled: spi::BlockingWiring {
                spi: p.SPI2,
                sck: sck(b.p13, &mut b.ctrl),
                miso: miso(b.p14, &mut b.ctrl),
                mosi: mosi(b.p15, &mut b.ctrl),
            },
            i2c3: i2c::Wiring {
                i2c: p.I2C3,
                scl,
                sda,
            },
            adc1: p.ADC1,
            tim3: p.TIM3,
            ch3: ch3,
            lane3_stream: dma1.s7,
            tim4: p.TIM4,
            pb6: pb6,
            lane4_stream: dma1.s0,
            tim2: p.TIM2,
            pb10: pb10,
            lane2_stream: dma1.s1,
            tim1: p.TIM1,
            sampler_stream: dma2.s5,
            tim9: p.TIM9,
            tim5: p.TIM5,
            iwdg: p.IWDG,
            ctrl_a: a.ctrl,
        },
        refusal,
    ))
}
