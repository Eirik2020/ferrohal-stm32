//! Bounded waits, shared by the drivers. FerroHAL owns no time source: a
//! bound is CPU cycles, measured by the caller's [`CycleClock`] when one is
//! set, otherwise counted as loop passes against the fewest cycles a pass
//! can take, so a counted bound is never shorter than asked.

use crate::time::CycleClock;

/// A wait's bound: `cycles` of CPU time, measured by the caller's clock if
/// there is one, otherwise counted as passes of at least `pass_cycles`.
pub(crate) struct Deadline {
    clock: Option<&'static dyn CycleClock>,
    start: u32,
    cycles: u32,
    passes: u32,
}

impl Deadline {
    pub(crate) fn new(clock: Option<&'static dyn CycleClock>, cycles: u32, pass_cycles: u32) -> Self {
        // Under half the clock's range, so a wrap is never mistaken for an
        // elapsed bound.
        let cycles = cycles.min(i32::MAX as u32);
        Self {
            clock,
            start: clock.map_or(0, |clock| clock.cycles()),
            cycles,
            passes: cycles / pass_cycles.max(1),
        }
    }

    /// Whether the bound has run out, counting this pass.
    pub(crate) fn expired(&mut self) -> bool {
        match self.clock {
            Some(clock) => clock.cycles().wrapping_sub(self.start) >= self.cycles,
            None => {
                if self.passes == 0 {
                    return true;
                }
                self.passes -= 1;
                false
            }
        }
    }
}

/// Polls until `done` or the deadline runs out.
pub(crate) fn wait(mut deadline: Deadline, mut done: impl FnMut() -> bool) -> bool {
    loop {
        if done() {
            return true;
        }
        if deadline.expired() {
            return done();
        }
    }
}
