//! RCC on the F4 clock tree (`rcc_f4`): the explicit clock configuration,
//! and the single owner of every shared RCC register.
//!
//! The user states every source, divider and multiplier. [`Config::validate`]
//! checks them against the chip's limits and computes the resulting
//! [`Clocks`]; it is a `const fn`, so a firmware can do it at compile time
//! and have its clock tree reviewed as a constant. [`RCC::freeze`] then
//! applies the configuration and returns [`Rcc`], the owner drivers borrow
//! to enable and reset their peripherals.
//!
//! The regulator scale (PWR VOS) is not written. The chips enabled so far
//! reach their maximum clock in the scale they reset to; a chip that needs
//! another scale must add it here, checked against its reference manual.
//!
//! The limits and the flash wait-state rule are ported from embassy-stm32
//! 0.6.0's `rcc/f247.rs` (MIT OR Apache-2.0, Embassy project contributors).
//! See `THIRD-PARTY.md`.

use stm32_metapac::{
    FLASH, RCC as RCC_REGS,
    flash::vals::Latency,
    rcc::vals::{Hpre, Pllm, Plln, Pllp, Pllq, Pllsrc, Ppre, Sw},
};

use crate::{chip::CLOCK_LIMITS, time::Hertz};

/// The HSI frequency on every F4.
pub const HSI: Hertz = Hertz(16_000_000);

/// Spins allowed for each wait below. One spin takes at least one cycle,
/// and these run at no more than 16 MHz (HSI) until the switch, so each
/// count is a floor on the time allowed: HSE gets at least 100 ms to start,
/// the PLL at least 6 ms to lock.
const HSE_SPINS: u32 = 1_600_000;
const PLL_SPINS: u32 = 100_000;
const SWITCH_SPINS: u32 = 10_000;
const LATENCY_SPINS: u32 = 1_000;

/// The RCC token from `Peripherals`.
pub struct RCC {
    _private: (),
}

impl RCC {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }

    /// Applies `config` and returns the owner of RCC's shared registers.
    ///
    /// Validation runs first, so a refused configuration writes nothing.
    /// Every wait on the hardware is counted; a source that does not start
    /// is reported, with the system still running from HSI.
    pub fn freeze(self, config: &Config) -> Result<Rcc, ClockError> {
        let clocks = config.validate()?;
        apply(config, &clocks)?;
        Ok(Rcc { clocks })
    }
}

/// The owner of RCC's shared enable and reset registers, and of the frozen
/// clock frequencies. Drivers take `&mut Rcc` at construction, so these
/// read-modify-writes never race: firmware does them in `init`, and a layer
/// that builds drivers at runtime wraps this owner in its own lock.
pub struct Rcc {
    clocks: Clocks,
}

impl Rcc {
    /// The frequencies `freeze` set.
    pub fn clocks(&self) -> Clocks {
        self.clocks
    }

    /// Turns a peripheral's clock on. The read back is the delay RM0368
    /// requires before the peripheral's registers are touched.
    pub(crate) fn enable(&mut self, bit: RccBit) {
        let mask = 1 << bit.bit;
        match bit.bus {
            Bus::Ahb1 => {
                RCC_REGS.ahb1enr().modify(|w| w.0 |= mask);
                let _ = RCC_REGS.ahb1enr().read();
            }
            Bus::Ahb2 => {
                RCC_REGS.ahb2enr().modify(|w| w.0 |= mask);
                let _ = RCC_REGS.ahb2enr().read();
            }
            Bus::Apb1 => {
                RCC_REGS.apb1enr().modify(|w| w.0 |= mask);
                let _ = RCC_REGS.apb1enr().read();
            }
            Bus::Apb2 => {
                RCC_REGS.apb2enr().modify(|w| w.0 |= mask);
                let _ = RCC_REGS.apb2enr().read();
            }
        }
    }

    /// Pulses a peripheral's reset, returning its registers to reset values.
    pub(crate) fn reset(&mut self, bit: RccBit) {
        let mask = 1 << bit.bit;
        match bit.bus {
            Bus::Ahb1 => {
                RCC_REGS.ahb1rstr().modify(|w| w.0 |= mask);
                RCC_REGS.ahb1rstr().modify(|w| w.0 &= !mask);
            }
            Bus::Ahb2 => {
                RCC_REGS.ahb2rstr().modify(|w| w.0 |= mask);
                RCC_REGS.ahb2rstr().modify(|w| w.0 &= !mask);
            }
            Bus::Apb1 => {
                RCC_REGS.apb1rstr().modify(|w| w.0 |= mask);
                RCC_REGS.apb1rstr().modify(|w| w.0 &= !mask);
            }
            Bus::Apb2 => {
                RCC_REGS.apb2rstr().modify(|w| w.0 |= mask);
                RCC_REGS.apb2rstr().modify(|w| w.0 &= !mask);
            }
        }
    }
}

/// The bus a peripheral sits on: which enable and reset registers hold its
/// bits, and which clock it runs from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Bus {
    Ahb1,
    Ahb2,
    Apb1,
    Apb2,
}

/// A peripheral's bit in its bus's enable and reset registers (the same
/// position in both). Chip facts: the glue layer supplies them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RccBit {
    pub(crate) bus: Bus,
    pub(crate) bit: u8,
}

impl RccBit {
    pub(crate) const fn new(bus: Bus, bit: u8) -> Self {
        Self { bus, bit }
    }
}

/// The frequencies a configuration produces. Drivers compute their
/// dividers from these and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Clocks {
    pub sysclk: Hertz,
    /// The core clock: HCLK on every F4.
    pub cpu: Hertz,
    pub hclk: Hertz,
    pub pclk1: Hertz,
    pub pclk2: Hertz,
    /// The clock of timers on APB1: twice `pclk1` when APB1 is divided.
    pub timclk1: Hertz,
    pub timclk2: Hertz,
    /// The PLL's Q output (USB, SDIO), when the PLL is on.
    pub pll_q: Option<Hertz>,
    pub(crate) flash_wait_states: u8,
}

impl Clocks {
    /// The clock of peripherals on `bus`.
    pub const fn bus(&self, bus: Bus) -> Hertz {
        match bus {
            Bus::Ahb1 | Bus::Ahb2 => self.hclk,
            Bus::Apb1 => self.pclk1,
            Bus::Apb2 => self.pclk2,
        }
    }
}

/// A complete clock tree. Nothing is chosen for the user: every divider is
/// stated, and [`validate`](Self::validate) only checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    pub hse: Option<Hse>,
    pub pll: Option<Pll>,
    pub sys: Sysclk,
    pub ahb_pre: AhbPrescaler,
    pub apb1_pre: ApbPrescaler,
    pub apb2_pre: ApbPrescaler,
    /// Sets the flash wait states for the resulting HCLK.
    pub supply: Supply,
}

impl Config {
    /// The reset state: 16 MHz from HSI, nothing divided.
    pub const fn hsi() -> Self {
        Self {
            hse: None,
            pll: None,
            sys: Sysclk::Hsi,
            ahb_pre: AhbPrescaler::Div1,
            apb1_pre: ApbPrescaler::Div1,
            apb2_pre: ApbPrescaler::Div1,
            supply: Supply::V2_7To3_6,
        }
    }

    /// Checks every frequency against the chip's limits and returns them.
    pub const fn validate(&self) -> Result<Clocks, ClockError> {
        let limits = &CLOCK_LIMITS;

        if let Some(hse) = self.hse {
            let (min, max) = match hse.mode {
                HseMode::Oscillator => limits.hse_oscillator,
                HseMode::Bypass => limits.hse_bypass,
            };
            if hse.freq.0 < min.0 || hse.freq.0 > max.0 {
                return Err(ClockError::HseFrequency);
            }
        }

        let mut pll_p = None;
        let mut pll_q = None;
        if let Some(pll) = self.pll {
            let input = match pll.source {
                PllSource::Hsi => HSI,
                PllSource::Hse => match self.hse {
                    Some(hse) => hse.freq,
                    None => return Err(ClockError::HseNotConfigured),
                },
            };
            if pll.prediv < 2 || pll.prediv > 63 {
                return Err(ClockError::PllDivider);
            }
            if pll.mul < 50 || pll.mul > 432 {
                return Err(ClockError::PllMultiplier);
            }
            let vco_in = input.0 / pll.prediv as u32;
            if vco_in < limits.pll_input.0.0 || vco_in > limits.pll_input.1.0 {
                return Err(ClockError::PllInput);
            }
            // At most 2.1 MHz * 432, so this fits in a u32.
            let vco = vco_in * pll.mul as u32;
            if vco < limits.pll_vco.0.0 || vco > limits.pll_vco.1.0 {
                return Err(ClockError::PllVco);
            }
            pll_p = Some(Hertz(vco / pll.divp.divisor()));
            if let Some(divq) = pll.divq {
                if divq < 2 || divq > 15 {
                    return Err(ClockError::PllDivider);
                }
                pll_q = Some(Hertz(vco / divq as u32));
            }
        }

        let sysclk = match self.sys {
            Sysclk::Hsi => HSI,
            Sysclk::Hse => match self.hse {
                Some(hse) => hse.freq,
                None => return Err(ClockError::HseNotConfigured),
            },
            Sysclk::PllP => match pll_p {
                Some(freq) => freq,
                None => return Err(ClockError::PllNotConfigured),
            },
        };
        if sysclk.0 > limits.sysclk_max.0 {
            return Err(ClockError::Sysclk);
        }
        let hclk = Hertz(sysclk.0 / self.ahb_pre.divisor());
        if hclk.0 > limits.hclk_max.0 {
            return Err(ClockError::Hclk);
        }
        let pclk1 = Hertz(hclk.0 / self.apb1_pre.divisor());
        if pclk1.0 > limits.pclk1_max.0 {
            return Err(ClockError::Pclk1);
        }
        let pclk2 = Hertz(hclk.0 / self.apb2_pre.divisor());
        if pclk2.0 > limits.pclk2_max.0 {
            return Err(ClockError::Pclk2);
        }

        let wait_states = match self.supply {
            Supply::V2_7To3_6 => (hclk.0.saturating_sub(1) / limits.flash_step_2v7.0) as u8,
        };
        if wait_states > limits.max_wait_states {
            return Err(ClockError::Hclk);
        }

        Ok(Clocks {
            sysclk,
            cpu: hclk,
            hclk,
            pclk1,
            pclk2,
            timclk1: timer_clock(pclk1, self.apb1_pre),
            timclk2: timer_clock(pclk2, self.apb2_pre),
            pll_q,
            flash_wait_states: wait_states,
        })
    }
}

/// A timer behind a divided APB runs at twice that bus.
const fn timer_clock(pclk: Hertz, pre: ApbPrescaler) -> Hertz {
    match pre {
        ApbPrescaler::Div1 => pclk,
        _ => Hertz(pclk.0 * 2),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Hse {
    pub freq: Hertz,
    pub mode: HseMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum HseMode {
    /// A crystal or resonator on OSC_IN and OSC_OUT.
    Oscillator,
    /// An external clock on OSC_IN, such as an ST-LINK's MCO.
    Bypass,
}

/// The main PLL: `source / prediv * mul`, then `/ divp` to SYSCLK and
/// `/ divq` to the 48 MHz domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Pll {
    pub source: PllSource,
    /// PLLM, 2 to 63.
    pub prediv: u8,
    /// PLLN, 50 to 432.
    pub mul: u16,
    pub divp: PllP,
    /// PLLQ, 2 to 15. `None` leaves the field at 2 and reports no Q clock.
    pub divq: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PllSource {
    Hsi,
    Hse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PllP {
    Div2,
    Div4,
    Div6,
    Div8,
}

impl PllP {
    const fn divisor(self) -> u32 {
        match self {
            Self::Div2 => 2,
            Self::Div4 => 4,
            Self::Div6 => 6,
            Self::Div8 => 8,
        }
    }

    const fn bits(self) -> Pllp {
        match self {
            Self::Div2 => Pllp::DIV2,
            Self::Div4 => Pllp::DIV4,
            Self::Div6 => Pllp::DIV6,
            Self::Div8 => Pllp::DIV8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Sysclk {
    Hsi,
    Hse,
    PllP,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AhbPrescaler {
    Div1,
    Div2,
    Div4,
    Div8,
    Div16,
    Div64,
    Div128,
    Div256,
    Div512,
}

impl AhbPrescaler {
    const fn divisor(self) -> u32 {
        match self {
            Self::Div1 => 1,
            Self::Div2 => 2,
            Self::Div4 => 4,
            Self::Div8 => 8,
            Self::Div16 => 16,
            Self::Div64 => 64,
            Self::Div128 => 128,
            Self::Div256 => 256,
            Self::Div512 => 512,
        }
    }

    const fn bits(self) -> Hpre {
        match self {
            Self::Div1 => Hpre::DIV1,
            Self::Div2 => Hpre::DIV2,
            Self::Div4 => Hpre::DIV4,
            Self::Div8 => Hpre::DIV8,
            Self::Div16 => Hpre::DIV16,
            Self::Div64 => Hpre::DIV64,
            Self::Div128 => Hpre::DIV128,
            Self::Div256 => Hpre::DIV256,
            Self::Div512 => Hpre::DIV512,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ApbPrescaler {
    Div1,
    Div2,
    Div4,
    Div8,
    Div16,
}

impl ApbPrescaler {
    const fn divisor(self) -> u32 {
        match self {
            Self::Div1 => 1,
            Self::Div2 => 2,
            Self::Div4 => 4,
            Self::Div8 => 8,
            Self::Div16 => 16,
        }
    }

    const fn bits(self) -> Ppre {
        match self {
            Self::Div1 => Ppre::DIV1,
            Self::Div2 => Ppre::DIV2,
            Self::Div4 => Ppre::DIV4,
            Self::Div8 => Ppre::DIV8,
            Self::Div16 => Ppre::DIV16,
        }
    }
}

/// The supply range, which sets how many flash wait states HCLK needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Supply {
    /// 2.7 V to 3.6 V: one wait state per 30 MHz.
    V2_7To3_6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ClockError {
    HseFrequency,
    HseNotConfigured,
    PllNotConfigured,
    PllDivider,
    PllMultiplier,
    /// The PLL input after PLLM is outside the chip's range.
    PllInput,
    /// The VCO frequency is outside the chip's range.
    PllVco,
    Sysclk,
    Hclk,
    Pclk1,
    Pclk2,
    /// HSE did not report ready within its bound. The system is still on HSI.
    HseTimeout,
    /// The PLL did not lock within its bound. The system is still on HSI.
    PllTimeout,
    /// A clock switch or the flash latency did not take within its bound.
    SwitchTimeout,
}

/// A chip's clock limits, from its datasheet. Supplied by the glue layer.
pub(crate) struct Limits {
    pub sysclk_max: Hertz,
    pub hclk_max: Hertz,
    pub pclk1_max: Hertz,
    pub pclk2_max: Hertz,
    pub hse_oscillator: (Hertz, Hertz),
    pub hse_bypass: (Hertz, Hertz),
    pub pll_input: (Hertz, Hertz),
    pub pll_vco: (Hertz, Hertz),
    /// HCLK per flash wait state at 2.7 V to 3.6 V.
    pub flash_step_2v7: Hertz,
    pub max_wait_states: u8,
}

/// Spins until `done` or `spins` run out.
fn wait(spins: u32, mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..spins {
        if done() {
            return true;
        }
    }
    done()
}

fn apply(config: &Config, clocks: &Clocks) -> Result<(), ClockError> {
    // Run from HSI while everything else is reconfigured.
    RCC_REGS.cr().modify(|w| w.set_hsion(true));
    if !wait(SWITCH_SPINS, || RCC_REGS.cr().read().hsirdy()) {
        return Err(ClockError::SwitchTimeout);
    }
    RCC_REGS.cfgr().modify(|w| w.set_sw(Sw::HSI));
    if !wait(SWITCH_SPINS, || RCC_REGS.cfgr().read().sws() == Sw::HSI) {
        return Err(ClockError::SwitchTimeout);
    }

    if let Some(hse) = config.hse {
        // HSEBYP may only change while HSE is off.
        RCC_REGS.cr().modify(|w| w.set_hseon(false));
        if !wait(HSE_SPINS, || !RCC_REGS.cr().read().hserdy()) {
            return Err(ClockError::HseTimeout);
        }
        RCC_REGS
            .cr()
            .modify(|w| w.set_hsebyp(hse.mode == HseMode::Bypass));
        RCC_REGS.cr().modify(|w| w.set_hseon(true));
        if !wait(HSE_SPINS, || RCC_REGS.cr().read().hserdy()) {
            return Err(ClockError::HseTimeout);
        }
    }

    RCC_REGS.cr().modify(|w| w.set_pllon(false));
    if !wait(PLL_SPINS, || !RCC_REGS.cr().read().pllrdy()) {
        return Err(ClockError::PllTimeout);
    }
    if let Some(pll) = config.pll {
        RCC_REGS.pllcfgr().modify(|w| {
            w.set_pllsrc(match pll.source {
                PllSource::Hsi => Pllsrc::HSI,
                PllSource::Hse => Pllsrc::HSE,
            });
            w.set_pllm(Pllm::from_bits(pll.prediv));
            w.set_plln(Plln::from_bits(pll.mul));
            w.set_pllp(pll.divp.bits());
            w.set_pllq(Pllq::from_bits(pll.divq.unwrap_or(2)));
        });
        RCC_REGS.cr().modify(|w| w.set_pllon(true));
        if !wait(PLL_SPINS, || RCC_REGS.cr().read().pllrdy()) {
            return Err(ClockError::PllTimeout);
        }
    }

    // HSI needs no wait state, so the target's count is always enough here.
    // Set it before the clock goes up.
    let latency = Latency::from_bits(clocks.flash_wait_states);
    FLASH.acr().modify(|w| {
        w.set_latency(latency);
        w.set_prften(true);
        w.set_icen(true);
        w.set_dcen(true);
    });
    if !wait(LATENCY_SPINS, || FLASH.acr().read().latency() == latency) {
        return Err(ClockError::SwitchTimeout);
    }

    let sw = match config.sys {
        Sysclk::Hsi => Sw::HSI,
        Sysclk::Hse => Sw::HSE,
        Sysclk::PllP => Sw::PLL1_P,
    };
    RCC_REGS.cfgr().modify(|w| {
        w.set_hpre(config.ahb_pre.bits());
        w.set_ppre1(config.apb1_pre.bits());
        w.set_ppre2(config.apb2_pre.bits());
        w.set_sw(sw);
    });
    if !wait(SWITCH_SPINS, || RCC_REGS.cfgr().read().sws() == sw) {
        return Err(ClockError::SwitchTimeout);
    }
    Ok(())
}
