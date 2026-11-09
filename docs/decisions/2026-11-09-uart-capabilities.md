# Decision: UART control and flag capabilities (2026-11-09)

ADR-004 - Status: promoted

Promoted to: [uart-ownership.md](../design/uart-ownership.md).

## Context

W5 adds `mask()`/`unmask()`. The line's enables sit in CR1 (`IDLEIE`, `PEIE`) and CR3
(`EIE`), which two other paths also write after open: `UartTx::send_break` (CR1's SBK on
usart_v2) and `set_config`. `Info` held a full, `Copy` register accessor reachable from
every handle, so "the transmit half is the only writer of CR1" was a comment, not a type.
A mask taken from the USART vector can preempt either writer between its read and write:
the mask is silently lost, or a break is silently cancelled. Policies forbid critical
sections and writing a register outside its single owner (architecture 4 and 13).

## Decision

1. One public `Line` handle owns the control registers, CR1 to CR3, BRR and PRESC: the
   frame, the interrupt enables and a break.
2. `UartIrq` keeps flag servicing only and cannot write an enable or SBK: it has no such
   method.
3. `UartTx` and `UartRx` keep the data paths: DMA stream registers, TDR and RDR, and the
   flag-clear stores `flush` needs.
4. `set_config`, `send_break` and, in W5, `mask`/`unmask` are `Line` methods;
   `Uart::split` hands the line out and `UartRx::new` returns it, so split and
   receive-only firmware keep the capability.
5. The crate-internal register wrapper is renamed `LineRegs`, so `Line` names the handle.

## Rejected

- A `masked`/`busy` deferral protocol on `UartIrq`: a second protocol to keep a second
  writer, and a window where `mask()` has returned but is not in force.
- Plain toggles with a documented "do not overlap" rule: safe code could still lose a
  mask or a break.
- Masking only on the unsplit `Uart`: split firmware, FerroWasp's model, keeps the gap.

## Consequences

`split`, `halves`, `UartRx::new`, `send_break`, `set_config` and `regs` move; FerroWasp
takes the extra tuple element at its rev bump. The bench calls masking from its checks
task. The seam is also where the `free()` redesign can hand a token back, and SPI and
I2C may follow; neither is decided here.
