//! The receive engine: the circular ring, the marks where the line went
//! idle or a fault struck, and the reads that copy chunks out.
//!
//! Requirements: UART-005, UART-007, UART-008, UART-009, UART-014, UART-015, UART-DMA-002, UART-DMA-009, UART-DMA-011

use core::{
    ptr,
    sync::atomic::{Ordering, fence},
};

use super::{
    ERR_DMA_OVERRUN, ERR_DMA_TRANSFER, ERR_FRAMING, ERR_NOISE, ERR_OVERRUN, ERR_PARITY, Parts, Uart,
};
use crate::{
    dma::{AnyChannel, Direction, Flags, Interrupts, Priority, StopTimeout, Transfer, WordSize},
    uart::{
        Chunk, ChunkEnd, Error, Health,
        ring::{self, Mark, MarkKind, MarkQueue, Take},
    },
    usart_ip::Unit,
    wait::Deadline,
};

/// The fewest CPU cycles one pass of the `read_within` loop (an empty
/// `try_read`) takes, used when no [`CycleClock`](crate::time::CycleClock) is
/// set. The fastest pass measured with the DWT cycle counter (`hil`
/// `wait_bounds`), halved: H753 at 400 MHz with caches on, 757 cycles
/// (2026-11-17, since a read also serves the transmit queue and the receive
/// channel's flags; 474 before). Slower configurations wait longer than asked
/// (F401 at 84 MHz: 1002 to 1086; H753 with caches off: about 2450, so up to
/// about 6.5 times the time asked); a caller clock makes every wait exact. A
/// faster pass would make a bound run short; the HIL check `wait_bounds`
/// catches that on every board.
const RX_PASS_CYCLES: u32 = 378;

/// Reporting order: the error that restarts reception comes first.
const ERRORS: [(u8, Error); 6] = [
    (ERR_DMA_TRANSFER, Error::Dma),
    (ERR_OVERRUN, Error::Overrun),
    (ERR_FRAMING, Error::Framing),
    (ERR_PARITY, Error::Parity),
    (ERR_NOISE, Error::Noise),
    (ERR_DMA_OVERRUN, Error::Dma),
];

/// The receive channel's interrupts.
const RX_INTERRUPTS: Interrupts = Interrupts {
    complete: true,
    half: true,
    error: true,
    fifo_error: true,
};

pub(super) struct Rx {
    dma: AnyChannel,
    request: u8,
    ring: *mut u8,
    len: u16,
    /// Bytes read since the ring last (re)started, wrapping.
    read_count: u32,
    /// Where the next byte to read sits in the ring.
    read_index: u16,
    /// Gaps in the byte stream so far.
    generation: u32,
    /// The channel did not stop; reception is off for good.
    halted: bool,
    /// Half-laps of the ring served, wrapping.
    halves: u32,
    /// `halves` when the ring last (re)started.
    base: u32,
    /// The ring's run, advanced at every restart; marks carry it.
    epoch: u32,
    /// Where the line went idle and where line errors struck.
    marks: MarkQueue,
    /// Errors with no position to report them at: DMA errors, and line
    /// errors when `marks` was full.
    errors: u8,
    health: Health,
}

impl Rx {
    /// Keeps the ring's address and length only: the caller's `'static`
    /// reference ends once the UART is open, leaving the driver the ring's
    /// only user.
    pub(super) fn new(dma: AnyChannel, request: u8, ring: &mut [u8], len: u16) -> Self {
        Self {
            dma,
            request,
            ring: ring.as_mut_ptr(),
            len,
            read_count: 0,
            read_index: 0,
            generation: 0,
            halted: false,
            halves: 0,
            base: 0,
            epoch: 0,
            marks: MarkQueue::new(),
            errors: 0,
            // `start_ring` counts the first start too; `health` hides it.
            health: Health::default(),
        }
    }

    pub(super) fn buffer(&self) -> (*mut u8, usize) {
        (self.ring, usize::from(self.len))
    }

    /// Stops the receive channel and clears its flags. A channel that does not
    /// stop leaves reception halted: reads report `Halted` from then on.
    pub(super) fn halt(&mut self) -> Result<(), StopTimeout> {
        // Interrupts off first: a channel that is stopping can still set its
        // transfer-complete flag after `stop` (measured on the F407,
        // 2026-11-14), and a freed channel's vector may serve another
        // driver by then.
        self.dma.set_interrupts(Interrupts::default());
        let stopped = self.dma.stop(crate::dma::STOP_SPINS);
        self.dma.clear_flags(Flags::ALL);
        if stopped.is_err() {
            self.halted = true;
        }
        stopped
    }

    /// Starts, or restarts, the ring from its first byte. Marks of the old
    /// run are dropped.
    pub(super) fn start_ring(&mut self, unit: Unit) -> Result<(), Error> {
        if self.dma.stop(crate::dma::STOP_SPINS).is_err() {
            self.halted = true;
            return Err(Error::Halted);
        }
        self.dma.clear_flags(Flags::ALL);
        self.base = self.halves;
        self.epoch = self.epoch.wrapping_add(1);
        self.marks.clear();
        self.read_count = 0;
        self.read_index = 0;
        self.health.restarts = self.health.restarts.saturating_add(1);
        let transfer = Transfer::one_shot(
            self.request,
            Direction::PeripheralToMemory,
            unit.rx_address(),
            self.ring as u32,
            self.len,
            WordSize::Byte,
            Priority::High,
        )
        .circular()
        .with_interrupts(RX_INTERRUPTS)
        .for_usart();
        // SAFETY: the ring is this UART's for good, `len` is its length, and
        // the CPU only ever reads it. The routing trait vouched for the
        // request.
        unsafe { self.dma.start(&transfer) }.map_err(|_| Error::Halted)
    }

    /// Serves the receive channel's flags: counts half-laps and records
    /// errors. Returns whether a half filled and whether a fault was
    /// recorded.
    pub(super) fn serve_dma(&mut self) -> (bool, bool) {
        let flags = self.dma.flags();
        if !flags.any() {
            return (false, false);
        }
        self.dma.clear_flags(flags);
        // Both set means the service ran late by up to half a lap; the two
        // crossings still count once each.
        let steps = u32::from(flags.half) + u32::from(flags.complete);
        self.halves = self.halves.wrapping_add(steps);
        let mut bits = 0;
        if flags.transfer_error {
            bits |= ERR_DMA_TRANSFER;
            self.health.dma_errors = self.health.dma_errors.saturating_add(1);
        }
        if flags.direct_mode_error || flags.fifo_error {
            bits |= ERR_DMA_OVERRUN;
            self.health.dma_overruns = self.health.dma_overruns.saturating_add(1);
        }
        self.errors |= bits;
        (steps > 0, bits != 0)
    }

    /// Serves the USART: marks where the line went idle and where line
    /// errors struck, and clears them. Returns whether the line went idle
    /// and whether a fault was recorded.
    pub(super) fn serve_line(&mut self, unit: Unit) -> (bool, bool) {
        let (bits, idle) = unit.take_events();
        if bits == 0 && !idle {
            return (false, false);
        }
        let (at, _) = self.count_written();
        let epoch = self.epoch;
        let health = &mut self.health;
        for (bit, kind, count) in [
            (ERR_OVERRUN, MarkKind::Overrun, &mut health.overruns),
            (ERR_FRAMING, MarkKind::Framing, &mut health.framing_errors),
            (ERR_PARITY, MarkKind::Parity, &mut health.parity_errors),
            (ERR_NOISE, MarkKind::Noise, &mut health.noise_errors),
        ] {
            if bits & bit == 0 {
                continue;
            }
            *count = count.saturating_add(1);
            if !self.marks.push(Mark { at, kind, epoch }) {
                // No room: report it without its position rather than lose it.
                health.marks_lost = health.marks_lost.saturating_add(1);
                self.errors |= bit;
            }
        }
        // A lost idle mark only merges two chunks; nothing else is lost.
        if idle
            && !self.marks.push(Mark {
                at,
                kind: MarkKind::Idle,
                epoch,
            })
        {
            health.marks_lost = health.marks_lost.saturating_add(1);
        }
        (idle, bits != 0)
    }

    /// Bytes DMA has written since the ring (re)started, wrapping, and its
    /// position in the ring.
    ///
    /// The half-lap count is what has been served plus the half and complete
    /// flags still pending, read before the channel's remaining count. The
    /// flags are sticky, so this holds while the service is less than a
    /// full lap late (UART-DMA-002); a crossing between the flag read and the
    /// count read shows in the position, which `ring::written` accounts for.
    fn count_written(&self) -> (u32, u16) {
        let flags = self.dma.flags();
        let remaining = u32::from(self.dma.remaining());
        let pending = u32::from(flags.half) + u32::from(flags.complete);
        // Nothing read from the ring may be read before the count.
        fence(Ordering::SeqCst);
        let halves = self.halves.wrapping_sub(self.base).wrapping_add(pending);
        ring::written(halves, remaining, u32::from(self.len))
    }

    /// Where DMA has written to, after serving the channel and handling
    /// whatever stopped or lapped the ring: errors whose position was lost,
    /// a stopped channel, a ring overrun. Each of those is a gap.
    fn check_ring(&mut self, unit: Unit) -> Result<(u32, u16), Error> {
        if self.halted {
            return Err(Error::Halted);
        }
        self.serve_dma();
        if let Some(error) = self.take_error() {
            if !self.dma.is_running() {
                self.start_ring(unit)?;
            }
            self.gap();
            return Err(error);
        }
        if !self.dma.is_running() {
            // Stopped with no error flag seen: restart and report a gap.
            self.start_ring(unit)?;
            self.gap();
            return Err(Error::Dma);
        }
        let (written, index) = self.count_written();
        if written.wrapping_sub(self.read_count) > u32::from(self.len) {
            self.resync(written, index);
            self.health.buffer_overruns = self.health.buffer_overruns.saturating_add(1);
            self.gap();
            return Err(Error::BufferOverrun);
        }
        Ok((written, index))
    }

    /// Copies `buf.len()` bytes, already known to have arrived, out of the
    /// ring. `Err(BufferOverrun)` if DMA lapped the reader meanwhile.
    fn copy_out(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        match self.copy_out_exact(buf) {
            Some(len) => Ok(len),
            None => {
                self.gap();
                Err(Error::BufferOverrun)
            }
        }
    }

    /// As `copy_out`, but `None` for an overrun during the copy.
    fn copy_out_exact(&mut self, buf: &mut [u8]) -> Option<usize> {
        let len = buf.len();
        let mut index = usize::from(self.read_index);
        for byte in buf.iter_mut() {
            // SAFETY: `index` is inside the ring, which this UART owns; DMA
            // may be writing elsewhere in it, so each byte is read once,
            // volatile, after the count that said it had arrived.
            *byte = unsafe { ptr::read_volatile(self.ring.add(index)) };
            index += 1;
            if index == usize::from(self.len) {
                index = 0;
            }
        }
        // Had DMA lapped the reader while it copied, the bytes are suspect.
        self.serve_dma();
        let (written, position) = self.count_written();
        if written.wrapping_sub(self.read_count) > u32::from(self.len) {
            self.resync(written, position);
            self.health.buffer_overruns = self.health.buffer_overruns.saturating_add(1);
            return None;
        }
        self.read_count = self.read_count.wrapping_add(len as u32);
        self.read_index = index as u16;
        Some(len)
    }

    fn gap(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn gap_chunk(&mut self, error: Error) -> Chunk {
        self.gap();
        Chunk {
            len: 0,
            end: ChunkEnd::Gap(error),
            generation: self.generation,
        }
    }

    /// The oldest error recorded without a position and not yet reported.
    fn take_error(&mut self) -> Option<Error> {
        let (bit, error) = ERRORS.iter().find(|(bit, _)| self.errors & bit != 0)?;
        self.errors &= !bit;
        Some(*error)
    }

    /// Drops everything unread and carries on from DMA's position.
    fn resync(&mut self, written: u32, index: u16) {
        self.read_count = written;
        self.read_index = index;
    }

    fn try_read(&mut self, unit: Unit, buf: &mut [u8]) -> Result<usize, Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        let written = self.check_ring(unit)?;
        loop {
            let room = buf.len().min(usize::from(self.len)) as u32;
            match ring::plan(
                self.read_count,
                written.0,
                room,
                self.marks.peek(),
                self.epoch,
            ) {
                Take::Drop => self.marks.pop(),
                Take::ToMark { len: 0, kind } => {
                    self.marks.pop();
                    if let Some(error) = line_error(kind) {
                        self.gap();
                        return Err(error);
                    }
                }
                Take::ToMark { len, .. } | Take::Full { len } => {
                    // The mark, if any, stays: an error is reported once the
                    // bytes before it are out. `plan` keeps `len` within
                    // `room`; the clamp lets the optimiser see it too.
                    let len = (len as usize).min(buf.len());
                    return self.copy_out(&mut buf[..len]);
                }
                Take::Wait => {
                    let available = written.0.wrapping_sub(self.read_count).min(room);
                    let available = (available as usize).min(buf.len());
                    return self.copy_out(&mut buf[..available]);
                }
            }
        }
    }

    fn read_chunk(&mut self, unit: Unit, buf: &mut [u8]) -> Result<Option<Chunk>, Error> {
        let written = match self.check_ring(unit) {
            Ok(written) => written,
            Err(Error::Halted) => return Err(Error::Halted),
            Err(error) => {
                return Ok(Some(Chunk {
                    len: 0,
                    end: ChunkEnd::Gap(error),
                    generation: self.generation,
                }));
            }
        };
        loop {
            let room = buf.len().min(usize::from(self.len)) as u32;
            match ring::plan(
                self.read_count,
                written.0,
                room,
                self.marks.peek(),
                self.epoch,
            ) {
                Take::Drop => self.marks.pop(),
                Take::ToMark { len, kind } => {
                    // `plan` keeps `len` within `room`; the clamp lets the
                    // optimiser see it too.
                    let len = (len as usize).min(buf.len());
                    let len = self.copy_out_exact(&mut buf[..len]);
                    self.marks.pop();
                    let Some(len) = len else {
                        // Lapped during the copy: `copy_out_exact` resynced.
                        return Ok(Some(self.gap_chunk(Error::BufferOverrun)));
                    };
                    match line_error(kind) {
                        Some(error) => {
                            self.gap();
                            return Ok(Some(Chunk {
                                len,
                                end: ChunkEnd::Gap(error),
                                generation: self.generation,
                            }));
                        }
                        // An idle line right after a boundary: nothing new.
                        None if len == 0 => {}
                        None => {
                            return Ok(Some(Chunk {
                                len,
                                end: ChunkEnd::Idle,
                                generation: self.generation,
                            }));
                        }
                    }
                }
                Take::Full { len } => {
                    let len = (len as usize).min(buf.len());
                    return match self.copy_out_exact(&mut buf[..len]) {
                        Some(len) => Ok(Some(Chunk {
                            len,
                            end: ChunkEnd::Full,
                            generation: self.generation,
                        })),
                        None => Ok(Some(self.gap_chunk(Error::BufferOverrun))),
                    };
                }
                Take::Wait => return Ok(None),
            }
        }
    }
}

/// The error a mark records, if it is not an idle line.
fn line_error(kind: MarkKind) -> Option<Error> {
    match kind {
        MarkKind::Idle => None,
        MarkKind::Overrun => Some(Error::Overrun),
        MarkKind::Framing => Some(Error::Framing),
        MarkKind::Parity => Some(Error::Parity),
        MarkKind::Noise => Some(Error::Noise),
    }
}

impl<W: Parts> Uart<W> {
    /// Moves the transmit queue on, as every call does: a reader on a link
    /// whose transmit channel's vector is unbound must not leave the frames
    /// it waits for unsent (UART-DMA-008).
    fn serve_tx(&mut self) {
        if let Some(tx) = self.tx.as_mut() {
            tx.service(self.unit);
        }
    }

    /// Copies out what has arrived, up to `buf.len()` bytes, without
    /// waiting. `Ok(0)` means nothing yet.
    ///
    /// Chunk boundaries do not matter here, but errors keep their place: the
    /// bytes received before a line error are returned first, and the error
    /// on the next call.
    pub fn try_read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.serve_tx();
        self.rx.try_read(self.unit, buf)
    }

    /// The next chunk, copied into `buf`, if one is complete.
    ///
    /// A chunk ends where the line went idle, where `buf` is full, or at a
    /// gap: a line error, a ring overrun or a DMA error, each of which also
    /// advances [`generation`](Self::generation). `Ok(None)` means the bytes
    /// since the last chunk are still arriving. `Err` only for
    /// [`Error::Halted`].
    pub fn read_chunk(&mut self, buf: &mut [u8]) -> Result<Option<Chunk>, Error> {
        self.serve_tx();
        self.rx.read_chunk(self.unit, buf)
    }

    /// As [`try_read`](Self::try_read), but waits for at least one byte for
    /// as long as `frames` frames take at the configured baud rate.
    /// `Err(Timeout)` if none came.
    pub fn read_within(&mut self, buf: &mut [u8], frames: u32) -> Result<usize, Error> {
        let cycles = frames.saturating_mul(self.frame_cycles);
        let mut deadline = Deadline::new(self.clock, cycles, RX_PASS_CYCLES);
        loop {
            let len = self.try_read(buf)?;
            if len > 0 || buf.is_empty() {
                return Ok(len);
            }
            if deadline.expired() {
                break;
            }
        }
        match self.try_read(buf)? {
            0 if !buf.is_empty() => Err(Error::Timeout),
            len => Ok(len),
        }
    }

    /// Bytes waiting to be read, chunk boundaries or not.
    pub fn available(&mut self) -> Result<u16, Error> {
        self.serve_tx();
        let (written, _) = self.rx.check_ring(self.unit)?;
        Ok(written.wrapping_sub(self.rx.read_count) as u16)
    }

    /// Advances at every gap in the byte stream: each line error, ring
    /// overrun or DMA error, whether reported by `read_chunk` or by
    /// `try_read`. Two chunks with the same generation have no loss between
    /// them.
    pub fn generation(&self) -> u32 {
        self.rx.generation
    }

    /// Counts of every receive fault since the UART was opened.
    pub fn health(&self) -> Health {
        Health {
            // The first start is not a restart.
            restarts: self.rx.health.restarts.saturating_sub(1),
            ..self.rx.health
        }
    }
}
