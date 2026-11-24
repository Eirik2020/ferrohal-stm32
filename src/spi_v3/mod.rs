//! SPI v3 (`spi_v3`, metapac's `spi_v3_i2s`: the F7's SPI, RM0431 28.5 and
//! RM0385 32.5): the register operations the [`crate::spi`] driver needs.
//! The driver itself, and the routing traits the glue implements, are
//! re-exported here.
//!
//! v3 is v2 with a 32-bit FIFO each way. Three things change, and the rest is
//! v2's (`spi_v2`), down to the SPI staying enabled (SPE) from `configure`
//! on, holding SCK at its idle level between transfers:
//!
//! - **Frames.** The data size is CR2's DS, set to 8 bits, and the receive
//!   threshold FRXTH to a quarter of the FIFO, so RXNE, and the receive DMA
//!   request with it, rises for every byte (RM0431 28.5.9, "Data packing").
//!   At the reset threshold, 16 bits, the last byte of an odd-length
//!   transfer would wait in the FIFO for a partner that never comes.
//! - **Access.** DR is read and written a byte at a time (`dr8`), by the CPU
//!   and by the streams, whose peripheral size is a byte: a 16-bit access
//!   would pack two frames, and LDMA_TX/LDMA_RX stay clear.
//! - **The end.** TXE rises while the transmit FIFO is still half full, so a
//!   transfer is over only when FTLVL is empty and BSY clear, and what a
//!   stopped transfer left in the receive FIFO is read out until FRLVL is
//!   empty (RM0431 28.5.9, "Procedure for disabling the SPI").
//!
//! ES0360 2.11.1 and ES0290 2.11.2 (BSY can stay high after a transfer)
//! concern slave mode only; FerroHAL is master only.
//!
//! Requirements: SPI-V3-001, SPI-V3-002, SPI-POLL-001, SPI-POLL-002, SPI-010

use stm32_metapac::spi::vals::{Br, Cpha, Cpol, Ds, Frlvl, Frxth, Ftlvl, Lsbfirst, Mstr};

use crate::spi::{BitOrder, Config, Phase, Polarity};

/// The register block.
pub type Regs = stm32_metapac::spi::Spi;

/// The receive FIFO's depth in 8-bit frames (32 bits, RM0431 28.5.9).
const RX_FIFO_FRAMES: u32 = 4;

/// One SPI's registers, as the driver uses them.
#[derive(Clone, Copy)]
pub struct Unit(Regs);

impl Unit {
    /// v3 needs no hold between the last frame and the end of a transfer.
    pub(crate) const HOLD_AFTER_END: bool = false;

    pub(crate) const fn new(regs: Regs) -> Self {
        Self(regs)
    }

    /// Master, 8-bit frames with an 8-bit receive threshold, software NSS
    /// held high (so no mode fault from the NSS pin, which the firmware may
    /// use as a GPIO), then enabled.
    pub(crate) fn configure(self, code: u8, config: &Config) {
        let regs = self.0;
        regs.cr1().modify(|w| w.set_spe(false));
        // DS has no all-zero value (0000 to 0010 are reserved), so CR2 is
        // written whole.
        regs.cr2().write(|w| {
            w.set_ds(Ds::BITS8);
            w.set_frxth(Frxth::QUARTER);
        });
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
            w.set_ssm(true);
            w.set_ssi(true);
        });
        regs.cr1().modify(|w| w.set_spe(true));
    }

    pub(crate) fn tx_address(self) -> u32 {
        self.0.dr8().as_ptr() as u32
    }

    pub(crate) fn rx_address(self) -> u32 {
        self.0.dr8().as_ptr() as u32
    }

    /// Receive requests on, before the streams start (RM0431 28.5.9).
    /// v3 has no transfer size: the streams' counts end the transfer.
    pub(crate) fn enable_rx_dma(self, _len: u16) {
        self.0.cr2().modify(|w| w.set_rxdmaen(true));
    }

    /// Transmit requests on, after the streams: the first byte goes out.
    pub(crate) fn go(self) {
        self.0.cr2().modify(|w| w.set_txdmaen(true));
    }

    /// The last frame is out: the transmit FIFO empty and BSY clear (RM0431
    /// 28.5.9). TXE alone only says the FIFO is half empty.
    pub(crate) fn done(self) -> bool {
        let sr = self.0.sr().read();
        sr.ftlvl() == Ftlvl::EMPTY && !sr.bsy()
    }

    /// Overrun and mode fault, cleared: OVR by reading DR then SR, MODF by
    /// reading SR then writing CR1 (which `configure` does, so a mode fault
    /// is answered with a reset).
    pub(crate) fn take_errors(self) -> (bool, bool) {
        let regs = self.0;
        let sr = regs.sr().read();
        if sr.ovr() {
            let _ = regs.dr8().read();
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

    /// A polled transfer of `len` bytes begins: nothing to do, the SPI is
    /// already enabled.
    pub(crate) fn start_polled(self, _len: u16) {}

    pub(crate) fn tx_ready(self) -> bool {
        self.0.sr().read().txe()
    }

    /// One frame out: a byte write of DR, which an 8-bit frame needs (a
    /// half-word write would send two).
    pub(crate) fn write_byte(self, byte: u8) {
        self.0.dr8().write_value(byte);
    }

    pub(crate) fn rx_ready(self) -> bool {
        self.0.sr().read().rxne()
    }

    pub(crate) fn read_byte(self) -> u8 {
        self.0.dr8().read()
    }

    /// A polled transfer ends: nothing to undo.
    pub(crate) fn end_polled(self) {}

    /// Closes the SPI: SPE off. Only a teardown (`Spi::free`) calls this;
    /// between transfers the driver keeps SPE on (see the module header). A
    /// reopen starts from `configure`, which writes CR1 with SPE clear
    /// first, after `Spi::open`'s RCC reset has emptied the FIFOs.
    pub(crate) fn disable(self) {
        self.0.cr1().modify(|w| w.set_spe(false));
    }

    /// After a stopped or failed transfer: requests off, then up to `spins`
    /// register reads for the frames already in the SPI (the transmit FIFO,
    /// the one shifting) to finish, since the SPI stays enabled; then the
    /// receive FIFO read until FRLVL is empty, so the next transfer doesn't
    /// receive its leftovers first, and any overrun cleared. A mode fault
    /// drops MSTR and SPE; put them back.
    pub(crate) fn reset_state(self, spins: u32) {
        let regs = self.0;
        self.end();
        let _ = crate::wait::spin(spins, || self.done());
        // At most the FIFO's depth: a bounded drain, whatever the flags say.
        for _ in 0..RX_FIFO_FRAMES {
            if regs.sr().read().frlvl() == Frlvl::EMPTY {
                break;
            }
            let _ = regs.dr8().read();
        }
        let sr = regs.sr().read();
        if sr.modf() {
            regs.cr1().modify(|w| {
                w.set_mstr(Mstr::MASTER);
                w.set_spe(true);
            });
        }
    }
}
