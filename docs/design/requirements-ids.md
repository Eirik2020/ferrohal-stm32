# Requirement IDs and what a user maps onto

Source: ADR-006 (archived: [`../decisions/archive/2026/2026-11-10-doc-map-dissolution.md`](../decisions/archive/2026/2026-11-10-doc-map-dissolution.md)).


The model
is [requirements-system.md](requirements-system.md).

## Requirement IDs

- IDs are named, such as `UART-012`, not paths, and carry no project prefix: the crate name says whose they are. The layer above sets a requirement once, and every part that fulfils it declares the ID, so `usart_v2` and `usart_v4` share one requirement and a replacement part keeps it. A requirement only one part has gets that part's own family, such as `USARTV2-003`.
- IDs stay unique across projects by naming the catalog, not by a hash. A catalog is named by its crate, and a link names both, such as `ferrohal-stm32:UART-012`, so two catalogs may share a prefix and a reader can still read the link.

## What FerroHAL offers its users

- FerroHAL presents its requirements and supplies its verification evidence. A project using it maps its own requirements onto them, and a mapped requirement inherits the evidence. For example, a FerroWasp requirement that UART shall be capable of bidirectional communication using DMA maps to `ferrohal-stm32:UART-001`, that the API shall support bidirectional communication using DMA.
- The mapping lives in the project using FerroHAL. FerroHAL knows nothing of it, as the toaster rule requires.
- A mapping names the crate and the ID (`ferrohal-stm32:UART-012`). Its status is what the revision of FerroHAL the project builds gives, from that revision's own requirements and evidence; moving the pin flags the files that import the crate.
