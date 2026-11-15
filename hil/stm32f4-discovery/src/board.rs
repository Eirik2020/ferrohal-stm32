//! STM32F4 Discovery (MB997, STM32F407VGT6): die 0x413's debugger bench.
//! The F401 proves every F4 IP version; this board proves what is particular
//! to die 0x413 and what the F401 cannot reach (the car rule,
//! `docs/design/requirements-system.md`): the die's own UART routes, its
//! clock tree at 168 MHz, CCM RAM's refusal, the F4's USB core (USB-F4-001)
//! and the ROM's USB DFU (RESET-004).
//!
//! The links, on the P1/P2 headers (UM1472 Table 7):
//!
//! | Link | TX -> RX | Header pins | Carries |
//! | --- | --- | --- | --- |
//! | D1 | PC10 -> PC11 | P2-37 -> P2-38 | USART3 (AF7) or UART4 (AF8) |
//! | D2 | PC12 -> PD2 | P2-35 -> P2-34 | UART5 (AF8) |
//! | D3 | PD8 -> PD9 | P1-40 -> P1-41 | USART3 (AF7), LQFP100 only |
//!
//! PC10 and PC12 also reach the CS43L22 audio DAC's I2S clock and data inputs;
//! every run holds its reset, PD4, low, which powers it down (CS43L22
//! datasheet: inputs leak at most 10 uA). CN5, the F407's own USB port, is
//! cabled for runs B and C.
//!
//! The clock: the ST-LINK's 8 MHz MCO on PH0, in bypass. UM1472 7.11.1 fits
//! both R68 (the MCO) and R25 (the X2 crystal) by default, so crystal mode is
//! not isolated on this board; the Foxeer proves it.

// Every run binary compiles this file and uses its own run's wiring only.
#![allow(dead_code)]

use ferrohal_stm32::{
    chip::{Peripherals, TIM1, TIM2, TIM8, UART4, UART5, USART3, USB_OTG_FS},
    dma::Stream,
    gpio::{Alternate, Ctrl, Output, Pin, Speed},
    rcc::{
        AhbPrescaler, ApbPrescaler, ClockError, Clocks, Config, Hse, HseMode, Pll, PllP, PllSource,
        Rcc, Supply, Sysclk,
    },
    time::Hertz,
    uart::Wiring,
};

/// 168 MHz, the die's ceiling, as the Foxeer's tree but with the MCO in
/// bypass: 8 MHz / 8 * 336 = 336 MHz VCO, / 2 = 168 MHz, / 7 = 48 MHz for USB.
pub const CLOCK_CONFIG: Config = Config {
    hse: Some(Hse {
        freq: Hertz::mhz(8),
        mode: HseMode::Bypass,
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
pub const CLOCKS: Clocks = match CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("CLOCK_CONFIG breaks the F407's limits"),
};

type Af7<const P: char, const N: u8> = Pin<P, N, Alternate<7>>;
type Af8<const P: char, const N: u8> = Pin<P, N, Alternate<8>>;

/// USART3 over D1. DMA1 stream 3 (channel 4) transmits, stream 1 receives.
pub type Usart3C = Wiring<USART3, Af7<'C', 10>, Af7<'C', 11>, Stream<1, 3>, Stream<1, 1>>;
/// UART4 over D1. DMA1 stream 4 (channel 4) transmits, stream 2 receives.
pub type Uart4C = Wiring<UART4, Af8<'C', 10>, Af8<'C', 11>, Stream<1, 4>, Stream<1, 2>>;
/// USART3 over D3, the LQFP100's pins. DMA1 stream 4 (channel 7) transmits,
/// stream 1 receives.
pub type Usart3D = Wiring<USART3, Af7<'D', 8>, Af7<'D', 9>, Stream<1, 4>, Stream<1, 1>>;
/// UART5 over D2. DMA1 stream 7 transmits, stream 0 receives.
pub type Uart5D2 = Wiring<UART5, Af8<'C', 12>, Af8<'D', 2>, Stream<1, 7>, Stream<1, 0>>;

/// LD4, the green user LED, on PD12.
pub type Led = Pin<'D', 12, Output>;

/// Run A's parts: the first route wired, and what the later routes take as
/// each frees its predecessor.
pub struct RunA {
    pub rcc: Rcc,
    pub led: Led,
    pub ctrl_c: Ctrl<'C'>,
    pub usart3: Usart3C,
    pub uart4: UART4,
    pub s4: Stream<1, 4>,
    pub s2: Stream<1, 2>,
    pub pd8: Af7<'D', 8>,
    pub pd9: Af7<'D', 9>,
    pub uart5: Uart5D2,
}

pub fn run_a(p: Peripherals) -> Result<RunA, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);
    let mut codec_reset = d.p4.into_push_pull_output(&mut d.ctrl);
    codec_reset.set_low();
    let led = d.p12.into_push_pull_output(&mut d.ctrl);
    let tx = c.p10.into_alternate::<7>(&mut c.ctrl);
    let rx = c.p11.into_alternate::<7>(&mut c.ctrl);
    let tx5 = c.p12.into_alternate::<8>(&mut c.ctrl);
    let rx5 = d.p2.into_alternate::<8>(&mut d.ctrl);
    let pd8 = d.p8.into_alternate::<7>(&mut d.ctrl);
    let pd9 = d.p9.into_alternate::<7>(&mut d.ctrl);
    Ok(RunA {
        rcc,
        led,
        ctrl_c: c.ctrl,
        usart3: Wiring {
            uart: p.USART3,
            tx,
            rx,
            tx_dma: dma1.s3,
            rx_dma: dma1.s1,
        },
        uart4: p.UART4,
        s4: dma1.s4,
        s2: dma1.s2,
        pd8,
        pd9,
        uart5: Wiring {
            uart: p.UART5,
            tx: tx5,
            rx: rx5,
            tx_dma: dma1.s7,
            rx_dma: dma1.s0,
        },
    })
}

pub struct RunB {
    pub rcc: Rcc,
    pub led: Led,
    pub usb: USB_OTG_FS,
    pub dm: Pin<'A', 11, Alternate<10>>,
    pub dp: Pin<'A', 12, Alternate<10>>,
    /// CN5's VBUS (UM1472 7.5, LD7): high with a cable in.
    pub vbus: Pin<'A', 9>,
    pub tim2: TIM2,
}

pub fn run_b(p: Peripherals) -> Result<RunB, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut a = p.GPIOA.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let mut codec_reset = d.p4.into_push_pull_output(&mut d.ctrl);
    codec_reset.set_low();
    let led = d.p12.into_push_pull_output(&mut d.ctrl);
    let mut dm = a.p11.into_alternate::<10>(&mut a.ctrl);
    dm.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let mut dp = a.p12.into_alternate::<10>(&mut a.ctrl);
    dp.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let vbus = a.p9.into_input(&mut a.ctrl);
    Ok(RunB {
        rcc,
        led,
        usb: p.USB_OTG_FS,
        dm,
        dp,
        vbus,
        tim2: p.TIM2,
    })
}

/// Run D: TIM1's complementary output CH1N on PE8 (AF1), read back by a
/// sampler: TIM8's update request has DMA2 stream 1 copy port E's input
/// register. Only DMA2 reaches the GPIO ports on the F4, and TIM8's update is
/// on DMA2, which the F401 lacks: its only break timer, TIM1, is also its only
/// timer whose update reaches DMA2. No links.
pub struct RunD {
    pub rcc: Rcc,
    pub tim1: TIM1,
    pub pe8: Pin<'E', 8, Alternate<1>>,
    pub tim8: TIM8,
    pub port_e: Stream<2, 1>,
}

pub fn run_d(p: Peripherals) -> Result<RunD, ClockError> {
    let mut rcc = p
        .RCC
        .freeze(&CLOCK_CONFIG)
        .map_err(|refused| refused.error())?;
    let mut e = p.GPIOE.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);
    Ok(RunD {
        tim1: p.TIM1,
        pe8: e.p8.into_alternate::<1>(&mut e.ctrl),
        tim8: p.TIM8,
        port_e: dma2.s1,
        rcc,
    })
}

