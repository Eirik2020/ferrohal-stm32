# Execution model: scheduler-agnostic

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 3, dissolved 2026-11-10.

**[Decided]** FerroHAL drives peripherals; the scheduler owns execution.

- It never defines interrupt handlers, never sets NVIC priorities and never enables vectors in the NVIC. It may set a peripheral's own interrupt-enable bits.
- Drivers expose plain service methods (read and clear flags, `on_interrupt`-style handlers) that the scheduler's handlers call.
- There is no async in the core, no time source owned by FerroHAL, and no time crate in the core. Bounded waits are counted, or the caller supplies the bound. Time crates are reached through adapter features (G1, `docs/policies/features.md`).
- RTIC-first means owned driver values that are moved into `#[local]`.
