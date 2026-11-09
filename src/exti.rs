//! External interrupts on GPIO lines, exposed as they are: `exti_v1` (F4)
//! and `exti_h7` (H7, CPU1's mask and pending registers). Both have the
//! same per-line registers (rising and falling trigger, software trigger,
//! mask, pending), so they share this source; SYSCFG's EXTICR picks which
//! port drives each line.
//!
//! The EXTI token splits, with SYSCFG, into sixteen line tokens and a
//! [`Ctrl`]. `Ctrl` owns the registers lines share by read-modify-write
//! (EXTICR, the trigger selections, the mask), the way `gpio::Ctrl` owns a
//! port's. A line routes to the pin with its number on one port. It only
//! watches that pin's level, so it doesn't take the pin: the pin stays with
//! whoever configured it, as an input with its pull, or as a peripheral's
//! pin (an SPI's SCK can count its own edges). The line token is what's
//! exclusive. Clearing a line's pending flag and triggering it in software
//! are single writes that touch only that line, so the [`ExtiHandle`] an
//! interrupt handler holds needs no owner.
//!
//! FerroHAL enables the line's interrupt in EXTI only; the scheduler binds
//! the vector ([`Exti::interrupt`]) and clears the line in its handler.
//!
//! Requirements: EXTI-001, EXTI-002, EXTI-003, EXTI-004

use core::{convert::Infallible, marker::PhantomData};

use stm32_metapac::{EXTI as EXTI_REGS, SYSCFG as SYSCFG_REGS, exti::regs::Lines};

use crate::{
    chip::{EXTI, Interrupt, SYSCFG, SYSCFG_ENABLE},
    gpio::{Port, PortInstance, PortRegisters},
    rcc::Rcc,
};

/// Which edges set a line pending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Edge {
    Rising,
    Falling,
    Both,
}

/// The owner of the registers the sixteen lines share.
pub struct Ctrl {
    _private: (),
}

/// Line `N`, not yet bound to a pin.
pub struct Line<const N: u8> {
    _private: (),
}

macro_rules! parts {
    ($($field:ident: $n:literal),+ $(,)?) => {
        /// The split EXTI: sixteen GPIO lines and the owner of their shared
        /// registers.
        pub struct Parts {
            pub ctrl: Ctrl,
            $(pub $field: Line<$n>,)+
        }

        impl EXTI {
            /// Turns on SYSCFG's clock (for EXTICR) and hands out the GPIO
            /// lines, each masked with no edge selected.
            pub fn split(self, _syscfg: SYSCFG, rcc: &mut Rcc) -> Parts {
                rcc.enable(SYSCFG_ENABLE);
                EXTI_REGS.imr(0).modify(|w| w.0 &= !0xffff);
                EXTI_REGS.rtsr(0).modify(|w| w.0 &= !0xffff);
                EXTI_REGS.ftsr(0).modify(|w| w.0 &= !0xffff);
                EXTI_REGS.pr(0).write_value(Lines(0xffff));
                Parts {
                    ctrl: Ctrl { _private: () },
                    $($field: Line { _private: () },)+
                }
            }
        }
    };
}

parts!(
    l0: 0, l1: 1, l2: 2, l3: 3, l4: 4, l5: 5, l6: 6, l7: 7,
    l8: 8, l9: 9, l10: 10, l11: 11, l12: 12, l13: 13, l14: 14, l15: 15,
);

/// The vector line `n` raises: its own for 0 to 4, shared for 5 to 9 and
/// for 10 to 15. The same on every chip here.
pub const fn vector(n: u8) -> Interrupt {
    match n {
        0 => Interrupt::EXTI0,
        1 => Interrupt::EXTI1,
        2 => Interrupt::EXTI2,
        3 => Interrupt::EXTI3,
        4 => Interrupt::EXTI4,
        5..=9 => Interrupt::EXTI9_5,
        _ => Interrupt::EXTI15_10,
    }
}

fn lines(n: u8) -> Lines {
    Lines(1 << (n & 15))
}

fn set_edge(n: u8, edge: Edge) {
    let n = usize::from(n & 15);
    EXTI_REGS
        .rtsr(0)
        .modify(|w| w.set_line(n, matches!(edge, Edge::Rising | Edge::Both)));
    EXTI_REGS
        .ftsr(0)
        .modify(|w| w.set_line(n, matches!(edge, Edge::Falling | Edge::Both)));
}

fn set_mask(n: u8, listen: bool) {
    EXTI_REGS
        .imr(0)
        .modify(|w| w.set_line(usize::from(n & 15), listen));
}

impl<const N: u8> Line<N> {
    /// Routes port `P`'s pin `N` to this line, sets its edges, clears any
    /// stale pending flag and unmasks it. Configure the pin yourself (an
    /// input with a pull, or a peripheral's pin): the line only watches it.
    pub fn route<const P: char>(self, _ctrl: &mut Ctrl, edge: Edge) -> Exti<P, N>
    where
        Port<P>: PortInstance,
    {
        const { assert!(N < 16, "GPIO lines are 0 to 15") };
        // Ports are numbered from A = 0 in EXTICR, on F4 and H7 alike.
        let port = (P as u32).wrapping_sub('A' as u32) as u8;
        SYSCFG_REGS
            .exticr(usize::from((N / 4) & 3))
            .modify(|w| w.set_exti(usize::from(N % 4), port));
        set_edge(N, edge);
        ExtiHandle { line: N }.clear();
        set_mask(N, true);
        Exti { _line: PhantomData }
    }
}

/// EXTI line `N`, routed to port `P`'s pin `N`.
pub struct Exti<const P: char, const N: u8> {
    _line: PhantomData<Line<N>>,
}

impl<const P: char, const N: u8> Exti<P, N>
where
    Port<P>: PortInstance,
{
    /// The vector to bind; shared with other lines for 5 and up.
    pub const fn interrupt(&self) -> Interrupt {
        vector(N)
    }

    /// What an interrupt handler needs: pending check and clear.
    pub const fn handle(&self) -> ExtiHandle {
        ExtiHandle { line: N }
    }

    pub fn is_pending(&self) -> bool {
        self.handle().is_pending()
    }

    pub fn clear(&self) {
        self.handle().clear();
    }

    /// Sets the line pending from software, as an edge would.
    pub fn trigger(&self) {
        self.handle().trigger();
    }

    /// The pin's level now, read from its port's input register, whatever
    /// mode it is in.
    pub fn is_high(&self) -> bool {
        <Port<P> as PortRegisters>::regs()
            .idr()
            .read()
            .idr(usize::from(N & 15))
            == stm32_metapac::gpio::vals::Idr::HIGH
    }

    /// Unmasks the line's interrupt.
    pub fn listen(&mut self, _ctrl: &mut Ctrl) {
        set_mask(N, true);
    }

    /// Masks the line. A masked line ignores its edges altogether: they
    /// neither raise the interrupt nor set it pending (RM0368 10.2.4,
    /// RM0433 20.3.1, confirmed on the F401).
    pub fn unlisten(&mut self, _ctrl: &mut Ctrl) {
        set_mask(N, false);
    }

    pub fn set_edge(&mut self, _ctrl: &mut Ctrl, edge: Edge) {
        set_edge(N, edge);
    }

    /// Masks the line, deselects its edges and gives the line back.
    pub fn release(self, _ctrl: &mut Ctrl) -> Line<N> {
        set_mask(N, false);
        EXTI_REGS
            .rtsr(0)
            .modify(|w| w.set_line(usize::from(N & 15), false));
        EXTI_REGS
            .ftsr(0)
            .modify(|w| w.set_line(usize::from(N & 15), false));
        self.clear();
        Line { _private: () }
    }
}

/// A line's pending flag, for an interrupt handler: reading it, clearing it
/// and triggering it are single accesses to that line's bit, so this is
/// `Copy` and needs no owner. Handles for several lines can share one
/// vector's handler (lines 5 to 9, and 10 to 15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ExtiHandle {
    line: u8,
}

impl ExtiHandle {
    pub const fn line(self) -> u8 {
        self.line
    }

    pub fn is_pending(self) -> bool {
        EXTI_REGS.pr(0).read().line(usize::from(self.line & 15))
    }

    /// Write-one-to-clear: only this line's bit is written.
    pub fn clear(self) {
        EXTI_REGS.pr(0).write_value(lines(self.line));
    }

    /// Writing 1 to SWIER sets the line pending; 0 bits have no effect.
    pub fn trigger(self) {
        EXTI_REGS.swier(0).write_value(lines(self.line));
    }

    pub const fn interrupt(self) -> Interrupt {
        vector(self.line)
    }
}

impl<const P: char, const N: u8> embedded_hal::digital::ErrorType for Exti<P, N> {
    type Error = Infallible;
}

impl<const P: char, const N: u8> embedded_hal::digital::InputPin for Exti<P, N>
where
    Port<P>: PortInstance,
{
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(Exti::is_high(self))
    }

    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(!Exti::is_high(self))
    }
}
