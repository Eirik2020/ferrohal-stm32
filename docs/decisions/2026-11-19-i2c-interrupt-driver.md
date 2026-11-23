# ADR-013: An interrupt-driven I2C master on lent buffers (2026-11-19)

ADR-013 - Status: promoted

Promoted to: [i2c-interrupt.md](../design/i2c-interrupt.md) (design).

## Context

FerroHAL offered one I2C master, `I2c<W>`, and it blocks: the caller waits out every
bit, about 6 ms for a 32-byte read at 50 kHz. Whether to block is the user's choice,
and a user who does not want to wait had no driver to pick. The ferroforge-stm32 I2C
port is to take the SPI port's lent-buffer shape, with an address in place of a chip
select.

Four variants were prototyped and benched against each other on the F401 (`i2c_v1`), the
F767 and the H753 (`i2c_v2`): data by DMA or by the event interrupt, on lent buffers or
on driver-owned ones with a copy each way. The measurements are in
[`../notes/2026-11-19-i2c-interrupt-bench.md`](../notes/2026-11-19-i2c-interrupt-bench.md).

## Decision

1. A second driver, `InterruptI2c<W>`, beside `I2c<W>`, which stays: the user picks
   either. One owner, `&mut self` everywhere, the `Service` trait (ADR-011). `start`
   returns at once; `on_interrupt` returns `Some(Outcome)` at the end; `abort` ends a
   transaction the caller's own bound has passed. The driver has no timer;
   `bound_cycles` gives the blocking driver's bound.
2. Lent buffers. A transaction carries `&'static mut [u8]` buffers in and hands them
   back with the outcome, on a refusal and from `abort`. DMA buffers are checked with
   `chip::check_dma_buffer` at each `start`.
3. One transaction shape. One 7-bit address, a write phase, a read phase, or a write
   then a read joined by a repeated start; each phase 1 to 255 bytes (no reload,
   ES0392 2.19.5). Anything rarer stays with the blocking driver.
4. The data path per phase, by threshold. Opened from a `DmaWiring`, a phase at least
   the IP version's threshold long moves by DMA, any other from the event interrupt;
   opened from a plain `Wiring`, every phase is interrupt-fed. `i2c_v1` writes by DMA
   from 2 bytes and reads from 3 (a one-byte read cannot end by DMA, RM0368 18.3.8);
   `i2c_v2` from 2.
5. Aborts put the bus back with a stop. `i2c_v1` keeps the blocking driver's recovery
   (STOP, then SWRST once the bus is free); `i2c_v2` sets CR2.STOP and leaves PE on, and
   a later `start` that still finds the bus held recovers the peripheral and refuses
   with `BusBusy`.
6. Routes from the glue. `I2cTxDma` and `I2cRxDma` are emitted per part by the glue
   generator; an I2C with BDMA-only routes gets none.
7. `i2c_v1` lands first; `i2c_v2` follows.

## Rejected

- Driver-owned buffers with a copy each way: they cost more and gained nothing.
- One data path for every phase: a one-byte phase is cheaper interrupt-fed, and from two
  bytes DMA wins.
- An `i2c_v2` abort that only cycles PE: it left the slave mid-read.

## Consequences

- Requirements I2C-007 to I2C-013 and I2C-V1-002, I2C-V1-003.
- Lent buffers are new in FerroHAL; SPI and UART keep their owned buffers until a
  decision of their own.
