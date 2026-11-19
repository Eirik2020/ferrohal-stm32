//! The transmit engine: a queue of whole frames in the transmit buffer, one
//! DMA transfer per frame, chained by the service.
//!
//! Every queued frame's outcome is kept for [`Uart::take_outcome`], and the
//! USART's transmission complete after the last frame is reported once as
//! [`Events::tx_idle`](super::Events::tx_idle).
//!
//! Requirements: UART-006, UART-010, UART-011, UART-013, UARTDMA-008, UARTDMA-009, UARTDMA-012, UARTDMA-013, UART-023

use core::{
    ptr,
    sync::atomic::{Ordering, fence},
};

use super::{ERR_DMA_OVERRUN, ERR_DMA_TRANSFER, REGISTER_PASS_CYCLES, Uart};
use crate::{
    dma::{AnyStream, Direction, Flags, Interrupts, Priority, StopTimeout, Transfer, WordSize},
    uart::{
        Error, SendError, TxHealth, Wiring,
        stream_dma::{Instance, RxDma, RxPin, TxDma, TxPin},
        tx_ring::{self, FRAMES, Frame, FrameQueue, Outcomes, TxOutcome},
    },
    usart_ip::Unit,
    wait::{Deadline, wait},
};

/// The fewest CPU cycles one pass of the transmit wait (a busy service)
/// takes, used when no [`CycleClock`](crate::time::CycleClock) is set: the
/// fastest pass measured with the DWT cycle counter (`hil` `wait_bounds`),
/// halved: H753 at 400 MHz with caches on, 108 cycles. Slower
/// configurations wait longer than asked (F401 at 84 MHz: 184; H753 with
/// caches off: about 390).
const TX_PASS_CYCLES: u32 = 48;

pub(super) struct Tx {
    dma: AnyStream,
    request: u8,
    buf: *mut u8,
    cap: u16,
    frames: FrameQueue,
    /// Where the last queued frame ends.
    write: u16,
    /// The frame at the queue's head is in flight.
    running: bool,
    /// The stream did not stop or start; transmission is off for good.
    halted: bool,
    errors: u8,
    health: TxHealth,
    /// Finished frames' outcomes, for `take_outcome`.
    outcomes: Outcomes,
    /// The queue emptied and the USART's transmission complete is awaited:
    /// TCIE is on (unless masked) and every service polls TC.
    draining: bool,
    /// TC came after the last frame and is not yet reported.
    idle: bool,
    /// The queue is empty and TC has come since its last frame.
    drained: bool,
    /// The line's interrupts are masked (`Uart::mask`): TCIE stays off.
    masked: bool,
    /// The caller asked for TC to raise the USART's vector
    /// (`set_tx_idle_interrupt`); without it a drain is only polled.
    notify: bool,
}

impl Tx {
    pub(super) fn new(dma: AnyStream, request: u8, buf: &mut [u8], cap: u16) -> Self {
        Self {
            dma,
            request,
            buf: buf.as_mut_ptr(),
            cap,
            frames: FrameQueue::new(),
            write: 0,
            running: false,
            halted: false,
            errors: 0,
            health: TxHealth::default(),
            outcomes: Outcomes::new(),
            draining: false,
            idle: false,
            drained: true,
            masked: false,
            notify: false,
        }
    }

    pub(super) fn buffer(&self) -> (*mut u8, usize) {
        (self.buf, usize::from(self.cap))
    }

    /// Stops a stream an earlier owner left running, and clears its flags.
    pub(super) fn reset_stream(&mut self) -> Result<(), StopTimeout> {
        self.dma.stop(crate::dma::STOP_SPINS)?;
        self.dma.clear_flags(Flags::ALL);
        Ok(())
    }

    /// Stops the transmit stream, drops the queue, and marks transmission
    /// off for good: what `free` runs.
    pub(super) fn halt(&mut self) -> Result<(), StopTimeout> {
        self.halted = true;
        self.draining = false;
        self.dma.set_interrupts(Interrupts::default());
        let stopped = self.dma.stop(crate::dma::STOP_SPINS);
        self.dma.clear_flags(Flags::ALL);
        self.drop_queue();
        self.running = false;
        stopped
    }

    /// Drops every queued frame, each with the outcome `Dropped`.
    fn drop_queue(&mut self) {
        for _ in 0..self.frames.len() {
            self.outcomes.push(TxOutcome::Dropped);
        }
        self.frames.clear();
    }

    /// Waits for the USART's transmission complete: the queue just emptied.
    fn arm_drain(&mut self, unit: Unit) {
        self.draining = true;
        unit.set_tc_interrupt(self.notify && !self.masked);
    }

    /// Reports TC once after the last frame: polled by every service, and
    /// raised on the USART's vector by TCIE while armed.
    fn serve_drain(&mut self, unit: Unit) {
        if self.draining && unit.tc() {
            self.draining = false;
            self.idle = true;
            self.drained = true;
            unit.set_tc_interrupt(false);
        }
    }

    /// Whether TC came after the last frame since the last call.
    pub(super) fn take_idle(&mut self) -> bool {
        core::mem::take(&mut self.idle)
    }

    /// Holds TCIE off while the line is masked, and restores it.
    pub(super) fn set_masked(&mut self, unit: Unit, masked: bool) {
        self.masked = masked;
        unit.set_tc_interrupt(self.draining && self.notify && !masked);
    }

    /// Whether TC raises the USART's vector while a drain is awaited.
    pub(super) fn set_notify(&mut self, unit: Unit, on: bool) {
        self.notify = on;
        unit.set_tc_interrupt(self.draining && on && !self.masked);
    }

    /// The oldest outcome not yet taken.
    pub(super) fn take_outcome(&mut self) -> Option<TxOutcome> {
        self.outcomes.take()
    }

    /// Finishes the frame in flight if it is done, and starts the next.
    /// Returns whether a frame finished (or failed), and whether it failed.
    pub(super) fn service(&mut self, unit: Unit) -> (bool, bool) {
        let mut finished = (false, false);
        if self.running {
            let flags = self.dma.flags();
            if flags.failed() {
                if self.dma.stop(crate::dma::STOP_SPINS).is_err() {
                    // As in `time_out`: no vector for a stream left running.
                    self.dma.set_interrupts(Interrupts::default());
                    self.halted = true;
                }
                self.dma.clear_flags(Flags::ALL);
                self.frames.pop();
                self.outcomes.push(TxOutcome::Failed);
                self.running = false;
                self.health.dma_errors = self.health.dma_errors.saturating_add(1);
                self.errors |= if flags.transfer_error {
                    ERR_DMA_TRANSFER
                } else {
                    ERR_DMA_OVERRUN
                };
                finished = (true, true);
            } else if flags.complete && !self.dma.is_running() {
                self.dma.clear_flags(Flags::ALL);
                self.frames.pop();
                self.outcomes.push(TxOutcome::Sent);
                self.running = false;
                self.health.sent = self.health.sent.saturating_add(1);
                finished = (true, false);
            } else {
                if flags.fifo_error {
                    self.dma.clear_flags(Flags {
                        fifo_error: true,
                        ..Flags::default()
                    });
                }
                return finished;
            }
        }
        if self.halted {
            // A halted transmitter sends nothing more: its frames are
            // dropped, each with its outcome.
            self.drop_queue();
            return finished;
        }
        let Some(frame) = self.frames.peek() else {
            if finished.0 {
                self.arm_drain(unit);
            }
            self.serve_drain(unit);
            return finished;
        };
        // A new frame: the drain before it is moot, and its own end arms
        // the next.
        if self.draining {
            self.draining = false;
            unit.set_tc_interrupt(false);
        }
        self.drained = false;
        unit.clear_tc();
        let transfer = Transfer::one_shot(
            self.request,
            Direction::MemoryToPeripheral,
            unit.tx_address(),
            // The frame lies inside the buffer.
            self.buf.wrapping_add(usize::from(frame.offset)) as u32,
            frame.len,
            WordSize::Byte,
            Priority::Medium,
        )
        .for_usart();
        // SAFETY: the frame's bytes are in the transmit buffer, written before
        // the frame was queued and untouched until it is popped, after the
        // stream has finished with it. The routing trait vouched for the
        // request.
        match unsafe { self.dma.start(&transfer) } {
            Ok(()) => self.running = true,
            // Still enabled: a stop that never took. Fail closed, its
            // interrupts off as in `time_out`.
            Err(_) => {
                self.dma.set_interrupts(Interrupts::default());
                self.halted = true;
                self.drop_queue();
            }
        }
        finished
    }

    /// The oldest transmit error not yet reported.
    fn take_error(&mut self) -> Result<(), Error> {
        if self.halted {
            return Err(Error::Halted);
        }
        if self.errors != 0 {
            self.errors = 0;
            return Err(Error::Dma);
        }
        Ok(())
    }

    /// The longest frame that fits now.
    fn room(&self) -> u16 {
        if self.frames.len() >= FRAMES {
            return 0;
        }
        let oldest = self.frames.peek().map(|frame| frame.offset);
        tx_ring::largest(self.write, oldest, self.cap)
    }

    /// A frame did not finish in time: stop the stream, drop the queue.
    fn time_out<T>(&mut self) -> Result<T, Error> {
        self.health.timeouts = self.health.timeouts.saturating_add(1);
        // Interrupts off first: a stream that does not stop finishes its
        // frame later and sets its complete flag, which nothing here will
        // clear once transmission is halted, so its vector would fire
        // without end. The next transfer's start enables them again.
        self.dma.set_interrupts(Interrupts::default());
        let stopped = self.dma.stop(crate::dma::STOP_SPINS).is_ok();
        self.dma.clear_flags(Flags::ALL);
        self.drop_queue();
        self.running = false;
        if stopped {
            Err(Error::Timeout)
        } else {
            self.halted = true;
            Err(Error::Halted)
        }
    }
}

impl<U, TxP, RxP, TxD, RxD> Uart<Wiring<U, TxP, RxP, TxD, RxD>>
where
    U: Instance,
    TxP: TxPin<U>,
    RxP: RxPin<U>,
    TxD: TxDma<U>,
    RxD: RxDma<U>,
{
    /// The transmit engine. A full UART always has one; `None` never
    /// happens, and reads as a halted transmitter.
    fn tx(&mut self) -> Result<&mut Tx, Error> {
        self.tx.as_mut().ok_or(Error::Halted)
    }

    /// Queues `frame` whole, or not at all, and starts it if the line is
    /// free. Frames go out in order, back to back.
    ///
    /// With the transmit stream's vector bound to
    /// [`on_interrupt`](Uart::on_interrupt), each finished frame starts the
    /// next from the interrupt. Without it, queued frames move on whenever
    /// the UART is served or called: `try_send`, `write`, `flush` or
    /// `on_interrupt` from a task.
    pub fn try_send(&mut self, frame: &[u8]) -> Result<(), SendError> {
        let unit = self.unit;
        let Ok(tx) = self.tx() else {
            return Err(SendError::Halted);
        };
        if tx.halted {
            return Err(SendError::Halted);
        }
        if frame.is_empty() {
            return Ok(());
        }
        let len = u16::try_from(frame.len())
            .ok()
            .filter(|&len| len <= tx.cap)
            .ok_or(SendError::TooLarge)?;
        tx.service(unit);
        if tx.frames.len() >= FRAMES {
            return Err(SendError::Full);
        }
        let oldest = tx.frames.peek().map(|frame| frame.offset);
        let offset = tx_ring::place(tx.write, oldest, tx.cap, len).ok_or(SendError::Full)?;
        // SAFETY: `place` found `offset..offset + len` inside the buffer and
        // outside every queued frame, so DMA does not read it until the
        // frame is pushed below.
        unsafe {
            ptr::copy_nonoverlapping(frame.as_ptr(), tx.buf.add(usize::from(offset)), frame.len())
        };
        // The bytes land before the stream that sends them starts.
        fence(Ordering::SeqCst);
        tx.frames.push(Frame { offset, len });
        tx.write = offset + len;
        tx.service(unit);
        Ok(())
    }

    /// Starts sending as much of `data` as fits as one frame, waiting first
    /// for room. Returns how many bytes it took.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        let unit = self.unit;
        let cap = self.tx()?.cap;
        let budget = self.budget(cap);
        let mut deadline = Deadline::new(self.clock, budget, TX_PASS_CYCLES);
        let tx = self.tx()?;
        tx.take_error()?;
        if data.is_empty() {
            return Ok(0);
        }
        loop {
            let len = data.len().min(usize::from(self.tx()?.room()));
            if len > 0 {
                return match self.try_send(&data[..len]) {
                    Ok(()) => Ok(len),
                    // Room was just measured, and only this call fills it.
                    Err(_) => Err(Error::Halted),
                };
            }
            let tx = self.tx()?;
            tx.service(unit);
            tx.take_error()?;
            if deadline.expired() {
                return tx.time_out();
            }
        }
    }

    /// Waits until every queued frame has left the wire.
    pub fn flush(&mut self) -> Result<(), Error> {
        let unit = self.unit;
        let cap = self.tx()?.cap;
        let budget = self.budget(cap);
        let mut deadline = Deadline::new(self.clock, budget, TX_PASS_CYCLES);
        loop {
            let tx = self.tx()?;
            tx.service(unit);
            tx.take_error()?;
            if tx.frames.len() == 0 {
                break;
            }
            if deadline.expired() {
                return tx.time_out();
            }
        }
        let deadline = Deadline::new(self.clock, self.budget(1), REGISTER_PASS_CYCLES);
        if wait(deadline, || unit.tc()) {
            Ok(())
        } else {
            Err(Error::Timeout)
        }
    }

    /// Whether every queued frame has gone.
    pub fn is_idle(&mut self) -> Result<bool, Error> {
        let unit = self.unit;
        let tx = self.tx()?;
        tx.service(unit);
        tx.take_error()?;
        Ok(tx.frames.len() == 0)
    }

    /// Whether a frame of at least one byte fits now.
    pub fn write_ready(&mut self) -> Result<bool, Error> {
        let unit = self.unit;
        let tx = self.tx()?;
        tx.service(unit);
        tx.take_error()?;
        Ok(tx.room() > 0)
    }

    /// Frames queued, the one in flight included.
    pub fn queued(&self) -> usize {
        self.tx.as_ref().map_or(0, |tx| tx.frames.len())
    }

    /// The oldest finished frame's outcome not yet taken, in the order the
    /// frames were queued: one per frame [`try_send`](Self::try_send) or
    /// [`write`](Self::write) queued, `None` until the next finishes.
    /// [`TX_FRAMES`](crate::uart::TX_FRAMES) are kept; older ones are
    /// overwritten and come back as one [`TxOutcome::Lost`] in their place.
    pub fn take_outcome(&mut self) -> Option<TxOutcome> {
        let unit = self.unit;
        let tx = self.tx.as_mut()?;
        tx.service(unit);
        tx.take_outcome()
    }

    /// Whether the line has gone quiet: the transmit queue is empty and the
    /// USART's transmission complete has come since its last frame, so the
    /// last stop bit has left the wire. A state read, as of the last service:
    /// with [`set_tx_idle_interrupt`](Self::set_tx_idle_interrupt) on and the
    /// USART's vector bound, its interrupt sets it; otherwise a call of
    /// [`on_interrupt`](Self::on_interrupt) first serves the line.
    pub fn tx_drained(&self) -> bool {
        self.tx
            .as_ref()
            .is_some_and(|tx| tx.drained && tx.frames.len() == 0 && !tx.halted)
    }

    /// Whether the USART's transmission complete (TC) raises its vector
    /// once the transmit queue empties, so [`Events::tx_idle`](super::Events::tx_idle) and
    /// [`tx_drained`](Self::tx_drained) come by interrupt. Off when the UART
    /// is opened: a drain is then seen only when the UART is served.
    ///
    /// While on, TC stays pending after the last frame until a service
    /// sees it: a handler bound to the USART's vector must reach this
    /// `Uart` until then. Turn it off, or free the UART from inside the
    /// lock that holds it, before moving it out of the handlers' reach.
    pub fn set_tx_idle_interrupt(&mut self, on: bool) {
        let unit = self.unit;
        if let Some(tx) = self.tx.as_mut() {
            tx.set_notify(unit, on);
        }
    }

    /// Frames sent, DMA errors and timeouts since the UART was opened.
    pub fn tx_health(&self) -> TxHealth {
        self.tx.as_ref().map_or(TxHealth::default(), |tx| tx.health)
    }
}
