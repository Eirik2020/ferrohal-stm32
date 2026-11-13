# Docs against code, K1-K8 (API review)

Docs and code that disagree; ADR-003 gaps. Hub: [2026-11-11-api-review.md](2026-11-11-api-review.md). `main` `d2bdccd`.

## Findings

| id | sev | finding | evidence |
|---|---|---|---|
| K1 | M | Bootloader path breaks 2 stated absolutes; justified (one-way jump, nothing left to race), unrecorded | `README.md:7`: never "writes the NVIC, or takes a critical section". `docs/design/scope.md:33`, `ownership.md:11`: shared registers only through their owner. `reset.rs:119` `enter_bootloader`: disables interrupts, writes NVIC ICER/ICPR, SysTick, VTOR. F4 `map_system_memory` (`stm32f401re.rs:304`, `stm32f405rg.rs:373`): writes RCC APB2ENR without `&mut Rcc` |
| K2 | L | ADR-003 gap: `SIG-002` has no target | rule 2 renames `SIG-001` -> `UID-001`; `SIG-002` = die revision (`requirements/reset.yaml:102`), not a UID |
| K3 | L | ADR-003 gap: mechanism families | `SPIPOLL-*` (`spi/requirements.yaml:176`) named by mechanism like `UARTDMA-*`; SPI's DMA engine is plain `SPI-*` |
| K4 | L | Shared-source count disagrees 3 ways | ADR-003 rule 4: `iwdg.rs`, `timer*.rs`. `DEBT.md` §1 "five": `dma_stream`, `exti`, `timer`, UART + SPI engines. By DEBT's own test (one source, two IP versions through a `Unit`) nine: + `iwdg.rs`, `i2c/mod.rs`, `adc/mod.rs`, `spi/polled.rs` |
| K5 | L | Trailers: 2 formats | one line (`uart/mod.rs:20`); wrapped (`iwdg.rs:25-26`). A one-line scan misses `IWDG-007` |
| K6 | L | Catalog IDs in no `src/` trailer | `UARTDMA-011`, `-012` (`uart/requirements.yaml:493`, `:513`, added by #33); `uart/stream_dma.rs:50` lists `UARTDMA-002`, `-008`, `-009` |
| K7 | L | Stale pointers | `lib.rs:11-21` omits `adc_v2`, `adc_v4`, `sdmmc_v2`, `adc`, `sdmmc`. `lib.rs:93`: "stream DMA" comment above `adc_v2`. `AGENTS.md:18` "touch IP driver": `src/<ip>_vN/mod.rs` + its `requirements.yaml`; no per-IP directory has a catalog, most logic is neutral. `docs/plans/2026-11-08-debt-sweep-gates.md:21`: `IWDG-007` listed as next free, taken. `docs/policies/documentation.md:46`: ratchets 23943 / 36834; `tools/check_docs.py`: 22 511 / 39 326 |
| K8 | L | Doc glitches | Two doc comments merged on one item: `gpio_v2/mod.rs:39`, `sdmmc_v2/mod.rs:127`. `adc/mod.rs:48`: `Config` = "the bus settings". `uart/stream_dma.rs:110`: SAFETY names `port`; the field is `regs` |

## Fix (opinion)

- K1: name the one exception in `scope.md` + `ownership.md`; soften the README line.
- K2: a family for `SIG-002`: `SIGNATURE-*` (matches the module) or `REV-*`.
- K3: rule 2 says which way mechanism families go.
- K4: one list, in ADR-003 rule 4 and `DEBT.md`.
- K5: one trailer format; `tools/check_names.py` reads the wrapped form.
- K6: add both IDs to `uart/stream_dma.rs`'s trailer; `check_names.py` checks catalog -> trailer too.
- K7, K8: fix in place.
