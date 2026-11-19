//! Core-cycle stamps through the lane switch (`Awg::listen`, `Awg::speak`),
//! an exchange's start and the frame-end turn, behind the profiling features
//! (`docs/policies/features.md`). They measure the prepared switch's cycle
//! count, which `docs/design/lanes.md` requires after any change to the
//! switch path. The bench enables DWT CYCCNT; this only reads it.
//!
//! Requirements: AWG-013

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// Stamp slots.
pub const ENTRY: usize = 0;
pub const OUTPUTS: usize = 1;
pub const NEAREST: usize = 2;
pub const RETIMED: usize = 3;
pub const LANES: usize = 4;
pub const BEGUN: usize = 5;
pub const S_ENTRY: usize = 18;
pub const S_PARKED: usize = 19;
pub const S_RETIMED: usize = 20;
pub const S_DONE: usize = 21;
pub const ENTRY2: usize = 22;
/// Per lane n (0 to 7, the generator's numbering): entry of
/// `CompareLane::listen`, before `dma.start`, after it: 23 + 3 n + 0, 1, 2.
/// Keyed by lane, not channel, so lanes on two timers' same channel do not
/// share a slot.
pub const LANE: usize = 23;
pub const SLOTS: usize = LANE + 3 * 8;

/// The lane `Awg::listen` is starting, for `CompareLane::listen`'s stamps.
#[cfg(feature = "profile-lanes")]
static CURRENT: AtomicU32 = AtomicU32::new(0);

/// Sets the lane the next `CompareLane::listen` stamps belong to.
#[cfg(feature = "profile-lanes")]
#[inline(always)]
pub(crate) fn set_lane(lane: usize) {
    CURRENT.store((lane & 7) as u32, Relaxed);
}

/// Lane slot `k` (0, 1, 2) of the lane set by [`set_lane`].
#[cfg(feature = "profile-lanes")]
#[inline(always)]
pub(crate) fn lane(k: usize) -> usize {
    LANE + 3 * CURRENT.load(Relaxed) as usize + k
}

static STAMP: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
/// Slots stamped since the last commit, a bit each, in two words.
static SEEN: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];

/// Records the cycle counter in `slot`.
#[inline(always)]
pub(crate) fn stamp(slot: usize) {
    if let Some(s) = STAMP.get(slot) {
        s.store(cortex_m::peripheral::DWT::cycle_count(), Relaxed);
        if let Some(word) = SEEN.get(slot / 32) {
            word.fetch_or(1 << (slot & 31), Relaxed);
        }
    }
}

/// One stage's cycles over every listen or speak committed.
pub struct Stage {
    pub name: &'static str,
    least: AtomicU32,
    most: AtomicU32,
    sum: AtomicU32,
    count: AtomicU32,
}

impl Stage {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            least: AtomicU32::new(u32::MAX),
            most: AtomicU32::new(0),
            sum: AtomicU32::new(0),
            count: AtomicU32::new(0),
        }
    }
    fn add(&self, cycles: u32) {
        if cycles < self.least.load(Relaxed) {
            self.least.store(cycles, Relaxed);
        }
        if cycles > self.most.load(Relaxed) {
            self.most.store(cycles, Relaxed);
        }
        self.sum
            .store(self.sum.load(Relaxed).wrapping_add(cycles), Relaxed);
        self.count
            .store(self.count.load(Relaxed).wrapping_add(1), Relaxed);
    }
    /// (least, mean, most, count) in core cycles.
    pub fn get(&self) -> (u32, u32, u32, u32) {
        let count = self.count.load(Relaxed);
        let mean = self.sum.load(Relaxed).checked_div(count).unwrap_or(0);
        (
            self.least.load(Relaxed),
            mean,
            self.most.load(Relaxed),
            count,
        )
    }
    fn clear(&self) {
        self.least.store(u32::MAX, Relaxed);
        self.most.store(0, Relaxed);
        self.sum.store(0, Relaxed);
        self.count.store(0, Relaxed);
    }
}

/// The stages, in the order `listen` and `speak` run them.
pub static STAGES: [Stage; 30] = [
    Stage::new("listen: whole"),
    Stage::new("listen: set_outputs(false)"),
    Stage::new("listen: retime (apply_rate, both timers)"),
    Stage::new("unused (was retime, registers)"),
    Stage::new("listen: every lane's CompareLane::listen"),
    Stage::new("listen: begin (stage, start)"),
    Stage::new("listen: between lanes (loop, calls)"),
    Stage::new("speak: whole"),
    Stage::new("speak: park"),
    Stage::new("speak: retime"),
    Stage::new("speak: set_outputs(true)"),
    Stage::new("one stamp (overhead in each stage above)"),
    Stage::new("lane 0: before dma.start"),
    Stage::new("lane 0: dma.start"),
    Stage::new("lane 1: before dma.start"),
    Stage::new("lane 1: dma.start"),
    Stage::new("lane 2: before dma.start"),
    Stage::new("lane 2: dma.start"),
    Stage::new("lane 3: before dma.start"),
    Stage::new("lane 3: dma.start"),
    Stage::new("lane 4: before dma.start"),
    Stage::new("lane 4: dma.start"),
    Stage::new("lane 5: before dma.start"),
    Stage::new("lane 5: dma.start"),
    Stage::new("lane 6: before dma.start"),
    Stage::new("lane 6: dma.start"),
    Stage::new("lane 7: before dma.start"),
    Stage::new("lane 7: dma.start"),
    Stage::new("unused"),
    Stage::new("unused"),
];

fn at(slot: usize) -> u32 {
    STAMP.get(slot).map_or(0, |s| s.load(Relaxed))
}

fn seen(slot: usize) -> bool {
    SEEN.get(slot / 32)
        .is_some_and(|word| word.load(Relaxed) & (1 << (slot & 31)) != 0)
}

/// Folds the stamps of the listen that just ended into [`STAGES`].
pub(crate) fn commit_listen() {
    let d = |a: usize, b: usize| at(b).wrapping_sub(at(a));
    STAGES[0].add(d(ENTRY, BEGUN));
    STAGES[11].add(d(ENTRY, ENTRY2));
    STAGES[1].add(d(ENTRY2, OUTPUTS));
    // Stage C on: the retime has no `nearest` (it is prepared), so this
    // is the whole retime; `NEAREST` is stamped only by `set_rate`.
    STAGES[2].add(d(OUTPUTS, RETIMED));
    STAGES[4].add(d(RETIMED, LANES));
    STAGES[5].add(d(LANES, BEGUN));
    let mut inside = 0u32;
    for n in 0..8 {
        let base = LANE + 3 * n;
        if seen(base) {
            let before = d(base, base + 1);
            let start = d(base + 1, base + 2);
            STAGES[12 + 2 * n].add(before);
            STAGES[13 + 2 * n].add(start);
            inside = inside.wrapping_add(before).wrapping_add(start);
        }
    }
    STAGES[6].add(d(RETIMED, LANES).wrapping_sub(inside));
    SEEN.iter().for_each(|word| word.store(0, Relaxed));
}

/// Folds the stamps of the speak that just ended into [`STAGES`].
pub(crate) fn commit_speak() {
    let d = |a: usize, b: usize| at(b).wrapping_sub(at(a));
    STAGES[7].add(d(S_ENTRY, S_DONE));
    STAGES[8].add(d(S_ENTRY, S_PARKED));
    STAGES[9].add(d(S_PARKED, S_RETIMED));
    STAGES[10].add(d(S_RETIMED, S_DONE));
    SEEN.iter().for_each(|word| word.store(0, Relaxed));
}

/// Clears every stage.
pub fn clear() {
    for s in &STAGES {
        s.clear();
    }
}

/// PROFILING ONLY (`profile-start`): an exchange's start by part. Each mark
/// adds the cycles since the one before to its stage. Marks count only
/// between [`start_begin`] and [`start_end`], so `play`'s `begin` is not
/// counted when `listen` calls it.
#[cfg(feature = "profile-start")]
static START_LAST: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "profile-start")]
static START_ON: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "profile-start")]
pub static START_STAGES: [Stage; 10] = [
    Stage::new("load: into arm_frame (Awg and the lanes' dispatch)"),
    Stage::new("arm_frame: checks (busy, limits, DMA reach)"),
    Stage::new("arm_frame: first duty, transfer built"),
    Stage::new("arm_frame: dma.start"),
    Stage::new("load: out of arm_frame and Awg::load_frame"),
    Stage::new("play: arm readers ahead"),
    Stage::new("play: masks"),
    Stage::new("play: stage first timer"),
    Stage::new("play: stage second timer"),
    Stage::new("play: start"),
];

#[cfg(feature = "profile-start")]
#[inline(always)]
pub(crate) fn start_begin() {
    START_LAST.store(cortex_m::peripheral::DWT::cycle_count(), Relaxed);
    START_ON.store(true, Relaxed);
}

#[cfg(feature = "profile-start")]
#[inline(always)]
pub(crate) fn start_mark(stage: usize) {
    if !START_ON.load(Relaxed) {
        return;
    }
    let now = cortex_m::peripheral::DWT::cycle_count();
    if let Some(s) = START_STAGES.get(stage) {
        s.add(now.wrapping_sub(START_LAST.load(Relaxed)));
    }
    START_LAST.store(now, Relaxed);
}

#[cfg(feature = "profile-start")]
#[inline(always)]
pub(crate) fn start_end() {
    START_ON.store(false, Relaxed);
}

#[cfg(feature = "profile-start")]
pub fn clear_start() {
    for s in START_STAGES.iter() {
        s.clear();
    }
}

/// PROFILING ONLY (`profile-start`): the run of `on_interrupt` that sees a
/// frame end, by part.
#[cfg(feature = "profile-start")]
pub static TURN_STAGES: [Stage; 4] = [
    Stage::new("serve: the lanes (and any interrupt given back)"),
    Stage::new("serve: the readers ahead"),
    Stage::new("serve: halt, or leave the timers running"),
    Stage::new("serve: listen at the end (lane-listen-at-end)"),
];

#[cfg(feature = "profile-start")]
pub(crate) fn turn_add(stamps: &[u32; 5]) {
    for (stage, pair) in TURN_STAGES.iter().zip(stamps.windows(2)) {
        stage.add(pair[1].wrapping_sub(pair[0]));
    }
}

#[cfg(feature = "profile-start")]
pub fn clear_turn() {
    for s in TURN_STAGES.iter() {
        s.clear();
    }
}
