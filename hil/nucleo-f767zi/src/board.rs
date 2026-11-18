//! Nucleo-F767ZI (MB1137): the clock tree, and which pins each run uses.
//! This file is chip-specific; FerroHAL's routing traits check every choice
//! here at compile time.
//!
//! The links, on the Zio headers (UM1974 Table 19), each a UART's TX wired to its
//! own RX:
//!
//! | Link | TX -> RX | Zio sockets |
//! | --- | --- | --- |
//! | F1 | USART2 PD5 -> PD6 | CN9-6 -> CN9-4 |
//! | F2 | USART6 PG14 -> PG9 | CN10-14 -> CN10-16 |
//! | F3 | UART4 PD1 -> PD0 | CN9-27 -> CN9-25 |
//! | F4 | UART5 PC12 -> PD2 | CN8-10 -> CN8-12 |
//! | F5 | USART3 or UART4 PC10 -> PC11 | CN8-6 -> CN8-8 |
//! | F6 | UART7 PE8 -> PE7 | CN10-18 -> CN10-20 |
//!
//! And each SPI's MOSI wired to its own MISO:
//!
//! | Link | MOSI -> MISO | Zio sockets |
//! | --- | --- | --- |
//! | S1 | SPI1 or SPI3 PB5 -> PB4 | CN7-13 -> CN7-19 |
//! | S2 | SPI2 PC3 -> PC2 | CN9-5 -> CN10-9 |
//! | S3 | SPI4 PE6 -> PE5 | CN9-20 -> CN9-18 |
//! | S4 | SPI5 PF9 -> PF8 | CN9-28 -> CN9-24 |
//!
//! Run F runs SPI1, SPI2, SPI4 and SPI5 over them, each SCK also routed to EXTI,
//! which counts its edges. Run D runs F1, F2, F3, F4 and F6 at once; run E F5 as UART4 and USART3 through
//! the ST-LINK's virtual COM port (PD8, PD9; SB5 and SB6, UM1974 7.9), where
//! `hil/vcp_echo.py` sends every byte back.
//!
//! Runs A to C need no links: A the clock tree (voltage scale and over-drive)
//! and every timer against the DWT, B the independent watchdog across the reset
//! it causes, C the I2C master on I2C1 (PB8 and PB9, Zio CN7-2 and CN7-4, the
//! Arduino SCL and SDA), pulled up internally, no slave fitted. The SPI links
//! and the second Pico are in `docs/plans/2026-11-17-f411-f722-f745.md` until
//! their runs exist.
//!
//! The F7's UARTs (usart_v3) have no driver yet, so every run reports over the
//! debugger alone. Each run starts from reset, so the pins a run does not use
//! stay inputs.

// Every run binary compiles this file and uses its own run's wiring only.
#![allow(dead_code)]

#[path = "../../common/ident.rs"]
mod ident;

/// This image's identity block, at 0x0800_0400 (`memory.x`).
#[used]
#[unsafe(link_section = ".ferro_ident")]
static FERRO_IDENT: [u8; ident::LEN] = ident::block(concat!(
    "FERRO-ID v=1 board=nucleo-f767zi part=STM32F767ZI crate=",
    env!("CARGO_PKG_NAME"),
    " run=",
    env!("CARGO_BIN_NAME"),
    " git=",
    env!("FERRO_GIT"),
));

use ferrohal_stm32::{
    chip::{
        I2C1, Peripherals, SPI1, SPI2, SPI4, SPI5, TIM1, TIM2, TIM3, TIM4, TIM5, TIM8, TIM9, TIM10,
        TIM11, TIM12, TIM13, TIM14, UART4, UART5, UART7, USART2, USART3, USART6,
    },
    dma::Stream,
    exti,
    gpio::{Alternate, Ctrl, OpenDrain, Output, Pin, Port, PortInstance, Pull, Speed},
    i2c,
    rcc::{
        AhbPrescaler, ApbPrescaler, ClockError, Clocks, Config, Pll, PllP, PllSource, Rcc, Supply,
        Sysclk,
    },
    spi,
    time::Hertz,
    timer::{Tick, Timebase},
    uart::Wiring,
};

/// 216 MHz, the F7's ceiling, from HSI so it needs no solder bridge: 16 MHz
/// / 8 * 216 = 432 MHz VCO, / 2 = 216 MHz, / 9 = 48 MHz; APB1 at 54 MHz, APB2
/// at 108. HCLK is above the 180 MHz over-drive threshold, so `freeze` must put
/// the regulator in over-drive (RCC-004).
pub const CLOCK_CONFIG: Config = Config {
    hse: None,
    pll: Some(Pll {
        source: PllSource::Hsi,
        prediv: 8,
        mul: 216,
        divp: PllP::Div2,
        divq: Some(9),
    }),
    sys: Sysclk::PllP,
    ahb_pre: AhbPrescaler::Div1,
    apb1_pre: ApbPrescaler::Div4,
    apb2_pre: ApbPrescaler::Div2,
    supply: Supply::V2_7To3_6,
};

/// The frequencies `CLOCK_CONFIG` gives, worked out and checked while
/// compiling.
pub const CLOCKS: Clocks = match CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("CLOCK_CONFIG breaks the F7's limits"),
};

/// The IWDG's LSI, measured across a reset: 50 ms at the nominal 32 kHz came
/// back as 53.96 ms (run B, 2026-11-17), so this board's LSI runs at 29.65 kHz
/// (DS11532 Table 46 gives 17 to 47 kHz).
pub const LSI_HZ: Hertz = Hertz::hz(29_650);

/// The blue user LED LD2 on PB7 (UM1974 7.5): lit when a run passes.
pub type Led = Pin<'B', 7, Output>;

// Run A: the clock tree and every timer, against the DWT. No links.

/// A control-loop rate and a microsecond timebase, checked while compiling.
pub const CONTROL_RATE: Hertz = Hertz::khz(8);
const _: () = assert!(Tick::<TIM4>::check(&CLOCKS, CONTROL_RATE).is_ok());
const _: () = assert!(Timebase::<TIM2>::check(&CLOCKS, Hertz::mhz(1)).is_ok());

/// The timers as options, so the checks can take each in turn.
pub struct RunA {
    pub rcc: Rcc,
    pub led: Led,
    pub tim1: Option<TIM1>,
    pub tim2: Option<TIM2>,
    pub tim3: Option<TIM3>,
    pub tim4: Option<TIM4>,
    pub tim5: Option<TIM5>,
    pub tim8: Option<TIM8>,
    pub tim9: Option<TIM9>,
    pub tim10: Option<TIM10>,
    pub tim11: Option<TIM11>,
    pub tim12: Option<TIM12>,
    pub tim13: Option<TIM13>,
    pub tim14: Option<TIM14>,
}

fn freeze(p_rcc: ferrohal_stm32::rcc::RCC) -> Result<Rcc, ClockError> {
    p_rcc
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())
}

pub fn run_a(p: Peripherals) -> Result<RunA, ClockError> {
    let mut rcc = freeze(p.RCC)?;
    let mut b = p.GPIOB.split(&mut rcc);
    let led = b.p7.into_push_pull_output(&mut b.ctrl);
    Ok(RunA {
        rcc,
        led,
        tim1: Some(p.TIM1),
        tim2: Some(p.TIM2),
        tim3: Some(p.TIM3),
        tim4: Some(p.TIM4),
        tim5: Some(p.TIM5),
        tim8: Some(p.TIM8),
        tim9: Some(p.TIM9),
        tim10: Some(p.TIM10),
        tim11: Some(p.TIM11),
        tim12: Some(p.TIM12),
        tim13: Some(p.TIM13),
        tim14: Some(p.TIM14),
    })
}

// Run B: the independent watchdog, across the reset it causes. No links.

pub struct RunB {
    pub rcc: Rcc,
    pub led: Led,
    pub iwdg: ferrohal_stm32::chip::IWDG,
}

pub fn run_b(p: Peripherals) -> Result<RunB, ClockError> {
    let mut rcc = freeze(p.RCC)?;
    let mut b = p.GPIOB.split(&mut rcc);
    let led = b.p7.into_push_pull_output(&mut b.ctrl);
    Ok(RunB {
        rcc,
        led,
        iwdg: p.IWDG,
    })
}

// Run C: the I2C, I2C1 on the Zio's Arduino SCL and SDA (PB8, PB9), pulled
// up internally, no slave fitted. No links.

pub type I2c1Bus =
    i2c::Wiring<I2C1, Pin<'B', 8, Alternate<4, OpenDrain>>, Pin<'B', 9, Alternate<4, OpenDrain>>>;

pub struct RunC {
    pub rcc: Rcc,
    pub led: Led,
    pub i2c: I2c1Bus,
}

pub fn run_c(p: Peripherals) -> Result<RunC, ClockError> {
    let mut rcc = freeze(p.RCC)?;
    let mut b = p.GPIOB.split(&mut rcc);
    let led = b.p7.into_push_pull_output(&mut b.ctrl);
    // SCL and SDA pulled up: with no slave fitted the lines idle high, so a
    // vacant address is answered by the bus itself with a NACK.
    let mut scl: Pin<'B', 8, Alternate<4, OpenDrain>> = b.p8.into_alternate_open_drain(&mut b.ctrl);
    scl.set_pull(&mut b.ctrl, Pull::Up);
    let mut sda: Pin<'B', 9, Alternate<4, OpenDrain>> = b.p9.into_alternate_open_drain(&mut b.ctrl);
    sda.set_pull(&mut b.ctrl, Pull::Up);
    Ok(RunC {
        rcc,
        led,
        i2c: i2c::Wiring {
            i2c: p.I2C1,
            scl,
            sda,
        },
    })
}

/// A UART's alternate-function pin.
type Af<const P: char, const N: u8, const AF: u8> = Pin<P, N, Alternate<AF>>;

/// A TX and an RX pin of one port, in alternate function `AF`. RX is pulled
/// up: without its link it reads an idle line, and the test reports silence
/// rather than noise.
fn uart_pins<const P: char, const T: u8, const R: u8, const AF: u8>(
    tx: Pin<P, T>,
    rx: Pin<P, R>,
    ctrl: &mut Ctrl<P>,
) -> (Af<P, T, AF>, Af<P, R, AF>)
where
    Port<P>: PortInstance,
{
    let mut tx = tx.into_alternate::<AF>(ctrl);
    tx.set_speed(ctrl, Speed::Medium);
    let mut rx = rx.into_alternate::<AF>(ctrl);
    rx.set_pull(ctrl, Pull::Up);
    (tx, rx)
}

// Run D: F1, F2, F3, F4 and F6 at once. The streams are the only ones each
// UART's requests reach that leave the others theirs (the generated glue's
// rows): DMA1 serves all but USART6.

pub type Usart2Loop = Wiring<USART2, Af<'D', 5, 7>, Af<'D', 6, 7>, Stream<1, 6>, Stream<1, 5>>;
pub type Usart6Loop = Wiring<USART6, Af<'G', 14, 8>, Af<'G', 9, 8>, Stream<2, 6>, Stream<2, 1>>;
pub type Uart4Loop = Wiring<UART4, Af<'D', 1, 8>, Af<'D', 0, 8>, Stream<1, 4>, Stream<1, 2>>;
pub type Uart5Loop = Wiring<UART5, Af<'C', 12, 8>, Af<'D', 2, 8>, Stream<1, 7>, Stream<1, 0>>;
pub type Uart7Loop = Wiring<UART7, Af<'E', 8, 8>, Af<'E', 7, 8>, Stream<1, 1>, Stream<1, 3>>;

pub struct RunD {
    pub rcc: Rcc,
    pub led: Led,
    pub usart2: Usart2Loop,
    pub usart6: Usart6Loop,
    pub uart4: Uart4Loop,
    pub uart5: Uart5Loop,
    pub uart7: Uart7Loop,
}

pub fn run_d(p: Peripherals) -> Result<RunD, ClockError> {
    let mut rcc = freeze(p.RCC)?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let mut e = p.GPIOE.split(&mut rcc);
    let mut g = p.GPIOG.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let led = b.p7.into_push_pull_output(&mut b.ctrl);
    let (tx2, rx2) = uart_pins(d.p5, d.p6, &mut d.ctrl);
    let (tx6, rx6) = uart_pins(g.p14, g.p9, &mut g.ctrl);
    let (tx4, rx4) = uart_pins(d.p1, d.p0, &mut d.ctrl);
    // UART5's TX and RX sit on two ports.
    let mut tx5 = c.p12.into_alternate::<8>(&mut c.ctrl);
    tx5.set_speed(&mut c.ctrl, Speed::Medium);
    let mut rx5 = d.p2.into_alternate::<8>(&mut d.ctrl);
    rx5.set_pull(&mut d.ctrl, Pull::Up);
    let (tx7, rx7) = uart_pins(e.p8, e.p7, &mut e.ctrl);
    Ok(RunD {
        rcc,
        led,
        usart2: Wiring {
            uart: p.USART2,
            tx: tx2,
            rx: rx2,
            tx_dma: dma1.s6,
            rx_dma: dma1.s5,
        },
        usart6: Wiring {
            uart: p.USART6,
            tx: tx6,
            rx: rx6,
            tx_dma: dma2.s6,
            rx_dma: dma2.s1,
        },
        uart4: Wiring {
            uart: p.UART4,
            tx: tx4,
            rx: rx4,
            tx_dma: dma1.s4,
            rx_dma: dma1.s2,
        },
        uart5: Wiring {
            uart: p.UART5,
            tx: tx5,
            rx: rx5,
            tx_dma: dma1.s7,
            rx_dma: dma1.s0,
        },
        uart7: Wiring {
            uart: p.UART7,
            tx: tx7,
            rx: rx7,
            tx_dma: dma1.s1,
            rx_dma: dma1.s3,
        },
    })
}

// Run E: F5 as UART4 (PC10 -> PC11), and USART3 through the ST-LINK.

pub type Uart4PortC = Wiring<UART4, Af<'C', 10, 8>, Af<'C', 11, 8>, Stream<1, 4>, Stream<1, 2>>;
pub type Usart3Vcp = Wiring<USART3, Af<'D', 8, 7>, Af<'D', 9, 7>, Stream<1, 3>, Stream<1, 1>>;

pub struct RunE {
    pub rcc: Rcc,
    pub led: Led,
    pub uart4: Uart4PortC,
    pub usart3: Usart3Vcp,
}

pub fn run_e(p: Peripherals) -> Result<RunE, ClockError> {
    let mut rcc = freeze(p.RCC)?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let led = b.p7.into_push_pull_output(&mut b.ctrl);
    let (tx4, rx4) = uart_pins(c.p10, c.p11, &mut c.ctrl);
    let (tx3, rx3) = uart_pins(d.p8, d.p9, &mut d.ctrl);
    Ok(RunE {
        rcc,
        led,
        uart4: Wiring {
            uart: p.UART4,
            tx: tx4,
            rx: rx4,
            tx_dma: dma1.s4,
            rx_dma: dma1.s2,
        },
        usart3: Wiring {
            uart: p.USART3,
            tx: tx3,
            rx: rx3,
            tx_dma: dma1.s3,
            rx_dma: dma1.s1,
        },
    })
}

// Run F: SPI1, SPI2, SPI4 and SPI5 over S1 to S4. DMA2 serves SPI1, SPI4 and
// SPI5 on six streams of their own, DMA1 SPI2.

/// The bench's SPI clock: the fastest a 108 MHz APB2's prescaler gives at or
/// below 10 MHz, as on the F401 (whose 84 MHz gives 5.25).
pub const SPI: spi::Config = spi::Config::new(Hertz::mhz(10));
const _: () = assert!(ferrohal_stm32::spi::check::<SPI1>(&CLOCKS, &SPI).is_ok());

type Af5<const P: char, const N: u8> = Pin<P, N, Alternate<5>>;

/// SPI1 on port B (SCK PB3, MISO PB4, MOSI PB5), over S1.
pub type Spi1PortB =
    spi::Wiring<SPI1, Af5<'B', 3>, Af5<'B', 4>, Af5<'B', 5>, Stream<2, 5>, Stream<2, 2>>;
/// SPI2 with SCK on PB10 (CN10-32), MISO and MOSI on PC2, PC3, over S2.
pub type Spi2PortC =
    spi::Wiring<SPI2, Af5<'B', 10>, Af5<'C', 2>, Af5<'C', 3>, Stream<1, 4>, Stream<1, 3>>;
/// SPI4 on port E (SCK PE2, CN9-14), over S3.
pub type Spi4PortE =
    spi::Wiring<SPI4, Af5<'E', 2>, Af5<'E', 5>, Af5<'E', 6>, Stream<2, 1>, Stream<2, 0>>;
/// SPI5 on port F (SCK PF7, CN9-26), over S4.
pub type Spi5PortF =
    spi::Wiring<SPI5, Af5<'F', 7>, Af5<'F', 8>, Af5<'F', 9>, Stream<2, 4>, Stream<2, 3>>;

pub struct RunF {
    pub rcc: Rcc,
    pub exti: exti::Parts,
    pub spi1: Spi1PortB,
    pub spi2: Spi2PortC,
    pub spi4: Spi4PortE,
    pub spi5: Spi5PortF,
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

pub fn run_f(p: Peripherals) -> Result<RunF, ClockError> {
    let mut rcc = freeze(p.RCC)?;
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut e = p.GPIOE.split(&mut rcc);
    let mut f = p.GPIOF.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    let exti = p.EXTI.split(p.SYSCFG.split(&mut rcc).exticr);
    Ok(RunF {
        exti,
        spi1: spi::Wiring {
            spi: p.SPI1,
            sck: sck(b.p3, &mut b.ctrl),
            miso: miso(b.p4, &mut b.ctrl),
            mosi: sck(b.p5, &mut b.ctrl),
            tx_dma: dma2.s5,
            rx_dma: dma2.s2,
        },
        spi2: spi::Wiring {
            spi: p.SPI2,
            sck: sck(b.p10, &mut b.ctrl),
            miso: miso(c.p2, &mut c.ctrl),
            mosi: sck(c.p3, &mut c.ctrl),
            tx_dma: dma1.s4,
            rx_dma: dma1.s3,
        },
        spi4: spi::Wiring {
            spi: p.SPI4,
            sck: sck(e.p2, &mut e.ctrl),
            miso: miso(e.p5, &mut e.ctrl),
            mosi: sck(e.p6, &mut e.ctrl),
            tx_dma: dma2.s1,
            rx_dma: dma2.s0,
        },
        spi5: spi::Wiring {
            spi: p.SPI5,
            sck: sck(f.p7, &mut f.ctrl),
            miso: miso(f.p8, &mut f.ctrl),
            mosi: sck(f.p9, &mut f.ctrl),
            tx_dma: dma2.s4,
            rx_dma: dma2.s3,
        },
        rcc,
    })
}
