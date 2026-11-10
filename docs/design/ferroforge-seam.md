# The FerroForge seam: foot-guns and fixes

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 8, dissolved 2026-11-10. The rtic-monotonics
versions referred to below are 2.1.0, 2.2.1 and 3.0.0 *(measured)*.

| # | Foot-gun | Fix | Status |
|---|---|---|---|
| 1 | The monotonic's timer is owned twice. rtic-monotonics takes its timer completely (registers, the RCC bit, its own handler, its NVIC priority), while FerroHAL's `Peripherals` would also hand out a token for it | The monotonic setup path consumes FerroHAL's timer token by value. rtic-monotonics 3's `start()` takes no token, so the wrapper does this; start it in init | Still applies after the version bump |
| 2 | No per-IP instance traits | `Instance` checked at construction, then erased ([ownership.md](ownership.md)) | Resolved |
| 3 | A chip named twice (FerroHAL names vs rtic-monotonics chip features) | Pass metapac's chip names through. rtic-monotonics 3 selects the chip on the user's own stm32-metapac dependency | Dissolved by the version bump |
| 4 | `rt` enabled by nobody, or by two crates | FerroHAL owns `rt` behind one feature, which the backend turns on | Decided |
| 5 | Two metapac versions in one build | rtic-monotonics 3 accepts stm32-metapac `>=16`, so it unifies with FerroHAL's copy | Dissolved by the version bump |
| 6 | Task crates keyed on chips | HAL-specific task crates are keyed on IP-version features and checked against one representative chip per IP version | Decided |

- **[Decided]** Branch FerroForge and bump it to rtic 2.4.0 and rtic-monotonics 3.0.0. rtic-monotonics 3 is a breaking release: FerroForge's `[timer-monotonic] chip-feature` mechanism must change, interrupt-based monotonics take their priority in `start()`, and fugit moves to 0.6, which affects every task crate that reads time.
- **[Proposal]** FerroForge chooses between the old HAL and FerroHAL through backend data, for example a second backend entry per chip.
- **[Decided]** RTIC itself is not modified.
