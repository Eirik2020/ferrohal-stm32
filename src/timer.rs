//! General timers, exposed as counters: `timer_v1` (F4) and `timer_v3` (H7).
//! Their counter core (CR1, DIER, SR, EGR, CNT, PSC, ARR) is the same, so
//! they share this source.
//!
//! - [`Tick`]: the update interrupt at an exact rate, for a scheduler or a
//!   watchdog. The handler calls [`Tick::on_interrupt`].
//! - [`Timebase`]: a 32-bit timer counting freely at an exact frequency,
//!   read from anywhere through a `Copy` [`TimebaseReader`].
//!
//! Both work out the prescaler and reload from the timer's clock with a
//! `const fn` ([`divider`]), so a board can assert its rates while
//! compiling. A rate the clock doesn't divide exactly is refused rather
//! than rounded: a tick that drifts is worse than one that won't build.
//!
//! A timer's counter clock is its bus's timer clock (`Clocks::timer`): twice
//! the bus clock when the bus prescaler divides.
//!
//! Requirements: TIMER-001, TIMER-002, TIMER-003, TIMER-004, TIMER-005

use stm32_metapac::timer::{
    TimCore, TimGp32,
    regs::{ArrCore, DierCore, SrCore},
    vals::Urs,
};

#[path = "timer_pwm.rs"]
mod pwm;
pub use pwm::{CompareLane, Error, Pwm, PwmChannel, Sampler, nearest};

#[path = "timer_awg.rs"]
mod awg;
pub use awg::{Awg, Lanes};

use crate::{
    chip::Interrupt,
    rcc::{Bus, Clocks, Rcc, RccBit},
    resource::IrqError,
    time::Hertz,
};

/// A timer this chip has. Chip facts: the glue layer implements it.
pub trait Instance: crate::Sealed + 'static {
    /// Its name in the reference manual.
    const NAME: &'static str;
    /// The bus it sits on; it counts at that bus's timer clock.
    const BUS: Bus;
    #[doc(hidden)]
    const ENABLE: RccBit;
    /// The vector its update interrupt raises (shared with other timers on
    /// some chips, such as the F4's `TIM1_UP_TIM10`).
    const UPDATE: Interrupt;
    const UPDATE_NAME: &'static str;
    /// Counter width in bits: 16 or 32.
    const BITS: u8;
    #[doc(hidden)]
    fn core() -> TimCore;
}

/// A timer with a 32-bit counter (TIM2 and TIM5 on these chips).
pub trait Instance32: Instance {
    #[doc(hidden)]
    fn gp32() -> TimGp32;
}

/// A timer with capture/compare channels. Chip facts: the glue implements
/// it.
pub trait ChannelTimer: Instance {
    /// Channels 1 to `CHANNELS`.
    const CHANNELS: u8;
    /// Whether it has a break unit (BDTR), whose MOE bit must be set for
    /// any output to drive its pin: TIM1, TIM8 and TIM15 to TIM17.
    const BREAK: bool;
}

/// A timer that another timer's trigger output can start: `M`'s TRGO reaches
/// this timer as internal trigger `ITR` (RM0090 table 94, RM0368 table 54,
/// RM0433 table 336). A chip fact, from the glue.
pub trait TriggeredBy<M: ChannelTimer>: ChannelTimer {
    const ITR: u8;
}

/// A pin, in the right alternate function, that carries channel `C` of `T`.
pub trait ChannelPin<T: ChannelTimer, const C: u8>: crate::Sealed {}

/// A pin, in the right alternate function, that carries the complementary
/// output of channel `C` of `T` (CHxN).
pub trait ComplementaryPin<T: ChannelTimer, const C: u8>: crate::Sealed {}

/// A DMA stream that serves the compare request of channel `C` of `T`.
///
/// # Safety
///
/// `REQUEST` must route that request to this stream.
pub unsafe trait CompareDma<T: ChannelTimer, const C: u8>:
    crate::dma::StreamInstance + crate::dma::StreamInterrupt
{
    const REQUEST: u8;
}

/// A DMA stream that serves the update request of `T`.
///
/// # Safety
///
/// `REQUEST` must route that request to this stream.
pub unsafe trait UpdateDma<T: Instance>:
    crate::dma::StreamInstance + crate::dma::StreamInterrupt
{
    const REQUEST: u8;
}

/// A DMA stream whose peripheral port reaches the GPIO ports: on F4 only
/// DMA2's (DMA1's reaches APB1 alone), on H7 every DMA1 and DMA2 stream.
pub trait GpioDma: crate::dma::StreamInstance {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError {
    /// The rate is 0, or the timer clock isn't a whole multiple of it.
    Inexact,
    /// The rate is too slow for the prescaler and reload together, or too
    /// fast for the clock.
    OutOfRange,
    /// A waveform generator's two timers run at different periods.
    PeriodMismatch,
}

/// The prescaler and reload that divide `clock` down to `rate` exactly:
/// (PSC + 1, ARR + 1), the prescaler at most 65 536 and the reload at most
/// `2^bits`. The smallest prescaler that fits, so the counter runs as fast
/// as it can.
pub const fn divider(clock: Hertz, rate: Hertz, bits: u8) -> Result<(u32, u64), ConfigError> {
    if rate.0 == 0 || !clock.0.is_multiple_of(rate.0) {
        return Err(ConfigError::Inexact);
    }
    let total = (clock.0 / rate.0) as u64;
    let reload_max: u64 = if bits >= 32 { 1 << 32 } else { 1 << 16 };
    if total < 2 {
        return Err(ConfigError::OutOfRange);
    }
    // The smallest prescaler that brings the reload into range, then up
    // from there to the first that divides exactly.
    let mut prescaler = total.div_ceil(reload_max);
    if prescaler == 0 {
        prescaler = 1;
    }
    while prescaler <= 65_536 {
        if total.is_multiple_of(prescaler) && total / prescaler >= 2 {
            return Ok((prescaler as u32, total / prescaler));
        }
        prescaler += 1;
    }
    Err(ConfigError::OutOfRange)
}

/// Clears the update flag. SR's flags are write-zero-to-clear, and these
/// drivers own the whole timer, so clearing the others too loses nothing.
fn clear_update(core: TimCore) {
    core.sr().write_value(SrCore(0));
}

/// Stops, loads PSC and ARR through an update event without raising its
/// flag, and leaves the counter at 0, stopped.
fn load(core: TimCore, prescaler: u32, reload: u64) {
    core.cr1().write(|w| w.set_urs(Urs::COUNTER_ONLY));
    core.dier().write_value(DierCore(0));
    core.psc().write_value((prescaler - 1) as u16);
    // The raw word: a 32-bit timer takes all of it, a 16-bit one the low
    // half, which is all a 16-bit reload has.
    core.arr().write_value(ArrCore((reload - 1) as u32));
    core.egr().write(|w| w.set_ug(true));
    core.cnt().write(|w| w.set_cnt(0));
    clear_update(core);
}

/// A timer's update interrupt at a fixed rate.
pub struct Tick {
    core: TimCore,
    rate: Hertz,
    vector: Interrupt,
}

impl Tick {
    /// Enables and resets the timer, sets it to overflow at `rate` exactly,
    /// enables its update interrupt and starts it.
    pub fn new<T: Instance>(rcc: &mut Rcc, _timer: T, rate: Hertz) -> Result<Self, ConfigError> {
        let (prescaler, reload) = divider(rcc.clocks().timer(T::BUS), rate, T::BITS)?;
        rcc.enable(T::ENABLE);
        rcc.reset(T::ENABLE);
        let core = T::core();
        load(core, prescaler, reload);
        core.dier().write(|w| w.set_uie(true));
        core.cr1().modify(|w| {
            w.set_arpe(true);
            w.set_cen(true);
        });
        Ok(Self {
            core,
            rate,
            vector: T::UPDATE,
        })
    }

    /// Whether `T` can tick at `rate` under `clocks`. A `const fn`, for a
    /// board's compile-time asserts.
    pub const fn check<T: Instance>(clocks: &Clocks, rate: Hertz) -> Result<(), ConfigError> {
        match divider(clocks.timer(T::BUS), rate, T::BITS) {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Serves the update interrupt: clears the flag and says whether it was
    /// set. `false` on a shared vector raised by another timer.
    pub fn on_interrupt(&mut self) -> bool {
        let pending = self.core.sr().read().uif();
        if pending {
            clear_update(self.core);
        }
        pending
    }

    pub fn rate(&self) -> Hertz {
        self.rate
    }

    /// The vector to bind to [`on_interrupt`](Self::on_interrupt).
    pub fn interrupt(&self) -> Interrupt {
        self.vector
    }

    /// Checks the NVIC, reading only: the vector is enabled.
    pub fn verify(&self) -> Result<(), IrqError> {
        crate::resource::verify_vectors([self.vector].into_iter())
    }

    /// Stops counting; no more ticks until [`resume`](Self::resume).
    pub fn pause(&mut self) {
        self.core.cr1().modify(|w| w.set_cen(false));
    }

    /// Counts on from where it stopped.
    pub fn resume(&mut self) {
        self.core.cr1().modify(|w| w.set_cen(true));
    }
}

/// A 32-bit timer counting freely at a fixed frequency, from 0 at
/// [`new`](Self::new), wrapping at 2^32.
pub struct Timebase {
    reader: TimebaseReader,
}

/// Reads a [`Timebase`]: one register read, so `Copy` and usable from any
/// context.
#[derive(Clone, Copy)]
pub struct TimebaseReader {
    counter: TimGp32,
    frequency: Hertz,
}

// SAFETY: a reader only reads CNT, which nothing else writes once the
// timebase runs.
unsafe impl Send for TimebaseReader {}
// SAFETY: as above.
unsafe impl Sync for TimebaseReader {}

impl Timebase {
    /// Enables and resets the timer and starts it counting at `frequency`
    /// exactly, over its whole 32-bit range.
    pub fn new<T: Instance32>(
        rcc: &mut Rcc,
        _timer: T,
        frequency: Hertz,
    ) -> Result<Self, ConfigError> {
        let clock = rcc.clocks().timer(T::BUS);
        let prescaler = Self::prescaler(clock, frequency)?;
        rcc.enable(T::ENABLE);
        rcc.reset(T::ENABLE);
        let core = T::core();
        load(core, prescaler, 1 << 32);
        core.cr1().modify(|w| w.set_cen(true));
        Ok(Self {
            reader: TimebaseReader {
                counter: T::gp32(),
                frequency,
            },
        })
    }

    /// Whether `T` can count at `frequency` under `clocks`.
    pub const fn check<T: Instance32>(
        clocks: &Clocks,
        frequency: Hertz,
    ) -> Result<(), ConfigError> {
        match Self::prescaler(clocks.timer(T::BUS), frequency) {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }

    const fn prescaler(clock: Hertz, frequency: Hertz) -> Result<u32, ConfigError> {
        if frequency.0 == 0 || !clock.0.is_multiple_of(frequency.0) {
            return Err(ConfigError::Inexact);
        }
        let prescaler = clock.0 / frequency.0;
        if prescaler > 65_536 {
            return Err(ConfigError::OutOfRange);
        }
        Ok(prescaler)
    }

    pub fn now(&self) -> u32 {
        self.reader.now()
    }

    pub fn reader(&self) -> TimebaseReader {
        self.reader
    }
}

impl TimebaseReader {
    /// The count now.
    pub fn now(&self) -> u32 {
        self.counter.cnt().read()
    }

    pub fn frequency(&self) -> Hertz {
        self.frequency
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divider_is_exact_or_refuses() {
        // 1 kHz from 84 MHz on a 16-bit timer: 2 * 42 000.
        assert_eq!(divider(Hertz::mhz(84), Hertz::khz(1), 16), Ok((2, 42_000)));
        // The same on a 32-bit timer needs no prescaler.
        assert_eq!(divider(Hertz::mhz(84), Hertz::khz(1), 32), Ok((1, 84_000)));
        // 8 kHz control loop from the H7's 200 MHz timer clock.
        assert_eq!(divider(Hertz::mhz(200), Hertz::khz(8), 16), Ok((1, 25_000)));
        // 1 Hz on 16 bits: some exact split with both halves in range.
        let (p, r) = divider(Hertz::mhz(84), Hertz(1), 16).unwrap();
        assert_eq!(u64::from(p) * r, 84_000_000);
        assert!(r <= 65_536 && p <= 65_536);
        assert_eq!(
            divider(Hertz::mhz(84), Hertz(7_000_001), 16),
            Err(ConfigError::Inexact)
        );
        assert_eq!(
            divider(Hertz::mhz(84), Hertz(0), 16),
            Err(ConfigError::Inexact)
        );
        assert_eq!(
            divider(Hertz::mhz(84), Hertz::mhz(84), 16),
            Err(ConfigError::OutOfRange)
        );
    }
}
