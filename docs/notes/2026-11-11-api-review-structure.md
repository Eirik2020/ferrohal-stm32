# Files, modules and tests, S1-S7 (API review)

File placement, glue sharing, tests. Hub: [2026-11-11-api-review.md](2026-11-11-api-review.md). `main` `d2bdccd`.

## Findings

| id | sev | finding | evidence |
|---|---|---|---|
| S1 | M | F4 glue repeats what H7 glue shares | H7: `chips/shared/h7.rs`. F4: no shared file; identical `VREFIN_CAL`, `TS_CAL1/2`, `UID_BASE`, `SYSTEM_MEMORY`, `map_system_memory`, `SYSCFG_ENABLE` (`stm32f401re.rs:293-311`, `stm32f405rg.rs:362-380`) and IWDG helpers. Top-level items that differ: `DMA_RAM` size, `FRACTIONAL_STOP_BITS`, `CLOCK_LIMITS`, `BONDED`, token lists. Line counts: U1 |
| S2 | L | Submodules sit beside their parent; directories that hold one file | `timer_pwm.rs`, `timer_awg.rs` are children of `timer` by `#[path]` (`timer.rs:32`, `:36`). `dma_stream.rs`: the name appears nowhere else (module = `dma_v1`/`dma_v2`). 12 per-IP directories hold a lone `mod.rs`. ADR-003 rule 4 accepts timer + iwdg as one-file exceptions |
| S3 | L | UART engine = one 1,674-line file | `uart/stream_dma.rs`: state, traits, `Uart`, `Line`, `UartTx` (`:637-855`), `UartRx` (`:925-1346`), `UartIrq` (`:1388-1494`), `RESOURCES` (`:1524-1593`), `embedded-io` impls. Next: `rcc_h7rm0433/mod.rs` 1,160, `timer_pwm.rs` 1,039 |
| S4 | L | `Wiring::RESOURCES` placement differs | in the engine: UART, SPI; beside the `Wiring` struct: I2C, ADC |
| S5 | L | Clock-solver tests live in chip files, named by board | `rcc_f4` (910 lines), `rcc_h7rm0433` (1,160): no test module. `solve_matches_the_proven_tree` (`stm32f401re.rs:589`), `_the_foxeer_tree` (`stm32f405rg.rs:779`), `_the_lucid_tree` (`stm32h743vi.rs:824`). Tests per chip file: 2, 7, 5, 1 (H753) |
| S6 | L | Known: chip exclusion = one `compile_error!` listing every pair | `lib.rs:47-55`: 6 pairs at 4 chips, 15 at 6 (`DEBT.md` §1) |
| S7 | L | Code leftovers | `timer.rs:381` `Tick::stop` and `:390` `Tick::free` repeat the same 3 writes. `rcc_f4/mod.rs:133`, `:156`: `enable`/`reset` hand-written 4-arm matches; H7 uses `bus_registers!` (`rcc_h7rm0433/mod.rs:140`) |

## Fix (opinion)

- S1: `chips/shared/f4.rs`, before the next family copies the pattern. Owner 2026-11-11: routing generator first; applies to routing rows, constants could move earlier.
- S2: `src/timer/{mod,pwm,awg}.rs`, `src/dma/mod.rs`, still pulled in twice by `#[path]`; then a directory means "has children".
- S3: split along its types: `tx.rs`, `rx.rs`, `irq.rs`.
- S4: `RESOURCES` beside the struct.
- S5: keep the flight boards' proven trees as fixtures; give naming + placement a rule.
- S7: `free` calls `stop`; F4 uses the macro (U2).
