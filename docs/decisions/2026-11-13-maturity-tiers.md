# ADR-009: Maturity by evidence tier, per IP, die and series (2026-11-13)

ADR-009 - Status: promoted

Promoted to: [maturity.md](../design/maturity.md) (design).

## Context

FerroHAL had one maturity notion: a requirement verified on a Nucleo, with the flight
boards' runs as smoke tests carrying no evidence. With a generator able to emit many
parts, a part's maturity needs a definition that keeps the car rule: evidence belongs
to the IP version, the die or the series, not to a part.

## Decision (owner, 2026-11-13)

1. Capability is separate from maturity, and an IP from a die. A part *requires* its
   platform (die glue, clock module, GPIO, reset and system, flash latency, memory
   map); everything else is an optional capability. A part without its platform is
   unsupported; a part without a driver still runs.
2. Three evidence axes: IP version (die-independent, proven on any die carrying it),
   die (its glue, proven on a board with that die), series (shared code, proven by any
   member).
3. One ladder, by the bench that ran: `BRD` board-verified (a customer's exact board),
   `HIL` HIL-characterised (debugger, loopbacks, peripheral emulators), `DBG` debugger
   smoke (no wiring), `FLSH` flashed and run by any means without full debugger
   coverage, `INH` inherited (no run of its own).
4. A capability on a part is as strong as its weakest axis; a part's score is its
   platform's tier, with a per-capability table. A capability not listed is not
   covered. No part is gated by its tier.
5. Tiers are computed from the evidence, never declared, and checked at the merge gate.

## Consequences

- Supersedes the rule that smoke runs carry no evidence (`requirements-system.md`): a
  flashed board's runs are `FLSH` evidence.
- reqtool records each run's tier, as its generic environment.
- `docs/CHIPS.md` gains the computed tier tables once the tool exists.

## Rejected

- Gating a part on an unproven IP version: evidence belongs to the IP, so an unproven
  UART leaves the UART uncovered, not the part.
- One tier per part from its weakest capability: it hides what is covered.
