//! USART v2 (`usart_v2`, the SR/DR register set of F1, F2 and F4): the
//! register operations the [`crate::uart`] driver needs. The driver itself,
//! and the routing traits the glue implements, are re-exported here.
//!
//! Requirements: USART-001, USART-002, USART-003, USART-004, USART-005, USARTV2-001

use stm32_metapac::usart::{
    regs::Sr,
    vals::{M0, Ps, Stop},
};

pub use crate::uart::stream_dma::{
    Info, Instance, Line, RxDma, RxPin, TxDma, TxPin, Uart, UartIrq, UartRx, UartTx,
};
use crate::uart::{
    Config, Parity, StopBits,
    stream_dma::{ERR_FRAMING, ERR_NOISE, ERR_OVERRUN, ERR_PARITY},
};

/// The register block. Every F4 U(S)ART uses the USART layout; a UART-only
/// instance (UART4, UART5) is the same registers minus the synchronous ones.
pub type Regs = stm32_metapac::usart::Usart;

/// RX and TX inversion: v2 has no such bits.
pub(crate) const INVERSION: bool = false;

/// One USART's register block, as the driver uses it.
#[derive(Clone, Copy)]
pub struct LineRegs(Regs);

impl LineRegs {
    pub(crate) const fn new(regs: Regs) -> Self {
        Self(regs)
    }

    pub(crate) fn regs(self) -> Regs {
        self.0
    }

    pub(crate) fn rx_address(self) -> u32 {
        self.0.dr().as_ptr() as u32
    }

    pub(crate) fn tx_address(self) -> u32 {
        self.0.dr().as_ptr() as u32
    }

    /// Writes the frame settings with the transmitter and receiver off, the
    /// USART on and both DMA requests enabled.
    pub(crate) fn write_frame(self, config: &Config, brr: u16) {
        let regs = self.0;
        regs.cr1().write(|w| w.set_ue(true));
        regs.cr2().write(|w| {
            w.set_stop(match config.stop_bits {
                StopBits::Half => Stop::STOP0P5,
                StopBits::One => Stop::STOP1,
                StopBits::OneAndHalf => Stop::STOP1P5,
                StopBits::Two => Stop::STOP2,
            })
        });
        regs.brr().write(|w| w.set_brr(brr));
        // With DMAR set, EIE raises the USART interrupt on overrun, framing
        // and noise; without it an overrun would stop reception silently.
        regs.cr3().write(|w| {
            w.set_dmat(true);
            w.set_dmar(true);
            w.set_eie(true);
        });
        regs.cr1().write(|w| {
            w.set_ue(true);
            match config.parity {
                Parity::None => w.set_m0(M0::BIT8),
                Parity::Even | Parity::Odd => {
                    // The parity bit is the ninth.
                    w.set_m0(M0::BIT9);
                    w.set_pce(true);
                    w.set_ps(if config.parity == Parity::Odd {
                        Ps::ODD
                    } else {
                        Ps::EVEN
                    });
                    w.set_peie(true);
                }
            }
        });
        // Stale errors from before the reset or the change.
        let _ = regs.sr().read();
        let _ = regs.dr().read();
    }

    /// Turns the receiver on, with the idle-line interrupt that ends a
    /// chunk, and the transmitter if the UART has one.
    pub(crate) fn enable(self, tx: bool) {
        self.0.cr1().modify(|w| {
            w.set_te(tx);
            w.set_re(true);
            w.set_idleie(true);
        });
    }

    pub(crate) fn disable(self) {
        self.0.cr1().modify(|w| w.set_ue(false));
    }

    /// Clears or sets the line's event interrupt enables: idle and parity in
    /// CR1, errors in CR3. `RXNEIE` is left alone: the receive path is DMA's.
    pub(crate) fn set_event_enables(self, on: bool) {
        let parity = self.0.cr1().read().pce();
        self.0.cr1().modify(|w| {
            w.set_idleie(on);
            // PEIE only matters with parity; without it the bit stays as the
            // frame settings left it.
            w.set_peie(on && parity);
        });
        self.0.cr3().modify(|w| w.set_eie(on));
    }

    /// Clears TC before a transfer, as RM0368 asks. RXNE and LBD are written
    /// 1, which leaves them alone: a read-modify-write could clear an RXNE
    /// that DMA has not served yet. CTS is written 0: it is only ever set
    /// with CTSE on, which FerroHAL never sets, and on the F4's UART4 and
    /// UART5 the bit is reserved, to be kept at its reset value of 0
    /// (RM0090 section 30.6.1).
    pub(crate) fn clear_tc(self) {
        let mut sr = Sr(0);
        sr.set_rxne(true);
        sr.set_lbd(true);
        self.0.sr().write_value(sr);
    }

    pub(crate) fn tc(self) -> bool {
        self.0.sr().read().tc()
    }

    /// SBK in CR1. After `Uart::split`, the transmit half is the only
    /// writer of CR1.
    pub(crate) fn request_break(self) {
        self.0.cr1().modify(|w| w.set_sbk(true));
    }

    /// The hardware clears SBK at the break's stop bit.
    pub(crate) fn break_pending(self) -> bool {
        self.0.cr1().read().sbk()
    }

    /// Reads and clears overrun, framing, parity and noise, and whether the
    /// line went idle.
    pub(crate) fn take_events(self) -> (u8, bool) {
        let sr = self.0.sr().read();
        let idle = sr.idle();
        if !(idle || sr.pe() || sr.fe() || sr.ne() || sr.ore()) {
            return (0, false);
        }
        // Reading DR after SR clears all five. DMA has taken every good
        // byte, so DR holds the faulty one, or nothing new at an idle line.
        let _ = self.0.dr().read();
        let mut bits = 0;
        for (set, bit) in [
            (sr.ore(), ERR_OVERRUN),
            (sr.fe(), ERR_FRAMING),
            (sr.pe(), ERR_PARITY),
            (sr.ne(), ERR_NOISE),
        ] {
            if set {
                bits |= bit;
            }
        }
        (bits, idle)
    }
}
