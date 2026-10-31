//! SPI v4 (`spi_v4`, metapac's `spi_v4_i2s`: the CFG1/CFG2/CR1/CR2/SR/IFCR
//! register set of H7 and later): the register operations the
//! [`crate::spi`] driver needs. The driver itself, and the routing traits
//! the glue implements, are re-exported here.
//!
//! Each transfer enables the SPI and disables it again (RM0433 50.4.14),
//! with the transfer size in TSIZE, so EOT ends it. Disabling after each
//! one also keeps clear of ES0392 2.22.2 (a CSTART within one SCK of EOT
//! can stall the master). AFCNTR keeps the pins driven while the SPI is
//! disabled, so SCK holds its idle level between transfers. CRCSIZE stays
//! at its reset value, 7 (ES0392 2.22.5). The driver holds one SCK period
//! between EOT and disabling (ES0392 2.22.6). FIFO and DMA requests are
//! one byte each.

use stm32_metapac::spi::vals::{Comm, Cpha, Cpol, Fthlv, Lsbfirst, Master, Mbr, Sp};

pub use crate::spi::stream_dma::{
    Info, Instance, MisoPin, MosiPin, SckPin, Spi, SpiRxDma, SpiTxDma,
};
use crate::spi::{BitOrder, Config, Phase, Polarity};

/// The register block.
pub type Regs = stm32_metapac::spi::Spi;

/// One SPI's registers, as the driver uses them.
#[derive(Clone, Copy)]
pub struct Unit(Regs);

impl Unit {
    /// Hold one SCK period between EOT and disabling (ES0392 2.22.6).
    pub(crate) const HOLD_AFTER_END: bool = true;

    pub(crate) const fn new(regs: Regs) -> Self {
        Self(regs)
    }

    /// Master, full duplex, 8-bit frames, software NSS held high (no mode
    /// fault from the NSS pin), pins kept driven while disabled. Left
    /// disabled: each transfer enables it. CFG1 and CFG2 are written only
    /// with SPE clear, as RM0433 requires.
    pub(crate) fn configure(self, code: u8, config: &Config) {
        let regs = self.0;
        regs.cr1().modify(|w| w.set_spe(false));
        // SSI first: with SSM set, NSS reads SSI, and a master that sees NSS
        // low takes a mode fault, which clears MASTER at once.
        regs.cr1().write(|w| w.set_ssi(true));
        regs.ier().write(|_| {});
        regs.cfg1().write(|w| {
            w.set_mbr(Mbr::from_bits(code & 7));
            w.set_dsize(7);
            w.set_fthlv(Fthlv::ONE_FRAME);
            w.set_crcsize(7);
        });
        regs.cfg2().write(|w| {
            w.set_master(Master::MASTER);
            w.set_comm(Comm::FULL_DUPLEX);
            w.set_sp(Sp::MOTOROLA);
            w.set_cpol(match config.mode.polarity {
                Polarity::IdleLow => Cpol::IDLE_LOW,
                Polarity::IdleHigh => Cpol::IDLE_HIGH,
            });
            w.set_cpha(match config.mode.phase {
                Phase::CaptureOnFirstTransition => Cpha::FIRST_EDGE,
                Phase::CaptureOnSecondTransition => Cpha::SECOND_EDGE,
            });
            w.set_lsbfirst(match config.bit_order {
                BitOrder::MsbFirst => Lsbfirst::MSBFIRST,
                BitOrder::LsbFirst => Lsbfirst::LSBFIRST,
            });
            w.set_ssm(true);
            w.set_afcntr(true);
        });
        self.clear_flags();
    }

    pub(crate) fn tx_address(self) -> u32 {
        self.0.txdr8().as_ptr() as u32
    }

    pub(crate) fn rx_address(self) -> u32 {
        self.0.rxdr8().as_ptr() as u32
    }

    /// The transfer size, and receive requests on, before the streams start
    /// (RM0433 50.4.14 step 1). The SPI is disabled here.
    pub(crate) fn enable_rx_dma(self, len: u16) {
        let regs = self.0;
        regs.cr2().write(|w| w.set_tsize(len));
        regs.cfg1().modify(|w| w.set_rxdmaen(true));
    }

    /// Transmit requests on after the streams (step 3), then enable (step 4)
    /// and start.
    pub(crate) fn go(self) {
        let regs = self.0;
        regs.cfg1().modify(|w| w.set_txdmaen(true));
        regs.cr1().modify(|w| w.set_spe(true));
        regs.cr1().modify(|w| w.set_cstart(true));
    }

    /// TSIZE frames have been sent and received.
    pub(crate) fn done(self) -> bool {
        self.0.sr().read().eot()
    }

    /// Overrun and mode fault; `end` clears them.
    pub(crate) fn take_errors(self) -> (bool, bool) {
        let sr = self.0.sr().read();
        (sr.ovr(), sr.modf())
    }

    /// Closes the transfer (RM0433 50.4.14): flags cleared, the SPI
    /// disabled, then the DMA requests off.
    pub(crate) fn end(self) {
        let regs = self.0;
        self.clear_flags();
        regs.cr1().modify(|w| w.set_spe(false));
        regs.cfg1().modify(|w| {
            w.set_txdmaen(false);
            w.set_rxdmaen(false);
        });
    }

    /// After a stopped or failed transfer: disabling resets the SPI's state
    /// and flushes its FIFOs. A mode fault drops MASTER; put it back.
    pub(crate) fn reset_state(self) {
        let regs = self.0;
        let modf = regs.sr().read().modf();
        self.end();
        if modf {
            regs.cr1().write(|w| w.set_ssi(true));
            regs.cfg2().modify(|w| w.set_master(Master::MASTER));
        }
    }

    fn clear_flags(self) {
        self.0.ifcr().write(|w| {
            w.set_eotc(true);
            w.set_txtfc(true);
            w.set_udrc(true);
            w.set_ovrc(true);
            w.set_crcec(true);
            w.set_tifrec(true);
            w.set_modfc(true);
            w.set_tserfc(true);
            w.set_suspc(true);
        });
    }
}
