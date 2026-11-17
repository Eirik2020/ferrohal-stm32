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
- **[Decided]** Every wiring describes itself as `const` data, `Wiring::RESOURCES` (the instance, each pin with its function, each stream with its request, the vector), defined beside the wiring struct in the neutral module, not in the engine (API review S4, 2026-11-17). Rows are built with `Resource::pin`.
- **[Decided]** A constructor's refusal carries its module's `ConfigError`, and a running driver's failures its `Error`; one enum never holds both (API review D8, 2026-11-17). The clock tree's refusals are `rcc::ClockError`, which USB's constructor passes on, since its only refusal is a missing 48 MHz source.
- **[Decided]** Every bus driver (SPI, polled SPI, I2C) answers `name`, `frequency` (the bus clock in use), `kernel_clock` and `set_clock`, and its module a `const fn check::<Instance>(&clocks, &config)` giving that frequency; `health` only where the driver counts faults it cannot return from a call (the DMA engines, I2C's cleared bus errors), so not the polled SPI. A `Config` has a `const fn new` and `with_*` builders for its optional fields, and derives `defmt::Format` unless a field's type lacks it (SPI's embedded-hal `Mode`) (API review N18, N19, 2026-11-17).
- **[Decided]** Device module for RTIC: FerroHAL exports an svd2rust-shaped device module (the `Interrupt` enum, `NVIC_PRIO_BITS`, `Peripherals::steal()` and the vector table). RTIC's `device = …` and `peripherals = true` then work unchanged.
