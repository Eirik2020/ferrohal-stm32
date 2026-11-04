//! An arbitrary waveform generator: compare lanes on one or two timers, each
//! playing its own sequence of samples, all from one start. Part of the timer
//! module, for both IP versions.
//!
//! A sample is a duty in timer counts, held for one period of the timer, so the
//! sample rate is the timer's rate and full scale is [`Awg::period`]. A waveform
//! ends at duty 0: the lane appends it, so the output falls and stays low. What
//! the samples mean is the caller's. A protocol encodes its frames as samples.
//!
//! With two timers, either the first starts the second through its trigger
//! output, on one edge ([`Awg::linked`]), or both start in software, one after
//! the other, for timers with no trigger route between them
//! ([`Awg::independent`]). Both timers must run at one period, so a sample means
//! the same on every lane.
//!
//! Lanes are numbered in the order they were given: the first timer's, then the
//! second's. The firmware binds each lane's stream vector, or polls, and calls
//! [`Awg::on_interrupt`] with that lane. FerroHAL enables nothing in the NVIC.
//!
//! Requirements: AWG-001, AWG-002, AWG-003, AWG-004, AWG-005, AWG-006, AWG-007

use super::pwm::{CompareLane, Error, Pwm};
use super::{ChannelTimer, ConfigError, TriggeredBy};
use crate::{chip::Interrupt, time::Hertz};

/// The compare lanes of one timer, as a tuple: none, or one to four.
pub trait Lanes<T: ChannelTimer> {
    /// How many lanes.
    const COUNT: usize;
    /// Lane `index`'s channel, numbered from 1. 0 past the last lane.
    fn channel(index: usize) -> u8;
    /// Loads lane `index`; see [`CompareLane::arm`].
    fn arm(&mut self, index: usize, samples: &[u32]) -> Result<(), Error>;
    /// Serves lane `index`'s stream; see [`CompareLane::on_interrupt`].
    fn serve(&mut self, index: usize) -> Option<Result<(), Error>>;
    /// Stops every lane, with its output at duty 0.
    fn abort_all(&mut self);
    /// Lane `index`'s stream vector.
    fn vector(&self, index: usize) -> Option<Interrupt>;
    /// Turns every lane's output on or off.
    fn set_outputs(pwm: &mut Pwm<T>, on: bool);
}

impl<T: ChannelTimer> Lanes<T> for () {
    const COUNT: usize = 0;
    fn channel(_index: usize) -> u8 {
        0
    }
    fn arm(&mut self, _index: usize, _samples: &[u32]) -> Result<(), Error> {
        Err(Error::Empty)
    }
    fn serve(&mut self, _index: usize) -> Option<Result<(), Error>> {
        None
    }
    fn abort_all(&mut self) {}
    fn vector(&self, _index: usize) -> Option<Interrupt> {
        None
    }
    fn set_outputs(_pwm: &mut Pwm<T>, _on: bool) {}
}

macro_rules! lanes {
    ($count:literal: $($index:tt $C:ident),+) => {
        impl<T: ChannelTimer, $(const $C: u8),+> Lanes<T> for ($(CompareLane<T, $C>,)+) {
            const COUNT: usize = $count;
            fn channel(index: usize) -> u8 {
                match index {
                    $($index => $C,)+
                    _ => 0,
                }
            }
            fn arm(&mut self, index: usize, samples: &[u32]) -> Result<(), Error> {
                match index {
                    $($index => self.$index.arm(samples),)+
                    _ => Err(Error::Empty),
                }
            }
            fn serve(&mut self, index: usize) -> Option<Result<(), Error>> {
                match index {
                    $($index => self.$index.on_interrupt(),)+
                    _ => None,
                }
            }
            fn abort_all(&mut self) {
                $(self.$index.abort();)+
            }
            fn vector(&self, index: usize) -> Option<Interrupt> {
                match index {
                    $($index => Some(self.$index.interrupt()),)+
                    _ => None,
                }
            }
            fn set_outputs(pwm: &mut Pwm<T>, on: bool) {
                $(pwm.set_output::<$C>(on);)+
            }
        }
    };
}

lanes!(1: 0 A);
lanes!(2: 0 A, 1 B);
lanes!(3: 0 A, 1 B, 2 C);
lanes!(4: 0 A, 1 B, 2 C, 3 D);

/// The bit in `stage`'s mask for `channel`, numbered from 1. 0 for no channel.
const fn channel_bit(channel: u8) -> u8 {
    if channel == 0 {
        0
    } else {
        1 << ((channel - 1) & 3)
    }
}

/// What the generator's lanes are doing, one bit per lane. Lanes number at most
/// eight, so a byte holds them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Playing {
    /// Loaded, and not yet finished.
    active: u8,
    /// The timers are running.
    started: bool,
}

/// How a two-timer generator starts its second timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Start {
    /// There is no second timer.
    Single,
    /// By the first timer's trigger output.
    Linked,
    /// In software, just after the first.
    Independent,
}

/// See the [module documentation](self).
pub struct Awg<A: ChannelTimer, B: ChannelTimer, LA, LB> {
    first: Pwm<A>,
    second: Option<Pwm<B>>,
    first_lanes: LA,
    second_lanes: LB,
    start: Start,
    state: Playing,
}

impl<A: ChannelTimer, LA: Lanes<A>> Awg<A, A, LA, ()> {
    /// A generator on one timer.
    pub fn single(first: Pwm<A>, lanes: LA) -> Self {
        Self {
            first,
            second: None,
            first_lanes: lanes,
            second_lanes: (),
            start: Start::Single,
            state: Playing::default(),
        }
    }
}

impl<A: ChannelTimer, B: ChannelTimer, LA: Lanes<A>, LB: Lanes<B>> Awg<A, B, LA, LB> {
    /// A generator on two timers at one period, the first starting the second
    /// through its trigger output, so every lane begins on one edge. Refuses
    /// timers at different periods.
    pub fn linked(
        mut first: Pwm<A>,
        mut second: Pwm<B>,
        first_lanes: LA,
        second_lanes: LB,
    ) -> Result<Self, ConfigError>
    where
        B: TriggeredBy<A>,
    {
        if first.period() != second.period() {
            return Err(ConfigError::PeriodMismatch);
        }
        first.lead();
        second.follow(&first);
        Ok(Self {
            first,
            second: Some(second),
            first_lanes,
            second_lanes,
            start: Start::Linked,
            state: Playing::default(),
        })
    }

    /// A generator on two timers at one period, started in software one after
    /// the other, for timers with no trigger route between them. The second
    /// begins a few instructions after the first. Refuses timers at different
    /// periods.
    pub fn independent(
        first: Pwm<A>,
        second: Pwm<B>,
        first_lanes: LA,
        second_lanes: LB,
    ) -> Result<Self, ConfigError> {
        if first.period() != second.period() {
            return Err(ConfigError::PeriodMismatch);
        }
        Ok(Self {
            first,
            second: Some(second),
            first_lanes,
            second_lanes,
            start: Start::Independent,
            state: Playing::default(),
        })
    }

    /// How many lanes.
    pub const fn lanes(&self) -> usize {
        LA::COUNT + LB::COUNT
    }

    /// Full scale for a sample: the timer's period, in counts.
    pub fn period(&self) -> u32 {
        self.first.period()
    }

    /// Samples a second.
    pub fn sample_rate(&self) -> Hertz {
        self.first.frequency()
    }

    /// Turns every lane's output on or off. Off, a pin falls to its inactive
    /// level.
    pub fn set_outputs(&mut self, on: bool) {
        LA::set_outputs(&mut self.first, on);
        if let Some(second) = self.second.as_mut() {
            LB::set_outputs(second, on);
        }
    }

    /// Whether a waveform is loaded or playing.
    pub const fn is_busy(&self) -> bool {
        self.state.active != 0
    }

    /// Loads `samples` on `lane`, for the next [`play`](Self::play). The lane's
    /// first sample is its duty from the first period on.
    pub fn load(&mut self, lane: usize, samples: &[u32]) -> Result<(), Error> {
        if self.state.started {
            return Err(Error::Busy);
        }
        let bit = match u32::try_from(lane) {
            Ok(index) if lane < self.lanes() => 1u8 << (index & 7),
            _ => return Err(Error::Empty),
        };
        if lane < LA::COUNT {
            self.first_lanes.arm(lane, samples)?;
        } else {
            self.second_lanes.arm(lane - LA::COUNT, samples)?;
        }
        self.state.active |= bit;
        Ok(())
    }

    /// Plays every loaded lane, from one start. Refused while one plays, or with
    /// nothing loaded.
    pub fn play(&mut self) -> Result<(), Error> {
        if self.state.started {
            return Err(Error::Busy);
        }
        if self.state.active == 0 {
            return Err(Error::Empty);
        }
        let (first, second) = self.masks();
        self.first.stage(first);
        if let Some(timer) = self.second.as_mut() {
            timer.stage(second);
        }
        self.state.started = true;
        self.first.start();
        if self.start == Start::Independent
            && let Some(timer) = self.second.as_mut()
        {
            timer.start();
        }
        Ok(())
    }

    /// The compare requests to open on each timer: the loaded lanes' channels.
    fn masks(&self) -> (u8, u8) {
        let (mut first, mut second) = (0, 0);
        let mut lane = 0;
        while lane < LA::COUNT + LB::COUNT {
            if self.state.active & (1 << (lane & 7)) != 0 {
                if lane < LA::COUNT {
                    first |= channel_bit(LA::channel(lane));
                } else {
                    second |= channel_bit(LB::channel(lane - LA::COUNT));
                }
            }
            lane += 1;
        }
        (first, second)
    }

    /// Serves `lane`'s stream: `Some` once its waveform has ended. When the last
    /// lane ends, the timers stop and their requests close. A lane that fails
    /// stops the whole generator, since its waveforms belong together.
    pub fn on_interrupt(&mut self, lane: usize) -> Option<Result<(), Error>> {
        let result = if lane < LA::COUNT {
            self.first_lanes.serve(lane)
        } else {
            self.second_lanes.serve(lane.wrapping_sub(LA::COUNT))
        }?;
        if result.is_err() {
            self.abort();
            return Some(result);
        }
        self.state.active &= !(1u8 << (lane & 7));
        if self.state.active == 0 {
            self.halt();
        }
        Some(result)
    }

    /// [`on_interrupt`](Self::on_interrupt), for firmware that polls.
    pub fn poll(&mut self, lane: usize) -> Option<Result<(), Error>> {
        self.on_interrupt(lane)
    }

    /// Stops everything, with every output at duty 0, ready to load again.
    pub fn abort(&mut self) {
        self.first_lanes.abort_all();
        self.second_lanes.abort_all();
        self.halt();
        self.state.active = 0;
    }

    fn halt(&mut self) {
        self.first.stop();
        self.first.close_requests();
        if let Some(second) = self.second.as_mut() {
            second.stop();
            second.close_requests();
        }
        self.state.started = false;
    }

    /// `lane`'s stream vector, to bind to [`on_interrupt`](Self::on_interrupt).
    pub fn interrupt(&self, lane: usize) -> Option<Interrupt> {
        if lane < LA::COUNT {
            self.first_lanes.vector(lane)
        } else {
            self.second_lanes.vector(lane.wrapping_sub(LA::COUNT))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::channel_bit;

    #[test]
    fn each_channel_has_its_own_request_bit() {
        assert_eq!(channel_bit(0), 0);
        assert_eq!(channel_bit(1), 0b0001);
        assert_eq!(channel_bit(2), 0b0010);
        assert_eq!(channel_bit(3), 0b0100);
        assert_eq!(channel_bit(4), 0b1000);
    }
}
