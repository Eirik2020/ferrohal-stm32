# ADR-001: G5, safe by default (2026-11-08)

ADR-001 - Status: promoted

Promoted to: [`../policies/safety.md`](../policies/safety.md) (policy). This record keeps
the rationale and the rejected alternatives. Approved 2026-11-08.

## Context

The API handed out metapac register blocks through six safe accessors, plus five unsafe
hatches of its own. A HAL's purpose is a safe abstraction; registers belong to the PAC.
Reference HALs do the opposite: f4xx-hal re-exports its PAC (`lib.rs:20-28`) whose `Periph`
derefs to registers (`stm32f4-0.16.0/src/generic.rs:40`); h7xx-hal likewise (`lib.rs:97+`,
`stm32h7-0.15.1/src/stm32h743/mod.rs:3743`) and adds safe `inner()/inner_mut()`. There,
safe code can reconfigure any register.

## Decision

1. G5: no public `unsafe fn`; an exception needs written rationale, dated owner approval,
   and a default-off feature gate.
2. Exception: `Peripherals::steal` behind `rtic` (`rtic = ["rt"]`). RTIC names the item;
   soundness needs `unsafe`; `take()` is the safe path.
3. The six accessors and the other hatches become crate-internal.
4. `device` keeps `Interrupt`, `NVIC_PRIO_BITS`, `interrupt!`, `Peripherals`.
5. Register writes are not wrapped in `unsafe fn`; the register-access rule and the lints
   carry the guarantee.

## Evidence

- `rtic-macros-2.4.0/src/codegen/module.rs:38` emits `device: #device::Peripherals::steal()`;
  the call sits in a generated `pub unsafe fn new` (`:294-299`).
- T1-T3 (2026-11-09): the real crate compiles; without the item the expansion fails with
  E0599; with a safe `pub fn steal` it compiles. The keyword is ours, not RTIC's.
- metapac: 0 `Deref` impls; peripherals are `pub const` blocks from `unsafe from_ptr`, so any
  metapac re-export would hand out safe register access.
- Counts and the T1-T3 experiments: `../notes/2026-11-08-rtic-and-hal-comparison.md`.

## Consequences

Checks `safe-api` + `no-metapac`; two denied lints; requirement API-001; the debt sweep
internalises the accessors; ferro-wasp adds `rtic` at its rev bump; the `free()` redesign
removes the need for `steal`.

## Rejected

- No exception: breaks RTIC `peripherals = true`; nothing else supplies the item.
- Safe `steal()`: unsound - two token sets alias one peripheral.
- `unsafe` accessors instead of internal: still a public path to registers.
- Wrapping every register write: no soundness gain, redefines `unsafe` as "touches hardware".
