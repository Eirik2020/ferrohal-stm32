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
//! USART2 (PA2, PA3) needs no link: SB13 and SB14 wire it to the ST-LINK's
//! virtual COM port, where `hil/vcp_echo.py` sends every byte back.
//!
//! Each run starts from reset, so the pins a run does not use stay inputs
//! and the links on them carry nothing.

// Every run binary compiles this file and uses its own run's wiring only.
#![allow(dead_code)]

use ferrohal_stm32::{
    chip::{
        Peripherals, SPI1, SPI2, SPI3, TIM1, TIM2, TIM3, TIM4, TIM5, TIM9, TIM10, TIM11, USART1,
        USART2, USART6,
    },
    dma::Stream,
    exti,
    gpio::{Alternate, Ctrl, Output, Parts, Pin, Port, PortInstance, Pull, Speed},
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
const _: () = assert!(Uart::check::<USART6>(&CLOCKS, &SBUS_PLAIN).is_ok());
const _: () = assert!(matches!(
    Uart::check::<USART6>(&CLOCKS, &SBUS_PLAIN.with_inverted_rx(true)),
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
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
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
    let mut rcc = p.RCC.freeze(&HSE_CLOCK_CONFIG)?;
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
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
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
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
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
    Spi::check::<SPI1>(&CLOCKS, &SPI),
    Ok(Hertz(5_250_000))
));
const _: () = assert!(matches!(
    Spi::check::<SPI2>(&CLOCKS, &SPI),
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
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
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
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
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
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
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
const _: () = assert!(Tick::check::<TIM4>(&CLOCKS, CONTROL_RATE).is_ok());
const _: () = assert!(Timebase::check::<TIM2>(&CLOCKS, Hertz::mhz(1)).is_ok());

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
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
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
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
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
