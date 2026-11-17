//! Bounded waits, shared by the drivers. FerroHAL owns no time source: a
//! bound is CPU cycles, measured by the caller's [`CycleClock`] when one is
//! set, otherwise counted as loop passes against the fewest cycles a pass
//! can take, so a counted bound is never shorter than asked.
//!
//! Requirements: WAIT-001

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
    pub(crate) fn new(
        clock: Option<&'static dyn CycleClock>,
        cycles: u32,
        pass_cycles: u32,
    ) -> Self {
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

    /// A bound of `passes` polls and no clock: for waits with no
    /// [`CycleClock`] to hand (the clock tree, the watchdog, a stream's
    /// stop), counted in register reads, the `*_SPINS` constants.
    pub(crate) const fn passes(passes: u32) -> Self {
        Self {
            clock: None,
            start: 0,
            cycles: 0,
            passes,
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
pub(crate) fn wait(deadline: Deadline, mut done: impl FnMut() -> bool) -> bool {
    poll(deadline, || done().then_some(())).is_some()
}

/// Polls `spins` times at most until `done`: [`wait`] on [`Deadline::passes`].
pub(crate) fn spin(spins: u32, done: impl FnMut() -> bool) -> bool {
    wait(Deadline::passes(spins), done)
}

/// Polls until `check` gives a value, or the deadline runs out: a wait that
/// can end in more than one way (a flag, or an error found on the way).
pub(crate) fn poll<T>(mut deadline: Deadline, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    loop {
        if let Some(value) = check() {
            return Some(value);
        }
        if deadline.expired() {
            return check();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Deadline, poll, spin};

    #[test]
    fn spin_stops_when_done() {
        let mut reads = 0;
        assert!(spin(10, || {
            reads += 1;
            reads == 3
        }));
        assert_eq!(reads, 3);
    }

    #[test]
    fn spin_gives_up_after_its_passes() {
        let mut reads = 0u32;
        assert!(!spin(5, || {
            reads += 1;
            false
        }));
        // The first look, one per pass, then a last one.
        assert_eq!(reads, 7);
    }

    #[test]
    fn poll_returns_the_first_value() {
        let mut reads = 0;
        let found = poll(Deadline::passes(10), || {
            reads += 1;
            (reads == 4).then_some(reads)
        });
        assert_eq!(found, Some(4));
        assert_eq!(poll(Deadline::passes(3), || None::<()>), None);
    }
}
