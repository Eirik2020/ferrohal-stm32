//! The interrupt-driven I2C master (`ferrohal_stm32::i2c::InterruptI2c`)
//! against the Pico 1 simulator at `0x2A`, once with every byte
//! moved by the event interrupt and once from a DMA wiring.
//!
//! Each mode runs the same checks:
//!
//! - **rows**: a write, a write then a read (repeated start), and a read
//!   alone, of each of [`LENS`] bytes, [`REPS`] times, each checked against
//!   the simulator (its ECHO register for writes, its pattern stream for
//!   reads), and each phase's data path checked against the driver's
//!   thresholds through the health counts. Timings print as `ROW` lines:
//!   `start` (cycles in `start`), `isr` (cycles inside the vectors, summed,
//!   over `irqs` services) and `wall`, means over the repetitions.
//! - **refusals**: a start while one runs, an empty transaction, an empty
//!   phase, a phase past its buffer and past [`MAX_PHASE`], an address past
//!   7 bits: each refused with the transaction handed back, the bus
//!   untouched.
//! - **faults**: a vacant address NACKs, and the next transaction runs.
//! - **abort**: a stretch past the caller's bound is aborted, the buffers
//!   come back, and the bus runs again.
//! - **health**: the driver's counts equal the run's own.

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use ferrohal_stm32::i2c::{
    Error, Fault, InterruptHealth, MAX_PHASE, Outcome, Part, StartError, Transaction,
};

use crate::tests::{Fail, Report};

pub const ADDR: u8 = 0x2A;
const VACANT: u8 = 0x18;
const REG_ID: u8 = 0x00;
const REG_SEED: u8 = 0x03;
const REG_PATTERN: u8 = 0x04;
const REG_ECHO: u8 = 0x10;
const REG_STRETCH: u8 = 0x20;
const SIM_ID: u8 = 0xA1;
/// How much of a write the ECHO register keeps.
const ECHO_LEN: usize = 16;

/// The simulator's buffers are 64 bytes each way (`hil/pico-i2c-sim`): a
/// write is its pointer plus at most 63 bytes, a read at most 64. 2 and 3
/// straddle the thresholds.
pub const LENS: [usize; 6] = [1, 2, 3, 6, 16, 32];
pub const REPS: u32 = 10;

static ISR_SUM: AtomicU32 = AtomicU32::new(0);
static ISR_N: AtomicU32 = AtomicU32::new(0);
static DONE: AtomicBool = AtomicBool::new(false);

fn now() -> u32 {
    cortex_m::peripheral::DWT::cycle_count()
}

fn pattern(index: u32) -> u8 {
    (index.wrapping_mul(2_654_435_761) >> 24) as u8
}

/// What the run needs of the driver, whichever wiring opened it.
pub trait Bus {
    /// A refusal: the reason, with the transaction handed back.
    fn start(&mut self, t: Transaction) -> Result<(), (StartError, Transaction)>;
    fn abort(&mut self) -> Option<Outcome>;
    fn service(&mut self) -> Option<Outcome>;
    fn bound(&self, write: usize, read: usize) -> u32;
    fn health(&self) -> InterruptHealth;
    /// The shortest write and read phases this bus moves by DMA; `None`
    /// for a direction it never does.
    fn dma_from(&self) -> (Option<usize>, Option<usize>);
}

/// What the waiting side and the vectors share.
pub struct Slot<B: Bus> {
    pub bus: B,
    pub outcome: Option<Outcome>,
}

/// A vector's service, timed: call from every bound hardware task.
pub fn serve<B: Bus, M: rtic::Mutex<T = Slot<B>>>(m: &mut M) {
    let t0 = now();
    m.lock(|s| {
        if let Some(o) = s.bus.service() {
            s.outcome = Some(o);
            DONE.store(true, Ordering::Release);
        }
    });
    ISR_SUM.fetch_add(now().wrapping_sub(t0), Ordering::Relaxed);
    ISR_N.fetch_add(1, Ordering::Relaxed);
}

#[derive(Clone, Copy, Default)]
struct Sample {
    start: u32,
    isr: u32,
    irqs: u32,
    wall: u32,
}

#[derive(Clone, Copy, Default)]
struct Sum {
    start: u64,
    isr: u64,
    irqs: u64,
    wall: u64,
    n: u32,
}

impl Sum {
    fn add(&mut self, s: Sample) {
        self.start += u64::from(s.start);
        self.isr += u64::from(s.isr);
        self.irqs += u64::from(s.irqs);
        self.wall += u64::from(s.wall);
        self.n += 1;
    }

    fn print(&self, mode: &str, shape: &str, len: usize) {
        let n = u64::from(self.n.max(1));
        defmt::info!(
            "ROW {=str} {=str} len={=usize} n={=u32} start={=u64} isr={=u64} irqs={=u64} wall={=u64}",
            mode,
            shape,
            len,
            self.n,
            self.start / n,
            self.isr / n,
            self.irqs / n,
            self.wall / n
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Write,
    WriteRead,
    Read,
}

impl Shape {
    fn name(self) -> &'static str {
        match self {
            Shape::Write => "write",
            Shape::WriteRead => "write-read",
            Shape::Read => "read",
        }
    }
}

/// The run's own counts, against which the driver's health is checked.
#[derive(Clone, Copy, Default)]
struct Expected {
    transactions: u32,
    nacks: u32,
    aborts: u32,
    dma_phases: u32,
    irq_phases: u32,
}

/// The waiting side: the shared slot, the lent buffers, the run's counts.
pub struct Harness<B: Bus, M: rtic::Mutex<T = Slot<B>>> {
    pub m: M,
    tx: Option<&'static mut [u8]>,
    rx: Option<&'static mut [u8]>,
    expected: Expected,
    base: InterruptHealth,
    dma_from: (Option<usize>, Option<usize>),
    _b: core::marker::PhantomData<B>,
}

impl<B: Bus, M: rtic::Mutex<T = Slot<B>>> Harness<B, M> {
    /// `tx` and `rx` must be longer than [`MAX_PHASE`], for the refusal of
    /// a phase past it.
    pub fn new(m: M, tx: &'static mut [u8], rx: &'static mut [u8]) -> Self {
        Self {
            m,
            tx: Some(tx),
            rx: Some(rx),
            expected: Expected::default(),
            base: InterruptHealth::default(),
            dma_from: (None, None),
            _b: core::marker::PhantomData,
        }
    }

    /// A new driver is in the slot: its counts start from zero.
    pub fn reopened(&mut self) {
        self.expected = Expected::default();
        self.base = self.m.lock(|s| s.bus.health());
        self.dma_from = self.m.lock(|s| s.bus.dma_from());
    }

    fn take(&mut self, write: &[u8], read: usize) -> Option<Transaction> {
        let (tx, rx) = (self.tx.take()?, self.rx.take()?);
        if let Some(dst) = tx.get_mut(..write.len()) {
            dst.copy_from_slice(write);
        }
        let (wp, spare_tx) = if write.is_empty() {
            (None, Some(tx))
        } else {
            (
                Some(Part {
                    buf: tx,
                    len: write.len(),
                }),
                None,
            )
        };
        let (rp, spare_rx) = if read == 0 {
            (None, Some(rx))
        } else {
            (Some(Part { buf: rx, len: read }), None)
        };
        self.tx = spare_tx;
        self.rx = spare_rx;
        Some(Transaction {
            address: ADDR,
            write: wp,
            read: rp,
        })
    }

    fn give_back(&mut self, t: Transaction) {
        if let Some(p) = t.write {
            self.tx = Some(p.buf);
        }
        if let Some(p) = t.read {
            self.rx = Some(p.buf);
        }
    }

    /// Both buffers are home.
    fn home(&self) -> Result<(), Fail> {
        if self.tx.is_some() && self.rx.is_some() {
            Ok(())
        } else {
            Err(Fail::Unexpected("a lent buffer did not come back"))
        }
    }

    fn reset_stats() {
        ISR_SUM.store(0, Ordering::Relaxed);
        ISR_N.store(0, Ordering::Relaxed);
        DONE.store(false, Ordering::Release);
    }

    /// One transaction: started, waited for within the driver's bound (and
    /// `extra` cycles), aborted past it.
    fn run(
        &mut self,
        address: u8,
        write: &[u8],
        read: usize,
        extra: u32,
    ) -> (Result<(), Fault>, Sample) {
        let Some(mut t) = self.take(write, read) else {
            return (Err(Fault::Dma), Sample::default());
        };
        t.address = address;
        let bound = self
            .m
            .lock(|s| s.bus.bound(write.len(), read))
            .saturating_add(extra);
        Self::reset_stats();
        let t0 = now();
        let started = self.m.lock(|s| {
            s.outcome = None;
            s.bus.start(t)
        });
        let start = now().wrapping_sub(t0);
        if let Err((_, t)) = started {
            self.give_back(t);
            return (
                Err(Fault::Dma),
                Sample {
                    start,
                    ..Sample::default()
                },
            );
        }
        let done = loop {
            if DONE.load(Ordering::Acquire) {
                break true;
            }
            if now().wrapping_sub(t0) > bound {
                break false;
            }
        };
        let wall = now().wrapping_sub(t0);
        let outcome = self.m.lock(|s| {
            if done {
                s.outcome.take()
            } else {
                s.bus.abort()
            }
        });
        let sample = Sample {
            start,
            isr: ISR_SUM.load(Ordering::Relaxed),
            irqs: ISR_N.load(Ordering::Relaxed),
            wall,
        };
        let Some(o) = outcome else {
            return (Err(Fault::Dma), sample);
        };
        let result = o.result;
        self.give_back(o.transaction);
        match result {
            Ok(()) => self.expected.transactions += 1,
            Err(Fault::I2c(Error::Nack)) => self.expected.nacks += 1,
            Err(Fault::Aborted) => self.expected.aborts += 1,
            Err(_) => {}
        }
        (result, sample)
    }

    /// The phases a transaction of `write` then `read` bytes moves by DMA
    /// and by interrupt, by the bus's thresholds.
    fn phases(&self, write: usize, read: usize) -> (u32, u32) {
        let (wf, rf) = self.dma_from;
        let mut dma = 0;
        let mut irq = 0;
        for (len, from) in [(write, wf), (read, rf)] {
            if len == 0 {
                continue;
            }
            if from.is_some_and(|f| len >= f) {
                dma += 1;
            } else {
                irq += 1;
            }
        }
        (dma, irq)
    }

    /// One transaction that must succeed, its phases counted.
    fn ok(&mut self, write: &[u8], read: usize) -> Result<Sample, Fail> {
        let (r, s) = self.run(ADDR, write, read, 0);
        r.map_err(fault)?;
        let (d, i) = self.phases(write.len(), read);
        self.expected.dma_phases += d;
        self.expected.irq_phases += i;
        Ok(s)
    }

    fn read_back(&self, len: usize) -> &[u8] {
        self.rx.as_deref().and_then(|r| r.get(..len)).unwrap_or(&[])
    }

    fn id(&mut self) -> Result<(), Fail> {
        self.ok(&[REG_ID], 1)?;
        match self.read_back(1).first() {
            Some(&SIM_ID) => Ok(()),
            other => Err(Fail::Mismatch {
                at: 0,
                sent: SIM_ID,
                got: other.copied().unwrap_or(0),
            }),
        }
    }

    fn echo_matches(&mut self, payload: &[u8]) -> Result<(), Fail> {
        if payload.is_empty() {
            return Ok(());
        }
        let kept = payload.len().min(ECHO_LEN);
        self.ok(&[REG_ECHO], kept)?;
        for (i, (&got, &want)) in self.read_back(kept).iter().zip(payload).enumerate() {
            if got != want {
                return Err(Fail::Mismatch {
                    at: i,
                    sent: want,
                    got,
                });
            }
        }
        Ok(())
    }

    /// One row: `REPS` transactions of one shape and length, each checked
    /// against the simulator and its data path against the thresholds.
    fn row(&mut self, shape: Shape, len: usize, mode: &str) -> Result<(), Fail> {
        let mut sum = Sum::default();
        let mut result = Ok(());
        for rep in 0..REPS {
            let seed = (rep as u8).wrapping_mul(37).wrapping_add(len as u8);
            let one = (|| -> Result<Sample, Fail> {
                let (write, read): (&[u8], usize);
                let mut w = [0u8; 64];
                match shape {
                    Shape::Write => {
                        w[0] = REG_ECHO;
                        for (i, b) in w.iter_mut().enumerate().take(len).skip(1) {
                            *b = pattern(i as u32 + u32::from(seed));
                        }
                        (write, read) = (&w[..len], 0);
                    }
                    Shape::WriteRead => {
                        self.ok(&[REG_SEED, seed], 0)?;
                        (write, read) = (&[REG_PATTERN], len);
                    }
                    Shape::Read => {
                        self.ok(&[REG_SEED, seed], 0)?;
                        self.ok(&[REG_PATTERN], 0)?;
                        (write, read) = (&[], len);
                    }
                }
                let before = self.m.lock(|s| s.bus.health());
                let s = self.ok(write, read)?;
                let after = self.m.lock(|s| s.bus.health());
                let (d, i) = self.phases(write.len(), read);
                let got_d = after.dma_phases.wrapping_sub(before.dma_phases);
                let got_i = after.irq_phases.wrapping_sub(before.irq_phases);
                if got_d != d || got_i != i {
                    return Err(Fail::Count {
                        expected: d * 100 + i,
                        got: got_d * 100 + got_i,
                    });
                }
                if shape == Shape::Write {
                    self.echo_matches(&w[1..len])?;
                } else {
                    for (i, &b) in self.read_back(len).iter().enumerate() {
                        let want = pattern(u32::from(seed) + i as u32);
                        if b != want {
                            return Err(Fail::Mismatch {
                                at: i,
                                sent: want,
                                got: b,
                            });
                        }
                    }
                }
                Ok(s)
            })();
            match one {
                Ok(s) => sum.add(s),
                Err(f) => {
                    result = Err(f);
                    break;
                }
            }
        }
        sum.print(mode, shape.name(), len);
        result
    }

    /// Every check for the driver now in the slot, with `mode` naming it
    /// and `verifies` the requirements its rows add (the IP version's).
    pub fn suite(&mut self, report: &mut Report, link: &str, mode: &str, verifies: &'static str) {
        self.reopened();
        report.check_for(link, "I2C-007", "the simulator answers", self.id());
        for shape in [Shape::Write, Shape::WriteRead, Shape::Read] {
            for len in LENS {
                let r = self.row(shape, len, mode);
                report.check_for(
                    link,
                    verifies,
                    "a row matches the simulator, each phase on its path",
                    r,
                );
            }
        }
        report.check_for(link, "I2C-007", "every lent buffer came back", self.home());
        self.refusals(report, link);
        // A vacant address: a NACK with the buffers, and the next one runs.
        let (r, _) = self.run(VACANT, &[0], 0, 0);
        report.check_for(
            link,
            "I2C-010",
            "a vacant address ends in a NACK",
            match r {
                Err(Fault::I2c(Error::Nack)) => self.home(),
                other => Err(outcome_fail(other)),
            },
        );
        report.check_for(link, "I2C-010", "the bus runs after the NACK", self.id());
        self.abort(report, link);
        report.check_for(
            link,
            "I2C-013",
            "the health counts equal the run's",
            self.health(),
        );
    }

    fn refusals(&mut self, report: &mut Report, link: &str) {
        let before = self.m.lock(|s| s.bus.health());
        // A start while one runs: a 32-byte read takes milliseconds.
        let busy = (|| -> Result<(), Fail> {
            self.ok(&[REG_PATTERN], 0)?;
            let first = self.take(&[], 32).ok_or(Fail::Unexpected("buffers out"))?;
            Self::reset_stats();
            self.m
                .lock(|s| {
                    s.outcome = None;
                    s.bus.start(first)
                })
                .map_err(|(e, t)| {
                    self.give_back(t);
                    start_fail(e)
                })?;
            let second = Transaction {
                address: ADDR,
                write: None,
                read: None,
            };
            let refused = self.m.lock(|s| s.bus.start(second));
            let verdict = match refused {
                Err((StartError::Busy, _)) => Ok(()),
                Err(_) => Err(Fail::Unexpected("refused, but not as busy")),
                Ok(()) => Err(Fail::Unexpected("a second start ran")),
            };
            // Let the first one finish, then take its buffers back.
            let t0 = now();
            while !DONE.load(Ordering::Acquire) && now().wrapping_sub(t0) < 50_000_000 {
                core::hint::spin_loop();
            }
            let outcome = self.m.lock(|s| s.outcome.take().or_else(|| s.bus.abort()));
            match outcome {
                Some(o) => {
                    if o.result.is_ok() {
                        self.expected.transactions += 1;
                        let (d, i) = self.phases(0, 32);
                        self.expected.dma_phases += d;
                        self.expected.irq_phases += i;
                    }
                    self.give_back(o.transaction);
                }
                None => return Err(Fail::Unexpected("the first transaction never ended")),
            }
            verdict
        })();
        report.check_for(
            link,
            "I2C-009",
            "a start while one runs is refused as busy",
            busy,
        );
        let cases: [(&str, usize, usize, u8, StartError); 5] = [
            (
                "an empty transaction is refused",
                0,
                0,
                ADDR,
                StartError::Empty,
            ),
            ("an empty phase is refused", 1, 0, ADDR, StartError::Empty),
            (
                "a phase past its buffer is refused",
                2,
                0,
                ADDR,
                StartError::Empty,
            ),
            (
                "a phase past 255 bytes is refused",
                3,
                0,
                ADDR,
                StartError::TooLarge,
            ),
            (
                "an address past 7 bits is refused",
                4,
                1,
                0x80,
                StartError::TooLarge,
            ),
        ];
        for (name, case, read, address, want) in cases {
            let r = (|| -> Result<(), Fail> {
                let read_part = if case == 0 {
                    None
                } else {
                    let rx = self.rx.take().ok_or(Fail::Unexpected("buffers out"))?;
                    let len = match case {
                        1 => 0,
                        2 => rx.len() + 1,
                        3 => MAX_PHASE + 1,
                        _ => read,
                    };
                    Some(Part { buf: rx, len })
                };
                let t = Transaction {
                    address,
                    write: None,
                    read: read_part,
                };
                match self.m.lock(|s| s.bus.start(t)) {
                    Err((e, t)) => {
                        self.give_back(t);
                        self.home()?;
                        if e == want {
                            Ok(())
                        } else {
                            Err(start_fail(e))
                        }
                    }
                    Ok(()) => Err(Fail::Unexpected("the start ran")),
                }
            })();
            report.check_for(link, "I2C-009", name, r);
        }
        let after = self.m.lock(|s| s.bus.health());
        // Only the busy check's first transaction moved the bus.
        let moved = after.transactions.wrapping_sub(before.transactions);
        report.check_for(
            link,
            "I2C-009",
            "the refusals left the bus untouched",
            if moved == 1 {
                Ok(())
            } else {
                Err(Fail::Count {
                    expected: 1,
                    got: moved,
                })
            },
        );
        report.check_for(
            link,
            "I2C-009",
            "the bus runs after the refusals",
            self.id(),
        );
    }

    /// A stretch past the bound: the caller aborts, the buffers come back,
    /// and the bus runs again (polled every 100 ms up to 10 s, logged).
    fn abort(&mut self, report: &mut Report, link: &str) {
        let stretch = 20_000u16.to_le_bytes();
        let armed = self
            .ok(&[REG_STRETCH, stretch[0], stretch[1]], 0)
            .map(|_| ());
        report.check_for(link, "I2C-011", "the stretch is armed", armed);
        let (r, _) = self.run(ADDR, &[REG_PATTERN], 6, 0);
        report.check_for(
            link,
            "I2C-011",
            "a stretch past the bound is aborted with its buffers",
            match r {
                Err(Fault::Aborted) => self.home(),
                other => Err(outcome_fail(other)),
            },
        );
        crate::tests::delay_ms(40);
        let mut tries = 0u32;
        let mut ran = self.id();
        while ran.is_err() && tries < 100 {
            crate::tests::delay_ms(100);
            tries += 1;
            ran = self.id();
        }
        defmt::info!("ABORT recovered after {=u32} retries of 100 ms", tries);
        report.check_for(
            link,
            "I2C-011,I2C-V1-003",
            "the bus runs after the abort",
            ran,
        );
    }

    fn health(&mut self) -> Result<(), Fail> {
        let h = self.m.lock(|s| s.bus.health());
        let e = self.expected;
        let b = self.base;
        let pairs = [
            (
                "transactions",
                e.transactions,
                h.transactions.wrapping_sub(b.transactions),
            ),
            ("nacks", e.nacks, h.nacks.wrapping_sub(b.nacks)),
            ("aborts", e.aborts, h.aborts.wrapping_sub(b.aborts)),
            ("faults", 0, h.faults.wrapping_sub(b.faults)),
            ("dma_errors", 0, h.dma_errors.wrapping_sub(b.dma_errors)),
            ("busy", 0, h.busy.wrapping_sub(b.busy)),
        ];
        defmt::info!("HEALTH {}", h);
        for (field, want, got) in pairs {
            if want != got {
                defmt::error!(
                    "health {=str}: expected {=u32}, got {=u32}",
                    field,
                    want,
                    got
                );
                return Err(Fail::Count {
                    expected: want,
                    got,
                });
            }
        }
        Ok(())
    }
}

fn fault(f: Fault) -> Fail {
    match f {
        Fault::I2c(e) => Fail::I2c(e),
        Fault::Dma => Fail::Unexpected("a stream fault"),
        Fault::Aborted => Fail::Unexpected("aborted past its bound"),
    }
}

fn outcome_fail(r: Result<(), Fault>) -> Fail {
    match r {
        Ok(()) => Fail::Unexpected("it completed"),
        Err(f) => fault(f),
    }
}

fn start_fail(e: StartError) -> Fail {
    match e {
        StartError::Busy => Fail::Unexpected("refused: busy"),
        StartError::BusBusy => Fail::Unexpected("refused: the bus stayed busy"),
        StartError::Empty => Fail::Unexpected("refused: empty"),
        StartError::TooLarge => Fail::Unexpected("refused: too large"),
        StartError::Buffer(_) => Fail::Unexpected("refused: the buffer"),
    }
}
