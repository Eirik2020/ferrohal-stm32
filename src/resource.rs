//! What a driver's wiring uses, as data, the check that its interrupt
//! vectors are bound as it needs, and what a constructor that refuses hands
//! back. Shared by every peripheral driver.
//!
//! Requirements: RES-001

/// One resource a wiring uses, as data: for checking a board's manifest
/// against the types that wire it (`Wiring::RESOURCES` and the like).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Resource {
    /// The peripheral instance, by its reference-manual name.
    Peripheral(&'static str),
    Pin {
        signal: Signal,
        port: char,
        pin: u8,
        af: u8,
    },
    Dma {
        signal: Signal,
        controller: u8,
        stream: u8,
        /// The request: `dma_v2`'s channel, or `dma_v1`'s DMAMUX ID.
        request: u8,
    },
    /// An interrupt vector the driver needs bound, by its name.
    Interrupt(&'static str),
}

/// Which of a peripheral's signals a pin or DMA stream carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Signal {
    Tx,
    Rx,
    Sck,
    Miso,
    Mosi,
    Scl,
    Sda,
    /// A timer channel's output, by its number (1 to 4).
    Channel(u8),
    /// A timer channel's complementary output (CHxN), by its number.
    Complementary(u8),
}

/// What a vector check found wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum IrqError {
    /// This vector is masked in the NVIC: nothing serves it.
    NotEnabled(crate::chip::Interrupt),
    /// These two vectors have different priorities, so one could preempt
    /// the other in the middle of the driver's interrupt work.
    PriorityMismatch(crate::chip::Interrupt, crate::chip::Interrupt),
}

/// A constructor's refusal: why, and everything it took by value, handed
/// back so that the caller can retry with another configuration or use the
/// parts elsewhere. The parts are what the constructor moved in, in argument
/// order: a tuple when it took more than one, such as `Uart::new`'s
/// `(Wiring, Buffers)`.
///
/// The parts come back as they went in, but the hardware behind them may
/// not have stayed untouched: the constructor's own documentation says what
/// it may already have done (such as enabling and resetting the
/// peripheral) by the time it refuses.
///
/// Printing one, with `Debug` or `defmt::Format`, prints the reason alone:
/// the parts are tokens and buffers, with nothing to show.
#[must_use = "a refusal holds the tokens and buffers the constructor took"]
pub struct Refused<E, P> {
    error: E,
    parts: P,
}

impl<E, P> Refused<E, P> {
    pub(crate) const fn new(error: E, parts: P) -> Self {
        Self { error, parts }
    }

    /// Everything the constructor took by value, to retry or use elsewhere.
    pub fn into_parts(self) -> P {
        self.parts
    }

    /// The reason and the parts together.
    pub fn into_inner(self) -> (E, P) {
        (self.error, self.parts)
    }
}

impl<E: Copy, P> Refused<E, P> {
    /// Why the constructor refused.
    pub fn error(&self) -> E {
        self.error
    }
}

impl<E: core::fmt::Debug, P> core::fmt::Debug for Refused<E, P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.error.fmt(f)
    }
}

#[cfg(feature = "defmt")]
impl<E: defmt::Format, P> defmt::Format for Refused<E, P> {
    fn format(&self, f: defmt::Formatter<'_>) {
        self.error.format(f)
    }
}

/// Checks the NVIC, reading only: every vector enabled, all at the first
/// one's priority. FerroHAL never sets NVIC state; this tells the firmware
/// whether its scheduler set it as a driver needs.
pub(crate) fn verify_vectors(
    mut vectors: impl Iterator<Item = crate::chip::Interrupt>,
) -> Result<(), IrqError> {
    use cortex_m::peripheral::NVIC;
    let Some(first) = vectors.next() else {
        return Ok(());
    };
    if !NVIC::is_enabled(first) {
        return Err(IrqError::NotEnabled(first));
    }
    let priority = NVIC::get_priority(first);
    for vector in vectors {
        if !NVIC::is_enabled(vector) {
            return Err(IrqError::NotEnabled(vector));
        }
        if NVIC::get_priority(vector) != priority {
            return Err(IrqError::PriorityMismatch(first, vector));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Refused;

    /// Not `Copy` or `Clone`, as a token is not.
    #[derive(Debug, PartialEq, Eq)]
    struct Token(u8);

    #[test]
    fn a_refusal_hands_back_what_it_took() {
        let mut ring = [0u8; 4];
        let address = ring.as_ptr();
        let refused = Refused::new(7u8, (Token(1), Token(2), &mut ring[..]));
        assert_eq!(refused.error(), 7);
        let (first, second, buffer) = refused.into_parts();
        assert_eq!((first, second), (Token(1), Token(2)));
        assert_eq!(buffer.as_ptr(), address);
        assert_eq!(buffer.len(), 4);
    }

    #[test]
    fn a_refusal_comes_apart_whole() {
        // A reason that is not `Copy` either, so only `into_inner` reaches it.
        let (error, part) = Refused::new(Token(9), Token(3)).into_inner();
        assert_eq!((error, part), (Token(9), Token(3)));
    }

    #[test]
    fn a_refusal_prints_its_reason_alone() {
        use core::fmt::Write;

        struct Out([u8; 32], usize);
        impl Write for Out {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                let end = self.1 + s.len();
                self.0
                    .get_mut(self.1..end)
                    .ok_or(core::fmt::Error)?
                    .copy_from_slice(s.as_bytes());
                self.1 = end;
                Ok(())
            }
        }
        let mut out = Out([0; 32], 0);
        write!(
            out,
            "{:?}",
            Refused::new(crate::uart::ConfigError::BaudRate, Token(1))
        )
        .unwrap();
        assert_eq!(&out.0[..out.1], b"BaudRate");
    }
}
