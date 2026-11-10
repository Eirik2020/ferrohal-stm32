//! The board's clock tree, shared by its firmwares.

use ferrohal_stm32::{
    rcc::{
        AhbPrescaler, ApbPrescaler, ClockError, Clocks, Config as ClockConfig, Hse, HseMode, Pll,
        PllP, PllSource, Supply, Sysclk,
    },
    time::Hertz,
};

/// The board's 8 MHz crystal to 168 MHz, with 48 MHz on PLLQ for USB.
pub const CLOCK_CONFIG: ClockConfig = ClockConfig {
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
pub const CLOCKS: Clocks = match CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("CLOCK_CONFIG breaks the F405's limits"),
};

/// A configuration that must refuse: `PllP` as the system clock with no PLL
/// configured at all, as the Nucleo-F401's run V refuses.
pub const REFUSAL_CONFIG: ClockConfig = ClockConfig {
    hse: Some(Hse {
        freq: Hertz::mhz(8),
        mode: HseMode::Oscillator,
    }),
    pll: None,
    sys: Sysclk::PllP,
    ahb_pre: AhbPrescaler::Div1,
    apb1_pre: ApbPrescaler::Div4,
    apb2_pre: ApbPrescaler::Div2,
    supply: Supply::V2_7To3_6,
};
const _: () = assert!(matches!(
    REFUSAL_CONFIG.validate(),
    Err(ClockError::PllNotConfigured)
));

