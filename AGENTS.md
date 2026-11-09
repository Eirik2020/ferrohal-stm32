# FerroHAL - agent entry

STM32 HAL on stm32-metapac. Chips: stm32f401re, stm32f405rg, stm32h743vi, stm32h753zi.
Public repo. Bench-verified.

## Authority

- This file governs this tree only. Workspace `CLAUDE.md` = siblings. `ferrohal-handover.md` = history, not rules.
- Normative: `docs/policies/{safety,documentation,agent-docs}.md`, `ferrohal-architecture.md`, `requirements/goals.md`.

## Read when

| task | docs |
|---|---|
| change public API | `docs/policies/safety.md`, architecture §5 |
| add/move requirements | `docs/policies/documentation.md`, `requirements/goals.md`, an `src/**/requirements.yaml` |
| fix limitation/close debt | `DEBT.md`, `RESOLVED.md` (indexes) -> owning file |
| touch IP driver | `src/<ip>_vN/mod.rs` + its `requirements.yaml` |
| run/extend benches | `hil/WIRING.md`, `requirements/benches.md`, run module doc |
| plan work | `docs/plans/2026-11-08-debt-sweep.md` (+`-gates`), `docs/plans/2026-11-08-free-redesign.md` |
| naming/catalogs | `docs/decisions/README.md` (ADR-003), `docs/notes/2026-11-08-naming-audit.md` |
| unsafe/registers | `docs/policies/safety.md`, `docs/decisions/README.md`, `docs/notes/2026-11-08-rtic-and-hal-comparison.md` |
| record/find decision | `docs/decisions/README.md` -> `docs/decisions/index/<year>.md` |
| find a document | `docs/INDEX.md` (generated; `docs/index.json` for tools) |
| change design | `ferrohal-architecture.md` (map), `docs/design/<topic>.md`, decisions hub |
| after housekeeping date | `docs/housekeeping/README.md`, `state.md`, `tools/housekeeping.py` |

## Commands

```
checks/run.sh                    # clippy x2/chip, no-panic, glue, docs, host tests
python3 -I tools/check_docs.py   # budgets/tokens, decisions, housekeeping due
python3 -I tools/docs_index.py   # regenerate docs/INDEX.md + docs/index.json
python3 -I tools/docs_read.py <path> [section]  # one section, 400-token cap
python3 -I tools/housekeeping.py # the review report
hil/run-f401.sh [runs]           # Nucleo-F401RE
hil/run-h753.sh [runs]           # Nucleo-H753ZI
hil/run-dfu.sh <board>
hil/run-i2c-sim.sh [flash|run]   # Pico 1 I2C slave, both benches
```

Bench traps: watchdog/bootloader runs need `--no-catch-reset`; run M + DFU need H753 CN13
cable (Pico borrows it); probes by VID:PID 0483:374b/374e (W8 -> serial).

## Rules

- G5: no `unsafe fn` public; only `Peripherals::steal` behind `rtic`. Registers -> take metapac.
- No unsafe wrappers around register writes; token-owning module only; PAC imports confined.
- One `shall` per requirement; module trailer names its IDs.
- `DEBT.md`/`RESOLVED.md` = pointer indexes, no prose.
- Nothing documented only in commit/PR/chat.
- Docs: human vs agent classes + budgets/tokens per `docs/policies/documentation.md`; split, never compress.
- Housekeeping forced: `state.md` due date or +10 docs -> `tools/housekeeping.py`, record pass.
- Don't touch `flash-backups/`, `requirements/evidence/`. `ferrosmith-board-schema.md` stays untracked.
- Workspace habits: `../agent-memory/MEMORY.md`.

## State 2026-11-09

Docs system merged (PR #7); RTIC T1-T3 confirmed; debt sweep W1-W2 done. Next: W3-W13,
then the `free()` redesign. Details `docs/plans/`.

## Style

rustfmt; one commit per item, detailed body; dates YYYY-MM-DD; British prose only in
human docs.
