//! The PWM, compare-lane and sampler checks, shared by the bench firmwares.
//! They need no wiring: a pin's input register reads its level while a
//! timer drives it, so a sampler records each output's own waveform.
//!
//! Lanes send 16-bit patterns the way DShot does, one bit per period: a 1
//! is a pulse of 3/4 of the period, a 0 one of 3/8. The checks decode the
//! samples back into bits and timing.

// Each run binary compiles this file and uses the checks it needs.
#![allow(dead_code)]

use cortex_m::peripheral::DWT;
use ferrohal_stm32::{
    time::Hertz,
    timer::{Awg, ChannelTimer, CompareLane, Pwm, Sampler},
};

use crate::tests::{Fail, Report};

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
>(
    report: &mut Report,
    name: &str,
    pwm: &mut Pwm<T>,
    lanes: (&mut CompareLane<T, A>, &mut CompareLane<T, B>),
    pins: (u8, u8),
    patterns: (u16, u16),
    sampler: &mut Sampler<U>,
    cpu: Hertz,
) {
    let period = pwm.period();
    // Samples per bit, in sixteenths.
    let per_bit16 = (u64::from(sampler.rate().0) * 16 / u64::from(pwm.frequency().0)) as u32;
    // Eight spare bits around the frame.
    let count = (per_bit16 as usize * (BITS + 8)) / 16;
    let result = (|| {
        lanes
            .0
            .arm(pwm, &duties(patterns.0, period))
            .map_err(|_| Fail::Count {
                expected: BITS as u32,
                got: 0,
            })?;
        lanes
            .1
            .arm(pwm, &duties(patterns.1, period))
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
        let first = wait(budget, || lanes.0.poll());
        let second = wait(budget, || lanes.1.poll());
        let sampled = wait(budget, || sampler.poll());
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
        sampler.stop();
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

/// The waveform generator on two lanes of one timer, read back by `sampler`:
/// each lane's own samples, a ramp, one lane alone, and what it refuses.
pub fn awg_check<T: ChannelTimer, U: ferrohal_stm32::timer::Instance, const A: u8, const B: u8>(
    report: &mut Report,
    name: &str,
    awg: &mut Awg<T, T, (CompareLane<T, A>, CompareLane<T, B>), ()>,
    pins: (u8, u8),
    sampler: &mut Sampler<U>,
    cpu: Hertz,
) {
    let period = awg.period();
    let per_bit16 = (u64::from(sampler.rate().0) * 16 / u64::from(awg.sample_rate().0)) as u32;
    let count = (per_bit16 as usize * (BITS + 8)) / 16;
    let patterns = (0x3c96u16, 0xe14du16);
    let budget = cpu.0 / 100;

    // Plays what's loaded, sampling it; `Err` if anything didn't finish.
    let play = |awg: &mut Awg<T, T, (CompareLane<T, A>, CompareLane<T, B>), ()>,
                sampler: &mut Sampler<U>,
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
            sampler.stop();
            awg.abort();
            return Err(unfinished);
        }
        for lane in 0..2 {
            if lanes & (1 << lane) != 0 && !matches!(wait(budget, || awg.poll(lane)), Some(Ok(())))
            {
                sampler.stop();
                awg.abort();
                return Err(unfinished);
            }
        }
        match wait(budget, || sampler.poll()) {
            Some(Ok(_)) => Ok(()),
            _ => Err(unfinished),
        }
    };

    // Each lane its own waveform, from one start.
    let loaded = awg.load(0, &duties(patterns.0, period)).is_ok()
        && awg.load(1, &duties(patterns.1, period)).is_ok();
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
    let played = match awg.load(0, &ramp) {
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
    let played = match awg.load(1, &duties(patterns.1, period)) {
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
    let past_end = awg.load(2, &[1, 2]).is_err();
    let _ = awg.load(0, &duties(patterns.0, period));
    let started = awg.play().is_ok();
    let while_playing = awg.load(1, &[1, 2]).is_err() && awg.play().is_err();
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

/// Frees a two-lane generator with a waveform playing, then reopens its timer
/// from the token `free` handed back. Both lines must go quiet - every lane at
/// duty 0 and the timer stopped, so nothing high reaches the sampler - and the
/// token must open the timer again at the generator's rate, with its period.
pub fn awg_free_check<
    T: ChannelTimer,
    U: ferrohal_stm32::timer::Instance,
    const A: u8,
    const B: u8,
>(
    report: &mut Report,
    name: &str,
    awg: Awg<T, T, (CompareLane<T, A>, CompareLane<T, B>), ()>,
    pins: (u8, u8),
    sampler: &mut Sampler<U>,
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    cpu: Hertz,
) {
    let mut awg = awg;
    let period = awg.period();
    let rate = awg.sample_rate();
    let loaded = awg.load(0, &duties(0xffff, period)).is_ok()
        && awg.load(1, &duties(0xffff, period)).is_ok()
        && awg.play().is_ok();
    let (token, _) = awg.free();
    report.check(
        name,
        "AWG: free stops a playing generator, both lines held low",
        (|| {
            if !loaded {
                return Err(Fail::Count {
                    expected: 2,
                    got: 0,
                });
            }
            sampler.start(1000).map_err(|_| Fail::Count {
                expected: 1000,
                got: 0,
            })?;
            if !matches!(wait(cpu.0 / 100, || sampler.poll()), Some(Ok(_))) {
                sampler.stop();
                return Err(Fail::Count {
                    expected: 1000,
                    got: 0,
                });
            }
            let mask = (1u16 << pins.0) | (1u16 << pins.1);
            let high = sampler.samples().iter().filter(|s| *s & mask != 0).count() as u32;
            if high != 0 {
                return Err(Fail::Count {
                    expected: 0,
                    got: high,
                });
            }
            Ok(())
        })(),
    );
    report.check(
        name,
        "the freed token opens the timer again, at the generator's period",
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
fn play_one<T: ChannelTimer, const C: u8>(
    awg: &mut Awg<T, T, (CompareLane<T, C>,), ()>,
    samples: &[u32; 8],
    budget: u32,
) -> Result<(), Fail> {
    let failed = Fail::Count {
        expected: 1,
        got: 0,
    };
    if awg.load(0, samples).is_err() || awg.play().is_err() {
        awg.abort();
        return Err(failed);
    }
    if matches!(wait(budget, || awg.poll(0)), Some(Ok(()))) {
        Ok(())
    } else {
        awg.abort();
        Err(failed)
    }
}

/// Out over a wire: `awg`'s lane plays, and `sampler` reads the far end of the
/// wire, on bit `far_pin` of its port.
pub fn awg_wire_out_check<T: ChannelTimer, U: ferrohal_stm32::timer::Instance, const C: u8>(
    report: &mut Report,
    name: &str,
    awg: &mut Awg<T, T, (CompareLane<T, C>,), ()>,
    sampler: &mut Sampler<U>,
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
        if !matches!(wait(budget, || sampler.poll()), Some(Ok(_))) {
            sampler.stop();
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
        if !matches!(wait(budget, || sampler.poll()), Some(Ok(_))) {
            sampler.stop();
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
pub fn awg_wire_in_check<N: ChannelTimer, const NC: u8, F: ChannelTimer, const FC: u8>(
    report: &mut Report,
    name: &str,
    near: &mut Awg<N, N, (CompareLane<N, NC>,), ()>,
    near_pin: u8,
    far: &mut Awg<F, F, (CompareLane<F, FC>,), ()>,
    cpu: Hertz,
) {
    near.set_active_low(true);
    far.set_active_low(true);
    let sent = scaled(&REPLY, far.period());
    let budget = cpu.0 / 100;
    let result = (|| {
        let rate = near
            .listen(CAPTURE_RATE, CAPTURES)
            .map_err(|_| Fail::Count {
                expected: CAPTURES as u32,
                got: 0,
            })?;
        play_one(far, &sent, budget)?;
        if !matches!(wait(budget, || near.poll(0)), Some(Ok(()))) {
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
pub fn awg_two_way_check<N: ChannelTimer, const NC: u8, F: ChannelTimer, const FC: u8>(
    report: &mut Report,
    name: &str,
    near: &mut Awg<N, N, (CompareLane<N, NC>,), ()>,
    near_pin: u8,
    far: &mut Awg<F, F, (CompareLane<F, FC>,), ()>,
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
        let rate = far
            .listen(CAPTURE_RATE, CAPTURES)
            .map_err(|_| short(CAPTURES))?;
        play_one(near, &sent, budget)?;
        if !matches!(wait(budget, || far.poll(0)), Some(Ok(()))) {
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
        let rate = near
            .listen(CAPTURE_RATE, CAPTURES)
            .map_err(|_| short(CAPTURES))?;
        play_one(far, &answer, budget)?;
        if !matches!(wait(budget, || near.poll(0)), Some(Ok(()))) {
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
        far.listen(CAPTURE_RATE, CAPTURES)
            .map_err(|_| short(CAPTURES))?;
        play_one(near, &sent, budget)?;
        if !matches!(wait(budget, || far.poll(0)), Some(Ok(()))) {
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

/// A running PWM output's duty, read from `samples` of its pin: high for
/// `sixteenths`/16 of the time, to within 1/16.
pub fn duty_check<T: ferrohal_stm32::timer::Instance>(
    report: &mut Report,
    name: &str,
    sampler: &mut Sampler<T>,
    pin: u8,
    sixteenths: u32,
    cpu: Hertz,
) {
    report.check(
        name,
        "duty as set, to 1/16 [PWM-001]",
        (|| {
            sampler.start(1000).map_err(|_| Fail::Count {
                expected: 1000,
                got: 0,
            })?;
            if !matches!(wait(cpu.0 / 100, || sampler.poll()), Some(Ok(_))) {
                sampler.stop();
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
        })(),
    );
}

/// One period from `Pwm::start_one_pulse`: the first period carries the set
/// duty, then the line holds still and the counter has stopped itself.
pub fn one_pulse_check<T: ChannelTimer, U: ferrohal_stm32::timer::Instance>(
    report: &mut Report,
    name: &str,
    pwm: &mut Pwm<T>,
    pin: u8,
    sixteenths: u32,
    sampler: &mut Sampler<U>,
    cpu: Hertz,
) {
    let per_period = (u64::from(sampler.rate().0) / u64::from(pwm.frequency().0)).max(1) as usize;
    let count = per_period * 3;
    let result = (|| {
        pwm.stage(0);
        sampler.start(count).map_err(|_| Fail::Count {
            expected: count as u32,
            got: 0,
        })?;
        pwm.start_one_pulse();
        if !matches!(wait(cpu.0 / 100, || sampler.poll()), Some(Ok(_))) {
            sampler.stop();
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
