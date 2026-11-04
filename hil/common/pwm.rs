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
fn pulses(samples: &[u16], pin: u8, out: &mut [(usize, usize); 32]) -> usize {
    let mut count = 0;
    let mut start = None;
    for (i, sample) in samples.iter().enumerate() {
        let high = sample & (1 << pin) != 0;
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
/// then nothing.
fn decode(samples: &[u16], pin: u8, pattern: u16, per_bit16: u32) -> Result<(), Fail> {
    let mut found = [(0, 0); 32];
    let count = pulses(samples, pin, &mut found);
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
fn wait<R>(cycles: u32, mut poll: impl FnMut() -> Option<R>) -> Option<R> {
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
pub fn lane_pair_check<T: ChannelTimer, const A: u8, const B: u8>(
    report: &mut Report,
    name: &str,
    pwm: &mut Pwm<T>,
    lanes: (&mut CompareLane<T, A>, &mut CompareLane<T, B>),
    pins: (u8, u8),
    patterns: (u16, u16),
    sampler: &mut Sampler,
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
            .arm(&duties(patterns.0, period))
            .map_err(|_| Fail::Count {
                expected: BITS as u32,
                got: 0,
            })?;
        lanes
            .1
            .arm(&duties(patterns.1, period))
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
            lanes.0.abort();
            lanes.1.abort();
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
        report.check(name, "both lanes and the sampler finish [PWM-003]", Err(fail));
        sampler.stop();
        return;
    }
    report.check(name, "both lanes and the sampler finish [PWM-003]", Ok(()));
    report.check(
        name,
        "first lane: its 16 bits, one period apart [PWM-002]",
        decode(sampler.samples(), pins.0, patterns.0, per_bit16),
    );
    report.check(
        name,
        "second lane: its 16 bits, one period apart [PWM-002]",
        decode(sampler.samples(), pins.1, patterns.1, per_bit16),
    );
}

/// The waveform generator on two lanes of one timer, read back by `sampler`:
/// each lane's own samples, a ramp, one lane alone, and what it refuses.
pub fn awg_check<T: ChannelTimer, const A: u8, const B: u8>(
    report: &mut Report,
    name: &str,
    awg: &mut Awg<T, T, (CompareLane<T, A>, CompareLane<T, B>), ()>,
    pins: (u8, u8),
    sampler: &mut Sampler,
    cpu: Hertz,
) {
    let period = awg.period();
    let per_bit16 = (u64::from(sampler.rate().0) * 16 / u64::from(awg.sample_rate().0)) as u32;
    let count = (per_bit16 as usize * (BITS + 8)) / 16;
    let patterns = (0x3c96u16, 0xe14du16);
    let budget = cpu.0 / 100;

    // Plays what's loaded, sampling it; `Err` if anything didn't finish.
    let play = |awg: &mut Awg<T, T, (CompareLane<T, A>, CompareLane<T, B>), ()>,
                    sampler: &mut Sampler,
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
            if lanes & (1 << lane) != 0 && !matches!(wait(budget, || awg.poll(lane)), Some(Ok(()))) {
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
        Err(Fail::Count { expected: 2, got: 0 })
    };
    report.check(name, "AWG: each lane plays its own samples [AWG-001 AWG-002]", played.and_then(|()| {
        decode(sampler.samples(), pins.0, patterns.0, per_bit16)?;
        decode(sampler.samples(), pins.1, patterns.1, per_bit16)
    }));

    // Samples hold their duty for one period each: a ramp reads back rising.
    let ramp: [u32; 7] = core::array::from_fn(|i| period * (2 + 2 * i as u32) / 16);
    let played = match awg.load(0, &ramp) {
        Ok(()) => play(awg, sampler, 0b01),
        Err(_) => Err(Fail::Count { expected: 1, got: 0 }),
    };
    report.check(name, "AWG: a ramp reads back in order, and ends low [AWG-001 AWG-003]", played.and_then(|()| {
        let mut found = [(0, 0); 32];
        let pulses = pulses(sampler.samples(), pins.0, &mut found);
        if pulses != ramp.len() {
            return Err(Fail::Count { expected: ramp.len() as u32, got: pulses as u32 });
        }
        let rising = found[..pulses].windows(2).all(|pair| pair[1].1 >= pair[0].1);
        let wider = found[pulses - 1].1 > found[0].1;
        let low = sampler.samples().last().is_some_and(|sample| sample & (1 << pins.0) == 0);
        if rising && wider && low {
            Ok(())
        } else {
            Err(Fail::Count { expected: 1, got: 0 })
        }
    }));

    // One lane alone leaves the other low.
    let played = match awg.load(1, &duties(patterns.1, period)) {
        Ok(()) => play(awg, sampler, 0b10),
        Err(_) => Err(Fail::Count { expected: 1, got: 0 }),
    };
    report.check(name, "AWG: an unloaded lane stays low [AWG-002]", played.and_then(|()| {
        let high = sampler.samples().iter().filter(|s| *s & (1 << pins.0) != 0).count();
        if high != 0 {
            return Err(Fail::Count { expected: 0, got: high as u32 });
        }
        decode(sampler.samples(), pins.1, patterns.1, per_bit16)
    }));

    // Refusals, and a clean run after an abort.
    let nothing = awg.play().is_err();
    let past_end = awg.load(2, &[1, 2]).is_err();
    let _ = awg.load(0, &duties(patterns.0, period));
    let started = awg.play().is_ok();
    let while_playing = awg.load(1, &[1, 2]).is_err() && awg.play().is_err();
    awg.abort();
    let idle = !awg.is_busy();
    report.check(name, "AWG: refuses what it can't do, and an abort leaves it ready [AWG-004 AWG-007]",
        if nothing && past_end && started && while_playing && idle {
            Ok(())
        } else {
            Err(Fail::Count { expected: 5, got: u32::from(nothing) + u32::from(past_end) + u32::from(started) + u32::from(while_playing) + u32::from(idle) })
        });
}

/// A running PWM output's duty, read from `samples` of its pin: high for
/// `sixteenths`/16 of the time, to within 1/16.
pub fn duty_check(
    report: &mut Report,
    name: &str,
    sampler: &mut Sampler,
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
    let still = !slave.is_running() && slave.counter() == 0;
    report.check(
        name,
        "the slave waits for the master [PWM-004]",
        if still {
            Ok(())
        } else {
            Err(Fail::Count { expected: 0, got: slave.counter() })
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
            Err(Fail::Count { expected: 0, got: stopped })
        },
    );
    report.check(
        name,
        "100 starts: they begin together [PWM-005]",
        if worst <= max_skew {
            Ok(())
        } else {
            Err(Fail::Count { expected: max_skew, got: worst })
        },
    );
    defmt::info!("{=str}: at most {=u32} counts apart, read one after the other", name, worst);
}
