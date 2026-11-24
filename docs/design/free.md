# `free()`: handing tokens back

Every driver handle that owns a peripheral token hands it back with `free(self) -> T`,
stopping what it ran as a `Drop` would: requirement `API-002`, proven on both Nucleos
(runs V/U free each UART, SPI and I2C and read them off; the timer runs reopen theirs).

Source: ADR-002, [`../decisions/2026-11-08-free-teardown-brief.md`](../decisions/2026-11-08-free-teardown-brief.md).

## The rules, and what meets them

0. `free()` is the inverse of the constructor: it hands
   back exactly what `new` took, as a refusal already does. A UART opened with its pins
   and DMA channels frees to its token, both pins and both typed DMA channel tokens; a handle
   keeps those parts typed so they come back typed. **Met by every driver:** `Adc` (its
   token), `AdcScan` (the ADC, its DMA channel, its buffer), `Sdmmc` (its wiring), `UsbFs`
   (its token and both pins), `Uart` (its wiring and buffers, or a receive-only one's
   receive wiring and ring), `Spi` (its wiring and buffers), `BlockingSpi` and `I2c`
   (their wiring), `PwmChannel` (its pin), `CompareLane` (its wiring and buffers),
   `Sampler` (its timer, DMA channel and buffer), `LaneGroup` (its `Pwm`s and lanes), timers. A
   handle is generic over its whole wiring (`Uart<W>`), so a board
   names its route once, as a type alias.
   A handle that owns DMA buffers (`Uart`,
   `Spi`, `CompareLane`, `Sampler`, `AdcScan`) refuses to free while one of its
   DMA channels does not stop: `free` returns `Result<parts, Refused<StopTimeout, Self>>`,
   so the buffers never leave under a running DMA channel, and the handle comes back to try
   again (API-003, run T on the H753). Before, a failed stop was ignored and the buffers
   handed out anyway.

1. Every handle that can touch a peripheral carries the token, or a borrow of it; a
   second handle is a compile error, not a runtime rule. **Met:** the token lives in
   `Pwm<T>`, `Tick<T>`, `Timebase<T>`, `Uart<W>`, `Spi<W>`,
   `BlockingSpi<W>`, `I2c<W>` and `Sampler<T, S>`; a `LaneGroup` frees both its timers;
   `PwmChannel` borrows its `Pwm`; the UART has one owner and no halves (ADR-011).
2. `free(self) -> T` returns the token only when the last handle is gone. **Met:** a
   UART frees whole, a receive-only one to its `RxWiring`; a channel
   borrows, so it is gone before `Pwm::free`.
3. A channel or lane that outlives `Pwm::free` must not be constructible. **Met:**
   `PwmChannel<'a, ..>` borrows; a `CompareLane` holds no timer access and writes
   only through a `&Pwm` it is handed.
4. A freed or aborted timer leaves its outputs inactive at once. The duties are
   preloaded and a stopped counter makes no update to load them, so without one an
   output stopped mid-pulse would hold high: `Pwm::free` and `LaneGroup::abort` write duty 0
   and raise the update event themselves.

A lane names its channel as a const generic beside its wiring (`CompareLane<T, C, W>`):
stable Rust cannot read the channel off the wiring's type.

Proof: `checks/no-metapac` probes that a receive-only UART frees only to its
`RxWiring` (E0308) and has no transmit operations (E0599); `checks/no-panic` runs every
`free()` on all five chips.
