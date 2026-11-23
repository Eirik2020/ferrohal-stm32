# Crate and module structure

Source: ADR-006 (archived: [`../decisions/archive/2026/2026-11-10-doc-map-dissolution.md`](../decisions/archive/2026/2026-11-10-doc-map-dissolution.md)).


The module and feature
naming scheme is [ADR-017](../decisions/2026-11-23-naming-scheme.md).

- Hierarchy, bottom up: **IP modules → glue → chips → families → vendors**. Each level is a feature-flag bundle of the level below. Only one vendor exists for now.
- One module per metapac IP version (`usart_v2`, `spi_v4`, `gpio_v2`, …), each behind its own feature. Chip features are thin bundles of IP-version features, and family features bundle chips. Each IP lives in its own folder, the unit a `ferrohal-<ip>` crate would be cut from ([naming.md](naming.md), ADR-012).
- Chip glue grouped by family (ADR-012): `src/chips/<family>/` (`f4/`, `f7/`, `h7/`) holds the family's shared facts (`common.rs`) and glue macros (`macros.rs`), its dies, its errata tables and one chip file per line, named by the lines it serves (`stm32f411.rs`, `stm32f76x_f77x.rs`). What spans families sits in `src/chips/`: `macros.rs` (every chip), `m7.rs` (the Cortex-M7 chips). The F7's glue is written in the F4's macros (`../f4/macros.rs`), as its RCC reuses the F4's tree. `src/chips/gen/<part>.rs` stays flat and generated, and the generator emits the chip selection in `lib.rs` ([glue-generator.md](glue-generator.md)).
- IP modules are chip-neutral: they speak registers only. Chip facts (DMA request maps, pin alternate functions, clock trees, per-chip constants) live in the glue layer, generated from metapac metadata.
- FerroHAL uses metapac's own IP-version grouping as the definition of "shared IP". If metapac groups chips wrongly, accept it for now and fix it upstream rather than keeping a local exception table.
- Start as one crate. An IP module moves into its own crate (`ferrohal-<ip>`) once another HAL reuses it.
- Chip feature names are metapac's exact chip names, passed straight through. FerroHAL owns metapac's `rt` feature behind one FerroHAL feature.
- Pin stm32-metapac exactly. A metapac upgrade is a deliberate, reviewed change.
