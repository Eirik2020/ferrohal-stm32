# Crate layering above FerroHAL

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 7, dissolved 2026-11-10.

**[Decided]**

```text
async layer (left to others; assumed to follow embassy-stm32's implementation)
ferroforge-stm32   RTIC/FerroForge drivers and task groups, built the way FerroWasp does it today
ferrohal-stm32     the neutral HAL
stm32-metapac
```

- FerroHAL knows nothing about FerroForge. FerroForge knows FerroHAL only as backend data: the device path and which HAL crate to select.
- **[Decided]** Hooks the core keeps so an embassy-style async layer can be built on top without a fork:
  1. **Interrupt servicing from a static context:** per-instance static access (safe or `unsafe`) to read and clear flags and to mask interrupt-enable bits. The erased static `Info` from [ownership.md](ownership.md) provides this.
  2. **A raw `unsafe` DMA transfer API** (pointer plus length) alongside the safe API, which takes owned or `'static` buffers. Leak soundness is then the async layer's responsibility.
  3. **Stop and status primitives** reachable directly: request stop, is-running, remaining count and circular mode with half-transfer flags.
  4. **Shared-register writes through a lockable single owner** ([ownership.md](ownership.md)).
  5. **Timer handover:** a timer token can be consumed by an external owner, such as a time driver or a monotonic.
  6. **Stealable zero-sized tokens.**
- Wakers, lost wakeups, cancellation semantics and the choice of executor are the async layer's concerns, not the core's.
