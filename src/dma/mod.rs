//! The DMA (metapac's `dma`), exposed as it is: `dma_v2` (F2, F4, F7) and
//! `dma_v1` (H7). Its units are channels, and what a channel serves is a
//! request, on every chip, as metapac's metadata names them (`DMA1_CH0`,
//! `request`); the reference manuals call a channel a stream, and the F4's
//! request its channel selection (CHSEL). The two versions differ only in
//! how a channel picks its request, so they share this source:
//!
//! - `dma_v2`: a request number (CHSEL, 0 to 7) on the channel, from the
//!   chip's fixed request table.
//! - `dma_v1`: a request ID (1 to 127) written to the DMAMUX channel the
//!   channel owns, so any channel can serve any request. It also has
//!   TRBUFF, which RM0433 requires for U(S)ART transfers.
//!
//! Each controller has eight channels. A channel serves one request at a
//! time; it has a FIFO, which every transfer here bypasses in direct
//! mode, and five flags: transfer complete, half transfer, transfer error,
//! direct-mode error and FIFO error. All of them are here, readable and
//! clearable by the caller. The engine's double buffering and FIFO
//! thresholds are not: no driver used them, and nothing proved them.
//!
//! A controller token splits into eight channel tokens. Drivers erase a
//! channel token into an [`AnyChannel`], one type for every channel. Starting a
//! transfer is `unsafe`: the caller vouches for the memory. Drivers above
//! build safe APIs on it from memory they own.
//!
//! Shared registers: the flag registers (LISR, HISR) hold four channels
//! each, but they are read-only, and their clear registers (LIFCR, HIFCR)
//! are write-one-to-clear, so a channel clears its own flags with a single
//! write that cannot disturb the others. On `dma_v1`, each channel owns its
//! DMAMUX channel's configuration register outright. The controller's clock
//! is the only shared write, made through [`Rcc`].
//!
//! With the `fault-injection` feature, `fault` makes a channel ignore
//! requests to stop, for benches that prove a driver's answer to a channel
//! that will not stop. No firmware that flies enables it.
//!
//! Requirements: DMA-001

use core::sync::atomic::{Ordering, fence};

/// Polls allowed for a channel to report itself stopped, every driver's
/// bound: a stopping channel finishes at most its current beat, so this is
/// generous. The ADC gave 100 000 before 2026-11-17 (API review U8), for no
/// recorded reason.
pub(crate) const STOP_SPINS: u32 = 10_000;

/// Bench fault injection (feature `fault-injection`): a channel marked stuck
/// ignores [`AnyChannel::request_stop`], so a running channel keeps EN set and
/// every bounded stop on it times out, as a channel that will not stop
/// would. A channel that has stopped on its own still reads stopped. Marks
/// are global, by controller and channel number, and survive a driver's
/// `free`; nothing in FerroHAL sets one.
#[cfg(feature = "fault-injection")]
pub mod fault {
    use core::sync::atomic::{AtomicU32, Ordering};

    static STUCK: AtomicU32 = AtomicU32::new(0);

    const fn bit(controller: u8, channel: u8) -> u32 {
        1 << (((controller & 3) as u32) * 8 + (channel & 7) as u32)
    }

    /// Channel `channel` of DMA controller `controller` ignores stop requests
    /// from now on.
    pub fn stick(controller: u8, channel: u8) {
        STUCK.fetch_or(bit(controller, channel), Ordering::SeqCst);
    }

    /// The channel obeys stop requests again. It may still be running.
    pub fn release(controller: u8, channel: u8) {
        STUCK.fetch_and(!bit(controller, channel), Ordering::SeqCst);
    }

    pub(super) fn stuck(controller: u8, channel: u8) -> bool {
        STUCK.load(Ordering::SeqCst) & bit(controller, channel) != 0
    }
}

use stm32_metapac::dma::{
    Dma as Regs,
    vals::{Dir, Dmdis, Pl, Size},
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

/// A controller's eight channels.
pub struct Channels<const D: u8> {
    pub ch0: Channel<D, 0>,
    pub ch1: Channel<D, 1>,
    pub ch2: Channel<D, 2>,
    pub ch3: Channel<D, 3>,
    pub ch4: Channel<D, 4>,
    pub ch5: Channel<D, 5>,
    pub ch6: Channel<D, 6>,
    pub ch7: Channel<D, 7>,
}

impl<const D: u8> Dma<D>
where
    Dma<D>: DmaInstance,
{
    /// Turns on the controller's clock and hands out its channels. The
    /// controller is not reset: its channels belong to different drivers.
    pub fn split(self, rcc: &mut Rcc) -> Channels<D> {
        rcc.enable(<Dma<D> as DmaInstance>::ENABLE);
        Channels {
            ch0: Channel::new(),
            ch1: Channel::new(),
            ch2: Channel::new(),
            ch3: Channel::new(),
            ch4: Channel::new(),
            ch5: Channel::new(),
            ch6: Channel::new(),
            ch7: Channel::new(),
        }
    }
}

/// Channel `N` of controller `D`. Zero-sized; routing traits are implemented
/// on it, so the type checks which requests it can serve.
pub struct Channel<const D: u8, const N: u8> {
    _private: (),
}

impl<const D: u8, const N: u8> Channel<D, N> {
    /// The token. Made only where one is owed: by `Dma::split`, once per
    /// channel, and by `timer::ReaderDma::take`, for a token a reader was
    /// given and has been freed of.
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }
}

impl<const D: u8, const N: u8> Channel<D, N> {
    /// The DMA controller's number.
    pub const CONTROLLER: u8 = D;
    /// The channel's number on its controller.
    pub const NUMBER: u8 = N;
}

/// The interrupt vector a channel raises. Chip facts: the glue implements
/// it for every channel.
pub trait ChannelInterrupt {
    const INTERRUPT: stm32_metapac::Interrupt;
    /// The vector's name.
    const INTERRUPT_NAME: &'static str;
}

/// A channel token, erasable into an [`AnyChannel`].
///
/// Outside FerroHAL a token's channel is reached only by giving the token
/// up. The drivers' constructors reach it while the caller keeps the
/// token, so that a refusal can hand it back, through a method that needs
/// a key only FerroHAL can make. Giving the token up works through any
/// bound:
///
/// ```
/// fn one_owner<S: ferrohal_stm32::dma::ChannelInstance>(token: S) -> ferrohal_stm32::dma::AnyChannel {
///     token.into_any()
/// }
/// ```
///
/// Reaching it while keeping the token doesn't, and neither does naming the
/// key. An argument that never returns, such as `todo!()`, compiles, but
/// the call never happens. Stable rustdoc doesn't check the error codes
/// below; they are the ones the compiler gives.
///
/// ```compile_fail,E0061
/// fn two_owners<S: ferrohal_stm32::dma::ChannelInstance>(token: &S) {
///     let _ = (token.claim(), token.claim());
/// }
/// ```
///
/// ```compile_fail,E0603
/// fn a_key() -> ferrohal_stm32::dma::claim::Key {
///     todo!()
/// }
/// ```
pub trait ChannelInstance: crate::Sealed + claim::Claim {
    /// The controller's number and the channel's on it.
    const ID: (u8, u8);
    fn into_any(self) -> AnyChannel;
}

mod claim {
    /// Proof that a call comes from inside FerroHAL: only this module can
    /// make one, and nothing hands one out.
    pub struct Key(());

    /// A channel reached while the caller's token stays where it is. Any
    /// code can see this method through a `ChannelInstance` bound, so it
    /// takes a [`Key`].
    pub trait Claim {
        fn claim(&self, key: Key) -> super::AnyChannel;
    }

    /// The channel's [`AnyChannel`](super::AnyChannel), a second owner beside the
    /// token. The constructor that calls this keeps the pair to one: on
    /// success it drops the token and keeps the channel, on a refusal it
    /// drops the channel and returns the token.
    pub(crate) fn channel<S: Claim>(token: &S) -> super::AnyChannel {
        token.claim(Key(()))
    }
}

pub(crate) use claim::channel as claim;

impl<const D: u8, const N: u8> crate::Sealed for Channel<D, N> {}

impl<const D: u8, const N: u8> ChannelInstance for Channel<D, N>
where
    Dma<D>: DmaInstance,
{
    const ID: (u8, u8) = (D, N);
    fn into_any(self) -> AnyChannel {
        claim::channel(&self)
    }
}

impl<const D: u8, const N: u8> claim::Claim for Channel<D, N>
where
    Dma<D>: DmaInstance,
{
    fn claim(&self, _key: claim::Key) -> AnyChannel {
        const { assert!(N < 8, "a controller has channels 0 to 7") };
        AnyChannel {
            regs: <Dma<D> as DmaRegisters>::regs(),
            index: N,
            #[cfg(feature = "fault-injection")]
            controller: D,
            #[cfg(feature = "dma_v1")]
            mux: <Dma<D> as DmaRegisters>::mux(),
            #[cfg(feature = "dma_v1")]
            mux_channel: <Dma<D> as DmaInstance>::MUX_BASE + N,
        }
    }
}

/// One channel, whichever it is. Owned: one `AnyChannel` per channel.
pub struct AnyChannel {
    regs: Regs,
    index: u8,
    #[cfg(feature = "fault-injection")]
    controller: u8,
    #[cfg(feature = "dma_v1")]
    mux: Dmamux,
    #[cfg(feature = "dma_v1")]
    mux_channel: u8,
}

// SAFETY: an `AnyChannel` is the only owner of its channel's registers, and of
// its bits in the shared flag registers, so moving it to another context
// moves that ownership with it.
unsafe impl Send for AnyChannel {}

/// Why a buffer cannot serve a DMA transfer: what `chip::check_dma_buffer`
/// returns, and what every driver's refusal wraps as its `Buffer` reason
/// (API review D6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BufferError {
    /// The buffer lies in memory the DMA controller cannot reach, such as the
    /// H7's DTCM or the F405's CCM RAM.
    Unreachable,
    /// The data cache is on and the buffer is not in memory the MPU marks
    /// non-cacheable. The drivers do no cache maintenance, so they refuse
    /// rather than move stale bytes.
    Cacheable,
}

/// A channel's status flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Flags {
    pub complete: bool,
    pub half: bool,
    /// A bus error. The channel has disabled itself.
    pub transfer_error: bool,
    /// In direct mode, the channel could not keep up. In peripheral-to-memory
    /// mode with a buffer that is not ready, the channel disables itself.
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

    /// A transfer or direct-mode error: the ones that stop a transfer. A
    /// FIFO error is advisory in direct mode.
    pub const fn failed(self) -> bool {
        self.transfer_error || self.direct_mode_error
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

/// Which channel interrupts to raise. Their flags are set either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Interrupts {
    pub complete: bool,
    pub half: bool,
    /// Transfer error and direct-mode error.
    pub error: bool,
    pub fifo_error: bool,
}

/// The interrupts a one-shot transfer raises: its end, and an error.
pub(crate) const ONE_SHOT_INTERRUPTS: Interrupts = Interrupts {
    complete: true,
    half: false,
    error: true,
    fifo_error: false,
};

impl Transfer {
    /// One pass over `len` words of memory, incrementing, at `priority`,
    /// raising [`ONE_SHOT_INTERRUPTS`], not bufferable: the shape every
    /// driver's transfer starts from (API review U3).
    pub(crate) const fn one_shot(
        request: u8,
        direction: Direction,
        peripheral: u32,
        memory: u32,
        len: u16,
        size: WordSize,
        priority: Priority,
    ) -> Self {
        Self {
            request,
            #[cfg(feature = "dma_v1")]
            bufferable: false,
            direction,
            peripheral,
            memory,
            len,
            size,
            memory_increment: true,
            circular: false,
            priority,
            interrupts: ONE_SHOT_INTERRUPTS,
        }
    }

    /// Round and round the buffer, for ever.
    pub(crate) const fn circular(self) -> Self {
        Self {
            circular: true,
            ..self
        }
    }

    pub(crate) const fn with_interrupts(self, interrupts: Interrupts) -> Self {
        Self { interrupts, ..self }
    }

    /// TRBUFF on `dma_v1`, which RM0433 requires for a U(S)ART's channel
    /// (ES0392 2.20.4: it keeps the channel from locking when another
    /// transfer runs beside it). `dma_v2` has no such bit: unchanged.
    pub(crate) const fn for_usart(self) -> Self {
        #[cfg(feature = "dma_v1")]
        return Self {
            bufferable: true,
            ..self
        };
        #[cfg(not(feature = "dma_v1"))]
        self
    }
}

/// One transfer, stated in full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Transfer {
    /// Which request the channel serves. `dma_v2`: CHSEL, 0 to 7.
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
    /// Words to move, 1 to 65 535.
    pub len: u16,
    pub size: WordSize,
    pub memory_increment: bool,
    pub circular: bool,
    pub priority: Priority,
    pub interrupts: Interrupts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum StartError {
    /// The channel is still enabled. Stop it first.
    Busy,
    /// `len` is 0, or `request` is outside its range.
    Invalid,
}

/// The channel did not report itself disabled within the bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct StopTimeout;

impl AnyChannel {
    /// A handle for reading and clearing this channel's flags from an
    /// interrupt handler while the `AnyChannel` itself lives elsewhere.
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
        self.regs.st(channel_index(self.index)).cr().read().en()
    }

    /// Turns the channel's interrupts on or off. Allowed while it runs: the
    /// reference manuals protect most of CR while EN is set, but not the
    /// interrupt enables, nor FCR's FEIE.
    pub fn set_interrupts(&mut self, interrupts: Interrupts) {
        let st = self.regs.st(channel_index(self.index));
        st.cr().modify(|w| {
            w.set_tcie(interrupts.complete);
            w.set_htie(interrupts.half);
            w.set_teie(interrupts.error);
            w.set_dmeie(interrupts.error);
        });
        st.fcr().modify(|w| w.set_feie(interrupts.fifo_error));
    }

    /// Tells the channel to stop. It finishes the word in flight first;
    /// [`is_running`](Self::is_running) turns false once it has.
    pub fn request_stop(&mut self) {
        #[cfg(feature = "fault-injection")]
        if fault::stuck(self.controller, self.index) {
            return;
        }
        self.regs
            .st(channel_index(self.index))
            .cr()
            .modify(|w| w.set_en(false));
    }

    /// Stops the channel and waits at most `spins` reads for it to report
    /// itself stopped. Stopping a running channel sets its complete flag;
    /// that is left for the caller to clear.
    pub fn stop(&mut self, spins: u32) -> Result<(), StopTimeout> {
        self.request_stop();
        if !crate::wait::spin(spins, || !self.is_running()) {
            return Err(StopTimeout);
        }
        // Nothing the channel wrote may be read before this.
        fence(Ordering::SeqCst);
        Ok(())
    }

    /// Configures and starts a transfer.
    ///
    /// # Safety
    ///
    /// `memory` must point to `len` words of memory
    /// that stay valid until the channel has stopped, and that nothing but
    /// this channel writes in the meantime (or reads, for a
    /// peripheral-to-memory transfer). `peripheral` must be the data
    /// register of the peripheral `request` selects on this channel.
    pub(crate) unsafe fn start(&mut self, transfer: &Transfer) -> Result<(), StartError> {
        let prepared = Prepared::new(transfer)?;
        // SAFETY: the caller's contract, unchanged.
        unsafe { self.start_prepared(&prepared) }
    }

    /// Starts a transfer from register words computed beforehand
    /// ([`Prepared::new`]): the flags cleared, five stores, the enable. No
    /// arithmetic, so a driver can switch a channel in a fixed, short time,
    /// as RM0090 10.3.17 (p. 324) and RM0433 15.3.19 (p. 656) allow once EN
    /// reads 0.
    ///
    /// # Safety
    ///
    /// As [`start`](Self::start), for the transfer `prepared` was made from.
    pub(crate) unsafe fn start_prepared(&mut self, prepared: &Prepared) -> Result<(), StartError> {
        if self.is_running() {
            return Err(StartError::Busy);
        }
        let st = self.regs.st(channel_index(self.index));
        self.clear_flags(Flags::ALL);
        // No synchronisation, no event generation: the request passes
        // straight through to the channel.
        #[cfg(feature = "dma_v1")]
        self.mux
            .ccr(usize::from(self.mux_channel & 15))
            .write(|w| w.set_dmareq_id(prepared.request));
        st.par().write_value(prepared.par);
        st.m0ar().write_value(prepared.m0ar);
        st.ndtr()
            .write_value(stm32_metapac::dma::regs::Ndtr(u32::from(prepared.ndt)));
        st.fcr().write_value(prepared.fcr);
        st.cr().write_value(prepared.cr);
        // Everything the CPU wrote to the buffers lands before DMA owns them.
        fence(Ordering::SeqCst);
        let mut cr = prepared.cr;
        cr.set_en(true);
        st.cr().write_value(cr);
        Ok(())
    }
}

/// A transfer's register words, computed once by [`Prepared::new`] and
/// started any number of times by [`AnyChannel::start_prepared`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Prepared {
    #[cfg(feature = "dma_v1")]
    request: u8,
    par: u32,
    m0ar: u32,
    ndt: u16,
    fcr: stm32_metapac::dma::regs::Fcr,
    cr: stm32_metapac::dma::regs::Cr,
}

impl Prepared {
    /// The register words for `transfer`, EN clear. Refuses what `start`
    /// refuses: a length of 0 or a request out of range.
    pub(crate) fn new(transfer: &Transfer) -> Result<Self, StartError> {
        #[cfg(feature = "dma_v2")]
        let request_valid = transfer.request <= 7;
        #[cfg(feature = "dma_v1")]
        let request_valid = (1..=127).contains(&transfer.request);
        if transfer.len == 0 || !request_valid {
            return Err(StartError::Invalid);
        }
        Ok(Self::words(transfer))
    }

    /// The register words of a word-sized, memory-to-peripheral transfer
    /// at very high priority for `request` into `peripheral`, with no
    /// memory yet: a compare lane's send, prepared once at assembly so a
    /// start only sets the memory ([`with_memory`](Self::with_memory)).
    /// `request` comes from a glue trait, so it is valid by construction.
    pub(crate) fn send_words(request: u8, peripheral: u32) -> Self {
        Self::words(&Transfer::one_shot(
            request,
            Direction::MemoryToPeripheral,
            peripheral,
            0,
            1,
            WordSize::Word,
            Priority::VeryHigh,
        ))
    }

    /// These words with `len` items from `memory`. `len` must not be 0,
    /// which the caller's own check guarantees (a lane's `Frame`).
    pub(crate) const fn with_memory(mut self, memory: u32, len: u16) -> Self {
        self.m0ar = memory;
        self.ndt = len;
        self
    }

    fn words(transfer: &Transfer) -> Self {
        // Direct mode: each word goes straight through, so peripheral and
        // memory word sizes match (one `size` sets both).
        let mut fcr = stm32_metapac::dma::regs::Fcr(0);
        fcr.set_dmdis(Dmdis::ENABLED);
        fcr.set_feie(transfer.interrupts.fifo_error);
        let mut cr = stm32_metapac::dma::regs::Cr(0);
        #[cfg(feature = "dma_v2")]
        cr.set_chsel(transfer.request);
        #[cfg(feature = "dma_v1")]
        cr.set_trbuff(transfer.bufferable);
        cr.set_dir(match transfer.direction {
            Direction::PeripheralToMemory => Dir::PERIPHERAL_TO_MEMORY,
            Direction::MemoryToPeripheral => Dir::MEMORY_TO_PERIPHERAL,
        });
        cr.set_psize(transfer.size.bits());
        cr.set_msize(transfer.size.bits());
        cr.set_minc(transfer.memory_increment);
        cr.set_circ(transfer.circular);
        cr.set_pl(match transfer.priority {
            Priority::Low => Pl::LOW,
            Priority::Medium => Pl::MEDIUM,
            Priority::High => Pl::HIGH,
            Priority::VeryHigh => Pl::VERY_HIGH,
        });
        cr.set_tcie(transfer.interrupts.complete);
        cr.set_htie(transfer.interrupts.half);
        cr.set_teie(transfer.interrupts.error);
        cr.set_dmeie(transfer.interrupts.error);
        Self {
            #[cfg(feature = "dma_v1")]
            request: transfer.request,
            par: transfer.peripheral,
            m0ar: transfer.memory,
            ndt: transfer.len,
            fcr,
            cr,
        }
    }
}

/// A channel number as a register index. Channels are 0 to 7 by
/// construction; the mask lets the compiler see that too, so metapac's
/// bounds checks on `st(n)`, `isr(n)` and the flag fields fold away and no
/// panic path is left.
fn channel_index(index: u8) -> usize {
    usize::from(index & 7)
}

/// Read and clear access to one channel's flags, and its count.
///
/// Every operation is a register read or a write-one-to-clear of this
/// channel's own bits, so a handle and its `AnyChannel` can be used from
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
        let index = channel_index(self.index);
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
        self.regs.st(channel_index(self.index)).ndtr().read().ndt()
    }
}
