//! I2C v2 (`i2c_v2`, metapac's `i2c_v2`: the CR1/CR2/TIMINGR/ISR/ICR
//! register set of the H7): the register operations the [`crate::i2c`]
//! driver needs. The driver itself, and the routing traits the glue
//! implements, are in [`crate::i2c`].
//!
//! A transfer is CR2's program: the address, the direction, NBYTES and
//! START, with AUTOEND on the last segment so the hardware sends its
//! stop, and software end (TC) between segments, where the next
//! segment's START is a repeated start (RM0433 §47.4.9, Table 389).
//! NBYTES is eight bits: a longer segment runs in chunks, RELOAD high
//! and the next loaded at TCR. TXIS and RXNE hand the bytes over.
//!
//! A NACK sets NACKF, the bus errors BERR, ARLO and OVR; all clear
//! through ICR, and PE off then on resets the state machines and
//! releases the lines while the timing register keeps its value
//! (RM0433 §47.4.6).
//!
//! Requirements: I2C-005, I2CV2-001

use stm32_metapac::i2c::I2c as Regs;

pub use crate::i2c::{ConfigError, Error};
use crate::time::Hertz;

/// The register block.
pub type Block = Regs;

/// One I2C's registers, as the driver uses them.
#[derive(Clone, Copy)]
pub struct Unit(Regs);

/// TIMINGR as one value (RM0433 §47.6.8): PRESC[31:28], SCLDEL[23:20],
/// SDADEL[19:16], SCLH[15:8], SCLL[7:0], and the SCL it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    /// The whole TIMINGR value, packed.
    timingr: u32,
    /// The SCL these values run.
    speed: Hertz,
}

impl Timing {
    /// The SCL these values run on the wire, from the same counts: the
    /// period is (SCLL + 1 + SCLH + 1) x tPRESC.
    pub const fn speed(&self) -> Hertz {
        self.speed
    }
}

impl Unit {
    pub(crate) const fn new(regs: Regs) -> Self {
        Self(regs)
    }

    /// The timing for `speed` from a kernel clock of `kernel_clock`, or
    /// why the pair cannot run.
    ///
    /// The recipe is RM0433 §47.4.5's, with the analog filter on and the
    /// digital filter off, and its Table 388's values: the SCL low and
    /// high times come from the mode's minimum low, with the rest of the
    /// asked period high; the smallest PRESC whose prescaled tick fits
    /// SCLL and SCLH in eight bits is taken; SCLDEL meets the data
    /// setup from the rise and Table 388's tSU;DAT; SDADEL sits between
    /// §47.4.5's bounds, the tAF of DS12117 Table 94 (50 to 260 ns) in
    /// them. Every count rounds up, so the SCL produced is at or below
    /// the speed asked for.
    pub const fn timing(kernel_clock: Hertz, speed: Hertz) -> Result<Timing, ConfigError> {
        if speed.0 < 1_000 || speed.0 > 1_000_000 {
            return Err(ConfigError::Speed);
        }
        if kernel_clock.0 == 0 {
            return Err(ConfigError::KernelClock);
        }
        // One kernel clock tick, in nanoseconds, rounded up so no
        // computed time is short.
        let tick = 1_000_000_000u32.div_ceil(kernel_clock.0);
        // The mode's specification values (RM0433 Table 388, and the I2C
        // specification's SCL times): minimum low and high, rise and
        // fall, data setup, and the maximum data hold.
        let (low_min, high_min, rise, fall, setup, hold_max): (u32, u32, u32, u32, u32, u32) =
            if speed.0 <= 100_000 {
                (4_700, 4_000, 1_000, 300, 250, 3_450)
            } else if speed.0 <= 400_000 {
                (1_300, 600, 300, 300, 100, 900)
            } else {
                (500, 260, 120, 120, 50, 450)
            };
        // The period asked for, split around the mode's minimum low,
        // with the rest high but never under the mode's minimum.
        let period = 1_000_000_000u32.div_ceil(speed.0);
        let mut low = low_min;
        if period - low < high_min {
            low = period - high_min;
        }
        let high = period - low;
        if low < low_min {
            return Err(ConfigError::Speed);
        }
        // The smallest prescaler that fits every field (RM0433 §47.4.5).
        let mut presc = 0;
        while presc < 16 {
            let tp = tick * (presc + 1);
            // The guard keeps the divisions below from carrying a
            // division-by-zero edge into the link: `tick` is at least one
            // (`div_ceil` of a smaller number), so this never fires.
            if tp == 0 {
                presc += 1;
                continue;
            }
            let scll = low.div_ceil(tp) - 1;
            let sclh = high.div_ceil(tp) - 1;
            let scldel = (rise + setup).div_ceil(tp) - 1;
            if scll > 255 || sclh > 255 || scldel > 15 {
                presc += 1;
                continue;
            }
            // SDADEL's bounds (RM0433 §47.4.5): at least the fall over
            // the analog filter's shortest delay and three kernel
            // ticks, at most the data-hold maximum under the filter's
            // longest delay and four ticks. The lower bound rounds up
            // and the upper down, so the window only narrows. The
            // analog filter's delays are DS12117 Table 94's: 50 ns at
            // the least, 260 at the most.
            let low_num = fall as i32 - 50 - 3 * tick as i32;
            let high_num = hold_max as i32 - 260 - 4 * tick as i32;
            if high_num < 0 {
                presc += 1;
                continue;
            }
            let lower = if low_num <= 0 {
                0
            } else {
                (low_num as u32).div_ceil(tp)
            };
            let upper = high_num as u32 / tp;
            if lower > 15 || lower > upper {
                presc += 1;
                continue;
            }
            let sdadel = lower;
            // All fields fit: the period from the same counts, at or
            // below the speed asked for. The guard keeps the division's
            // edge out of the link: the period is two counts at the
            // least, so it never fires.
            let achieved = tp * (scll + 1) + tp * (sclh + 1);
            if achieved == 0 {
                return Err(ConfigError::Speed);
            }
            return Ok(Timing {
                timingr: (presc << 28) | (scldel << 20) | (sdadel << 16) | (sclh << 8) | scll,
                speed: Hertz(1_000_000_000 / achieved),
            });
        }
        Err(ConfigError::Speed)
    }

    /// Writes the timing with PE clear, the analog filter on and the
    /// digital filter off (RM0433 §47.4.6's flow, Figure 542); `arm`
    /// sets PE.
    pub(crate) fn apply(self, timing: &Timing) {
        let regs = self.0;
        regs.cr1().write(|w| {
            w.set_pe(false);
            w.set_anfoff(false);
            w.set_dnf(stm32_metapac::i2c::vals::Dnf::NO_FILTER);
            w.set_nostretch(false);
        });
        regs.timingr().write(|w| {
            w.set_presc((timing.timingr >> 28) as u8);
            w.set_scldel(((timing.timingr >> 20) & 0xF) as u8);
            w.set_sdadel(((timing.timingr >> 16) & 0xF) as u8);
            w.set_sclh(((timing.timingr >> 8) & 0xFF) as u8);
            w.set_scll((timing.timingr & 0xFF) as u8);
        });
    }

    /// Closes the I2C: PE off. Only a teardown (`I2c::free`) calls this —
    /// the driver keeps PE on for its life. A reopen runs `new` again,
    /// whose `apply` writes with PE clear first, as RM0433 §47.4.6's flow
    /// requires.
    pub(crate) fn disable(self) {
        self.0.cr1().modify(|w| w.set_pe(false));
    }

    /// ISR's BUSY (RM0433 §47.6.9).
    pub(crate) fn busy(self) -> bool {
        self.0.isr().read().busy()
    }

    /// PE on, then CR2's program: the 7-bit address in SADD[7:1], the
    /// direction, NBYTES as the first chunk of `len` (RELOAD high when
    /// more follow), AUTOEND on the last segment so its stop is the
    /// hardware's, and START — a repeated start when the bus is running
    /// (RM0433 §47.4.9, Table 389, §47.6.5).
    pub(crate) fn arm(self, addr: u8, read: bool, len: usize, last: bool) {
        let regs = self.0;
        regs.cr1().modify(|w| w.set_pe(true));
        regs.cr2().write(|w| {
            w.set_sadd(u16::from(addr) << 1);
            w.set_dir(match read {
                true => stm32_metapac::i2c::vals::Dir::READ,
                false => stm32_metapac::i2c::vals::Dir::WRITE,
            });
            w.set_add10(stm32_metapac::i2c::vals::Addmode::BIT7);
            w.set_nbytes(len.min(255) as u8);
            w.set_reload(match len > 255 {
                true => stm32_metapac::i2c::vals::Reload::NOT_COMPLETED,
                false => stm32_metapac::i2c::vals::Reload::COMPLETED,
            });
            w.set_autoend(match last {
                true => stm32_metapac::i2c::vals::Autoend::AUTOMATIC,
                false => stm32_metapac::i2c::vals::Autoend::SOFTWARE,
            });
            w.set_start(true);
        });
    }

    /// The address phase is the hardware's; the first wait of the
    /// segment sees it through: TXIS or RXNE once it is acknowledged,
    /// NACKF if it is not.
    pub(crate) fn addr_step(self, _addr: u8, _read: bool, _len: usize, _last: bool) -> bool {
        true
    }

    /// TXIS, with a chunk's NBYTES loaded at TCR when the segment is
    /// longer than one chunk (RM0433 §47.4.11: the last chunk clears
    /// RELOAD).
    pub(crate) fn tx_ready(self, position: usize, len: usize) -> bool {
        let regs = self.0;
        if regs.isr().read().tcr() {
            let left = (len - position) as u32;
            regs.cr2().modify(|w| {
                w.set_nbytes(left.min(255) as u8);
                w.set_reload(match left > 255 {
                    true => stm32_metapac::i2c::vals::Reload::NOT_COMPLETED,
                    false => stm32_metapac::i2c::vals::Reload::COMPLETED,
                });
            });
        }
        regs.isr().read().txis()
    }

    /// One byte into TXDR (RM0433 §47.6.12).
    pub(crate) fn send(self, byte: u8) {
        self.0.txdr().write(|w| w.set_txdata(byte));
    }

    /// RXNE, with the chunk reload of `tx_ready`.
    pub(crate) fn recv_ready(self, position: usize, len: usize, _last: bool) -> bool {
        let regs = self.0;
        if regs.isr().read().tcr() {
            let left = (len - position) as u32;
            regs.cr2().modify(|w| {
                w.set_nbytes(left.min(255) as u8);
                w.set_reload(match left > 255 {
                    true => stm32_metapac::i2c::vals::Reload::NOT_COMPLETED,
                    false => stm32_metapac::i2c::vals::Reload::COMPLETED,
                });
            });
        }
        regs.isr().read().rxne()
    }

    /// One byte out of RXDR (RM0433 §47.6.11).
    pub(crate) fn recv(self, _position: usize, _len: usize, _last: bool) -> u8 {
        self.0.rxdr().read().rxdata()
    }

    /// Between segments: TC, which the next START clears (RM0433
    /// §47.4.9). The last segment needs no software end — AUTOEND sends
    /// its stop.
    pub(crate) fn op_done(self, _read: bool, last: bool) -> bool {
        if last {
            return true;
        }
        self.0.isr().read().tc()
    }

    /// Nothing to arm: AUTOEND carries the last segment's stop, and the
    /// next segment's START is the repeated start.
    pub(crate) fn end_op(self, _read: bool, _last: bool) {}

    /// STOPF, which AUTOEND sets once the stop is out (RM0433 §47.6.9).
    pub(crate) fn stop_done(self) -> bool {
        self.0.isr().read().stopf()
    }

    /// STOPF clears through ICR (RM0433 §47.6.10).
    pub(crate) fn stop_clear(self) {
        self.0.icr().write(|w| w.set_stopcf(true));
    }

    /// NACKF, BERR, ARLO and OVR, each cleared through ICR as it is
    /// taken (RM0433 §47.6.9, §47.6.10).
    pub(crate) fn error(self) -> Option<Error> {
        let regs = self.0;
        let isr = regs.isr().read();
        if isr.nackf() {
            regs.icr().write(|w| w.set_nackcf(true));
            return Some(Error::Nack);
        }
        if isr.berr() {
            regs.icr().write(|w| w.set_berrcf(true));
            return Some(Error::Bus);
        }
        if isr.arlo() {
            regs.icr().write(|w| w.set_arlocf(true));
            return Some(Error::ArbitrationLost);
        }
        if isr.ovr() {
            regs.icr().write(|w| w.set_ovrcf(true));
            return Some(Error::Overrun);
        }
        None
    }

    /// Puts the bus back: PE off resets the state machines and releases
    /// the lines while TIMINGR keeps its value, the flags clear through
    /// ICR, and PE returns (RM0433 §47.4.6).
    pub(crate) fn recover(self) {
        let regs = self.0;
        regs.cr1().modify(|w| w.set_pe(false));
        regs.icr().write(|w| {
            w.set_nackcf(true);
            w.set_stopcf(true);
            w.set_berrcf(true);
            w.set_arlocf(true);
            w.set_ovrcf(true);
            w.set_timoutcf(true);
        });
        regs.cr1().modify(|w| w.set_pe(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The H7 bench's PCLK1: 120 MHz, which the Nucleo-H753ZI's tree runs
    /// on APB1. Every mode produces its timing, at or below the speed
    /// asked for, with every field of TIMINGR in range.
    #[test]
    fn every_mode_from_the_bench_pclk1() {
        for &(asked, bound) in &[
            (100_000u32, 100_000u32),
            (400_000, 400_000),
            (1_000_000, 1_000_000),
        ] {
            let timing = Unit::timing(Hertz::mhz(120), Hertz(asked))
                .unwrap_or_else(|_| panic!("{asked} Hz runs from 120 MHz"));
            assert!(
                timing.speed().0 <= bound,
                "{asked} Hz: {}",
                timing.speed().0
            );
            let presc = timing.timingr >> 28;
            let scldel = (timing.timingr >> 20) & 0xF;
            let sdadel = (timing.timingr >> 16) & 0xF;
            let sclh = (timing.timingr >> 8) & 0xFF;
            let scll = timing.timingr & 0xFF;
            assert!(presc <= 15);
            assert!(scldel <= 15);
            assert!(sdadel <= 15);
            assert!(sclh <= 255 && scll <= 255);
            assert!(scll > 0 && sclh > 0, "{asked} Hz: SCLL or SCLH zero");
        }
    }

    /// The counts the recipe promises: the low and high times over the
    /// mode's minimums, the setup over its bound, and the period at or
    /// above the asked one, at 120 MHz standard mode.
    #[test]
    fn the_counts_meet_the_specification() {
        let timing =
            Unit::timing(Hertz::mhz(120), Hertz::khz(100)).expect("120 MHz to 100 kHz runs");
        let tick = 1_000_000_000u32.div_ceil(120_000_000);
        let tp = tick * ((timing.timingr >> 28) + 1);
        let low = tp * ((timing.timingr & 0xFF) + 1);
        let high = tp * (((timing.timingr >> 8) & 0xFF) + 1);
        let setup = tp * (((timing.timingr >> 20) & 0xF) + 1);
        assert!(low >= 4_700, "tLOW {low}");
        assert!(high >= 4_000, "tHIGH {high}");
        assert!(setup >= 1_250, "tSCLDEL {setup}");
        assert!(low + high >= 10_000, "period {}", low + high);
    }

    #[test]
    fn out_of_range_is_refused() {
        assert_eq!(
            Unit::timing(Hertz::mhz(120), Hertz(1_000_001)),
            Err(ConfigError::Speed)
        );
        assert_eq!(
            Unit::timing(Hertz::mhz(120), Hertz(900)),
            Err(ConfigError::Speed)
        );
        assert_eq!(
            Unit::timing(Hertz(0), Hertz::khz(100)),
            Err(ConfigError::KernelClock)
        );
    }
}
