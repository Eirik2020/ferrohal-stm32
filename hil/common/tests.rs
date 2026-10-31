//! The loopback checks. They use only `ferrohal_stm32::uart` and
//! `embedded_io`, so nothing here names the chip: every board's firmware
//! in `hil/` runs this same file against its own `board.rs`.
//!
//! Every check runs over a [`Link`]: a transmit half and the receive half
//! that hears it. That is one UART with its TX jumpered to its own RX
//! ([`Loop`]), one whose bytes come back through the host ([`HostEcho`]),
//! two UARTs wired TX to RX ([`Cross`]), or borrowed halves of two UARTs
//! ([`Pair`]) so both directions of a cross can stream at once.

// Each firmware compiles this file into itself and uses only the checks its
// wiring allows.
#![allow(dead_code)]

use core::sync::atomic::{AtomicU32, Ordering};

use embedded_io::Write;
use ferrohal_stm32::uart::{
    Chunk, ChunkEnd, Config, ConfigError, Error, Parity, SendError, StopBits, Uart, UartRx, UartTx,
};

/// Frames to wait for the next byte before calling the line silent.
const PATIENCE: u32 = 200;

static CYCLES_PER_MS: AtomicU32 = AtomicU32::new(0);

/// Tells the checks how fast the core runs, for the waits that are not
/// measured in frames.
pub fn set_cpu_hz(hz: u32) {
    CYCLES_PER_MS.store(hz / 1000, Ordering::Relaxed);
}

fn delay_ms(ms: u32) {
    let cycles = CYCLES_PER_MS.load(Ordering::Relaxed);
    for _ in 0..ms {
        cortex_m::asm::delay(cycles);
    }
}

/// Why a check failed.
#[derive(defmt::Format)]
pub enum Fail {
    /// The driver reported an error the check did not expect.
    Uart(Error),
    /// A configuration the check needs was refused.
    Config(ConfigError),
    /// The line went silent this many bytes short.
    Silent { missing: usize },
    /// The byte at `at` came back wrong.
    Mismatch { at: usize, sent: u8, got: u8 },
    /// An error the check provokes never showed up.
    NotReported(Error),
    /// A configuration that must be refused was accepted.
    NotRefused(ConfigError),
    /// `try_send` refused a frame it should have taken, or took one it
    /// should have refused.
    Send(SendError),
    /// A chunk ended otherwise than expected: how it did end.
    Chunk {
        end: ChunkEnd,
        len: usize,
        generation: u32,
    },
    /// A counted wait ran short of its floor, or far past it, in cycles;
    /// or a wait loop ran faster than the HAL's per-pass floor.
    Bound { waited: u32, floor: u32 },
}

/// Passes if `result` is the refusal `expected`.
pub fn expect_refusal(result: Result<(), ConfigError>, expected: ConfigError) -> Result<(), Fail> {
    match result {
        Err(error) if error == expected => Ok(()),
        Err(error) => Err(Fail::Config(error)),
        Ok(()) => Err(Fail::NotRefused(expected)),
    }
}

impl From<Error> for Fail {
    fn from(error: Error) -> Self {
        Self::Uart(error)
    }
}

impl From<ConfigError> for Fail {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

/// A transmit half and the receive half that hears it.
pub trait Link {
    fn name(&self) -> &'static str;
    fn tx(&mut self) -> &mut UartTx;
    fn rx(&mut self) -> &mut UartRx;
    /// Frames to wait for the next byte before calling the line silent.
    fn patience(&self) -> u32 {
        PATIENCE
    }
    /// How long, after a flush, until every byte sent has arrived.
    fn settle_ms(&self) -> u32 {
        5
    }
}

/// A link that can change the frame on both its ends.
pub trait Configurable: Link {
    fn set_config(&mut self, config: &Config) -> Result<(), ConfigError>;
}

/// One UART, TX jumpered to its own RX.
pub struct Loop<'a> {
    pub name: &'static str,
    pub uart: &'a mut Uart,
}

impl Link for Loop<'_> {
    fn name(&self) -> &'static str {
        self.name
    }
    fn tx(&mut self) -> &mut UartTx {
        self.uart.tx()
    }
    fn rx(&mut self) -> &mut UartRx {
        self.uart.rx()
    }
}

impl Configurable for Loop<'_> {
    fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        self.uart.set_config(config)
    }
}

/// One UART whose bytes the host sends back, through the ST-LINK's virtual
/// COM port (`hil/vcp_echo.py`). USB adds milliseconds each way.
pub struct HostEcho<'a> {
    pub name: &'static str,
    pub uart: &'a mut Uart,
}

impl Link for HostEcho<'_> {
    fn name(&self) -> &'static str {
        self.name
    }
    fn tx(&mut self) -> &mut UartTx {
        self.uart.tx()
    }
    fn rx(&mut self) -> &mut UartRx {
        self.uart.rx()
    }
    fn patience(&self) -> u32 {
        // At least 43 ms at 115200, well over a USB round trip.
        500
    }
    fn settle_ms(&self) -> u32 {
        500
    }
}

impl Configurable for HostEcho<'_> {
    fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        self.uart.set_config(config)
    }
}

/// Two UARTs: `from`'s TX wired to `to`'s RX.
pub struct Cross<'a> {
    pub name: &'static str,
    pub from: &'a mut Uart,
    pub to: &'a mut Uart,
}

impl Link for Cross<'_> {
    fn name(&self) -> &'static str {
        self.name
    }
    fn tx(&mut self) -> &mut UartTx {
        self.from.tx()
    }
    fn rx(&mut self) -> &mut UartRx {
        self.to.rx()
    }
}

impl Configurable for Cross<'_> {
    fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        self.from.set_config(config)?;
        self.to.set_config(config)
    }
}

/// Borrowed halves of two UARTs: `tx`'s line wired to `rx`'s. It cannot
/// change the frame, so it streams only.
pub struct Pair<'a> {
    pub name: &'static str,
    pub tx: &'a mut UartTx,
    pub rx: &'a mut UartRx,
}

impl Link for Pair<'_> {
    fn name(&self) -> &'static str {
        self.name
    }
    fn tx(&mut self) -> &mut UartTx {
        self.tx
    }
    fn rx(&mut self) -> &mut UartRx {
        self.rx
    }
}

/// Counts and logs check results.
pub struct Report {
    pub failed: u32,
}

impl Report {
    pub const fn new() -> Self {
        Self { failed: 0 }
    }

    pub fn check(&mut self, link: &str, name: &str, result: Result<(), Fail>) {
        match result {
            Ok(()) => defmt::info!("PASS {=str}: {=str}", link, name),
            Err(fail) => {
                self.failed += 1;
                defmt::error!("FAIL {=str}: {=str}: {}", link, name, fail);
            }
        }
    }
}

/// Byte `index` of a test stream: a hash, so a dropped, repeated or
/// shifted byte shows up at once.
fn pattern(index: usize) -> u8 {
    ((index as u32).wrapping_mul(2_654_435_761) >> 24) as u8
}

/// Reads until the line has been silent for the link's patience, and says
/// how many bytes and errors that took.
pub fn drain(link: &mut dyn Link) -> (usize, usize) {
    let mut buf = [0; 64];
    let (mut bytes, mut errors) = (0, 0);
    let patience = link.patience();
    loop {
        match link.rx().read_within(&mut buf, patience) {
            Ok(len) => bytes += len,
            Err(Error::Timeout) => return (bytes, errors),
            Err(_) => errors += 1,
        }
    }
}

/// Fills `buf` from the line, failing on any error or on silence.
fn read_exact(link: &mut dyn Link, buf: &mut [u8]) -> Result<(), Fail> {
    let patience = link.patience();
    let mut filled = 0;
    while filled < buf.len() {
        match link.rx().read_within(&mut buf[filled..], patience) {
            Ok(len) => filled += len,
            Err(Error::Timeout) => {
                return Err(Fail::Silent {
                    missing: buf.len() - filled,
                });
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Sends `data` and expects it back.
pub fn echo(link: &mut dyn Link, data: &[u8]) -> Result<(), Fail> {
    let mut back = [0; 64];
    let back = &mut back[..data.len()];
    link.tx().write_all(data)?;
    link.tx().flush()?;
    read_exact(link, back)?;
    for (at, (&sent, &got)) in data.iter().zip(back.iter()).enumerate() {
        if sent != got {
            return Err(Fail::Mismatch { at, sent, got });
        }
    }
    Ok(())
}

/// One stream's progress, for [`stream`] and [`streams`].
struct Progress {
    sent: usize,
    received: usize,
}

impl Progress {
    /// Starts what fits of the next chunk, and checks what has arrived.
    /// Returns whether anything moved.
    fn step(&mut self, link: &mut dyn Link, total: usize) -> Result<bool, Fail> {
        let mut chunk = [0; 64];
        let mut back = [0; 64];
        let mut moved = false;
        if self.sent < total {
            let len = chunk.len().min(total - self.sent);
            for (offset, byte) in chunk[..len].iter_mut().enumerate() {
                *byte = pattern(self.sent + offset);
            }
            let written = match link.tx().try_send(&chunk[..len]) {
                Ok(()) => len,
                Err(SendError::Full) => 0,
                Err(SendError::TooLarge | SendError::Halted) => return Err(Error::Halted.into()),
            };
            self.sent += written;
            moved |= written > 0;
        }
        let len = link.rx().try_read(&mut back)?;
        for (offset, &got) in back[..len].iter().enumerate() {
            let at = self.received + offset;
            if got != pattern(at) {
                return Err(Fail::Mismatch {
                    at,
                    sent: pattern(at),
                    got,
                });
            }
        }
        self.received += len;
        Ok(moved || len > 0)
    }
}

/// Streams `total` bytes through the link, reading while writing, and
/// checks every one. Exercises chunked transmit and many laps of the
/// receive ring.
pub fn stream(link: &mut dyn Link, total: usize) -> Result<(), Fail> {
    streams(&mut [link], total)
}

/// Streams `total` bytes through every link at once, interleaved, so their
/// DMA streams and interrupts run side by side.
pub fn streams(links: &mut [&mut dyn Link], total: usize) -> Result<(), Fail> {
    const MAX: usize = 4;
    let mut progress = [const {
        Progress {
            sent: 0,
            received: 0,
        }
    }; MAX];
    // Silent for this long on every link at once: give up.
    let patience_ms = links
        .iter()
        .map(|link| link.settle_ms())
        .max()
        .unwrap_or(0)
        .max(50);
    let mut idle = 0;
    loop {
        let mut moved = false;
        let mut done = true;
        for (link, progress) in links.iter_mut().zip(progress.iter_mut()) {
            moved |= progress.step(&mut **link, total)?;
            done &= progress.received >= total;
        }
        if done {
            return Ok(());
        }
        if moved {
            idle = 0;
        } else {
            // In 100 µs steps.
            idle += 1;
            if idle > patience_ms * 10 {
                let missing = links
                    .iter()
                    .zip(progress.iter())
                    .map(|(_, p)| total - p.received)
                    .sum();
                return Err(Fail::Silent { missing });
            }
            cortex_m::asm::delay(CYCLES_PER_MS.load(Ordering::Relaxed) / 10);
        }
    }
}

/// Sends three rings' worth without reading: the next read must report the
/// lost bytes, and the link must work afterwards.
pub fn ring_overrun(link: &mut dyn Link, ring_len: usize) -> Result<(), Fail> {
    let chunk = [0x55; 64];
    for _ in 0..(3 * ring_len).div_ceil(chunk.len()) {
        link.tx().write_all(&chunk)?;
    }
    link.tx().flush()?;
    // Bytes still in flight (through the host, say) arrive meanwhile.
    for _ in 0..link.settle_ms() {
        match link.rx().available() {
            Err(Error::BufferOverrun) => {
                drain(link);
                return echo(link, b"after the overrun");
            }
            Err(error) => return Err(error.into()),
            Ok(_) => delay_ms(1),
        }
    }
    Err(Fail::NotReported(Error::BufferOverrun))
}

/// Sends bytes `start..start + len` of the test pattern.
fn send_pattern(link: &mut dyn Link, start: usize, len: usize) -> Result<(), Fail> {
    let mut chunk = [0; 64];
    let mut sent = 0;
    while sent < len {
        let n = chunk.len().min(len - sent);
        for (offset, byte) in chunk[..n].iter_mut().enumerate() {
            *byte = pattern(start + sent + offset);
        }
        link.tx().write_all(&chunk[..n])?;
        sent += n;
    }
    link.tx().flush()?;
    Ok(())
}

/// Reads `len` bytes and checks they are `start..start + len` of the test
/// pattern.
fn expect_pattern(link: &mut dyn Link, start: usize, len: usize) -> Result<(), Fail> {
    let mut back = [0; 64];
    let mut got = 0;
    while got < len {
        let n = back.len().min(len - got);
        read_exact(link, &mut back[..n])?;
        for (offset, &byte) in back[..n].iter().enumerate() {
            let at = start + got + offset;
            if byte != pattern(at) {
                return Err(Fail::Mismatch {
                    at,
                    sent: pattern(at),
                    got: byte,
                });
            }
        }
        got += n;
    }
    Ok(())
}

/// The receive DMA interrupt held off while the ring crosses both its end
/// and its half: `read` must count the pending flags itself, and neither
/// lose bytes nor report an overrun that did not happen.
///
/// Each round shifts the ring by a sixteenth unmasked, then sends 7/8 of a
/// ring with the interrupt masked and reads it back before unmasking. 7/8
/// of a ring crosses two half marks whenever it starts more than 1/4 of a
/// ring past one, and the shift (15/16 a ring per round) walks the start
/// through every sixteenth, so at least five of the eight rounds leave both
/// flags pending, whatever position earlier checks left the ring in.
/// `mask(true)` must mask the link's receive DMA vector in the NVIC,
/// `mask(false)` unmask it.
pub fn late_interrupt(
    link: &mut dyn Link,
    ring_len: usize,
    mask: &mut dyn FnMut(bool),
) -> Result<(), Fail> {
    let shift = ring_len / 16;
    let late = ring_len * 7 / 8;
    drain(link);
    let mut at = 0;
    for _ in 0..8 {
        send_pattern(link, at, shift)?;
        expect_pattern(link, at, shift)?;
        at += shift;
        mask(true);
        let result = send_pattern(link, at, late).and_then(|()| {
            delay_ms(link.settle_ms());
            expect_pattern(link, at, late)
        });
        mask(false);
        result?;
        at += late;
    }
    echo(link, b"after the late interrupt")
}

/// The next complete chunk, polling for up to the link's settle time.
fn next_chunk(link: &mut dyn Link, buf: &mut [u8]) -> Result<Chunk, Fail> {
    for _ in 0..=link.settle_ms() {
        if let Some(chunk) = link.rx().read_chunk(buf)? {
            return Ok(chunk);
        }
        delay_ms(1);
    }
    Err(Fail::Silent { missing: buf.len() })
}

/// Fails unless `chunk` ended as `end` with `len` bytes of the pattern from
/// `start`, in generation `generation`.
fn expect_chunk(
    chunk: Chunk,
    buf: &[u8],
    end: ChunkEnd,
    start: usize,
    len: usize,
    generation: u32,
) -> Result<(), Fail> {
    if chunk.end != end || chunk.len != len || chunk.generation != generation {
        return Err(Fail::Chunk {
            end: chunk.end,
            len: chunk.len,
            generation: chunk.generation,
        });
    }
    for (offset, &got) in buf[..len].iter().enumerate() {
        let at = start + offset;
        if got != pattern(at) {
            return Err(Fail::Mismatch {
                at,
                sent: pattern(at),
                got,
            });
        }
    }
    Ok(())
}

/// Bursts with pauses between them come out as one chunk per burst, ended
/// by the idle line, with no gap.
pub fn chunks_end_at_idle(link: &mut dyn Link) -> Result<(), Fail> {
    const BURSTS: [usize; 5] = [7, 25, 64, 1, 40];
    drain(link);
    let generation = link.rx().generation();
    let mut at = 0;
    for len in BURSTS {
        send_pattern(link, at, len)?;
        // Many idle frames at any rate tested; one is enough for IDLE.
        delay_ms(2);
        at += len;
    }
    let mut buf = [0u8; 64];
    let mut start = 0;
    for len in BURSTS {
        let chunk = next_chunk(link, &mut buf)?;
        expect_chunk(chunk, &buf, ChunkEnd::Idle, start, len, generation)?;
        start += len;
    }
    Ok(())
}

/// A burst longer than the caller's buffer comes out as full chunks, then
/// the rest ended by the idle line.
pub fn chunk_fills_the_buffer(link: &mut dyn Link) -> Result<(), Fail> {
    drain(link);
    let generation = link.rx().generation();
    send_pattern(link, 0, 100)?;
    delay_ms(2);
    let mut buf = [0u8; 32];
    for (start, len, end) in [
        (0, 32, ChunkEnd::Full),
        (32, 32, ChunkEnd::Full),
        (64, 32, ChunkEnd::Full),
        (96, 4, ChunkEnd::Idle),
    ] {
        let chunk = next_chunk(link, &mut buf)?;
        expect_chunk(chunk, &buf, end, start, len, generation)?;
    }
    Ok(())
}

/// A break ends its chunk with a framing gap and advances the generation by
/// exactly one; the chunks around it are whole.
pub fn break_is_a_gap(link: &mut dyn Link) -> Result<(), Fail> {
    drain(link);
    let generation = link.rx().generation();
    let framing = link.rx().health().framing;
    send_pattern(link, 0, 10)?;
    delay_ms(2);
    link.tx().send_break()?;
    delay_ms(2);
    send_pattern(link, 10, 10)?;
    delay_ms(2);
    let mut buf = [0u8; 64];
    let before = next_chunk(link, &mut buf)?;
    expect_chunk(before, &buf, ChunkEnd::Idle, 0, 10, generation)?;
    // The break's all-zero frame, ended by the framing error it raised.
    let gap = next_chunk(link, &mut buf)?;
    if gap.end != ChunkEnd::Gap(Error::Framing) || gap.generation != generation + 1 {
        return Err(Fail::Chunk {
            end: gap.end,
            len: gap.len,
            generation: gap.generation,
        });
    }
    let after = next_chunk(link, &mut buf)?;
    expect_chunk(after, &buf, ChunkEnd::Idle, 10, 10, generation + 1)?;
    if link.rx().health().framing <= framing {
        return Err(Fail::NotReported(Error::Framing));
    }
    Ok(())
}

/// A ring overrun comes out as an empty chunk ending in a gap, and the
/// stream carries on in the next generation.
pub fn overrun_is_a_gap(link: &mut dyn Link, ring_len: usize) -> Result<(), Fail> {
    drain(link);
    let generation = link.rx().generation();
    let overruns = link.rx().health().buffer_overrun;
    let chunk = [0x55; 64];
    for _ in 0..(3 * ring_len).div_ceil(chunk.len()) {
        link.tx().write_all(&chunk)?;
    }
    link.tx().flush()?;
    delay_ms(link.settle_ms());
    let mut buf = [0u8; 64];
    let gap = next_chunk(link, &mut buf)?;
    if gap.end != ChunkEnd::Gap(Error::BufferOverrun) || gap.len != 0 || gap.generation != generation + 1 {
        return Err(Fail::Chunk {
            end: gap.end,
            len: gap.len,
            generation: gap.generation,
        });
    }
    if link.rx().health().buffer_overrun != overruns + 1 {
        return Err(Fail::NotReported(Error::BufferOverrun));
    }
    drain(link);
    echo(link, b"after the overrun gap")
}

/// Frame lengths for the transmit checks: varied, so frames wrap the
/// transmit ring at different points.
const FRAME_LENS: [usize; 7] = [17, 40, 5, 33, 64, 9, 26];

/// Queues frames of the pattern from `start` with `try_send` until the
/// queue refuses one, without waiting. Returns how many bytes went in.
fn queue_frames(link: &mut dyn Link, start: usize, tx_len: usize) -> Result<usize, Fail> {
    let mut frame = [0u8; 64];
    let mut queued = 0;
    for len in FRAME_LENS.iter().copied().cycle().take(32) {
        let len = len.min(tx_len).min(frame.len());
        for (offset, byte) in frame[..len].iter_mut().enumerate() {
            *byte = pattern(start + queued + offset);
        }
        match link.tx().try_send(&frame[..len]) {
            Ok(()) => queued += len,
            Err(SendError::Full) => break,
            Err(error) => return Err(Fail::Send(error)),
        }
    }
    Ok(queued)
}

/// Frames queued back to back, more than one transfer's worth, arrive whole
/// and in order, and the count of frames sent follows.
pub fn frames_in_order(link: &mut dyn Link, tx_len: usize) -> Result<(), Fail> {
    drain(link);
    let sent = link.tx().health().sent;
    let queued = queue_frames(link, 0, tx_len)?;
    if link.tx().queued() < 2 {
        return Err(Fail::Silent { missing: queued });
    }
    let frames = link.tx().queued() as u32;
    link.tx().flush()?;
    expect_pattern(link, 0, queued)?;
    if link.tx().health().sent < sent + frames {
        return Err(Fail::Silent { missing: queued });
    }
    Ok(())
}

/// With nothing calling the transmit half after the frames are queued, the
/// transmit stream's interrupt alone starts each next frame.
pub fn frames_chain_from_the_interrupt(link: &mut dyn Link, tx_len: usize) -> Result<(), Fail> {
    drain(link);
    let queued = queue_frames(link, 0, tx_len)?;
    // Hands off the transmit half: only the interrupt can move the queue.
    delay_ms(link.settle_ms().max(2 * queued as u32 / 10 + 5));
    if link.tx().queued() != 0 {
        return Err(Fail::Silent { missing: queued });
    }
    expect_pattern(link, 0, queued)
}

/// With all of the UART's vectors masked, only the first frame goes on its
/// own; polling sends the rest. Every vector serves the transmit queue, so
/// all three are masked.
pub fn frames_go_out_by_polling(
    link: &mut dyn Link,
    tx_len: usize,
    mask_all_irqs: &mut dyn FnMut(bool),
) -> Result<(), Fail> {
    drain(link);
    mask_all_irqs(true);
    let result = (|| {
        let queued = queue_frames(link, 0, tx_len)?;
        let waiting = link.tx().queued();
        delay_ms(link.settle_ms().max(2 * queued as u32 / 10 + 5));
        // The first frame went; the queue could not move past it.
        if waiting < 2 || link.tx().queued() == 0 {
            return Err(Fail::NotReported(Error::Timeout));
        }
        // Polling alone moves it. Under a ring of bytes arrives meanwhile,
        // which the reader counts with the receive vector masked.
        link.tx().flush()?;
        Ok(queued)
    })();
    mask_all_irqs(false);
    expect_pattern(link, 0, result?)
}

/// A frame longer than the transmit buffer is refused outright, and the
/// link carries on.
pub fn oversized_frame_refused(link: &mut dyn Link, tx_len: usize) -> Result<(), Fail> {
    drain(link);
    let big = [0u8; 300];
    if tx_len >= big.len() {
        return Ok(());
    }
    match link.tx().try_send(&big[..tx_len + 1]) {
        Err(SendError::TooLarge) => {}
        Err(error) => return Err(Fail::Send(error)),
        Ok(()) => return Err(Fail::Send(SendError::TooLarge)),
    }
    echo(link, b"after the oversized frame")
}

/// Sends a break, which the receiver sees as a frame with no stop bit: it
/// must be reported as a framing error, and the link must recover.
pub fn line_break(link: &mut dyn Link) -> Result<(), Fail> {
    link.tx().send_break()?;
    expect_error(link, Error::Framing)?;
    echo(link, b"after the break")
}

/// Reads until the line goes quiet, and fails unless `expected` came up.
fn expect_error(link: &mut dyn Link, expected: Error) -> Result<(), Fail> {
    let patience = link.patience();
    let mut buf = [0; 8];
    let mut seen = false;
    loop {
        match link.rx().read_within(&mut buf, patience) {
            Err(Error::Timeout) => break,
            Err(error) if error == expected => seen = true,
            // Garbled bytes, and other errors the garbling causes.
            Ok(_) | Err(_) => {}
        }
    }
    if seen {
        Ok(())
    } else {
        Err(Fail::NotReported(expected))
    }
}

/// Switches frame settings, then echoes and streams at them.
pub fn reconfigured(link: &mut dyn Configurable, config: &Config) -> Result<(), Fail> {
    link.set_config(config)?;
    drain(link);
    echo(link, b"reconfigured")?;
    stream(link, 2048)
}

/// The sender runs at half the receiver's baud rate and sends zeros: each
/// one holds the line low past the receiver's stop bit, a framing error.
pub fn framing_mismatch(cross: &mut Cross) -> Result<(), Fail> {
    cross.from.set_config(&Config::new(57_600))?;
    cross.to.set_config(&Config::new(115_200))?;
    drain(cross);
    cross.tx().write_all(&[0; 16])?;
    cross.tx().flush()?;
    let result = expect_error(cross, Error::Framing);
    restore(cross)?;
    result
}

/// Even parity into a receiver expecting odd: every byte is a parity
/// error. The receiver is not transmitting, so ES0299 2.10.2 (PE cleared by
/// a write to DR) cannot hide it.
pub fn parity_mismatch(cross: &mut Cross) -> Result<(), Fail> {
    let with = |parity| Config {
        baudrate: 115_200,
        parity,
        stop_bits: StopBits::One,
    };
    cross.from.set_config(&with(Parity::Even))?;
    cross.to.set_config(&with(Parity::Odd))?;
    drain(cross);
    cross.tx().write_all(b"parity")?;
    cross.tx().flush()?;
    let result = expect_error(cross, Error::Parity);
    restore(cross)?;
    result
}

/// Both ends back to 115200 8N1, and a line through to prove it.
fn restore(cross: &mut Cross) -> Result<(), Fail> {
    cross.set_config(&Config::new(115_200))?;
    drain(cross);
    echo(cross, b"restored")
}

/// Every check a jumpered loop can take, starting from the frame the link
/// was opened with, `config`, and ending back on it. `mask_rx_irq` masks
/// (`true`) or unmasks (`false`) the link's receive DMA vector in the NVIC;
/// `mask_all_irqs` all three of its vectors (line, receive, transmit).
pub fn full_suite(
    report: &mut Report,
    link: &mut dyn Configurable,
    config: &Config,
    ring_len: usize,
    tx_len: usize,
    mask_rx_irq: &mut dyn FnMut(bool),
    mask_all_irqs: &mut dyn FnMut(bool),
) {
    let name = link.name();
    let (bytes, errors) = drain(link);
    if bytes > 0 || errors > 0 {
        defmt::warn!(
            "{=str}: line was not quiet: {} bytes, {} errors",
            name,
            bytes,
            errors
        );
    }
    report.check(name, "echo", echo(link, b"hello ferrohal\n"));
    report.check(name, "echo, buffer-sized", echo(link, &[0xa5; 64][..tx_len.min(64)]));
    report.check(name, "stream 8 KiB", stream(link, 8 * 1024));
    report.check(name, "ring overrun reported", ring_overrun(link, ring_len));
    report.check(
        name,
        "counted and clocked waits within their bounds",
        wait_bounds(
            link,
            CYCLES_PER_MS.load(Ordering::Relaxed) * 1000,
            config.baudrate,
        ),
    );
    report.check(
        name,
        "receive interrupt late by most of a lap",
        late_interrupt(link, ring_len, mask_rx_irq),
    );
    report.check(name, "break reported", line_break(link));
    report.check(name, "chunks end where the line goes idle", chunks_end_at_idle(link));
    report.check(name, "a full buffer ends a chunk", chunk_fills_the_buffer(link));
    report.check(name, "a break ends its chunk with a gap", break_is_a_gap(link));
    report.check(name, "a ring overrun is a gap", overrun_is_a_gap(link, ring_len));
    report.check(name, "queued frames arrive whole and in order", frames_in_order(link, tx_len));
    report.check(name, "the interrupt chains queued frames", frames_chain_from_the_interrupt(link, tx_len));
    report.check(
        name,
        "frames go out by polling with the vectors masked",
        frames_go_out_by_polling(link, tx_len, mask_all_irqs),
    );
    report.check(name, "an oversized frame is refused", oversized_frame_refused(link, tx_len));
    report.check(name, "1 Mbaud", reconfigured(link, &ONE_MBAUD));
    report.check(name, "57600 8E2", reconfigured(link, &EVEN_PARITY_TWO_STOP));
    report.check(name, "back to its own frame", reconfigured(link, config));
    defmt::info!("{=str}: health {}, transmit {}", name, link.rx().health(), link.tx().health());
}

/// The HAL's per-pass cycle floor for the read loop (`uart::stream_dma`);
/// a counted wait is only as long as promised while no pass is faster.
const RX_PASS_CYCLES: u32 = 224;
const TX_PASS_CYCLES: u32 = 48;
/// The most a counted wait may overshoot: the slowest configuration
/// measured (H753, caches off) passes about 3.2 times slower than the
/// fastest the floor is set from, the floor is half that fastest pass, and
/// the HAL budgets 12-bit frames where this measures 10: 3.2 x 2 x 1.2,
/// rounded up with margin.
const COUNTED_OVERSHOOT: u32 = 12;

/// The DWT cycle counter as FerroHAL's caller clock. The firmware must
/// have enabled it.
pub struct DwtClock;

impl ferrohal_stm32::uart::CycleClock for DwtClock {
    fn cycles(&self) -> u32 {
        cortex_m::peripheral::DWT::cycle_count()
    }
}

pub static DWT_CLOCK: DwtClock = DwtClock;

/// Counted waits are never shorter than asked and not more than
/// `COUNTED_OVERSHOOT` times longer; waits measured by a caller clock last
/// the time asked.
///
/// Measures with the DWT cycle counter, which the firmware must enable:
/// - one pass of each wait loop (an empty `try_read`, a busy `is_idle`),
///   which must cost at least the HAL's floor for it;
/// - `read_within(buf, 100)` on a silent line, counted, then again with
///   the DWT clock set: at least 100 frames at 8N1, and at most
///   `COUNTED_OVERSHOOT` times that counted, or 1.5 times clocked.
pub fn wait_bounds(link: &mut dyn Link, cpu_hz: u32, baudrate: u32) -> Result<(), Fail> {
    use cortex_m::peripheral::DWT;
    const PASSES: u32 = 10_000;
    const FRAMES: u32 = 100;
    drain(link);
    let mut buf = [0u8; 16];

    let start = DWT::cycle_count();
    for _ in 0..PASSES {
        let _ = core::hint::black_box(link.rx().try_read(&mut buf));
    }
    let rx_pass = DWT::cycle_count().wrapping_sub(start) / PASSES;

    link.tx().write(&[0x55; 64])?;
    let (mut passes, start) = (0u32, DWT::cycle_count());
    while !link.tx().is_idle()? {
        passes += 1;
    }
    let tx_pass = DWT::cycle_count().wrapping_sub(start) / passes.max(1);
    drain(link);

    let mut timed = |link: &mut dyn Link| {
        let start = DWT::cycle_count();
        let silent = link.rx().read_within(&mut buf, FRAMES);
        (silent, DWT::cycle_count().wrapping_sub(start))
    };
    let (counted, counted_waited) = timed(link);
    link.rx().set_clock(Some(&DWT_CLOCK));
    let (clocked, clocked_waited) = timed(link);
    link.rx().set_clock(None);

    let frames_time = FRAMES * 10 * (cpu_hz / baudrate);
    let tenths = |waited: u32| waited / (frames_time / 10).max(1);
    defmt::info!(
        "{=str}: cycles per pass: read {=u32} (floor {=u32}), transmit {=u32} (floor {=u32}); read_within({=u32}) waited {=u32}/10 of the frame time counted, {=u32}/10 clocked",
        link.name(),
        rx_pass,
        RX_PASS_CYCLES,
        tx_pass,
        TX_PASS_CYCLES,
        FRAMES,
        tenths(counted_waited),
        tenths(clocked_waited)
    );
    for silent in [counted, clocked] {
        match silent {
            Err(Error::Timeout) => {}
            Err(error) => return Err(error.into()),
            Ok(len) => return Err(Fail::Mismatch { at: len, sent: 0, got: 0 }),
        }
    }
    if rx_pass < RX_PASS_CYCLES || tx_pass < TX_PASS_CYCLES {
        return Err(Fail::Bound { waited: rx_pass, floor: RX_PASS_CYCLES });
    }
    if counted_waited < frames_time || counted_waited > COUNTED_OVERSHOOT * frames_time {
        return Err(Fail::Bound { waited: counted_waited, floor: frames_time });
    }
    if clocked_waited < frames_time || clocked_waited > frames_time * 3 / 2 {
        return Err(Fail::Bound { waited: clocked_waited, floor: frames_time });
    }
    Ok(())
}

/// A `mask_rx_irq` for [`full_suite`]: masks or unmasks `interrupt` in the
/// NVIC. Test code only: FerroHAL itself never touches the NVIC.
pub fn nvic_mask<I: cortex_m::interrupt::InterruptNumber>(interrupt: I) -> impl FnMut(bool) {
    move |masked| {
        if masked {
            cortex_m::peripheral::NVIC::mask(interrupt);
        } else {
            // SAFETY: the vector's handler was bound by RTIC, which had it
            // unmasked before this check masked it.
            unsafe { cortex_m::peripheral::NVIC::unmask(interrupt) };
        }
    }
}

/// A `mask_all_irqs` for [`full_suite`]: masks or unmasks each of
/// `interrupts` in the NVIC.
pub fn nvic_mask_all<I: cortex_m::interrupt::InterruptNumber, const N: usize>(
    interrupts: [I; N],
) -> impl FnMut(bool) {
    move |masked| {
        for interrupt in interrupts {
            if masked {
                cortex_m::peripheral::NVIC::mask(interrupt);
            } else {
                // SAFETY: as `nvic_mask`.
                unsafe { cortex_m::peripheral::NVIC::unmask(interrupt) };
            }
        }
    }
}

/// Logs the outcome, calls `pass` if every check passed, and ends the run
/// through semihosting: status 0 on a pass, 1 otherwise.
pub fn finish(report: &Report, pass: impl FnOnce()) -> ! {
    use cortex_m_semihosting::debug::{self, EXIT_FAILURE, EXIT_SUCCESS};
    if report.failed == 0 {
        pass();
        defmt::info!("all checks passed");
        debug::exit(EXIT_SUCCESS);
    } else {
        defmt::error!("{} checks failed", report.failed);
        debug::exit(EXIT_FAILURE);
    }
    // Only reached without a debugger to catch the exit.
    loop {
        cortex_m::asm::wfi();
    }
}

pub const ONE_MBAUD: Config = Config::new(1_000_000);
pub const EVEN_PARITY_TWO_STOP: Config = Config {
    baudrate: 57_600,
    parity: Parity::Even,
    stop_bits: StopBits::Two,
};
