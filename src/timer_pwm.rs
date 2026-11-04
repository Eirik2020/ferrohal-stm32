//! Timer outputs and timer-paced DMA: PWM, compare lanes (the DShot
//! transmit primitive) and pin samplers (its receive primitive). Part of
//! the timer module, for both IP versions.
//!
//! # Ownership
//!
//! A timer's channels share their mode, enable and DMA-request registers
//! (CCMR, CCER, DIER), so everything that writes those goes through
//! `&mut Pwm`: making a channel, turning an output on or off, opening and
//! closing compare requests, staging and starting. A [`PwmChannel`] writes
//! only its own compare register, and a [`CompareLane`] its own compare
//! register and DMA stream.
//!
//! # A lane's transfer
//!
//! [`CompareLane::arm`] writes the first duty to the channel's compare
//! register and starts its stream with the rest, then a 0. [`Pwm::stage`]
//! stops the counter, sets it to its reload value and opens the armed
//! channels' compare requests; [`Pwm::start`] runs it. The first update
//! latches each first duty; every compare match after that has the stream
//! write the next duty, which the following update latches. The final 0
//! holds the output low once the stream is done.
//!
//! Requirements: PWM-001, PWM-002, PWM-003, PWM-004, PWM-005, PWM-006

use core::{
    marker::PhantomData,
    sync::atomic::{Ordering, fence},
};

use stm32_metapac::timer::{
    TimAdv, TimGp16,
    vals::{Mms, Sms, Ts, Urs},
};

/// PWM mode 1 in the general-purpose CCMR: `timer_v3` gives that field its
/// own type, with the H7's extra modes.
#[cfg(feature = "timer_v1")]
const PWM_MODE1: stm32_metapac::timer::vals::Ocm = stm32_metapac::timer::vals::Ocm::PWM_MODE1;
#[cfg(feature = "timer_v3")]
const PWM_MODE1: stm32_metapac::timer::vals::OcmGp = stm32_metapac::timer::vals::OcmGp::PWM_MODE1;

use super::{
    ChannelPin, ChannelTimer, CompareDma, ComplementaryPin, ConfigError, GpioDma, Instance,
    TriggeredBy, UpdateDma,
};
use crate::{
    chip::Interrupt,
    dma::{
        Channel, Direction, FifoMode, Flags, Interrupts, Priority, StreamInterrupt, Transfer,
        WordSize,
    },
    gpio::{Port, PortInstance},
    rcc::Rcc,
    resource::IrqError,
    time::Hertz,
};

/// Spins allowed for a DMA stream to report itself stopped.
const STOP_SPINS: u32 = 10_000;

/// Why a lane or sampler refused or failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// A transfer is running.
    Busy,
    /// Nothing to send, or no samples asked for.
    Empty,
    /// Longer than the buffer.
    TooLarge,
    /// The stream reported a transfer or direct-mode error; it was stopped.
    Dma,
}

/// The prescaler and reload nearest `rate` from `clock`: (PSC + 1,
/// ARR + 1, the rate they give). The smallest prescaler that fits, for the
/// finest duty steps. For PWM, where the rate needs only be near (DShot
/// at 600 kHz from the H7's 200 MHz timer clock is 333.33 counts).
pub const fn nearest(
    clock: Hertz,
    rate: Hertz,
    bits: u8,
) -> Result<(u32, u64, Hertz), ConfigError> {
    if rate.0 == 0 {
        return Err(ConfigError::Inexact);
    }
    let reload_max: u64 = if bits >= 32 { 1 << 32 } else { 1 << 16 };
    // Rounded to nearest.
    let total = (clock.0 as u64 + rate.0 as u64 / 2) / rate.0 as u64;
    if total < 2 {
        return Err(ConfigError::OutOfRange);
    }
    let prescaler = total.div_ceil(reload_max);
    // Never 0, as `total` is at least 2; saying so lets the compiler see it,
    // so the divisions below have no panic path.
    if prescaler == 0 || prescaler > 65_536 {
        return Err(ConfigError::OutOfRange);
    }
    let reload = (total + prescaler / 2) / prescaler;
    if reload < 2 || reload > reload_max {
        return Err(ConfigError::OutOfRange);
    }
    let actual = clock.0 as u64 / (prescaler * reload);
    Ok((prescaler as u32, reload, Hertz(actual as u32)))
}

/// Duty values are compare counts, 0 to [`Pwm::period`]: the output is
/// active while the counter is below the duty (PWM mode 1).
pub struct Pwm<T> {
    gp: TimGp16,
    clock: Hertz,
    period: u32,
    frequency: Hertz,
    _timer: PhantomData<T>,
}

impl<T: ChannelTimer> Pwm<T> {
    /// Enables and resets the timer and sets it to count at `rate`, as near
    /// as the clock allows ([`frequency`](Self::frequency) says what it
    /// got), with every channel off. Stopped until [`start`](Self::start).
    pub fn new(rcc: &mut Rcc, _timer: T, rate: Hertz) -> Result<Self, ConfigError> {
        let clock = rcc.clocks().timer(T::BUS);
        let (prescaler, reload, frequency) = nearest(clock, rate, T::BITS)?;
        rcc.enable(T::ENABLE);
        rcc.reset(T::ENABLE);
        let gp = Self::gp();
        let core = T::core();
        super::load(core, prescaler, reload);
        gp.cr1().modify(|w| {
            w.set_urs(Urs::COUNTER_ONLY);
            w.set_arpe(true);
        });
        if T::BREAK {
            // Outputs drive their pins only with MOE set.
            Self::adv().bdtr().modify(|w| w.set_moe(true));
        }
        Ok(Self {
            gp,
            clock,
            period: reload as u32,
            frequency,
            _timer: PhantomData,
        })
    }

    /// Stops the timer and sets it to count at `rate`, as near as the
    /// clock allows. Channels keep their modes, outputs and duties; scale
    /// the duties to the new [`period`](Self::period).
    pub fn set_frequency(&mut self, rate: Hertz) -> Result<Hertz, ConfigError> {
        let (prescaler, reload, frequency) = nearest(self.clock, rate, T::BITS)?;
        self.stop();
        reload_counter(self.gp, prescaler, reload);
        self.period = reload as u32;
        self.frequency = frequency;
        Ok(frequency)
    }

    /// The rate `T` would run at for `rate` under `clocks`.
    pub const fn check(clocks: &crate::rcc::Clocks, rate: Hertz) -> Result<Hertz, ConfigError> {
        match nearest(clocks.timer(T::BUS), rate, T::BITS) {
            Ok((_, _, actual)) => Ok(actual),
            Err(error) => Err(error),
        }
    }

    fn gp() -> TimGp16 {
        // SAFETY: a channel timer's base address; the registers used here
        // sit where the 16-bit general-purpose layout has them on every
        // channel timer.
        unsafe { TimGp16::from_ptr(T::core().as_ptr()) }
    }

    fn adv() -> TimAdv {
        // SAFETY: only for timers with a break unit, whose BDTR and CCER's
        // complementary bits sit where the advanced layout has them.
        unsafe { TimAdv::from_ptr(T::core().as_ptr()) }
    }

    /// Counts per period: the duty for an output always active.
    pub fn period(&self) -> u32 {
        self.period
    }

    /// The rate the timer runs at.
    pub fn frequency(&self) -> Hertz {
        self.frequency
    }

    /// Channel `C` in PWM mode 1 with its compare preloaded, at duty 0,
    /// its output enabled. `pin` proves the channel reaches a pin.
    pub fn channel<const C: u8, P: ChannelPin<T, C>>(&mut self, _pin: P) -> PwmChannel<T, C> {
        self.setup::<C>();
        self.gp
            .ccer()
            .modify(|w| w.set_cce(usize::from((C - 1) & 3), true));
        PwmChannel {
            gp: self.gp,
            _timer: PhantomData,
        }
    }

    /// Channel `C` driving its complementary output (CHxN) instead, at duty
    /// 0. Only timers with a break unit have one.
    pub fn complementary<const C: u8, P: ComplementaryPin<T, C>>(
        &mut self,
        _pin: P,
    ) -> PwmChannel<T, C> {
        self.setup::<C>();
        Self::adv()
            .ccer()
            .modify(|w| w.set_ccne(usize::from((C - 1) % 3), true));
        PwmChannel {
            gp: self.gp,
            _timer: PhantomData,
        }
    }

    fn setup<const C: u8>(&mut self) {
        const { assert!(C >= 1 && C <= 4, "channels are 1 to 4") };
        let index = usize::from((C - 1) & 3);
        self.gp
            .ccr(index)
            .write_value(stm32_metapac::timer::regs::Ccr1ch(0));
        self.gp.ccmr_output(index / 2).modify(|w| {
            w.set_ocm(index % 2, PWM_MODE1);
            w.set_ocpe(index % 2, true);
        });
    }

    /// Turns channel `C`'s outputs (CHx and CHxN) off or back on. Off, the
    /// pin falls to its inactive level.
    pub fn set_output<const C: u8>(&mut self, on: bool) {
        let index = usize::from((C - 1) & 3);
        if T::BREAK && index < 3 {
            let ccer = Self::adv().ccer();
            let complementary = ccer.read().ccne(index);
            ccer.modify(|w| {
                w.set_cce(index, on);
                if complementary || !on {
                    w.set_ccne(index, on && complementary);
                }
            });
        } else {
            self.gp.ccer().modify(|w| w.set_cce(index, on));
        }
    }

    /// Makes this timer the one others start from: its trigger output pulses
    /// when its counter is enabled. See [`follow`](Self::follow).
    pub fn lead(&mut self) {
        self.gp.cr2().modify(|w| w.set_mms(Mms::ENABLE));
    }

    /// Starts this timer from `master`'s [`start`](Self::start) from now on,
    /// so both begin on one edge. Don't `start` this one yourself. `stop`,
    /// `stage` and the rest work as ever; the next master start starts it
    /// again. `master` has to have called [`lead`](Self::lead).
    pub fn follow<M: ChannelTimer>(&mut self, _master: &Pwm<M>)
    where
        T: TriggeredBy<M>,
    {
        self.gp.smcr().modify(|w| {
            w.set_ts(Ts::from_bits(T::ITR));
            w.set_sms(Sms::TRIGGER_MODE);
        });
    }

    /// The counter's value now.
    pub fn counter(&self) -> u32 {
        // SAFETY: CNT, read whole: a 16-bit timer's upper half reads zero.
        unsafe { core::ptr::read_volatile(self.gp.cnt().as_ptr() as *const u32) }
    }

    /// Whether the counter is running.
    pub fn is_running(&self) -> bool {
        self.gp.cr1().read().cen()
    }

    /// Stops the counter, closes every compare request, sets the counter to
    /// its reload value (so the first count after `start` is an update that
    /// latches each channel's preloaded duty) and opens the compare
    /// requests of `channels` (bit 0 = channel 1).
    pub fn stage(&mut self, channels: u8) {
        self.stop();
        self.gp.dier().modify(|w| {
            for index in 0..4 {
                w.set_ccde(index, false);
            }
        });
        self.gp.cnt().write(|w| w.set_cnt((self.period - 1) as u16));
        if T::BITS == 32 {
            // A 32-bit counter takes the whole word.
            // SAFETY: CNT of a 32-bit timer, written whole.
            unsafe {
                core::ptr::write_volatile(self.gp.cnt().as_ptr() as *mut u32, self.period - 1);
            }
        }
        // Stale compare flags would raise a request at once.
        self.gp
            .sr()
            .write_value(stm32_metapac::timer::regs::SrGp16(0));
        self.gp.dier().modify(|w| {
            for index in 0..4 {
                if channels & (1 << index) != 0 {
                    w.set_ccde(index, true);
                }
            }
        });
    }

    /// Closes every compare request.
    pub fn close_requests(&mut self) {
        self.gp.dier().modify(|w| {
            for index in 0..4 {
                w.set_ccde(index, false);
            }
        });
    }

    pub fn start(&mut self) {
        self.gp.cr1().modify(|w| w.set_cen(true));
    }

    pub fn stop(&mut self) {
        self.gp.cr1().modify(|w| w.set_cen(false));
    }
}

/// One PWM channel's compare register.
pub struct PwmChannel<T, const C: u8> {
    gp: TimGp16,
    _timer: PhantomData<T>,
}

// SAFETY: a channel writes only its own compare register.
unsafe impl<T, const C: u8> Send for PwmChannel<T, C> {}

impl<T: ChannelTimer, const C: u8> PwmChannel<T, C> {
    fn ccr(
        &self,
    ) -> stm32_metapac::common::Reg<stm32_metapac::timer::regs::Ccr1ch, stm32_metapac::common::RW>
    {
        self.gp.ccr(usize::from((C - 1) & 3))
    }

    /// The duty from the next period on (the compare is preloaded).
    pub fn set_duty(&mut self, duty: u32) {
        self.ccr()
            .write_value(stm32_metapac::timer::regs::Ccr1ch(duty));
    }

    pub fn duty(&self) -> u32 {
        self.ccr().read().0
    }

    /// Feeds this channel's compare register from `stream`, out of `buffer`.
    pub fn into_lane<S: CompareDma<T, C>>(
        self,
        stream: S,
        buffer: &'static mut [u32],
    ) -> Result<CompareLane<T, C>, crate::uart::ConfigError> {
        crate::chip::check_dma_buffer(as_bytes(buffer))?;
        let mut dma = stream.into_channel();
        dma.stop(STOP_SPINS)
            .map_err(|_| crate::uart::ConfigError::DmaBusy)?;
        dma.clear_flags(Flags::ALL);
        Ok(CompareLane {
            channel: self,
            dma,
            request: S::REQUEST,
            vector: S::INTERRUPT,
            buffer,
            running: false,
        })
    }
}

/// New PSC and ARR, loaded at once through an update event that sets no
/// flag (URS is counter-only), the counter left at 0.
fn reload_counter(gp: TimGp16, prescaler: u32, reload: u64) {
    gp.psc().write_value((prescaler - 1) as u16);
    gp.arr()
        .write_value(stm32_metapac::timer::regs::ArrCore((reload - 1) as u32));
    gp.egr().write(|w| w.set_ug(true));
    gp.cnt().write(|w| w.set_cnt(0));
}

fn as_bytes(words: &[u32]) -> &[u8] {
    // SAFETY: the same memory, viewed as bytes, for the address check.
    unsafe { core::slice::from_raw_parts(words.as_ptr().cast(), core::mem::size_of_val(words)) }
}

/// A channel whose compare register a DMA stream feeds, one duty per
/// period, paced by the channel's own compare request.
pub struct CompareLane<T, const C: u8> {
    channel: PwmChannel<T, C>,
    dma: Channel,
    request: u8,
    vector: Interrupt,
    buffer: &'static mut [u32],
    running: bool,
}

impl<T: ChannelTimer, const C: u8> CompareLane<T, C> {
    /// Loads `duties[0]` into the compare register and starts the stream
    /// with the rest, then a 0 that holds the output low afterwards. The
    /// timer's `stage` and `start` set it going. At most the buffer's
    /// length, less one for the 0.
    pub fn arm(&mut self, duties: &[u32]) -> Result<(), Error> {
        if self.running {
            return Err(Error::Busy);
        }
        let (first, rest) = duties.split_first().ok_or(Error::Empty)?;
        let len = rest.len() + 1;
        let staged = self.buffer.get_mut(..len).ok_or(Error::TooLarge)?;
        let len = u16::try_from(len).map_err(|_| Error::TooLarge)?;
        if let Some((last, head)) = staged.split_last_mut() {
            head.copy_from_slice(rest);
            *last = 0;
        }
        self.channel.set_duty(*first);
        let transfer = Transfer {
            request: self.request,
            #[cfg(feature = "dma_v1")]
            bufferable: false,
            direction: Direction::MemoryToPeripheral,
            peripheral: self.channel.ccr().as_ptr() as u32,
            memory: self.buffer.as_ptr() as u32,
            double_buffer: None,
            len,
            size: WordSize::Word,
            memory_increment: true,
            circular: false,
            priority: Priority::VeryHigh,
            fifo: FifoMode::Direct,
            interrupts: Interrupts {
                complete: true,
                error: true,
                ..Interrupts::default()
            },
        };
        // SAFETY: the buffer is the lane's own for its life and holds `len`
        // words; the address is this channel's compare register, whose
        // request the stream serves.
        unsafe { self.dma.start(&transfer) }.map_err(|_| Error::Dma)?;
        self.running = true;
        Ok(())
    }

    /// Serves the stream's interrupt: `Some` once the transfer has ended.
    /// The output then holds low at duty 0; the timer keeps counting until
    /// the firmware stops it.
    pub fn on_interrupt(&mut self) -> Option<Result<(), Error>> {
        if !self.running {
            self.dma.clear_flags(Flags::ALL);
            return None;
        }
        let flags = self.dma.flags();
        if flags.transfer_error || flags.direct_mode_error {
            self.abort();
            return Some(Err(Error::Dma));
        }
        if !flags.complete {
            return None;
        }
        self.dma.clear_flags(Flags::ALL);
        self.running = false;
        Some(Ok(()))
    }

    /// [`on_interrupt`](Self::on_interrupt), for firmware that polls.
    pub fn poll(&mut self) -> Option<Result<(), Error>> {
        self.on_interrupt()
    }

    pub fn is_busy(&self) -> bool {
        self.running
    }

    /// Stops the stream and drops the output to duty 0.
    pub fn abort(&mut self) {
        let _ = self.dma.stop(STOP_SPINS);
        self.dma.clear_flags(Flags::ALL);
        fence(Ordering::SeqCst);
        self.channel.set_duty(0);
        self.running = false;
    }

    /// The vector to bind to [`on_interrupt`](Self::on_interrupt).
    pub fn interrupt(&self) -> Interrupt {
        self.vector
    }

    pub fn verify(&self) -> Result<(), IrqError> {
        crate::resource::verify_vectors([self.vector].into_iter())
    }

    /// The channel, for a duty outside a transfer.
    pub fn channel(&mut self) -> &mut PwmChannel<T, C> {
        &mut self.channel
    }
}

/// A port's pin levels, copied into memory by DMA at a timer's update rate:
/// one 16-bit sample of the whole port's input register per update.
///
/// On F4 the stream must be DMA2's (`GpioDma`), whose peripheral port
/// reaches GPIO. ES0299 2.2.11 and ES0182 2.2.19: DMA2 can corrupt data when
/// it serves an AHB peripheral (this sampler) and an APB2 peripheral at the
/// same time; keep APB2 traffic off DMA2 while sampling.
pub struct Sampler {
    timer: TimGp16,
    clock: Hertz,
    bits: u8,
    dma: Channel,
    request: u8,
    vector: Interrupt,
    input: u32,
    buffer: &'static mut [u16],
    rate: Hertz,
    state: SampleState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SampleState {
    Idle,
    Running(u16),
    Done(u16),
}

impl Sampler {
    /// Enables and resets `timer` to update at `rate` (as near as the clock
    /// allows; [`rate`](Self::rate) says what it got), and points `stream`
    /// at port `P`'s input register. Nothing runs until
    /// [`start`](Self::start).
    pub fn new<T, S, const P: char>(
        rcc: &mut Rcc,
        _timer: T,
        stream: S,
        rate: Hertz,
        buffer: &'static mut [u16],
    ) -> Result<Self, crate::uart::ConfigError>
    where
        T: Instance,
        S: UpdateDma<T> + GpioDma + StreamInterrupt,
        Port<P>: PortInstance,
    {
        let clock = rcc.clocks().timer(T::BUS);
        let (prescaler, reload, rate) =
            nearest(clock, rate, T::BITS).map_err(|_| crate::uart::ConfigError::Unsupported)?;
        crate::chip::check_dma_buffer(u16_bytes(buffer))?;
        rcc.enable(T::ENABLE);
        rcc.reset(T::ENABLE);
        super::load(T::core(), prescaler, reload);
        let mut dma = stream.into_channel();
        dma.stop(STOP_SPINS)
            .map_err(|_| crate::uart::ConfigError::DmaBusy)?;
        dma.clear_flags(Flags::ALL);
        Ok(Self {
            // SAFETY: the timer's base address; CR1, DIER, SR and CNT sit
            // where the general-purpose layout has them on every timer.
            timer: unsafe { TimGp16::from_ptr(T::core().as_ptr()) },
            clock,
            bits: T::BITS,
            dma,
            request: S::REQUEST,
            vector: S::INTERRUPT,
            input: <Port<P> as PortInstance>::regs().idr().as_ptr() as u32,
            buffer,
            rate,
            state: SampleState::Idle,
        })
    }

    pub fn rate(&self) -> Hertz {
        self.rate
    }

    /// Stops sampling and sets the timer to update at `rate`, as near as
    /// the clock allows.
    pub fn set_rate(&mut self, rate: Hertz) -> Result<Hertz, ConfigError> {
        let (prescaler, reload, rate) = nearest(self.clock, rate, self.bits)?;
        self.stop();
        reload_counter(self.timer, prescaler, reload);
        self.rate = rate;
        Ok(rate)
    }

    /// Takes `count` samples, from the next update on.
    pub fn start(&mut self, count: usize) -> Result<(), Error> {
        if matches!(self.state, SampleState::Running(_)) {
            return Err(Error::Busy);
        }
        if count == 0 {
            return Err(Error::Empty);
        }
        if count > self.buffer.len() {
            return Err(Error::TooLarge);
        }
        let len = u16::try_from(count).map_err(|_| Error::TooLarge)?;
        let transfer = Transfer {
            request: self.request,
            #[cfg(feature = "dma_v1")]
            bufferable: false,
            direction: Direction::PeripheralToMemory,
            peripheral: self.input,
            memory: self.buffer.as_mut_ptr() as u32,
            double_buffer: None,
            len,
            size: WordSize::HalfWord,
            memory_increment: true,
            circular: false,
            priority: Priority::VeryHigh,
            fifo: FifoMode::Direct,
            interrupts: Interrupts {
                complete: true,
                error: true,
                ..Interrupts::default()
            },
        };
        let timer = self.timer;
        timer.cr1().modify(|w| w.set_cen(false));
        timer.cnt().write(|w| w.set_cnt(0));
        timer
            .sr()
            .write_value(stm32_metapac::timer::regs::SrGp16(0));
        // SAFETY: the buffer is the sampler's own and holds `len` samples;
        // the address is a GPIO port's input register, read-only and
        // without side effects, which the update request paces.
        unsafe { self.dma.start(&transfer) }.map_err(|_| Error::Dma)?;
        timer.dier().modify(|w| w.set_ude(true));
        timer.cr1().modify(|w| w.set_cen(true));
        self.state = SampleState::Running(len);
        Ok(())
    }

    /// Serves the stream's interrupt: `Some` once the samples are in.
    pub fn on_interrupt(&mut self) -> Option<Result<usize, Error>> {
        let SampleState::Running(len) = self.state else {
            self.dma.clear_flags(Flags::ALL);
            return None;
        };
        let flags = self.dma.flags();
        if flags.transfer_error || flags.direct_mode_error {
            self.stop();
            return Some(Err(Error::Dma));
        }
        if !flags.complete {
            return None;
        }
        self.stop();
        self.state = SampleState::Done(len);
        Some(Ok(usize::from(len)))
    }

    pub fn poll(&mut self) -> Option<Result<usize, Error>> {
        self.on_interrupt()
    }

    /// The last completed run's samples; empty otherwise.
    pub fn samples(&self) -> &[u16] {
        match self.state {
            SampleState::Done(len) => self.buffer.get(..usize::from(len)).unwrap_or(&[]),
            _ => &[],
        }
    }

    /// Stops the timer and the stream.
    pub fn stop(&mut self) {
        self.timer.cr1().modify(|w| w.set_cen(false));
        self.timer.dier().modify(|w| w.set_ude(false));
        let _ = self.dma.stop(STOP_SPINS);
        self.dma.clear_flags(Flags::ALL);
        fence(Ordering::SeqCst);
        self.state = SampleState::Idle;
    }

    pub fn interrupt(&self) -> Interrupt {
        self.vector
    }

    pub fn verify(&self) -> Result<(), IrqError> {
        crate::resource::verify_vectors([self.vector].into_iter())
    }
}

fn u16_bytes(words: &[u16]) -> &[u8] {
    // SAFETY: the same memory, viewed as bytes, for the address check.
    unsafe { core::slice::from_raw_parts(words.as_ptr().cast(), core::mem::size_of_val(words)) }
}

// SAFETY: a sampler owns its timer and stream; the buffer is its own.
unsafe impl Send for Sampler {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rounds_and_reports() {
        // DShot600 from the F401's 84 MHz APB1 timer clock: exact.
        assert_eq!(
            nearest(Hertz::mhz(84), Hertz::khz(600), 16),
            Ok((1, 140, Hertz::khz(600)))
        );
        // From the H7's 200 MHz: 333 counts, 600.6 kHz.
        assert_eq!(
            nearest(Hertz::mhz(200), Hertz::khz(600), 16),
            Ok((1, 333, Hertz(600_600)))
        );
        assert_eq!(
            nearest(Hertz::mhz(84), Hertz(0), 16),
            Err(ConfigError::Inexact)
        );
        assert_eq!(
            nearest(Hertz::mhz(84), Hertz::mhz(84), 16),
            Err(ConfigError::OutOfRange)
        );
    }
}
