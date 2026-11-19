//! Nucleo-F767ZI (MB1137): the clock tree, and which pins each run uses.
//! This file is chip-specific; FerroHAL's routing traits check every choice
//! here at compile time.
//!
//! Runs A to C need no links: A the clock tree (voltage scale and over-drive)
//! and every timer against the DWT, B the independent watchdog across the reset
//! it causes, C the I2C master on I2C1 (PB8 and PB9, Zio CN7-2 and CN7-4, the
//! Arduino SCL and SDA), pulled up internally, no slave fitted. Run D uses the
//! same pins with the second Pico simulator fitted (`hil/WIRING.md`), plus
//! DMA1 for the interrupt-driven I2C master. The SPI links are in
//! `docs/plans/2026-11-17-f411-f722-f745.md` until their runs exist.
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
        I2C1, Peripherals, TIM1, TIM2, TIM3, TIM4, TIM5, TIM8, TIM9, TIM10, TIM11, TIM12, TIM13,
        TIM14,
    },
    dma::Stream,
    gpio::{Alternate, OpenDrain, Output, Pin, Pull},
    i2c,
    rcc::{
        AhbPrescaler, ApbPrescaler, ClockError, Clocks, Config, Pll, PllP, PllSource, Rcc, Supply,
        Sysclk,
    },
    time::Hertz,
    timer::{Tick, Timebase},
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

// Run D: run C's pins with the second Pico simulator fitted, and DMA1 for the
// interrupt-driven master: I2C1_TX on stream 6, I2C1_RX on stream 0, both
// channel 1 (RM0410 Table 27).

pub struct RunD {
    pub rcc: Rcc,
    pub led: Led,
    pub i2c: I2c1Bus,
    pub tx: Stream<1, 6>,
    pub rx: Stream<1, 0>,
}

pub fn run_d(p: Peripherals) -> Result<RunD, ClockError> {
    let mut rcc = freeze(p.RCC)?;
    let dma1 = p.DMA1.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let led = b.p7.into_push_pull_output(&mut b.ctrl);
    // Pulled up here and at the Pico's end, as the H753's run S.
    let mut scl: Pin<'B', 8, Alternate<4, OpenDrain>> = b.p8.into_alternate_open_drain(&mut b.ctrl);
    scl.set_pull(&mut b.ctrl, Pull::Up);
    let mut sda: Pin<'B', 9, Alternate<4, OpenDrain>> = b.p9.into_alternate_open_drain(&mut b.ctrl);
    sda.set_pull(&mut b.ctrl, Pull::Up);
    Ok(RunD {
        rcc,
        led,
        i2c: i2c::Wiring {
            i2c: p.I2C1,
            scl,
            sda,
        },
        tx: dma1.s6,
        rx: dma1.s0,
    })
}
