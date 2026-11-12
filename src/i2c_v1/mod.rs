//! I2C v1 (`i2c_v1`, metapac's `i2c_v1`: the CR1/CR2/OAR/DR/SR1/SR2/CCR/
//! TRISE register set of the F2 and F4): the register operations the
//! [`crate::i2c`] driver needs. The driver itself, and the routing traits
//! the glue implements, are in [`crate::i2c`].
//!
//! The events the driver walks (RM0368 §27.3.3; the interface stretches
//! SCL until each one's software sequence is done, so a bounded poll
//! cannot miss one):
//!
//! - **EV5** (SB): the start is out. Cleared by reading SR1 then writing
//!   the address byte to DR.
//! - **EV6** (ADDR): the address was acknowledged. Cleared by reading
//!   SR1 then SR2. A one-byte receive disables ACK before the clear and
//!   (on the last segment) arms its stop right after; a two-byte receive
//!   disables ACK and sets POS, which stretches SCL over the second byte
//!   until the stop or repeated start is armed.
//! - **EV7 / EV7_1** (RxNE): a byte is in DR. The second-to-last byte of
//!   a receive of three or more disables ACK as it is taken; the last
//!   byte arms the stop, or the repeated start of the next segment.
//! - **EV8 / EV8_2** (TxE / BTF): the data register is empty / the last
//!   byte is fully out. A write segment's stop is armed at BTF.
//!
//! A NACK during the address or a byte sets SR1's AF; the bus errors set
//! BERR, ARLO or OVR. All four clear by writing zero, and PE off then on
//! resets the state machines and releases the lines while the timing
//! registers keep their values (RM0368 §27.6.2).
//!
//! Requirements: I2C-005, I2CV1-001, I2C-002, I2C-004, I2C-006

use stm32_metapac::i2c::I2c as Regs;

pub use crate::i2c::{ConfigError, Error};
use crate::time::Hertz;

/// Register reads a recovery waits for the stop it armed to reach the
/// wire, bounded.
const SPINS: u32 = 10_000;

/// The register block.
pub type Block = Regs;

/// One I2C's registers, as the driver uses them.
#[derive(Clone, Copy)]
pub struct Unit(Regs);

/// The timing values `apply` writes: CR2's FREQ, CCR's mode, duty and
/// divider, and TRISE (RM0368 §27.6.4, §27.6.11, §27.6.12, §27.6.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    /// PCLK1 in MHz, the CR2 FREQ field (six bits).
    freq: u8,
    /// CCR's F/S: standard or fast mode.
    fast: bool,
    /// CCR's DUTY, which only fast mode reads.
    duty: bool,
    /// CCR[11:0]: the divider, TPCLK1 counts.
    ccr: u16,
    /// TRISE[5:0]: the maximum rise time, TPCLK1 counts.
    trise: u8,
    /// The SCL these values run.
    speed: Hertz,
}

impl Timing {
    /// The SCL these values run on the wire. RM0368 §27.6.11: standard
    /// mode holds each half CCR counts; fast mode with DUTY low holds
    /// SCL low two CCR counts and high one; DUTY high holds low sixteen
    /// and high nine.
    pub const fn speed(&self) -> Hertz {
        self.speed
    }
}

impl Unit {
    pub(crate) const fn new(regs: Regs) -> Self {
        Self(regs)
    }

    /// The timing for `speed` from a PCLK1 of `kernel_clock`, or why the
    /// pair cannot run. Fm mode is anything above 100 kHz; the divider
    /// lands at or under the speed asked for, DUTY low first, with CCR's
    /// minimum of 4 (1 in DUTY-high fast mode) and maximum 0x0FFF
    /// (RM0368 §27.6.11); TRISE carries the I2C specification's maximum
    /// rise time, 1000 ns in standard mode and 300 ns in fast, as whole
    /// PCLK1 counts plus one (RM0368 §27.6.13). PCLK1 must be a whole
    /// number of MHz, 2 to 50 (the CR2 FREQ field, RM0368 §27.4).
    pub const fn timing(kernel_clock: Hertz, speed: Hertz) -> Result<Timing, ConfigError> {
        if kernel_clock.0 < 2_000_000 || kernel_clock.0 > 50_000_000 {
            return Err(ConfigError::KernelClock);
        }
        if !kernel_clock.0.is_multiple_of(1_000_000) {
            return Err(ConfigError::KernelClock);
        }
        if speed.0 < 1_000 || speed.0 > 400_000 {
            return Err(ConfigError::Speed);
        }
        let freq = (kernel_clock.0 / 1_000_000) as u8;
        let fast = speed.0 > 100_000;
        // The divider: whole TPCLK1 counts, CCR's minimum and maximum
        // observed (RM0368 §27.6.11).
        let (duty, ccr) = if !fast {
            let ccr = kernel_clock.0 / (2 * speed.0);
            if ccr < 4 || ccr > 0x0FFF {
                return Err(ConfigError::Speed);
            }
            (false, ccr)
        } else {
            let duty0 = kernel_clock.0 / (3 * speed.0);
            if duty0 >= 4 && duty0 <= 0x0FFF {
                (false, duty0)
            } else {
                let duty1 = kernel_clock.0 / (25 * speed.0);
                if duty1 < 1 || duty1 > 0x0FFF {
                    return Err(ConfigError::Speed);
                }
                (true, duty1)
            }
        };
        // TRISE: the maximum rise time over whole PCLK1 counts, plus
        // one (RM0368 §27.6.13); 300 ns in fast mode, else 1000 ns.
        let rise_ns = if fast { 300 } else { 1000 };
        let trise = rise_ns * freq as u32 / 1_000 + 1;
        if trise > 63 {
            return Err(ConfigError::KernelClock);
        }
        // The SCL on the wire, from the same counts: one TPCLK1 is
        // 1000/freq ns, so the period is counts x 1000/freq ns and the
        // speed 1e6 x freq / counts kHz.
        let counts = if !fast {
            2 * ccr
        } else if !duty {
            3 * ccr
        } else {
            25 * ccr
        };
        let speed_out = Hertz(1_000_000 * freq as u32 / counts);
        Ok(Timing {
            freq,
            fast,
            duty,
            ccr: ccr as u16,
            trise: trise as u8,
            speed: speed_out,
        })
    }

    /// Writes the timing with PE clear (TRISE and CCR are written only
    /// while the I2C is disabled, RM0368 §27.6.13); `arm` sets PE.
    pub(crate) fn apply(self, timing: &Timing) {
        let regs = self.0;
        regs.cr1().modify(|w| w.set_pe(false));
        regs.cr2().write(|w| w.set_freq(timing.freq));
        regs.ccr().write(|w| {
            w.set_ccr(timing.ccr);
            w.set_f_s(match timing.fast {
                false => stm32_metapac::i2c::vals::FS::STANDARD,
                true => stm32_metapac::i2c::vals::FS::FAST,
            });
            w.set_duty(match timing.duty {
                false => stm32_metapac::i2c::vals::Duty::DUTY2_1,
                true => stm32_metapac::i2c::vals::Duty::DUTY16_9,
            });
        });
        regs.trise().write(|w| w.set_trise(timing.trise));
    }

    /// Closes the I2C: PE off. Only a teardown (`I2c::free`) calls this —
    /// the driver keeps PE on for its life. A reopen runs `new` again,
    /// whose `apply` writes with PE clear first, as RM0368 §27.6.13
    /// requires.
    pub(crate) fn disable(self) {
        self.0.cr1().modify(|w| w.set_pe(false));
    }

    /// SR2's BUSY, which clears once the stop has been out (RM0368
    /// §27.6.8).
    pub(crate) fn busy(self) -> bool {
        self.0.sr2().read().busy()
    }

    /// PE on, POS low, and START, with ACK high only for a receive of
    /// three or more bytes (the one- and two-byte sequences set it low
    /// at EV6). With the bus running, START sends a repeated start
    /// instead (RM0368 §27.3.3, §27.3.11).
    pub(crate) fn arm(self, _addr: u8, read: bool, len: usize, _last: bool) {
        self.0.cr1().modify(|w| {
            w.set_pe(true);
            w.set_start(true);
            w.set_pos(stm32_metapac::i2c::vals::Pos::CURRENT);
            w.set_ack(read && len > 2);
        });
    }

    /// One step of the address phase: EV5 sends the address byte, EV6
    /// clears ADDR and arms the one- and two-byte receive sequences.
    /// True once the phase is done. The interface stretches SCL through
    /// both events, so no step can be missed by polling (RM0368 §27.3.3).
    pub(crate) fn addr_step(self, addr: u8, read: bool, len: usize, last: bool) -> bool {
        let regs = self.0;
        let sr1 = regs.sr1().read();
        if sr1.start() {
            let _ = regs.sr1().read();
            regs.dr().write(|w| w.set_dr(addr << 1 | u8::from(read)));
            return false;
        }
        if sr1.addr() {
            // A one-byte receive disables ACK before ADDR clears, and
            // arms its stop right after if this is the last segment; a
            // two-byte receive sets POS, which stretches SCL over the
            // second byte until the tail is armed.
            if read && len == 1 {
                regs.cr1().modify(|w| w.set_ack(false));
            }
            if read && len == 2 {
                regs.cr1().modify(|w| {
                    w.set_ack(false);
                    w.set_pos(stm32_metapac::i2c::vals::Pos::NEXT);
                });
            }
            let _ = regs.sr1().read();
            let _ = regs.sr2().read();
            if read && len == 1 && last {
                regs.cr1().modify(|w| w.set_stop(true));
            }
            return true;
        }
        false
    }

    /// EV8: the data register is empty. The counts are v2's chunking
    /// bookkeeping, unused here.
    pub(crate) fn tx_ready(self, _position: usize, _len: usize) -> bool {
        self.0.sr1().read().txe()
    }

    /// One byte out (RM0368 §27.3.2).
    pub(crate) fn send(self, byte: u8) {
        self.0.dr().write(|w| w.set_dr(byte));
    }

    /// EV7: a byte is in DR. The two-byte sequence waits at BTF for its
    /// second byte, with SCL stretched until the tail is armed (RM0368
    /// §27.3.3).
    pub(crate) fn recv_ready(self, position: usize, len: usize, _last: bool) -> bool {
        if len == 2 && position == 1 {
            return self.0.sr1().read().btf();
        }
        self.0.sr1().read().rxne()
    }

    /// One byte in, and the receive tail: the second-to-last byte of a
    /// receive of three or more disables ACK, and the last arms the
    /// stop, or the repeated start of the next segment (RM0368 §27.3.3).
    pub(crate) fn recv(self, position: usize, len: usize, last: bool) -> u8 {
        let regs = self.0;
        if len > 2 && position + 2 == len {
            regs.cr1().modify(|w| w.set_ack(false));
        }
        let byte = regs.dr().read().dr();
        if position + 1 == len && len > 1 {
            regs.cr1().modify(|w| {
                if last {
                    w.set_stop(true);
                } else {
                    w.set_start(true);
                }
            });
        }
        if position + 1 == len && len == 1 && !last {
            // A one-byte receive mid-transaction: its NACK is armed, and
            // the repeated start follows the byte.
            regs.cr1().modify(|w| w.set_start(true));
        }
        byte
    }

    /// A write segment ends at EV8_2 (BTF: the last byte fully out) so
    /// its stop or the next start comes after it (RM0368 §27.3.3); a
    /// read segment armed its tail while taking its last byte.
    pub(crate) fn op_done(self, read: bool, _last: bool) -> bool {
        if read {
            return true;
        }
        self.0.sr1().read().btf()
    }

    /// Arms the stop that closes a write segment; a read segment armed
    /// its own tail, and a repeated start is the next `arm`'s START.
    pub(crate) fn end_op(self, read: bool, last: bool) {
        if !read && last {
            self.0.cr1().modify(|w| w.set_stop(true));
        }
    }

    /// SR2's BUSY clears once the stop has been out (RM0368 §27.6.8).
    pub(crate) fn stop_done(self) -> bool {
        !self.0.sr2().read().busy()
    }

    /// Nothing to clear: BUSY drops by itself.
    pub(crate) fn stop_clear(self) {}

    /// AF, BERR, ARLO and OVR, each cleared as it is taken, by writing
    /// zero (RM0368 §27.6.7).
    pub(crate) fn error(self) -> Option<Error> {
        let regs = self.0;
        let sr1 = regs.sr1().read();
        if sr1.af() {
            regs.sr1().modify(|w| w.set_af(false));
            return Some(Error::Nack);
        }
        if sr1.berr() {
            regs.sr1().modify(|w| w.set_berr(false));
            return Some(Error::Bus);
        }
        if sr1.arlo() {
            regs.sr1().modify(|w| w.set_arlo(false));
            return Some(Error::ArbitrationLost);
        }
        if sr1.ovr() {
            regs.sr1().modify(|w| w.set_ovr(false));
            return Some(Error::Overrun);
        }
        None
    }

    /// Puts the bus back: the stop the refused transfer never got, then
    /// the flags cleared by writing zero (RM0368 §27.6.7), the bus waited
    /// back to idle within a bound (BUSY clears only after a stop, RM0368
    /// §27.6.8), and PE cycled, which releases the lines while the
    /// timing registers keep their values (RM0368 §27.6.2).
    pub(crate) fn recover(self) {
        let regs = self.0;
        regs.cr1().modify(|w| {
            w.set_start(false);
            w.set_stop(true);
            w.set_ack(false);
            w.set_pos(stm32_metapac::i2c::vals::Pos::CURRENT);
        });
        regs.sr1().modify(|w| {
            w.set_af(false);
            w.set_berr(false);
            w.set_arlo(false);
            w.set_ovr(false);
        });
        for _ in 0..SPINS {
            if !regs.sr2().read().busy() {
                break;
            }
        }
        regs.cr1().modify(|w| w.set_pe(false));
        regs.cr1().modify(|w| w.set_pe(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RM0368 §27.6.11's own worked example: FREQ 08 at 100 kHz standard
    /// mode, CCR 0x28; and §27.6.13's TRISE 09h with it.
    #[test]
    fn the_manuals_example_timing() {
        let timing = Unit::timing(Hertz::mhz(8), Hertz::khz(100)).expect("8 MHz to 100 kHz runs");
        assert_eq!(timing.freq, 8);
        assert!(!timing.fast);
        assert_eq!(timing.ccr, 0x28);
        assert_eq!(timing.trise, 9);
        assert_eq!(timing.speed(), Hertz::khz(100));
    }

    /// The boards' PCLK1: the F401 and F405 benches and flight images run
    /// 42 MHz on APB1.
    #[test]
    fn fast_mode_from_the_boards_pclk1() {
        let timing = Unit::timing(Hertz::mhz(42), Hertz::khz(400)).expect("42 MHz to 400 kHz runs");
        assert!(timing.fast);
        assert!(!timing.duty);
        // 42 MHz / (3 x 400 kHz) = 35; TRISE 300 ns + 1.
        assert_eq!(timing.ccr, 35);
        assert_eq!(timing.trise, 13);
        assert_eq!(timing.speed(), Hertz::khz(400));
        let timing = Unit::timing(Hertz::mhz(42), Hertz::khz(100)).expect("42 MHz to 100 kHz runs");
        assert!(!timing.fast);
        assert_eq!(timing.ccr, 210);
        assert_eq!(timing.trise, 43);
        assert_eq!(timing.speed(), Hertz::khz(100));
    }

    #[test]
    fn out_of_range_is_refused() {
        assert_eq!(
            Unit::timing(Hertz::mhz(42), Hertz(401_000)),
            Err(ConfigError::Speed)
        );
        assert_eq!(
            Unit::timing(Hertz::mhz(42), Hertz(900)),
            Err(ConfigError::Speed)
        );
        assert_eq!(
            Unit::timing(Hertz(42_500_000), Hertz::khz(100)),
            Err(ConfigError::KernelClock)
        );
        assert_eq!(
            Unit::timing(Hertz::mhz(1), Hertz::khz(100)),
            Err(ConfigError::KernelClock)
        );
    }
}
