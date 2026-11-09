//! USART v4 (`usart_v4`, the ISR/ICR/RDR/TDR register set of G4, H5, H7
//! and others): the register operations the [`crate::uart`] driver needs.
//! The driver itself, and the routing traits the glue implements, are
//! re-exported here.
//!
//! Unlike v2, errors clear through ICR, a write-one-to-clear register, so
//! clearing one never reads the data register. The receive and transmit
//! FIFOs stay off: DMA serves one byte per request.
//!
//! Requirements: USART-001, USART-002, USART-003, USART-004, USARTV4-001

use stm32_metapac::usart::{
    regs::Icr,
    vals::{M0, M1, Ps, Stop},
};

pub use crate::uart::stream_dma::{
    Info, Instance, Line, RxDma, RxPin, TxDma, TxPin, Uart, UartIrq, UartRx, UartTx,
};
use crate::uart::{
    Config, Parity, StopBits,
    stream_dma::{ERR_FRAMING, ERR_NOISE, ERR_OVERRUN, ERR_PARITY},
};

/// The register block of a USART or UART (not LPUART, whose BRR differs).
pub type Regs = stm32_metapac::usart::Usart;

/// RX and TX inversion: CR2's RXINV and TXINV.
pub(crate) const INVERSION: bool = true;

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
        self.0.rdr().as_ptr() as u32
    }

    pub(crate) fn tx_address(self) -> u32 {
        self.0.tdr().as_ptr() as u32
    }

    /// Writes the frame settings with the transmitter and receiver off, the
    /// USART on and both DMA requests enabled. Most of CR1 to CR3 can only
    /// be written while UE is clear, so the USART goes off first.
    pub(crate) fn write_frame(self, config: &Config, brr: u16) {
        let regs = self.0;
        regs.cr1().write(|w| w.set_ue(false));
        regs.cr2().write(|w| {
            w.set_stop(match config.stop_bits {
                StopBits::Half => Stop::STOP0P5,
                StopBits::One => Stop::STOP1,
                StopBits::OneAndHalf => Stop::STOP1P5,
                StopBits::Two => Stop::STOP2,
            });
            w.set_rxinv(config.invert_rx);
            w.set_txinv(config.invert_tx);
        });
        // EIE raises the USART interrupt on overrun, framing and noise.
        // DDRE stays clear, so a byte with an error still goes to DMA and
        // reception carries on.
        regs.cr3().write(|w| {
            w.set_dmat(true);
            w.set_dmar(true);
            w.set_eie(true);
        });
        regs.presc().write(|w| w.0 = 0);
        regs.brr().write(|w| w.set_brr(brr));
        regs.cr1().write(|w| {
            w.set_m1(M1::M0);
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
            w.set_ue(true);
        });
        // Stale errors from before the reset or the change.
        regs.icr().write_value(error_flags());
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

    pub(crate) fn clear_tc(self) {
        self.0.icr().write(|w| w.set_tc(true));
    }

    pub(crate) fn tc(self) -> bool {
        self.0.isr().read().tc()
    }

    /// SBKRQ in RQR, a write-only request register: no other bit changes.
    pub(crate) fn request_break(self) {
        self.0.rqr().write(|w| w.set_sbkrq(true));
    }

    /// SBKF stays set until the break's stop bit.
    pub(crate) fn break_pending(self) -> bool {
        self.0.isr().read().sbkf()
    }

    /// Reads overrun, framing, parity and noise, and whether the line went
    /// idle, and clears the ones set.
    pub(crate) fn take_events(self) -> (u8, bool) {
        let isr = self.0.isr().read();
        let idle = isr.idle();
        let mut bits = 0;
        let mut clear = Icr(0);
        for (set, bit) in [
            (isr.ore(), ERR_OVERRUN),
            (isr.fe(), ERR_FRAMING),
            (isr.pe(), ERR_PARITY),
            (isr.ne(), ERR_NOISE),
        ] {
            if set {
                bits |= bit;
            }
        }
        if bits != 0 || idle {
            clear.set_ore(isr.ore());
            clear.set_fe(isr.fe());
            clear.set_pe(isr.pe());
            clear.set_ne(isr.ne());
            clear.set_idle(idle);
            self.0.icr().write_value(clear);
        }
        (bits, idle)
    }
}

/// ICR bits for every error `take_events` reports.
fn error_flags() -> Icr {
    let mut icr = Icr(0);
    icr.set_ore(true);
    icr.set_fe(true);
    icr.set_pe(true);
    icr.set_ne(true);
    icr
}
