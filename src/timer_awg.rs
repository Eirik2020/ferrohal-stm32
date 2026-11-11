//! An arbitrary waveform generator: compare lanes on one or two timers, each
//! playing its own sequence of samples, all from one start. Part of the timer
//! module, for both IP versions.
//!
//! A sample is a duty in timer counts, held for one period of the timer, so the
//! sample rate is the timer's rate and full scale is [`Awg::period`]. A waveform
//! ends at duty 0: the lane appends it, so the output falls and stays low. What
//! the samples mean is the caller's. A protocol encodes its frames as samples.
//!
//! Lanes built two-way read as well. [`Awg::listen`] releases the outputs, runs
//! the timers at the capture rate and has each lane's stream copy its port's input
//! register into memory, paced by its own channel's compare request, so a lane can
//! answer on the pin it sends on. [`Awg::speak`] turns them back. [`recover`] turns
//! what was read back into samples. Outputs can be active low, for a line that
//! idles high.
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
//! Requirements: AWG-001, AWG-002, AWG-003, AWG-004, AWG-005, AWG-006, AWG-007, AWG-008, AWG-009, AWG-010, AWG-011, AWG-012

use super::pwm::{CompareLane, Error, Pwm};
use super::{ChannelTimer, ConfigError, TriggeredBy};
use crate::{chip::Interrupt, resource::Refused, time::Hertz};

/// What the generator's constructors hand back when they refuse: the reason,
/// and both timers with both lane groups.
pub type AwgRefused<A, B, LA, LB> = crate::resource::Refused<ConfigError, (Pwm<A>, Pwm<B>, LA, LB)>;

/// The compare lanes of one timer, as a tuple: none, or one to four.
pub trait Lanes<T: ChannelTimer> {
    /// How many lanes.
    const COUNT: usize;
    /// Lane `index`'s channel, numbered from 1. 0 past the last lane.
    fn channel(index: usize) -> u8;
    /// Loads lane `index`; see [`CompareLane::arm`]. The timer handle goes
    /// along, for the compare write.
    fn arm(&mut self, index: usize, pwm: &Pwm<T>, samples: &[u32]) -> Result<(), Error>;
    /// Serves lane `index`'s stream; see [`CompareLane::on_interrupt`].
    fn serve(&mut self, index: usize) -> Option<Result<(), Error>>;
    /// Stops every lane, with its output at duty 0.
    fn abort_all(&mut self, pwm: &Pwm<T>);
    /// Lane `index`'s stream vector.
    fn vector(&self, index: usize) -> Option<Interrupt>;
    /// Turns every lane's output on or off.
    fn set_outputs(pwm: &Pwm<T>, on: bool);
    /// Makes every lane's output active low, or active high.
    fn set_active_low(pwm: &Pwm<T>, low: bool);
    /// Turns every lane's stream to read; see [`CompareLane::listen`].
    fn listen(&mut self, pwm: &Pwm<T>, count: usize, duty: u32) -> Result<(), Error>;
    /// What lane `index` last read.
    fn samples(&self, index: usize) -> &[u16];
    /// Sets every lane's duty to 0, its inactive level.
    fn park(&mut self, pwm: &Pwm<T>);
}

impl<T: ChannelTimer> Lanes<T> for () {
    const COUNT: usize = 0;
    fn channel(_index: usize) -> u8 {
        0
    }
    fn arm(&mut self, _index: usize, _pwm: &Pwm<T>, _samples: &[u32]) -> Result<(), Error> {
        Err(Error::Empty)
    }
    fn serve(&mut self, _index: usize) -> Option<Result<(), Error>> {
        None
    }
    fn abort_all(&mut self, _pwm: &Pwm<T>) {}
    fn vector(&self, _index: usize) -> Option<Interrupt> {
        None
    }
    fn set_outputs(_pwm: &Pwm<T>, _on: bool) {}
    fn set_active_low(_pwm: &Pwm<T>, _low: bool) {}
    fn listen(&mut self, _pwm: &Pwm<T>, _count: usize, _duty: u32) -> Result<(), Error> {
        Ok(())
    }
    fn samples(&self, _index: usize) -> &[u16] {
        &[]
    }
    fn park(&mut self, _pwm: &Pwm<T>) {}
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
            fn arm(&mut self, index: usize, pwm: &Pwm<T>, samples: &[u32]) -> Result<(), Error> {
                match index {
                    $($index => self.$index.arm(pwm, samples),)+
                    _ => Err(Error::Empty),
                }
            }
            fn serve(&mut self, index: usize) -> Option<Result<(), Error>> {
                match index {
                    $($index => self.$index.on_interrupt(),)+
                    _ => None,
                }
            }
            fn abort_all(&mut self, pwm: &Pwm<T>) {
                $(self.$index.abort(pwm);)+
            }
            fn vector(&self, index: usize) -> Option<Interrupt> {
                match index {
                    $($index => Some(self.$index.interrupt()),)+
                    _ => None,
                }
            }
            fn set_outputs(pwm: &Pwm<T>, on: bool) {
                $(pwm.set_output::<$C>(on);)+
            }
            fn set_active_low(pwm: &Pwm<T>, low: bool) {
                $(pwm.set_active_low::<$C>(low);)+
            }
            fn listen(&mut self, pwm: &Pwm<T>, count: usize, duty: u32) -> Result<(), Error> {
                $(self.$index.listen(pwm, count, duty)?;)+
                Ok(())
            }
            fn samples(&self, index: usize) -> &[u16] {
                match index {
                    $($index => self.$index.samples(),)+
                    _ => &[],
                }
            }
            fn park(&mut self, pwm: &Pwm<T>) {
                $(pwm.set_duty::<$C>(0);)+
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
    /// Turned to read: the outputs are released and the timers at the capture
    /// rate, until `speak`.
    listening: bool,
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
    /// The sample rate to go back to after listening.
    sample_rate: Hertz,
}

impl<A: ChannelTimer, LA: Lanes<A>> Awg<A, A, LA, ()> {
    /// A generator on one timer.
    pub fn single(first: Pwm<A>, lanes: LA) -> Self {
        let rate = first.frequency();
        Self {
            first,
            second: None,
            first_lanes: lanes,
            second_lanes: (),
            start: Start::Single,
            state: Playing::default(),
            sample_rate: rate,
        }
    }
}

impl<A: ChannelTimer, B: ChannelTimer, LA: Lanes<A>, LB: Lanes<B>> Awg<A, B, LA, LB> {
    /// A generator on two timers at one period, the first starting the second
    /// through its trigger output, so every lane begins on one edge. Refuses
    /// timers at different periods, with both timers and both lane groups
    /// handed back.
    pub fn linked(
        first: Pwm<A>,
        second: Pwm<B>,
        first_lanes: LA,
        second_lanes: LB,
    ) -> Result<Self, AwgRefused<A, B, LA, LB>>
    where
        B: TriggeredBy<A>,
    {
        if first.period() != second.period() {
            return Err(Refused::new(
                ConfigError::PeriodMismatch,
                (first, second, first_lanes, second_lanes),
            ));
        }
        first.lead();
        second.follow(&first);
        let rate = first.frequency();
        Ok(Self {
            first,
            second: Some(second),
            first_lanes,
            second_lanes,
            start: Start::Linked,
            state: Playing::default(),
            sample_rate: rate,
        })
    }

    /// A generator on two timers at one period, started in software one after
    /// the other, for timers with no trigger route between them. The second
    /// begins a few instructions after the first. Refuses timers at different
    /// periods, with both timers and both lane groups handed back.
    pub fn independent(
        first: Pwm<A>,
        second: Pwm<B>,
        first_lanes: LA,
        second_lanes: LB,
    ) -> Result<Self, AwgRefused<A, B, LA, LB>> {
        if first.period() != second.period() {
            return Err(Refused::new(
                ConfigError::PeriodMismatch,
                (first, second, first_lanes, second_lanes),
            ));
        }
        let rate = first.frequency();
        Ok(Self {
            first,
            second: Some(second),
            first_lanes,
            second_lanes,
            start: Start::Independent,
            state: Playing::default(),
            sample_rate: rate,
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
        LA::set_outputs(&self.first, on);
        if let Some(second) = self.second.as_ref() {
            LB::set_outputs(second, on);
        }
    }

    /// Makes every lane's output active low, or active high. Active low, a line
    /// idles high: duty 0 holds it high, and a sample's pulse pulls it low.
    pub fn set_active_low(&mut self, low: bool) {
        LA::set_active_low(&self.first, low);
        if let Some(second) = self.second.as_ref() {
            LB::set_active_low(second, low);
        }
    }

    /// Whether a waveform is loaded or playing.
    pub const fn is_busy(&self) -> bool {
        self.state.active != 0
    }

    /// Loads `samples` on `lane`, for the next [`play`](Self::play). The lane's
    /// first sample is its duty from the first period on.
    pub fn load(&mut self, lane: usize, samples: &[u32]) -> Result<(), Error> {
        if self.state.started || self.state.listening {
            return Err(Error::Busy);
        }
        let bit = match u32::try_from(lane) {
            Ok(index) if lane < self.lanes() => 1u8 << (index & 7),
            _ => return Err(Error::Empty),
        };
        if lane < LA::COUNT {
            self.first_lanes.arm(lane, &self.first, samples)?;
        } else if let Some(second) = self.second.as_ref() {
            self.second_lanes.arm(lane - LA::COUNT, second, samples)?;
        } else {
            return Err(Error::Empty);
        }
        self.state.active |= bit;
        Ok(())
    }

    /// Plays every loaded lane, from one start. Refused while one plays, or with
    /// nothing loaded.
    pub fn play(&mut self) -> Result<(), Error> {
        if self.state.started || self.state.listening {
            return Err(Error::Busy);
        }
        if self.state.active == 0 {
            return Err(Error::Empty);
        }
        self.begin();
        Ok(())
    }

    /// Opens the active lanes' requests and starts the timers, all from one start.
    fn begin(&mut self) {
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
    }

    /// Releases every output and reads every lane's port `count` times at `rate`,
    /// or as near as the clock allows, which it returns. Every lane has to be
    /// two-way. Refused while a waveform is loaded or plays. Serve it through
    /// [`on_interrupt`](Self::on_interrupt), read it through
    /// [`samples`](Self::samples), and [`speak`](Self::speak) to send again.
    pub fn listen(&mut self, rate: Hertz, count: usize) -> Result<Hertz, Error> {
        if self.state.started || self.state.active != 0 || self.state.listening {
            return Err(Error::Busy);
        }
        self.sample_rate = self.first.frequency();
        self.set_outputs(false);
        self.state.listening = true;
        let got = match self.retime(rate) {
            Ok(got) => got,
            Err(error) => {
                self.abort();
                return Err(error);
            }
        };
        // A compare event once a period, wherever in the period it falls.
        let first = self
            .first_lanes
            .listen(&self.first, count, self.first.period() / 2);
        let listening = match self.second.as_ref() {
            Some(second) => {
                let duty = second.period() / 2;
                first.and_then(|()| self.second_lanes.listen(second, count, duty))
            }
            None => first,
        };
        if let Err(error) = listening {
            self.abort();
            return Err(error);
        }
        let all = self.lanes();
        let mut lane = 0;
        while lane < all {
            self.state.active |= 1 << (lane & 7);
            lane += 1;
        }
        self.begin();
        Ok(got)
    }

    fn retime(&mut self, rate: Hertz) -> Result<Hertz, Error> {
        let got = self.first.set_frequency(rate).map_err(|_| Error::Rate)?;
        if let Some(second) = self.second.as_mut() {
            second.set_frequency(rate).map_err(|_| Error::Rate)?;
        }
        Ok(got)
    }

    /// What `lane` last read, as whole port samples; see [`recover`].
    pub fn samples(&self, lane: usize) -> &[u16] {
        if lane < LA::COUNT {
            self.first_lanes.samples(lane)
        } else {
            self.second_lanes.samples(lane.wrapping_sub(LA::COUNT))
        }
    }

    /// Turns back from listening: the sample rate again, every output on at its
    /// inactive level. Refused while still reading.
    pub fn speak(&mut self) -> Result<(), Error> {
        if self.state.started {
            return Err(Error::Busy);
        }
        self.state.listening = false;
        let rate = self.sample_rate;
        self.first_lanes.park(&self.first);
        if let Some(second) = self.second.as_ref() {
            self.second_lanes.park(second);
        }
        self.retime(rate)?;
        self.set_outputs(true);
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
        self.first_lanes.abort_all(&self.first);
        if let Some(second) = self.second.as_ref() {
            self.second_lanes.abort_all(second);
        }
        self.halt();
        self.state.active = 0;
        if self.state.listening {
            self.state.listening = false;
            let rate = self.sample_rate;
            let _ = self.retime(rate);
        }
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

/// Recovers samples from what a lane or sampler read: bit `pin` of each of
/// `captured`, taken `captures_per_sample16` / 16 times per sample of the waveform,
/// with `full_scale` a sample's full scale. Fills `out` and returns how many
/// samples it found, up to the last with any pulse.
///
/// Time runs from the first active edge, so a waveform has to start with a pulse.
/// Each sample is how much of its period was active, and a sample of 0 between
/// others reads as 0. Accuracy is one capture in each period.
pub fn recover(
    captured: &[u16],
    pin: u8,
    active_low: bool,
    captures_per_sample16: u32,
    full_scale: u32,
    out: &mut [u32],
) -> usize {
    let Some(mask) = 1u16.checked_shl(u32::from(pin)) else {
        return 0;
    };
    if captures_per_sample16 == 0 {
        return 0;
    }
    let active = |sample: &u16| (sample & mask != 0) != active_low;
    // The line may already be active as the capture starts: skip to an inactive
    // capture, then to the first active one, so time starts on an edge.
    let quiet = captured
        .iter()
        .position(|sample| !active(sample))
        .unwrap_or(captured.len());
    let rest = captured.get(quiet..).unwrap_or(&[]);
    let Some(rise) = rest.iter().position(active) else {
        return 0;
    };
    let waveform = rest.get(rise..).unwrap_or(&[]);
    let per_sample = u64::from(captures_per_sample16);
    let mut found = 0;
    for (index, slot) in out.iter_mut().enumerate() {
        let begin = ((index as u64) * per_sample / 16) as usize;
        let end = ((index as u64 + 1) * per_sample / 16) as usize;
        let Some(window) = waveform.get(begin..end) else {
            break;
        };
        let high = window.iter().filter(|sample| active(sample)).count() as u64;
        let duty = (high * 16 * u64::from(full_scale) + per_sample / 2) / per_sample;
        *slot = u32::try_from(duty).unwrap_or(u32::MAX);
        if high != 0 {
            found = index + 1;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{channel_bit, recover};

    #[test]
    fn each_channel_has_its_own_request_bit() {
        assert_eq!(channel_bit(0), 0);
        assert_eq!(channel_bit(1), 0b0001);
        assert_eq!(channel_bit(2), 0b0010);
        assert_eq!(channel_bit(3), 0b0100);
        assert_eq!(channel_bit(4), 0b1000);
    }

    /// `captured` for a waveform of `duties` out of 8 on bit 2, 8 captures a sample.
    fn captures(duties: &[u8], active_high: bool) -> ([u16; 96], usize) {
        let mut captured = [0u16; 96];
        let (on, off) = if active_high {
            (0b0100, 0)
        } else {
            (0, 0b0100)
        };
        captured.fill(off);
        let mut at = 4;
        for duty in duties {
            for capture in 0..8 {
                captured[at] = if capture < *duty { on } else { off };
                at += 1;
            }
        }
        (captured, at)
    }

    #[test]
    fn samples_come_back_as_sent() {
        let (captured, _) = captures(&[4, 2, 6, 7], true);
        let mut out = [0; 8];
        assert_eq!(recover(&captured, 2, false, 8 * 16, 800, &mut out), 4);
        assert_eq!(out[..4], [400, 200, 600, 700]);
    }

    #[test]
    fn a_zero_between_samples_reads_as_zero() {
        let (captured, _) = captures(&[4, 0, 6], true);
        let mut out = [9; 8];
        assert_eq!(recover(&captured, 2, false, 8 * 16, 800, &mut out), 3);
        assert_eq!(out[..3], [400, 0, 600]);
    }

    #[test]
    fn active_low_reads_the_low_part_as_the_pulse() {
        let (captured, _) = captures(&[4, 2, 6], false);
        let mut out = [0; 8];
        assert_eq!(recover(&captured, 2, true, 8 * 16, 800, &mut out), 3);
        assert_eq!(out[..3], [400, 200, 600]);
    }

    #[test]
    fn other_pins_change_nothing() {
        let (mut captured, _) = captures(&[4, 2], true);
        for (index, sample) in captured.iter_mut().enumerate() {
            *sample |= if index % 2 == 0 { 0b1011 } else { 0 };
        }
        let mut out = [0; 8];
        assert_eq!(recover(&captured, 2, false, 8 * 16, 800, &mut out), 2);
        assert_eq!(out[..2], [400, 200]);
    }

    #[test]
    fn a_line_active_at_the_start_waits_for_its_next_edge() {
        let mut captured = [0u16; 32];
        captured[..3].fill(0b0100);
        captured[8..12].fill(0b0100);
        let mut out = [0; 4];
        assert_eq!(recover(&captured, 2, false, 8 * 16, 800, &mut out), 1);
        assert_eq!(out[0], 400);
    }

    #[test]
    fn a_fraction_of_a_capture_per_sample_stays_in_step() {
        // 2.5 captures a sample: windows of 2, 3, 2, 3 captures.
        let captured = [0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0];
        let mut out = [0; 4];
        assert_eq!(recover(&captured, 2, false, 40, 100, &mut out), 4);
        assert_eq!(out, [80, 120, 80, 120]);
    }

    #[test]
    fn nothing_to_read_gives_nothing() {
        let mut out = [0; 4];
        assert_eq!(recover(&[0; 16], 2, false, 8 * 16, 800, &mut out), 0);
        assert_eq!(recover(&[], 2, false, 8 * 16, 800, &mut out), 0);
        let (captured, _) = captures(&[4], true);
        assert_eq!(recover(&captured, 16, false, 8 * 16, 800, &mut out), 0);
        assert_eq!(recover(&captured, 2, false, 0, 800, &mut out), 0);
    }

    #[test]
    fn it_stops_at_the_end_of_out() {
        let (captured, _) = captures(&[4, 2, 6, 7], true);
        let mut out = [0; 2];
        assert_eq!(recover(&captured, 2, false, 8 * 16, 800, &mut out), 2);
        assert_eq!(out, [400, 200]);
    }
}
