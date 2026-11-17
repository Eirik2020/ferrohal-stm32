//! What a driver's wiring uses, as data, the vectors it needs and the check
//! that they are bound, the [`Service`] every interrupt-served driver
//! offers, and what a constructor that refuses hands back. Shared by every
//! peripheral driver.
//!
//! Requirements: RESOURCE-001, RESOURCE-002

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
    /// An analog pin — an ADC input, with no alternate function.
    Analog { signal: Signal, port: char, pin: u8 },
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

/// An instance's static facts: its register wrapper, held by the glue in one
/// `static` per instance. One shape for every driver that keeps one
/// (API review U6: SPI's and I2C's were the same type written twice); each
/// driver module states it is `Sync` for its own `Unit`.
pub struct Info<U> {
    unit: U,
}

impl<U: Copy> Info<U> {
    pub(crate) const fn new(unit: U) -> Self {
        Self { unit }
    }

    pub(crate) fn unit(&self) -> U {
        self.unit
    }
}

impl Resource {
    /// A pin row from the pin's type: its port, number and alternate
    /// function, carrying `signal` (API review U5).
    pub(crate) const fn pin<P: crate::gpio::AltPin>(signal: Signal) -> Self {
        Self::Pin {
            signal,
            port: P::PORT,
            pin: P::PIN,
            af: P::AF,
        }
    }
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
    /// An ADC input, by its channel number.
    Adc(u8),
    /// USB's D- line.
    Dm,
    /// USB's D+ line.
    Dp,
    /// A card bus's clock.
    Ck,
    /// A card bus's command line.
    Cmd,
    /// A card bus's data line, by its number.
    Data(u8),
}

/// What a vector check found wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum IrqError {
    /// This vector is masked in the NVIC: nothing serves it.
    NotEnabled(crate::chip::Interrupt),
}

/// A driver served from interrupts: the scheduler's handler for each of the
/// driver's [`Vectors`] calls [`on_interrupt`](Self::on_interrupt), and acts
/// on the event it returns (spawns a reader, wakes a task). Neutral about the
/// scheduler: RTIC, an executor or a bare `#[interrupt]` all call the same
/// method (ADR-011).
///
/// The driver has one owner, and `on_interrupt` takes it `&mut`: a task and
/// a handler that share a driver share it under the scheduler's lock, which
/// is the only concurrency control there is. `on_interrupt` is also safe to
/// call from a task, with nothing pending, which is how firmware with the
/// vectors unbound polls. Each driver also has it as an inherent method, so
/// callers need not import this trait.
pub trait Service {
    /// What one service found: a completion, a fault, a frame's end.
    type Event;

    /// Serves whatever the driver's vectors have pending, and says what
    /// happened.
    fn on_interrupt(&mut self) -> Self::Event;
}

/// The most vectors one driver needs: an arbitrary waveform generator's
/// eight lanes.
pub const MAX_VECTORS: usize = 8;

/// The interrupt vectors a driver needs bound to its
/// [`on_interrupt`](Service::on_interrupt), from the driver's `vectors()`.
/// Each may run at its own priority; the scheduler's lock orders them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Vectors {
    list: [Option<crate::chip::Interrupt>; MAX_VECTORS],
}

impl Vectors {
    /// No vectors.
    pub(crate) const fn none() -> Self {
        Self {
            list: [None; MAX_VECTORS],
        }
    }

    /// One vector.
    pub(crate) const fn one(vector: crate::chip::Interrupt) -> Self {
        Self::none().with(vector)
    }

    /// These and `vector`, unless it is listed already. A ninth is dropped:
    /// no driver has one.
    pub(crate) const fn with(mut self, vector: crate::chip::Interrupt) -> Self {
        let mut i = 0;
        while i < MAX_VECTORS {
            match self.list[i] {
                Some(listed) if listed as u16 == vector as u16 => return self,
                Some(_) => i += 1,
                None => {
                    self.list[i] = Some(vector);
                    return self;
                }
            }
        }
        self
    }

    /// Every vector, in the order the driver lists them, each once.
    pub fn iter(&self) -> impl Iterator<Item = crate::chip::Interrupt> + '_ {
        self.list.iter().flatten().copied()
    }

    /// Whether `vector` is one of these.
    pub fn contains(&self, vector: crate::chip::Interrupt) -> bool {
        self.iter().any(|listed| listed == vector)
    }

    /// Checks the NVIC, reading only: every vector enabled. FerroHAL never
    /// sets NVIC state; this tells the firmware whether its scheduler set it
    /// as the driver needs.
    pub fn verify(&self) -> Result<(), IrqError> {
        verify_vectors(self.iter())
    }
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

/// Checks the NVIC, reading only: every vector enabled.
fn verify_vectors(vectors: impl Iterator<Item = crate::chip::Interrupt>) -> Result<(), IrqError> {
    use cortex_m::peripheral::NVIC;
    for vector in vectors {
        if !NVIC::is_enabled(vector) {
            return Err(IrqError::NotEnabled(vector));
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
