# ADR-011: One owner per UART, and a scheduler-neutral `Service` trait (2026-11-17)

ADR-011 - Status: accepted

Supersedes [ADR-004](2026-11-09-uart-capabilities.md) whole: its decisions 1 to 5 (the
`Line` handle, the flag-only `UartIrq`, the `UartTx`/`UartRx` data halves, `split` handing
out the line, and the `LineRegs` name). Its premise, one writer per register and no
critical section in a driver, is kept and restated below.

## Context

The UART was the one driver handed out in pieces: `Uart::open` returned the UART and a
`UartIrq`, `split` gave a `Line`, a `UartTx` and a `UartRx`, and a receive-only UART had
an `RxLine`. The pieces lived in different contexts, so the state they shared (half-lap
count, receive marks, transmit queue, fault counts) lived in a static `Info` as atomics,
and the transmit stream, written by both the task and the interrupt, needed a
`servicing`/`deferred` protocol. The model was copied from FerroWasp's own UART.

The owner judged it a design error (2026-11-16, the ferroforge-stm32 handover): it puts a
concurrency protocol inside a HAL whose execution model (`docs/design/execution-model.md`)
says the scheduler owns execution, it is the only driver shaped this way (SPI, I2C, ADC and
the timers already have one owner each, measured 2026-11-17: no atomics, `split` or `join`
outside `uart/`), and it makes a FerroForge task that owns "the UART" impossible to name
as one type. RTIC already gives every resource shared between a task and a handler a
lock with priority-ceiling semantics; a driver that is `&mut self` everywhere needs
nothing more.

The API review left the interrupt service non-uniform (D12: `on_interrupt` returns `()`,
`bool` or `Option<Result<..>>`, with a `poll` alias on some) and the vectors in four
shapes (D13, U10). Rows D12, D13, N7, N9, N10, N12, S3, U3, U10 and the UART parts of N2
and N16 were assigned to this redesign (owner, 2026-11-17, `docs/DEBT.md` section 2).

## Decision (owner, 2026-11-16 and 2026-11-17)

1. **One owner.** `Uart<W>` holds the instance token, both pins, both stream tokens, both
   buffers and every piece of driver state, as plain fields. Every operation takes
   `&mut self`. There are no handles: `UartIrq`, `Line`, `RxLine`, `UartTx` and `UartRx`
   go, with `split`, `join`, `halves`, `line()`, `tx()` and `rx()`.
2. **No atomics, no protocol.** The atomics in `State`, `TxState`, `MarkQueue` and
   `FrameQueue` become plain fields; the `servicing`/`deferred` protocol goes. `Info`, the
   per-instance static, keeps only the register block.
3. **Concurrency is the scheduler's.** A task and a handler that both use the UART share
   it under the scheduler's lock (an RTIC `#[shared]` resource and `lock`). The driver still
   has no critical section (`docs/design/ownership.md`), and still writes only registers
   it owns: one owner is now the single writer of every USART and stream register.
4. **`on_interrupt(&mut self) -> Events`** serves the USART, the receive stream and the
   transmit stream, from any of the three vectors or from a task, and reports what
   happened: the line went idle (a frame ended), a ring half filled, a transmit frame
   finished, a fault was recorded. A handler acts on them, for example by spawning the
   reader; it reads nothing itself.
5. **The reader serves the receive stream's flags itself** before it counts what DMA
   wrote, as the transmit side already chains frames without its vector (UARTDMA-008).
   UARTDMA-002 still holds: a half-lap count served up to most of a lap late, by the
   handler or the reader, loses and repeats nothing, because the sticky half and complete
   flags still pending are counted with the served ones.
6. **A `Service` trait**, scheduler-neutral, at the crate root:
   `trait Service { type Event; fn on_interrupt(&mut self) -> Self::Event; }`. Implemented
   by every interrupt-served driver: `Uart`, `Spi` (stream DMA), `AdcScan`, `Tick`,
   `CompareLane`, `Sampler` and `Awg`. Each keeps an inherent `on_interrupt`, so callers
   need no import; the `poll` aliases go, since `on_interrupt` is callable from a task.
   This settles D12. The two drivers whose service did not fit the shape (`AdcScan`
   returned borrowed samples; `Awg` took a lane index) are fitted by Q2 below.
7. **One vector type.** `resource::Vectors` lists a driver's vectors, with `iter()` and
   `verify()`, from `vectors()` on every interrupt-served driver. It replaces
   `UartInterrupts`, `SpiInterrupts` and the `interrupt()` + `verify()` pairs (D13, U10).
8. **Names**, under the owner's picks of 2026-11-17 (`docs/design/naming.md`): the
   register wrapper is `Unit` as in SPI, I2C and ADC (N7); a DMA fault is `Error::Dma` as in
   the other drivers, with the cause kept in `Health` (N10); `Health` fields are plural and
   saturate (N12); the refusal alias is `UartRefused` (N1); no module prefix beyond the
   driver type (N2); `mask`/`unmask` stay the names for the line's interrupt enables (N16).
9. **Files.** The engine splits along what it does (S3): `uart/stream_dma/` with the
   driver, the receive engine, the transmit engine and the `embedded-io` impls apart.
10. **`embedded-io`'s `write_all` and `read_exact` are overridden** with versions that
    cannot panic, and both join `checks/no-panic`.

## Forward compatibility (owner-agreed, 2026-11-17)

- USART v3 (F722, F745, F767) is USART v4 without PRESC and the FIFOs, at the same
  offsets: it is served by the `usart_v4` code with only the PRESC write gated.
- The engine reaches DMA only through the erased stream's start, stop, flags, clear,
  remaining count and interrupt enables. A channel-DMA engine (`bdma_v1` behind DMAMUX,
  for the G474 and the H7's BDMA) slots in by offering the same operations; the crate-
  internal trait that names them is written when that second engine lands, not before.

## Questions put to the owner, answered (2026-11-17)

The owner took the recommended answer to each:

- Q1: half-duplex (HDSEL) and TX/RX swap are `Config` fields, in this redesign. Both are
  frame settings: `usart_v4` has both bits; `usart_v2` has HDSEL and refuses swap. A
  receive-only UART refuses both, since each needs the TX pin it does not hold. In
  half-duplex the receiver hears every byte the UART sends, so the ring holds the echo;
  the driver does not filter it.
- Q2: the trait exactly as decision 6 states it, with `AdcScan` reporting completion and
  `samples()` reading them, and `Awg` serving every lane per call and reporting the
  lanes that ended as a mask.
- Q3: a receive-only UART is the same `Uart<W>`, opened by `Uart::new_rx` on an
  `RxWiring`; the transmit operations exist only on a full `Wiring`.

## Rejected

- Keeping the split for firmware that wants a reader and a writer in different tasks:
  RTIC's lock already gives that, at the cost the bench measures (a low-priority holder
  delays the handler by its critical section), and the atomics were the price of avoiding
  it.
- Masking the USART's own enables while a task holds the UART instead of a scheduler lock:
  a second protocol, and the NVIC priority of the handler is the scheduler's to manage.
- A crate-internal DMA-engine trait now: one engine exists, so it would be shaped by a
  guess about the second.

## Consequences

FerroWasp's UART code changes shape at its next revision bump: one shared resource per
UART, and `lock` where it used a half. Every bench and `hil/` run that used `split`,
`UartIrq` or `Line` changes, and the F401 (`usart_v2`) and H753 (`usart_v4`) benches rerun.
The bench also measures how long a low-priority lock holder delays the USART and DMA
handlers, so the cost of decision 3 is on record. UART-013 names the UART, not a transmit
half. Once built, the design moves to `docs/design/uart-ownership.md`.
