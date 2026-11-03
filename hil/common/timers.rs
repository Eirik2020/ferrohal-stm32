//! The timer checks, shared by the bench firmwares. They need no wiring:
//! the DWT cycle counter, on the same oscillator as the timers, is the
//! reference every rate is measured against.
//!
//! The firmware keeps the running `Tick` in a [`TickBench`] shared with the
//! update vectors' handlers, which call [`TickBench::on_interrupt`].

// Each run binary compiles this file and uses the checks it needs.
#![allow(dead_code)]

use cortex_m::peripheral::DWT;
use ferrohal_stm32::{
    time::Hertz,
    timer::{Tick, Timebase},
};
use rtic::Mutex;

use crate::tests::{Fail, Report};

/// What the update handlers share with the checks task.
pub struct TickBench {
    pub tick: Option<Tick>,
    pub count: u32,
    /// DWT at the first and the latest tick.
    pub first: Option<u32>,
    pub last: u32,
}

impl TickBench {
    pub const fn new() -> Self {
        Self {
            tick: None,
            count: 0,
            first: None,
            last: 0,
        }
    }

    /// For every timer update vector's handler.
    pub fn on_interrupt(&mut self) {
        if let Some(tick) = self.tick.as_mut()
            && tick.on_interrupt()
        {
            let now = DWT::cycle_count();
            self.first.get_or_insert(now);
            self.last = now;
            self.count += 1;
        }
    }
}

fn spin_until(cycles: u32, mut done: impl FnMut() -> bool) -> bool {
    let start = DWT::cycle_count();
    while DWT::cycle_count().wrapping_sub(start) < cycles {
        if done() {
            return true;
        }
    }
    done()
}

/// `ticks` ticks of `tick`, timed against the DWT, then a pause that must
/// stop them. `tick` is dropped at the end, paused.
pub fn tick_check<B: Mutex<T = TickBench>>(
    report: &mut Report,
    name: &str,
    bench: &mut B,
    tick: Tick,
    cpu: Hertz,
    ticks: u32,
) {
    let rate = tick.rate().0;
    let period = cpu.0 / rate;
    report.check(
        name,
        "update vector enabled [TIMER-005]",
        tick.verify().map_err(|error| Fail::Irq(Some(error))),
    );
    bench.lock(|b| {
        b.count = 0;
        b.first = None;
        b.tick = Some(tick);
    });
    let arrived = spin_until(period.saturating_mul(ticks + 4), || {
        bench.lock(|b| b.count) > ticks
    });
    let (count, first, last) = bench.lock(|b| (b.count, b.first, b.last));
    report.check(
        name,
        "ticks at its rate, to 0.05 % of the DWT [TIMER-001 RCC-002]",
        (|| {
            if !arrived {
                return Err(Fail::Count {
                    expected: ticks + 1,
                    got: count,
                });
            }
            let periods = count - 1;
            let measured = last.wrapping_sub(first.unwrap_or(last));
            let expected = periods * period;
            // Interrupt latency moves single edges by a few hundred cycles;
            // the rate itself must agree to 1 part in 2000.
            let slack = expected / 2000 + 2_000;
            if measured.abs_diff(expected) > slack {
                return Err(Fail::Time {
                    cycles: measured,
                    min: expected - slack,
                    max: expected + slack,
                });
            }
            Ok(())
        })(),
    );
    report.check(
        name,
        "pause stops the ticks [TIMER-002]",
        (|| {
            bench.lock(|b| {
                if let Some(t) = b.tick.as_mut() {
                    t.pause();
                }
            });
            let paused = bench.lock(|b| b.count);
            spin_until(period.saturating_mul(4), || false);
            let after = bench.lock(|b| b.tick.take().map(|_| b.count));
            match after {
                Some(after) if after == paused => Ok(()),
                Some(after) => Err(Fail::Count {
                    expected: paused,
                    got: after,
                }),
                None => Err(Fail::Count {
                    expected: paused,
                    got: 0,
                }),
            }
        })(),
    );
}

/// `base` counts at its frequency, to 0.01 % of the DWT over 20 ms.
pub fn timebase_check(report: &mut Report, name: &str, base: &Timebase, cpu: Hertz) {
    let reader = base.reader();
    report.check(
        name,
        "counts at its frequency, to 0.01 % of the DWT [TIMER-003 RCC-002]",
        (|| {
            let window = cpu.0 / 50;
            let (start_count, start_cycles) = (reader.now(), DWT::cycle_count());
            spin_until(window, || false);
            let (end_count, end_cycles) = (reader.now(), DWT::cycle_count());
            let counted = end_count.wrapping_sub(start_count);
            let cycles = end_cycles.wrapping_sub(start_cycles);
            let expected =
                (u64::from(cycles) * u64::from(reader.frequency().0) / u64::from(cpu.0)) as u32;
            let slack = expected / 10_000 + 2;
            if counted.abs_diff(expected) > slack {
                return Err(Fail::Count {
                    expected,
                    got: counted,
                });
            }
            Ok(())
        })(),
    );
}
