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
    resource::IrqError,
    time::Hertz,
    timer::{Instance, Instance32, Tick, Timebase},
};
use rtic::Mutex;

use crate::tests::{Fail, Report};

/// The shared surface of a benched tick: implemented for [`Tick`] itself,
/// and for a board's enum of benched ticks ([`benched_ticks!`]). The timer
/// token stays sealed inside the handle, so a benched tick cannot be freed
/// while its vector is bound.
pub trait BenchedTick {
    /// Serves the tick's update interrupt: clears the flag and says whether
    /// it was set.
    fn on_interrupt(&mut self) -> bool;
    /// Stops counting; no more ticks until resumed.
    fn pause(&mut self);
    /// The tick's rate.
    fn rate(&self) -> Hertz;
    /// Checks the NVIC, reading only: the vector is enabled.
    fn verify(&self) -> Result<(), IrqError>;
    /// Stops the counter, kills its update interrupt and clears its flag.
    fn stop(&mut self);
    /// Sets the tick's update vector pending in the NVIC, as another timer
    /// sharing it would.
    fn pend(&self);
}

impl<T: Instance> BenchedTick for Tick<T> {
    fn on_interrupt(&mut self) -> bool {
        self.on_interrupt()
    }
    fn pause(&mut self) {
        self.pause()
    }
    fn rate(&self) -> Hertz {
        self.rate()
    }
    fn verify(&self) -> Result<(), IrqError> {
        self.vectors().verify()
    }
    fn stop(&mut self) {
        self.stop()
    }
    fn pend(&self) {
        self.vectors().iter().for_each(cortex_m::peripheral::NVIC::pend);
    }
}

/// Declares a board's benched ticks: one enum over the timers the run ticks,
/// each variant a [`Tick`] with the timer token sealed inside, with the
/// [`BenchedTick`] surface. A run that ticks more than one timer in turn
/// stores the enum in its [`TickBench`]; a run with a single tick stores the
/// `Tick` itself.
#[allow(unused_macros)]
macro_rules! benched_ticks {
    ($(#[$meta:meta])* $name:ident { $($variant:ident : $timer:ident),+ $(,)? }) => {
        $(#[$meta])*
        pub enum $name {
            $($variant(ferrohal_stm32::timer::Tick<$timer>),)+
        }

        impl crate::timers::BenchedTick for $name {
            fn on_interrupt(&mut self) -> bool {
                match self {
                    $(Self::$variant(tick) => tick.on_interrupt(),)+
                }
            }
            fn pause(&mut self) {
                match self {
                    $(Self::$variant(tick) => tick.pause(),)+
                }
            }
            fn rate(&self) -> ferrohal_stm32::time::Hertz {
                match self {
                    $(Self::$variant(tick) => tick.rate(),)+
                }
            }
            fn verify(&self) -> Result<(), ferrohal_stm32::resource::IrqError> {
                match self {
                    $(Self::$variant(tick) => tick.vectors().verify(),)+
                }
            }
            fn stop(&mut self) {
                match self {
                    $(Self::$variant(tick) => tick.stop(),)+
                }
            }
            fn pend(&self) {
                match self {
                    $(Self::$variant(tick) => tick
                        .vectors()
                        .iter()
                        .for_each(cortex_m::peripheral::NVIC::pend),)+
                }
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use benched_ticks;

/// What the update handlers share with the checks task. The benched tick
/// moves in and is stopped and dropped at the end of its check.
pub struct TickBench<T: BenchedTick> {
    pub tick: Option<T>,
    pub count: u32,
    /// Every pass of a handler, whether the tick answered or not.
    pub passes: u32,
    /// DWT at the first and the latest tick.
    pub first: Option<u32>,
    pub last: u32,
}

impl<T: BenchedTick> TickBench<T> {
    pub const fn new() -> Self {
        Self {
            tick: None,
            count: 0,
            passes: 0,
            first: None,
            last: 0,
        }
    }

    /// For every timer update vector's handler.
    pub fn on_interrupt(&mut self) {
        self.passes = self.passes.wrapping_add(1);
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
/// stop them. The tick moves into the bench, timer token sealed inside,
/// and comes back stopped, for the caller to drop or to `free` for a
/// second use from the same token.
pub fn tick_check<E, B>(
    report: &mut Report,
    name: &str,
    bench: &mut B,
    tick: E,
    cpu: Hertz,
    ticks: u32,
) -> E
where
    E: BenchedTick,
    B: Mutex<T = TickBench<E>>,
{
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
        "served twice for one update: true, then false [TIMER-006]",
        (|| {
            // Under the lock the update vector is masked: an update arrives
            // within two periods and waits, flag set, for this task to serve
            // it twice. The handler that runs after the lock finds the flag
            // clear, unless a later update has come.
            let (first, second) = bench.lock(|b| {
                spin_until(period.saturating_mul(2), || false);
                match b.tick.as_mut() {
                    Some(tick) => (tick.on_interrupt(), tick.on_interrupt()),
                    None => (false, false),
                }
            });
            match (first, second) {
                (true, false) => Ok(()),
                (first, second) => Err(Fail::Count {
                    expected: 0b10,
                    got: u32::from(first) << 1 | u32::from(second),
                }),
            }
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
            bench.lock(|b| {
                if let Some(tick) = b.tick.as_mut() {
                    tick.stop();
                }
            });
            let after = bench.lock(|b| b.tick.as_ref().map(|_| b.count));
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
    report.check(
        name,
        "a pass its timer did not raise reports false [TIMER-006]",
        (|| {
            // The tick is stopped, its flag clear: the vector pended by hand
            // is what another timer sharing it raises. The handler must run
            // and the tick must not count it.
            let (passes, count) = bench.lock(|b| (b.passes, b.count));
            bench.lock(|b| {
                if let Some(tick) = b.tick.as_ref() {
                    tick.pend();
                }
            });
            spin_until(period, || bench.lock(|b| b.passes) != passes);
            let (passes_after, count_after) = bench.lock(|b| (b.passes, b.count));
            if passes_after == passes {
                return Err(Fail::Count {
                    expected: passes.wrapping_add(1),
                    got: passes_after,
                });
            }
            if count_after != count {
                return Err(Fail::Count {
                    expected: count,
                    got: count_after,
                });
            }
            Ok(())
        })(),
    );
    bench
        .lock(|b| b.tick.take())
        .unwrap_or_else(|| defmt::unreachable!())
}

/// `base` counts at its frequency, to 0.01 % of the DWT over 20 ms.
pub fn timebase_check<T: Instance32>(
    report: &mut Report,
    name: &str,
    base: &Timebase<T>,
    cpu: Hertz,
) {
    let reader = base.handle();
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
