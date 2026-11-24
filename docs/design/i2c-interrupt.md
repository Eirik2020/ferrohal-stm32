# I2C: the interrupt-driven master on lent buffers

Source: ADR-013, [`../decisions/2026-11-19-i2c-interrupt-driver.md`](../decisions/2026-11-19-i2c-interrupt-driver.md).
The measurements behind it: [`../notes/2026-11-19-i2c-interrupt-bench.md`](../notes/2026-11-19-i2c-interrupt-bench.md).

FerroHAL offers two I2C masters, and the user picks. `I2c<W>` blocks: the caller waits
out the transaction. `InterruptI2c<W>` does not: `start` returns at once and the I2C's
vectors carry the transaction. Neither replaces the other.

`InterruptI2c<W>` has one owner and every operation takes `&mut self`, as the UART does
([uart-ownership](uart-ownership.md)). It implements `Service`.

| Operation | What it does |
|---|---|
| `new`, `new_dma`, `free` | open from a plain `Wiring` or a `DmaWiring`; free to the same |
| `start` | begins a transaction and returns; refuses with the transaction handed back |
| `on_interrupt` (also `Service`) | serves the event and error vectors; `Some(Outcome)` at the end |
| `abort` | ends a running transaction and puts the bus back |
| `bound_cycles` | the blocking driver's bound for a transaction of that size |

## Rules

- **Lent buffers.** A transaction carries `&'static mut [u8]` buffers in, and the
  `Outcome` hands them back: at the end, on a refusal and from `abort`. The driver keeps
  no memory of its own.
- **One transaction shape.** One 7-bit address; a write phase, a read phase, or a write
  then a read joined by a repeated start. A phase is 1 to 255 bytes (`MAX_PHASE`): reload
  mode is never used (ES0392 2.19.5), and `i2c_v1` takes the same cap for one rule.
  Anything rarer stays with the blocking driver.
- **The data path is chosen per phase.** Opened from a `DmaWiring`, a phase at least the
  IP version's threshold long moves by DMA and any shorter one from the event interrupt.
  Opened from a plain `Wiring`, every phase is interrupt-fed. `i2c_v1` writes by DMA from
  2 bytes and reads from 3: a one-byte read cannot end by DMA (RM0368 18.3.8). The
  thresholds are `DMA_FROM_WRITE` and `DMA_FROM_READ`.
- **DMA memory is checked at each `start`.** A buffer the channel cannot use is refused
  with `chip::check_dma_buffer`'s verdict, and the transaction comes back.
- **The driver has no timer.** A slave may stretch SCL without limit. The caller bounds
  a transaction with its own timer and calls `abort` when it passes; the transaction
  ends with `Fault::Aborted`.
- **An abort puts the bus back with a stop.** `i2c_v1` keeps the blocking driver's
  recovery: STOP, then SWRST once the bus is free.
- **No path panics.** A fault is an `Outcome`: `Fault::I2c` with what the peripheral
  reported, `Fault::Dma`, or `Fault::Aborted`.
- **Routes come from the glue.** `I2cTxDma` and `I2cRxDma` are emitted per part by the
  glue generator ([glue-generator](glue-generator.md)); an I2C with BDMA-only routes
  gets none and is opened from a plain `Wiring`.

## IP versions

- `i2c_v1` (F4) carries the driver: `src/i2c/interrupt.rs` is chip-neutral and
  `src/i2c_v1/interrupt.rs` is its register half. Requirements I2C-007 to I2C-013,
  I2C-V1-002 and I2C-V1-003.
- `i2c_v2` (F7, H7) has the blocking driver only. Its register half is not built.
