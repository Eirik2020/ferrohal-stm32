//! RCC on the F7 clock tree (`rcc_f7`, RM0431 and RM0385 chapter 5): the
//! explicit clock configuration, and the single owner of every shared RCC
//! register.
//!
//! The tree is the F4's (`rcc_f4`), and so are the configuration, its
//! validation and the solver, which this module mounts from the shared
//! source: the same [`Config`], [`Clocks`] and [`Request`]. What differs is
//! written here.
//!
//! - **Over-drive.** HCLK above 180 MHz, up to 216, needs the regulator's
//!   over-drive, which also raises APB1 to 54 MHz and APB2 to 108 (DS11853
//!   Table 16, DS10916 Table 18). `validate` decides it from HCLK
//!   (`Limits::overdrive`), and `freeze` enters it with RM0431 4.1.4's
//!   sequence: the PLL started on HSI or HSE, ODEN then ODRDY, ODSWEN then
//!   ODSWRDY, and only then the switch. A tree that does not need it leaves
//!   it with sequence 1: ODEN and ODSWEN cleared together, still on HSI.
//!   RM0431 asks that no peripheral clock be on during the switch:
//!   `freeze` comes before every driver, and turns on only PWR's.
//! - **Voltage scale.** Scale 1 (VOS 11), the scale 216 MHz needs, written
//!   with the PLL off as RM0431 4.1.4 requires, as on the F4. It is the
//!   reset value; it is written anyway, since a bootloader may have
//!   lowered it.
//! - **Flash.** Up to seven wait states, one per 30 MHz at 2.7 V to 3.6 V
//!   (RM0431 and RM0385 Table 5), and the ART accelerator in place of the
//!   F4's instruction and data caches.
//! - **Kernel clocks.** DCKCFGR2 is written to its reset value: the
//!   U(S)ARTs, I2Cs and LPTIM1 on their APB clock, and the 48 MHz domain
//!   (USB, SDMMC, RNG) on PLLQ. [`Kernel::Bus`] relies on it. DCKCFGR1's
//!   TIMPRE is cleared, so a timer behind a divided APB runs at twice its
//!   bus, as `Clocks::timclk1` records.
//!
//! Requirements: RCC-001, RCC-002, RCC-003, RCC-004, RESET-001

use crate::wait::spin;
use stm32_metapac::{
    FLASH, PWR, RCC as RCC_REGS,
    flash::vals::Latency,
    rcc::vals::{Pllm, Plln, Pllq, Pllsrc, Sw, Timpre},
};

pub use crate::rcc_kernel::Kernel;
pub use crate::rcc_shared::{
    AhbPrescaler, ApbPrescaler, ClockError, Hse, HseMode, PllSource, RccBit,
};
use crate::{chip::CLOCK_LIMITS, resource::Refused, time::Hertz};

// The F4's clock tree: its configuration, validation and solver.
#[path = "../rcc_f4/tree.rs"]
mod tree;
pub(crate) use tree::Limits;
pub(crate) use tree::Overdrive;
pub use tree::{Bus, Clocks, Config, HSI, Pll, PllP, Request, Supply, Sysclk};

/// Spins allowed for each wait below. One spin takes at least one cycle,
/// and these run at no more than 16 MHz (HSI) until the switch, so each
/// count is a floor on the time allowed: HSE gets at least 100 ms to start,
/// the PLL at least 6 ms to lock, the regulator at least 6 ms to reach its
/// scale or switch to and from the over-drive.
const HSE_SPINS: u32 = 1_600_000;
const PLL_SPINS: u32 = 100_000;
const VOS_SPINS: u32 = 100_000;
const OVERDRIVE_SPINS: u32 = 100_000;
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
    /// Validation runs first, so a refused configuration writes nothing, and
    /// the RCC token comes back in the refusal. Every wait on the hardware is
    /// counted; a source that does not start is reported, with the system
    /// still running from HSI and whatever the attempt wrote in place.
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
/// read-modify-writes never race: firmware does them in `init`, and a layer
/// that builds drivers at runtime wraps this owner in its own lock.
pub struct Rcc {
    clocks: Clocks,
}

/// RCC_CSR's reset flags. A single read: no `Rcc` needed.
pub(crate) fn read_reset_cause() -> crate::reset::Cause {
    let csr = RCC_REGS.csr().read();
    crate::reset::Cause {
        power_on: csr.porrstf(),
        pin: csr.padrstf(),
        brownout: csr.borrstf(),
        software: csr.sftrstf(),
        independent_watchdog: csr.wdgrstf(),
        window_watchdog: csr.wwdgrstf(),
        low_power: csr.lpwrrstf(),
    }
}

/// Clears RCC_CSR's reset flags (RMVF). Read-modify-write of a register only
/// the reset flags and LSI use, before the watchdog could want the LSI.
pub(crate) fn clear_reset_flags() {
    RCC_REGS.csr().modify(|w| w.set_rmvf(true));
}

impl Rcc {
    /// The frequencies `freeze` set.
    pub fn clocks(&self) -> Clocks {
        self.clocks
    }

    /// Checks the USB port's 48 MHz clock: PLLQ, which `Config` set and
    /// `freeze` selects (CK48MSEL). PLLSAI's P output, the other source, is
    /// not offered.
    pub fn usb_clock(&mut self) -> Result<(), ClockError> {
        match self.clocks.pll_q {
            Some(Hertz(48_000_000)) => Ok(()),
            _ => Err(ClockError::Usb),
        }
    }

    /// Why the chip last reset, read from RCC_CSR, whose flags are then
    /// cleared (RMVF) so the next reset reports only itself.
    pub fn reset_cause(&mut self) -> crate::reset::Cause {
        let cause = read_reset_cause();
        clear_reset_flags();
        cause
    }
}

bus_registers!(
    Ahb1 => ahb1enr, ahb1rstr;
    Ahb2 => ahb2enr, ahb2rstr;
    Apb1 => apb1enr, apb1rstr;
    Apb2 => apb2enr, apb2rstr;
);

/// Writes `config`, whose frequencies `clocks` holds. Every wait is counted.
fn apply(config: &Config, clocks: &Clocks) -> Result<(), ClockError> {
    // Run from HSI while everything else is reconfigured.
    RCC_REGS.cr().modify(|w| w.set_hsion(true));
    if !spin(SWITCH_SPINS, || RCC_REGS.cr().read().hsirdy()) {
        return Err(ClockError::SwitchTimeout);
    }
    RCC_REGS.cfgr().modify(|w| w.set_sw(Sw::HSI));
    if !spin(SWITCH_SPINS, || RCC_REGS.cfgr().read().sws() == Sw::HSI) {
        return Err(ClockError::SwitchTimeout);
    }

    // PWR answers only with its clock on; the read back is the delay before
    // its registers are touched, as in `Rcc::enable`.
    RCC_REGS.apb1enr().modify(|w| w.set_pwren(true));
    let _ = RCC_REGS.apb1enr().read();
    // A tree an earlier freeze left in over-drive leaves it now, on HSI
    // (RM0431 4.1.4, sequence 1); it is entered again below if needed.
    if PWR.cr1().read().oden() {
        PWR.cr1().modify(|w| {
            w.set_oden(false);
            w.set_odswen(false);
        });
        if !spin(OVERDRIVE_SPINS, || !PWR.csr1().read().odswrdy()) {
            return Err(ClockError::SupplyTimeout);
        }
    }

    if let Some(hse) = config.hse {
        // HSEBYP may only change while HSE is off.
        RCC_REGS.cr().modify(|w| w.set_hseon(false));
        if !spin(HSE_SPINS, || !RCC_REGS.cr().read().hserdy()) {
            return Err(ClockError::HseTimeout);
        }
        RCC_REGS
            .cr()
            .modify(|w| w.set_hsebyp(hse.mode == HseMode::Bypass));
        RCC_REGS.cr().modify(|w| w.set_hseon(true));
        if !spin(HSE_SPINS, || RCC_REGS.cr().read().hserdy()) {
            return Err(ClockError::HseTimeout);
        }
    }

    RCC_REGS.cr().modify(|w| w.set_pllon(false));
    if !spin(PLL_SPINS, || !RCC_REGS.cr().read().pllrdy()) {
        return Err(ClockError::PllTimeout);
    }
    if let Some(pll) = config.pll {
        // VOS may change only while the PLL is off (RM0431 4.1.4).
        PWR.cr1().modify(|w| w.set_vos(CLOCK_LIMITS.vos));
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
        // The over-drive while the PLL locks, still on HSI or HSE (RM0431
        // 4.1.4, steps 3 to 5).
        if clocks.overdrive {
            PWR.cr1().modify(|w| w.set_oden(true));
            if !spin(OVERDRIVE_SPINS, || PWR.csr1().read().odrdy()) {
                return Err(ClockError::SupplyTimeout);
            }
            PWR.cr1().modify(|w| w.set_odswen(true));
            if !spin(OVERDRIVE_SPINS, || PWR.csr1().read().odswrdy()) {
                return Err(ClockError::SupplyTimeout);
            }
        }
        if !spin(PLL_SPINS, || RCC_REGS.cr().read().pllrdy()) {
            return Err(ClockError::PllTimeout);
        }
        // The scale takes effect only once the PLL runs (RM0431 4.1.4), so
        // ready is asked only now, before anything is switched to the PLL.
        if !spin(VOS_SPINS, || PWR.csr1().read().vosrdy()) {
            return Err(ClockError::SupplyTimeout);
        }
    }

    // The kernel clocks `Kernel::Bus` and `usb_clock` assume: DCKCFGR2's
    // reset value, and TIMPRE clear.
    RCC_REGS.dckcfgr2().write(|_| {});
    RCC_REGS.dckcfgr1().modify(|w| w.set_timpre(Timpre::MUL2));

    // HSI needs no wait state, so the target's count is always enough here.
    // Set it before the clock goes up.
    let latency = Latency::from_bits(clocks.flash_wait_states);
    // The prefetch queue, unless the die's erratum says this revision lacks
    // it. The erratum is checked, never a code.
    let prefetch = !matches!(crate::chip::PREFETCH_ERRATUM, Some(e) if e.affects(crate::signature::revision()));
    FLASH.acr().modify(|w| {
        w.set_latency(latency);
        w.set_prften(prefetch);
        w.set_arten(true);
    });
    if !spin(LATENCY_SPINS, || FLASH.acr().read().latency() == latency) {
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
    if !spin(SWITCH_SPINS, || RCC_REGS.cfgr().read().sws() == sw) {
        return Err(ClockError::SwitchTimeout);
    }
    Ok(())
}
