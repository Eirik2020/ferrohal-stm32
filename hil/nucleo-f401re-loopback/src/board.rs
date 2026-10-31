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
//!
//! USART2 (PA2, PA3) needs no link: SB13 and SB14 wire it to the ST-LINK's
//! virtual COM port, where `hil/vcp_echo.py` sends every byte back.
//!
//! Each run starts from reset, so the pins a run does not use stay inputs
//! and the links on them carry nothing.

// Every run binary compiles this file and uses its own run's wiring only.
#![allow(dead_code)]

use ferrohal_stm32::{
    chip::{Peripherals, USART1, USART2, USART6},
    dma::Stream,
    gpio::{Alternate, Ctrl, Output, Parts, Pin, Port, PortInstance, Pull, Speed},
    rcc::{
        AhbPrescaler, ApbPrescaler, ClockError, Clocks, Config, Pll, PllP, PllSource, Rcc,
        Supply, Sysclk,
    },
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
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
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
