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
    timer::{ChannelTimer, CompareLane, Pwm, Sampler},
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
        report.check(name, "both lanes and the sampler finish", Err(fail));
        sampler.stop();
        return;
    }
    report.check(name, "both lanes and the sampler finish", Ok(()));
    report.check(
        name,
        "first lane: its 16 bits, one period apart",
        decode(sampler.samples(), pins.0, patterns.0, per_bit16),
    );
    report.check(
        name,
        "second lane: its 16 bits, one period apart",
        decode(sampler.samples(), pins.1, patterns.1, per_bit16),
    );
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
        "duty as set, to 1/16",
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
