# ADR-007: Time and panic adapters behind features (2026-11-13)

ADR-007 - Status: promoted

Promoted to: [goals.md](../../requirements/goals.md) (G1, G2) and
[features.md](../policies/features.md) (policy).

## Context

G1 said FerroHAL owns no time source and G2 that no public function can reach a panic,
with no room for the ecosystem around them: a time crate (rtic-time, embassy-time) or a
third-party stack that can panic (`synopsys-usb-otg` behind `usb`) had no place in the
goals. The `usb` feature already broke the letter of G2.

## Decision

Owner, 2026-11-13, in their words: "FerroHAL should have features that enable adapters
that allow you to use the different time crates, like rtic-time, embassy-time.
Internally, it shall not use them to stay time agnostic. Same way with no-panic.
FerroHAL guarantees no-panic, except explicitly listed with a feature gate, but it
allows adapters that could cause a panic, if the user wants to enable the feature. This
way the default experience is no-panic, with the flexibility of not breaking the
ecosystem."

1. G1: the core uses no time crate; adapter features may connect FerroHAL to one, and
   the core never calls an adapter.
2. G2: the default build reaches no panic; a feature that can is off by default and
   listed as an exception in `docs/policies/features.md`.
3. Allowing is not building: the owner said the adapters are not to be implemented now.
   Those not built are on the roadmap (`docs/CHIPS.md`).

## Consequences

- `usb` becomes G2's first listed exception, so DEBT's "USB sits outside the no-panic
  proof" is no longer debt; an in-crate, panic-free USB core joins the roadmap.
- The features policy lists every optional feature, what it adapts and whether it can
  panic; a new feature joins it in the commit that adds it.
