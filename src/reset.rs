//! Why the chip last reset, as `Rcc::reset_cause` reads it.

/// The reset flags RCC latched, chip-neutral. More than one can be set: a
/// power-on reset also sets the pin flag, for one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ResetCause {
    pub power_on: bool,
    pub pin: bool,
    pub brownout: bool,
    pub software: bool,
    pub independent_watchdog: bool,
    pub window_watchdog: bool,
    pub low_power: bool,
}
