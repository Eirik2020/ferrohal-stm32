//! The receive ring's logic, free of hardware so it can be tested on the
//! host: where DMA has written to, the queue of chunk boundaries the
//! interrupt records, and how a read splits the ring into chunks.
//!
//! Positions are byte counts since the ring last (re)started, wrapping at
//! 2^32; the ring length is even, so they stay continuous when the half-lap
//! count wraps.
//!
//! Requirements: UART-DMA-001, UART-DMA-003, UART-DMA-004, UART-DMA-005

/// Bytes DMA has written, and its index in the ring, from the half-laps
/// counted since the ring started (served by the interrupt, plus flags
/// still pending) and the channel's remaining count (NDTR).
///
/// A crossing after the count was taken shows in the position: an even
/// count with the position in the second half means the half-transfer
/// flag has since been set, an odd count with the position in the first
/// half means the complete one has.
pub(crate) fn written(halves: u32, remaining: u32, len: u32) -> (u32, u16) {
    let position = len - remaining.min(len);
    let lap = if halves % 2 == 1 && position < len / 2 {
        halves.wrapping_add(1) / 2
    } else {
        halves / 2
    };
    let count = lap.wrapping_mul(len).wrapping_add(position);
    let index = if position == len { 0 } else { position as u16 };
    (count, index)
}

/// What a mark records at its position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkKind {
    /// The line went idle: a chunk ends here.
    Idle = 0,
    Overrun = 1,
    Framing = 2,
    Parity = 3,
    Noise = 4,
}

/// A boundary in the ring: the byte count at which it was seen, and in
/// which run of the ring (`epoch`, advanced at every restart).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    pub at: u32,
    pub kind: MarkKind,
    pub epoch: u32,
}

/// Marks the queue holds. A burst can end, and errors can strike, at most
/// this many times between two reads before marks are lost.
pub(crate) const MARKS: usize = 16;

/// A first-in, first-out queue of marks: the service pushes, the reader
/// pops. Plain fields: the UART's one owner holds it (ADR-011).
pub(crate) struct MarkQueue {
    slots: [Mark; MARKS],
    head: u32,
    tail: u32,
}

impl MarkQueue {
    pub const fn new() -> Self {
        Self {
            slots: [Mark {
                at: 0,
                kind: MarkKind::Idle,
                epoch: 0,
            }; MARKS],
            head: 0,
            tail: 0,
        }
    }

    /// `false` if the queue is full and the mark was dropped.
    pub fn push(&mut self, mark: Mark) -> bool {
        if self.head.wrapping_sub(self.tail) as usize >= MARKS {
            return false;
        }
        if let Some(slot) = self.slots.get_mut(self.head as usize % MARKS) {
            *slot = mark;
        }
        self.head = self.head.wrapping_add(1);
        true
    }

    /// The oldest mark, left in place.
    pub fn peek(&self) -> Option<Mark> {
        if self.head == self.tail {
            return None;
        }
        self.slots.get(self.tail as usize % MARKS).copied()
    }

    /// Drops the oldest mark.
    pub fn pop(&mut self) {
        if self.head != self.tail {
            self.tail = self.tail.wrapping_add(1);
        }
    }

    /// Drops every mark.
    pub fn clear(&mut self) {
        self.tail = self.head;
    }
}

/// What the next read takes out of the ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Take {
    /// Nothing complete yet: the bytes so far are not followed by a mark,
    /// and do not fill the caller's buffer.
    Wait,
    /// `len` bytes that end at the mark at the head of the queue, which the
    /// read then consumes.
    ToMark { len: u32, kind: MarkKind },
    /// `len` bytes, the caller's whole buffer, with no mark among them.
    Full { len: u32 },
    /// The mark at the head of the queue is stale (an earlier run of the
    /// ring) or lies behind the read position: drop it and look again.
    Drop,
}

/// How a read of at most `room` bytes splits the ring, given where it has
/// read to, where DMA has written to, and the oldest mark.
pub(crate) fn plan(read: u32, written: u32, room: u32, mark: Option<Mark>, epoch: u32) -> Take {
    let available = written.wrapping_sub(read);
    if let Some(mark) = mark {
        if mark.epoch != epoch {
            return Take::Drop;
        }
        let len = mark.at.wrapping_sub(read);
        // A mark behind the read position wraps to a distance past half the
        // count's range; drop it.
        if len > u32::MAX / 2 {
            return Take::Drop;
        }
        if len <= available {
            if len <= room {
                return Take::ToMark {
                    len,
                    kind: mark.kind,
                };
            }
            return Take::Full { len: room };
        }
        // Ahead of where DMA had written when the caller looked: recorded
        // since. Keep it; read as if no mark had come yet.
    }
    if room > 0 && available >= room {
        Take::Full { len: room }
    } else {
        Take::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEN: u32 = 256;

    #[test]
    fn written_follows_the_dma_across_laps() {
        // Start of the ring.
        assert_eq!(written(0, LEN, LEN), (0, 0));
        // 100 bytes in, no half crossed.
        assert_eq!(written(0, LEN - 100, LEN), (100, 100));
        // Half crossed and counted.
        assert_eq!(written(1, LEN - 200, LEN), (200, 200));
        // Second lap, 10 bytes in.
        assert_eq!(written(2, LEN - 10, LEN), (266, 10));
    }

    #[test]
    fn written_accounts_for_a_crossing_not_yet_counted() {
        // Even count, position in the second half: half-transfer pending.
        assert_eq!(written(0, LEN - 150, LEN), (150, 150));
        // Odd count, position back in the first half: complete pending.
        assert_eq!(written(1, LEN - 20, LEN), (276, 20));
    }

    #[test]
    fn written_stays_continuous_when_the_count_wraps() {
        let before = written(u32::MAX - 1, LEN - 10, LEN).0;
        let after = written(u32::MAX, LEN - 200, LEN).0;
        assert_eq!(after.wrapping_sub(before), 190);
        let wrapped = written(0, LEN - 10, LEN).0;
        assert_eq!(wrapped.wrapping_sub(after), LEN - 200 + 10);
    }

    fn mark(at: u32, kind: MarkKind) -> Mark {
        Mark { at, kind, epoch: 1 }
    }

    #[test]
    fn queue_is_first_in_first_out_and_bounded() {
        let mut queue = MarkQueue::new();
        assert_eq!(queue.peek(), None);
        for i in 0..MARKS as u32 {
            assert!(queue.push(mark(i, MarkKind::Idle)));
        }
        assert!(
            !queue.push(mark(99, MarkKind::Framing)),
            "full queue takes no more"
        );
        for i in 0..MARKS as u32 {
            assert_eq!(queue.peek(), Some(mark(i, MarkKind::Idle)));
            queue.pop();
        }
        assert_eq!(queue.peek(), None);
        queue.pop();
        assert_eq!(queue.peek(), None, "popping an empty queue is harmless");
    }

    #[test]
    fn queue_keeps_kind_and_epoch() {
        let mut queue = MarkQueue::new();
        let m = Mark {
            at: 0xdead_beef,
            kind: MarkKind::Noise,
            epoch: 0x00ab_cdef,
        };
        assert!(queue.push(m));
        assert_eq!(queue.peek(), Some(m));
        queue.clear();
        assert_eq!(queue.peek(), None);
    }

    #[test]
    fn plan_waits_for_an_open_chunk() {
        assert_eq!(plan(0, 10, 64, None, 1), Take::Wait);
        assert_eq!(plan(0, 0, 64, None, 1), Take::Wait);
    }

    #[test]
    fn plan_takes_up_to_the_mark() {
        let m = mark(25, MarkKind::Idle);
        assert_eq!(
            plan(0, 30, 64, Some(m), 1),
            Take::ToMark {
                len: 25,
                kind: MarkKind::Idle
            }
        );
        // An error mark ends the chunk the same way.
        let e = mark(5, MarkKind::Framing);
        assert_eq!(
            plan(0, 30, 64, Some(e), 1),
            Take::ToMark {
                len: 5,
                kind: MarkKind::Framing
            }
        );
        // An empty chunk: the mark sits at the read position.
        assert_eq!(
            plan(25, 30, 64, Some(m), 1),
            Take::ToMark {
                len: 0,
                kind: MarkKind::Idle
            }
        );
    }

    #[test]
    fn plan_fills_the_buffer_before_a_distant_mark() {
        let m = mark(100, MarkKind::Idle);
        assert_eq!(plan(0, 120, 64, Some(m), 1), Take::Full { len: 64 });
        // No mark at all, but enough bytes to fill the buffer.
        assert_eq!(plan(0, 64, 64, None, 1), Take::Full { len: 64 });
    }

    #[test]
    fn plan_drops_stale_and_passed_marks() {
        let stale = Mark {
            at: 10,
            kind: MarkKind::Idle,
            epoch: 0,
        };
        assert_eq!(plan(0, 30, 64, Some(stale), 1), Take::Drop);
        // Behind the read position (after a resync).
        let behind = mark(5, MarkKind::Idle);
        assert_eq!(plan(20, 30, 64, Some(behind), 1), Take::Drop);
    }

    #[test]
    fn plan_keeps_a_mark_recorded_after_the_write_count_was_read() {
        // DMA was at 30 when the reader looked; the interrupt has since
        // marked 50. The mark stays for the next read.
        let ahead = mark(50, MarkKind::Idle);
        assert_eq!(plan(0, 30, 64, Some(ahead), 1), Take::Wait);
        assert_eq!(plan(0, 30, 16, Some(ahead), 1), Take::Full { len: 16 });
    }

    #[test]
    fn plan_works_across_the_count_wrapping() {
        let read = u32::MAX - 5;
        let m = mark(read.wrapping_add(20), MarkKind::Idle);
        assert_eq!(
            plan(read, read.wrapping_add(30), 64, Some(m), 1),
            Take::ToMark {
                len: 20,
                kind: MarkKind::Idle
            }
        );
    }
}
