//! The stream DMA, exposed as it is: `dma_v2` (F2, F4, F7) and `dma_v1`
//! (H7). The two differ only in how a stream picks its request, so they
//! share this source:
//!
//! - `dma_v2`: a channel number (CHSEL, 0 to 7) on the stream, from the
//!   chip's fixed request table.
//! - `dma_v1`: a request ID (1 to 127) written to the stream's own DMAMUX
//!   channel, so any stream can serve any request. It also has TRBUFF,
//!   which RM0433 requires for U(S)ART transfers.
//!
//! Each controller has eight streams. A stream serves one request at a
//! time; it has a FIFO with a direct mode, double buffering, and five
//! flags: transfer complete, half transfer, transfer error, direct-mode
//! error and FIFO error. All of them are here, readable and clearable by
//! the caller.
//!
//! A controller token splits into eight stream tokens. Drivers erase a
//! stream token into a [`Channel`], one type for every stream. Starting a
//! transfer is `unsafe`: the caller vouches for the memory. Drivers above
//! build safe APIs on it from memory they own.
//!
//! Shared registers: the flag registers (LISR, HISR) hold four streams
//! each, but they are read-only, and their clear registers (LIFCR, HIFCR)
//! are write-one-to-clear, so a stream clears its own flags with a single
//! write that cannot disturb the others. On `dma_v1`, each stream owns its
//! DMAMUX channel's configuration register outright. The controller's clock
//! is the only shared write, made through [`Rcc`].
//!
//! Requirements: DMA-001

use core::sync::atomic::{Ordering, fence};

use stm32_metapac::dma::{
    Dma as Regs,
    vals::{Dir, Dmdis, Fth, Pl, Size},
};

#[cfg(feature = "dma_v1")]
use stm32_metapac::dmamux::Dmamux;

use crate::rcc::{Rcc, RccBit};

/// A DMA controller token from `Peripherals`; `D` is its number.
pub struct Dma<const D: u8> {
    _private: (),
}

impl<const D: u8> Dma<D> {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }
}

/// The controller's register block and its DMAMUX. Crate-internal: a public
/// bound naming them would hand metapac out.
pub(crate) trait DmaRegisters {
    fn regs() -> Regs;
    #[cfg(feature = "dma_v1")]
    fn mux() -> Dmamux;
}

/// A DMA controller this chip has. Chip facts: the glue layer implements it.
#[allow(private_bounds)] // `DmaRegisters` is the crate-internal half.
pub trait DmaInstance: crate::Sealed + DmaRegisters {
    #[doc(hidden)]
    const ENABLE: RccBit;
    #[cfg(feature = "dma_v1")]
    #[doc(hidden)]
    const MUX_BASE: u8;
}

/// A controller's eight streams.
pub struct Streams<const D: u8> {
    pub s0: Stream<D, 0>,
    pub s1: Stream<D, 1>,
    pub s2: Stream<D, 2>,
    pub s3: Stream<D, 3>,
    pub s4: Stream<D, 4>,
    pub s5: Stream<D, 5>,
    pub s6: Stream<D, 6>,
    pub s7: Stream<D, 7>,
}

impl<const D: u8> Dma<D>
where
    Dma<D>: DmaInstance,
{
    /// Turns on the controller's clock and hands out its streams. The
    /// controller is not reset: its streams belong to different drivers.
    pub fn split(self, rcc: &mut Rcc) -> Streams<D> {
        rcc.enable(<Dma<D> as DmaInstance>::ENABLE);
        Streams {
            s0: Stream::new(),
            s1: Stream::new(),
            s2: Stream::new(),
            s3: Stream::new(),
            s4: Stream::new(),
            s5: Stream::new(),
            s6: Stream::new(),
            s7: Stream::new(),
        }
    }
}

/// Stream `N` of controller `D`. Zero-sized; routing traits are implemented
/// on it, so the type checks which requests it can serve.
pub struct Stream<const D: u8, const N: u8> {
    _private: (),
}

impl<const D: u8, const N: u8> Stream<D, N> {
    const fn new() -> Self {
        Self { _private: () }
    }

    /// A second token for this stream.
    ///
    /// # Safety
    ///
    /// Nothing else may use the stream while this one is in use.
    pub unsafe fn steal() -> Self {
        Self::new()
    }
}

impl<const D: u8, const N: u8> Stream<D, N> {
    /// The DMA controller's number.
    pub const CONTROLLER: u8 = D;
    /// The stream's number on its controller.
    pub const NUMBER: u8 = N;
}

/// The interrupt vector a stream raises. Chip facts: the glue implements
/// it for every stream.
pub trait StreamInterrupt {
    const INTERRUPT: stm32_metapac::Interrupt;
    const NAME: &'static str;
}

/// A stream token, erasable into a [`Channel`].
///
/// Outside FerroHAL a token's stream is reached only by giving the token
/// up. The drivers' constructors reach it while the caller keeps the
/// token, so that a refusal can hand it back, through a method that needs
/// a key only FerroHAL can make. Giving the token up works through any
/// bound:
///
/// ```
/// fn one_owner<S: ferrohal_stm32::dma::StreamInstance>(token: S) -> ferrohal_stm32::dma::Channel {
///     token.into_channel()
/// }
/// ```
///
/// Reaching it while keeping the token doesn't, and neither does naming the
/// key. An argument that never returns, such as `todo!()`, compiles, but
/// the call never happens. Stable rustdoc doesn't check the error codes
/// below; they are the ones the compiler gives.
///
/// ```compile_fail,E0061
/// fn two_owners<S: ferrohal_stm32::dma::StreamInstance>(token: &S) {
///     let _ = (token.claim(), token.claim());
/// }
/// ```
///
/// ```compile_fail,E0603
/// fn a_key() -> ferrohal_stm32::dma::claim::Key {
///     todo!()
/// }
/// ```
pub trait StreamInstance: crate::Sealed + claim::Claim {
    fn into_channel(self) -> Channel;
}

mod claim {
    /// Proof that a call comes from inside FerroHAL: only this module can
    /// make one, and nothing hands one out.
    pub struct Key(());

    /// A stream reached while the caller's token stays where it is. Any
    /// code can see this method through a `StreamInstance` bound, so it
    /// takes a [`Key`].
    pub trait Claim {
        fn claim(&self, key: Key) -> super::Channel;
    }

    /// The stream's [`Channel`](super::Channel), a second owner beside the
    /// token. The constructor that calls this keeps the pair to one: on
    /// success it drops the token and keeps the channel, on a refusal it
    /// drops the channel and returns the token.
    pub(crate) fn channel<S: Claim>(token: &S) -> super::Channel {
        token.claim(Key(()))
    }
}

pub(crate) use claim::channel as claim_channel;

impl<const D: u8, const N: u8> crate::Sealed for Stream<D, N> {}

impl<const D: u8, const N: u8> StreamInstance for Stream<D, N>
where
    Dma<D>: DmaInstance,
{
    fn into_channel(self) -> Channel {
        claim::channel(&self)
    }
}

impl<const D: u8, const N: u8> claim::Claim for Stream<D, N>
where
    Dma<D>: DmaInstance,
{
    fn claim(&self, _key: claim::Key) -> Channel {
        const { assert!(N < 8, "a controller has streams 0 to 7") };
        Channel {
            regs: <Dma<D> as DmaRegisters>::regs(),
            index: N,
            #[cfg(feature = "dma_v1")]
            mux: <Dma<D> as DmaRegisters>::mux(),
            #[cfg(feature = "dma_v1")]
            mux_channel: <Dma<D> as DmaInstance>::MUX_BASE + N,
        }
    }
}

/// One stream, whichever it is. Owned: one `Channel` per stream.
pub struct Channel {
    regs: Regs,
    index: u8,
    #[cfg(feature = "dma_v1")]
    mux: Dmamux,
    #[cfg(feature = "dma_v1")]
    mux_channel: u8,
}

// SAFETY: a `Channel` is the only owner of its stream's registers, and of
// its bits in the shared flag registers, so moving it to another context
// moves that ownership with it.
unsafe impl Send for Channel {}

/// A stream's status flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Flags {
    pub complete: bool,
    pub half: bool,
    /// A bus error. The stream has disabled itself.
    pub transfer_error: bool,
    /// In direct mode, the stream could not keep up. In peripheral-to-memory
    /// mode with a buffer that is not ready, the stream disables itself.
    pub direct_mode_error: bool,
    /// The FIFO over- or underran, or its threshold does not fit the burst.
    pub fifo_error: bool,
}

impl Flags {
    pub const ALL: Self = Self {
        complete: true,
        half: true,
        transfer_error: true,
        direct_mode_error: true,
        fifo_error: true,
    };

    pub const fn any(self) -> bool {
        self.complete
            || self.half
            || self.transfer_error
            || self.direct_mode_error
            || self.fifo_error
    }

    pub const fn any_error(self) -> bool {
        self.transfer_error || self.direct_mode_error || self.fifo_error
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Direction {
    PeripheralToMemory,
    MemoryToPeripheral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Priority {
    Low,
    Medium,
    High,
    VeryHigh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum WordSize {
    Byte,
    HalfWord,
    Word,
}

impl WordSize {
    const fn bits(self) -> Size {
        match self {
            Self::Byte => Size::BITS8,
            Self::HalfWord => Size::BITS16,
            Self::Word => Size::BITS32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FifoMode {
    /// Each word goes straight through. Peripheral and memory word sizes
    /// must match.
    Direct,
    /// Buffered, flushed to memory at this fill level.
    Threshold(FifoThreshold),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FifoThreshold {
    Quarter,
    Half,
    ThreeQuarters,
    Full,
}

/// Which stream interrupts to raise. Their flags are set either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Interrupts {
    pub complete: bool,
    pub half: bool,
    /// Transfer error and direct-mode error.
    pub error: bool,
    pub fifo_error: bool,
}

/// One transfer, stated in full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Transfer {
    /// Which request the stream serves. `dma_v2`: CHSEL, 0 to 7.
    /// `dma_v1`: the DMAMUX request ID, 1 to 127.
    pub request: u8,
    /// TRBUFF: bufferable transfers. RM0433 requires it for U(S)ART and
    /// LPUART, and forbids it otherwise.
    #[cfg(feature = "dma_v1")]
    pub bufferable: bool,
    pub direction: Direction,
    /// The peripheral register's address.
    pub peripheral: u32,
    /// The memory buffer's address (M0AR).
    pub memory: u32,
    /// The second buffer (M1AR), for double-buffer mode. The stream switches
    /// buffers each time it completes one, which implies circular mode.
    pub double_buffer: Option<u32>,
    /// Words to move, 1 to 65 535.
    pub len: u16,
    pub size: WordSize,
    pub memory_increment: bool,
    pub circular: bool,
    pub priority: Priority,
    pub fifo: FifoMode,
    pub interrupts: Interrupts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum StartError {
    /// The stream is still enabled. Stop it first.
    Busy,
    /// `len` is 0, or `request` is outside its range.
    Invalid,
}

/// The stream did not report itself disabled within the bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct StopTimeout;

/// Which buffer a double-buffered stream is filling or draining.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Target {
    Memory0,
    Memory1,
}

impl Channel {
    /// A handle for reading and clearing this stream's flags from an
    /// interrupt handler while the `Channel` itself lives elsewhere.
    pub fn flag_handle(&self) -> FlagHandle {
        FlagHandle {
            regs: self.regs,
            index: self.index,
        }
    }

    pub fn flags(&self) -> Flags {
        self.flag_handle().flags()
    }

    /// Clears the flags set in `flags`, and no others.
    pub fn clear_flags(&mut self, flags: Flags) {
        self.flag_handle().clear(flags);
    }

    /// NDTR: words still to move in the current buffer.
    pub fn remaining(&self) -> u16 {
        self.flag_handle().remaining()
    }

    pub fn is_running(&self) -> bool {
        self.regs.st(stream_index(self.index)).cr().read().en()
    }

    /// In double-buffer mode, the buffer the stream is using now.
    pub fn current_target(&self) -> Target {
        if self.regs.st(stream_index(self.index)).cr().read().ct()
            == stm32_metapac::dma::vals::Ct::MEMORY1
        {
            Target::Memory1
        } else {
            Target::Memory0
        }
    }

    /// Points the buffer the stream is not using at `address`.
    ///
    /// # Safety
    ///
    /// The memory at `address` must stay valid, and untouched by anything
    /// but this stream, until the stream has moved on from it or stopped.
    pub unsafe fn set_idle_buffer(&mut self, address: u32) {
        let st = self.regs.st(stream_index(self.index));
        match self.current_target() {
            Target::Memory0 => st.m1ar().write_value(address),
            Target::Memory1 => st.m0ar().write_value(address),
        }
    }

    /// Turns the stream's interrupts on or off. Allowed while it runs: the
    /// reference manuals protect most of CR while EN is set, but not the
    /// interrupt enables, nor FCR's FEIE.
    pub fn set_interrupts(&mut self, interrupts: Interrupts) {
        let st = self.regs.st(stream_index(self.index));
        st.cr().modify(|w| {
            w.set_tcie(interrupts.complete);
            w.set_htie(interrupts.half);
            w.set_teie(interrupts.error);
            w.set_dmeie(interrupts.error);
        });
        st.fcr().modify(|w| w.set_feie(interrupts.fifo_error));
    }

    /// A second handle on this stream, for a driver whose task and
    /// interrupt both start transfers.
    ///
    /// # Safety
    ///
    /// The two handles must never use the stream at the same time; the
    /// driver's own protocol has to guarantee it.
    pub(crate) unsafe fn alias(&self) -> Channel {
        Channel {
            regs: self.regs,
            index: self.index,
            #[cfg(feature = "dma_v1")]
            mux: self.mux,
            #[cfg(feature = "dma_v1")]
            mux_channel: self.mux_channel,
        }
    }

    /// Tells the stream to stop. It finishes the word in flight first;
    /// [`is_running`](Self::is_running) turns false once it has.
    pub fn request_stop(&mut self) {
        self.regs
            .st(stream_index(self.index))
            .cr()
            .modify(|w| w.set_en(false));
    }

    /// Stops the stream and waits at most `spins` reads for it to report
    /// itself stopped. Stopping a running stream sets its complete flag;
    /// that is left for the caller to clear.
    pub fn stop(&mut self, spins: u32) -> Result<(), StopTimeout> {
        self.request_stop();
        for _ in 0..spins {
            if !self.is_running() {
                // Nothing the stream wrote may be read before this.
                fence(Ordering::SeqCst);
                return Ok(());
            }
        }
        if self.is_running() {
            Err(StopTimeout)
        } else {
            fence(Ordering::SeqCst);
            Ok(())
        }
    }

    /// Configures and starts a transfer.
    ///
    /// # Safety
    ///
    /// `memory` (and `double_buffer`) must point to `len` words of memory
    /// that stay valid until the stream has stopped, and that nothing but
    /// this stream writes in the meantime (or reads, for a
    /// peripheral-to-memory transfer). `peripheral` must be the data
    /// register of the peripheral `request` selects on this stream.
    pub unsafe fn start(&mut self, transfer: &Transfer) -> Result<(), StartError> {
        #[cfg(feature = "dma_v2")]
        let request_valid = transfer.request <= 7;
        #[cfg(feature = "dma_v1")]
        let request_valid = (1..=127).contains(&transfer.request);
        if transfer.len == 0 || !request_valid {
            return Err(StartError::Invalid);
        }
        if self.is_running() {
            return Err(StartError::Busy);
        }
        let st = self.regs.st(stream_index(self.index));
        self.clear_flags(Flags::ALL);

        // No synchronisation, no event generation: the request passes
        // straight through to the stream.
        #[cfg(feature = "dma_v1")]
        self.mux
            .ccr(usize::from(self.mux_channel & 15))
            .write(|w| w.set_dmareq_id(transfer.request));

        st.par().write_value(transfer.peripheral);
        st.m0ar().write_value(transfer.memory);
        if let Some(second) = transfer.double_buffer {
            st.m1ar().write_value(second);
        }
        st.ndtr().write(|w| w.set_ndt(transfer.len));
        st.fcr().write(|w| match transfer.fifo {
            FifoMode::Direct => {
                w.set_dmdis(Dmdis::ENABLED);
                w.set_feie(transfer.interrupts.fifo_error);
            }
            FifoMode::Threshold(threshold) => {
                w.set_dmdis(Dmdis::DISABLED);
                w.set_fth(match threshold {
                    FifoThreshold::Quarter => Fth::QUARTER,
                    FifoThreshold::Half => Fth::HALF,
                    FifoThreshold::ThreeQuarters => Fth::THREE_QUARTERS,
                    FifoThreshold::Full => Fth::FULL,
                });
                w.set_feie(transfer.interrupts.fifo_error);
            }
        });
        st.cr().write(|w| {
            #[cfg(feature = "dma_v2")]
            w.set_chsel(transfer.request);
            #[cfg(feature = "dma_v1")]
            w.set_trbuff(transfer.bufferable);
            w.set_dir(match transfer.direction {
                Direction::PeripheralToMemory => Dir::PERIPHERAL_TO_MEMORY,
                Direction::MemoryToPeripheral => Dir::MEMORY_TO_PERIPHERAL,
            });
            w.set_psize(transfer.size.bits());
            w.set_msize(transfer.size.bits());
            w.set_minc(transfer.memory_increment);
            w.set_circ(transfer.circular || transfer.double_buffer.is_some());
            w.set_dbm(transfer.double_buffer.is_some());
            w.set_pl(match transfer.priority {
                Priority::Low => Pl::LOW,
                Priority::Medium => Pl::MEDIUM,
                Priority::High => Pl::HIGH,
                Priority::VeryHigh => Pl::VERY_HIGH,
            });
            w.set_tcie(transfer.interrupts.complete);
            w.set_htie(transfer.interrupts.half);
            w.set_teie(transfer.interrupts.error);
            w.set_dmeie(transfer.interrupts.error);
        });
        // Everything the CPU wrote to the buffers lands before DMA owns them.
        fence(Ordering::SeqCst);
        st.cr().modify(|w| w.set_en(true));
        Ok(())
    }
}

/// A stream number as a register index. Streams are 0 to 7 by
/// construction; the mask lets the compiler see that too, so metapac's
/// bounds checks on `st(n)`, `isr(n)` and the flag fields fold away and no
/// panic path is left.
fn stream_index(index: u8) -> usize {
    usize::from(index & 7)
}

/// Read and clear access to one stream's flags, and its count.
///
/// Every operation is a register read or a write-one-to-clear of this
/// stream's own bits, so a handle and its `Channel` can be used from
/// different priorities without a lock. Which of them clears which flag is
/// the driver's protocol to keep.
pub struct FlagHandle {
    regs: Regs,
    index: u8,
}

// SAFETY: see the type's documentation: nothing it does can race.
unsafe impl Send for FlagHandle {}

impl FlagHandle {
    fn slot(&self) -> (usize, usize) {
        let index = stream_index(self.index);
        (index / 4, index % 4)
    }

    pub fn flags(&self) -> Flags {
        let (register, slot) = self.slot();
        let isr = self.regs.isr(register).read();
        Flags {
            complete: isr.tcif(slot),
            half: isr.htif(slot),
            transfer_error: isr.teif(slot),
            direct_mode_error: isr.dmeif(slot),
            fifo_error: isr.feif(slot),
        }
    }

    pub fn clear(&self, flags: Flags) {
        let (register, slot) = self.slot();
        self.regs.ifcr(register).write(|w| {
            w.set_tcif(slot, flags.complete);
            w.set_htif(slot, flags.half);
            w.set_teif(slot, flags.transfer_error);
            w.set_dmeif(slot, flags.direct_mode_error);
            w.set_feif(slot, flags.fifo_error);
        });
    }

    pub fn remaining(&self) -> u16 {
        self.regs.st(stream_index(self.index)).ndtr().read().ndt()
    }
}
