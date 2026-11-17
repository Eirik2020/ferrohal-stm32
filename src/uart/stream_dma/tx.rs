//! The transmit engine: a queue of whole frames in the transmit buffer, one
//! DMA transfer per frame, chained by the service.
//!
//! Requirements: UART-006, UART-010, UART-011, UART-013, UARTDMA-008, UARTDMA-009, UARTDMA-012

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
        tx_ring::{self, FRAMES, Frame, FrameQueue},
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

/// The transmit stream's interrupts: a frame done, or a DMA error. A FIFO
/// error is advisory in direct mode and only cleared.
const TX_INTERRUPTS: Interrupts = Interrupts {
    complete: true,
    half: false,
    error: true,
    fifo_error: false,
};

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
        self.dma.set_interrupts(Interrupts::default());
        let stopped = self.dma.stop(crate::dma::STOP_SPINS);
        self.dma.clear_flags(Flags::ALL);
        self.frames.clear();
        self.running = false;
        stopped
    }

    /// Finishes the frame in flight if it is done, and starts the next.
    /// Returns whether a frame finished (or failed), and whether it failed.
    pub(super) fn service(&mut self, unit: Unit) -> (bool, bool) {
        let mut finished = (false, false);
        if self.running {
            let flags = self.dma.flags();
            if flags.transfer_error || flags.direct_mode_error {
                if self.dma.stop(crate::dma::STOP_SPINS).is_err() {
                    self.halted = true;
                }
                self.dma.clear_flags(Flags::ALL);
                self.frames.pop();
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
            return finished;
        }
        let Some(frame) = self.frames.peek() else {
            return finished;
        };
        unit.clear_tc();
        let transfer = Transfer {
            request: self.request,
            // ES0392 2.20.4: TRBUFF keeps a U(S)ART stream from locking when
            // another transfer runs beside it.
            #[cfg(feature = "dma_v1")]
            bufferable: true,
            direction: Direction::MemoryToPeripheral,
            peripheral: unit.tx_address(),
            // The frame lies inside the buffer.
            memory: self.buf.wrapping_add(usize::from(frame.offset)) as u32,
            len: frame.len,
            size: WordSize::Byte,
            memory_increment: true,
            circular: false,
            priority: Priority::Medium,
            interrupts: TX_INTERRUPTS,
        };
        // SAFETY: the frame's bytes are in the transmit buffer, written before
        // the frame was queued and untouched until it is popped, after the
        // stream has finished with it. The routing trait vouched for the
        // request.
        match unsafe { self.dma.start(&transfer) } {
            Ok(()) => self.running = true,
            // Still enabled: a stop that never took. Fail closed.
            Err(_) => self.halted = true,
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
        let stopped = self.dma.stop(crate::dma::STOP_SPINS).is_ok();
        self.dma.clear_flags(Flags::ALL);
        self.frames.clear();
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

    /// Frames sent, DMA errors and timeouts since the UART was opened.
    pub fn tx_health(&self) -> TxHealth {
        self.tx.as_ref().map_or(TxHealth::default(), |tx| tx.health)
    }
}
