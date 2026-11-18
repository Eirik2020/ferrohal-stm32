//! PROFILING ONLY, never merged: core-cycle stamps through `Awg::listen` and
//! `Awg::speak`, behind the `profile-listen` feature, for step 0 of the lane
//! switch design note. The bench enables DWT CYCCNT; this only reads it.

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// Stamp slots.
pub const ENTRY: usize = 0;
pub const OUTPUTS: usize = 1;
pub const NEAREST: usize = 2;
pub const RETIMED: usize = 3;
pub const LANES: usize = 4;
pub const BEGUN: usize = 5;
/// Per channel C (1 to 4): entry of `CompareLane::listen`, before
/// `dma.start`, after it: 6 + 3 (C - 1) + 0, 1, 2.
pub const CHANNEL: usize = 6;
pub const S_ENTRY: usize = 18;
pub const S_PARKED: usize = 19;
pub const S_RETIMED: usize = 20;
pub const S_DONE: usize = 21;
pub const ENTRY2: usize = 22;
pub const SLOTS: usize = 23;

static STAMP: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
static SEEN: AtomicU32 = AtomicU32::new(0);
/// A register read at each stamp beside the cycle counter (0: none), so a
/// bench can place each stamp on its own witness (a stream's NDTR).
static WATCH: AtomicU32 = AtomicU32::new(0);
static WATCHED: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
/// The watched values of the last listen and the last speak, as committed.
static LAST_LISTEN: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
static LAST_SPEAK: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];

/// Reads the 32-bit register at `address` at every stamp from now on; 0 stops.
/// The address must be a readable register without read side effects (a DMA
/// stream's NDTR).
pub fn watch(address: u32) {
    WATCH.store(address, Relaxed);
}

/// What the watched register read at each slot of the last listen and of the
/// last speak. Slots are the `pub` constants of this module.
pub fn last() -> ([u32; SLOTS], [u32; SLOTS]) {
    (
        core::array::from_fn(|i| LAST_LISTEN[i].load(Relaxed)),
        core::array::from_fn(|i| LAST_SPEAK[i].load(Relaxed)),
    )
}

fn snapshot(into: &[AtomicU32; SLOTS]) {
    for (to, from) in into.iter().zip(WATCHED.iter()) {
        to.store(from.load(Relaxed), Relaxed);
    }
}

/// Records the cycle counter in `slot`.
#[inline(always)]
pub(crate) fn stamp(slot: usize) {
    if let Some(s) = STAMP.get(slot) {
        s.store(cortex_m::peripheral::DWT::cycle_count(), Relaxed);
        let address = WATCH.load(Relaxed);
        if address != 0
            && let Some(w) = WATCHED.get(slot)
        {
            // SAFETY: profiling only; `watch`'s caller names a readable
            // register without read side effects.
            w.store(
                unsafe { core::ptr::read_volatile(address as *const u32) },
                Relaxed,
            );
        }
        SEEN.fetch_or(1 << (slot & 31), Relaxed);
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
        let mean = if count == 0 {
            0
        } else {
            self.sum.load(Relaxed) / count
        };
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
pub static STAGES: [Stage; 22] = [
    Stage::new("listen: whole"),
    Stage::new("listen: set_outputs(false)"),
    Stage::new("listen: retime, nearest"),
    Stage::new("listen: retime, registers"),
    Stage::new("listen: every lane's CompareLane::listen"),
    Stage::new("listen: begin (stage, start)"),
    Stage::new("lane CH1: before dma.start"),
    Stage::new("lane CH1: dma.start"),
    Stage::new("lane CH2: before dma.start"),
    Stage::new("lane CH2: dma.start"),
    Stage::new("lane CH3: before dma.start"),
    Stage::new("lane CH3: dma.start"),
    Stage::new("lane CH4: before dma.start"),
    Stage::new("lane CH4: dma.start"),
    Stage::new("listen: between lanes (loop, calls)"),
    Stage::new("speak: whole"),
    Stage::new("speak: park"),
    Stage::new("speak: retime"),
    Stage::new("speak: set_outputs(true)"),
    Stage::new("one stamp (overhead in each stage above)"),
    Stage::new("unused"),
    Stage::new("unused"),
];

fn at(slot: usize) -> u32 {
    STAMP.get(slot).map_or(0, |s| s.load(Relaxed))
}

fn seen(slot: usize) -> bool {
    SEEN.load(Relaxed) & (1 << (slot & 31)) != 0
}

/// Folds the stamps of the listen that just ended into [`STAGES`].
pub(crate) fn commit_listen() {
    let d = |a: usize, b: usize| at(b).wrapping_sub(at(a));
    STAGES[0].add(d(ENTRY, BEGUN));
    STAGES[19].add(d(ENTRY, ENTRY2));
    STAGES[1].add(d(ENTRY2, OUTPUTS));
    STAGES[2].add(d(OUTPUTS, NEAREST));
    STAGES[3].add(d(NEAREST, RETIMED));
    STAGES[4].add(d(RETIMED, LANES));
    STAGES[5].add(d(LANES, BEGUN));
    let mut inside = 0u32;
    for c in 0..4 {
        let base = CHANNEL + 3 * c;
        if seen(base) {
            let before = d(base, base + 1);
            let start = d(base + 1, base + 2);
            STAGES[6 + 2 * c].add(before);
            STAGES[7 + 2 * c].add(start);
            inside = inside.wrapping_add(before).wrapping_add(start);
        }
    }
    STAGES[14].add(d(RETIMED, LANES).wrapping_sub(inside));
    snapshot(&LAST_LISTEN);
    SEEN.store(0, Relaxed);
}

/// Folds the stamps of the speak that just ended into [`STAGES`].
pub(crate) fn commit_speak() {
    let d = |a: usize, b: usize| at(b).wrapping_sub(at(a));
    STAGES[15].add(d(S_ENTRY, S_DONE));
    STAGES[16].add(d(S_ENTRY, S_PARKED));
    STAGES[17].add(d(S_PARKED, S_RETIMED));
    STAGES[18].add(d(S_RETIMED, S_DONE));
    snapshot(&LAST_SPEAK);
    SEEN.store(0, Relaxed);
}

/// Clears every stage.
pub fn clear() {
    for s in &STAGES {
        s.clear();
    }
}
