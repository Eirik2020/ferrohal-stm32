//! GPIO v2 (`gpio_v2`): every pin a typestate, every port's shared
//! configuration registers behind one owner.
//!
//! A port token from `Peripherals` splits into sixteen pins and a [`Ctrl`].
//! `Ctrl` owns the registers all sixteen pins share (MODER, OTYPER,
//! OSPEEDR, PUPDR, AFRL, AFRH), so changing a pin's mode takes `&mut Ctrl`
//! and two pins can never race on a read-modify-write. Driving an output
//! uses BSRR, a single write that touches only that pin, so it needs no
//! owner.
//!
//! Which pins a package bonds out is glued data: each chip's `BONDED` table,
//! which `tools/check_glue.py` holds every routing row against. A port the
//! package does not bond has no token. A bonded port still splits into all
//! sixteen pins, so an unbonded one can be split out but not routed; see
//! `DEBT.md`.
//!
//! Requirements: GPIO-001

use core::{convert::Infallible, marker::PhantomData};

use stm32_metapac::gpio::{
    Gpio,
    vals::{Idr, Moder, Odr, Ospeedr, Ot, Pupdr},
};

use crate::rcc::{Rcc, RccBit};

/// A GPIO port token from `Peripherals`, `P` being its letter.
pub struct Port<const P: char> {
    _private: (),
}

impl<const P: char> Port<P> {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }
}

/// One port's register block. Crate-internal: a public bound naming it would
/// hand metapac out.
pub(crate) trait PortRegisters {
    fn regs() -> Gpio;
}

/// A GPIO port this chip has. Chip facts: the glue layer implements it.
#[allow(private_bounds)] // `PortRegisters` is the crate-internal half.
pub trait PortInstance: crate::Sealed + PortRegisters {
    #[doc(hidden)]
    const ENABLE: RccBit;
}

/// The owner of a port's shared configuration registers.
pub struct Ctrl<const P: char> {
    _private: (),
}

macro_rules! parts {
    ($($field:ident: $n:literal),+ $(,)?) => {
        /// A split port: its sixteen pins, in their reset mode, and the
        /// owner of their shared registers.
        pub struct Parts<const P: char> {
            pub ctrl: Ctrl<P>,
            $(pub $field: Pin<P, $n>,)+
        }

        impl<const P: char> Port<P>
        where
            Port<P>: PortInstance,
        {
            /// Turns on the port's clock and hands out its pins.
            pub fn split(self, rcc: &mut Rcc) -> Parts<P> {
                rcc.enable(<Port<P> as PortInstance>::ENABLE);
                Parts {
                    ctrl: Ctrl { _private: () },
                    $($field: Pin::new(),)+
                }
            }
        }
    };
}

parts!(
    p0: 0, p1: 1, p2: 2, p3: 3, p4: 4, p5: 5, p6: 6, p7: 7,
    p8: 8, p9: 9, p10: 10, p11: 11, p12: 12, p13: 13, p14: 14, p15: 15,
);

/// A pin in an alternate function, described as data: for board manifests
/// and resource lists. An analog pin implements it too, with `AF` 0 — it has
/// no alternate function.
pub trait AltPin {
    /// The port's letter.
    const PORT: char;
    const PIN: u8;
    const AF: u8;
}

impl<const P: char, const N: u8, const AF: u8> AltPin for Pin<P, N, Alternate<AF>> {
    const PORT: char = P;
    const PIN: u8 = N;
    const AF: u8 = AF;
}

impl<const P: char, const N: u8> AltPin for Pin<P, N, Analog> {
    const PORT: char = P;
    const PIN: u8 = N;
    const AF: u8 = 0;
}

/// Reset mode for most pins.
pub struct Input;
/// Push-pull output.
pub struct Output;
/// Driven by a peripheral, through alternate function `AF`.
pub struct Alternate<const AF: u8>;
pub struct Analog;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Pull {
    None,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Speed {
    Low,
    Medium,
    High,
    VeryHigh,
}

/// Pin `N` of port `P`, in mode `M`. Zero-sized.
pub struct Pin<const P: char, const N: u8, M = Input> {
    _mode: PhantomData<M>,
}

impl<const P: char, const N: u8, M> Pin<P, N, M> {
    const fn new() -> Self {
        Self { _mode: PhantomData }
    }
}

impl<const P: char, const N: u8, M> crate::Sealed for Pin<P, N, M> {}

impl<const P: char, const N: u8, M> Pin<P, N, M>
where
    Port<P>: PortInstance,
{
    fn regs() -> Gpio {
        <Port<P> as PortRegisters>::regs()
    }

    fn set_mode(mode: Moder) {
        Self::regs()
            .moder()
            .modify(|w| w.set_moder(usize::from(N), mode));
    }

    /// Hands the pin to a peripheral. The function is selected before the
    /// mode changes, so the pin never drives another function's signal.
    pub fn into_alternate<const AF: u8>(self, _ctrl: &mut Ctrl<P>) -> Pin<P, N, Alternate<AF>> {
        const { assert!(AF < 16, "alternate functions are 0 to 15") };
        let regs = Self::regs();
        regs.otyper()
            .modify(|w| w.set_ot(usize::from(N), Ot::PUSH_PULL));
        regs.afr(usize::from(N / 8))
            .modify(|w| w.set_afr(usize::from(N % 8), AF));
        Self::set_mode(Moder::ALTERNATE);
        Pin::new()
    }

    /// As [`into_alternate`](Self::into_alternate), with the output open
    /// drain: the peripheral pulls the line low and lets it float high, for
    /// shared lines other devices hold or pull — I2C's SCL and SDA.
    pub fn into_alternate_open_drain<const AF: u8>(
        self,
        _ctrl: &mut Ctrl<P>,
    ) -> Pin<P, N, Alternate<AF>> {
        const { assert!(AF < 16, "alternate functions are 0 to 15") };
        let regs = Self::regs();
        regs.otyper()
            .modify(|w| w.set_ot(usize::from(N), Ot::OPEN_DRAIN));
        regs.afr(usize::from(N / 8))
            .modify(|w| w.set_afr(usize::from(N % 8), AF));
        Self::set_mode(Moder::ALTERNATE);
        Pin::new()
    }

    /// A push-pull output, driven low first so it never glitches high.
    pub fn into_push_pull_output(self, _ctrl: &mut Ctrl<P>) -> Pin<P, N, Output> {
        let regs = Self::regs();
        regs.bsrr().write(|w| w.set_br(usize::from(N), true));
        regs.otyper()
            .modify(|w| w.set_ot(usize::from(N), Ot::PUSH_PULL));
        Self::set_mode(Moder::OUTPUT);
        Pin::new()
    }

    pub fn into_input(self, _ctrl: &mut Ctrl<P>) -> Pin<P, N, Input> {
        Self::set_mode(Moder::INPUT);
        Pin::new()
    }

    pub fn into_analog(self, _ctrl: &mut Ctrl<P>) -> Pin<P, N, Analog> {
        Self::set_mode(Moder::ANALOG);
        Pin::new()
    }

    pub fn set_pull(&mut self, _ctrl: &mut Ctrl<P>, pull: Pull) {
        let value = match pull {
            Pull::None => Pupdr::FLOATING,
            Pull::Up => Pupdr::PULL_UP,
            Pull::Down => Pupdr::PULL_DOWN,
        };
        Self::regs()
            .pupdr()
            .modify(|w| w.set_pupdr(usize::from(N), value));
    }

    pub fn set_speed(&mut self, _ctrl: &mut Ctrl<P>, speed: Speed) {
        let value = match speed {
            Speed::Low => Ospeedr::LOW_SPEED,
            Speed::Medium => Ospeedr::MEDIUM_SPEED,
            Speed::High => Ospeedr::HIGH_SPEED,
            Speed::VeryHigh => Ospeedr::VERY_HIGH_SPEED,
        };
        Self::regs()
            .ospeedr()
            .modify(|w| w.set_ospeedr(usize::from(N), value));
    }

    fn input_high() -> bool {
        Self::regs().idr().read().idr(usize::from(N)) == Idr::HIGH
    }
}

impl<const P: char, const N: u8> Pin<P, N, Output>
where
    Port<P>: PortInstance,
{
    pub fn set_high(&mut self) {
        Self::regs()
            .bsrr()
            .write(|w| w.set_bs(usize::from(N), true));
    }

    pub fn set_low(&mut self) {
        Self::regs()
            .bsrr()
            .write(|w| w.set_br(usize::from(N), true));
    }

    pub fn is_set_high(&self) -> bool {
        Self::regs().odr().read().odr(usize::from(N)) == Odr::HIGH
    }
}

impl<const P: char, const N: u8, M> embedded_hal::digital::ErrorType for Pin<P, N, M> {
    type Error = Infallible;
}

impl<const P: char, const N: u8> embedded_hal::digital::OutputPin for Pin<P, N, Output>
where
    Port<P>: PortInstance,
{
    fn set_high(&mut self) -> Result<(), Infallible> {
        Pin::set_high(self);
        Ok(())
    }

    fn set_low(&mut self) -> Result<(), Infallible> {
        Pin::set_low(self);
        Ok(())
    }
}

impl<const P: char, const N: u8> embedded_hal::digital::StatefulOutputPin for Pin<P, N, Output>
where
    Port<P>: PortInstance,
{
    fn is_set_high(&mut self) -> Result<bool, Infallible> {
        Ok(Pin::is_set_high(self))
    }

    fn is_set_low(&mut self) -> Result<bool, Infallible> {
        Ok(!Pin::is_set_high(self))
    }
}

impl<const P: char, const N: u8> embedded_hal::digital::InputPin for Pin<P, N, Input>
where
    Port<P>: PortInstance,
{
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(Self::input_high())
    }

    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(!Self::input_high())
    }
}
