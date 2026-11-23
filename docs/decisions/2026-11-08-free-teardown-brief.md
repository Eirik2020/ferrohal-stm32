# ADR-002: Ownership and `free()` (2026-11-08)

ADR-002 - Status: promoted

Promoted to: [free.md](../design/free.md) (design).

## Context

G5 removes `steal()`, but firmware legitimately needs one peripheral in a second role
(a timer as `Tick`, then as `Pwm`; a UART polled, then with DMA). `Refused` already
returns tokens when a constructor refuses, so `free()` is the missing half of the same
contract. Ecosystem precedent exists: `stm32h7xx-hal-0.16` has 11 `free(...)` methods
(`free(self) -> ($peripheral, $rec)`); `stm32f4xx-hal-0.23` has none.

## Decision

1. Handles own their tokens and can hand them back (`free()`), across the drivers.
2. Sub-handles are lifetime-bound or joinable, so a handle cannot outlive the token it
   uses.
3. Until `free()` exists, the six bench "second use" checks move into fresh-boot runs so
   `tokens!::steal` can be deleted.

## Rejected

No alternative was recorded.

## Consequences

- A safe `free()` needs a redesign of the handles, not one added method: a channel or
  lane that held register access without a borrow could outlive its timer's `free()`.
  What blocked each driver and the shape that landed are in
  [free-shapes.md](../design/archive/2026/free-shapes.md) (archived).
- The fresh-boot bench runs become redundant once `free()` exists.
