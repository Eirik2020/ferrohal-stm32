//! Frequencies.
//!
//! [`Hertz`] is FerroHAL's own type, not fugit's `Rate`, on purpose:
//!
//! - fugit is pre-1.0 and its majors split the ecosystem (0.3 for
//!   rtic-time 2, 0.6 for rtic-monotonics 3). Carrying it in `Clocks` and
//!   the configurations would tie every fugit major to a FerroHAL one.
//! - FerroHAL owns no time source, so it needs frequencies only, never
//!   fugit's instants and durations.
//! - `rcc::Config::validate` runs at compile time, and fugit's operators
//!   are not `const fn`.
//!
//! The `fugit` feature adds `From` conversions to and from fugit 0.6's
//! `HertzU32`. It pulls in fugit and its one dependency, `gcd`, and nothing
//! from RTIC or embassy.

/// A frequency in hertz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Hertz(pub u32);

impl Hertz {
    pub const fn hz(hz: u32) -> Self {
        Self(hz)
    }

    pub const fn khz(khz: u32) -> Self {
        Self(khz * 1_000)
    }

    pub const fn mhz(mhz: u32) -> Self {
        Self(mhz * 1_000_000)
    }

    pub const fn to_hz(self) -> u32 {
        self.0
    }
}

#[cfg(feature = "fugit")]
impl From<Hertz> for fugit::HertzU32 {
    fn from(freq: Hertz) -> Self {
        Self::from_raw(freq.0)
    }
}

#[cfg(feature = "fugit")]
impl From<fugit::HertzU32> for Hertz {
    fn from(rate: fugit::HertzU32) -> Self {
        Self(rate.to_raw())
    }
}

/// A free-running count of CPU cycles, wrapping at 2^32, that the caller
/// owns: FerroHAL owns no time source. Give one to a driver (`set_clock`)
/// and every wait lasts the time it asks for, instead of a counted bound
/// that can run several times longer on a slow configuration. The Cortex-M
/// DWT cycle counter is the usual one.
pub trait CycleClock: Sync {
    fn cycles(&self) -> u32;
}
