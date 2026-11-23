# Maturity: capability, evidence axes and tiers

Source: ADR-009, [`../decisions/2026-11-13-maturity-tiers.md`](../decisions/2026-11-13-maturity-tiers.md).
The silicon layers (series, die, device, package, part): [glue-generator.md](glue-generator.md).
The car rule: [requirements-system.md](requirements-system.md).

## Required vs supported

- A part's **platform** is what FerroHAL needs to run on it at all: the die's
  glue, the clock module for its die, GPIO, reset and system (cause, UID, revision),
  flash latency, the memory map. A part without its platform is unsupported (the G474
  today: no G4 clock module).
- Everything else is a **capability**: one per driver whose IP version the
  die carries. Optional: a part runs without any. DMA and the watchdog are capabilities.

## Evidence axes

- **IP version**, die-independent: proven on any die that carries it, for
  every die and series that does.
- **Die**: its glue (routing, RCC bits, vectors, memory map, bonding read from
  the package), proven on a board with that die.
- **Series**: code a series shares, proven by any member.

## Tiers

| code | tier | the bench that ran |
|---|---|---|
| `BRD` | board-verified | a customer's exact board, tested in full |
| `HIL` | HIL-characterised | a dev board: debugger, loopbacks, peripheral emulators |
| `DBG` | debugger smoke | a dev board with a debugger, no wiring |
| `FLSH` | flashed | flashed and run by any means (USB, debugger, UART, SPI), without full debugger coverage |
| `INH` | inherited | no run of its own: the axes' evidence |

- **[Decided, built 2026-11-15]** A run is evidence at its bench's tier, stamped into the
  record: `reqtool.toml` lists `tiers` (`BRD`, `HIL`, `DBG`, `FLSH`) and each board's tier
  (`board_tiers`), and reqtool (#7) stamps every board record with it, so evidence keeps
  the tier it ran at when a bench is rewired (owner, 2026-11-15: stamped, not mapped at
  read time). The Nucleos and the STM32F4 Discovery are `HIL`, the Foxeer and the Lucid
  `FLSH`.
- A capability on a part is as strong as its weakest axis. A part's score is
  its platform's tier; beside it, a table per capability. A capability not listed is not
  covered. No part is gated by its tier.
- **[Decided, built 2026-11-15]** Tiers are computed from the evidence, never declared:
  `glue-gen tiers all` reads `reqtool check --json` and writes `docs/CHIPS.md`'s tier table
  and `hil/COVERAGE.md`'s matrix; the merge gate regenerates both. The rules
  (`tools/glue-gen/src/tiers.rs`):
  - a requirement reqtool calls verified has every declaring file covered by a counting
    run (the car rule, per file); its tier is the weakest of the boards that verified it;
  - it applies to a part unless a version suffix (`USART-V2`) or a series suffix
    (`RESET-H7`) names another, or it is scoped to another die or series (GLUE-003 the H7,
    GLUE-004 die 0x413);
  - the die axis is the strongest board of the part's die whose glue probe (GLUE-001) is
    verified; a capability is its requirements and the die axis, the weakest; the platform
    likewise with the platform's requirements;
  - a part with no verified evidence of its own is marked `INH`; a capability whose IP
    version FerroHAL does not drive on the part is n/a; one without evidence is –.
