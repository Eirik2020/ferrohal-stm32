# ADR-013: An interrupt-driven I2C master on lent buffers (2026-11-19)

ADR-013 - Status: accepted

## Context

`I2c<W>` is blocking: the caller's CPU waits out every bit, about 6 ms for a 32-byte read
at 50 kHz on the F401 bench. SPI has a DMA driver beside its polled one; I2C's module
doc said "No DMA: an I2C exchange is a few bytes". A GPS stream read, or a
control loop with a barometer on the bus, cannot spend that wire time. The
ferroforge-stm32 I2C port is to take the SPI port's lent-buffer shape (its D22), with an
address in place of a chip select.

The owner asked for the variants to be prototyped and benched against each other rather
than chosen on paper (2026-11-19). Four were built and run on the F401 (`i2c_v1`), the
F767 (`i2c_v2` with DMA) and the H753 (`i2c_v2`, interrupt-fed: its one benchable I2C,
I2C4, has only BDMA routes, RM0433 Table 128), against the Pico 1 simulator at 50 kHz:
data by DMA or by the event interrupt, on lent buffers or on driver-owned ones with a
copy each way. The prototype, its patches and the full tables are kept outside the tree.

What the benches measured (CPU cycles a transaction costs the caller; means of ten):

| | F401, 84 MHz | F767, 216 MHz |
|---|---|---|
| DMA, a write or a read, any length 2-32 | about 2 900 | about 3 900 |
| DMA, a write then a read | about 6 100 | about 6 400 |
| interrupt-fed, a byte | about 470 | about 580 |
| blocking, 32 bytes read | 502 800 | 1 316 600 |
| owned over lent: `start`, and the copy out | +140 to +230, 100-200 | +370 to +660, 350-525 |

A phase of one byte is cheaper interrupt-fed on both; from two bytes DMA wins, except a
two-byte read on `i2c_v1`, which waits once at BTF and costs 2 406 against 2 931. On
`i2c_v2`, an abort that only cycled PE left the slave mid-read: the next transaction
NACKed and, in earlier rounds, the bus stayed held until the Pico was replugged. Setting
CR2.STOP instead brought the bus back at once on both `i2c_v2` boards.

## Decision

1. **A second driver, `InterruptI2c<W>`, beside `I2c<W>`.** One owner, `&mut self`
   everywhere, the `Service` trait (ADR-011). `start` returns at once; `on_interrupt`
   serves the event and error vectors and, on `i2c_v1`, the receive stream's, and returns
   `Some(Outcome)` at the end; `abort` ends a transaction the caller's own bound has
   passed. The driver has no timer; it offers the blocking driver's bound as
   `bound_cycles`.
2. **Lent buffers.** A transaction carries `&'static mut [u8]` buffers in and hands them
   back with the outcome, on a refusal and from `abort`. Owned buffers cost more and gave
   nothing the bench found. DMA buffers are checked with `chip::check_dma_buffer` at each
   `start`.
3. **One transaction shape.** One 7-bit address, a write phase, a read phase, or a write
   then a read joined by a repeated start; each phase 1 to 255 bytes (no reload,
   ES0392 2.19.5). Anything rarer stays with the blocking driver's operation list.
4. **The data path per phase, by threshold.** Opened from a `DmaWiring` (both streams),
   a phase at least the IP version's threshold long moves by DMA, any other from the
   event interrupt; opened from a plain `Wiring`, every phase is interrupt-fed. The
   thresholds are the bench's crossovers: `i2c_v1` writes from 2 bytes and reads from 3
   (a one-byte read cannot end by DMA, RM0368 18.3.8); `i2c_v2` from 2.
5. **Aborts put the bus back with a stop.** `i2c_v1` keeps the blocking driver's
   recovery (STOP, then SWRST once the bus is free); `i2c_v2` sets CR2.STOP and leaves PE
   on, and a later `start` that still finds the bus held recovers the peripheral and
   refuses with `BusBusy`.
6. **Routes from the glue.** `I2cTxDma` and `I2cRxDma` are emitted per part by the glue
   generator, as the SPI and UART routes are; an I2C with BDMA-only routes gets none.
7. **Landing.** `i2c_v1` first, with the F401 bench. `i2c_v2` follows once the F767's
   intermittent 32-byte miscompares (three in three runs, a different mode and byte each
   time, never on the H753 or F401) have a measured cause.

## Consequences

- `docs/CHIPS.md` lists the driver on `i2c_v1` and the `i2c_v2` half on the roadmap.
- Requirements I2C-007 to I2C-013 and I2C-V1-002, I2C-V1-003; the bench is
  `requirements/benches.md`, "Interrupt-driven I2C on the Nucleo-F401RE".
- Lent buffers are new in FerroHAL; SPI and UART keep their owned buffers until a
  decision of their own.
