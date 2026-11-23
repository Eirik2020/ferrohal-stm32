# ADR-019: The glue offers everything the silicon has (2026-11-23)

ADR-019 - Status: promoted

Promoted to: [glue-generator.md](../design/glue-generator.md) (design).

## Context

ADR-008 generates the chip glue from metapac. It did not say how much of a chip the
glue offers: only what a bench or a known user needs, or all of it.

## Decision

1. For every IP version FerroHAL drives, the glue offers everything the silicon has:
   each instance, pin row and DMA route metapac lists.
2. Left out, each with its reason listed: an instance a driver cannot open without
   disturbing another, a block the driver does not carry, and a block with no driver.

## Rejected

No alternative was recorded.

## Consequences

- `docs/design/glue-generator.md` lists what is left out and why.
- No code changes: the generator already emits this.
