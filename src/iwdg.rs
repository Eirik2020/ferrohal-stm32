//! The independent watchdog: `iwdg_v1` (F4) and `iwdg_v2` (H7's IWDG1,
//! which adds a window register, left at its reset value: no window). The
//! key, prescaler and reload registers are the same, so they share this
//! source.
//!
//! Once started it can't be stopped short of a reset: it counts down from
//! its reload value on the LSI clock, and resets the chip when it reaches 0
//! unless [`Watchdog::feed`] reloads it first. The LSI's frequency varies
//! from chip to chip (on the F4, 17 to 47 kHz, DS10086 and DS8626), so the
//! timeout is nominal: from the 32 kHz the datasheets give as typical.
//! Choose a timeout with that margin in mind. `rcc`'s `reset_cause` tells
//! a firmware, after the fact, that the watchdog reset it.

use stm32_metapac::iwdg::vals::{Key, Pr};

use crate::{chip::WatchdogPeripheral, time::Hertz};

/// The LSI's typical frequency, which the timeouts are worked out from.
pub const LSI: Hertz = Hertz::khz(32);

/// Register reads allowed for the prescaler and reload to reach the
/// watchdog's clock domain: about five LSI cycles, so this is generous.
const UPDATE_SPINS: u32 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// Shorter than one count at the fastest prescaler, or longer than the
    /// slowest prescaler's full reload: 125 us to about 32.8 s at 32 kHz.
    Timeout,
    /// The new prescaler and reload didn't reach the watchdog within the
    /// bound. It runs, with whatever it had.
    Update,
}

/// The prescaler code and the reload value nearest `timeout_us` at the
/// nominal LSI.
pub const fn divider(timeout_us: u32) -> Result<(u8, u16), Error> {
    // LSI counts in the timeout, before the prescaler.
    let counts = timeout_us as u64 * LSI.0 as u64 / 1_000_000;
    let mut code = 0;
    while code < 7 {
        let divide = 4u64 << code;
        let reload = (counts + divide / 2) / divide;
        if reload >= 1 && reload <= 4096 {
            return Ok((code, (reload - 1) as u16));
        }
        if reload == 0 {
            return Err(Error::Timeout);
        }
        code += 1;
    }
    Err(Error::Timeout)
}

/// The running watchdog. `feed` is a single key write, so the handle can
/// sit wherever the firmware feeds from.
pub struct Watchdog {
    timeout_us: u32,
}

impl Watchdog {
    /// Starts the watchdog with `timeout_us`, nominal. It runs from here on;
    /// feed it within the timeout.
    pub fn start(_iwdg: WatchdogPeripheral, timeout_us: u32) -> Result<Self, Error> {
        let (code, reload) = divider(timeout_us)?;
        let regs = crate::chip::iwdg_regs();
        // Starting turns the LSI on; then unlock, load, wait, reload.
        regs.kr().write(|w| w.set_key(Key::START));
        regs.kr().write(|w| w.set_key(Key::ENABLE));
        regs.pr().write(|w| w.set_pr(Pr::from_bits(code)));
        regs.rlr().write(|w| w.set_rl(reload));
        let mut settled = false;
        for _ in 0..UPDATE_SPINS {
            let sr = regs.sr().read();
            if !sr.pvu() && !sr.rvu() {
                settled = true;
                break;
            }
        }
        regs.kr().write(|w| w.set_key(Key::RESET));
        if !settled {
            return Err(Error::Update);
        }
        Ok(Self { timeout_us })
    }

    /// Whether `timeout_us` can be set. A `const fn`, for compile-time
    /// asserts.
    pub const fn check(timeout_us: u32) -> Result<(), Error> {
        match divider(timeout_us) {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Reloads the counter: the next reset is a full timeout away.
    pub fn feed(&self) {
        crate::chip::iwdg_regs().kr().write(|w| w.set_key(Key::RESET));
    }

    /// The timeout asked for, nominal.
    pub fn timeout_us(&self) -> u32 {
        self.timeout_us
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divider_picks_the_finest_prescaler() {
        // 50 ms at 32 kHz: 1600 counts, / 4 = 400.
        assert_eq!(divider(50_000), Ok((0, 399)));
        // 1 s: 32 000 counts; / 8 = 4000.
        assert_eq!(divider(1_000_000), Ok((1, 3999)));
        // 30 s: / 256 = 3750.
        assert_eq!(divider(30_000_000), Ok((6, 3749)));
        assert_eq!(divider(0), Err(Error::Timeout));
        assert_eq!(divider(40_000_000), Err(Error::Timeout));
    }
}
