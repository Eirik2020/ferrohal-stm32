//! SPI v2 (`spi_v2`, metapac's `spi_v2_i2s`: the CR1/CR2/SR/DR register set
//! of F2, F4 and F7): the register operations the [`crate::spi`] driver
//! needs. The driver itself, and the routing traits the glue implements,
//! are re-exported here.
//!
//! The SPI stays enabled (SPE) from `configure` on, holding SCK at its idle
//! level between transfers; a transfer only turns the DMA requests on and
//! off. That also keeps clear of ES0299 2.11.1 / ES0182 2.12.1 (BSY can
//! stay high if the SPI is disabled mid-frame): SPE only drops in
//! `configure`, with the bus idle.

use stm32_metapac::spi::vals::{Br, Cpha, Cpol, Dff, Lsbfirst, Mstr};

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
    /// v2 needs no hold between the last frame and the end of a transfer.
    pub(crate) const HOLD_AFTER_END: bool = false;

    pub(crate) const fn new(regs: Regs) -> Self {
        Self(regs)
    }

    /// Master, 8-bit frames, software NSS held high (so no mode fault from
    /// the NSS pin, which the firmware may use as a GPIO), then enabled.
    pub(crate) fn configure(self, code: u8, config: &Config) {
        let regs = self.0;
        regs.cr1().modify(|w| w.set_spe(false));
        regs.cr2().write(|_| {});
        regs.cr1().write(|w| {
            w.set_mstr(Mstr::MASTER);
            w.set_br(Br::from_bits(code & 7));
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
            w.set_dff(Dff::BITS8);
            w.set_ssm(true);
            w.set_ssi(true);
        });
        regs.cr1().modify(|w| w.set_spe(true));
    }

    pub(crate) fn tx_address(self) -> u32 {
        self.0.dr().as_ptr() as u32
    }

    pub(crate) fn rx_address(self) -> u32 {
        self.0.dr().as_ptr() as u32
    }

    /// Receive requests on, before the streams start (RM0368 20.3.9).
    /// v2 has no transfer size: the streams' counts end the transfer.
    pub(crate) fn enable_rx_dma(self, _len: u16) {
        self.0.cr2().modify(|w| w.set_rxdmaen(true));
    }

    /// Transmit requests on, after the streams: the first byte goes out.
    pub(crate) fn go(self) {
        self.0.cr2().modify(|w| w.set_txdmaen(true));
    }

    /// The last frame is out: TXE set and BSY clear (RM0368 20.3.9).
    pub(crate) fn done(self) -> bool {
        let sr = self.0.sr().read();
        sr.txe() && !sr.bsy()
    }

    /// Overrun and mode fault, cleared: OVR by reading DR then SR, MODF by
    /// reading SR then writing CR1 (which `configure` does, so a mode fault
    /// is answered with a reset).
    pub(crate) fn take_errors(self) -> (bool, bool) {
        let regs = self.0;
        let sr = regs.sr().read();
        if sr.ovr() {
            let _ = regs.dr().read();
            let _ = regs.sr().read();
        }
        (sr.ovr(), sr.modf())
    }

    /// Requests off; the SPI stays enabled and idle.
    pub(crate) fn end(self) {
        self.0.cr2().modify(|w| {
            w.set_txdmaen(false);
            w.set_rxdmaen(false);
        });
    }

    /// After a stopped or failed transfer: requests off, then up to `spins`
    /// register reads for the frames already in the SPI (the one shifting,
    /// the one in its buffer) to finish, since the SPI stays enabled; then
    /// the byte they left in DR read out, so the next transfer doesn't
    /// receive it first, and any overrun cleared. A mode fault drops MSTR
    /// and SPE; put them back.
    pub(crate) fn reset_state(self, spins: u32) {
        let regs = self.0;
        self.end();
        for _ in 0..spins {
            if self.done() {
                break;
            }
        }
        let _ = regs.dr().read();
        let sr = regs.sr().read();
        if sr.modf() {
            regs.cr1().modify(|w| {
                w.set_mstr(Mstr::MASTER);
                w.set_spe(true);
            });
        }
    }
}
