# Public API surface

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 5, dissolved 2026-11-10. G5 (no public
`unsafe fn`) is policy: [../policies/safety.md](../policies/safety.md).

- **[Decided]** Implement `embedded-hal` 1.0 (and `embedded-io` where it fits) for everything those traits can express.
- **[Decided]** DMA and other capabilities that embedded-hal cannot express use per-IP, family-wide APIs, configured per board. Each IP version's API exposes its engine as it really is. `dma_v2` has FIFO and direct-mode errors, NDTR and double buffering; `bdma_v1` has circular mode and no double buffering *(measured)*. Making traits chip-neutral is a job for the adapter layer.
- **[Decided]** The transport mechanism (how a buffer swap happens on each IP) is internal to that IP's driver. Any mechanism is acceptable as long as it is robust:
  - every DMA and peripheral error flag can be read and cleared by the caller;
  - there is a bounded stop that reports failure, with no unbounded spin;
  - there is no panic on a hardware error.
- **[Decided]** Fault policy: the ultimate goal is a no-panic HAL in which every fault flag is handled. The prototype balances this against development speed.
- **[Decided]** Type protection matches stm32f4xx-hal: pin mode typestates, per-chip pin alternate-function traits, and DMA stream, channel and peripheral pairings as trait impls. Wrong routing is a compile error. For the prototype these impls are hand-written, in the same trait shapes a generator would emit later.
- **[Decided]** No resolver ever chooses DMA streams, channels, pins or timers. The developer states them explicitly and the types check them.
- **[Decided]** Device module for RTIC: FerroHAL exports an svd2rust-shaped device module (the `Interrupt` enum, `NVIC_PRIO_BITS`, `Peripherals::steal()` and the vector table). RTIC's `device = …` and `peripherals = true` then work unchanged.
