//! General timers, exposed as counters: `timer_v1` (F4) and `timer_v3` (H7).
//! Their counter core (CR1, DIER, SR, EGR, CNT, PSC, ARR) is the same, so
//! they share this source.
//!
//! - [`Tick`]: the update interrupt at an exact rate, for a scheduler or a
//!   watchdog. The handler calls [`Tick::on_interrupt`].
//! - [`Timebase`]: a 32-bit timer counting freely at an exact frequency,
//!   read from anywhere through a `Copy` [`TimebaseReader`].
//!
//! Both handles carry their timer token and hand it back with
//! [`free`](Tick::free)/[`free`](Timebase::free): a second handle for the
//! same timer needs the token, so it is a compile error, not a runtime rule.
//! A rate the clock doesn't divide exactly is refused with the token in the
//! [`Refused`].
//!
//! Both work out the prescaler and reload from the timer's clock with a
//! `const fn` ([`divider`]), so a board can assert its rates while
//! compiling. A rate the clock doesn't divide exactly is refused rather
//! than rounded: a tick that drifts is worse than one that won't build.
//!
//! A timer's counter clock is its bus's timer clock (`Clocks::timer`): twice
//! the bus clock when the bus prescaler divides.
//!
//! Requirements: TIMER-001, TIMER-002, TIMER-003, TIMER-004, TIMER-005, API-002

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
pub use awg::{Awg, Lanes, recover};

use crate::{
    chip::Interrupt,
    rcc::{Clocks, Rcc, RccBit},
    resource::IrqError,
    resource::Refused,
    time::Hertz,
};

/// A timer this chip has. Chip facts: the glue layer implements it.
pub trait Instance: crate::Sealed + 'static {
    /// Its name in the reference manual.
    const NAME: &'static str;
    /// Its kernel clock: the timer clock of the bus it sits on.
    const KERNEL: crate::rcc::Kernel;
    #[doc(hidden)]
    const ENABLE: RccBit;
    /// The vector its update interrupt raises (shared with other timers on
    /// some chips, such as the F4's `TIM1_UP_TIM10`).
    const UPDATE: Interrupt;
    const UPDATE_NAME: &'static str;
    /// Counter width in bits: 16 or 32.
    const BITS: u8;
    #[doc(hidden)]
    fn info() -> &'static Info;
}

/// Per-instance facts: the timer's core registers. Chip facts: the glue
/// layer holds one static `Info` per timer. Its fields are private, so no
/// register type crosses the public API (G5).
pub struct Info {
    core: TimCore,
}

// SAFETY: `core` is an address.
unsafe impl Sync for Info {}

impl Info {
    pub(crate) const fn new(core: TimCore) -> Self {
        Self { core }
    }

    pub(crate) fn core(&self) -> TimCore {
        self.core
    }
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

/// One compare lane's wiring, as the board states it: channel `C` of timer
/// `T`, the pin in that channel's alternate function, and the DMA stream
/// that serves the channel's compare request. The IP traits check that they
/// belong together; this carries them and describes itself as data for the
/// board's manifest (`RESOURCES`), and [`assemble`][pwm] builds the lane
/// from it by type.
///
/// [pwm]: crate::timer::Pwm
pub struct LaneWiring<T, const C: u8, Pin, Stream> {
    /// The pin, in the channel's alternate function.
    pub pin: Pin,
    /// The DMA stream serving the channel's compare request.
    pub stream: Stream,
    _timer: core::marker::PhantomData<fn() -> T>,
}

/// One complementary lane's wiring (`CHxN`): as [`LaneWiring`], with the pin
/// in the complementary output's alternate function.
pub struct ComplementaryLaneWiring<T, const C: u8, Pin, Stream> {
    /// The pin, in the complementary output's alternate function.
    pub pin: Pin,
    /// The DMA stream serving the channel's compare request.
    pub stream: Stream,
    _timer: core::marker::PhantomData<fn() -> T>,
}

impl<T, const C: u8, Pin, Stream> LaneWiring<T, C, Pin, Stream> {
    /// A wiring for channel `C` of timer `T`, from its pin and stream.
    pub const fn new(pin: Pin, stream: Stream) -> Self {
        Self {
            pin,
            stream,
            _timer: core::marker::PhantomData,
        }
    }
}

impl<T, const C: u8, Pin, Stream> ComplementaryLaneWiring<T, C, Pin, Stream> {
    /// A wiring for channel `C`'s complementary output of timer `T`.
    pub const fn new(pin: Pin, stream: Stream) -> Self {
        Self {
            pin,
            stream,
            _timer: core::marker::PhantomData,
        }
    }
}

/// What a lane's assembly hands back when it refuses: the reason, the wiring,
/// and the buffers it took, in a tuple with the wiring first.
pub type LaneRefusal<T, const C: u8, Pin, Stream, Buffers> =
    crate::resource::Refused<ConfigError, (LaneWiring<T, C, Pin, Stream>, Buffers)>;

/// As [`LaneRefusal`], for a complementary lane's assembly.
pub type ComplementaryLaneRefusal<T, const C: u8, Pin, Stream, Buffers> =
    crate::resource::Refused<ConfigError, (ComplementaryLaneWiring<T, C, Pin, Stream>, Buffers)>;

/// What a two-way lane's assembly returns: the lane, or the refusal with
/// everything it took.
pub type TwoWayLaneAssembly<T, const C: u8, Pin, Stream> = Result<
    CompareLane<T, C, LaneWiring<T, C, Pin, Stream>>,
    LaneRefusal<T, C, Pin, Stream, (&'static mut [u32], &'static mut [u16])>,
>;

impl<T, Pin, const CD: u8, const SN: u8, const C: u8>
    LaneWiring<T, C, Pin, crate::dma::Stream<CD, SN>>
where
    T: ChannelTimer,
    Pin: ChannelPin<T, C> + crate::gpio::AltPin,
    crate::dma::Stream<CD, SN>: CompareDma<T, C>,
{
    /// Everything this wiring uses, as data: the instance, the pin with its
    /// alternate function, and the stream with its request.
    pub const RESOURCES: [crate::resource::Resource; 3] = [
        crate::resource::Resource::Peripheral(T::NAME),
        crate::resource::Resource::Pin {
            signal: crate::resource::Signal::Channel(C),
            port: Pin::PORT,
            pin: Pin::PIN,
            af: Pin::AF,
        },
        crate::resource::Resource::Dma {
            signal: crate::resource::Signal::Channel(C),
            controller: CD,
            stream: SN,
            request: <crate::dma::Stream<CD, SN> as CompareDma<T, C>>::REQUEST,
        },
    ];
}

impl<T, Pin, const CD: u8, const SN: u8, const C: u8>
    ComplementaryLaneWiring<T, C, Pin, crate::dma::Stream<CD, SN>>
where
    T: ChannelTimer,
    Pin: ComplementaryPin<T, C> + crate::gpio::AltPin,
    crate::dma::Stream<CD, SN>: CompareDma<T, C>,
{
    /// Everything this wiring uses, as data: the instance, the pin with its
    /// alternate function, and the stream with its request.
    pub const RESOURCES: [crate::resource::Resource; 3] = [
        crate::resource::Resource::Peripheral(T::NAME),
        crate::resource::Resource::Pin {
            signal: crate::resource::Signal::Complementary(C),
            port: Pin::PORT,
            pin: Pin::PIN,
            af: Pin::AF,
        },
        crate::resource::Resource::Dma {
            signal: crate::resource::Signal::Complementary(C),
            controller: CD,
            stream: SN,
            request: <crate::dma::Stream<CD, SN> as CompareDma<T, C>>::REQUEST,
        },
    ];
}

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
    /// A DMA stream was left running by an earlier owner and did not stop.
    DmaBusy,
    /// A lane's or sampler's buffer cannot serve DMA.
    Buffer(crate::dma::BufferError),
}

impl From<crate::dma::BufferError> for ConfigError {
    fn from(error: crate::dma::BufferError) -> Self {
        Self::Buffer(error)
    }
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

/// A timer's update interrupt at a fixed rate. The handle carries the timer
/// token: [`free`](Self::free) is the only way to build another handle for
/// the same timer.
pub struct Tick<T: Instance> {
    core: TimCore,
    rate: Hertz,
    vector: Interrupt,
    _timer: T,
}

impl<T: Instance> Tick<T> {
    /// Enables and resets the timer, sets it to overflow at `rate` exactly,
    /// enables its update interrupt and starts it. Refuses with the timer
    /// token handed back: the rate is checked before the timer is touched.
    pub fn new(rcc: &mut Rcc, timer: T, rate: Hertz) -> Result<Self, Refused<ConfigError, T>> {
        let (prescaler, reload) = match divider(T::KERNEL.clock(&rcc.clocks()), rate, T::BITS) {
            Ok(parts) => parts,
            Err(error) => return Err(Refused::new(error, timer)),
        };
        rcc.enable(T::ENABLE);
        rcc.reset(T::ENABLE);
        let core = T::info().core();
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
            _timer: timer,
        })
    }

    /// Whether `T` can tick at `rate` under `clocks`. A `const fn`, for a
    /// board's compile-time asserts: [`Tick::<TIM4>::check`].
    pub const fn check(clocks: &Clocks, rate: Hertz) -> Result<(), ConfigError> {
        match divider(T::KERNEL.clock(clocks), rate, T::BITS) {
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

    /// Stops the counter, kills its update interrupt and clears its flag:
    /// what [`free`](Self::free) does before handing the token back.
    pub fn stop(&mut self) {
        self.core.cr1().modify(|w| w.set_cen(false));
        self.core.dier().write_value(DierCore(0));
        clear_update(self.core);
    }

    /// Hands the timer token back, stopping the counter and its update
    /// interrupt: what a `Drop` would do, made explicit so the token can
    /// move out.
    pub fn free(self) -> T {
        let Self {
            core,
            rate: _,
            vector: _,
            _timer,
        } = self;
        core.cr1().modify(|w| w.set_cen(false));
        core.dier().write_value(DierCore(0));
        clear_update(core);
        _timer
    }
}

/// A 32-bit timer counting freely at a fixed frequency, from 0 at
/// [`new`](Self::new), wrapping at 2^32. The handle carries the timer token.
pub struct Timebase<T: Instance32> {
    core: TimCore,
    reader: TimebaseReader,
    _timer: T,
}

/// Reads a [`Timebase`]: one register read, so `Copy` and usable from any
/// context. A reader keeps reading the counter after the timebase is freed —
/// a freed timebase stops, and the reader reports the frozen count.
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

impl<T: Instance32> Timebase<T> {
    /// Enables and resets the timer and starts it counting at `frequency`
    /// exactly, over its whole 32-bit range. Refuses with the timer token
    /// handed back: the rate is checked before the timer is touched.
    pub fn new(rcc: &mut Rcc, timer: T, frequency: Hertz) -> Result<Self, Refused<ConfigError, T>> {
        let clock = T::KERNEL.clock(&rcc.clocks());
        let prescaler = match Self::prescaler(clock, frequency) {
            Ok(prescaler) => prescaler,
            Err(error) => return Err(Refused::new(error, timer)),
        };
        rcc.enable(T::ENABLE);
        rcc.reset(T::ENABLE);
        let core = T::info().core();
        load(core, prescaler, 1 << 32);
        core.cr1().modify(|w| w.set_cen(true));
        Ok(Self {
            core,
            reader: TimebaseReader {
                counter: T::gp32(),
                frequency,
            },
            _timer: timer,
        })
    }

    /// Whether `T` can count at `frequency` under `clocks`. A `const fn`,
    /// for a board's compile-time asserts: [`Timebase::<TIM2>::check`].
    pub const fn check(clocks: &Clocks, frequency: Hertz) -> Result<(), ConfigError> {
        match Self::prescaler(T::KERNEL.clock(clocks), frequency) {
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

    /// Hands the timer token back, stopping the counter: what a `Drop`
    /// would do, made explicit so the token can move out.
    pub fn free(self) -> T {
        self.core.cr1().modify(|w| w.set_cen(false));
        self._timer
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
