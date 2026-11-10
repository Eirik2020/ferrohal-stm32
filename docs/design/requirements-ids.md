# Requirement IDs and what a user maps onto

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 9's catalog half, dissolved 2026-11-10. The model
is [requirements-system.md](requirements-system.md).

## Requirement IDs

- **[Decided]** IDs are named, such as `UART-012`, not paths, and carry no project prefix: the crate name says whose they are. The layer above sets a requirement once, and every part that fulfils it declares the ID, so `usart_v2` and `usart_v4` share one requirement and a replacement part keeps it. A requirement only one part has gets that part's own family, such as `USARTV2-003`.
- **[Decided]** IDs stay unique across projects by naming the catalog, not by a hash. A catalog is named by its crate, and a link names both, such as `ferrohal-stm32:UART-012`, so two catalogs may share a prefix and a reader can still read the link.

## What FerroHAL offers its users

- **[Decided]** FerroHAL presents its requirements and supplies its verification evidence. A project using it maps its own requirements onto them, and a mapped requirement inherits the evidence. For example, a FerroWasp requirement that UART shall be capable of bidirectional communication using DMA maps to `ferrohal-stm32:UART-001`, that the API shall support bidirectional communication using DMA.
- **[Decided]** The mapping lives in the project using FerroHAL. FerroHAL knows nothing of it, as the toaster rule requires.
- **[Proposal]** A mapping records the FerroHAL version and the hash of the requirement's text, so a changed requirement flags the mapping.
- **[Proposal]** A mapped requirement inherits only the evidence for what its image compiles in: the IP versions and the chip's glue.
