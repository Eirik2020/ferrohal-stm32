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
//!
//! Starting also freezes the counter while a debugger holds the core
//! halted (`DBGMCU`), so a breakpoint inside a session does not become a
//! reset. `iwdg_v2` adds a window register: [`Watchdog::start_windowed`]
//! resets the chip if fed before the window opens, which catches firmware
//! that feeds in a tight loop without doing its work, and keeps it running
//! when fed inside the window. The window is written last, after the
//! prescaler and reload have settled, and its write is the reload that
//! starts the count (RM0433 45.3.3). [`Watchdog::start`]
//! works from the nominal 32 kHz; [`Watchdog::start_with_lsi`] takes the
//! frequency a board measured, since the LSI varies from chip to chip.
//!
//! Requirements: IWDG-001, IWDG-002, IWDG-003, IWDG-004, IWDG-005, IWDG-006,
//! IWDG-007

use stm32_metapac::iwdg::vals::{Key, Pr};

use crate::{chip::WatchdogPeripheral, resource::Refused, time::Hertz};

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
    /// The window is longer than the timeout, or shorter than one count.
    Window,
}

/// The prescaler code and the reload value nearest `timeout_us` at the
/// nominal LSI.
pub const fn divider(timeout_us: u32) -> Result<(u8, u16), Error> {
    divider_with_lsi(timeout_us, LSI)
}

/// The prescaler code and the reload value nearest `timeout_us` at `lsi`,
/// the frequency a board measured, instead of the nominal [`LSI`].
pub const fn divider_with_lsi(timeout_us: u32, lsi: Hertz) -> Result<(u8, u16), Error> {
    // LSI counts in the timeout, before the prescaler.
    let counts = timeout_us as u64 * lsi.0 as u64 / 1_000_000;
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

/// The prescaler code, reload and window for a windowed watchdog: the
/// window, in counts at the chosen prescaler, must be inside the timeout
/// and at least one count.
pub const fn window_divider(timeout_us: u32, window_us: u32) -> Result<(u8, u16, u16), Error> {
    let (code, reload) = match divider(timeout_us) {
        Ok(parts) => parts,
        Err(error) => return Err(error),
    };
    // The window is compared with the counter, which ticks through the same
    // prescaler as the timeout. The counter loads the reload value and counts
    // down from it, so a window of `ticks` tick times is the register value
    // `ticks - 1`.
    let counts = window_us as u64 * LSI.0 as u64 / 1_000_000;
    // The prescaler divider per code: a match keeps every arm a nonzero
    // constant, so the divisions below carry no panic path for the public
    // API check. `divider` never returns a code above 6.
    let divide = match code {
        0 => 4u64,
        1 => 8,
        2 => 16,
        3 => 32,
        4 => 64,
        5 => 128,
        _ => 256,
    };
    let ticks = (counts + divide / 2) / divide;
    if ticks == 0 {
        return Err(Error::Window);
    }
    let window = ticks - 1;
    if window <= reload as u64 {
        Ok((code, reload, window as u16))
    } else {
        Err(Error::Window)
    }
}

/// Whether every pending register update (PVU, RVU, and on `iwdg_v2` WVU)
/// reached the watchdog's clock domain within [`UPDATE_SPINS`] reads: the
/// status register reading zero, which RM0433 45.3.3 waits for.
fn updated() -> bool {
    let regs = crate::chip::iwdg_regs();
    for _ in 0..UPDATE_SPINS {
        let sr = regs.sr().read();
        #[cfg(feature = "iwdg_v2")]
        let busy = sr.pvu() || sr.rvu() || sr.wvu();
        #[cfg(feature = "iwdg_v1")]
        let busy = sr.pvu() || sr.rvu();
        if !busy {
            return true;
        }
    }
    false
}

/// The running watchdog. `feed` is a single key write, so the handle can
/// sit wherever the firmware feeds from.
pub struct Watchdog {
    timeout_us: u32,
}

impl Watchdog {
    /// Starts the watchdog with `timeout_us`, nominal. It runs from here on;
    /// feed it within the timeout. Refuses with the watchdog token handed
    /// back when the timeout cannot be set; once the watchdog is running
    /// (an `Update` refusal), only a reset stops it.
    pub fn start(
        iwdg: WatchdogPeripheral,
        timeout_us: u32,
    ) -> Result<Self, Refused<Error, WatchdogPeripheral>> {
        let (code, reload) = match divider(timeout_us) {
            Ok(parts) => parts,
            Err(error) => return Err(Refused::new(error, iwdg)),
        };
        Self::start_with(iwdg, timeout_us, code, reload, None)
    }

    /// Starts the watchdog with `timeout_us` at the LSI frequency the board
    /// measured, rather than at the nominal 32 kHz of [`LSI`]. The LSI
    /// varies from chip to chip (17 to 47 kHz on the F4), so a board that
    /// needs the timeout to hold should measure its own and pass it here.
    pub fn start_with_lsi(
        iwdg: WatchdogPeripheral,
        timeout_us: u32,
        lsi: Hertz,
    ) -> Result<Self, Refused<Error, WatchdogPeripheral>> {
        let (code, reload) = match divider_with_lsi(timeout_us, lsi) {
            Ok(parts) => parts,
            Err(error) => return Err(Refused::new(error, iwdg)),
        };
        Self::start_with(iwdg, timeout_us, code, reload, None)
    }

    /// Starts the watchdog with a window: it resets the chip if fed before
    /// the last `window_us` of the timeout. `iwdg_v2` only; the F4s have no
    /// window register, so this is absent there.
    ///
    /// Refuses with the token handed back when the timeout or window cannot
    /// be set, as [`Watchdog::start`] does. After an `Update` refusal the
    /// window may already be in force, so feed only inside it.
    #[cfg(feature = "iwdg_v2")]
    pub fn start_windowed(
        iwdg: WatchdogPeripheral,
        timeout_us: u32,
        window_us: u32,
    ) -> Result<Self, Refused<Error, WatchdogPeripheral>> {
        let (code, reload, window) = match window_divider(timeout_us, window_us) {
            Ok(parts) => parts,
            Err(error) => return Err(Refused::new(error, iwdg)),
        };
        Self::start_with(iwdg, timeout_us, code, reload, Some(window))
    }

    fn start_with(
        iwdg: WatchdogPeripheral,
        timeout_us: u32,
        code: u8,
        reload: u16,
        window: Option<u16>,
    ) -> Result<Self, Refused<Error, WatchdogPeripheral>> {
        let regs = crate::chip::iwdg_regs();
        // A breakpoint must not become a reset: freeze the counter while a
        // debugger holds the core halted.
        crate::chip::set_watchdog_freeze();
        // Starting turns the LSI on; then unlock, load, and wait for the
        // prescaler and reload to reach the watchdog's clock domain.
        regs.kr().write(|w| w.set_key(Key::START));
        regs.kr().write(|w| w.set_key(Key::ENABLE));
        regs.pr().write(|w| w.set_pr(Pr::from_bits(code)));
        regs.rlr().write(|w| w.set_rl(reload));
        let settled = updated();
        #[cfg(feature = "iwdg_v2")]
        if let (true, Some(window)) = (settled, window) {
            // RM0433 45.3.3, window enabled: writing WINR reloads the counter
            // from RLR, so it is the last step. A key reload after it would
            // land with the counter at its reload, above the window, and
            // reset the chip; the bench measured that within 1 ms of the
            // start. Like ST's own driver, this leaves register access open:
            // the first feed closes it. Waiting for WVU means the window is
            // in force once this returns.
            regs.winr().write(|w| w.set_win(window));
            if !updated() {
                return Err(Refused::new(Error::Update, iwdg));
            }
            return Ok(Self { timeout_us });
        }
        #[cfg(feature = "iwdg_v1")]
        let _ = window;
        // No window (RM0433 45.3.3, RM0368 and RM0090's sequence): reload the
        // counter from the new values. If they never settled, no window has
        // been written either, so this reload is safe and keeps the watchdog
        // fed with whatever it had.
        regs.kr().write(|w| w.set_key(Key::RESET));
        if !settled {
            return Err(Refused::new(Error::Update, iwdg));
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

    /// Another handle on the running watchdog, for a second context to feed
    /// it from. Feeding is a single key write, so handles need no owner.
    pub const fn handle(&self) -> Self {
        Self {
            timeout_us: self.timeout_us,
        }
    }

    /// Reloads the counter: the next reset is a full timeout away.
    pub fn feed(&self) {
        crate::chip::iwdg_regs()
            .kr()
            .write(|w| w.set_key(Key::RESET));
    }

    /// The timeout asked for, nominal.
    pub fn timeout_us(&self) -> u32 {
        self.timeout_us
    }

    /// Whether the watchdog is frozen while a debugger holds the core
    /// halted. [`Watchdog::start`] sets it.
    pub fn is_frozen_on_halt() -> bool {
        crate::chip::watchdog_frozen()
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

    #[test]
    fn divider_follows_the_lsi() {
        // The same 50 ms at a board's measured 30 kHz: 1500 counts, / 4 =
        // 375 ticks, reload 374. At the nominal 32 kHz it stays 399.
        assert_eq!(divider_with_lsi(50_000, Hertz::hz(30_000)), Ok((0, 374)));
        assert_eq!(divider_with_lsi(50_000, LSI), Ok((0, 399)));
        assert_eq!(divider_with_lsi(50_000, Hertz::hz(0)), Err(Error::Timeout));
    }

    #[test]
    fn window_is_inside_the_timeout() {
        // 50 ms at 32 kHz: 1600 counts, / 4 = 400 ticks, reload 399. A
        // 10 ms window is 80 ticks, so the register takes 79.
        assert_eq!(window_divider(50_000, 10_000), Ok((0, 399, 79)));
        // A window as long as the timeout is the whole reload.
        assert_eq!(window_divider(50_000, 50_000), Ok((0, 399, 399)));
        // No window, or one longer than the timeout.
        assert_eq!(window_divider(50_000, 0), Err(Error::Window));
        assert_eq!(window_divider(50_000, 60_000), Err(Error::Window));
    }
}
