//! The Pico 1 I2C peripheral simulator: a DW `apb_i2c` slave at address
//! `0x2A` on both of the RP2040's I2C blocks, for the Nucleo benches'
//! live-path runs (the wiring list is `hil/WIRING.md`). This file is the
//! library: the register map the master-side suite in
//! `hil/common/i2c_live.rs` speaks, pure enough to unit test on the host.
//!
//! | Reg | Name | A write sets | A read returns |
//! | --- | --- | --- | --- |
//! | `0x00` | ID | nothing | `0xA1`, then the bus tag at `0x01` |
//! | `0x01` | the bus tag | nothing | `[tag]` |
//! | `0x02` | CAPS | nothing | `[0x03]`: bit 0 data-phase NACK, bit 1 stretch |
//! | `0x03` | SEED | the pattern seed | `[seed]` |
//! | `0x04` | PATTERN | the pattern seed | `pattern(seed + i)` for `i = 0..` |
//! | `0x10` | ECHO | up to 16 bytes, kept | the kept bytes, zero-padded |
//! | `0x20` | STRETCH | µs (LE16) to hold SCL before the next read | `[lo, hi]` |
//! | `0x22` | NACK | 1: the next write's first data byte is refused | `[0]` once spent |
//! | `0x30` | BYTES | nothing | data bytes the last write left, u32 LE |
//! | `0x34` | TRANS | nothing | transactions since CLEAR, u32 LE |
//! | `0x38` | SHAPE | nothing | `[0]` none, `[1]` write, `[2]` read, `[3]` write-read |
//! | `0x3C` | CLEAR | 1: zero BYTES, TRANS and SHAPE, spend the one-shots | `[0]` |
//! | `0x3D` | BOOT | 1: reboot into the USB bootloader (run-t sends it) | `[0]` |
//!
//! Every write's first byte is the register pointer; the rest is payload.
//! The counters make the master's cross-checks byte-exact: the map counts
//! what the wire did, the master counts what it asked for, and the runs
//! compare. CLEAR lets each run start from zero without power-cycling the
//! Pico, so one firmware serves every bench run idempotently.
//!
//! The DW `apb_i2c` underneath has three behaviours the bench found and the
//! runs and this simulator are built around:
//!
//! - **An I2C pin must be `into_alternate_open_drain`.** A push-pull
//!   alternate drives SDA high through every acknowledge and NACKs every
//!   transfer to a live slave. Every flight board wired its barometer bus
//!   push-pull until the bench found it.
//! - **A `TX_ABORT` flushes both FIFOs and holds them until `IC_CLR_TX_ABRT`
//!   is read**, so a second `respond_to_read` after an abort would push into
//!   a stopped bus: one call per read is this simulator's contract, and the
//!   driver's.
//! - **The DW never delivers a NACKed receive byte**, so a data-phase
//!   refusal is invisible on the wire; the simulator spends its one-shot
//!   arm on a timed warden instead, and the driver reports the refusal from
//!   the absence of bytes, not from a flag.

#![no_std]

/// The address the simulator answers on both buses. Nothing on either
/// bench answers `0x18`, the address the NACK-only suites use, so the two
/// kinds of run never see each other's traffic.
pub const ADDR: u8 = 0x2A;

pub const REG_ID: u8 = 0x00;
pub const REG_CAPS: u8 = 0x02;
pub const REG_SEED: u8 = 0x03;
pub const REG_PATTERN: u8 = 0x04;
pub const REG_ECHO: u8 = 0x10;
pub const REG_STRETCH: u8 = 0x20;
pub const REG_NACK: u8 = 0x22;
pub const REG_BYTES: u8 = 0x30;
pub const REG_TRANS: u8 = 0x34;
pub const REG_SHAPE: u8 = 0x38;
pub const REG_CLEAR: u8 = 0x3C;
/// 1: the simulator reboots into its USB bootloader, so the next flash
/// needs no finger on the button. The run that sends it is `run-t`.
pub const REG_BOOT: u8 = 0x3D;

/// The refused-data capability: the slave ACKs the address, then NACKs
/// the first data byte (the DW block's `IC_SLV_DATA_NACK_ONLY`).
pub const CAP_DATA_NACK: u8 = 1 << 0;
/// The clock-stretching capability: the slave holds SCL while it decides.
pub const CAP_STRETCH: u8 = 1 << 1;

pub const SHAPE_NONE: u8 = 0;
pub const SHAPE_WRITE: u8 = 1;
pub const SHAPE_READ: u8 = 2;
pub const SHAPE_WRITE_READ: u8 = 3;

/// The ID register's first byte: nothing else on either bus sends it.
pub const ID: u8 = 0xA1;
/// How much of the ECHO register a write keeps: 0x10 to 0x1F, so the
/// control registers from 0x20 on are not part of it.
pub const ECHO_LEN: usize = 16;
/// The longest stretch the simulator will hold: long past any master's
/// bound, short of wedging a bench.
pub const STRETCH_MAX_US: u32 = 100_000;

/// Byte `index` of the pattern stream: a hash, so a dropped, repeated or
/// shifted byte shows up at once. The master's live suite computes the
/// same hash — `hil/common/tests.rs`'s `pattern` — so all three must stay
/// identical.
pub fn pattern(index: u32) -> u8 {
    (index.wrapping_mul(2_654_435_761) >> 24) as u8
}

/// One bus's register map and counters.
pub struct State {
    /// The ID register's second byte: `1` for I2C0 (the F401 bench),
    /// `2` for I2C1 (the H753 bench).
    tag: u8,
    pointer: u8,
    seed: u8,
    echo: [u8; ECHO_LEN],
    echo_kept: usize,
    stretch_us: u32,
    stretch_queued: Option<u32>,
    nack_armed: bool,
    bytes: u32,
    trans: u32,
    shape: u8,
    /// The BOOT register was written: reboot into the USB bootloader.
    boot: bool,
}

impl State {
    pub fn new(tag: u8) -> Self {
        Self {
            tag,
            pointer: REG_ID,
            seed: 0,
            echo: [0; ECHO_LEN],
            echo_kept: 0,
            stretch_us: 0,
            stretch_queued: None,
            nack_armed: false,
            bytes: 0,
            trans: 0,
            shape: SHAPE_NONE,
            boot: false,
        }
    }
}

impl State {
    /// What a write's bytes do. `data[0]` is the register pointer; the
    /// rest is payload. Every write leaves its payload length in BYTES,
    /// whatever register it touched.
    pub fn on_write(&mut self, data: &[u8]) {
        let Some((&pointer, payload)) = data.split_first() else {
            return;
        };
        self.pointer = pointer;
        self.bytes = payload.len() as u32;
        match pointer {
            REG_SEED | REG_PATTERN => {
                if let Some(&seed) = payload.first() {
                    self.seed = seed;
                }
            }
            REG_ECHO => {
                // A pointer-only write points at the register; only a
                // write with payload rewrites it.
                if !payload.is_empty() {
                    let kept = payload.len().min(ECHO_LEN);
                    self.echo[..kept].copy_from_slice(&payload[..kept]);
                    self.echo_kept = kept;
                }
            }
            REG_STRETCH => {
                if !payload.is_empty() {
                    let mut low = [0u8; 2];
                    let len = payload.len().min(low.len());
                    low[..len].copy_from_slice(&payload[..len]);
                    let us = u32::from(low[0]) | u32::from(low[1]) << 8;
                    self.stretch_us = us.min(STRETCH_MAX_US);
                    // One-shot: the next read phase stretches, and only
                    // that.
                    self.stretch_queued = Some(self.stretch_us);
                }
            }
            REG_NACK => {
                // One-shot: the register reports armed until the refusal
                // spends it. A pointer-only write (a read-back) must not
                // spend it.
                if !payload.is_empty() {
                    self.nack_armed = payload.first().is_some_and(|&v| v != 0);
                }
            }
            REG_CLEAR if payload.first().is_some_and(|&v| v == 1) => {
                self.bytes = 0;
                self.trans = 0;
                self.shape = SHAPE_NONE;
                // The one-shots too, so a run that died mid-scenario leaves
                // nothing armed for the next one.
                self.nack_armed = false;
                self.stretch_queued = None;
            }
            REG_BOOT if payload.first().is_some_and(|&v| v == 1) => {
                self.boot = true;
            }
            _ => {}
        }
    }
}

impl State {
    /// A transaction the slave witnessed: one Write, Read or WriteRead.
    /// Counted the moment the wire makes it real, so the master's reads
    /// never race the simulator's bookkeeping.
    pub fn on_transaction(&mut self, shape: u8) {
        self.trans = self.trans.saturating_add(1);
        self.shape = shape;
    }

    /// A transaction that ended in a hardware abort: the refused write
    /// (the NACK spent), or a read the master walked away from. The
    /// refused byte was never pushed, so BYTES is zero and the one-shot
    /// NACK is spent.
    pub fn on_abort(&mut self) {
        self.bytes = 0;
        self.trans = self.trans.saturating_add(1);
        self.nack_armed = false;
        self.stretch_queued = None;
    }

    /// The one-shot stretch for the read phase about to be answered.
    pub fn take_stretch(&mut self) -> Option<u32> {
        self.stretch_queued.take()
    }

    /// Whether the hardware must refuse the next written data byte.
    pub fn nack_armed(&self) -> bool {
        self.nack_armed
    }

    /// Whether a BOOT request is pending, spending it.
    pub fn take_boot(&mut self) -> bool {
        core::mem::take(&mut self.boot)
    }

    /// Fills `out` from the register the last write pointed at. A read
    /// pointed into the pattern register is the hash stream: it flows on
    /// past the region for as many bytes as the master reads, so a long
    /// read is hash the whole way. Every other register is itself, byte
    /// by byte: each byte is what its own address holds.
    pub fn fill(&self, out: &mut [u8]) {
        let from = self.pointer as usize;
        if (REG_PATTERN as usize..0x10).contains(&from) {
            let base = u32::from(self.seed) + (from - REG_PATTERN as usize) as u32;
            for (i, byte) in out.iter_mut().enumerate() {
                *byte = pattern(base + i as u32);
            }
            return;
        }
        out.fill(0);
        for (i, byte) in out.iter_mut().enumerate() {
            let at = from + i;
            *byte = match at {
                0x00 => ID,
                0x01 => self.tag,
                0x02 => CAP_DATA_NACK | CAP_STRETCH,
                0x03 => self.seed,
                0x04..=0x0F => pattern(u32::from(self.seed) + (at - 0x04) as u32),
                0x10..=0x1F if at - 0x10 < self.echo_kept => self.echo[at - 0x10],
                0x20 => self.stretch_us as u8,
                0x21 => (self.stretch_us >> 8) as u8,
                0x22 => u8::from(self.nack_armed),
                0x30..=0x33 => (self.bytes >> (8 * (at - 0x30))) as u8,
                0x34..=0x37 => (self.trans >> (8 * (at - 0x34))) as u8,
                0x38 => self.shape,
                0x3D => u8::from(self.boot),
                _ => 0,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many bytes a read gives the checks: the echo register whole,
    /// and more than any register the master reads.
    const READ: usize = 16;

    /// Reads from `reg`, the way the master does: point the map at the
    /// register with a pointer-only write, then read. The check compares
    /// the prefix it cares about.
    fn read_at(state: &mut State, reg: u8) -> [u8; READ] {
        state.on_write(&[reg]);
        let mut out = [0; READ];
        state.fill(&mut out);
        out
    }

    #[test]
    fn id_names_the_simulator_and_the_bus() {
        let mut state = State::new(1);
        assert_eq!(
            &read_at(&mut state, REG_ID)[..3],
            &[ID, 1, CAP_DATA_NACK | CAP_STRETCH]
        );
        let mut state = State::new(2);
        assert_eq!(&read_at(&mut state, REG_ID)[..2], &[ID, 2]);
    }

    #[test]
    fn caps_report_both_capabilities() {
        let mut state = State::new(1);
        assert_eq!(read_at(&mut state, REG_CAPS)[0], CAP_DATA_NACK | CAP_STRETCH);
    }

    #[test]
    fn the_pattern_is_the_hash_the_master_computes() {
        // The same formula as hil/common/tests.rs's `pattern`: a mismatch
        // would fail every bench read, so pin it here too.
        for index in 0..64u32 {
            assert_eq!(
                pattern(index),
                ((index.wrapping_mul(2_654_435_761)) >> 24) as u8
            );
        }
        let mut state = State::new(1);
        state.on_write(&[REG_SEED, 0x37]);
        assert_eq!(
            &read_at(&mut state, REG_PATTERN)[..4],
            &[pattern(0x37), pattern(0x38), pattern(0x39), pattern(0x3A)]
        );
    }

    #[test]
    fn a_long_pattern_read_is_hash_the_whole_way() {
        let mut state = State::new(1);
        state.on_write(&[REG_SEED, 0x53]);
        state.on_write(&[REG_PATTERN]);
        let mut out = [0; 32];
        state.fill(&mut out);
        // The stream flows past the 12-byte region: the master's long read
        // is hash at every index, not the registers the addresses cross.
        for (i, byte) in out.iter().enumerate() {
            assert_eq!(*byte, pattern(0x53 + i as u32));
        }
    }

    #[test]
    fn a_clear_spends_the_one_shots() {
        let mut state = State::new(2);
        state.on_write(&[REG_STRETCH, 0x40, 0x1F]); // 8000 µs
        state.on_write(&[REG_NACK, 1]);
        assert!(state.nack_armed());
        assert!(state.take_stretch().is_some());
        // The stretch was spent by the read; the arm waits for its write.
        state.on_write(&[REG_NACK, 1]);
        state.on_write(&[REG_CLEAR, 1]);
        assert!(!state.nack_armed());
        assert!(state.take_stretch().is_none());
    }

    #[test]
    fn a_boot_request_latches_once() {
        let mut state = State::new(1);
        assert!(!state.take_boot());
        state.on_write(&[REG_BOOT, 0]);
        assert!(!state.take_boot());
        state.on_write(&[REG_BOOT, 1]);
        assert!(state.take_boot());
        assert!(!state.take_boot());
    }

    #[test]
    fn an_echo_write_reads_back() {
        let mut state = State::new(2);
        let sent = [0xA5, 0x3C, 0x5A, 0x00, 0xFF];
        let mut write = [REG_ECHO, 0, 0, 0, 0, 0];
        write[1..].copy_from_slice(&sent);
        state.on_write(&write);
        let out = read_at(&mut state, REG_ECHO);
        assert_eq!(&out[..sent.len()], &sent[..]);
        // Past the kept bytes the echo reads zero.
        assert_eq!(out[sent.len()], 0);
    }

    #[test]
    fn a_stretch_is_capped_and_one_shot() {
        let mut state = State::new(1);
        // The register is 16 bits, so the firmware cap of 100 000 µs is
        // above anything a write can ask for: 0xFFFF µs passes whole.
        state.on_write(&[REG_STRETCH, 0xFF, 0xFF]);
        assert_eq!(state.take_stretch(), Some(0xFFFF));
        assert_eq!(state.take_stretch(), None);
        state.on_write(&[REG_STRETCH, 0x2C, 0x01]);
        assert_eq!(state.take_stretch(), Some(300));
        // The register still reads back what was asked for.
        assert_eq!(&read_at(&mut state, REG_STRETCH)[..2], &[0x2C, 0x01]);
    }

    #[test]
    fn the_counters_count_what_the_wire_did() {
        let mut state = State::new(1);
        state.on_write(&[REG_CLEAR, 1]);
        // The CLEAR zeroes BYTES; the write's own byte count does not
        // land because the register it touched was CLEAR.
        assert_eq!(state.bytes, 0);
        // The CLEAR write itself is the first transaction.
        state.on_transaction(SHAPE_WRITE);
        assert_eq!(&read_at(&mut state, REG_TRANS)[..4], &[1, 0, 0, 0]);
        assert_eq!(read_at(&mut state, REG_SHAPE)[0], SHAPE_WRITE);
        state.on_write(&[REG_ECHO, 1, 2, 3]);
        state.on_transaction(SHAPE_WRITE);
        state.on_write(&[REG_PATTERN]);
        state.on_transaction(SHAPE_WRITE_READ);
        assert_eq!(&read_at(&mut state, REG_TRANS)[..4], &[3, 0, 0, 0]);
        assert_eq!(read_at(&mut state, REG_SHAPE)[0], SHAPE_WRITE_READ);
        // A write of only a pointer leaves no data bytes.
        assert_eq!(&read_at(&mut state, REG_BYTES)[..4], &[0, 0, 0, 0]);
        state.on_write(&[REG_CLEAR, 1]);
        assert_eq!(&read_at(&mut state, REG_TRANS)[..4], &[0, 0, 0, 0]);
        assert_eq!(read_at(&mut state, REG_SHAPE)[0], SHAPE_NONE);
    }

    #[test]
    fn a_refusal_spends_the_arm_and_leaves_nothing() {
        let mut state = State::new(2);
        state.on_write(&[REG_CLEAR, 1]);
        state.on_transaction(SHAPE_WRITE);
        state.on_write(&[REG_NACK, 1]);
        assert!(state.nack_armed());
        assert_eq!(read_at(&mut state, REG_NACK)[0], 1);
        state.on_transaction(SHAPE_WRITE);
        // The master's refused write: the address was taken, the data
        // byte was not. The abort is what counts it.
        state.on_abort();
        assert!(!state.nack_armed());
        assert_eq!(read_at(&mut state, REG_NACK)[0], 0);
        assert_eq!(&read_at(&mut state, REG_BYTES)[..4], &[0, 0, 0, 0]);
        assert_eq!(&read_at(&mut state, REG_TRANS)[..4], &[3, 0, 0, 0]);
    }
}




