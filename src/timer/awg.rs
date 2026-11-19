//! The arbitrary waveform generator, [`Awg`]; its documentation is on the type,
//! since this module is private. Part of the timer module, for both IP versions.
//!
//! Requirements: AWG-001, AWG-002, AWG-003, AWG-004, AWG-005, AWG-006, AWG-007, AWG-008, AWG-009, AWG-010, AWG-011, AWG-012, AWG-013, AWG-014, AWG-015, AWG-016, AWG-017, AWG-018, AWG-019, AWG-020, AWG-021, API-002, RESOURCE-002

use super::pwm::{CompareLane, Error, Frame, FrameLimits, PreparedRate, Pwm, lane_ccer_bits};
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
    /// Every lane's CCER bits, ORed: (owned, send); see [`lane_ccer_bits`].
    fn ccer_bits(&self, active_low: bool) -> (u16, u16);
    /// Prepares lane `index`'s read; see [`CompareLane::prepare_listen`].
    fn prepare_listen(&mut self, index: usize, count: usize) -> Result<(), Error>;
    /// Starts lane `index`'s prepared read; see [`CompareLane::listen`].
    fn listen(&mut self, index: usize, pwm: &Pwm<T>, duty: u32) -> Result<(), Error>;
    /// Lane `index`'s input register, if two-way.
    fn port(&self, index: usize) -> Option<u32>;
    /// What lane `index` last read.
    fn samples(&self, index: usize) -> &[u16];
    /// Sets every lane's duty to 0, its inactive level.
    fn park(&mut self, pwm: &Pwm<T>);
    /// See [`CompareLane::arm_frame`].
    fn arm_frame(
        &mut self,
        index: usize,
        pwm: &Pwm<T>,
        frame: Frame,
    ) -> Result<(), Refused<Error, Frame>>;
    /// See [`CompareLane::take_frame`].
    fn take_frame(&mut self, index: usize) -> Option<Frame>;
    /// Turns lane `index`'s end-of-transfer
    /// interrupt on or off; its error interrupts stay on.
    fn set_complete_interrupt(&mut self, index: usize, on: bool);
    /// See [`CompareLane::lend_samples`].
    fn lend_samples(
        &mut self,
        index: usize,
        samples: &'static mut [u16],
    ) -> Result<(), Refused<Error, &'static mut [u16]>>;
    /// See [`CompareLane::take_samples`].
    fn take_samples(&mut self, index: usize) -> Option<&'static mut [u16]>;
    /// See [`CompareLane::is_prepared`].
    fn is_prepared(&self, index: usize) -> bool;
    /// See `CompareLane::ahead_target`.
    fn ahead_target(&mut self, index: usize, count: usize) -> Result<(u32, u16), Error>;
    /// See `CompareLane::target_at`.
    fn target_at(&self, index: usize) -> Option<u32>;
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
    fn ccer_bits(&self, _active_low: bool) -> (u16, u16) {
        (0, 0)
    }
    fn prepare_listen(&mut self, _index: usize, _count: usize) -> Result<(), Error> {
        Err(Error::Empty)
    }
    fn listen(&mut self, _index: usize, _pwm: &Pwm<T>, _duty: u32) -> Result<(), Error> {
        Err(Error::Empty)
    }
    fn port(&self, _index: usize) -> Option<u32> {
        None
    }
    fn samples(&self, _index: usize) -> &[u16] {
        &[]
    }
    fn park(&mut self, _pwm: &Pwm<T>) {}
    fn arm_frame(
        &mut self,
        _index: usize,
        _pwm: &Pwm<T>,
        frame: Frame,
    ) -> Result<(), Refused<Error, Frame>> {
        Err(Refused::new(Error::Empty, frame))
    }
    fn take_frame(&mut self, _index: usize) -> Option<Frame> {
        None
    }
    fn set_complete_interrupt(&mut self, _index: usize, _on: bool) {}
    fn lend_samples(
        &mut self,
        _index: usize,
        samples: &'static mut [u16],
    ) -> Result<(), Refused<Error, &'static mut [u16]>> {
        Err(Refused::new(Error::Empty, samples))
    }
    fn take_samples(&mut self, _index: usize) -> Option<&'static mut [u16]> {
        None
    }
    fn is_prepared(&self, _index: usize) -> bool {
        false
    }
    fn ahead_target(&mut self, _index: usize, _count: usize) -> Result<(u32, u16), Error> {
        Err(Error::Empty)
    }
    fn target_at(&self, _index: usize) -> Option<u32> {
        None
    }
}

macro_rules! lanes {
    ($count:literal: $($index:tt $C:ident $W:ident),+) => {
        impl<T: ChannelTimer, $(const $C: u8, $W),+> Lanes<T> for ($(CompareLane<T, $C, $W>,)+) {
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
                    $($index => self.$index.vectors().iter().next(),)+
                    _ => None,
                }
            }
            fn ccer_bits(&self, active_low: bool) -> (u16, u16) {
                let (mut owned, mut send) = (0, 0);
                $(
                    let (o, s) = lane_ccer_bits($C, T::BREAK, self.$index.is_complementary(), active_low);
                    owned |= o;
                    send |= s;
                )+
                (owned, send)
            }
            fn prepare_listen(&mut self, index: usize, count: usize) -> Result<(), Error> {
                match index {
                    $($index => self.$index.prepare_listen(count),)+
                    _ => Err(Error::Empty),
                }
            }
            fn listen(&mut self, index: usize, pwm: &Pwm<T>, duty: u32) -> Result<(), Error> {
                match index {
                    $($index => self.$index.listen(pwm, duty),)+
                    _ => Err(Error::Empty),
                }
            }
            fn port(&self, index: usize) -> Option<u32> {
                match index {
                    $($index => self.$index.port(),)+
                    _ => None,
                }
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
            fn arm_frame(&mut self, index: usize, pwm: &Pwm<T>, frame: Frame) -> Result<(), Refused<Error, Frame>> {
                match index {
                    $($index => self.$index.arm_frame(pwm, frame),)+
                    _ => Err(Refused::new(Error::Empty, frame)),
                }
            }
            fn take_frame(&mut self, index: usize) -> Option<Frame> {
                match index {
                    $($index => self.$index.take_frame(),)+
                    _ => None,
                }
            }
            fn set_complete_interrupt(&mut self, index: usize, on: bool) {
                match index {
                    $($index => self.$index.set_complete_interrupt(on),)+
                    _ => {}
                }
            }
            fn lend_samples(
                &mut self,
                index: usize,
                samples: &'static mut [u16],
            ) -> Result<(), Refused<Error, &'static mut [u16]>> {
                match index {
                    $($index => self.$index.lend_samples(samples),)+
                    _ => Err(Refused::new(Error::Empty, samples)),
                }
            }
            fn take_samples(&mut self, index: usize) -> Option<&'static mut [u16]> {
                match index {
                    $($index => self.$index.take_samples(),)+
                    _ => None,
                }
            }
            fn is_prepared(&self, index: usize) -> bool {
                match index {
                    $($index => self.$index.is_prepared(),)+
                    _ => false,
                }
            }
            fn ahead_target(&mut self, index: usize, count: usize) -> Result<(u32, u16), Error> {
                match index {
                    $($index => self.$index.ahead_target(count),)+
                    _ => Err(Error::Empty),
                }
            }
            fn target_at(&self, index: usize) -> Option<u32> {
                match index {
                    $($index => self.$index.target_at(),)+
                    _ => None,
                }
            }
        }
    };
}

lanes!(1: 0 A WA);
lanes!(2: 0 A WA, 1 B WB);
lanes!(3: 0 A WA, 1 B WB, 2 C WC);
lanes!(4: 0 A WA, 1 B WB, 2 C WC, 3 D WD);

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
    /// Readers on their own streams now reading, one bit
    /// per reader.
    ahead_active: u8,
    /// Readers on their own streams armed, one bit per reader, so the switch checks a mask instead of walking the readers.
    armed: u8,
    /// Listen from the run that sees the frame end, inside `on_interrupt`.
    listen_on_end: bool,
    /// The frame has ended with every reader the
    /// plan uses armed, and the timers were left running at the send rate,
    /// their requests closed, so `listen` retimes them without a stop.
    idling: bool,
}

/// How a reader on its own stream is paced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opening {
    /// The timer's update request.
    Update,
    /// A compare request of a channel no lane uses, numbered from 1.
    Compare(u8),
    /// The update request of a timer no lane uses,
    /// kept running at the capture rate (`Awg::read_ahead_paced`).
    Paced,
}

/// A reader on a stream of its own, armed (enabled,
/// its request closed) while the frame plays, so the switch to listening
/// opens a request instead of starting a stream. One per timer and port it
/// reads.
struct AheadReader {
    dma: crate::dma::AnyStream,
    /// The stream's controller and number: its token, which
    /// [`Awg::free`] gives back through [`ReaderStreams`].
    id: (u8, u8),
    request: u8,
    /// The port's input register.
    port: u32,
    /// On the second timer.
    second: bool,
    opening: Opening,
    /// The transfer `prepare_listen` computed, and the address it writes.
    prepared: Option<crate::dma::Prepared>,
    prepared_at: u32,
    /// The lane whose memory it reads into.
    lane: u8,
    /// Enabled, its request closed, waiting for the switch.
    armed: bool,
}

impl AheadReader {
    /// Stops `stream` and keeps it as its registers and its id. Refuses with
    /// the stream if it does not stop.
    fn new<S: crate::dma::StreamInstance>(
        stream: S,
        request: u8,
        port: u32,
        second: bool,
        opening: Opening,
    ) -> Result<Self, Refused<Error, S>> {
        let mut dma = crate::dma::claim(&stream);
        if dma.stop(crate::dma::STOP_SPINS).is_err() {
            return Err(Refused::new(Error::Dma, stream));
        }
        dma.clear_flags(crate::dma::Flags::ALL);
        // The token becomes its id: `Awg::free` mints it back once, through
        // `ReaderStreams::take`, so the stream has one owner throughout.
        drop(stream);
        Ok(Self {
            dma,
            id: S::ID,
            request,
            port,
            second,
            opening,
            prepared: None,
            prepared_at: 0,
            lane: 0,
            armed: false,
        })
    }

    /// Stops the stream, whatever it was doing.
    fn disarm(&mut self) {
        let _ = self.dma.stop(crate::dma::STOP_SPINS);
        self.dma.clear_flags(crate::dma::Flags::ALL);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.armed = false;
    }
}

const AHEAD: usize = 4;

/// The DMA streams a generator's readers held, handed back by
/// [`Awg::free`]: each one taken out once, as the token it was given as.
#[derive(Debug, PartialEq, Eq)]
pub struct ReaderStreams {
    ids: [Option<(u8, u8)>; AHEAD],
}

impl ReaderStreams {
    /// Stream `N` of DMA controller `D`, if a reader held it and it has not
    /// been taken yet.
    pub fn take<const D: u8, const N: u8>(&mut self) -> Option<crate::dma::Stream<D, N>>
    where
        crate::dma::Dma<D>: crate::dma::DmaInstance,
    {
        let slot = self.ids.iter_mut().find(|id| **id == Some((D, N)))?;
        *slot = None;
        Some(crate::dma::Stream::new())
    }

    /// Whether every stream has been taken.
    pub fn is_empty(&self) -> bool {
        self.ids.iter().all(Option::is_none)
    }
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

/// An arbitrary waveform generator: compare lanes on one or two timers, each
/// playing its own sequence of samples, all from one start. Part of the timer
/// module, for both IP versions.
///
/// A sample is a duty in timer counts, held for one period of the timer, so the
/// sample rate is the timer's rate and full scale is [`Awg::period`]. A waveform
/// ends at duty 0: the lane appends it, so the output falls and stays low. What
/// the samples mean is the caller's. A protocol encodes its frames as samples.
///
/// Lanes built two-way read as well. [`Awg::listen`] releases the outputs, runs
/// the timers at the capture rate and has each lane's stream copy its port's input
/// register into memory, paced by its own channel's compare request, so a lane can
/// answer on the pin it sends on. [`Awg::speak`] turns them back. [`recover`] turns
/// what was read back into samples. Outputs can be active low, for a line that
/// idles high.
///
/// With two timers, either the first starts the second through its trigger
/// output, on one edge ([`Awg::linked`]), or both start in software, one after
/// the other, for timers with no trigger route between them
/// ([`Awg::independent`]). Both timers must run at one period, so a sample means
/// the same on every lane.
///
/// Lanes are numbered in the order they were given: the first timer's, then the
/// second's. The firmware binds each lane's stream vector, or polls, and calls
/// [`Awg::on_interrupt`] with that lane. FerroHAL enables nothing in the NVIC.
pub struct Awg<A: ChannelTimer, B: ChannelTimer, LA, LB> {
    first: Pwm<A>,
    second: Option<Pwm<B>>,
    first_lanes: LA,
    second_lanes: LB,
    start: Start,
    state: Playing,
    /// Active low, as last set.
    active_low: bool,
    /// Whether the outputs are on (CCER's send bits set).
    outputs_on: bool,
    /// Each timer's CCER bits for its lanes: (owned, send), per
    /// [`lane_ccer_bits`], for `active_low`.
    ccer: [(u16, u16); 2],
    /// What [`prepare_listen`](Awg::prepare_listen) computed.
    plan: Option<Plan>,
    /// Readers on their own streams.
    ahead: [Option<AheadReader>; AHEAD],
    /// The pacer's registers and its rate.
    pacer: Option<(super::pwm::Pacer, Hertz)>,
}

/// A listen computed beforehand, so the switch itself is register stores.
#[derive(Clone, Copy)]
struct Plan {
    /// Each timer's capture rate and the duty its compare event sits at.
    listen: [(PreparedRate, u32); 2],
    /// Each timer's send rate, to go back to.
    send: [PreparedRate; 2],
    /// Lanes whose own stream reads (bit n for lane n).
    readers: u8,
    /// For each lane, the lane whose read it shares.
    reader_of: [u8; 8],
    /// Readers on their own streams this plan uses, one
    /// bit per reader.
    ahead: u8,
    /// What the readers in `ahead` open on each timer:
    /// the update request, and compare requests (bit 0 = channel 1), whose
    /// compare registers take the listen duty.
    open: [(bool, u8); 2],
    /// A paced reader is in `ahead`.
    paced: bool,
    /// What was asked for: the rate and the count.
    asked: (Hertz, usize),
}

impl<A: ChannelTimer, LA: Lanes<A>> Awg<A, A, LA, ()> {
    /// A generator on one timer.
    pub fn single(first: Pwm<A>, lanes: LA) -> Self {
        let mut awg = Self {
            first,
            second: None,
            first_lanes: lanes,
            second_lanes: (),
            start: Start::Single,
            state: Playing::default(),
            active_low: false,
            outputs_on: true,
            ccer: [(0, 0); 2],
            plan: None,
            ahead: [const { None }; AHEAD],
            pacer: None,
        };
        awg.refresh_ccer();
        awg
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
        let mut awg = Self {
            first,
            second: Some(second),
            first_lanes,
            second_lanes,
            start: Start::Linked,
            state: Playing::default(),
            active_low: false,
            outputs_on: true,
            ccer: [(0, 0); 2],
            plan: None,
            ahead: [const { None }; AHEAD],
            pacer: None,
        };
        awg.refresh_ccer();
        Ok(awg)
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
        let mut awg = Self {
            first,
            second: Some(second),
            first_lanes,
            second_lanes,
            start: Start::Independent,
            state: Playing::default(),
            active_low: false,
            outputs_on: true,
            ccer: [(0, 0); 2],
            plan: None,
            ahead: [const { None }; AHEAD],
            pacer: None,
        };
        awg.refresh_ccer();
        Ok(awg)
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
        self.first.rate()
    }

    /// Turns every lane's output on or off, each timer's in one CCER store.
    /// Off, a pin is released (not driven by the timer), with its polarity
    /// bits cleared as RM0090 Table 95 note 1 (p. 577) and RM0433 Table 337
    /// note 1 (p. 1608) require; on, it drives at its polarity.
    pub fn set_outputs(&mut self, on: bool) {
        let [(owned, send), (owned2, send2)] = self.ccer;
        self.first.write_ccer(owned, if on { send } else { 0 });
        if let Some(second) = self.second.as_ref() {
            second.write_ccer(owned2, if on { send2 } else { 0 });
        }
        self.outputs_on = on;
    }

    /// Makes every lane's output active low, or active high. Active low, a line
    /// idles high: duty 0 holds it high, and a sample's pulse pulls it low.
    /// With the outputs off it takes effect when they turn on.
    pub fn set_active_low(&mut self, low: bool) {
        self.active_low = low;
        self.refresh_ccer();
        if self.outputs_on {
            self.set_outputs(true);
        }
    }

    fn refresh_ccer(&mut self) {
        self.ccer = [
            self.first_lanes.ccer_bits(self.active_low),
            self.second_lanes.ccer_bits(self.active_low),
        ];
    }

    /// Whether a waveform is loaded or playing.
    pub const fn is_busy(&self) -> bool {
        self.state.active != 0 || self.state.ahead_active != 0
    }

    /// Loads `samples` on `lane`, for the next [`play`](Self::play). The lane's
    /// first sample is its duty from the first period on.
    pub fn load(&mut self, lane: usize, samples: &[u32]) -> Result<(), Error> {
        if self.state.started || self.state.listening {
            return Err(Error::Busy);
        }
        // Idling timers would latch a loaded first duty before `play`.
        if self.state.idling {
            self.halt();
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

    /// Reads port `P` for the first timer's lanes on it
    /// through `stream`, paced by the first timer's update request, armed
    /// while the frame plays. Refused if a reader already reads that port on
    /// that timer, or with no room.
    pub fn read_ahead_first_update<S, const P: char>(
        &mut self,
        stream: S,
    ) -> Result<(), Refused<Error, S>>
    where
        S: super::UpdateDma<A> + super::GpioDma,
        crate::gpio::Port<P>: crate::gpio::PortInstance,
    {
        let port = Self::idr::<P>();
        self.add_ahead(stream, S::REQUEST, port, false, Opening::Update)
    }

    /// As [`read_ahead_first_update`](Self::read_ahead_first_update),
    /// on the second timer.
    pub fn read_ahead_second_update<S, const P: char>(
        &mut self,
        stream: S,
    ) -> Result<(), Refused<Error, S>>
    where
        S: super::UpdateDma<B> + super::GpioDma,
        crate::gpio::Port<P>: crate::gpio::PortInstance,
    {
        let port = Self::idr::<P>();
        self.add_ahead(stream, S::REQUEST, port, true, Opening::Update)
    }

    /// As [`read_ahead_first_update`](Self::read_ahead_first_update),
    /// paced by the compare request of the first timer's channel `C`, which no
    /// lane may use. For a second port on a timer, since one update request
    /// reads one port.
    pub fn read_ahead_first_compare<const C: u8, S, const P: char>(
        &mut self,
        stream: S,
    ) -> Result<(), Refused<Error, S>>
    where
        S: super::CompareDma<A, C> + super::GpioDma,
        crate::gpio::Port<P>: crate::gpio::PortInstance,
    {
        if (0..LA::COUNT).any(|lane| LA::channel(lane) == C) {
            return Err(Refused::new(Error::Busy, stream));
        }
        let port = Self::idr::<P>();
        self.add_ahead(stream, S::REQUEST, port, false, Opening::Compare(C))
    }

    /// As [`read_ahead_first_compare`](Self::read_ahead_first_compare),
    /// on the second timer.
    pub fn read_ahead_second_compare<const C: u8, S, const P: char>(
        &mut self,
        stream: S,
    ) -> Result<(), Refused<Error, S>>
    where
        S: super::CompareDma<B, C> + super::GpioDma,
        crate::gpio::Port<P>: crate::gpio::PortInstance,
    {
        if (0..LB::COUNT).any(|lane| LB::channel(lane) == C) {
            return Err(Refused::new(Error::Busy, stream));
        }
        let port = Self::idr::<P>();
        self.add_ahead(stream, S::REQUEST, port, true, Opening::Compare(C))
    }

    fn idr<const P: char>() -> u32
    where
        crate::gpio::Port<P>: crate::gpio::PortInstance,
    {
        use crate::gpio::PortRegisters;
        <crate::gpio::Port<P> as PortRegisters>::regs()
            .idr()
            .as_ptr() as u32
    }

    /// A reader for port `P` paced by `pacer`'s update
    /// request, for lanes whose streams cannot reach GPIO. One reader serves
    /// every lane on the port, on either timer. `pacer` must run at the rate
    /// `prepare_listen` will ask for, its update interrupt off; the caller
    /// keeps it and keeps it running. Its request opens only at the switch.
    pub fn read_ahead_paced<T: super::ChannelTimer, S, const P: char>(
        &mut self,
        pacer: &Pwm<T>,
        stream: S,
    ) -> Result<(), Refused<Error, S>>
    where
        S: super::UpdateDma<T> + super::GpioDma,
        crate::gpio::Port<P>: crate::gpio::PortInstance,
    {
        if self.pacer.is_some() {
            return Err(Refused::new(Error::Busy, stream));
        }
        let port = Self::idr::<P>();
        self.add_ahead(stream, S::REQUEST, port, false, Opening::Paced)?;
        let handle = pacer.pacer();
        handle.close();
        self.pacer = Some((handle, pacer.rate()));
        Ok(())
    }

    /// The paced reader's port, if one is registered.
    fn paced_port(&self) -> Option<u32> {
        self.ahead
            .iter()
            .flatten()
            .find(|r| r.opening == Opening::Paced)
            .map(|r| r.port)
    }

    /// Opens or closes the pacer's update request,
    /// clearing its flag first when opening.
    fn pace(&self, open: bool) {
        if let Some((handle, _)) = self.pacer {
            handle.open(open);
        }
    }

    /// Registers a reader on `stream`, refusing with the stream while a
    /// waveform plays or is read, if a reader already reads that port on
    /// that timer (or that timer's update request), with no room, or if the
    /// stream does not stop.
    fn add_ahead<S: crate::dma::StreamInstance>(
        &mut self,
        stream: S,
        request: u8,
        port: u32,
        second: bool,
        opening: Opening,
    ) -> Result<(), Refused<Error, S>> {
        if self.state.started || self.state.listening {
            return Err(Refused::new(Error::Busy, stream));
        }
        let taken = self.ahead.iter().flatten().any(|r| {
            r.second == second
                && (r.port == port
                    || matches!((r.opening, opening), (Opening::Update, Opening::Update)))
        });
        if taken {
            return Err(Refused::new(Error::Busy, stream));
        }
        let Some(slot) = self.ahead.iter_mut().find(|slot| slot.is_none()) else {
            return Err(Refused::new(Error::TooLarge, stream));
        };
        *slot = Some(AheadReader::new(stream, request, port, second, opening)?);
        self.plan = None;
        Ok(())
    }

    /// The reader on its own stream for `port` on the
    /// first (`second` false) or second timer, if any.
    fn ahead_for(&self, second: bool, port: u32) -> Option<usize> {
        self.ahead.iter().position(
            |r| matches!(r, Some(r) if r.port == port && (r.second == second || Self::paced(r))),
        )
    }

    fn paced(r: &AheadReader) -> bool {
        r.opening == Opening::Paced
    }

    /// Enables every reader this plan uses that is not
    /// yet waiting, its request closed, if its lane still holds the memory
    /// it was prepared for. Runs before the frame starts, outside the
    /// switch.
    fn arm_ahead(&mut self) {
        let Some(plan) = self.plan else {
            return;
        };
        for (index, slot) in self.ahead.iter_mut().enumerate() {
            if plan.ahead & (1 << (index & 7)) == 0 {
                continue;
            }
            let Some(reader) = slot.as_mut() else {
                continue;
            };
            if reader.armed && reader.dma.is_running() {
                continue;
            }
            let lane = usize::from(reader.lane);
            let at = if lane < LA::COUNT {
                self.first_lanes.target_at(lane)
            } else {
                self.second_lanes.target_at(lane - LA::COUNT)
            };
            reader.armed = false;
            self.state.armed &= !(1 << (index & 7));
            let Some(prepared) = reader.prepared else {
                continue;
            };
            if at != Some(reader.prepared_at) {
                continue;
            }
            // SAFETY: the memory is the lane's own or lent to it, and
            // `take_samples` refuses while this reader is armed or running;
            // it holds the prepared count, as `ahead_target` checked; the
            // address is a GPIO port's input register, read-only and without
            // side effects, which this stream's request paces once opened.
            if unsafe { reader.dma.start_prepared(&prepared) }.is_ok() {
                reader.armed = true;
                self.state.armed |= 1 << (index & 7);
            }
        }
    }

    /// What a frame for `lane` is checked against.
    pub fn frame_limits(&self, lane: usize) -> Option<FrameLimits> {
        if lane < LA::COUNT {
            Some(self.first.frame_limits())
        } else if lane < self.lanes() {
            self.second.as_ref().map(Pwm::frame_limits)
        } else {
            None
        }
    }

    /// Loads a checked frame on `lane`, played in
    /// place, for the next [`play`](Self::play). As [`load`](Self::load),
    /// refusing with the frame.
    pub fn load_frame(&mut self, lane: usize, frame: Frame) -> Result<(), Refused<Error, Frame>> {
        super::start_mark!(begin);
        if self.state.started || self.state.listening {
            return Err(Refused::new(Error::Busy, frame));
        }
        // Idling timers would latch a loaded first duty before `play`.
        if self.state.idling {
            self.halt();
        }
        let bit = match u32::try_from(lane) {
            Ok(index) if lane < self.lanes() => 1u8 << (index & 7),
            _ => return Err(Refused::new(Error::Empty, frame)),
        };
        if lane < LA::COUNT {
            self.first_lanes.arm_frame(lane, &self.first, frame)?;
        } else if let Some(second) = self.second.as_ref() {
            self.second_lanes
                .arm_frame(lane - LA::COUNT, second, frame)?;
        } else {
            return Err(Refused::new(Error::Empty, frame));
        }
        self.state.active |= bit;
        super::start_mark!(4);
        super::start_mark!(end);
        Ok(())
    }

    /// `lane`'s frame back, once it has ended.
    pub fn take_frame(&mut self, lane: usize) -> Option<Frame> {
        if lane < LA::COUNT {
            self.first_lanes.take_frame(lane)
        } else {
            self.second_lanes.take_frame(lane.wrapping_sub(LA::COUNT))
        }
    }

    /// Lends `lane` samples memory to read into; see
    /// [`CompareLane::lend_samples`]. With a shared reader only a reader
    /// lane's memory is written ([`reader_of`](Self::reader_of)).
    pub fn lend_samples(
        &mut self,
        lane: usize,
        samples: &'static mut [u16],
    ) -> Result<(), Refused<Error, &'static mut [u16]>> {
        if self.state.listening {
            return Err(Refused::new(Error::Busy, samples));
        }
        if lane < LA::COUNT {
            self.first_lanes.lend_samples(lane, samples)
        } else {
            self.second_lanes
                .lend_samples(lane.wrapping_sub(LA::COUNT), samples)
        }
    }

    /// `lane`'s lent samples back, once it has stopped.
    pub fn take_samples(&mut self, lane: usize) -> Option<&'static mut [u16]> {
        if self
            .ahead
            .iter()
            .flatten()
            .any(|r| usize::from(r.lane) == lane && (r.armed || r.dma.is_running()))
        {
            return None;
        }
        if lane < LA::COUNT {
            self.first_lanes.take_samples(lane)
        } else {
            self.second_lanes.take_samples(lane.wrapping_sub(LA::COUNT))
        }
    }

    /// The lane whose read `lane` shares, as the last
    /// [`prepare_listen`](Self::prepare_listen) set; `lane` itself without
    /// one.
    pub fn reader_of(&self, lane: usize) -> usize {
        match &self.plan {
            Some(plan) => usize::from(plan.reader_of.get(lane).copied().unwrap_or(0)),
            None => lane,
        }
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
        super::start_mark!(begin);
        self.arm_ahead();
        self.quiet_all_but_last();
        super::start_mark!(5);
        self.begin();
        super::start_mark!(end);
        Ok(())
    }

    /// Only the last active lane (the highest numbered: the second timer starts from the
    /// first's trigger, after it) raises its end-of-transfer interrupt; the
    /// run it starts serves every lane. A lane still running then gets its
    /// interrupt back (`on_interrupt`), so frames of unequal length still end.
    fn quiet_all_but_last(&mut self) {
        let active = self.state.active;
        if active == 0 {
            return;
        }
        let last = 7 - active.leading_zeros() as usize;
        for lane in 0..last {
            if active & (1 << lane) == 0 {
                continue;
            }
            if lane < LA::COUNT {
                self.first_lanes.set_complete_interrupt(lane, false);
            } else {
                self.second_lanes
                    .set_complete_interrupt(lane - LA::COUNT, false);
            }
        }
    }

    /// Listen from the run that sees the frame end, inside
    /// [`on_interrupt`](Self::on_interrupt), instead of from the caller after
    /// it returns. Returns whether it will, which it always does.
    pub fn listen_on_end(&mut self) -> bool {
        self.state.listen_on_end = true;
        self.state.listen_on_end
    }

    /// Whether it is turned to read.
    pub const fn is_listening(&self) -> bool {
        self.state.listening
    }

    /// Opens the active lanes' requests and starts the timers, all from one start.
    fn begin(&mut self) {
        let (first, second) = self.masks();
        super::start_mark!(6);
        self.first.stage(first);
        super::start_mark!(7);
        if let Some(timer) = self.second.as_mut() {
            timer.stage(second);
        }
        super::start_mark!(8);
        self.state.started = true;
        self.first.start();
        if self.start == Start::Independent
            && let Some(timer) = self.second.as_mut()
        {
            timer.start();
        }
        super::start_mark!(9);
    }

    /// `begin` for a listen with readers armed ahead:
    /// stages both timers with the lane readers' requests, then opens each
    /// armed reader's request, then starts. Refused if a reader the plan
    /// uses is not armed.
    fn begin_ahead(&mut self, plan: &Plan) -> Result<(), Error> {
        if self.state.armed & plan.ahead != plan.ahead {
            return Err(Error::NotPrepared);
        }
        self.set_ahead_compares(plan);
        let (first, second) = self.masks();
        self.first.stage(first);
        if let Some(timer) = self.second.as_mut() {
            timer.stage(second);
        }
        self.first.open_requests(plan.open[0].0, plan.open[0].1);
        if let Some(timer) = self.second.as_ref() {
            timer.open_requests(plan.open[1].0, plan.open[1].1);
        }
        if plan.paced {
            self.pace(true);
        }
        self.state.ahead_active = plan.ahead;
        self.state.started = true;
        self.first.start();
        if self.start == Start::Independent
            && let Some(timer) = self.second.as_mut()
        {
            timer.start();
        }
        Ok(())
    }

    /// The listen duty into the compare register of
    /// each channel a reader in the plan opens.
    fn set_ahead_compares(&self, plan: &Plan) {
        for index in 0..4 {
            if plan.open[0].1 & (1 << index) != 0 {
                self.first.set_compare(index, plan.listen[0].1);
            }
            if plan.open[1].1 & (1 << index) != 0
                && let Some(second) = self.second.as_ref()
            {
                second.set_compare(index, plan.listen[1].1);
            }
        }
    }

    /// At a frame's end with every reader the
    /// plan uses armed (none, stage G1), closes the lanes' compare requests and leaves the
    /// timers running at the send rate, every output at duty 0, the closing
    /// sample each frame ends with. Whether it did.
    fn idle_running(&mut self) -> bool {
        let Some(plan) = self.plan.as_ref() else {
            return false;
        };
        // With no reader ahead the lanes' own streams start at the switch,
        // their requests closed until `listen` opens them, so the timers can
        // idle as well.
        if self.state.listening || self.state.armed & plan.ahead != plan.ahead {
            return false;
        }
        self.first.close_requests();
        if let Some(second) = self.second.as_ref() {
            second.close_requests();
        }
        self.state.started = false;
        self.state.idling = true;
        true
    }

    /// `listen` from idling timers. Releases the
    /// outputs, starts the lanes' own reads and sets the readers' compare
    /// duties, then retimes each running timer by PSC, ARR and an update
    /// generation, which with URS counter-only raises no update request,
    /// clears its flags and opens the requests. No stop, stage or start.
    fn listen_running(&mut self, plan: &Plan) -> Result<(), Error> {
        self.state.idling = false;
        if self.state.armed & plan.ahead != plan.ahead {
            self.abort();
            return Err(Error::NotPrepared);
        }
        self.set_outputs(false);
        super::stamp!(super::profile::OUTPUTS);
        self.state.listening = true;
        super::stamp!(super::profile::RETIMED);
        if let Err(error) = self.listen_lanes(plan) {
            self.abort();
            return Err(error);
        }
        self.set_ahead_compares(plan);
        super::stamp!(super::profile::LANES);
        self.state.active = plan.readers;
        let (first, second) = self.masks();
        self.first.retime_running(&plan.listen[0].0);
        if let Some(timer) = self.second.as_mut() {
            timer.retime_running(&plan.listen[1].0);
        }
        self.first
            .open_requests(plan.open[0].0, plan.open[0].1 | first);
        if let Some(timer) = self.second.as_ref() {
            timer.open_requests(plan.open[1].0, plan.open[1].1 | second);
        }
        if plan.paced {
            self.pace(true);
        }
        self.state.ahead_active = plan.ahead;
        self.state.started = true;
        super::stamp!(super::profile::BEGUN);
        #[cfg(feature = "profile-listen")]
        super::profile::commit_listen();
        Ok(())
    }

    /// Computes a listen of `count` port samples at `rate`, or as near as
    /// the clock allows, which it returns, so that [`listen`](Self::listen)
    /// and [`speak`](Self::speak) are register stores only. Every lane has to
    /// be two-way. Holds until the next `prepare_listen`; the send rate it
    /// returns to is the one the timers run at now. Refused while a
    /// waveform is loaded, plays or is being read.
    ///
    /// The lanes of one timer that read one port share one reader (a paced
    /// reader serves its port on both timers): only that reader's stream
    /// runs, and [`samples`](Self::samples) gives its read for each of them.
    pub fn prepare_listen(&mut self, rate: Hertz, count: usize) -> Result<Hertz, Error> {
        if self.state.started || self.state.active != 0 || self.state.listening {
            return Err(Error::Busy);
        }
        let first = self.first.prepare_rate(rate).map_err(|_| Error::Rate)?;
        let second = match self.second.as_ref() {
            Some(timer) => Some(timer.prepare_rate(rate).map_err(|_| Error::Rate)?),
            None => None,
        };
        let mut reader_of = [0u8; 8];
        let mut readers = 0u8;
        #[allow(unused_mut)]
        let mut ahead_mask = 0u8;
        #[allow(unused_mut)]
        let mut open = [(false, 0u8); 2];
        #[allow(unused_mut)]
        let mut paced = false;
        let paced_port = self.paced_port();
        let all = self.lanes().min(8);
        let mut lane = 0;
        while lane < all {
            let (group_start, port) = if lane < LA::COUNT {
                (0, self.first_lanes.port(lane))
            } else {
                (LA::COUNT, self.second_lanes.port(lane - LA::COUNT))
            };
            let Some(port) = port else {
                return Err(Error::NoInput);
            };
            let mut reader = lane;
            // A paced reader serves its port on both timers.
            let group_start = if paced_port == Some(port) {
                0
            } else {
                group_start
            };
            let mut other = group_start;
            while other < lane {
                let theirs = if other < LA::COUNT {
                    self.first_lanes.port(other)
                } else {
                    self.second_lanes.port(other - LA::COUNT)
                };
                if theirs == Some(port) {
                    reader = other;
                    break;
                }
                other += 1;
            }
            if let Some(slot) = reader_of.get_mut(lane) {
                *slot = reader as u8;
            }
            if reader == lane {
                let ahead = self.ahead_for(lane >= LA::COUNT, port);
                match ahead {
                    Some(index) => {
                        let (at, len) = if lane < LA::COUNT {
                            self.first_lanes.ahead_target(lane, count)?
                        } else {
                            self.second_lanes.ahead_target(lane - LA::COUNT, count)?
                        };
                        if let Some(Some(r)) = self.ahead.get_mut(index) {
                            // A reader still armed for other memory stops first.
                            if r.armed || r.dma.is_running() {
                                r.disarm();
                                self.state.armed &= !(1 << (index & 7));
                            }
                            let t = usize::from(r.second) & 1;
                            match r.opening {
                                Opening::Update => open[t].0 = true,
                                Opening::Compare(c) => open[t].1 |= 1 << (c.wrapping_sub(1) & 3),
                                Opening::Paced => paced = true,
                            }
                            let transfer = crate::dma::Transfer::one_shot(
                                r.request,
                                crate::dma::Direction::PeripheralToMemory,
                                r.port,
                                at,
                                len,
                                crate::dma::WordSize::HalfWord,
                                crate::dma::Priority::VeryHigh,
                            );
                            r.prepared =
                                Some(crate::dma::Prepared::new(&transfer).map_err(|_| Error::Dma)?);
                            r.prepared_at = at;
                            r.lane = lane as u8;
                        }
                        ahead_mask |= 1 << (index & 7);
                    }
                    _ => {
                        if lane < LA::COUNT {
                            self.first_lanes.prepare_listen(lane, count)?;
                        } else {
                            self.second_lanes.prepare_listen(lane - LA::COUNT, count)?;
                        }
                        readers |= 1 << (lane & 7);
                    }
                }
            }
            lane += 1;
        }
        let send_first = self.first.current_rate();
        let send_second = self
            .second
            .as_ref()
            .map_or(send_first, |t| t.current_rate());
        let second = second.unwrap_or(first);
        self.plan = Some(Plan {
            // A compare event once a period, wherever in the period it falls.
            listen: [(first, first.period() / 2), (second, second.period() / 2)],
            send: [send_first, send_second],
            readers,
            reader_of,
            ahead: ahead_mask,
            open,
            paced,
            asked: (rate, count),
        });
        // A paced reader samples at its pacer's rate, whatever the lane
        // timers can reach.
        if paced && let Some((_, at)) = self.pacer {
            return Ok(at);
        }
        Ok(first.rate())
    }

    /// The rate and count the last [`prepare_listen`](Self::prepare_listen)
    /// was asked for, and the rate it gives, if any.
    pub fn prepared(&self) -> Option<(Hertz, usize, Hertz)> {
        let plan = self.plan.as_ref()?;
        let mut lane = 0;
        while lane < self.lanes().min(8) {
            if plan.readers & (1 << (lane & 7)) != 0 {
                let ready = if lane < LA::COUNT {
                    self.first_lanes.is_prepared(lane)
                } else {
                    self.second_lanes.is_prepared(lane - LA::COUNT)
                };
                // Prepared into memory the lane no longer reads into.
                if !ready {
                    return None;
                }
            }
            lane += 1;
        }
        for (index, slot) in self.ahead.iter().enumerate() {
            if plan.ahead & (1 << (index & 7)) == 0 {
                continue;
            }
            let r = slot.as_ref()?;
            let lane = usize::from(r.lane);
            let at = if lane < LA::COUNT {
                self.first_lanes.target_at(lane)
            } else {
                self.second_lanes.target_at(lane - LA::COUNT)
            };
            if at != Some(r.prepared_at) {
                return None;
            }
        }
        if plan.paced
            && let Some((_, at)) = self.pacer
        {
            return Some((plan.asked.0, plan.asked.1, at));
        }
        Some((plan.asked.0, plan.asked.1, plan.listen[0].0.rate()))
    }

    /// Releases every output and starts the prepared read
    /// ([`prepare_listen`](Self::prepare_listen)). Refused while a waveform
    /// is loaded or plays, or with nothing prepared. Serve it through
    /// [`on_interrupt`](Self::on_interrupt), read it through
    /// [`samples`](Self::samples), and [`speak`](Self::speak) to send again.
    pub fn listen(&mut self) -> Result<(), Error> {
        if self.state.started || self.state.active != 0 || self.state.listening {
            return Err(Error::Busy);
        }
        let Some(plan) = self.plan else {
            return Err(Error::NotPrepared);
        };
        super::stamp!(super::profile::ENTRY);
        super::stamp!(super::profile::ENTRY2);
        if self.state.idling {
            return self.listen_running(&plan);
        }
        self.set_outputs(false);
        super::stamp!(super::profile::OUTPUTS);
        self.state.listening = true;
        self.first.apply_rate(&plan.listen[0].0);
        if let Some(second) = self.second.as_mut() {
            second.apply_rate(&plan.listen[1].0);
        }
        super::stamp!(super::profile::RETIMED);
        if let Err(error) = self.listen_lanes(&plan) {
            self.abort();
            return Err(error);
        }
        super::stamp!(super::profile::LANES);
        self.state.active = plan.readers;
        if plan.ahead != 0 {
            if let Err(error) = self.begin_ahead(&plan) {
                self.abort();
                return Err(error);
            }
            super::stamp!(super::profile::BEGUN);
            #[cfg(feature = "profile-listen")]
            super::profile::commit_listen();
            return Ok(());
        }
        self.begin();
        super::stamp!(super::profile::BEGUN);
        #[cfg(feature = "profile-listen")]
        super::profile::commit_listen();
        Ok(())
    }

    /// Starts the read of each lane in `plan` that reads on its own stream.
    fn listen_lanes(&mut self, plan: &Plan) -> Result<(), Error> {
        let all = self.lanes().min(8);
        let mut lane = 0;
        while lane < all {
            if plan.readers & (1 << (lane & 7)) != 0 {
                #[cfg(feature = "profile-lanes")]
                super::profile::set_lane(lane);
                let started = if lane < LA::COUNT {
                    self.first_lanes.listen(lane, &self.first, plan.listen[0].1)
                } else if let Some(second) = self.second.as_ref() {
                    self.second_lanes
                        .listen(lane - LA::COUNT, second, plan.listen[1].1)
                } else {
                    Err(Error::Empty)
                };
                started?;
            }
            lane += 1;
        }
        Ok(())
    }

    /// What `lane` last read, as whole port samples; see [`recover`]. With a
    /// shared reader, the capture of the lane whose read it shares.
    pub fn samples(&self, lane: usize) -> &[u16] {
        if self.state.ahead_active != 0 {
            return &[];
        }
        let lane = match &self.plan {
            Some(plan) => usize::from(plan.reader_of.get(lane).copied().unwrap_or(0)),
            None => lane,
        };
        if lane < LA::COUNT {
            self.first_lanes.samples(lane)
        } else {
            self.second_lanes.samples(lane.wrapping_sub(LA::COUNT))
        }
    }

    /// Turns back from listening: the send rate again, every output on at its
    /// inactive level. Refused while still reading.
    pub fn speak(&mut self) -> Result<(), Error> {
        if self.state.started {
            return Err(Error::Busy);
        }
        super::stamp!(super::profile::S_ENTRY);
        self.state.listening = false;
        self.first_lanes.park(&self.first);
        if let Some(second) = self.second.as_ref() {
            self.second_lanes.park(second);
        }
        super::stamp!(super::profile::S_PARKED);
        if let Some(plan) = self.plan {
            self.first.apply_rate(&plan.send[0]);
            if let Some(second) = self.second.as_mut() {
                second.apply_rate(&plan.send[1]);
            }
        }
        super::stamp!(super::profile::S_RETIMED);
        self.set_outputs(true);
        super::stamp!(super::profile::S_DONE);
        #[cfg(feature = "profile-listen")]
        super::profile::commit_speak();
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

    /// Serves every lane's stream, from any lane's vector or from a task:
    /// `Some(Ok(ended))` once one or more waveforms have ended, bit `n` of
    /// `ended` set for lane `n`; `None` if none did. When the last lane
    /// ends, the timers stop and their requests close. A lane that fails
    /// stops the whole generator, since its waveforms belong together.
    pub fn on_interrupt(&mut self) -> Option<Result<u8, Error>> {
        #[cfg(feature = "profile-start")]
        let t0 = cortex_m::peripheral::DWT::cycle_count();
        let mut ended = 0u8;
        let mut lane = 0;
        while lane < (LA::COUNT + LB::COUNT).min(8) {
            let bit = 1u8 << (lane & 7);
            if self.state.active & bit != 0 {
                let served = if lane < LA::COUNT {
                    self.first_lanes.serve(lane)
                } else {
                    self.second_lanes.serve(lane.wrapping_sub(LA::COUNT))
                };
                match served {
                    Some(Err(error)) => {
                        self.abort();
                        return Some(Err(error));
                    }
                    Some(Ok(())) => {
                        self.state.active &= !bit;
                        ended |= bit;
                    }
                    None => {}
                }
            }
            lane += 1;
        }
        if ended != 0 && self.state.active != 0 && !self.state.listening {
            let mut lane = 0;
            while lane < (LA::COUNT + LB::COUNT).min(8) {
                if self.state.active & (1 << lane) != 0 {
                    if lane < LA::COUNT {
                        self.first_lanes.set_complete_interrupt(lane, true);
                    } else {
                        self.second_lanes
                            .set_complete_interrupt(lane - LA::COUNT, true);
                    }
                }
                lane += 1;
            }
        }
        #[cfg(feature = "profile-start")]
        let t1 = cortex_m::peripheral::DWT::cycle_count();
        if self.state.ahead_active != 0 {
            let mut failed = false;
            for (index, slot) in self.ahead.iter_mut().enumerate() {
                let bit = 1u8 << (index & 7);
                if self.state.ahead_active & bit == 0 {
                    continue;
                }
                let Some(r) = slot.as_mut() else {
                    continue;
                };
                let flags = r.dma.flags();
                if flags.failed() {
                    failed = true;
                    break;
                }
                if flags.complete {
                    r.dma.clear_flags(crate::dma::Flags::ALL);
                    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
                    r.armed = false;
                    self.state.armed &= !bit;
                    self.state.ahead_active &= !bit;
                    ended |= 1 << (r.lane & 7);
                }
            }
            if failed {
                self.abort();
                return Some(Err(Error::Dma));
            }
        }
        if ended == 0 {
            return None;
        }
        #[cfg(feature = "profile-start")]
        let t2 = cortex_m::peripheral::DWT::cycle_count();
        if self.state.active == 0 && self.state.ahead_active == 0 {
            let frame_end = !self.state.listening;
            let idled = self.idle_running();
            if !idled {
                self.halt();
            }
            #[cfg(feature = "profile-start")]
            let t3 = cortex_m::peripheral::DWT::cycle_count();
            let mut result = Some(Ok(ended));
            if frame_end && self.state.listen_on_end {
                self.state.listen_on_end = false;
                if let Err(error) = self.listen() {
                    result = Some(Err(error));
                }
            }
            #[cfg(feature = "profile-start")]
            if frame_end {
                let t4 = cortex_m::peripheral::DWT::cycle_count();
                super::profile::turn_add(&[t0, t1, t2, t3, t4]);
            }
            return result;
        }
        Some(Ok(ended))
    }

    /// Stops everything, with every output at duty 0, ready to load again.
    pub fn abort(&mut self) {
        self.first_lanes.abort_all(&self.first);
        if let Some(second) = self.second.as_ref() {
            self.second_lanes.abort_all(second);
        }
        self.halt();
        for r in self.ahead.iter_mut().flatten() {
            r.disarm();
        }
        self.state.ahead_active = 0;
        self.state.armed = 0;
        self.state.listen_on_end = false;
        // The lanes wrote duty 0 into the preload; the stopped timers make no
        // update to load it, so an output stopped mid-pulse would stay high.
        self.first.latch();
        if let Some(second) = self.second.as_ref() {
            second.latch();
        }
        self.state.active = 0;
        if self.state.listening {
            self.state.listening = false;
            if let Some(plan) = self.plan {
                self.first.apply_rate(&plan.send[0]);
                if let Some(second) = self.second.as_mut() {
                    second.apply_rate(&plan.send[1]);
                }
            }
        }
    }

    /// Stops everything and hands back what the constructor took: the timer
    /// handles and both lane sets, after [`abort`](Self::abort)'s sequence
    /// (every lane at duty 0 with its stream stopped, the timers stopped,
    /// their compare requests closed). Each lane then frees to its wiring and
    /// buffers ([`CompareLane::free`]), and each timer to its token
    /// ([`Pwm::free`]). The second timer is `None` on a generator built with
    /// [`single`](Awg::single). Every reader's stream comes back in the
    /// [`ReaderStreams`], stopped; a paced reader's timer stayed the caller's.
    #[allow(clippy::type_complexity)]
    pub fn free(self) -> (Pwm<A>, Option<Pwm<B>>, LA, LB, ReaderStreams) {
        let mut awg = self;
        awg.abort();
        let mut streams = ReaderStreams { ids: [None; AHEAD] };
        for (slot, reader) in streams.ids.iter_mut().zip(awg.ahead.iter_mut()) {
            if let Some(mut reader) = reader.take() {
                reader.disarm();
                *slot = Some(reader.id);
            }
        }
        let Self {
            first,
            second,
            first_lanes,
            second_lanes,
            ..
        } = awg;
        (first, second, first_lanes, second_lanes, streams)
    }

    fn halt(&mut self) {
        self.pace(false);
        self.first.stop();
        self.first.close_requests();
        {
            self.first.close_update();
            if let Some(second) = self.second.as_ref() {
                second.close_update();
            }
        }
        if let Some(second) = self.second.as_mut() {
            second.stop();
            second.close_requests();
        }
        self.state.started = false;
        self.state.idling = false;
    }

    /// Every lane's stream vector, to bind to
    /// [`on_interrupt`](Self::on_interrupt).
    pub fn vectors(&self) -> crate::resource::Vectors {
        let mut vectors = crate::resource::Vectors::none();
        let mut lane = 0;
        while lane < LA::COUNT + LB::COUNT {
            let vector = if lane < LA::COUNT {
                self.first_lanes.vector(lane)
            } else {
                self.second_lanes.vector(lane.wrapping_sub(LA::COUNT))
            };
            if let Some(vector) = vector {
                vectors = vectors.with(vector);
            }
            lane += 1;
        }
        vectors
    }
}

impl<A: ChannelTimer, B: ChannelTimer, LA: Lanes<A>, LB: Lanes<B>> crate::resource::Service
    for Awg<A, B, LA, LB>
{
    /// Which lanes ended (bit `n` for lane `n`), or why the generator
    /// stopped; `None` if nothing ended.
    type Event = Option<Result<u8, Error>>;

    fn on_interrupt(&mut self) -> Self::Event {
        Awg::on_interrupt(self)
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
