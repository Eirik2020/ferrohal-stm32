//! The I2C live-device checks, shared by the bench firmwares. They run
//! against the Pico 1 simulator (`hil/pico-i2c-sim`, wired per
//! `hil/WIRING.md`): a real DW `apb_i2c` slave at `SIM`, whose register
//! map this file and the simulator both implement — the two cannot
//! share code, so they must stay in step byte for byte, and the ID and
//! counter cross-checks here prove they are.
//!
//! What the NACK-only suite (`i2c.rs`) proves with the bus's own answer,
//! this suite proves against a device that answers: the ACK'd paths —
//! 1-, 2- and N-byte receives, a write that lands, a repeated start the
//! slave observes — the driver's bounds against a slave that stretches
//! SCL within them and past them, the data-phase refusal
//! (`IC_SLV_DATA_NACK_ONLY`, an address ACK followed by a NACKed byte),
//! the exact health accounting over live traffic, and the vacant
//! address still nacking after all of it. The simulator's own counters
//! (what the wire did) are compared against the transactions the master
//! started, so neither side's word is taken on faith.

// Each run binary compiles this file and uses the checks its links need.
#![allow(dead_code)]

use cortex_m::peripheral::DWT;
use embedded_hal::i2c::I2c as HalI2c;
use ferrohal_stm32::{
    i2c::{Error, I2c, I2cParts},
    time::Hertz,
};

use crate::tests::{Fail, Report};

/// The simulator's address. Nothing on either bench answers `VACANT`, so
/// the NACK-only suites and this one never see each other's traffic.
const SIM: u8 = 0x2A;
const VACANT: u8 = 0x18;

// The simulator's register map, exactly as hil/pico-i2c-sim/src/lib.rs
// spells it. A change there is a change here, or the cross-checks fail.
const REG_ID: u8 = 0x00;
const REG_CAPS: u8 = 0x02;
const REG_SEED: u8 = 0x03;
const REG_PATTERN: u8 = 0x04;
const REG_ECHO: u8 = 0x10;
const REG_STRETCH: u8 = 0x20;
const REG_NACK: u8 = 0x22;
const REG_BYTES: u8 = 0x30;
const REG_TRANS: u8 = 0x34;
const REG_SHAPE: u8 = 0x38;
const REG_CLEAR: u8 = 0x3C;

const CAP_DATA_NACK: u8 = 1 << 0;
const CAP_STRETCH: u8 = 1 << 1;
/// The shape a write-read transaction leaves in the simulator's map.
const SHAPE_WRITE_READ: u8 = 3;
/// The ID register's first byte: the simulator's name.
const SIM_ID: u8 = 0xA1;

/// A few hundred cycles of overhead around the cycle-counter reads.
const SLACK: u32 = 2_000;

/// The driver's bound for `bits` of wire time at `speed` on a `cpu`
/// core: the same recipe the NACK-only suite uses.
fn bound(cpu: Hertz, speed: Hertz, bits: u32) -> u32 {
    let bit = cpu.0 / speed.0.max(1);
    2 * bits.saturating_mul(bit) + 20_000
}

/// Byte `index` of the simulator's pattern stream — the same hash as
/// the simulator's and `tests.rs`'s.
fn pattern(index: u32) -> u8 {
    (index.wrapping_mul(2_654_435_761) >> 24) as u8
}

/// Spins `ms` away on the cycle counter, so waits between transfers do
/// not need a caller clock.
fn spin(cpu: Hertz, ms: u32) {
    let start = DWT::cycle_count();
    let cycles = cpu.0 / 1000 * ms;
    while DWT::cycle_count().wrapping_sub(start) < cycles {}
}

/// One live bus against the simulator. Every transfer the checks make
/// goes through here, so the transactions the master started can be
/// compared with the ones the simulator says it saw.
struct Live<'a, U: I2cParts> {
    bus: &'a mut I2c<U>,
    /// Transactions that reached the wire: what the simulator must
    /// count. A transfer refused before the wire moved (a Busy) never
    /// got there, so it is not counted; a refused data byte got as far
    /// as the wire, so it is.
    trans: u32,
    /// Transactions that completed: what the driver's health must count.
    done: u32,
}

impl<U: I2cParts> Live<'_, U> {
    /// One write transaction: the register, then its payload.
    fn write(&mut self, reg: u8, payload: &[u8]) -> Result<(), Fail> {
        let mut frame = [reg; 33];
        frame[1..=payload.len()].copy_from_slice(payload);
        HalI2c::write(self.bus, SIM, &frame[..=payload.len()]).map_err(Fail::I2c)?;
        self.trans += 1;
        self.done += 1;
        Ok(())
    }

    /// One write-read transaction: point at the register, read `out`.
    fn read(&mut self, reg: u8, out: &mut [u8]) -> Result<(), Fail> {
        HalI2c::write_read(self.bus, SIM, &[reg], out).map_err(Fail::I2c)?;
        self.trans += 1;
        self.done += 1;
        Ok(())
    }

    /// Reads `out` of the pattern stream, and says the bytes matched.
    fn pattern_matches(&mut self, len: usize, out: &mut [u8]) -> Result<(), Fail> {
        let seed = 0x53;
        self.write(REG_SEED, &[seed])?;
        out.fill(0);
        self.read(REG_PATTERN, &mut out[..len])?;
        for (i, byte) in out[..len].iter().enumerate() {
            let sent = pattern(u32::from(seed) + i as u32);
            if *byte != sent {
                return Err(Fail::I2cData {
                    at: i,
                    sent,
                    got: *byte,
                });
            }
        }
        Ok(())
    }

    /// One write-read transaction with the cycles it took. The caller
    /// does the counting: some of these complete and some do not.
    fn read_measured(&mut self, reg: u8, out: &mut [u8]) -> (Result<(), Error>, u32) {
        let start = DWT::cycle_count();
        let result = HalI2c::write_read(self.bus, SIM, &[reg], out);
        let waited = DWT::cycle_count().wrapping_sub(start);
        (result, waited)
    }
}

/// The live suite over the open bus, against the simulator on bus
/// `tag` (1: the F401's I2C3, 2: the H753's I2C4). It runs after the
/// NACK-only suite; the health deltas start from the baseline taken
/// here, so what earlier checks counted does not matter. The
/// `recover_cycles` allowance is what the driver's bounded recovery can
/// add past its deadline while the simulator still holds the line: the
/// F401's v1 spins on BUSY, the H7's v2 clears and returns.
pub fn suite<U: I2cParts>(
    report: &mut Report,
    link: &str,
    bus: &mut I2c<U>,
    cpu: Hertz,
    tag: u8,
    recover_cycles: u32,
) {
    let speed = bus.frequency();
    let baseline = bus.health();
    let mut live = Live {
        bus,
        trans: 0,
        done: 0,
    };
    let mut scratch = [0u8; 32];

    // Zero the simulator's counters through the map, so each run starts
    // from zero without power-cycling the Pico.
    let _ = live.write(REG_CLEAR, &[1]);

    // The identity: the simulator answers, with its own name, the bus
    // it serves, and the capabilities both ends must agree on.
    report.check_for(
        link,
        "I2C-003,I2C-006",
        "the simulator answers with its identity",
        (|| {
            let mut id = [0u8; 3];
            live.read(REG_ID, &mut id)?;
            let caps = CAP_DATA_NACK | CAP_STRETCH;
            if id[0] != SIM_ID {
                Err(Fail::I2cData {
                    at: 0,
                    sent: SIM_ID,
                    got: id[0],
                })
            } else if id[1] != tag {
                Err(Fail::I2cData {
                    at: 1,
                    sent: tag,
                    got: id[1],
                })
            } else if id[2] != caps {
                Err(Fail::I2cData {
                    at: 2,
                    sent: caps,
                    got: id[2],
                })
            } else {
                Ok(())
            }
        })(),
    );

    // One- and two-byte receives take their own arming on the v1 IP,
    // and the 32-byte one runs the FIFO path of both IPs: every length
    // must bring back exactly the stream both ends compute.
    report.check_for(
        link,
        "I2C-003,I2C-006",
        "a one-byte read returns the byte",
        live.pattern_matches(1, &mut scratch),
    );
    report.check_for(
        link,
        "I2C-003,I2C-006",
        "a two-byte read returns both bytes",
        live.pattern_matches(2, &mut scratch),
    );
    report.check_for(
        link,
        "I2C-003,I2C-006",
        "a 32-byte read returns the whole stream",
        live.pattern_matches(32, &mut scratch),
    );

    // A write that lands, read back through a repeated start.
    report.check_for(
        link,
        "I2C-006",
        "a write lands and reads back",
        (|| {
            let sent = [0x9C, 0xA5, 0x3C, 0x5A, 0x00, 0xFF, 0x42, 0x7E];
            let mut back = [0u8; 8];
            live.write(REG_ECHO, &sent)?;
            live.read(REG_ECHO, &mut back)?;
            for (i, byte) in back.iter().enumerate() {
                if *byte != sent[i] {
                    return Err(Fail::I2cData {
                        at: i,
                        sent: sent[i],
                        got: *byte,
                    });
                }
            }
            Ok(())
        })(),
    );

    // The slave saw the shape the master made: that read back was one
    // write-read transaction, and the map says so.
    report.check_for(
        link,
        "I2C-006",
        "the simulator saw the repeated start",
        (|| {
            let mut shape = [0u8; 1];
            live.read(REG_SHAPE, &mut shape)?;
            if shape[0] != SHAPE_WRITE_READ {
                Err(Fail::I2cData {
                    at: 0,
                    sent: SHAPE_WRITE_READ,
                    got: shape[0],
                })
            } else {
                Ok(())
            }
        })(),
    );

    // A stretch inside the bound: the slave holds SCL before it
    // answers, and the driver's own bound still covers the wait. The
    // read takes the wire time plus the stretch; without the stretch
    // it cannot take that long.
    report.check_for(
        link,
        "I2C-003",
        "a stretch within the bound still answers",
        (|| {
            let stretch_us = 1_000;
            let len = 4;
            // The wire bits of a write-read, the driver's own recipe: a
            // segment each, a start, an address and an acknowledge over
            // the byte counts of both.
            let bits = (12 + 9) + (12 + 9 * len as u32);
            live.write(REG_STRETCH, &(stretch_us as u16).to_le_bytes())?;
            let (result, waited) = live.read_measured(REG_PATTERN, &mut scratch[..len]);
            live.trans += 1;
            live.done += 1;
            result.map_err(Fail::I2c)?;
            let bit = cpu.0 / speed.0.max(1);
            let floor = bits * bit + stretch_us * cpu.0 / 1_000_000 - SLACK;
            let ceiling = bound(cpu, speed, bits) + SLACK;
            if waited < floor || waited > ceiling {
                Err(Fail::Time {
                    cycles: waited,
                    min: floor,
                    max: ceiling,
                })
            } else {
                Ok(())
            }
        })(),
    );

    // A stretch past the bound: the read times out inside the bound,
    // while the slave still holds the bus.
    report.check_for(
        link,
        "I2C-003",
        "a stretch past the bound times out",
        (|| {
            let len = 4;
            // As in the stretch within the bound: the driver's per-segment
            // recipe, which is the deadline it runs.
            let bits = (12 + 9) + (12 + 9 * len as u32);
            live.write(REG_STRETCH, &50_000u16.to_le_bytes())?;
            let (result, waited) = live.read_measured(REG_PATTERN, &mut scratch[..len]);
            live.trans += 1; // it reached the wire; it did not complete
            match result {
                Err(Error::Timeout) => {
                    // The deadline the driver ran, plus what its bounded
                    // recovery can add while the line is still held.
                    let ceiling = bound(cpu, speed, bits) + recover_cycles + SLACK;
                    if waited > ceiling {
                        Err(Fail::Time {
                            cycles: waited,
                            min: 0,
                            max: ceiling,
                        })
                    } else {
                        Ok(())
                    }
                }
                Err(other) => Err(Fail::I2cNotTimeout(Some(other))),
                Ok(()) => Err(Fail::I2cNotTimeout(None)),
            }
        })(),
    );

    // The slave keeps holding SCL, so the bus is still busy: the next
    // transfer says so inside its own bound. The F401's v1 sees the held
    // line and refuses before the wire moves (Busy); the H7's v2 runs
    // its hospitalities in hardware and loses the flag in its own
    // recovery, so it starts and its bound reports the stuck bus as a
    // Timeout. Either way the transfer is refused within its bound.
    // Neither is counted by the simulator: no address ever clocked.
    let mut stuck_as_timeout = false;
    report.check_for(
        link,
        "I2C-003",
        "the stuck bus reports Busy within its bound",
        (|| {
            let start = DWT::cycle_count();
            let result = HalI2c::write(live.bus, SIM, &[REG_CLEAR, 1]);
            let waited = DWT::cycle_count().wrapping_sub(start);
            match result {
                Err(Error::Busy) => stuck_as_timeout = false,
                Err(Error::Timeout) => stuck_as_timeout = true,
                Err(other) => return Err(Fail::I2cNotBusy(Some(other))),
                Ok(()) => return Err(Fail::I2cNotBusy(None)),
            }
            let ceiling = bound(cpu, speed, 12 + 9 * 2) + SLACK;
            if waited > ceiling {
                Err(Fail::Time {
                    cycles: waited,
                    min: 0,
                    max: ceiling,
                })
            } else {
                Ok(())
            }
        })(),
    );

    // Once the slave lets go, the bus answers again. The device has just
    // stretched itself out of a timeout, so the transfer that resyncs it
    // can be the one refused — the F401's v1 sees the held line and a
    // retry is what reaches the map; the H7's v2 cannot and fails one
    // attempt on the way. Both drivers promise the next transfer starts;
    // the device answers the one after.
    let mut recovery_nacks = 0u32;
    report.check_for(
        link,
        "I2C-003",
        "the bus answers once the slave lets go",
        (|| {
            spin(cpu, 80);
            let mut id = [0u8; 2];
            let mut result = live.read(REG_ID, &mut id);
            let mut tries = 0;
            while result.is_err() && tries < 3 {
                tries += 1;
                recovery_nacks += 1;
                spin(cpu, 5);
                id = [0u8; 2];
                result = live.read(REG_ID, &mut id);
            }
            result?;
            if id[0] != SIM_ID {
                Err(Fail::I2cData {
                    at: 0,
                    sent: SIM_ID,
                    got: id[0],
                })
            } else if id[1] != tag {
                Err(Fail::I2cData {
                    at: 1,
                    sent: tag,
                    got: id[1],
                })
            } else {
                Ok(())
            }
        })(),
    );

    // The data-phase refusal: the slave takes the address, then refuses
    // the first data byte. The master sees its NACK, and the map holds
    // nothing from the byte it refused.
    let mut extra_nacks = 0u32;
    report.check_for(
        link,
        "I2C-004",
        "a refused data byte nacks and leaves nothing",
        (|| {
            live.write(REG_NACK, &[1])?;
            // The simulator arms the DW block in the idle gap after the
            // write, a window a START can race: this throwaway read
            // absorbs it, and shows the arm through the map.
            match live.read(REG_NACK, &mut scratch[..1]) {
                Ok(()) => {
                    if scratch[0] != 1 {
                        return Err(Fail::I2cData {
                            at: 0,
                            sent: 1,
                            got: scratch[0],
                        });
                    }
                }
                Err(_) => extra_nacks += 1,
            }
            let start = DWT::cycle_count();
            let result = HalI2c::write(live.bus, SIM, &[REG_ECHO, 0xA5, 0x5A]);
            let waited = DWT::cycle_count().wrapping_sub(start);
            live.trans += 1; // the address was taken; the byte was not
            match result {
                Err(Error::Nack) => {
                    let ceiling = bound(cpu, speed, 12 + 9 * 3) + SLACK;
                    if waited > ceiling {
                        Err(Fail::Time {
                            cycles: waited,
                            min: 0,
                            max: ceiling,
                        })
                    } else {
                        Ok(())
                    }
                }
                Err(other) => Err(Fail::I2cNotNack(Some(other))),
                Ok(()) => Err(Fail::I2cNotNack(None)),
            }?;
            // The simulator finds the spent arm by polling the bus — the
            // refused byte is never delivered — and clears it after a
            // settle; the pause, then a throwaway read, covers both.
            spin(cpu, 3);
            if live.read(REG_ID, &mut scratch[..2]).is_err() {
                extra_nacks += 1;
            }
            let mut bytes = [0u8; 4];
            live.read(REG_BYTES, &mut bytes)?;
            if bytes != [0, 0, 0, 0] {
                Err(Fail::I2cData {
                    at: 0,
                    sent: 0,
                    got: bytes[0],
                })
            } else {
                Ok(())
            }
        })(),
    );

    // What the wire did, by the slave's own count: every transaction
    // the master started on the wire, including the timed-out read and
    // the refused write, and nothing the master never sent.
    report.check_for(
        link,
        "I2C-004",
        "the simulator counted the same transactions",
        (|| {
            let mut trans = [0u8; 4];
            live.read(REG_TRANS, &mut trans)?;
            let got = u32::from_le_bytes(trans);
            if got != live.trans {
                Err(Fail::Count {
                    expected: live.trans,
                    got,
                })
            } else {
                Ok(())
            }
        })(),
    );

    // And what the driver counted: the completed transactions, the one
    // timeout, the one busy, the one refused byte's NACK, and nothing
    // else.
    report.check_for(
        link,
        "I2C-004",
        "the health counts the live traffic",
        (|| {
            let health = live.bus.health();
            let expected = baseline.transactions + live.done;
            if health.transactions != expected {
                Err(Fail::I2cHealth {
                    field: "transactions",
                    expected,
                    got: health.transactions,
                })
            } else if health.timeouts != baseline.timeouts + 1 + u32::from(stuck_as_timeout) {
                Err(Fail::I2cHealth {
                    field: "timeouts",
                    expected: baseline.timeouts + 1 + u32::from(stuck_as_timeout),
                    got: health.timeouts,
                })
            } else if health.busy != baseline.busy + u32::from(!stuck_as_timeout) {
                Err(Fail::I2cHealth {
                    field: "busy",
                    expected: baseline.busy + u32::from(!stuck_as_timeout),
                    got: health.busy,
                })
            } else if health.nacks != baseline.nacks + 1 + extra_nacks + recovery_nacks {
                Err(Fail::I2cHealth {
                    field: "nacks",
                    expected: baseline.nacks + 1,
                    got: health.nacks,
                })
            } else if health.bus_errors != baseline.bus_errors
                || health.arbitration_lost != baseline.arbitration_lost
                || health.overruns != baseline.overruns
            {
                Err(Fail::I2cHealth {
                    field: "bus errors, arbitrations or overruns",
                    expected: baseline.bus_errors + baseline.arbitration_lost + baseline.overruns,
                    got: health.bus_errors + health.arbitration_lost + health.overruns,
                })
            } else {
                Ok(())
            }
        })(),
    );

    // The vacant address after all the live traffic: still a NACK
    // within its bound — recovery from success, not only from errors.
    report.check_for(
        link,
        "I2C-003,I2C-004",
        "a vacant write still nacks after the live traffic",
        (|| {
            let bits = 12 + 9 * 4;
            let start = DWT::cycle_count();
            let result = HalI2c::write(live.bus, VACANT, &[0xA5; 4]);
            let waited = DWT::cycle_count().wrapping_sub(start);
            match result {
                Err(Error::Nack) => {
                    let ceiling = bound(cpu, speed, bits) + SLACK;
                    if waited > ceiling {
                        Err(Fail::Time {
                            cycles: waited,
                            min: 0,
                            max: ceiling,
                        })
                    } else {
                        Ok(())
                    }
                }
                Err(other) => Err(Fail::I2cNotNack(Some(other))),
                Ok(()) => Err(Fail::I2cNotNack(None)),
            }
        })(),
    );
}
