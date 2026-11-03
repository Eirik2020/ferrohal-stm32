//! The transmit ring's logic, free of hardware so it can be tested on the
//! host: where the next frame fits, and the queue of frames waiting or in
//! flight.
//!
//! Every frame is contiguous in the ring, so it goes out as one DMA
//! transfer. A frame that does not fit before the ring's end goes to its
//! start, leaving the tail unused until the frames before it are gone.
//!
//! Requirements: UART-011, UARTDMA-006, UARTDMA-007

use core::sync::atomic::{AtomicU32, Ordering};

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

/// A single-producer, single-consumer queue of frames: the writer pushes,
/// the service that starts transfers (task or interrupt, one at a time)
/// pops. Atomics only.
pub(crate) struct FrameQueue {
    slots: [AtomicU32; FRAMES],
    head: AtomicU32,
    tail: AtomicU32,
}

impl FrameQueue {
    pub const fn new() -> Self {
        Self {
            slots: [const { AtomicU32::new(0) }; FRAMES],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    /// Producer side. `false` if the queue is full.
    pub fn push(&self, frame: Frame) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        if head.wrapping_sub(self.tail.load(Ordering::Acquire)) as usize >= FRAMES {
            return false;
        }
        self.slots[head as usize % FRAMES].store(
            (u32::from(frame.offset) << 16) | u32::from(frame.len),
            Ordering::Relaxed,
        );
        self.head.store(head.wrapping_add(1), Ordering::Release);
        true
    }

    /// Either side: the oldest frame, left in place.
    pub fn peek(&self) -> Option<Frame> {
        let tail = self.tail.load(Ordering::Acquire);
        if self.head.load(Ordering::Acquire) == tail {
            return None;
        }
        let packed = self.slots[tail as usize % FRAMES].load(Ordering::Relaxed);
        Some(Frame {
            offset: (packed >> 16) as u16,
            len: packed as u16,
        })
    }

    /// Consumer side: drops the oldest frame.
    pub fn pop(&self) {
        let tail = self.tail.load(Ordering::Relaxed);
        if self.head.load(Ordering::Acquire) != tail {
            self.tail.store(tail.wrapping_add(1), Ordering::Release);
        }
    }

    /// Frames queued, in flight included.
    pub fn len(&self) -> usize {
        self.head
            .load(Ordering::Acquire)
            .wrapping_sub(self.tail.load(Ordering::Acquire)) as usize
    }

    /// Consumer side: drops every frame.
    pub fn clear(&self) {
        let head = self.head.load(Ordering::Acquire);
        self.tail.store(head, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: u16 = 64;

    #[test]
    fn an_empty_ring_takes_a_frame_at_its_start() {
        assert_eq!(place(40, None, CAP, 10), Some(0));
        assert_eq!(place(0, None, CAP, CAP), Some(0), "a frame may fill the ring");
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
        for (write, oldest) in [(0, None), (20, Some(0)), (60, Some(30)), (10, Some(30)), (64, Some(1))] {
            let most = largest(write, oldest, CAP);
            if most > 0 {
                assert!(place(write, oldest, CAP, most).is_some(), "{write} {oldest:?}: {most} fits");
            }
            assert!(place(write, oldest, CAP, most + 1).is_none(), "{write} {oldest:?}: {} does not", most + 1);
        }
    }

    #[test]
    fn queue_is_first_in_first_out_and_bounded() {
        let queue = FrameQueue::new();
        assert_eq!(queue.peek(), None);
        for i in 0..FRAMES as u16 {
            assert!(queue.push(Frame { offset: i * 8, len: 8 }));
        }
        assert!(!queue.push(Frame { offset: 0, len: 1 }));
        assert_eq!(queue.len(), FRAMES);
        for i in 0..FRAMES as u16 {
            assert_eq!(queue.peek(), Some(Frame { offset: i * 8, len: 8 }));
            queue.pop();
        }
        assert_eq!(queue.len(), 0);
        queue.pop();
        assert_eq!(queue.peek(), None);
    }

    #[test]
    fn queue_keeps_offset_and_length() {
        let queue = FrameQueue::new();
        let frame = Frame { offset: 0xfffe, len: 0xabcd };
        assert!(queue.push(frame));
        assert_eq!(queue.peek(), Some(frame));
        queue.clear();
        assert_eq!(queue.peek(), None);
    }
}
