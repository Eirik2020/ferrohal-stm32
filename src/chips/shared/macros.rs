//! The macros every chip's glue builds from. They carry shapes only; the
//! facts (which peripherals, pins, streams and vectors) are the rows each
//! chip file passes them, which `tools/check_glue.py` reads.
//!
//! Names in the expansions (`Pin`, `Alternate`, `Interrupt`) resolve in the
//! chip file that invokes them, so each chip brings its own IP versions.

/// Zero-sized tokens for peripherals with no driver yet.
macro_rules! tokens {
    ($($name:ident),+ $(,)?) => {
        $(
            /// A peripheral token with no driver yet. Zero-sized.
            #[allow(non_camel_case_types)]
            pub struct $name {
                _private: (),
            }

            impl $name {
                const fn new() -> Self {
                    Self { _private: () }
                }

                /// A second token for this peripheral.
                ///
                /// # Safety
                ///
                /// Nothing else may use the peripheral while this token is
                /// in use.
                pub unsafe fn steal() -> Self {
                    Self::new()
                }
            }

            impl crate::Sealed for $name {}
        )+
    };
}

/// The chip's `Peripherals`, one field per token. `$doc` says which
/// peripherals are left out, and why.
macro_rules! peripherals {
    ($doc:literal; $($name:ident: $ty:ty),+ $(,)?) => {
        static TAKEN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

        /// Every peripheral of the chip, once. svd2rust's model: take this
        /// once, then move each token to the driver or task that owns it.
        ///
        #[doc = $doc]
        #[allow(non_snake_case)]
        pub struct Peripherals {
            $(pub $name: $ty,)+
        }

        impl Peripherals {
            /// The tokens, the first time. `None` after that.
            pub fn take() -> Option<Self> {
                if TAKEN.swap(true, core::sync::atomic::Ordering::AcqRel) {
                    None
                } else {
                    // SAFETY: the flag says this is the first time.
                    Some(unsafe { Self::steal() })
                }
            }

            /// The tokens, unconditionally. RTIC calls this when its app
            /// sets `peripherals = true`.
            ///
            /// # Safety
            ///
            /// No token from an earlier `take` or `steal` may still be in
            /// use.
            pub unsafe fn steal() -> Self {
                TAKEN.store(true, core::sync::atomic::Ordering::Release);
                Self {
                    $($name: <$ty>::new(),)+
                }
            }
        }
    };
}

/// Pin routing rows: (port, pin, alternate function).
macro_rules! pins {
    ($trait:ident<$usart:ident>: $(($port:literal, $pin:literal, $af:literal)),+ $(,)?) => {
        $(impl $trait<$usart> for Pin<$port, $pin, Alternate<$af>> {})+
    };
}

/// The vector each DMA stream raises.
macro_rules! stream_interrupts {
    ($($dma:literal: [$($stream:literal => $vector:ident),+];)+) => {
        $($(
            impl crate::dma::StreamInterrupt for crate::dma::Stream<$dma, $stream> {
                const INTERRUPT: Interrupt = Interrupt::$vector;
                const NAME: &'static str = stringify!($vector);
            }
        )+)+
    };
}

/// Whether all of `buffer` lies in one of `regions`, each (base, length).
pub(crate) fn within(buffer: &[u8], regions: &[(usize, usize)]) -> bool {
    let start = buffer.as_ptr() as usize;
    let end = start.saturating_add(buffer.len());
    regions
        .iter()
        .any(|&(base, len)| start >= base && end <= base.saturating_add(len))
}
