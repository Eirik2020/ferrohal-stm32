# Maturity: capability, evidence axes and tiers

Source: ADR-009, [`../decisions/2026-11-13-maturity-tiers.md`](../decisions/2026-11-13-maturity-tiers.md).
The silicon layers (series, die, device, package, part): [glue-generator.md](glue-generator.md).
The car rule: [requirements-system.md](requirements-system.md).

## Required vs supported

- **[Decided]** A part's **platform** is what FerroHAL needs to run on it at all: the die's
  glue, the clock module for its die, GPIO, reset and system (cause, UID, revision),
  flash latency, the memory map. A part without its platform is unsupported (the G474
  today: no G4 clock module).
- **[Decided]** Everything else is a **capability**: one per driver whose IP version the
  die carries. Optional: a part runs without any. DMA and the watchdog are capabilities.

## Evidence axes

- **[Decided]** **IP version**, die-independent: proven on any die that carries it, for
  every die and series that does.
- **[Decided]** **Die**: its glue (routing, RCC bits, vectors, memory map, bonding read from
  the package), proven on a board with that die.
- **[Decided]** **Series**: code a series shares, proven by any member.

## Tiers

| code | tier | the bench that ran |
|---|---|---|
| `BRD` | board-verified | a customer's exact board, tested in full |
| `HIL` | HIL-characterised | a dev board: debugger, loopbacks, peripheral emulators |
| `DBG` | debugger smoke | a dev board with a debugger, no wiring |
| `FLSH` | flashed | flashed and run by any means (USB, debugger, UART, SPI), without full debugger coverage |
| `INH` | inherited | no run of its own: the axes' evidence |

- **[Decided]** A run is evidence at its bench's tier; reqtool records the tier as the
  run's environment.
- **[Decided]** A capability on a part is as strong as its weakest axis. A part's score is
  its platform's tier; beside it, a table per capability. A capability not listed is not
  covered. No part is gated by its tier.
- **[Decided]** Tiers are computed from the evidence, never declared; `docs/CHIPS.md`'s
  tables are written by a command and checked at the merge gate.

## Today (derived from which benches exist, not yet computed)

| bench | tier | die | series |
|---|---|---|---|
| Nucleo-F401RE | `HIL` | 0x433 | F4 |
| Nucleo-H753ZI | `HIL` | 0x450 | H7 |
| Foxeer F405 V2 | `FLSH` | 0x413 | F4 |
| TBS Lucid H7 | `FLSH` | 0x450 | H7 |
