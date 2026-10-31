//! Frequencies.

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
