//! The F4's clock tree, which the F7 shares (RM0090 6.2, RM0431 5.2): HSI,
//! HSE and one main PLL (PLLM, PLLN, then P to SYSCLK and Q to the 48 MHz
//! domain), one AHB and two APBs. Its configuration, validation and solver,
//! mounted by `rcc_f4` and `rcc_f7`, each of which writes it to its own
//! registers. Where the trees differ, the F7's over-drive, the code says so.
//!
//! The limits and the flash wait-state rule are ported from embassy-stm32
//! 0.6.0's `rcc/f247.rs` (MIT OR Apache-2.0, Embassy project contributors).
//! See `THIRD-PARTY.md`.
//!
//! Requirements: RCC-001, RCC-002, RCC-003, RCC-004

use stm32_metapac::{
    pwr::vals::Vos,
    rcc::vals::{Hpre, Pllp, Ppre},
};

use super::{AhbPrescaler, ApbPrescaler, ClockError, Hse, HseMode, PllSource};
use crate::{chip::CLOCK_LIMITS, time::Hertz};

/// The HSI frequency on every F4 and F7.
pub const HSI: Hertz = Hertz(16_000_000);

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
    /// Whether HCLK needs the regulator's over-drive (RM0390 5.1.4, RM0431
    /// 4.1.4), which `freeze` then turns on. Never on a chip without one.
    pub(crate) overdrive: bool,
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

    /// The counter clock of timers on `bus`: twice the bus clock when its
    /// prescaler divides, as `timclk1` and `timclk2` record.
    pub const fn timer(&self, bus: Bus) -> Hertz {
        match bus {
            Bus::Apb1 => self.timclk1,
            Bus::Apb2 => self.timclk2,
            Bus::Ahb1 | Bus::Ahb2 => self.hclk,
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
        let (pclk1_max, pclk2_max) = limits.apb_max(hclk);
        if pclk1.0 > pclk1_max.0 {
            return Err(ClockError::Pclk1);
        }
        let pclk2 = Hertz(hclk.0 / self.apb2_pre.divisor());
        if pclk2.0 > pclk2_max.0 {
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
            overdrive: limits.needs_overdrive(hclk),
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

/// What a board asks the clocks for: its oscillator, the system frequency,
/// and, when the board has a USB or SDIO port, the frequency that port
/// needs — 48 MHz on every F4, taken exactly from the PLL's Q output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Request {
    /// The board's oscillator.
    pub hse: Hse,
    /// The system frequency.
    pub sysclk: Hertz,
    /// What the USB or SDIO port needs, when the board has one.
    pub usb: Option<Hertz>,
}

/// What the VCO search settles on: the PLLM, the multiplier, the P and Q
/// dividers, and the buses' prescalers.
type SolvedVco = (
    u32,
    u32,
    PllP,
    Option<u8>,
    AhbPrescaler,
    ApbPrescaler,
    ApbPrescaler,
);

/// The smallest APB divisor that keeps `hclk / divisor` within `max`, from
/// the divisors the APB prescalers offer.
const fn apb_pre(hclk: u32, max: u32) -> Option<ApbPrescaler> {
    let mut divisor = 1;
    while divisor <= 16 {
        if hclk / divisor <= max {
            return apb_pre_of(divisor);
        }
        divisor *= 2;
    }
    None
}

const fn apb_pre_of(divisor: u32) -> Option<ApbPrescaler> {
    match divisor {
        1 => Some(ApbPrescaler::Div1),
        2 => Some(ApbPrescaler::Div2),
        4 => Some(ApbPrescaler::Div4),
        8 => Some(ApbPrescaler::Div8),
        16 => Some(ApbPrescaler::Div16),
        _ => None,
    }
}

/// The smallest AHB divisor that keeps `sysclk / divisor` within `max`.
const fn ahb_pre(sysclk: u32, max: u32) -> Option<AhbPrescaler> {
    let mut divisor = 1;
    while divisor <= 512 {
        if sysclk / divisor <= max {
            return ahb_pre_of(divisor);
        }
        divisor *= if divisor < 16 { 2 } else { 4 };
    }
    None
}

const fn ahb_pre_of(divisor: u32) -> Option<AhbPrescaler> {
    match divisor {
        1 => Some(AhbPrescaler::Div1),
        2 => Some(AhbPrescaler::Div2),
        4 => Some(AhbPrescaler::Div4),
        8 => Some(AhbPrescaler::Div8),
        16 => Some(AhbPrescaler::Div16),
        64 => Some(AhbPrescaler::Div64),
        128 => Some(AhbPrescaler::Div128),
        256 => Some(AhbPrescaler::Div256),
        512 => Some(AhbPrescaler::Div512),
        _ => None,
    }
}

impl Config {
    /// Computes a configuration for `request`: the PLL from the HSE, with
    /// its input the oscillator's smallest division inside the chip's input
    /// range, and the VCO the smallest that both the system clock (through
    /// P) and the USB clock (through Q, exactly) divide down from. The AHB
    /// and APB prescalers are the smallest divisors that keep every bus
    /// within its limit, so the timers behind a divided APB run as fast as
    /// the chip allows. The result is validated before it is returned: what
    /// `solve` returns always freezes.
    ///
    /// This is a convenience over [`Config`]'s stated dividers, not a
    /// replacement: a board that wants a particular tree states it, and
    /// [`validate`](Self::validate) checks it as ever.
    pub const fn solve(request: Request) -> Result<Self, ClockError> {
        let limits = &CLOCK_LIMITS;
        let sysclk = request.sysclk.0;
        if sysclk > limits.sysclk_max.0 || sysclk == 0 {
            return Err(ClockError::Sysclk);
        }
        let hse_freq = request.hse.freq.0;
        // The PLL input: the largest PLLM that keeps it inside the chip's
        // range, tried largest first — an oscillator that divides to exactly
        // the input range's lower bound (1 MHz on a whole-MHz crystal) is
        // tried first.
        let mut prediv = hse_freq / limits.pll_input.0.0;
        if prediv > 63 {
            prediv = 63;
        }
        while prediv >= 2 {
            let vco_in = hse_freq / prediv;
            if vco_in > limits.pll_input.1.0 {
                // A smaller PLLM only raises the input further.
                break;
            }
            if vco_in < limits.pll_input.0.0 {
                prediv -= 1;
                continue;
            }
            let solved = Self::solve_vco(request, limits, sysclk, prediv, vco_in);
            match solved {
                Some((prediv, mul, divp, divq, ahb_pre, apb1_pre, apb2_pre)) => {
                    let config = Self {
                        hse: Some(request.hse),
                        pll: Some(Pll {
                            source: PllSource::Hse,
                            prediv: prediv as u8,
                            mul: mul as u16,
                            divp,
                            divq,
                        }),
                        sys: Sysclk::PllP,
                        ahb_pre,
                        apb1_pre,
                        apb2_pre,
                        supply: Supply::V2_7To3_6,
                    };
                    // What `solve` returns always freezes.
                    return match config.validate() {
                        Ok(_) => Ok(config),
                        Err(error) => Err(error),
                    };
                }
                None => prediv -= 1,
            }
        }
        Err(ClockError::Unsatisfiable)
    }
}

impl Config {
    /// The smallest VCO on the input `vco_in` that the system clock (through
    /// P) and the USB clock (through Q) both divide down from, with the
    /// buses' smallest prescalers alongside. Returns the PLLM it used, so
    /// the caller can go on to the next input when this one has no solution.
    const fn solve_vco(
        request: Request,
        limits: &Limits,
        sysclk: u32,
        prediv: u32,
        vco_in: u32,
    ) -> Option<SolvedVco> {
        let mut mul = 50;
        while mul <= 432 {
            let vco = vco_in * mul;
            if vco > limits.pll_vco.1.0 {
                break;
            }
            if vco < limits.pll_vco.0.0 {
                mul += 1;
                continue;
            }
            // The smallest P divisor that gives the system clock.
            let divp = if vco.is_multiple_of(2) && vco / 2 == sysclk {
                Some(PllP::Div2)
            } else if vco.is_multiple_of(4) && vco / 4 == sysclk {
                Some(PllP::Div4)
            } else if vco.is_multiple_of(6) && vco / 6 == sysclk {
                Some(PllP::Div6)
            } else if vco.is_multiple_of(8) && vco / 8 == sysclk {
                Some(PllP::Div8)
            } else {
                None
            };
            let divp = match divp {
                Some(divp) => divp,
                None => {
                    mul += 1;
                    continue;
                }
            };
            // Q: the USB clock, exactly.
            let divq = match request.usb {
                None => None,
                Some(usb) => {
                    let mut q = 2;
                    let mut found = None;
                    while q <= 15 {
                        if vco.is_multiple_of(q) && vco / q == usb.0 {
                            found = Some(q as u8);
                        }
                        q += 1;
                    }
                    match found {
                        Some(divq) => Some(divq),
                        None => {
                            mul += 1;
                            continue;
                        }
                    }
                }
            };
            // The buses: the smallest divisors inside their limits.
            let ahb_pre = match ahb_pre(sysclk, limits.hclk_max.0) {
                Some(ahb_pre) => ahb_pre,
                None => return None,
            };
            let hclk = sysclk / ahb_pre.divisor();
            let (pclk1_max, pclk2_max) = limits.apb_max(Hertz(hclk));
            let apb1_pre = match apb_pre(hclk, pclk1_max.0) {
                Some(apb1_pre) => apb1_pre,
                None => return None,
            };
            let apb2_pre = match apb_pre(hclk, pclk2_max.0) {
                Some(apb2_pre) => apb2_pre,
                None => return None,
            };
            return Some((prediv, mul, divp, divq, ahb_pre, apb1_pre, apb2_pre));
        }
        None
    }
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

    pub(super) const fn bits(self) -> Pllp {
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

impl AhbPrescaler {
    pub(super) const fn bits(self) -> Hpre {
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

impl ApbPrescaler {
    pub(super) const fn bits(self) -> Ppre {
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
    /// The regulator voltage scale these limits hold in, as PWR_CR's VOS
    /// field: `freeze` writes it before the PLL starts. It must be a value
    /// the chip's reference manual defines, never a reserved one.
    pub vos: Vos,
    /// The regulator's over-drive, on the chips that have one (the F446,
    /// F427/F429 and every F7; RM0390 5.1.3, RM0431 4.1.4): the HCLK above
    /// which it is needed, and the APB1 and APB2 limits while it is on.
    /// `pclk1_max` and `pclk2_max` hold with it off. `None`: no over-drive,
    /// and `freeze` never touches its bits.
    pub overdrive: Option<Overdrive>,
}

/// A chip's over-drive limits.
pub(crate) struct Overdrive {
    /// HCLK above this needs the over-drive.
    pub above: Hertz,
    pub pclk1_max: Hertz,
    pub pclk2_max: Hertz,
}

impl Limits {
    /// Whether `hclk` needs the over-drive: never on a chip without one.
    pub(crate) const fn needs_overdrive(&self, hclk: Hertz) -> bool {
        match &self.overdrive {
            Some(overdrive) => hclk.0 > overdrive.above.0,
            None => false,
        }
    }

    /// The APB1 and APB2 limits at `hclk`: higher with the over-drive on,
    /// which `hclk` decides; fixed on a chip without one.
    pub(crate) const fn apb_max(&self, hclk: Hertz) -> (Hertz, Hertz) {
        match &self.overdrive {
            Some(overdrive) if hclk.0 > overdrive.above.0 => {
                (overdrive.pclk1_max, overdrive.pclk2_max)
            }
            _ => (self.pclk1_max, self.pclk2_max),
        }
    }
}
