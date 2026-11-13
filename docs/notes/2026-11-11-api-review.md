# API review: consistency, naming, structure, reuse (hub)

Merged 2026-11-11, every fact re-measured at `main` `d2bdccd`, from:
- review 2026-11-10 at `7d1af83` (`review/REVIEW-2026-11-10-consistency.md`, uncommitted; host checks only);
- scan 2026-11-11: all 41 `src/` files (17 657 lines), `hil/`, `tools/`, both Nucleo benches.

Scope: Rust API shape. ADR-003/W14 covers catalogs, IDs, features, scripts, doc names.
Format: finding = fact with `file:line`; `Fix` = opinion, owner decides; sev M medium, L low.

## Parts

| ids | file |
|---|---|
| D1-D5 | [driver traits](2026-11-11-api-review-driver-traits.md) |
| D6-D13 | [driver behaviour](2026-11-11-api-review-driver-behaviour.md) |
| N1-N5 | [naming rules](2026-11-11-api-review-naming.md) |
| N6-N19 | [naming drift](2026-11-11-api-review-naming-drift.md) |
| S1-S7 | [files, modules, tests](2026-11-11-api-review-structure.md) |
| U1-U11 | [duplication](2026-11-11-api-review-reuse.md) |
| K1-K8 | [docs vs code](2026-11-11-api-review-docs.md) |
| B1-B3 + baseline | [bench runs](2026-11-11-api-review-bench.md) |

## Already consistent (the reference)

- Constructors: `new(&mut Rcc, wiring or token, &Config or rate, buffers?) -> Result<_, Refused<E, inputs>>`: UART, rx-only UART, SPI, `BlockingSpi`, I2C, ADC, SDMMC, USB, `Tick`, `Timebase`, `Pwm`, the watchdog starts, `freeze`.
- Shared-register owners: `Ctrl` in GPIO and EXTI; `&mut Rcc` everywhere.
- Wait-bound suffixes: `*_SPINS` register polls, `*_CYCLES` CPU cycles, `*_TRIES` retries.
- Builders `Config::new(x).with_*()` (UART, SPI); validation is `const fn`.
- 120 catalog IDs, unique; every trailer ID resolves.

## Changed from the review's figures

Per-IP directories with a lone `mod.rs`: 12, not 7. Shared sources: 9, not 8. Catalog IDs:
120, not 118 (#33). Matching run letters: S and W. `timer.rs`, `i2c/mod.rs` lines: +6 (#35).
"Interrupt service consistent" -> D12, D13. "F7, G4 next": not recorded in `docs/`.

## Decisions

- Owner 2026-11-11: routing generator before chip-glue sharing (U1, S1).
- Owner 2026-11-11: generated index split per area into `docs/index/`.
- ~~Open: driver genericity (D5)~~ closed 2026-11-13: every handle carries its token (`docs/design/free.md`).
- Open: run identity (B1).

## Status 2026-11-13

Closed: D5, K2, K3, K5-K8 (`RESOLVED.md`). The other 54 are open, one line each in
`docs/DEBT.md` §2, which is the ledger; these notes keep the evidence and the fixes.

## Order (opinion)

W14 is the breaking rename PR and nothing is on crates.io:
1. D6 + D7: shared DMA error.
2. D3 + D1: one driver shape; board code chip-neutral; halves what the generator must know.
3. D2: `rcc::Kernel`.
4. Renames: N1, N3, N4, then N6-N19.
5. ADR-003: K2-K5; rule 5 gains N2, N5.
6. K1: name the bootloader exception.
7. Checker: `check_names.py` reads wrapped trailers, catalog -> trailer; clippy `used_underscore_binding`, `module_name_repetitions` with an allow-list.

Any time: U7, U8, U5 (mechanical) -> U2, U4 -> U3, U6, U9, U10 (set structure: consult) ->
S2-S5, K6-K8, B1. After the generator: U1, S1.
Gate each: `checks/run.sh` + both Nucleo benches.
