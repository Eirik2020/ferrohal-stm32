# Crate and module structure

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 2, dissolved 2026-11-10. The module and feature
naming scheme is [ADR-003](../decisions/2026-11-08-naming-scheme.md).

- **[Decided]** Hierarchy, bottom up: **IP modules → glue → chips → families → vendors**. Each level is a feature-flag bundle of the level below. Only one vendor exists for now.
- **[Decided]** One module per metapac IP version (`usart_v2`, `spi_v4`, `gpio_v2`, …), each behind its own feature. Chip features are thin bundles of IP-version features, and family features bundle chips. DMA is the exception: its neutral `dma` module holds one engine per DMA kind, since one die can carry several DMA IPs, and the feature names the version ([naming.md](naming.md), ADR-012).
- **[Decided]** Chip glue grouped by family (ADR-012; lands on top of the F7 glue): `src/chips/<family>/` holds the family's shared facts and macros (`mod.rs`), its dies, its errata tables and one file per line named by the line (`stm32f411.rs`, not its first part); `src/chips/gen/<part>.rs` stays flat and generated; the generator also emits the chip selection (the one-chip check and the line mounts) that `lib.rs` now lists by hand.
- **[Decided]** IP modules are chip-neutral: they speak registers only. Chip facts (DMA request maps, pin alternate functions, clock trees, per-chip constants) live in the glue layer, generated from metapac metadata.
- **[Decided]** FerroHAL uses metapac's own IP-version grouping as the definition of "shared IP". If metapac groups chips wrongly, accept it for now and fix it upstream rather than keeping a local exception table.
- **[Decided]** Start as one crate. An IP module moves into its own crate (`ferrohal-<ip>`) once another HAL reuses it.
- **[Decided]** Chip feature names are metapac's exact chip names, passed straight through. FerroHAL owns metapac's `rt` feature behind one FerroHAL feature.
- **[Decided]** Pin stm32-metapac exactly. A metapac upgrade is a deliberate, reviewed change.
