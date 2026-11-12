# FerroHAL - agent entry

STM32 HAL on stm32-metapac. Chips: stm32f401re, stm32f405rg, stm32h743vi, stm32h753zi.
Public repo. Bench-verified.

## Authority

- This file governs this tree only. Workspace `CLAUDE.md` = siblings. `ferrohal-handover.md` = history, not rules.
- Normative: `docs/policies/{safety,documentation,agent-docs}.md`, `docs/design/`, `requirements/goals.md`.

## Read when

| task | docs |
|---|---|
| change public API | `docs/policies/safety.md`, `docs/design/api-surface.md` |
| add/move requirements | `docs/policies/documentation.md`, `requirements/goals.md`, an `src/**/requirements.yaml` |
| fix limitation/close debt | `DEBT.md`, `RESOLVED.md` (indexes) -> owning file |
| touch IP driver | `src/<ip>_vN/mod.rs` + its `requirements.yaml` |
| run/extend benches | `hil/WIRING.md`, `requirements/benches.md`, run module doc |
| plan work | `docs/plans/2026-11-08-w14-naming.md` (next), `docs/plans/2026-11-08-free-redesign.md`, `docs/plans/2026-11-08-debt-sweep.md` (+`-gates`) |
| naming/catalogs | `docs/decisions/README.md` (ADR-003), `docs/notes/2026-11-08-naming-audit.md` |
| API shape/renames/reuse | `docs/notes/2026-11-11-api-review.md` (hub) |
| unsafe/registers | `docs/policies/safety.md`, `docs/decisions/README.md`, `docs/notes/2026-11-08-rtic-and-hal-comparison.md` |
| record/find decision | `docs/decisions/README.md` -> `docs/decisions/index/<year>.md` |
| find a document | `docs/index/README.md` -> `docs/index/<area>.md` (generated; `index.json` for tools) |
| change design | `docs/design/<topic>.md`, decisions hub |
| after housekeeping date | `docs/housekeeping/README.md`, `state.md`, `tools/housekeeping.py` |

## Commands

```
checks/run.sh                    # clippy x2/chip, no-panic, safe-api, no-metapac, glue, docs, host
python3 -I tools/check_docs.py   # byte budgets, orphans, decisions, housekeeping due
python3 -I tools/docs_index.py   # regenerate docs/index/
python3 -I tools/docs_read.py <path> [section]  # one section, 400-token cap
python3 -I tools/housekeeping.py # the review report
hil/run-f401.sh [runs]           # Nucleo-F401RE
hil/run-h753.sh [runs]           # Nucleo-H753ZI
hil/run-dfu.sh <board>
hil/run-i2c-sim.sh [flash|run]   # Pico 1 I2C slave, both benches
```

Bench traps: watchdog/bootloader runs need `--no-catch-reset`; run M + DFU need H753 CN13
cable (Pico borrows it); probes by serial through `hil/probe.sh` (`F401_PROBE`,
`H753_PROBE`, or `PROBE_RS_PROBE`; one probe attached needs none). The rig's
capabilities, blind spots and improvement queue are in `hil/CAPABILITIES.md`.

## Rules

- Evidence: a debt item leaves `DEBT.md` only when hardware proves the fix. A fix no
  bench can reach stays open as Deferred (`DEBT.md` §7), ranked below every testable
  item.
- G5: no `unsafe fn` public; only `Peripherals::steal` behind `rtic`. Registers -> take metapac.
- No unsafe wrappers around register writes; token-owning module only; PAC imports confined.
- One `shall` per requirement; module trailer names its IDs.
- `DEBT.md`/`RESOLVED.md` = pointer indexes, no prose.
- Nothing documented only in commit/PR/chat.
- Docs: human vs agent classes; 20000 B always-loaded, 40000 B otherwise (`docs/policies/documentation.md`); split, never compress; a split is linked, never orphaned.
- Housekeeping forced: `state.md` due date or +10 docs -> `tools/housekeeping.py`, record pass.
- Don't touch `flash-backups/`, `requirements/evidence/`. `ferrosmith-board-schema.md` stays untracked.
- Workspace habits: `../agent-memory/MEMORY.md`.

## State 2026-11-13

Debt sweep W1-W13 done (#29); refusal runs (#31) and fault injection (#33) landed. The
`free()` redesign is done: every token-owning handle hands its token back
(`docs/design/free.md`), on branch `free-redesign-close`. Next: W14 naming, its own PR.
Details `docs/plans/`.

## Style

rustfmt; one commit per item, detailed body; dates YYYY-MM-DD; British prose only in
human docs.
