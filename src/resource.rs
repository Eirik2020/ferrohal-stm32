//! What a driver's wiring uses, as data, and the check that its interrupt
//! vectors are bound as it needs. Shared by every peripheral driver.
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
