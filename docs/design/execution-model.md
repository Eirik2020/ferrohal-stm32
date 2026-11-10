# Execution model: scheduler-agnostic

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 3, dissolved 2026-11-10.

**[Decided]** FerroHAL drives peripherals; the scheduler owns execution.

- It never defines interrupt handlers, never sets NVIC priorities and never enables vectors in the NVIC. It may set a peripheral's own interrupt-enable bits.
- Drivers expose plain service methods (read and clear flags, `on_interrupt`-style handlers) that the scheduler's handlers call.
- There is no async in the core and no time source owned by FerroHAL. Bounded waits are counted, or the caller supplies the bound.
- RTIC-first means owned driver values that are moved into `#[local]`.
