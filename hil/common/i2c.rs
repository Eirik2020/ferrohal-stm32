//! The I2C checks, shared by the bench firmwares. No slave is fitted: with
//! the lines pulled up internally, a vacant address is answered by the bus
//! itself with a NACK, which is what the checks prove the driver's error
//! paths with. Every kind of transfer is reported as that NACK within the
//! driver's own bound, measured with the cycle counter; the flags are
//! cleared and the bus put back, so the next one runs; the health counts
//! them; and the counted bound, without a clock, still ends.
//!
//! The timing registers and the refusals are the run binaries' own checks:
//! they read the registers through metapac and open the bus with a
//! handed-back wiring, which the shared file cannot.

// Each run binary compiles this file and uses the checks its links need.
#![allow(dead_code)]

use cortex_m::peripheral::DWT;
use embedded_hal::i2c::{I2c as HalI2c, Operation};
use ferrohal_stm32::{i2c::{Error, I2c}, time::Hertz};

use crate::tests::{Fail, Report};

/// The vacant address the checks ask for: nothing on the bench answers it,
/// so the bus itself gives the NACK.
const VACANT: u8 = 0x18;
/// Over the driver's own bound, for the cycle counter reads around the
/// call: a few hundred cycles of overhead.
const SLACK: u32 = 2_000;

/// The driver's bound for `bits` of wire time at `speed` on a `cpu` core:
/// twice the wire time plus the driver's margin.
fn bound(cpu: Hertz, speed: Hertz, bits: u32) -> u32 {
    let bit = cpu.0 / speed.0.max(1);
    2 * bits.saturating_mul(bit) + 20_000
}

/// A transfer to the vacant address: a NACK, and within its bound.
fn nack_within(result: Result<(), Error>, waited: u32, bound: u32) -> Result<(), Fail> {
    match result {
        Err(Error::Nack) => Ok(()),
        Err(other) => Err(Fail::I2cNotNack(Some(other))),
        Ok(()) => Err(Fail::I2cNotNack(None)),
    }
    .and_then(|()| {
        if waited <= bound + SLACK {
            Ok(())
        } else {
            Err(Fail::Time {
                cycles: waited,
                min: 0,
                max: bound + SLACK,
            })
        }
    })
}

/// The NACK suite over the open bus. `cpu` and the bus's own frequency
/// answer the bound; every transfer the checks make ends a NACK.
pub fn suite(report: &mut Report, link: &str, bus: &mut I2c, cpu: Hertz) {
    let speed = bus.frequency();
    let mut nacks = 0;

    // A write: NACK within the bound, measured, and the time kept for the
    // recovery check.
    #[allow(unused_assignments)]
    let mut first = 0;
    report.check_for(
        link,
        "I2C-003,I2C-004",
        "a vacant write nacks within its bound",
        {
            let start = DWT::cycle_count();
            let result = HalI2c::write(bus, VACANT, &[0xA5; 4]);
            first = DWT::cycle_count().wrapping_sub(start);
            nacks += 1;
            nack_within(result, first, bound(cpu, speed, 12 + 9 * 4))
        },
    );

    // A read: the same NACK, asked the other way.
    report.check_for(
        link,
        "I2C-003,I2C-004",
        "a vacant read nacks within its bound",
        {
            let mut read = [0u8; 8];
            let start = DWT::cycle_count();
            let result = HalI2c::read(bus, VACANT, &mut read);
            let waited = DWT::cycle_count().wrapping_sub(start);
            nacks += 1;
            nack_within(result, waited, bound(cpu, speed, 12 + 9 * 8))
        },
    );

    // A write-read: one transaction, two segments in the contract's sense.
    report.check_for(
        link,
        "I2C-004,I2C-006",
        "a vacant write-read nacks within its bound",
        {
            let start = DWT::cycle_count();
            let result = HalI2c::write_read(bus, VACANT, &[0x28; 2], &mut [0u8; 2]);
            let waited = DWT::cycle_count().wrapping_sub(start);
            nacks += 1;
            nack_within(result, waited, bound(cpu, speed, 12 + 9 * 2))
        },
    );

    // A transaction with a segment, an empty operation inside it, and two
    // more segments: the NACK of the first ends the whole transaction,
    // which must not spend the later segments' wire time.
    report.check_for(
        link,
        "I2C-006",
        "a vacant transaction nacks at its first segment",
        {
            let start = DWT::cycle_count();
            let result = HalI2c::transaction(
                bus,
                VACANT,
                &mut [
                    Operation::Write(&[0x28; 2]),
                    Operation::Write(&[0x00; 0]),
                    Operation::Read(&mut [0u8; 2]),
                    Operation::Write(&[0x55]),
                ],
            );
            let waited = DWT::cycle_count().wrapping_sub(start);
            nacks += 1;
            nack_within(result, waited, bound(cpu, speed, 12 + 9 * 2))
        },
    );

    // Every NACK above recovered the bus: another write answers at the
    // speed of the first, none the worse for the recoveries between.
    report.check_for(
        link,
        "I2C-003,I2C-004",
        "the bus answers after every recovery",
        {
            let start = DWT::cycle_count();
            let result = HalI2c::write(bus, VACANT, &[0x5A; 1]);
            let waited = DWT::cycle_count().wrapping_sub(start);
            nacks += 1;
            nack_within(result, waited, bound(cpu, speed, 12 + 9 * 1)).and_then(|()| {
                if waited <= first + SLACK {
                    Ok(())
                } else {
                    Err(Fail::Time {
                        cycles: waited,
                        min: 0,
                        max: first + SLACK,
                    })
                }
            })
        },
    );

    // The health: every NACK counted, nothing completed, nothing stuck.
    report.check_for(link, "I2C-004", "the health counts the nacks", {
        let health = bus.health();
        if health.nacks != nacks {
            Err(Fail::I2cHealth {
                field: "nacks",
                expected: nacks,
                got: health.nacks,
            })
        } else if health.transactions != 0 {
            Err(Fail::I2cHealth {
                field: "transactions",
                expected: 0,
                got: health.transactions,
            })
        } else if health.timeouts != 0 || health.busy != 0 {
            Err(Fail::I2cHealth {
                field: "timeouts or busy",
                expected: 0,
                got: health.timeouts + health.busy,
            })
        } else {
            Ok(())
        }
    });

    // Without a caller clock the bound is counted, which runs long but
    // must still end: this write is bounded by twelve times the driver's
    // clocked bound, in wall cycles.
    report.check_for(
        link,
        "I2C-003",
        "the counted bound ends without a clock",
        {
            bus.set_clock(None);
            let start = DWT::cycle_count();
            let result = HalI2c::write(bus, VACANT, &[0xA5; 4]);
            let waited = DWT::cycle_count().wrapping_sub(start);
            bus.set_clock(Some(&crate::tests::DWT_CLOCK));
            nack_within(result, waited, 12 * bound(cpu, speed, 12 + 9 * 4))
        },
    );
}

