//! `i2c_v1`'s half of the interrupt-driven master (`crate::i2c::interrupt`,
//! ADR-013). The events are those of the blocking walk in
//! `super` (RM0368 18.3.3, 18.3.8), served from ITEVTEN / ITERREN; the
//! bytes move by DMA (DMAEN, with LAST on a receive) or from TxE / RxNE
//! (ITBUFEN). ITBUFEN stays off while DMA moves the bytes (RM0368 18.3.8's
//! two notes).
//!
//! Requirements: I2C-V1-002, I2C-V1-003

use super::Unit;
use crate::i2c::{
    Error,
    interrupt::{Cursor, Step},
};
use stm32_metapac::i2c::vals::Pos;

impl Unit {
    /// The shortest write phase DMA moves: from two bytes a stream costs
    /// fewer CPU cycles than the event interrupt (ADR-013's F401 bench).
    pub(crate) const DMA_FROM_WRITE: usize = 2;
    /// The shortest read phase DMA moves. RM0368 18.3.8: a one-byte receive's
    /// NACK is programmed at EV6, so DMA cannot end it; a two-byte receive
    /// by interrupt waits once at BTF and costs fewer cycles than a stream
    /// (ADR-013's F401 bench).
    pub(crate) const DMA_FROM_READ: usize = 3;
    /// RM0368 18.3.8: a DMA receive ends at the stream's transfer complete,
    /// where the stop is programmed.
    pub(crate) const READ_ENDS_ON_STREAM: bool = true;

    pub(crate) fn irq_enable(self) {
        self.0.cr1().modify(|w| w.set_pe(true));
    }

    pub(crate) fn irq_data_address(self, _read: bool) -> u32 {
        self.0.dr().as_ptr() as u32
    }

    /// The last stop is off the wire: CR1.STOP cleared by the hardware and
    /// SR2.BUSY low.
    pub(crate) fn irq_idle(self) -> bool {
        !self.0.cr1().read().stop() && !self.0.sr2().read().busy()
    }

    /// Flags a refused transaction may have left: AF, ARLO, OVR, BERR.
    pub(crate) fn irq_clear(self) {
        self.0.sr1().modify(|w| {
            w.set_af(false);
            w.set_arlo(false);
            w.set_ovr(false);
            w.set_berr(false);
        });
    }

    /// Interrupts and DMA requests for the phase, then START (a repeated
    /// start when the bus is held).
    pub(crate) fn irq_begin(self, c: &Cursor<'_>, _restart: bool) {
        let regs = self.0;
        regs.cr2().modify(|w| {
            w.set_itevten(true);
            w.set_iterren(true);
            // A two-byte receive by interrupt waits at BTF: no RxNE.
            w.set_itbufen(!c.dma && !(c.read && c.len == 2));
            w.set_dmaen(c.dma);
            w.set_last(c.dma && c.read);
        });
        regs.cr1().modify(|w| {
            w.set_pe(true);
            w.set_pos(Pos::CURRENT);
            w.set_ack(c.read && (c.dma || c.len > 2));
            w.set_start(true);
        });
    }

    /// Interrupts and DMA requests off.
    pub(crate) fn irq_quiet(self) {
        let regs = self.0;
        regs.cr2().modify(|w| {
            w.set_itevten(false);
            w.set_iterren(false);
            w.set_itbufen(false);
            w.set_dmaen(false);
            w.set_last(false);
        });
        // CR1 is left alone: a read-modify-write could land just as the
        // hardware clears a pending STOP and set it again. `irq_begin` writes
        // ACK and POS for the next phase.
    }

    /// An aborted transaction: the blocking driver's recovery (a STOP, then
    /// SWRST once the bus is free).
    pub(crate) fn irq_abort(self) {
        self.recover();
    }

    pub(crate) fn irq_stop(self) {
        self.0.cr1().modify(|w| w.set_stop(true));
    }

    /// A refused or failed transaction: the stop, the flags cleared.
    pub(crate) fn irq_fail(self) {
        self.irq_quiet();
        self.irq_stop();
        self.irq_clear();
    }

    /// The receive stream has moved the last byte (the NACK was the
    /// hardware's, LAST): the stop.
    pub(crate) fn irq_read_dma_done(self) {
        self.irq_stop();
    }

    /// One service of the event and error flags.
    pub(crate) fn irq_service(self, c: &mut Cursor<'_>, bus_errors: &mut u32) -> Step {
        let regs = self.0;
        let sr1 = regs.sr1().read();
        if sr1.berr() {
            // ES0299 2.9.1: spurious in controller mode; counted, carried on.
            regs.sr1().modify(|w| w.set_berr(false));
            *bus_errors += 1;
        }
        if sr1.af() {
            regs.sr1().modify(|w| w.set_af(false));
            return Step::Fail(Error::Nack);
        }
        if sr1.arlo() {
            regs.sr1().modify(|w| w.set_arlo(false));
            return Step::Fail(Error::ArbitrationLost);
        }
        if sr1.ovr() {
            regs.sr1().modify(|w| w.set_ovr(false));
            return Step::Fail(Error::Overrun);
        }
        if sr1.start() {
            // EV5: SR1 read above, then the address byte.
            regs.dr()
                .write(|w| w.set_dr(c.address << 1 | u8::from(c.read)));
            return Step::Pending;
        }
        if sr1.addr() {
            // EV6, as the blocking walk does it, for the interrupt-fed
            // receives of one and two bytes.
            if c.read && !c.dma && c.len == 1 {
                regs.cr1().modify(|w| w.set_ack(false));
            }
            if c.read && !c.dma && c.len == 2 {
                regs.cr1().modify(|w| {
                    w.set_ack(false);
                    w.set_pos(Pos::NEXT);
                });
            }
            let _ = regs.sr2().read();
            if c.read && !c.dma && c.len == 1 && c.last {
                regs.cr1().modify(|w| w.set_stop(true));
            }
            c.addressed = true;
            return Step::Pending;
        }
        if !c.addressed {
            // A BTF left from the write phase until the repeated start is
            // out: nothing to do yet.
            return Step::Pending;
        }
        if !c.read {
            if !c.dma && sr1.txe() && c.pos < c.len {
                let byte = c.buf.get(usize::from(c.pos)).copied().unwrap_or(0);
                regs.dr().write(|w| w.set_dr(byte));
                c.pos += 1;
                if c.pos == c.len {
                    // The last byte is in DR: wait for BTF, not TxE.
                    regs.cr2().modify(|w| w.set_itbufen(false));
                }
                return Step::Pending;
            }
            if sr1.btf() && (c.dma || c.pos == c.len) {
                if c.dma {
                    // RM0368 18.3.8: DMA requests off, then BTF, then the stop.
                    regs.cr2().modify(|w| w.set_dmaen(false));
                    c.pos = c.len;
                }
                return Step::PhaseDone;
            }
            return Step::Pending;
        }
        if c.dma {
            // The receive stream's complete ends it.
            return Step::Pending;
        }
        if c.len == 2 {
            if sr1.btf() {
                regs.cr1().modify(|w| w.set_stop(true));
                for slot in 0..2 {
                    let byte = regs.dr().read().dr();
                    if let Some(b) = c.buf.get_mut(slot) {
                        *b = byte;
                    }
                }
                c.pos = 2;
                return Step::Finished;
            }
            return Step::Pending;
        }
        if sr1.rxne() && c.pos < c.len {
            if c.len > 2 && c.pos + 2 == c.len {
                regs.cr1().modify(|w| w.set_ack(false));
            }
            let byte = regs.dr().read().dr();
            if let Some(b) = c.buf.get_mut(usize::from(c.pos)) {
                *b = byte;
            }
            c.pos += 1;
            if c.pos == c.len {
                if c.len > 1 {
                    regs.cr1().modify(|w| w.set_stop(true));
                }
                return Step::Finished;
            }
        }
        Step::Pending
    }
}
