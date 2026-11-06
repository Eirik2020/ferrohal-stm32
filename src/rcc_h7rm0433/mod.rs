//! RCC on the H742/H743/H753/H750 clock tree (`rcc_h7rm0433`): the explicit
//! clock configuration, the power settings it depends on, and the single
//! owner of every shared RCC register.
//!
//! As on F4, the user states every source, divider and multiplier;
//! [`Config::validate`] (a `const fn`) checks them against the chip's
//! limits and computes the [`Clocks`], and [`RCC::freeze`] applies them.
//!
//! `freeze` also owns what the clock tree depends on, so `Peripherals` hands
//! out no token for it: the PWR supply configuration and voltage scale, the
//! flash wait states, the U(S)ART kernel clock muxes (set to their bus
//! clock, the only choice offered so far) and the SPI kernel clock muxes
//! (PLL1Q for SPI1 to SPI3, PCLK2 for SPI4 and SPI5).
//!
//! Limits are the ones both silicon revisions (Y and V) meet, from DS12117
//! and RM0433: VOS0 (480 MHz, rev V only) is not offered.
//!
//! Ported in part from embassy-stm32 0.6.0's `rcc/h.rs` (MIT OR
//! Apache-2.0, Embassy project contributors): the power-up order, the PLL
//! programming and the flash table. See `THIRD-PARTY.md`.
//!
//! Requirements: RCC-001, RCC-002

use stm32_metapac::{
    FLASH, PWR, RCC as RCC_REGS,
    pwr::vals::Vos,
    rcc::vals::{
        Hpre, Hsidiv, I2c4sel, I2c1235sel, Plldiv, Pllm, Plln, Pllrge, Pllsrc, Pllvcosel, Ppre,
        Saisel, Spi45sel, Sw, Usart16910sel, Usart234578sel, Usbsel,
    },
};

use crate::{chip::CLOCK_LIMITS, resource::Refused, time::Hertz};

/// The HSI frequency, undivided.
pub const HSI: Hertz = Hertz(64_000_000);

/// Spins allowed for each wait below. One spin takes at least one cycle,
/// and these run from HSI (64 MHz) until the switch, so each count is a
/// floor on the time allowed: at least 100 ms for HSE and the supply, 6 ms
/// for the PLL.
/// Spins allowed for HSI48 and the USB supply to report ready.
const HSI48_SPINS: u32 = 1_000_000;
const SUPPLY_SPINS: u32 = 6_400_000;
const HSE_SPINS: u32 = 6_400_000;
const PLL_SPINS: u32 = 400_000;
const SWITCH_SPINS: u32 = 40_000;

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
    ///
    /// The supply configuration (PWR_CR3) can be written once per power-on
    /// reset. This writes the LDO configuration the Nucleo boards use; a
    /// board that has already locked another one will time out here.
    pub fn freeze(self, config: &Config) -> Result<Rcc, Refused<ClockError, RCC>> {
        let clocks = match config.validate() {
            Ok(clocks) => clocks,
            Err(error) => return Err(Refused::new(error, self)),
        };
        apply(config, &clocks).map_err(|error| Refused::new(error, self))?;
        Ok(Rcc { clocks })
    }
}

/// The owner of RCC's shared enable and reset registers, and of the frozen
/// clock frequencies. Drivers take `&mut Rcc` at construction, so these
/// read-modify-writes never race.
pub struct Rcc {
    clocks: Clocks,
}

/// RCC_RSR's reset flags. A single read: no `Rcc` needed.
pub(crate) fn read_reset_cause() -> crate::reset::ResetCause {
    let rsr = RCC_REGS.rsr().read();
    crate::reset::ResetCause {
        power_on: rsr.porrstf(),
        pin: rsr.pinrstf(),
        brownout: rsr.borrstf(),
        software: rsr.sftrstf(),
        independent_watchdog: rsr.iwdg1rstf(),
        window_watchdog: rsr.wwdg1rstf(),
        low_power: rsr.lpwrrstf(),
    }
}

/// Clears RCC_RSR's reset flags (RMVF).
pub(crate) fn clear_reset_flags() {
    RCC_REGS.rsr().modify(|w| w.set_rmvf(true));
}

impl Rcc {
    /// Selects per_ck for the ADCs' kernel clock (ADCSEL), which reset leaves
    /// on HSI, 64 MHz. Whether HSI is running: `freeze` never stops it.
    pub(crate) fn adc_clock(&mut self) -> bool {
        RCC_REGS
            .d3ccipr()
            .modify(|w| w.set_adcsel(stm32_metapac::rcc::vals::Adcsel::PER));
        RCC_REGS.cr().read().hsirdy()
    }

    /// Starts the USB port's clock and supply: HSI48 as its kernel clock,
    /// and the 3.3 V USB level detector (PWR_CR3's USB33DEN), which `freeze`
    /// owns with the rest of PWR. HSI48 runs untrimmed: the CRS's sync to
    /// USB start-of-frame doesn't work on revision Y (ES0392 2.2.3).
    pub fn usb_clock(&mut self) -> Result<(), ClockError> {
        RCC_REGS.cr().modify(|w| w.set_hsi48on(true));
        if !wait(HSI48_SPINS, || RCC_REGS.cr().read().hsi48rdy()) {
            return Err(ClockError::UsbTimeout);
        }
        RCC_REGS.d2ccip2r().modify(|w| w.set_usbsel(Usbsel::HSI48));
        PWR.cr3().modify(|w| w.set_usb33den(true));
        if !wait(HSI48_SPINS, || PWR.cr3().read().usb33rdy()) {
            return Err(ClockError::UsbTimeout);
        }
        Ok(())
    }

    /// Why the chip last reset, read from RCC_RSR, whose flags are then
    /// cleared (RMVF) so the next reset reports only itself.
    pub fn reset_cause(&mut self) -> crate::reset::ResetCause {
        let cause = read_reset_cause();
        clear_reset_flags();
        cause
    }
}

macro_rules! bus_registers {
    ($($bus:ident => $enr:ident, $rstr:ident;)+) => {
        impl Rcc {
            /// Turns a peripheral's clock on, then reads the register back so
            /// the write has landed before the peripheral is touched.
            pub(crate) fn enable(&mut self, bit: RccBit) {
                let mask = 1 << bit.bit;
                match bit.bus {
                    $(Bus::$bus => {
                        RCC_REGS.$enr().modify(|w| w.0 |= mask);
                        let _ = RCC_REGS.$enr().read();
                    })+
                }
            }

            /// Pulses a peripheral's reset, returning its registers to reset
            /// values.
            pub(crate) fn reset(&mut self, bit: RccBit) {
                let mask = 1 << bit.bit;
                match bit.bus {
                    $(Bus::$bus => {
                        RCC_REGS.$rstr().modify(|w| w.0 |= mask);
                        RCC_REGS.$rstr().modify(|w| w.0 &= !mask);
                    })+
                }
            }
        }
    };
}

bus_registers!(
    Ahb1 => ahb1enr, ahb1rstr;
    Ahb2 => ahb2enr, ahb2rstr;
    Ahb3 => ahb3enr, ahb3rstr;
    Ahb4 => ahb4enr, ahb4rstr;
    Apb1L => apb1lenr, apb1lrstr;
    Apb1H => apb1henr, apb1hrstr;
    Apb2 => apb2enr, apb2rstr;
    Apb3 => apb3enr, apb3rstr;
    Apb4 => apb4enr, apb4rstr;
);

impl Rcc {
    /// The frequencies `freeze` set.
    pub fn clocks(&self) -> Clocks {
        self.clocks
    }
}

/// The bus a peripheral sits on: which enable and reset registers hold its
/// bits, and which clock it runs from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Bus {
    Ahb1,
    Ahb2,
    Ahb3,
    Ahb4,
    Apb1L,
    Apb1H,
    Apb2,
    Apb3,
    Apb4,
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
    /// The core clock: SYSCLK / D1CPRE.
    pub cpu: Hertz,
    /// The AXI and AHB clock: the core clock / HPRE.
    pub hclk: Hertz,
    /// APB1 (D2), APB2 (D2), APB3 (D1), APB4 (D3).
    pub pclk1: Hertz,
    pub pclk2: Hertz,
    pub pclk3: Hertz,
    pub pclk4: Hertz,
    /// Timers on APB1 and APB2 (TIMPRE at its reset value).
    pub timclk1: Hertz,
    pub timclk2: Hertz,
    pub pll1_q: Option<Hertz>,
    pub(crate) setup: Setup,
}

/// Register values `validate` works out and `freeze` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct Setup {
    flash_latency: u8,
    flash_wrhighfreq: u8,
    /// PLLRGE: 0 for 1-2 MHz, 1 for 2-4, 2 for 4-8, 3 for 8-16.
    pll_range: u8,
    pll_wide_vco: bool,
}

impl Clocks {
    /// The clock of peripherals on `bus`.
    pub const fn bus(&self, bus: Bus) -> Hertz {
        match bus {
            Bus::Ahb1 | Bus::Ahb2 | Bus::Ahb3 | Bus::Ahb4 => self.hclk,
            Bus::Apb1L | Bus::Apb1H => self.pclk1,
            Bus::Apb2 => self.pclk2,
            Bus::Apb3 => self.pclk3,
            Bus::Apb4 => self.pclk4,
        }
    }

    /// The counter clock of timers on `bus`: twice the bus clock when its
    /// prescaler divides (TIMPRE at reset), as `timclk1` and `timclk2`
    /// record. Only APB1 and APB2 carry timers.
    pub const fn timer(&self, bus: Bus) -> Hertz {
        match bus {
            Bus::Apb1L | Bus::Apb1H => self.timclk1,
            Bus::Apb2 => self.timclk2,
            _ => self.bus(bus),
        }
    }
}

/// A complete clock tree. Nothing is chosen for the user: every divider is
/// stated, and [`validate`](Self::validate) only checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    pub hse: Option<Hse>,
    pub pll1: Option<Pll>,
    pub sys: Sysclk,
    /// D1CPRE: SYSCLK to the core.
    pub cpu_pre: AhbPrescaler,
    /// HPRE: the core clock to AXI and AHB.
    pub ahb_pre: AhbPrescaler,
    pub apb1_pre: ApbPrescaler,
    pub apb2_pre: ApbPrescaler,
    pub apb3_pre: ApbPrescaler,
    pub apb4_pre: ApbPrescaler,
    pub supply: Supply,
    pub voltage_scale: VoltageScale,
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
    Oscillator,
    /// An external clock on OSC_IN, such as an ST-LINK's MCO.
    Bypass,
}

/// PLL1: `source / prediv * mul`, then `/ divp` to SYSCLK, `/ divq` and
/// `/ divr` to kernel clocks. The fractional divider stays off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Pll {
    pub source: PllSource,
    /// DIVM1, 1 to 63.
    pub prediv: u8,
    /// DIVN1, 4 to 512.
    pub mul: u16,
    /// DIVP1, an even number from 2 to 128. `None` leaves P off.
    pub divp: Option<u8>,
    /// DIVQ1, 1 to 128.
    pub divq: Option<u8>,
    /// DIVR1, 1 to 128.
    pub divr: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PllSource {
    Hsi,
    Hse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Sysclk {
    Hsi,
    Hse,
    Pll1P,
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

/// Where VCORE comes from. Written once per power-on reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Supply {
    /// The internal LDO, as on the Nucleo-144 boards.
    Ldo,
}

/// VCORE's range, which sets every clock's ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum VoltageScale {
    /// 1.15 V to 1.26 V: core 400 MHz, AXI/AHB 200 MHz, APB 100 MHz.
    Scale1,
    /// 1.05 V to 1.15 V: 300, 150, 75 MHz.
    Scale2,
    /// 0.95 V to 1.05 V, the reset scale: 200, 100, 50 MHz.
    Scale3,
}

impl VoltageScale {
    const fn bits(self) -> Vos {
        match self {
            Self::Scale1 => Vos::SCALE1,
            Self::Scale2 => Vos::SCALE2,
            Self::Scale3 => Vos::SCALE3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ClockError {
    /// HSI48 or the USB supply didn't report ready within its bound.
    UsbTimeout,
    HseFrequency,
    HseNotConfigured,
    PllNotConfigured,
    /// DIVM, DIVN or an output divider is outside its range, or DIVP is odd.
    PllDivider,
    /// The PLL input after DIVM is outside 1 to 16 MHz.
    PllInput,
    /// The VCO frequency fits neither VCO range for this input.
    PllVco,
    /// SYSCLK or the core clock is above the voltage scale's limit.
    Cpu,
    Hclk,
    Pclk,
    /// PWR did not report the supply or voltage scale ready.
    SupplyTimeout,
    /// HSE did not report ready within its bound. The system is on HSI.
    HseTimeout,
    /// The PLL did not lock within its bound. The system is on HSI.
    PllTimeout,
    /// A clock switch or the flash latency did not take within its bound.
    SwitchTimeout,
    /// No combination of the dividers meets the request.
    Unsatisfiable,
}

/// A chip's clock limits. Supplied by the glue layer.
pub(crate) struct Limits {
    /// (core, AXI/AHB, APB) maxima for scales 1, 2 and 3.
    pub scale1: (Hertz, Hertz, Hertz),
    pub scale2: (Hertz, Hertz, Hertz),
    pub scale3: (Hertz, Hertz, Hertz),
    pub hse_oscillator: (Hertz, Hertz),
    pub hse_bypass: (Hertz, Hertz),
    pub pll_input: (Hertz, Hertz),
    /// Allowed for inputs of 1 MHz up.
    pub vco_medium: (Hertz, Hertz),
    /// Allowed for inputs of 2 MHz up.
    pub vco_wide: (Hertz, Hertz),
}

const fn in_range(freq: u32, range: (Hertz, Hertz)) -> bool {
    freq >= range.0.0 && freq <= range.1.0
}

/// Flash wait states and programming delay for an AXI clock, RM0433
/// table 17. `None` above the scale's table.
const fn flash_setup(hclk: u32, scale: VoltageScale) -> Option<(u8, u8)> {
    const MHZ: u32 = 1_000_000;
    let rows: &[(u32, u8, u8)] = match scale {
        VoltageScale::Scale1 => &[
            (70 * MHZ, 0, 0),
            (140 * MHZ, 1, 1),
            (185 * MHZ, 2, 1),
            (210 * MHZ, 2, 2),
            (225 * MHZ, 3, 2),
        ],
        VoltageScale::Scale2 => &[
            (55 * MHZ, 0, 0),
            (110 * MHZ, 1, 1),
            (165 * MHZ, 2, 1),
            (225 * MHZ, 3, 2),
        ],
        VoltageScale::Scale3 => &[
            (45 * MHZ, 0, 0),
            (90 * MHZ, 1, 1),
            (135 * MHZ, 2, 1),
            (180 * MHZ, 3, 2),
            (225 * MHZ, 4, 2),
        ],
    };
    let mut i = 0;
    while i < rows.len() {
        if hclk <= rows[i].0 {
            return Some((rows[i].1, rows[i].2));
        }
        i += 1;
    }
    None
}

/// A timer behind a divided APB runs at twice that bus (TIMPRE clear).
const fn timer_clock(pclk: Hertz, pre: ApbPrescaler) -> Hertz {
    match pre {
        ApbPrescaler::Div1 => pclk,
        _ => Hertz(pclk.0 * 2),
    }
}

const fn divider_ok(div: Option<u8>, min: u8, max: u8) -> bool {
    match div {
        Some(div) => div >= min && div <= max,
        None => true,
    }
}

impl Config {
    /// Checks every frequency against the chip's limits and returns them.
    pub const fn validate(&self) -> Result<Clocks, ClockError> {
        let limits = &CLOCK_LIMITS;
        let (cpu_max, hclk_max, pclk_max) = match self.voltage_scale {
            VoltageScale::Scale1 => limits.scale1,
            VoltageScale::Scale2 => limits.scale2,
            VoltageScale::Scale3 => limits.scale3,
        };

        if let Some(hse) = self.hse {
            let range = match hse.mode {
                HseMode::Oscillator => limits.hse_oscillator,
                HseMode::Bypass => limits.hse_bypass,
            };
            if !in_range(hse.freq.0, range) {
                return Err(ClockError::HseFrequency);
            }
        }

        let mut pll_p = None;
        let mut pll_q = None;
        let mut pll_range = 0;
        let mut pll_wide_vco = false;
        if let Some(pll) = self.pll1 {
            let input = match pll.source {
                PllSource::Hsi => HSI,
                PllSource::Hse => match self.hse {
                    Some(hse) => hse.freq,
                    None => return Err(ClockError::HseNotConfigured),
                },
            };
            if pll.prediv < 1 || pll.prediv > 63 || pll.mul < 4 || pll.mul > 512 {
                return Err(ClockError::PllDivider);
            }
            if !divider_ok(pll.divp, 2, 128)
                || !divider_ok(pll.divq, 1, 128)
                || !divider_ok(pll.divr, 1, 128)
            {
                return Err(ClockError::PllDivider);
            }
            if let Some(divp) = pll.divp {
                // PLL1's P divider takes even values only.
                if divp % 2 != 0 {
                    return Err(ClockError::PllDivider);
                }
            }
            let reference = input.0 / pll.prediv as u32;
            if !in_range(reference, limits.pll_input) {
                return Err(ClockError::PllInput);
            }
            pll_range = if reference < 2_000_000 {
                0
            } else if reference < 4_000_000 {
                1
            } else if reference < 8_000_000 {
                2
            } else {
                3
            };
            // At most 16 MHz * 512, so this fits in a u32.
            let vco = reference * pll.mul as u32;
            if in_range(vco, limits.vco_medium) {
                pll_wide_vco = false;
            } else if reference >= 2_000_000 && in_range(vco, limits.vco_wide) {
                pll_wide_vco = true;
            } else {
                return Err(ClockError::PllVco);
            }
            // Checked again here so the division has no panic path; the
            // ranges above already rule zero out.
            if let Some(divp) = pll.divp {
                match vco.checked_div(divp as u32) {
                    Some(freq) => pll_p = Some(Hertz(freq)),
                    None => return Err(ClockError::PllDivider),
                }
            }
            if let Some(divq) = pll.divq {
                match vco.checked_div(divq as u32) {
                    Some(freq) => pll_q = Some(Hertz(freq)),
                    None => return Err(ClockError::PllDivider),
                }
            }
        }

        let sysclk = match self.sys {
            Sysclk::Hsi => HSI,
            Sysclk::Hse => match self.hse {
                Some(hse) => hse.freq,
                None => return Err(ClockError::HseNotConfigured),
            },
            Sysclk::Pll1P => match pll_p {
                Some(freq) => freq,
                None => return Err(ClockError::PllNotConfigured),
            },
        };
        if sysclk.0 > cpu_max.0 {
            return Err(ClockError::Cpu);
        }
        let cpu = Hertz(sysclk.0 / self.cpu_pre.divisor());
        let hclk = Hertz(cpu.0 / self.ahb_pre.divisor());
        if hclk.0 > hclk_max.0 {
            return Err(ClockError::Hclk);
        }
        let pclk1 = Hertz(hclk.0 / self.apb1_pre.divisor());
        let pclk2 = Hertz(hclk.0 / self.apb2_pre.divisor());
        let pclk3 = Hertz(hclk.0 / self.apb3_pre.divisor());
        let pclk4 = Hertz(hclk.0 / self.apb4_pre.divisor());
        if pclk1.0 > pclk_max.0
            || pclk2.0 > pclk_max.0
            || pclk3.0 > pclk_max.0
            || pclk4.0 > pclk_max.0
        {
            return Err(ClockError::Pclk);
        }
        let Some((flash_latency, flash_wrhighfreq)) = flash_setup(hclk.0, self.voltage_scale)
        else {
            return Err(ClockError::Hclk);
        };

        Ok(Clocks {
            sysclk,
            cpu,
            hclk,
            pclk1,
            pclk2,
            pclk3,
            pclk4,
            timclk1: timer_clock(pclk1, self.apb1_pre),
            timclk2: timer_clock(pclk2, self.apb2_pre),
            pll1_q: pll_q,
            setup: Setup {
                flash_latency,
                flash_wrhighfreq,
                pll_range,
                pll_wide_vco,
            },
        })
    }
}

/// What a board asks the clocks for: its oscillator and the system
/// frequency. The USB port runs from HSI48 on these chips, so it makes no
/// request; a board that routes PLL1's Q output to peripheral kernel clocks
/// (the Lucid's 100 MHz for SPI1 to SPI3) asks for it in `q`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Request {
    /// The board's oscillator.
    pub hse: Hse,
    /// The system frequency.
    pub sysclk: Hertz,
    /// What PLL1's Q output must provide, when the board routes it.
    pub q: Option<Hertz>,
}

/// What the VCO search settles on: the DIVM1, the multiplier, the P and Q
/// dividers, and the prescalers from SYSCLK down.
type SolvedTree = (
    u32,
    u32,
    Option<u8>,
    Option<u8>,
    AhbPrescaler,
    AhbPrescaler,
    ApbPrescaler,
    ApbPrescaler,
    ApbPrescaler,
    ApbPrescaler,
);

/// The smallest AHB divisor that keeps `clock / divisor` within `max`.
const fn ahb_pre(clock: u32, max: u32) -> Option<AhbPrescaler> {
    let mut divisor = 1;
    while divisor <= 512 {
        if clock / divisor <= max {
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

/// The smallest APB divisor that keeps `clock / divisor` within `max`.
const fn apb_pre(clock: u32, max: u32) -> Option<ApbPrescaler> {
    let mut divisor = 1;
    while divisor <= 16 {
        if clock / divisor <= max {
            return Some(match divisor {
                1 => ApbPrescaler::Div1,
                2 => ApbPrescaler::Div2,
                4 => ApbPrescaler::Div4,
                8 => ApbPrescaler::Div8,
                _ => ApbPrescaler::Div16,
            });
        }
        divisor *= 2;
    }
    None
}

impl Config {
    /// Computes a configuration for `request`: PLL1 from the HSE, with its
    /// input the oscillator divided as little as the chip's input range
    /// needs, and the VCO the smallest that the system clock (through P,
    /// even) and the Q request divide down from, in whichever VCO range
    /// fits. The prescalers are the smallest divisors that keep the core,
    /// AXI and every APB within the voltage scale's limits, and the voltage
    /// scale the lowest that supports the request, so a request a lower
    /// voltage meets runs at it. The result is validated before it is
    /// returned: what `solve` returns always freezes.
    ///
    /// USB needs no request: it runs from HSI48. This is a convenience over
    /// [`Config`]'s stated dividers, not a replacement: a board that wants a
    /// particular tree states it, and [`validate`](Self::validate) checks it
    /// as ever.
    pub const fn solve(request: Request) -> Result<Self, ClockError> {
        if request.sysclk.0 == 0 {
            return Err(ClockError::Cpu);
        }
        let limits = &CLOCK_LIMITS;
        if request.sysclk.0 > limits.scale1.0.0 {
            return Err(ClockError::Cpu);
        }
        // The scales, lowest voltage first: the first that supports the
        // request wins.
        let scales = [
            VoltageScale::Scale3,
            VoltageScale::Scale2,
            VoltageScale::Scale1,
        ];
        let mut index = 0;
        let mut last = ClockError::Unsatisfiable;
        while index < 3 {
            let voltage_scale = scales[index];
            let (cpu_max, hclk_max, pclk_max) = match voltage_scale {
                VoltageScale::Scale1 => limits.scale1,
                VoltageScale::Scale2 => limits.scale2,
                VoltageScale::Scale3 => limits.scale3,
            };
            if request.sysclk.0 > cpu_max.0 {
                index += 1;
                continue;
            }
            match Self::solve_tree(request, limits, voltage_scale, hclk_max.0, pclk_max.0) {
                Ok(config) => {
                    // What `solve` returns always freezes.
                    return match config.validate() {
                        Ok(_) => Ok(config),
                        Err(error) => Err(error),
                    };
                }
                Err(error) => {
                    last = error;
                    index += 1;
                }
            }
        }
        Err(last)
    }

    /// The tree within one voltage scale: the PLL input the oscillator
    /// divided as little as the chip's input range needs, and the VCO the
    /// smallest that the system clock (through P, even) and the Q request
    /// divide down from.
    const fn solve_tree(
        request: Request,
        limits: &Limits,
        voltage_scale: VoltageScale,
        hclk_max: u32,
        pclk_max: u32,
    ) -> Result<Self, ClockError> {
        let hse_freq = request.hse.freq.0;
        // The PLL input: the HSE divided as little as the 1 to 16 MHz input
        // range needs, tried divided least first.
        let mut prediv = 1;
        while prediv <= 63 {
            let input = hse_freq / prediv;
            if input > limits.pll_input.1.0 {
                prediv += 1;
                continue;
            }
            if input < limits.pll_input.0.0 {
                // A larger PLLM only lowers the input further.
                break;
            }
            if let Some((prediv, mul, divp, divq, cpu_pre, ahb_pre, apb1, apb2, apb3, apb4)) =
                Self::solve_vco(request, limits, input, request.sysclk.0, hclk_max, pclk_max)
            {
                return Ok(Self {
                    hse: Some(request.hse),
                    pll1: Some(Pll {
                        source: PllSource::Hse,
                        prediv: prediv as u8,
                        mul: mul as u16,
                        divp,
                        divq,
                        divr: None,
                    }),
                    sys: Sysclk::Pll1P,
                    cpu_pre,
                    ahb_pre,
                    apb1_pre: apb1,
                    apb2_pre: apb2,
                    apb3_pre: apb3,
                    apb4_pre: apb4,
                    supply: Supply::Ldo,
                    voltage_scale,
                });
            }
            prediv += 1;
        }
        Err(ClockError::Unsatisfiable)
    }
}

impl Config {
    /// The smallest VCO on the input `reference` that the system clock
    /// (through P, even) and the Q request divide down from, in whichever
    /// VCO range fits, with the prescalers alongside. Returns the DIVM1 it
    /// used, so the caller can go on to the next input when this one has no
    /// solution.
    const fn solve_vco(
        request: Request,
        limits: &Limits,
        reference: u32,
        sysclk: u32,
        hclk_max: u32,
        pclk_max: u32,
    ) -> Option<SolvedTree> {
        let mut mul = 4;
        while mul <= 512 {
            let vco = reference * mul;
            // The medium VCO fits any input; the wide one needs a reference
            // of 2 MHz or more, and tops out where both revisions meet.
            let in_medium = in_range(vco, limits.vco_medium);
            let in_wide = reference >= 2_000_000 && in_range(vco, limits.vco_wide);
            if !in_medium && !in_wide {
                if vco > limits.vco_wide.1.0 {
                    break;
                }
                mul += 1;
                continue;
            }
            // P: even, 2 to 128, giving the system clock exactly.
            if !vco.is_multiple_of(sysclk) {
                mul += 1;
                continue;
            }
            let p = vco / sysclk;
            if p < 2 || p > 128 || !p.is_multiple_of(2) {
                mul += 1;
                continue;
            }
            // Q: the request, exactly.
            let divq = match request.q {
                None => None,
                Some(q) => {
                    let mut divq = 1;
                    let mut found = None;
                    while divq <= 128 {
                        if vco.is_multiple_of(divq) && vco / divq == q.0 {
                            found = Some(divq as u8);
                        }
                        divq += 1;
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
            // The prescalers: the smallest divisors inside the scale's
            // limits, the core first.
            let cpu_pre = match ahb_pre(sysclk, sysclk) {
                Some(cpu_pre) => cpu_pre,
                None => return None,
            };
            let core = sysclk / cpu_pre.divisor();
            let ahb_pre = match ahb_pre(core, hclk_max) {
                Some(ahb_pre) => ahb_pre,
                None => return None,
            };
            let hclk = core / ahb_pre.divisor();
            let apb1 = match apb_pre(hclk, pclk_max) {
                Some(apb1) => apb1,
                None => return None,
            };
            let apb2 = match apb_pre(hclk, pclk_max) {
                Some(apb2) => apb2,
                None => return None,
            };
            let apb3 = match apb_pre(hclk, pclk_max) {
                Some(apb3) => apb3,
                None => return None,
            };
            let apb4 = match apb_pre(hclk, pclk_max) {
                Some(apb4) => apb4,
                None => return None,
            };
            return Some((
                prediv_value(reference, request),
                mul,
                Some(p as u8),
                divq,
                cpu_pre,
                ahb_pre,
                apb1,
                apb2,
                apb3,
                apb4,
            ));
        }
        None
    }
}

/// The DIVM1 that produced `reference` from the request's oscillator.
const fn prediv_value(reference: u32, request: Request) -> u32 {
    request.hse.freq.0 / reference
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

fn set_flash(latency: u8, wrhighfreq: u8) -> Result<(), ClockError> {
    FLASH.acr().modify(|w| {
        w.set_latency(latency);
        w.set_wrhighfreq(wrhighfreq);
    });
    if wait(SWITCH_SPINS, || FLASH.acr().read().latency() == latency) {
        Ok(())
    } else {
        Err(ClockError::SwitchTimeout)
    }
}

/// RM0433's power-up order: supply, voltage scale, oscillators, PLL, flash
/// wait states, prescalers, switch.
fn apply(config: &Config, clocks: &Clocks) -> Result<(), ClockError> {
    // The supply. Only the first write after power-on takes; a later one
    // with the same value is harmless.
    match config.supply {
        Supply::Ldo => PWR.cr3().modify(|w| {
            w.set_scuen(true);
            w.set_ldoen(true);
            w.set_bypass(false);
        }),
    }
    if !wait(SUPPLY_SPINS, || PWR.csr1().read().actvosrdy()) {
        return Err(ClockError::SupplyTimeout);
    }
    PWR.d3cr()
        .modify(|w| w.set_vos(config.voltage_scale.bits()));
    if !wait(SUPPLY_SPINS, || PWR.d3cr().read().vosrdy()) {
        return Err(ClockError::SupplyTimeout);
    }

    // Run from HSI, undivided, while everything else is reconfigured.
    RCC_REGS.cr().modify(|w| {
        w.set_hsidiv(Hsidiv::DIV1);
        w.set_hsion(true);
    });
    if !wait(SWITCH_SPINS, || RCC_REGS.cr().read().hsirdy()) {
        return Err(ClockError::SwitchTimeout);
    }
    RCC_REGS.cfgr().modify(|w| w.set_sw(Sw::HSI));
    if !wait(SWITCH_SPINS, || RCC_REGS.cfgr().read().sws() == Sw::HSI) {
        return Err(ClockError::SwitchTimeout);
    }

    match config.hse {
        Some(hse) => {
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
        None => RCC_REGS.cr().modify(|w| w.set_hseon(false)),
    }

    RCC_REGS.cr().modify(|w| w.set_pllon(0, false));
    if !wait(PLL_SPINS, || !RCC_REGS.cr().read().pllrdy(0)) {
        return Err(ClockError::PllTimeout);
    }
    if let Some(pll) = config.pll1 {
        RCC_REGS.pllckselr().modify(|w| {
            w.set_pllsrc(match pll.source {
                PllSource::Hsi => Pllsrc::HSI,
                PllSource::Hse => Pllsrc::HSE,
            });
            w.set_divm(0, Pllm::from_bits(pll.prediv));
        });
        RCC_REGS.pllcfgr().modify(|w| {
            w.set_pllfracen(0, false);
            w.set_pllvcosel(
                0,
                if clocks.setup.pll_wide_vco {
                    Pllvcosel::WIDE_VCO
                } else {
                    Pllvcosel::MEDIUM_VCO
                },
            );
            w.set_pllrge(0, Pllrge::from_bits(clocks.setup.pll_range));
            w.set_divpen(0, pll.divp.is_some());
            w.set_divqen(0, pll.divq.is_some());
            w.set_divren(0, pll.divr.is_some());
        });
        // Each field holds its divider less one.
        RCC_REGS.plldivr(0).write(|w| {
            w.set_plln(Plln::from_bits(pll.mul - 1));
            w.set_pllp(Plldiv::from_bits(pll.divp.unwrap_or(2) - 1));
            w.set_pllq(Plldiv::from_bits(pll.divq.unwrap_or(2) - 1));
            w.set_pllr(Plldiv::from_bits(pll.divr.unwrap_or(2) - 1));
        });
        RCC_REGS.cr().modify(|w| w.set_pllon(0, true));
        if !wait(PLL_SPINS, || RCC_REGS.cr().read().pllrdy(0)) {
            return Err(ClockError::PllTimeout);
        }
    }

    // Wait states for whichever is slower to read: the target, or HSI's
    // 64 MHz at the new scale, which is what runs until the switch.
    let (on_hsi_latency, on_hsi_wrhighfreq) =
        flash_setup(HSI.0, config.voltage_scale).unwrap_or((7, 3));
    set_flash(
        clocks.setup.flash_latency.max(on_hsi_latency),
        clocks.setup.flash_wrhighfreq.max(on_hsi_wrhighfreq),
    )?;

    RCC_REGS.d1cfgr().modify(|w| {
        w.set_d1cpre(config.cpu_pre.bits());
        w.set_hpre(config.ahb_pre.bits());
        w.set_d1ppre(config.apb3_pre.bits());
    });
    RCC_REGS.d2cfgr().modify(|w| {
        w.set_d2ppre1(config.apb1_pre.bits());
        w.set_d2ppre2(config.apb2_pre.bits());
    });
    RCC_REGS
        .d3cfgr()
        .modify(|w| w.set_d3ppre(config.apb4_pre.bits()));

    let sw = match config.sys {
        Sysclk::Hsi => Sw::HSI,
        Sysclk::Hse => Sw::HSE,
        Sysclk::Pll1P => Sw::PLL1_P,
    };
    RCC_REGS.cfgr().modify(|w| w.set_sw(sw));
    if !wait(SWITCH_SPINS, || RCC_REGS.cfgr().read().sws() == sw) {
        return Err(ClockError::SwitchTimeout);
    }
    set_flash(clocks.setup.flash_latency, clocks.setup.flash_wrhighfreq)?;

    // U(S)ART kernel clocks from their bus clocks, which is what
    // `Clocks::bus` reports for them.
    RCC_REGS.d2ccip2r().modify(|w| {
        w.set_usart16910sel(Usart16910sel::PCLK2);
        w.set_usart234578sel(Usart234578sel::PCLK1);
    });
    // SPI kernel clocks at their reset choices, written so a bootloader's
    // choice can't linger: SPI1 to SPI3 from PLL1Q, SPI4 and SPI5 from
    // PCLK2. The chip glue's `Kernel` for each instance says the same.
    RCC_REGS.d2ccip1r().modify(|w| {
        w.set_spi123sel(Saisel::PLL1_Q);
        w.set_spi45sel(Spi45sel::PCLK2);
    });
    // I2C kernel clocks from their bus clocks, written the same way:
    // I2C1 to I2C3 from PCLK1, I2C4 from PCLK4. The chip glue's `Kernel`
    // for each instance says the same.
    RCC_REGS.d2ccip2r().modify(|w| {
        w.set_i2c1235sel(I2c1235sel::PCLK1);
    });
    RCC_REGS.d3ccipr().modify(|w| {
        w.set_i2c4sel(I2c4sel::PCLK4);
    });
    Ok(())
}
