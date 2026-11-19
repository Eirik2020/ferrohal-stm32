//! The PWM, compare-lane and sampler checks, shared by the bench firmwares.
//! They need no wiring: a pin's input register reads its level while a
//! timer drives it, so a sampler records each output's own waveform.
//!
//! Lanes send 16-bit patterns the way DShot does, one bit per period: a 1
//! is a pulse of 3/4 of the period, a 0 one of 3/8. The checks decode the
//! samples back into bits and timing.

// Each run binary compiles this file and uses the checks it needs.
#![allow(dead_code)]

use core::cell::RefCell;

use cortex_m::{
    interrupt::{self, Mutex},
    peripheral::DWT,
};
use ferrohal_stm32::{
    time::Hertz,
    timer::{
        Awg, ChannelTimer, CompareLane, Error as PwmError, Frame, FrameLimits, Lanes, Pwm, Sampler,
    },
};

use crate::tests::{Fail, Report};

/// The bench's frame memory: a lane keeps none of its own (FerroHAL
/// ADR-013), so every play lends it a [`Frame`] written here. Copying the
/// duties in is the bench's work, as a user's; the lane plays what it is lent
/// in place. A bench seeds the pool once with [`lend_frames`], one block per
/// lane it plays at a time.
static FRAMES: Mutex<RefCell<[Option<&'static mut [u32]>; 16]>> =
    Mutex::new(RefCell::new([const { None }; 16]));

/// Adds `memory` to the bench's frame memory.
pub fn lend_frames(memory: &'static mut [u32]) {
    put(memory);
}

fn put(memory: &'static mut [u32]) {
    interrupt::free(|cs| {
        let mut pool = FRAMES.borrow(cs).borrow_mut();
        if let Some(slot) = pool.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(memory);
        }
    });
}

/// A block that holds `len` duties and the closing 0.
fn pick(len: usize) -> Option<&'static mut [u32]> {
    interrupt::free(|cs| {
        let mut pool = FRAMES.borrow(cs).borrow_mut();
        pool.iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|memory| memory.len() > len))
            .and_then(Option::take)
    })
}

/// `duties` written into a block of the pool and closed as a frame for
/// `limits`, refusing as `Frame::new` does (or `TooLarge` with no block
/// long enough), the block back in the pool.
fn frame(duties: &[u32], limits: FrameLimits) -> Result<Frame, PwmError> {
    let words = pick(duties.len()).ok_or(PwmError::TooLarge)?;
    if let Some(head) = words.get_mut(..duties.len()) {
        head.copy_from_slice(duties);
    }
    Frame::new(words, duties.len(), limits).map_err(|refused| {
        let (error, words) = refused.into_inner();
        put(words);
        error
    })
}

/// Returns a frame's memory to the pool, as a lane's `free` hands it back.
pub fn reclaim(frame: Option<Frame>) {
    if let Some(frame) = frame {
        put(frame.into_words());
    }
}

/// Plays `duties` on `lane` from the timer's next start, from the bench's
/// frame memory; the lane's last frame goes back to it first.
pub fn arm<T: ChannelTimer, const C: u8, W>(
    lane: &mut CompareLane<T, C, W>,
    pwm: &Pwm<T>,
    duties: &[u32],
) -> Result<(), PwmError> {
    reclaim(lane.take_frame());
    let frame = frame(duties, pwm.frame_limits())?;
    lane.arm(pwm, frame).map_err(|refused| {
        let (error, frame) = refused.into_inner();
        reclaim(Some(frame));
        error
    })
}

/// Loads `duties` on the generator's `lane`, as [`arm`] does on a lane.
pub fn load<A, B, LA, LB>(
    awg: &mut Awg<A, B, LA, LB>,
    lane: usize,
    duties: &[u32],
) -> Result<(), PwmError>
where
    A: ChannelTimer,
    B: ChannelTimer,
    LA: Lanes<A>,
    LB: Lanes<B>,
{
    reclaim(awg.take_frame(lane));
    let limits = awg.frame_limits(lane).ok_or(PwmError::Empty)?;
    let frame = frame(duties, limits)?;
    awg.load(lane, frame).map_err(|refused| {
        let (error, frame) = refused.into_inner();
        reclaim(Some(frame));
        error
    })
}

/// A 1 bit's pulse, as a fraction of the period, in sixteenths.
const ONE: u32 = 12;
/// A 0 bit's.
const ZERO: u32 = 6;
const BITS: usize = 16;

/// The duties for `pattern`, most significant bit first.
pub fn duties(pattern: u16, period: u32) -> [u32; BITS] {
    core::array::from_fn(|i| {
        let one = pattern & (0x8000 >> i) != 0;
        period * if one { ONE } else { ZERO } / 16
    })
}

/// Pulses on bit `pin` of `samples`: (first sample, length), in order.
/// Active low, the low runs carry the pulses.
fn pulses(samples: &[u16], pin: u8, active_low: bool, out: &mut [(usize, usize); 32]) -> usize {
    let mut count = 0;
    let mut start = None;
    for (i, sample) in samples.iter().enumerate() {
        let high = (sample & (1 << pin) != 0) != active_low;
        match (high, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if let Some(slot) = out.get_mut(count) {
                    *slot = (s, i - s);
                }
                count += 1;
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if let Some(slot) = out.get_mut(count) {
            *slot = (s, samples.len() - s);
        }
        count += 1;
    }
    count
}

/// Checks that bit `pin` of `samples` carries `pattern`: 16 pulses one
/// period apart (`per_bit` samples, in 1/16ths), each 3/4 or 3/8 of it,
/// then nothing. Active low, the line idles high and the pulses pull low,
/// as bidirectional DShot drives it.
pub fn decode(
    samples: &[u16],
    pin: u8,
    pattern: u16,
    per_bit16: u32,
    active_low: bool,
) -> Result<(), Fail> {
    let mut found = [(0, 0); 32];
    let count = pulses(samples, pin, active_low, &mut found);
    if count != BITS {
        let last = found[count.saturating_sub(1).min(31)];
        defmt::info!(
            "pin {}: {} samples, {}/16 per bit, first pulse at {}, last ends at {}",
            pin,
            samples.len(),
            per_bit16,
            found[0].0,
            last.0 + last.1
        );
        return Err(Fail::Count {
            expected: BITS as u32,
            got: count as u32,
        });
    }
    // Pulses longer than 9/16 of a period are 1s.
    let threshold16 = per_bit16 * 9 / 16;
    let mut got = 0u16;
    for (i, (_, len)) in found.iter().take(BITS).enumerate() {
        if (*len as u32) * 16 > threshold16 {
            got |= 0x8000 >> i;
        }
    }
    if got != pattern {
        let at = (got ^ pattern).leading_zeros() as usize;
        return Err(Fail::Mismatch {
            at,
            sent: (pattern >> (15 - at)) as u8 & 1,
            got: (got >> (15 - at)) as u8 & 1,
        });
    }
    // Rising edges one period apart, to within a sample and a tenth.
    for pair in found.windows(2).take(BITS - 1) {
        let gap16 = ((pair[1].0 - pair[0].0) as u32) * 16;
        let slack16 = 16 + per_bit16 / 10;
        if gap16.abs_diff(per_bit16) > slack16 {
            return Err(Fail::Time {
                cycles: gap16 / 16,
                min: (per_bit16 - slack16) / 16,
                max: (per_bit16 + slack16) / 16,
            });
        }
    }
    Ok(())
}

/// Polls `poll` until it returns `Some`, for at most `cycles`.
pub fn wait<R>(cycles: u32, mut poll: impl FnMut() -> Option<R>) -> Option<R> {
    let start = DWT::cycle_count();
    loop {
        if let Some(result) = poll() {
            return Some(result);
        }
        if DWT::cycle_count().wrapping_sub(start) > cycles {
            return None;
        }
    }
}

/// Two lanes of one timer send `patterns` from the same update, while
/// `sampler` records their port; each lane's pin must carry its pattern.
#[allow(clippy::too_many_arguments)]
pub fn lane_pair_check<
    T: ChannelTimer,
    U: ferrohal_stm32::timer::Instance,
    const A: u8,
    const B: u8,
    SU,
    WA,
    WB,
>(
    report: &mut Report,
    name: &str,
    pwm: &mut Pwm<T>,
    lanes: (&mut CompareLane<T, A, WA>, &mut CompareLane<T, B, WB>),
    pins: (u8, u8),
    patterns: (u16, u16),
    sampler: &mut Sampler<U, SU>,
    cpu: Hertz,
) {
    let period = pwm.period();
    // Samples per bit, in sixteenths.
    let per_bit16 = (u64::from(sampler.rate().0) * 16 / u64::from(pwm.rate().0)) as u32;
    // Eight spare bits around the frame.
    let count = (per_bit16 as usize * (BITS + 8)) / 16;
    let result = (|| {
        arm(lanes.0, pwm, &duties(patterns.0, period))
            .map_err(|_| Fail::Count {
                expected: BITS as u32,
                got: 0,
            })?;
        arm(lanes.1, pwm, &duties(patterns.1, period))
            .map_err(|_| Fail::Count {
                expected: BITS as u32,
                got: 0,
            })?;
        pwm.stage((1 << (A - 1)) | (1 << (B - 1)));
        // Sampling starts last, just before the timer, so the window holds
        // the frame and a few spare bits around it.
        sampler.start(count).map_err(|_| Fail::Count {
            expected: count as u32,
            got: 0,
        })?;
        pwm.start();
        let budget = cpu.0 / 100;
        let first = wait(budget, || lanes.0.on_interrupt());
        let second = wait(budget, || lanes.1.on_interrupt());
        let sampled = wait(budget, || sampler.on_interrupt());
        pwm.stop();
        pwm.close_requests();
        if !matches!(first, Some(Ok(()))) || !matches!(second, Some(Ok(()))) {
            lanes.0.abort(pwm);
            lanes.1.abort(pwm);
            return Err(Fail::Count {
                expected: 2,
                got: u32::from(first.is_some()) + u32::from(second.is_some()),
            });
        }
        match sampled {
            Some(Ok(_)) => Ok(()),
            _ => Err(Fail::Count {
                expected: count as u32,
                got: 0,
            }),
        }
    })();
    if let Err(fail) = result {
        report.check(
            name,
            "both lanes and the sampler finish [PWM-003]",
            Err(fail),
        );
        sampler.abort();
        return;
    }
    report.check(name, "both lanes and the sampler finish [PWM-003]", Ok(()));
    report.check(
        name,
        "first lane: its 16 bits, one period apart [PWM-002]",
        decode(sampler.samples(), pins.0, patterns.0, per_bit16, false),
    );
    report.check(
        name,
        "second lane: its 16 bits, one period apart [PWM-002]",
        decode(sampler.samples(), pins.1, patterns.1, per_bit16, false),
    );
}

/// ES0392 2.16.2 (ES0299 2.6.3, ES0182 2.7.3): a compare match at CNT = CCR =
/// ARR followed by one at CNT = CCR = 0 misses the second compare event, and
/// a lane, which writes each next duty on its compare request, would fall a
/// period behind (measured on the H753, 2026-11-17, before the refusal). The
/// first lane must refuse `[P/2, P-1, 0, P/2]` (P the period, so P-1 is ARR)
/// with `TopToZero`; the same waveform just clear of `Pwm::top_margin` must play
/// with every pulse starting with the second lane's steady P/2 reference
/// pulse of its own period [PWM-009].
#[allow(clippy::too_many_arguments)]
pub fn top_to_zero_check<
    T: ChannelTimer,
    U: ferrohal_stm32::timer::Instance,
    const A: u8,
    const B: u8,
    SU,
    WA,
    WB,
>(
    report: &mut Report,
    name: &str,
    pwm: &mut Pwm<T>,
    lanes: (&mut CompareLane<T, A, WA>, &mut CompareLane<T, B, WB>),
    pins: (u8, u8),
    sampler: &mut Sampler<U, SU>,
    cpu: Hertz,
) {
    let period = pwm.period();
    let wave = |high: u32| -> [u32; BITS] {
        core::array::from_fn(|i| match i % 4 {
            1 => high,
            2 => 0,
            _ => period / 2,
        })
    };
    let refused = arm(lanes.0, pwm, &wave(period - 1));
    report.check(
        name,
        "a duty at the top followed by 0 is refused [PWM-009]",
        match refused {
            Err(ferrohal_stm32::timer::Error::TopToZero) => Ok(()),
            _ => Err(Fail::Count {
                expected: 1,
                got: 0,
            }),
        },
    );
    if refused.is_ok() {
        lanes.0.abort(pwm);
    }
    // Just clear of the margin: the step the refusal leaves allowed.
    let step = wave(period - pwm.top_margin() - 1);
    let reference = [period / 2; BITS];
    let per_bit16 = (u64::from(sampler.rate().0) * 16 / u64::from(pwm.rate().0)) as u32;
    let count = (per_bit16 as usize * (BITS + 8)) / 16;
    let finished = (|| {
        arm(lanes.0, pwm, &step).ok()?;
        arm(lanes.1, pwm, &reference).ok()?;
        pwm.stage((1 << (A - 1)) | (1 << (B - 1)));
        sampler.start(count).ok()?;
        pwm.start();
        let budget = cpu.0 / 100;
        let first = wait(budget, || lanes.0.on_interrupt());
        let second = wait(budget, || lanes.1.on_interrupt());
        let sampled = wait(budget, || sampler.on_interrupt());
        pwm.stop();
        pwm.close_requests();
        match (first, second, sampled) {
            (Some(Ok(())), Some(Ok(())), Some(Ok(_))) => Some(()),
            _ => None,
        }
    })();
    if finished.is_none() {
        lanes.0.abort(pwm);
        lanes.1.abort(pwm);
        sampler.abort();
        report.check(
            name,
            "clear of the top margin, both lanes and the sampler finish [PWM-003]",
            Err(Fail::Count {
                expected: 3,
                got: 0,
            }),
        );
        return;
    }
    let mut steps = [(0, 0); 32];
    let mut periods = [(0, 0); 32];
    let stepped = pulses(sampler.samples(), pins.0, false, &mut steps);
    let referenced = pulses(sampler.samples(), pins.1, false, &mut periods);
    // The periods with a non-zero duty, in order: 12 of the 16.
    let expected = (0..BITS).filter(|i| i % 4 != 2);
    let slack = (per_bit16 / 16 / 4).max(1) as usize;
    let result = if referenced != BITS || stepped != 12 {
        defmt::info!("step lane: {} pulses, reference: {}", stepped, referenced);
        Err(Fail::Count {
            expected: 12,
            got: stepped as u32,
        })
    } else {
        expected
            .zip(steps.iter())
            .try_for_each(|(index, &(start, _))| {
                let at = periods[index].0;
                if start.abs_diff(at) <= slack {
                    Ok(())
                } else {
                    defmt::info!(
                        "step lane: duty {} starts at sample {}, its period at {}",
                        index,
                        start,
                        at
                    );
                    Err(Fail::Count {
                        expected: at as u32,
                        got: start as u32,
                    })
                }
            })
    };
    report.check(
        name,
        "clear of the top margin, a step to 0 keeps every later duty in its period [PWM-009]",
        result,
    );
}

/// The waveform generator on two lanes of one timer, read back by `sampler`:
/// each lane's own samples, a ramp, one lane alone, and what it refuses.
pub fn awg_check<
    T: ChannelTimer,
    U: ferrohal_stm32::timer::Instance,
    const A: u8,
    const B: u8,
    SU,
    WA,
    WB,
>(
    report: &mut Report,
    name: &str,
    awg: &mut Awg<T, T, (CompareLane<T, A, WA>, CompareLane<T, B, WB>), ()>,
    pins: (u8, u8),
    sampler: &mut Sampler<U, SU>,
    cpu: Hertz,
) {
    let period = awg.period();
    let per_bit16 = (u64::from(sampler.rate().0) * 16 / u64::from(awg.sample_rate().0)) as u32;
    let count = (per_bit16 as usize * (BITS + 8)) / 16;
    let patterns = (0x3c96u16, 0xe14du16);
    let budget = cpu.0 / 100;

    // Plays what's loaded, sampling it; `Err` if anything didn't finish.
    let play = |awg: &mut Awg<T, T, (CompareLane<T, A, WA>, CompareLane<T, B, WB>), ()>,
                sampler: &mut Sampler<U, SU>,
                lanes: u8|
     -> Result<(), Fail> {
        let unfinished = Fail::Count {
            expected: u32::from(lanes.count_ones()),
            got: 0,
        };
        sampler.start(count).map_err(|_| Fail::Count {
            expected: count as u32,
            got: 0,
        })?;
        if awg.play().is_err() {
            sampler.abort();
            awg.abort();
            return Err(unfinished);
        }
        // Each service reports every lane that ended since the last, as a
        // mask; wait until the loaded lanes have all ended.
        let mut ended = 0u8;
        let all = wait(budget, || match awg.on_interrupt() {
            Some(Ok(mask)) => {
                ended |= mask;
                (ended & lanes as u8 == lanes as u8).then_some(true)
            }
            Some(Err(_)) => Some(false),
            None => None,
        });
        if all != Some(true) {
            sampler.abort();
            awg.abort();
            return Err(unfinished);
        }
        match wait(budget, || sampler.on_interrupt()) {
            Some(Ok(_)) => Ok(()),
            _ => Err(unfinished),
        }
    };

    // Each lane its own waveform, from one start.
    let loaded = load(awg, 0, &duties(patterns.0, period)).is_ok()
        && load(awg, 1, &duties(patterns.1, period)).is_ok();
    let played = if loaded {
        play(awg, sampler, 0b11)
    } else {
        Err(Fail::Count {
            expected: 2,
            got: 0,
        })
    };
    report.check(
        name,
        "AWG: each lane plays its own samples [AWG-001 AWG-002]",
        played.and_then(|()| {
            decode(sampler.samples(), pins.0, patterns.0, per_bit16, false)?;
            decode(sampler.samples(), pins.1, patterns.1, per_bit16, false)
        }),
    );

    // Samples hold their duty for one period each: a ramp reads back rising.
    let ramp: [u32; 7] = core::array::from_fn(|i| period * (2 + 2 * i as u32) / 16);
    let played = match load(awg, 0, &ramp) {
        Ok(()) => play(awg, sampler, 0b01),
        Err(_) => Err(Fail::Count {
            expected: 1,
            got: 0,
        }),
    };
    report.check(
        name,
        "AWG: a ramp reads back in order, and ends low [AWG-001 AWG-003]",
        played.and_then(|()| {
            let mut found = [(0, 0); 32];
            let pulses = pulses(sampler.samples(), pins.0, false, &mut found);
            if pulses != ramp.len() {
                return Err(Fail::Count {
                    expected: ramp.len() as u32,
                    got: pulses as u32,
                });
            }
            let rising = found[..pulses]
                .windows(2)
                .all(|pair| pair[1].1 >= pair[0].1);
            let wider = found[pulses - 1].1 > found[0].1;
            let low = sampler
                .samples()
                .last()
                .is_some_and(|sample| sample & (1 << pins.0) == 0);
            if rising && wider && low {
                Ok(())
            } else {
                Err(Fail::Count {
                    expected: 1,
                    got: 0,
                })
            }
        }),
    );

    // One lane alone leaves the other low.
    let played = match load(awg, 1, &duties(patterns.1, period)) {
        Ok(()) => play(awg, sampler, 0b10),
        Err(_) => Err(Fail::Count {
            expected: 1,
            got: 0,
        }),
    };
    report.check(
        name,
        "AWG: an unloaded lane stays low [AWG-002]",
        played.and_then(|()| {
            let high = sampler
                .samples()
                .iter()
                .filter(|s| *s & (1 << pins.0) != 0)
                .count();
            if high != 0 {
                return Err(Fail::Count {
                    expected: 0,
                    got: high as u32,
                });
            }
            decode(sampler.samples(), pins.1, patterns.1, per_bit16, false)
        }),
    );

    // Refusals, and a clean run after an abort.
    let nothing = awg.play().is_err();
    let past_end = load(awg, 2, &[1, 2]).is_err();
    let _ = load(awg, 0, &duties(patterns.0, period));
    let started = awg.play().is_ok();
    let while_playing = load(awg, 1, &[1, 2]).is_err() && awg.play().is_err();
    awg.abort();
    let idle = !awg.is_busy();
    report.check(
        name,
        "AWG: refuses what it can't do, and an abort leaves it ready [AWG-004 AWG-007]",
        if nothing && past_end && started && while_playing && idle {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 5,
                got: u32::from(nothing)
                    + u32::from(past_end)
                    + u32::from(started)
                    + u32::from(while_playing)
                    + u32::from(idle),
            })
        },
    );
}

/// A generator on two timers, however it starts the second, plays a waveform
/// on every lane of both and every lane finishes, ten rounds over [AWG-005].
/// It needs no wire: finishing is what the requirement asks, and a second
/// timer that never started would leave its lanes unfinished.
pub fn awg_pair_check<A: ChannelTimer, B: ChannelTimer, LA: Lanes<A>, LB: Lanes<B>>(
    report: &mut Report,
    name: &str,
    awg: &mut Awg<A, B, LA, LB>,
    cpu: Hertz,
) {
    const ROUNDS: u32 = 10;
    let lanes = awg.lanes();
    let budget = cpu.0 / 100;
    let mut done = 0;
    for round in 0..ROUNDS {
        let samples = duties(0x8421u16.rotate_left(round), awg.period());
        let loaded = (0..lanes).all(|lane| load(awg, lane, &samples).is_ok());
        if !loaded || awg.play().is_err() {
            awg.abort();
            continue;
        }
        // Every lane polled in turn until all have finished: one finishing
        // first must not hide another that never starts.
        let start = DWT::cycle_count();
        let mut finished = 0u32;
        let mut failed = false;
        while finished.count_ones() < lanes as u32
            && DWT::cycle_count().wrapping_sub(start) < budget
        {
            match awg.on_interrupt() {
                Some(Ok(ended)) => finished |= u32::from(ended),
                Some(Err(_)) => {
                    failed = true;
                    break;
                }
                None => {}
            }
        }
        if finished.count_ones() == lanes as u32 && !failed {
            done += 1;
        } else {
            awg.abort();
        }
    }
    report.check(
        name,
        "AWG: every lane on both timers finishes [AWG-005]",
        if done == ROUNDS {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: ROUNDS,
                got: done,
            })
        },
    );
}

/// Stops a two-lane generator mid-waveform, by `abort` and then by `free`,
/// and reopens its timer from the token `free` handed back. Each stop must
/// leave both lines quiet at once - every lane at duty 0, latched though the
/// timer is stopped - so nothing high reaches the sampler; and the token must
/// open the timer again at the generator's rate, with its period.
pub fn awg_free_check<
    T: ChannelTimer,
    U: ferrohal_stm32::timer::Instance,
    const A: u8,
    const B: u8,
    SU,
    WA,
    WB,
>(
    report: &mut Report,
    name: &str,
    awg: Awg<T, T, (CompareLane<T, A, WA>, CompareLane<T, B, WB>), ()>,
    pins: (u8, u8),
    sampler: &mut Sampler<U, SU>,
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    cpu: Hertz,
) {
    let mut awg = awg;
    let period = awg.period();
    let rate = awg.sample_rate();
    let mask = (1u16 << pins.0) | (1u16 << pins.1);
    // Every bit a 1, so each period is mostly high: a stop lands mid-pulse.
    let start = |awg: &mut Awg<T, T, (CompareLane<T, A, WA>, CompareLane<T, B, WB>), ()>| {
        load(awg, 0, &duties(0xffff, period)).is_ok()
            && load(awg, 1, &duties(0xffff, period)).is_ok()
            && awg.play().is_ok()
    };
    let quiet = |sampler: &mut Sampler<U, SU>, started: bool| -> Result<(), Fail> {
        if !started {
            return Err(Fail::Count {
                expected: 2,
                got: 0,
            });
        }
        sampler.start(1000).map_err(|_| Fail::Count {
            expected: 1000,
            got: 0,
        })?;
        if !matches!(wait(cpu.0 / 100, || sampler.on_interrupt()), Some(Ok(_))) {
            sampler.abort();
            return Err(Fail::Count {
                expected: 1000,
                got: 0,
            });
        }
        let high = sampler.samples().iter().filter(|s| *s & mask != 0).count() as u32;
        if high != 0 {
            return Err(Fail::Count {
                expected: 0,
                got: high,
            });
        }
        Ok(())
    };

    let started = start(&mut awg);
    awg.abort();
    report.check(
        name,
        "AWG: abort mid-waveform holds both lines low [AWG-007]",
        quiet(sampler, started),
    );

    let started = start(&mut awg);
    let (pwm, _, (lane_a, lane_b), (), _) = awg.free();
    report.check(
        name,
        "AWG: free mid-waveform holds both lines low [API-002]",
        quiet(sampler, started),
    );
    // The lanes free to their wirings and lent frames, the timer to its
    // token; the frames go back to the bench's memory.
    for frame in [
        lane_a.free(&pwm).ok().and_then(|(_, frame, _)| frame),
        lane_b.free(&pwm).ok().and_then(|(_, frame, _)| frame),
    ] {
        reclaim(frame);
    }
    let token = pwm.free();
    report.check(
        name,
        "the freed token opens the timer again, at the generator's period [API-002]",
        match Pwm::new(rcc, token, rate) {
            Ok(pwm) if pwm.period() == period => {
                let _ = pwm.free();
                Ok(())
            }
            Ok(pwm) => {
                let got = pwm.period();
                let _ = pwm.free();
                Err(Fail::Count {
                    expected: period,
                    got,
                })
            }
            Err(refused) => Err(Fail::Timer(refused.error())),
        },
    );
}

/// The waveform the loopback checks send: duties in sixteenths of full scale, with
/// a 0 in the middle. It has to start with a pulse, since time runs from there.
const WAVE: [u32; 8] = [8, 4, 12, 0, 14, 2, 10, 6];
/// A different one, for the other direction on one wire.
const REPLY: [u32; 8] = [4, 12, 2, 14, 0, 10, 6, 8];
/// How fast a loopback check reads, and how many captures it takes: the waveform
/// and a few samples' worth either side.
const CAPTURE_RATE: Hertz = Hertz::mhz(6);
const CAPTURES: usize = 800;

fn scaled(wave: &[u32; 8], period: u32) -> [u32; 8] {
    core::array::from_fn(|i| period * wave[i] / 16)
}

/// Compares what was read with what was sent, each sample within a sixteenth of
/// full scale.
fn recovered(
    captured: &[u16],
    pin: u8,
    active_low: bool,
    capture_rate: Hertz,
    sample_rate: Hertz,
    period: u32,
    sent: &[u32; 8],
) -> Result<(), Fail> {
    let per_sample16 = (u64::from(capture_rate.0) * 16 / u64::from(sample_rate.0)) as u32;
    let mut out = [0; 8];
    let found =
        ferrohal_stm32::timer::recover(captured, pin, active_low, per_sample16, period, &mut out);
    if found != sent.len() {
        return Err(Fail::Count {
            expected: sent.len() as u32,
            got: found as u32,
        });
    }
    for (index, (got, want)) in out.iter().zip(sent).enumerate() {
        if got.abs_diff(*want) > period / 16 {
            defmt::info!(
                "sample {}: sent {}, read {}, of {}",
                index,
                want,
                got,
                period
            );
            return Err(Fail::Count {
                expected: *want,
                got: *got,
            });
        }
    }
    Ok(())
}

/// Plays `wave` on one lane of `awg`, waiting for it to end.
fn play_one<T: ChannelTimer, const C: u8, WC>(
    awg: &mut Awg<T, T, (CompareLane<T, C, WC>,), ()>,
    samples: &[u32; 8],
    budget: u32,
) -> Result<(), Fail> {
    let failed = Fail::Count {
        expected: 1,
        got: 0,
    };
    if load(awg, 0, samples).is_err() || awg.play().is_err() {
        awg.abort();
        return Err(failed);
    }
    if matches!(
        wait(budget, || awg.on_interrupt().map(|r| r.map(|_| ()))),
        Some(Ok(()))
    ) {
        Ok(())
    } else {
        awg.abort();
        Err(failed)
    }
}

/// Out over a wire: `awg`'s lane plays, and `sampler` reads the far end of the
/// wire, on bit `far_pin` of its port.
pub fn awg_wire_out_check<
    T: ChannelTimer,
    U: ferrohal_stm32::timer::Instance,
    const C: u8,
    SU,
    WC,
>(
    report: &mut Report,
    name: &str,
    awg: &mut Awg<T, T, (CompareLane<T, C, WC>,), ()>,
    sampler: &mut Sampler<U, SU>,
    far_pin: u8,
    cpu: Hertz,
) {
    let sent = scaled(&WAVE, awg.period());
    let budget = cpu.0 / 100;
    let result = (|| {
        sampler.start(CAPTURES).map_err(|_| Fail::Count {
            expected: CAPTURES as u32,
            got: 0,
        })?;
        play_one(awg, &sent, budget)?;
        if !matches!(wait(budget, || sampler.on_interrupt()), Some(Ok(_))) {
            sampler.abort();
            return Err(Fail::Count {
                expected: CAPTURES as u32,
                got: 0,
            });
        }
        recovered(
            sampler.samples(),
            far_pin,
            false,
            sampler.rate(),
            awg.sample_rate(),
            awg.period(),
            &sent,
        )
    })();
    report.check(
        name,
        "AWG out over the wire: samples read back at the far end [AWG-008]",
        result,
    );

    // With the sender's output off, nothing crosses: the far end reads
    // only its pull-up, the whole capture through.
    awg.set_outputs(false);
    let result = (|| {
        sampler.start(CAPTURES).map_err(|_| Fail::Count {
            expected: CAPTURES as u32,
            got: 0,
        })?;
        play_one(awg, &sent, budget)?;
        if !matches!(wait(budget, || sampler.on_interrupt()), Some(Ok(_))) {
            sampler.abort();
            return Err(Fail::Count {
                expected: CAPTURES as u32,
                got: 0,
            });
        }
        let pulled_low = sampler
            .samples()
            .iter()
            .filter(|sample| *sample & (1 << far_pin) == 0)
            .count();
        if pulled_low == 0 {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 0,
                got: pulled_low as u32,
            })
        }
    })();
    report.check(
        name,
        "AWG: with the output off, nothing crosses the wire [AWG-011]",
        result,
    );
    awg.set_outputs(true);
}

/// In over a wire: `far` plays active low, and `near` listens on its own lane's
/// pin, at bit `near_pin` of its port.
pub fn awg_wire_in_check<N: ChannelTimer, const NC: u8, F: ChannelTimer, const FC: u8, WFC, WNC>(
    report: &mut Report,
    name: &str,
    near: &mut Awg<N, N, (CompareLane<N, NC, WNC>,), ()>,
    near_pin: u8,
    far: &mut Awg<F, F, (CompareLane<F, FC, WFC>,), ()>,
    cpu: Hertz,
) {
    near.set_active_low(true);
    far.set_active_low(true);
    let sent = scaled(&REPLY, far.period());
    let budget = cpu.0 / 100;
    let result = (|| {
        let rate = near.prepare_listen(CAPTURE_RATE, CAPTURES).and_then(|rate| near.listen().map(|()| rate))
            .map_err(|_| Fail::Count {
                expected: CAPTURES as u32,
                got: 0,
            })?;
        play_one(far, &sent, budget)?;
        if !matches!(
            wait(budget, || near.on_interrupt().map(|r| r.map(|_| ()))),
            Some(Ok(()))
        ) {
            near.abort();
            return Err(Fail::Count {
                expected: CAPTURES as u32,
                got: 0,
            });
        }
        recovered(
            near.samples(0),
            near_pin,
            true,
            rate,
            far.sample_rate(),
            far.period(),
            &sent,
        )
    })();
    report.check(
        name,
        "AWG in over the wire: a lane reads the samples on its own pin [AWG-009]",
        result,
    );
    report.check(
        name,
        "AWG: a lane turns back to sending after listening [AWG-009]",
        near.speak().map_err(|_| Fail::Count {
            expected: 1,
            got: 0,
        }),
    );
}

/// One wire, both ways, both ends two-way and active low, as a bidirectional
/// protocol runs: `near` sends and `far` listens, then they swap. And with `near`'s
/// output off, nothing crosses.
pub fn awg_two_way_check<N: ChannelTimer, const NC: u8, F: ChannelTimer, const FC: u8, WFC, WNC>(
    report: &mut Report,
    name: &str,
    near: &mut Awg<N, N, (CompareLane<N, NC, WNC>,), ()>,
    near_pin: u8,
    far: &mut Awg<F, F, (CompareLane<F, FC, WFC>,), ()>,
    far_pin: u8,
    cpu: Hertz,
) {
    near.set_active_low(true);
    far.set_active_low(true);
    let budget = cpu.0 / 100;
    let short = |expected: usize| Fail::Count {
        expected: expected as u32,
        got: 0,
    };

    let sent = scaled(&WAVE, near.period());
    let result = (|| {
        let rate = far.prepare_listen(CAPTURE_RATE, CAPTURES).and_then(|rate| far.listen().map(|()| rate))
            .map_err(|_| short(CAPTURES))?;
        play_one(near, &sent, budget)?;
        if !matches!(
            wait(budget, || far.on_interrupt().map(|r| r.map(|_| ()))),
            Some(Ok(()))
        ) {
            far.abort();
            return Err(short(CAPTURES));
        }
        recovered(
            far.samples(0),
            far_pin,
            true,
            rate,
            near.sample_rate(),
            near.period(),
            &sent,
        )
    })();
    report.check(
        name,
        "AWG out over the wire: the far lane reads the samples [AWG-008 AWG-009]",
        result,
    );
    let spoke = far.speak().map_err(|_| short(1));

    let answer = scaled(&REPLY, far.period());
    let result = spoke.and_then(|()| {
        let rate = near.prepare_listen(CAPTURE_RATE, CAPTURES).and_then(|rate| near.listen().map(|()| rate))
            .map_err(|_| short(CAPTURES))?;
        play_one(far, &answer, budget)?;
        if !matches!(
            wait(budget, || near.on_interrupt().map(|r| r.map(|_| ()))),
            Some(Ok(()))
        ) {
            near.abort();
            return Err(short(CAPTURES));
        }
        recovered(
            near.samples(0),
            near_pin,
            true,
            rate,
            far.sample_rate(),
            far.period(),
            &answer,
        )
    });
    report.check(
        name,
        "AWG in over the wire: the near lane reads the reply on its own pin [AWG-009]",
        result,
    );
    let spoke = near.speak().map_err(|_| short(1));

    // With the sender's output off, the far end reads only the pull-up.
    near.set_outputs(false);
    let result = spoke.and_then(|()| {
        far.prepare_listen(CAPTURE_RATE, CAPTURES).and_then(|rate| far.listen().map(|()| rate))
            .map_err(|_| short(CAPTURES))?;
        play_one(near, &sent, budget)?;
        if !matches!(
            wait(budget, || far.on_interrupt().map(|r| r.map(|_| ()))),
            Some(Ok(()))
        ) {
            far.abort();
            return Err(short(CAPTURES));
        }
        let pulled_low = far
            .samples(0)
            .iter()
            .filter(|sample| *sample & (1 << far_pin) == 0)
            .count();
        if pulled_low == 0 {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 0,
                got: pulled_low as u32,
            })
        }
    });
    report.check(
        name,
        "AWG: with the output off, nothing crosses the wire [AWG-011]",
        result,
    );
    near.set_outputs(true);
    let _ = far.speak();
}

/// What one reader round saw, for the caller's checks (AWG-012 to AWG-021).
#[derive(Clone, Copy)]
pub struct ReaderRound {
    /// The rate `prepare_listen` returned, and whether `prepared` named it.
    pub rate: Hertz,
    pub prepared_names_it: bool,
    /// Right after the frame's end: the counter still enabled and the
    /// lane's compare at 0, before any listen (`at_end` off).
    pub ran_on: bool,
    pub duty_zero: bool,
    /// With `at_end`, the run that saw the end had turned to listening.
    pub listening_at_end: bool,
    /// The lane's CCER bits before the listen (output on), while listening
    /// (output off, polarity bits clear) and after `speak` (on, active low).
    pub on_before: bool,
    pub released_clear: bool,
    pub restored: bool,
    /// The reply was read into the lent memory, and both the frame and the
    /// lent memory came back.
    pub read_lent: bool,
    pub frame_back: bool,
    pub samples_back: bool,
}

/// A timer's state as the bench reads it from its registers: CR1.CEN, the
/// lane channel's compare register and CCER.
#[derive(Clone, Copy)]
pub struct TimerRegs {
    pub enabled: bool,
    pub compare: u32,
    pub ccer: u32,
}

/// One reader round over one wire: `near` plays a frame from lent memory
/// and turns to read, in the reader form the caller set up (its own stream,
/// armed ahead, or paced), into lent memory; `far` answers once `near`'s
/// frame has ended; the reply must come back whole. `regs` reads `near`'s
/// timer. The frame and lent memory go in and come back through the slots.
#[allow(clippy::too_many_arguments)]
pub fn awg_reader_round<N: ChannelTimer, const NC: u8, WNC, F: ChannelTimer, const FC: u8, WFC>(
    near: &mut Awg<N, N, (CompareLane<N, NC, WNC>,), ()>,
    near_pin: u8,
    far: &mut Awg<F, F, (CompareLane<F, FC, WFC>,), ()>,
    frame_slot: &mut Option<&'static mut [u32]>,
    rx_slot: &mut Option<&'static mut [u16]>,
    at_end: bool,
    regs: &dyn Fn() -> TimerRegs,
    cpu: Hertz,
) -> Result<ReaderRound, Fail> {
    let budget = cpu.0 / 100;
    let short = |expected: usize| Fail::Count {
        expected: expected as u32,
        got: 0,
    };
    let shift = 4 * u32::from((NC - 1) & 3);
    let (cce, ccp, ccnp) = (1u32 << shift, 1u32 << (shift + 1), 1u32 << (shift + 3));
    let mut seen = ReaderRound {
        rate: Hertz(0),
        prepared_names_it: false,
        ran_on: false,
        duty_zero: false,
        listening_at_end: false,
        on_before: false,
        released_clear: false,
        restored: false,
        read_lent: false,
        frame_back: false,
        samples_back: false,
    };

    let (Some(words), Some(rx)) = (frame_slot.take(), rx_slot.take()) else {
        return Err(short(1));
    };
    let rx_at = rx.as_ptr() as usize;
    let limits = near.frame_limits(0).ok_or(short(1))?;
    let wave = scaled(&WAVE, near.period());
    for (slot, duty) in words.iter_mut().zip(wave.iter()) {
        *slot = *duty;
    }
    let frame = match ferrohal_stm32::timer::Frame::new(words, wave.len(), limits) {
        Ok(frame) => frame,
        Err(refused) => {
            *frame_slot = Some(refused.into_parts());
            *rx_slot = Some(rx);
            return Err(short(2));
        }
    };
    if let Err(refused) = near.lend_samples(0, rx) {
        *rx_slot = Some(refused.into_parts());
        *frame_slot = Some(frame.into_words());
        return Err(short(3));
    }
    let answer = scaled(&REPLY, far.period());
    let result = (|| -> Result<(), Fail> {
        seen.rate = near.prepare_listen(CAPTURE_RATE, CAPTURES).map_err(|_| short(4))?;
        seen.prepared_names_it = near.prepared() == Some((CAPTURE_RATE, CAPTURES, seen.rate));
        if load(far, 0, &answer).is_err() {
            return Err(short(5));
        }
        if at_end {
            near.listen_on_end();
        }
        if let Err(refused) = near.load(0, frame) {
            *frame_slot = Some(refused.into_parts().into_words());
            return Err(short(6));
        }
        near.play().map_err(|_| short(7))?;
        if !matches!(wait(budget, || near.on_interrupt().map(|r| r.map(|_| ()))), Some(Ok(()))) {
            return Err(short(8));
        }
        let after = regs();
        if at_end {
            seen.listening_at_end = near.is_listening();
        } else {
            seen.ran_on = after.enabled;
            seen.duty_zero = after.compare == 0;
            seen.on_before = after.ccer & cce != 0;
            near.listen().map_err(|_| short(9))?;
        }
        let released = regs().ccer;
        seen.released_clear = released & (cce | ccp | ccnp) == 0;
        far.play().map_err(|_| short(10))?;
        if !matches!(wait(budget, || far.on_interrupt().map(|r| r.map(|_| ()))), Some(Ok(()))) {
            return Err(short(11));
        }
        if !matches!(wait(budget, || near.on_interrupt().map(|r| r.map(|_| ()))), Some(Ok(()))) {
            return Err(short(CAPTURES));
        }
        let read = near.samples(0);
        seen.read_lent = read.as_ptr() as usize == rx_at;
        recovered(read, near_pin, true, seen.rate, far.sample_rate(), far.period(), &answer)
    })();
    if result.is_err() {
        near.abort();
        far.abort();
    }
    let _ = near.speak();
    let restored = regs().ccer;
    seen.restored = restored & cce != 0 && restored & ccp != 0;
    match near.take_frame(0) {
        Some(frame) => {
            seen.frame_back = true;
            *frame_slot = Some(frame.into_words());
        }
        None => seen.frame_back = false,
    }
    match near.take_samples(0) {
        Some(rx) => {
            seen.samples_back = true;
            *rx_slot = Some(rx);
        }
        None => seen.samples_back = false,
    }
    result.map(|()| seen)
}

/// A running PWM output's duty, read from `samples` of its pin: high for
/// `sixteenths`/16 of the time, to within 1/16.
pub fn duty_check<T: ferrohal_stm32::timer::Instance, ST>(
    report: &mut Report,
    name: &str,
    sampler: &mut Sampler<T, ST>,
    pin: u8,
    sixteenths: u32,
    cpu: Hertz,
) {
    report.check(
        name,
        "duty as set, to 1/16 [PWM-001]",
        measure_duty(sampler, pin, sixteenths, cpu),
    );
}

/// As [`duty_check`], for a channel driving its complementary output
/// (CHxN): the duty set on the channel, on the CHxN pin.
pub fn complementary_duty_check<T: ferrohal_stm32::timer::Instance, ST>(
    report: &mut Report,
    name: &str,
    sampler: &mut Sampler<T, ST>,
    pin: u8,
    sixteenths: u32,
    cpu: Hertz,
) {
    report.check(
        name,
        "the complementary output carries the duty as set, to 1/16 [PWM-008]",
        measure_duty(sampler, pin, sixteenths, cpu),
    );
}

fn measure_duty<T: ferrohal_stm32::timer::Instance, ST>(
    sampler: &mut Sampler<T, ST>,
    pin: u8,
    sixteenths: u32,
    cpu: Hertz,
) -> Result<(), Fail> {
    (|| {
        sampler.start(1000).map_err(|_| Fail::Count {
            expected: 1000,
            got: 0,
        })?;
        if !matches!(wait(cpu.0 / 100, || sampler.on_interrupt()), Some(Ok(_))) {
            sampler.abort();
            return Err(Fail::Count {
                expected: 1000,
                got: 0,
            });
        }
        let samples = sampler.samples();
        let high = samples.iter().filter(|s| *s & (1 << pin) != 0).count() as u32;
        let got16 = high * 16 / samples.len().max(1) as u32;
        if got16.abs_diff(sixteenths) > 1 {
            return Err(Fail::Count {
                expected: sixteenths,
                got: got16,
            });
        }
        Ok(())
    })()
}

/// One period from `Pwm::start_one_pulse`: the first period carries the set
/// duty, then the line holds still and the counter has stopped itself.
pub fn one_pulse_check<T: ChannelTimer, U: ferrohal_stm32::timer::Instance, SU>(
    report: &mut Report,
    name: &str,
    pwm: &mut Pwm<T>,
    pin: u8,
    sixteenths: u32,
    sampler: &mut Sampler<U, SU>,
    cpu: Hertz,
) {
    let per_period = (u64::from(sampler.rate().0) / u64::from(pwm.rate().0)).max(1) as usize;
    let count = per_period * 3;
    let result = (|| {
        pwm.stage(0);
        sampler.start(count).map_err(|_| Fail::Count {
            expected: count as u32,
            got: 0,
        })?;
        pwm.start_one_pulse();
        if !matches!(wait(cpu.0 / 100, || sampler.on_interrupt()), Some(Ok(_))) {
            sampler.abort();
            return Err(Fail::Count {
                expected: count as u32,
                got: 0,
            });
        }
        let samples = sampler.samples();
        // The first period, as sampled on the pin.
        let first = &samples[..per_period.min(samples.len())];
        let high = first.iter().filter(|s| *s & (1 << pin) != 0).count() as u32;
        let got16 = high * 16 / first.len().max(1) as u32;
        if got16.abs_diff(sixteenths) > 2 {
            return Err(Fail::Count {
                expected: sixteenths,
                got: got16,
            });
        }
        // Nothing after it but the one edge back to the stopped level.
        let tail = &samples[first.len().min(samples.len())..];
        let mut edges = 0;
        let mut before = first.last().map(|s| s & (1 << pin) != 0).unwrap_or(false);
        for sample in tail {
            let now = sample & (1 << pin) != 0;
            if now != before {
                edges += 1;
                before = now;
            }
        }
        if edges > 1 {
            return Err(Fail::Count {
                expected: 1,
                got: edges,
            });
        }
        if pwm.is_running() {
            return Err(Fail::Level {
                expected_high: false,
            });
        }
        Ok(())
    })();
    report.check(name, "one period, then the counter stops [PWM-007]", result);
}

/// One timer starting another: the slave sits still until the master starts,
/// then runs within `max_skew` counts of it, frame after frame. No outputs:
/// these counters drive no pin.
pub fn trigger_suite<M, S>(
    report: &mut Report,
    name: &str,
    master: &mut Pwm<M>,
    slave: &mut Pwm<S>,
    max_skew: u32,
) where
    M: ChannelTimer,
    S: ferrohal_stm32::timer::TriggeredBy<M>,
{
    master.lead();
    slave.follow(master);
    cortex_m::asm::delay(10_000);
    // A stopped timer keeps its count, so "not counting" is the running bit,
    // not a zero counter: the slave may arrive here having run before.
    let still = !slave.is_running();
    report.check(
        name,
        "the slave waits for the master [PWM-004]",
        if still {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 0,
                got: slave.counter(),
            })
        },
    );
    let mut worst = 0u32;
    let mut stopped = 0u32;
    for _ in 0..100 {
        master.stop();
        slave.stop();
        master.stage(0);
        slave.stage(0);
        master.start();
        let (lead, follower) = (master.counter(), slave.counter());
        stopped += u32::from(!slave.is_running());
        // Either may just have wrapped from its staged reload value.
        let apart = lead.abs_diff(follower);
        worst = worst.max(apart.min(master.period().saturating_sub(apart)));
        cortex_m::asm::delay(1_000);
    }
    master.stop();
    slave.stop();
    report.check(
        name,
        "100 starts: the master's starts the slave [PWM-005]",
        if stopped == 0 {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 0,
                got: stopped,
            })
        },
    );
    report.check(
        name,
        "100 starts: they begin together [PWM-005]",
        if worst <= max_skew {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: max_skew,
                got: worst,
            })
        },
    );
    defmt::info!(
        "{=str}: at most {=u32} counts apart, read one after the other",
        name,
        worst
    );
}
