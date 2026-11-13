//! The ADC checks, shared by the bench firmwares: a link from a driven GPIO
//! into an analog pin gives both rails, and the internal reference a level
//! between them that the factory measured.

// Each run binary compiles this file and uses what it needs.
#![allow(dead_code)]

use ferrohal_stm32::{
    adc::{self, Adc, AdcDma, AdcScan, Channel, Config, Error, InternalChannels},
    rcc::Rcc,
};

use crate::tests::{Fail, Report};

/// A rail reads within this many parts in 100 of its end of the scale.
const RAIL_PERCENT: u32 = 3;
const READS: usize = 64;

fn expect(report: &mut Report, name: &str, what: &str, ok: bool, got: u32, wanted: u32) {
    let result = if ok {
        Ok(())
    } else {
        Err(Fail::Count {
            expected: wanted,
            got,
        })
    };
    report.check(name, what, result);
}

fn is_low(sample: u16, max: u16) -> bool {
    u32::from(sample) * 100 <= u32::from(max) * RAIL_PERCENT
}

fn is_high(sample: u16, max: u16) -> bool {
    u32::from(sample) * 100 >= u32::from(max) * (100 - RAIL_PERCENT)
}

fn worst<A: adc::Instance>(
    adc: &mut Adc<A>,
    channel: Channel<A>,
    pick_higher: bool,
) -> Result<u16, Error> {
    let mut sample = adc.read(channel)?;
    for _ in 1..READS {
        let next = adc.read(channel)?;
        sample = if pick_higher {
            sample.max(next)
        } else {
            sample.min(next)
        };
    }
    Ok(sample)
}

/// `drive` sets the level on the link into `channel`'s pin.
pub fn rails<A: adc::Instance>(
    report: &mut Report,
    name: &str,
    adc: &mut Adc<A>,
    channel: Channel<A>,
    settle_cycles: u32,
    mut drive: impl FnMut(bool),
) {
    let max = adc.max_sample();
    drive(false);
    cortex_m::asm::delay(settle_cycles);
    let low = worst(adc, channel, true);
    expect(
        report,
        name,
        "a driven low reads near 0 [ADC-001 ADC-002]",
        matches!(low, Ok(sample) if is_low(sample, max)),
        low.unwrap_or(u16::MAX).into(),
        0,
    );
    drive(true);
    cortex_m::asm::delay(settle_cycles);
    let high = worst(adc, channel, false);
    expect(
        report,
        name,
        "a driven high reads near full scale [ADC-002]",
        matches!(high, Ok(sample) if is_high(sample, max)),
        high.unwrap_or(0).into(),
        max.into(),
    );
    defmt::info!(
        "{=str}: low {}, high {} of {=u16}",
        name,
        low.unwrap_or(0),
        high.unwrap_or(0),
        max
    );
    drive(false);
    cortex_m::asm::delay(settle_cycles);
    let back = adc.read(channel);
    expect(
        report,
        name,
        "it follows the pin back down [ADC-002]",
        matches!(back, Ok(sample) if is_low(sample, max)),
        back.unwrap_or(u16::MAX).into(),
        0,
    );
}

pub fn internal<A: InternalChannels>(report: &mut Report, name: &str, adc: &mut Adc<A>) {
    let vdda = adc.vdda_millivolts();
    let mv = vdda.unwrap_or(0);
    expect(
        report,
        name,
        "VDDA from the internal reference is about 3.3 V [ADC-003]",
        (3000..=3600).contains(&mv),
        mv,
        3300,
    );
    let channel = adc.vrefint_channel();
    let lowest = worst(adc, channel, false).unwrap_or(0);
    let highest = worst(adc, channel, true).unwrap_or(u16::MAX);
    let spread = u32::from(highest.saturating_sub(lowest));
    expect(
        report,
        name,
        "the reference holds still, within 1 percent of scale [ADC-003]",
        spread * 100 <= u32::from(adc.max_sample()),
        spread,
        0,
    );
    let reference = adc.to_millivolts(highest, mv);
    expect(
        report,
        name,
        "the reference reads about 1.2 V [ADC-003]",
        (1150..=1280).contains(&reference),
        reference,
        1210,
    );
    let temperature = adc.temperature_decidegrees();
    let tenths = temperature.unwrap_or(-1000);
    expect(
        report,
        name,
        "the die is between 10 and 70 C [ADC-003]",
        (100..=700).contains(&tenths),
        tenths.unsigned_abs(),
        300,
    );
    defmt::info!(
        "{=str}: VDDA {=u32} mV, reference {=u32} mV, die {=i32} tenths of a degree",
        name,
        mv,
        reference,
        tenths
    );
}

/// `check` judges one finished sequence; `drive` sets the link's level.
pub fn scan<A: adc::Instance, S>(
    report: &mut Report,
    name: &str,
    scan: &mut AdcScan<A, S>,
    settle_cycles: u32,
    bound_cycles: u32,
    mut drive: impl FnMut(bool),
    check: impl Fn(&[u16], bool, u16) -> bool,
) {
    let max = scan.max_sample();
    let mut finished = 0u32;
    let mut failures = 0u32;
    let mut refused = true;
    for round in 0..64 {
        let level = round % 2 == 1;
        drive(level);
        cortex_m::asm::delay(settle_cycles);
        if scan.start().is_err() {
            failures += 1;
            continue;
        }
        refused &= scan.start() == Err(Error::Busy);
        let mut spins = bound_cycles / 8;
        let result = loop {
            if let Some(done) = scan.poll() {
                break Some(match done {
                    Ok(samples) => check(samples, level, max),
                    Err(_) => false,
                });
            }
            if spins == 0 {
                scan.abort();
                break None;
            }
            spins -= 1;
        };
        match result {
            Some(true) => finished += 1,
            _ => failures += 1,
        }
    }
    expect(
        report,
        name,
        "64 sequences over DMA follow the pin [ADC-004]",
        failures == 0,
        finished,
        64,
    );
    expect(
        report,
        name,
        "a second start while running is refused [ADC-005]",
        refused,
        0,
        1,
    );
    drive(false);
}

/// One sequence, polled within `bound_cycles`.
fn once<A: adc::Instance, S>(scan: &mut AdcScan<A, S>, bound_cycles: u32) -> bool {
    if scan.start().is_err() {
        return false;
    }
    for _ in 0..bound_cycles / 8 {
        if let Some(done) = scan.poll() {
            return done.is_ok_and(|samples| !samples.is_empty());
        }
    }
    scan.abort();
    false
}

/// Frees `scan` back to the ADC, its stream and its buffer, runs a sequence
/// from those parts, then frees the ADC and reopens it from its token.
/// `powered` reads the ADC's own enable bit, so the check sees it off.
pub fn freed<A: InternalChannels, S: AdcDma<A>>(
    report: &mut Report,
    name: &str,
    rcc: &mut Rcc,
    scan: AdcScan<A, S>,
    channels: &[Channel<A>],
    bound_cycles: u32,
    powered: impl Fn() -> bool,
) {
    let (adc, stream, buffer) = scan.free();
    let adc = match adc.into_scan(stream, channels, buffer) {
        Ok(mut again) => {
            let ran = once(&mut again, bound_cycles);
            expect(
                report,
                name,
                "a freed scan's ADC, stream and buffer scan again [API-002]",
                ran,
                u32::from(ran),
                1,
            );
            let (adc, _stream, _buffer) = again.free();
            adc
        }
        Err(refused) => {
            expect(
                report,
                name,
                "a freed scan's ADC, stream and buffer scan again [API-002]",
                false,
                0,
                1,
            );
            let (adc, _stream, _buffer) = refused.into_parts();
            adc
        }
    };
    let token = adc.free();
    let on = powered();
    expect(report, name, "a freed ADC is powered down [API-002]", !on, u32::from(on), 0);
    let mv = match Adc::new(rcc, token, &Config::slow_source()) {
        Ok(mut adc) => {
            let mv = adc.vdda_millivolts().unwrap_or(0);
            let _token = adc.free();
            mv
        }
        Err(_) => 0,
    };
    expect(
        report,
        name,
        "its token reopens it, VDDA about 3.3 V [API-002]",
        (3000..=3600).contains(&mv),
        mv,
        3300,
    );
}
