# UART ownership: one owner, served by `on_interrupt`

Source: ADR-011, [`../decisions/2026-11-17-uart-single-owner.md`](../decisions/2026-11-17-uart-single-owner.md),
which superseded ADR-004's line, flag and data handles
(archived: [`../decisions/archive/2026/2026-11-09-uart-capabilities.md`](../decisions/archive/2026/2026-11-09-uart-capabilities.md)).
The proposals the stream engine grew from are archived: [uart-ferrowasp.md](archive/2026/uart-ferrowasp.md).

One `Uart<W>` holds the instance token, both pins, both stream tokens, both buffers and
all driver state as plain fields. Every operation takes `&mut self`, so the one owner is
the single writer of every USART and stream register. A task and a handler that both use
the UART share it under the scheduler's lock (RTIC: a `#[shared]` resource); the driver
has no atomics, no critical section and no protocol of its own.

| Operation | Registers | Present on |
|---|---|---|
| `new`, `new_rx`, `set_config`, `free` | CR1, CR2, CR3, BRR, PRESC (v4 only), both streams | every UART; `set_config` per wiring |
| `mask`, `unmask` | CR1 IDLEIE/PEIE, CR3 EIE | every UART |
| `send_break` | RQR (v4) or CR1 SBK (v2) | a full `Wiring` only |
| `try_send`, `write`, `flush`, `embedded_io::Write` | TDR via DMA, TC | a full `Wiring` only |
| reads, `available`, `embedded_io::Read` | RDR via DMA, receive stream flags | every UART |
| `on_interrupt` (also `Service`) | SR/ISR read, ICR write, DR read on v2, all three vectors' flags | every UART |

## Rules

- **One owner.** No handle is handed out: no `split`, `join` or per-context view. A
  firmware that reads in one task and writes in another locks the one resource; the cost
  is that a low-priority holder delays the UART's handlers by its critical section.
- **Events out.** `on_interrupt` serves the USART, the receive stream and the transmit
  stream from any of the three vectors, or from a task, and returns `Events` (idle,
  filled, sent, fault). The handler acts on them (spawns the reader); it reads nothing.
- **Reception does not need its vector.** A read serves the receive stream's flags before
  counting, and still-pending half and complete flags count with the served ones, so a
  service up to most of a lap late loses nothing (UART-DMA-002). Transmission likewise
  chains queued frames on any call.
- **Reception is a circular ring.** The receive DMA never stops between frames: the
  driver marks where the line went idle and where a line error struck, and a read copies
  chunks out of the ring.
- **Data loss is part of the stream.** An overrun, a DMA error, a line error or a restart
  ends a chunk with a gap and advances a generation counter, so a reader cannot miss a
  discontinuity. `Health` counts each cause.
- **Transmit is frame-atomic and queued.** A frame goes in whole or not at all, and
  queued frames go out whole and in order.
- **Flag registers are store-only on the write side.** ICR on v4 and SR on v2 are written
  as a whole pattern of write-one-to-clear bits, never read-modify-written. The v2 clear
  reads DR, the byte DMA also reads: the documented cost of SR/DR clearing on v2.
- **Receive-only is the same type.** `Uart::new_rx` opens on an `RxWiring`; the transmit
  operations exist only on a full `Wiring`, so a UART without a TX pin cannot be asked to
  send (`checks/no-metapac`). It refuses half-duplex and swap, which need the TX pin.
- **`free` is the inverse of `new`.** It hands back the wiring and buffers whole, or the
  UART itself while a stream has not stopped ([free](free.md)).
- **Half-duplex and swap are `Config` fields.** `usart_v4` has both; `usart_v2` has
  half-duplex and refuses swap. In half-duplex the receiver hears every byte the UART
  sends, so the ring holds the echo; the driver does not filter it.
- **`write_all` and `read_exact` cannot panic.** The `embedded-io` defaults are
  overridden, and both are in `checks/no-panic`.
- **Names.** The register wrapper is `Unit`, as in SPI, I2C and ADC. A DMA fault is
  `Error::Dma`, with the cause kept in `Health`, whose fields are plural and saturate.
  The refusal alias is `UartRefused`. `mask` and `unmask` name the line's interrupt
  enables ([naming](naming.md)).
- **Files.** `uart/stream_dma/` holds the driver, the receive engine, the transmit
  engine and the `embedded-io` impls apart.

## IP versions and DMA

- `usart_v2` (F4) and `usart_v4` (H7) each supply a register wrapper, `Unit`; the engine
  in `uart/stream_dma/` is shared. `usart_v3` (F7) is served by the v4 module with only
  the PRESC write gated.
- The engine reaches DMA only through the erased stream: start, stop, running state,
  flags and their clear, remaining count, and interrupt enables. A channel-DMA engine
  (BDMA behind DMAMUX) slots in by offering the same operations; the crate-internal trait
  naming them is written when that engine lands (ADR-011).

## Interrupt service in every driver

- `Service`, at the crate root, is scheduler-neutral:
  `trait Service { type Event; fn on_interrupt(&mut self) -> Self::Event; }`. Every
  interrupt-served driver implements it and keeps an inherent `on_interrupt`, so a
  caller needs no import. There is no `poll` alias: `on_interrupt` is callable from a
  task.
- `AdcScan` reports completion and `samples()` reads them.
- `resource::Vectors` lists a driver's vectors, with `iter()` and `verify()`, from
  `vectors()` on every interrupt-served driver.
