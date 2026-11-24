//! The transmit ring's logic, free of hardware so it can be tested on the
//! host: where the next frame fits, and the queue of frames waiting or in
//! flight.
//!
//! Every frame is contiguous in the ring, so it goes out as one DMA
//! transfer. A frame that does not fit before the ring's end goes to its
//! start, leaving the tail unused until the frames before it are gone.
//!
//! The outcome queue gives each finished frame's outcome back, in the order
//! the frames were queued.
//!
//! Requirements: UART-DMA-006, UART-DMA-007, UART-DMA-010, UART-DMA-015

/// Where a frame of `len` bytes goes, given where the last one ended
/// (`write`) and where the oldest queued one starts (`oldest`, `None` with
/// the queue empty). `None` if it does not fit now.
///
/// The free space is the gap between `write` and `oldest`. A placement that
/// would make `write` reach `oldest` exactly is refused, so a full ring is
/// never mistaken for an empty one.
pub(crate) fn place(write: u16, oldest: Option<u16>, cap: u16, len: u16) -> Option<u16> {
    if len == 0 || len > cap {
        return None;
    }
    let Some(oldest) = oldest else {
        // Nothing queued: the whole ring is free.
        return Some(0);
    };
    if write >= oldest {
        // Used from `oldest` to `write`; free after `write` and before
        // `oldest`.
        if cap - write >= len {
            Some(write)
        } else if oldest > len {
            Some(0)
        } else {
            None
        }
    } else if oldest - write > len {
        // Wrapped: free between `write` and `oldest`.
        Some(write)
    } else {
        None
    }
}

/// The longest frame `place` would accept now.
pub(crate) fn largest(write: u16, oldest: Option<u16>, cap: u16) -> u16 {
    match oldest {
        None => cap,
        Some(oldest) if write >= oldest => (cap - write).max(oldest.saturating_sub(1)),
        Some(oldest) => oldest - write - 1,
    }
}

/// Frames the queue holds: sent, in flight, or waiting.
pub(crate) const FRAMES: usize = 8;

/// A frame's place in the ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    pub offset: u16,
    pub len: u16,
}

/// A first-in, first-out queue of frames: the writer pushes, the service
/// that finishes transfers pops. Plain fields: the UART's one owner holds it
/// (ADR-011).
pub(crate) struct FrameQueue {
    slots: [Frame; FRAMES],
    head: u32,
    tail: u32,
}

impl FrameQueue {
    pub const fn new() -> Self {
        Self {
            slots: [Frame { offset: 0, len: 0 }; FRAMES],
            head: 0,
            tail: 0,
        }
    }

    /// `false` if the queue is full.
    pub fn push(&mut self, frame: Frame) -> bool {
        if self.len() >= FRAMES {
            return false;
        }
        if let Some(slot) = self.slots.get_mut(self.head as usize % FRAMES) {
            *slot = frame;
        }
        self.head = self.head.wrapping_add(1);
        true
    }

    /// The oldest frame, left in place.
    pub fn peek(&self) -> Option<Frame> {
        if self.head == self.tail {
            return None;
        }
        self.slots.get(self.tail as usize % FRAMES).copied()
    }

    /// Drops the oldest frame.
    pub fn pop(&mut self) {
        if self.head != self.tail {
            self.tail = self.tail.wrapping_add(1);
        }
    }

    /// Frames queued, in flight included.
    pub fn len(&self) -> usize {
        self.head.wrapping_sub(self.tail) as usize
    }

    /// Drops every frame.
    pub fn clear(&mut self) {
        self.tail = self.head;
    }
}

/// What became of one transmit frame, given back by
/// [`Uart::take_outcome`](crate::uart::Uart::take_outcome) in the order
/// the frames were queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TxOutcome {
    /// The transmit DMA finished the frame: every byte is in the USART. The
    /// line goes quiet up to two characters later, which
    /// [`Events::tx_idle`](crate::uart::Events::tx_idle) reports.
    Sent,
    /// A DMA error ended the frame; part of it may have gone out.
    Failed,
    /// The queue was dropped, by a timeout or a channel that would not run,
    /// before the frame finished; part of it may have gone out.
    Dropped,
    /// This many older outcomes were overwritten before they were taken,
    /// in their place: the frames they stood for are the oldest not yet
    /// accounted for.
    Lost(u32),
}

/// The outcomes of finished frames, oldest first, waiting to be taken. It
/// holds [`FRAMES`]; a further outcome overwrites the oldest and counts it
/// lost, so transmission never waits on a caller that does not take them,
/// and the loss is reported in its place.
pub(crate) struct Outcomes {
    slots: [TxOutcome; FRAMES],
    head: u32,
    tail: u32,
    lost: u32,
}

impl Outcomes {
    pub const fn new() -> Self {
        Self {
            slots: [TxOutcome::Sent; FRAMES],
            head: 0,
            tail: 0,
            lost: 0,
        }
    }

    pub fn push(&mut self, outcome: TxOutcome) {
        if self.head.wrapping_sub(self.tail) as usize >= FRAMES {
            self.tail = self.tail.wrapping_add(1);
            self.lost = self.lost.saturating_add(1);
        }
        if let Some(slot) = self.slots.get_mut(self.head as usize % FRAMES) {
            *slot = outcome;
        }
        self.head = self.head.wrapping_add(1);
    }

    /// The oldest outcome not yet taken: a loss first, since the lost
    /// outcomes are older than every one still held.
    pub fn take(&mut self) -> Option<TxOutcome> {
        if self.lost != 0 {
            let lost = self.lost;
            self.lost = 0;
            return Some(TxOutcome::Lost(lost));
        }
        if self.head == self.tail {
            return None;
        }
        let outcome = self.slots.get(self.tail as usize % FRAMES).copied();
        self.tail = self.tail.wrapping_add(1);
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: u16 = 64;

    #[test]
    fn an_empty_ring_takes_a_frame_at_its_start() {
        assert_eq!(place(40, None, CAP, 10), Some(0));
        assert_eq!(
            place(0, None, CAP, CAP),
            Some(0),
            "a frame may fill the ring"
        );
    }

    #[test]
    fn refuses_empty_and_oversized_frames() {
        assert_eq!(place(0, None, CAP, 0), None);
        assert_eq!(place(0, None, CAP, CAP + 1), None);
    }

    #[test]
    fn frames_follow_each_other() {
        // One frame at 0..20 queued: the next goes after it.
        assert_eq!(place(20, Some(0), CAP, 30), Some(20));
        assert_eq!(place(20, Some(0), CAP, 44), Some(20), "exactly to the end");
        assert_eq!(place(20, Some(0), CAP, 45), None, "nothing free before 0");
    }

    #[test]
    fn a_frame_too_long_for_the_tail_wraps_to_the_start() {
        // Frames at 30..54 queued: 10 free at the end, 30 at the start.
        assert_eq!(place(54, Some(30), CAP, 10), Some(54));
        assert_eq!(place(54, Some(30), CAP, 20), Some(0));
        // Reaching `oldest` exactly is refused.
        assert_eq!(place(54, Some(30), CAP, 30), None);
    }

    #[test]
    fn a_wrapped_ring_fills_up_to_the_oldest_frame() {
        // Wrapped: frames at 30..60 and 0..10; free 10..30.
        assert_eq!(place(10, Some(30), CAP, 19), Some(10));
        assert_eq!(place(10, Some(30), CAP, 20), None, "would meet the oldest");
    }

    #[test]
    fn largest_agrees_with_place() {
        for (write, oldest) in [
            (0, None),
            (20, Some(0)),
            (60, Some(30)),
            (10, Some(30)),
            (64, Some(1)),
        ] {
            let most = largest(write, oldest, CAP);
            if most > 0 {
                assert!(
                    place(write, oldest, CAP, most).is_some(),
                    "{write} {oldest:?}: {most} fits"
                );
            }
            assert!(
                place(write, oldest, CAP, most + 1).is_none(),
                "{write} {oldest:?}: {} does not",
                most + 1
            );
        }
    }

    #[test]
    fn queue_is_first_in_first_out_and_bounded() {
        let mut queue = FrameQueue::new();
        assert_eq!(queue.peek(), None);
        for i in 0..FRAMES as u16 {
            assert!(queue.push(Frame {
                offset: i * 8,
                len: 8
            }));
        }
        assert!(!queue.push(Frame { offset: 0, len: 1 }));
        assert_eq!(queue.len(), FRAMES);
        for i in 0..FRAMES as u16 {
            assert_eq!(
                queue.peek(),
                Some(Frame {
                    offset: i * 8,
                    len: 8
                })
            );
            queue.pop();
        }
        assert_eq!(queue.len(), 0);
        queue.pop();
        assert_eq!(queue.peek(), None);
    }

    #[test]
    fn queue_keeps_offset_and_length() {
        let mut queue = FrameQueue::new();
        let frame = Frame {
            offset: 0xfffe,
            len: 0xabcd,
        };
        assert!(queue.push(frame));
        assert_eq!(queue.peek(), Some(frame));
        queue.clear();
        assert_eq!(queue.peek(), None);
    }

    #[test]
    fn outcomes_come_back_once_in_order() {
        let mut outcomes = Outcomes::new();
        assert_eq!(outcomes.take(), None);
        outcomes.push(TxOutcome::Sent);
        outcomes.push(TxOutcome::Failed);
        outcomes.push(TxOutcome::Dropped);
        assert_eq!(outcomes.take(), Some(TxOutcome::Sent));
        assert_eq!(outcomes.take(), Some(TxOutcome::Failed));
        assert_eq!(outcomes.take(), Some(TxOutcome::Dropped));
        assert_eq!(outcomes.take(), None, "each is given back once");
    }

    #[test]
    fn an_overwritten_outcome_is_counted_in_its_place() {
        let mut outcomes = Outcomes::new();
        // Three more than it holds: the three oldest are overwritten.
        for i in 0..FRAMES + 3 {
            outcomes.push(if i % 2 == 0 {
                TxOutcome::Sent
            } else {
                TxOutcome::Failed
            });
        }
        assert_eq!(outcomes.take(), Some(TxOutcome::Lost(3)));
        for i in 3..FRAMES + 3 {
            let expected = if i % 2 == 0 {
                TxOutcome::Sent
            } else {
                TxOutcome::Failed
            };
            assert_eq!(outcomes.take(), Some(expected), "outcome {i}");
        }
        assert_eq!(outcomes.take(), None);
    }

    #[test]
    fn the_exported_depth_is_the_queue_s() {
        assert_eq!(crate::uart::TX_FRAMES, FRAMES);
        let mut queue = FrameQueue::new();
        for _ in 0..crate::uart::TX_FRAMES {
            assert!(queue.push(Frame { offset: 0, len: 1 }));
        }
        assert!(!queue.push(Frame { offset: 0, len: 1 }));
    }
}
