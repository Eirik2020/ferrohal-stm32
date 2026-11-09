# Decision: naming scheme (2026-11-08)

ADR-003 - Status: accepted

Applied in the `free()` redesign PR as **W14**. Promotion target when W14 lands: a topic
design document at `docs/design/naming.md` (the chip-to-feature table and the naming
rules), linked from `ferrohal-architecture.md` §2 - a **design** promotion. Audit with
file-and-line evidence: [`../notes/2026-11-08-naming-audit.md`](../notes/2026-11-08-naming-audit.md).

## Context

A review found the naming inconsistent in seven groups: catalogs in two locations with
names that do not match their contents (`platform.yaml` holds RCC/GPIO/DMA/WAIT/RES;
`reset.yaml` holds SIG; `GLUE-*` live in the UART catalog); three ID prefixes for one
peripheral (`UART`, `USART`, `UARTDMA`); `RES-001` beside `RESET-001..004`; wildly uneven
requirement granularity; `dma_v1`/`dma_v2` numbered opposite to every other IP;
`rcc_h7rm0433` as the only family+manual feature name; three module patterns; and
script/crate/document case conventions that do not agree.

## Decision

1. **Catalogs**: one family per file, all under `src/requirements/<family>.yaml`; split
   `platform.yaml` into `rcc`, `gpio`, `dma`, `wait`, `resource`; `SIG-*` ->
   `signature.yaml`; `GLUE-*` -> `glue.yaml`; the four co-located catalogs move in.
2. **IDs**: `<FAMILY>-NNN`; `RES-001` -> `RESOURCE-001`; `SIG-001` -> `UID-001`; merge
   `USART-*` into `UART-*`, keeping per-part families (`USARTV4-*`, `SPIV2-*`, `I2CV1-*`);
   `UARTDMA-*` stays.
3. **Features**: `<ip>_v<metapac version>` where metapac versions the IP, `<ip>_<family>`
   where FerroHAL needs a family shape; a documented chip -> feature table in
   `ferrohal-architecture.md` §2; `rcc_h7rm0433` -> `rcc_h7_rm0433` (or `rcc_h7`); state
   explicitly that metapac's `dma_v1`/`dma_v2` numbering is inverted (F4 = v2, H7 = v1).
4. **Modules**: neutral module plus per-IP dirs for every new IP; `iwdg.rs` (one file,
   two names) and `timer*.rs` (split by function) documented as deliberate exceptions.
5. **Vocabulary**: *channel* (timer compare), *lane* (DMA-fed output), *stream* (DMA),
   *line* (UART), *handle* (irq/health); `set_*` versus `into_*`; `*_us`/`*_hz`.
6. **Benches/scripts**: `run-<bench>.sh`; run letters uppercase in prose, lowercase in
   file names.
7. **Docs**: caps for registries (`DEBT`, `RESOLVED`, `README`, `WIRING`), kebab-case
   for prose.
8. **Checker**: `tools/check_names.py` in `checks/run.sh` - every `Requirements:` ID
   resolves to exactly one catalog entry, no duplicate IDs, catalog name matches its
   family, feature names match the documented pattern.

## Consequence now

The debt sweep's interrupt-masking requirement is **`USART-005`**: `USART-001..004`
already exist (`src/uart/requirements.yaml:485-542`). This collision is why rule 8 exists.
