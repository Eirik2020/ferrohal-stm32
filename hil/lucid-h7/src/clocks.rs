//! The board's clock tree, shared by its firmwares.

use ferrohal_stm32::{
    rcc::{
        AhbPrescaler, ApbPrescaler, Clocks, Config as ClockConfig, Hse, HseMode, Pll, PllSource,
        Supply, Sysclk, VoltageScale,
    },
    time::Hertz,
};

/// The board's 8 MHz crystal to a 400 MHz core: 800 MHz wide VCO, 200 MHz AXI
/// and AHB, every APB at 100 MHz, 100 MHz on PLL1Q for SPI1 to SPI3. USB runs
/// from HSI48.
pub const CLOCK_CONFIG: ClockConfig = ClockConfig {
    hse: Some(Hse {
        freq: Hertz::mhz(8),
        mode: HseMode::Oscillator,
    }),
    pll1: Some(Pll {
        source: PllSource::Hse,
        prediv: 1,
        mul: 100,
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
pub const CLOCKS: Clocks = match CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("CLOCK_CONFIG breaks the H743's limits"),
};

/// A configuration that must refuse: `Pll1P` as the system clock with no
/// PLL1 configured at all, as the Nucleo-H753ZI's run U refuses.
pub const REFUSAL_CONFIG: ClockConfig = ClockConfig {
    hse: Some(Hse {
        freq: Hertz::mhz(8),
        mode: HseMode::Oscillator,
    }),
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
    REFUSAL_CONFIG.validate(),
    Err(ferrohal_stm32::rcc::ClockError::PllNotConfigured)
));
