# Crate and module structure

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 2, dissolved 2026-11-10. The module and feature
naming scheme is [ADR-003](../decisions/2026-11-08-naming-scheme.md).

- **[Decided]** Hierarchy, bottom up: **IP modules → glue → chips → families → vendors**. Each level is a feature-flag bundle of the level below. Only one vendor exists for now.
- **[Decided]** One module per metapac IP version (`usart_v2`, `dma_v2`, `gpio_v2`, …), each behind its own feature. Chip features are thin bundles of IP-version features, and family features bundle chips.
- **[Decided]** IP modules are chip-neutral: they speak registers only. Chip facts (DMA request maps, pin alternate functions, clock trees, per-chip constants) live in the glue layer, generated from metapac metadata.
- **[Decided]** FerroHAL uses metapac's own IP-version grouping as the definition of "shared IP". If metapac groups chips wrongly, accept it for now and fix it upstream rather than keeping a local exception table.
- **[Decided]** Start as one crate. An IP module moves into its own crate (`ferrohal-<ip>`) once another HAL reuses it.
- **[Decided]** Chip feature names are metapac's exact chip names, passed straight through. FerroHAL owns metapac's `rt` feature behind one FerroHAL feature.
- **[Decided]** Pin stm32-metapac exactly. A metapac upgrade is a deliberate, reviewed change.
