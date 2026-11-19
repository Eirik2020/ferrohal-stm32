//! `i2c_v2`'s half of the interrupt-driven master (`crate::i2c::interrupt`,
//! ADR-013). A phase is one CR2 program (RM0433 47.4.9, RM0410 33.4.9):
//! NBYTES is the whole phase, at most 255 with reload never set (ES0392
//! 2.19.5), and AUTOEND is set on the last phase, so the hardware sends the
//! stop and STOPF ends the transaction. A write phase with a read after it
//! ends at TC, and the read's START is the repeated start. The bytes move by
//! DMA (TXDMAEN, RXDMAEN) or from TXIS and RXNE (TXIE, RXIE). The end is
//! always the I2C's (TC or STOPF), never a stream's, so the streams need no
//! vector of their own.
//!
//! Requirements: I2C-V2-002, I2C-V2-003

use super::Unit;
use crate::i2c::{
    Error,
    interrupt::{Cursor, Step},
};
use stm32_metapac::i2c::vals;

impl Unit {
    /// The shortest write phase DMA moves: from two bytes a stream costs
    /// fewer CPU cycles than TXIS does (ADR-013's F767 bench: 3,901 against
    /// 4,041 cycles).
    pub(crate) const DMA_FROM_WRITE: usize = 2;
    /// The shortest read phase DMA moves. NBYTES counts any length, so DMA
    /// could end a one-byte read, but from two bytes a stream costs fewer
    /// cycles than RXNE does (ADR-013's F767 bench: 3,884 against 4,124).
    pub(crate) const DMA_FROM_READ: usize = 2;
    /// The end of a read is the I2C's (TC or STOPF), not the stream's.
    pub(crate) const READ_ENDS_ON_STREAM: bool = false;

    pub(crate) fn irq_enable(self) {
        self.0.cr1().modify(|w| w.set_pe(true));
    }

    pub(crate) fn irq_data_address(self, read: bool) -> u32 {
        if read {
            self.0.rxdr().as_ptr() as u32
        } else {
            self.0.txdr().as_ptr() as u32
        }
    }

    /// The last stop is off the wire: ISR.BUSY low.
    pub(crate) fn irq_idle(self) -> bool {
        !self.0.isr().read().busy()
    }

    /// What a refused or aborted transaction may have left: a byte in RXDR
    /// from an aborted read, and the NACKF, STOPF, BERR, ARLO and OVR flags.
    pub(crate) fn irq_clear(self) {
        if self.0.isr().read().rxne() {
            let _ = self.0.rxdr().read();
        }
        self.0.icr().write(|w| {
            w.set_nackcf(true);
            w.set_stopcf(true);
            w.set_berrcf(true);
            w.set_arlocf(true);
            w.set_ovrcf(true);
        });
    }

    /// Interrupts and DMA requests for the phase, then its CR2 program with
    /// START (a repeated start when the write phase left the bus held at TC).
    pub(crate) fn irq_begin(self, c: &Cursor<'_>, _restart: bool) {
        let regs = self.0;
        regs.cr1().modify(|w| {
            w.set_pe(true);
            w.set_txie(!c.dma && !c.read);
            w.set_rxie(!c.dma && c.read);
            w.set_tcie(true);
            w.set_stopie(true);
            w.set_nackie(true);
            w.set_errie(true);
            w.set_txdmaen(c.dma && !c.read);
            w.set_rxdmaen(c.dma && c.read);
        });
        regs.cr2().write(|w| {
            w.set_sadd(u16::from(c.address) << 1);
            w.set_dir(if c.read {
                vals::Dir::READ
            } else {
                vals::Dir::WRITE
            });
            w.set_add10(vals::Addmode::BIT7);
            // `check` refuses a phase over MAX_PHASE (255), so this fits.
            w.set_nbytes(c.len.min(255) as u8);
            w.set_reload(vals::Reload::COMPLETED);
            w.set_autoend(if c.last {
                vals::Autoend::AUTOMATIC
            } else {
                vals::Autoend::SOFTWARE
            });
            w.set_start(true);
        });
    }

    /// Interrupts and DMA requests off.
    pub(crate) fn irq_quiet(self) {
        self.0.cr1().modify(|w| {
            w.set_txie(false);
            w.set_rxie(false);
            w.set_tcie(false);
            w.set_stopie(false);
            w.set_nackie(false);
            w.set_errie(false);
            w.set_txdmaen(false);
            w.set_rxdmaen(false);
        });
    }

    /// An aborted transaction: interrupts and DMA off, then CR2.STOP, which
    /// the hardware sends after the byte in flight once a stretching slave
    /// lets SCL go (RM0433 47.7.2, CR2.STOP). PE stays set. Cycling it left
    /// the slave mid-read with nothing on the wire to end it, and the next
    /// transfer was NACKed on the F767 and H753; with the STOP both ran at
    /// once (ADR-013). A stop that never gets out is the next `start`'s:
    /// it recovers and refuses.
    pub(crate) fn irq_abort(self) {
        self.irq_quiet();
        self.0.cr2().modify(|w| w.set_stop(true));
    }

    /// The stop after a write phase with no read: AUTOEND sent it already,
    /// so the driver never reaches this on `i2c_v2`; it sets CR2.STOP for
    /// the shape the driver calls.
    pub(crate) fn irq_stop(self) {
        self.0.cr2().modify(|w| w.set_stop(true));
    }

    /// A refused or failed transaction. After a NACK the hardware sends the
    /// stop itself (RM0433 47.4.9); ARLO and OVR were recovered in
    /// `irq_service`. Interrupts and DMA off, flags cleared.
    pub(crate) fn irq_fail(self) {
        self.irq_quiet();
        self.irq_clear();
    }

    /// Not reached: a read ends at the I2C, not at its stream.
    pub(crate) fn irq_read_dma_done(self) {}

    /// One service of the event and error flags.
    pub(crate) fn irq_service(self, c: &mut Cursor<'_>, bus_errors: &mut u32) -> Step {
        let regs = self.0;
        let isr = regs.isr().read();
        if isr.berr() {
            // ES0392 2.19.4: spurious in controller mode; counted, carried on.
            regs.icr().write(|w| w.set_berrcf(true));
            *bus_errors += 1;
        }
        if isr.nackf() {
            regs.icr().write(|w| w.set_nackcf(true));
            return Step::Fail(Error::Nack);
        }
        if isr.arlo() {
            regs.icr().write(|w| w.set_arlocf(true));
            self.recover();
            return Step::Fail(Error::ArbitrationLost);
        }
        if isr.ovr() {
            regs.icr().write(|w| w.set_ovrcf(true));
            self.recover();
            return Step::Fail(Error::Overrun);
        }
        if !c.dma {
            if !c.read && isr.txis() && c.pos < c.len {
                let byte = c.buf.get(usize::from(c.pos)).copied().unwrap_or(0);
                regs.txdr().write(|w| w.set_txdata(byte));
                c.pos += 1;
            }
            if c.read && isr.rxne() && c.pos < c.len {
                let byte = regs.rxdr().read().rxdata();
                if let Some(b) = c.buf.get_mut(usize::from(c.pos)) {
                    *b = byte;
                }
                c.pos += 1;
            }
        } else {
            c.pos = c.len;
        }
        if isr.tc() && !c.last {
            return Step::PhaseDone;
        }
        if isr.stopf() {
            regs.icr().write(|w| w.set_stopcf(true));
            return Step::Finished;
        }
        Step::Pending
    }
}
