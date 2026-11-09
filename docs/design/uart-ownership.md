# UART ownership: line, flags, data (2026-11-09)

Source: ADR-004, [`../decisions/2026-11-09-uart-capabilities.md`](../decisions/2026-11-09-uart-capabilities.md).

One USART is handed out as capabilities, so each register has exactly one writer.

| Handle | Registers | Writes |
|---|---|---|
| `Line` | CR1, CR2, CR3, BRR, PRESC | frame, enables, `mask`/`unmask`, break |
| `UartIrq` | SR/ISR read, ICR write, DR read on v2 | flag clears only |
| `UartTx` | TDR via DMA, SR/ICR for TC | data, TC clear |
| `UartRx` | RDR via DMA | data |

## Rules

- Only `Line` writes CR1 to CR3. `send_break` and `set_config` are `Line` methods that
  borrow `&mut UartTx`, so a break, a frame change and a mask cannot reach the register
  from two contexts.
- `UartIrq` has no method that writes a control register; masking is not one of its
  operations.
- Flag registers are store-only on the write side: ICR on v4 and SR on v2 are written as
  a whole pattern with write-one-to-clear bits, never read-modify-written, so a second
  writer cannot lose another's bit. Reads are unconstrained.
- The transmit stream keeps its `servicing`/`deferred` protocol for its own CR: that
  register is written from both the task and the interrupt.

## Shapes

- `Uart::open` returns `(Uart, UartIrq)`; `Uart` holds the `Line`.
- `Uart::split` returns `(Line, UartTx, UartRx)`; `halves` borrows the three.
- `UartRx::new` returns `(Line, UartRx, UartIrq)`.
- Receive-only has no `UartTx`, so no break and no frame change that borrows one.

## Not solved here

The token is still not carried by any handle (`free.md`). The v2 flag clear reads DR,
which is the byte DMA also reads; that is the documented cost of SR/DR clearing on v2.
