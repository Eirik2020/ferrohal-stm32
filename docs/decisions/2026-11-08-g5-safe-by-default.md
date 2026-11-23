# ADR-001: G5, safe by default (2026-11-08)

ADR-001 - Status: promoted

Promoted to: [`../policies/safety.md`](../policies/safety.md) (policy).

## Context

The API handed out metapac register blocks through six safe accessors, plus five unsafe
hatches of its own. A HAL's purpose is a safe abstraction; registers belong to the PAC.
Reference HALs do the opposite: f4xx-hal and h7xx-hal re-export their PAC, whose
peripherals deref to registers, so safe code there can reconfigure any register. RTIC
with `peripherals = true` generates a call to `Peripherals::steal()`, and compiles only
when the device crate has that item. The file and line references and the three compile
experiments are in `../notes/2026-11-08-rtic-and-hal-comparison.md`.

## Decision

1. G5: no public `unsafe fn`; an exception needs written rationale, dated owner approval,
   and a default-off feature gate.
2. Exception: `Peripherals::steal` behind `rtic` (`rtic = ["rt"]`). RTIC names the item;
   soundness needs `unsafe`; `take()` is the safe path.
3. The `rtic` feature exposes only the absolute minimum RTIC needs.
4. The six accessors and the other hatches become crate-internal.
5. `device` keeps `Interrupt`, `NVIC_PRIO_BITS`, `interrupt!`, `Peripherals`.
6. Register writes are not wrapped in `unsafe fn`; the register-access rule and the lints
   carry the guarantee.

## Rejected

- No exception: breaks RTIC `peripherals = true`; nothing else supplies the item.
- Safe `steal()`: unsound - two token sets alias one peripheral.
- `unsafe` accessors instead of internal: still a public path to registers.
- Wrapping every register write: no soundness gain, redefines `unsafe` as "touches hardware".

## Consequences

Checks `safe-api` + `no-metapac`; two denied lints; requirement API-001; the debt sweep
internalises the accessors; ferro-wasp adds `rtic` at its rev bump; the `free()` redesign
removes the need for `steal`.
