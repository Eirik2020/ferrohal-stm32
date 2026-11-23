# FerroHAL - agent entry

STM32 HAL on stm32-metapac. Supported chips, limits and roadmap: `docs/CHIPS.md`.
Public repo: nothing private, no secrets, no name of a user's firmware.

## Authority

- This file is the highest authority in this repository; only the owner outranks it. A document that diverges from it is wrong: fix it. The output of a tool or script never outranks this file or reqtool.
- No agent writes to an `AGENTS.md` or `CLAUDE.md`, here or in any folder, without the owner's review of the exact text in chat and in the PR. Never split one.
- Normative: `docs/policies/{safety,features,documentation,agent-docs}.md`, `docs/design/`, `requirements/goals.md`.
- The old workspace handover is archived (restricted): history, not rules.

## Read when

| task | docs |
|---|---|
| change public API | `docs/policies/safety.md`, `docs/design/api-surface.md` |
| add a feature or adapter | `docs/policies/features.md` (G1, G2; ADR-007) |
| add/move requirements | `docs/policies/documentation.md`, `requirements/goals.md`, `src/requirements/<ip>.yaml` |
| fix/close debt | `docs/DEBT.md` (tagged; each item's origin), `docs/resolved/` -> owning file |
| what chips support; limits; roadmap | `docs/CHIPS.md` |
| silicon errata, a workaround | `docs/design/errata.md` (ADR-010), `docs/errata/`; check the erratum, never a `REV_ID` |
| what is verified, and on what evidence | `reqtool check` |
| what the rig can and cannot measure | `hil/CAPABILITIES.md` |
| touch IP driver | `src/<ip>/` (neutral driver, or the IP's own folder: ADR-012), `src/<ip>_vN/mod.rs` (register wrapper) + `src/requirements/<ip>.yaml` |
| add/change a user example | `examples/README.md`; built by `checks/run.sh` |
| run/extend benches | `hil/WIRING.md`, `requirements/benches.md`, run module doc |
| plan work | `docs/DEBT.md`; `docs/CHIPS.md` roadmap; open plan: `docs/plans/2026-11-17-f411-f722-f745.md` |
| naming/catalogs/IDs | `docs/design/naming.md`; `tools/check_names.py` holds it |
| chip glue, parts, dies, packages | `docs/design/glue-generator.md` (ADR-008), `tools/glue-gen` |
| maturity, tiers, evidence axes | `docs/design/maturity.md` (ADR-009) |
| API shape/renames/reuse | `docs/notes/2026-11-11-api-review.md` (hub) |
| unsafe/registers | `docs/policies/safety.md`, `docs/decisions/README.md`, `docs/notes/2026-11-08-rtic-and-hal-comparison.md` |
| record/find decision | `docs/decisions/README.md`; find by tag: `python3 -I tools/adr.py find <tag>` |
| find a document | `docs/index/README.md` -> `docs/index/<area>.md` (generated; `index.json` for tools) |
| change design | `docs/design/<topic>.md`, decisions hub |
| after housekeeping date | `docs/housekeeping/README.md`, `state.md`, `tools/housekeeping.py` |

## Commands

```
checks/run.sh                    # clippy x2 + rustdoc/chip, no-panic, safe-api, names, no-metapac, glue, docs, host
python3 -I tools/check_docs.py   # byte budgets, orphans, decisions, housekeeping due
python3 -I tools/docs_index.py   # regenerate docs/index/
python3 -I tools/docs_read.py <path> [section]  # one section, 400-token cap
python3 -I tools/housekeeping.py # the review report
hil/run-f401.sh [runs]           # Nucleo-F401RE
hil/run-h753.sh [runs]           # Nucleo-H753ZI
hil/run-discovery.sh [runs]      # STM32F4 Discovery
hil/flash-dfu.sh <board>         # a DFU-flashed board (`FLSH` tier): Foxeer, Lucid
hil/pico-i2c-sim.sh [flash|run]  # Pico 1 I2C slave, both benches
python3 -I tools/rig_run.py hil/run-<bench>.sh [runs]  # the same run, timed into the rig telemetry
rig <script> [args] | rig list | rig locks  # any hil/ script, one holder per board, pins, telemetry
python3 -I tools/rig_report.py [--since 24h] [--full]  # where the rig's time goes (hil/RIG.md)
```

Before every push, run what CI would fail the change on, with `REQTOOL` set to a reqtool
built at CI's `REQTOOL_REV` (`.github/workflows/ci.yml`):
- Docs only (Markdown outside `src/`, `requirements/` and `hil/`, but not `docs/CHIPS.md`):
  `tools/check_names.py`, `tools/docs_index.py --check`, `tools/check_docs.py` and
  `tools/check_decision_labels.py --diff origin/main`, the only steps such a change can break.
- Anything else (code, glue, chip data, requirements, evidence, procedures, benches, the
  generated tables, manifests): the full `checks/run.sh` and `tools/glue-gate.sh` without
  `--no-tiers`; `--no-tiers` skips the tier tables CI checks, and run.sh skips reqtool when it
  finds none.

A local pass is not "fixed": only green repo CI on the pushed head is. When repo CI fails,
read the failing jobs' own logs first (`gh run view <id> --log-failed`, or the job log) and fix
every failure they show before pushing again; never guess from a local run.

CI trap: a gitleaks CDN 504 fails Publication security spuriously; `gh run rerun <id> --failed`,
never change the workflow. Bench traps: watchdog/bootloader runs need `--no-catch-reset`; run M + DFU use the H753's
CN13 cable, which stays connected; probes by serial through `hil/probe.sh` (`F401_PROBE`,
`H753_PROBE`, or `PROBE_RS_PROBE`; one probe attached needs none). The rig's
capabilities, blind spots and improvement queue are in `hil/CAPABILITIES.md`.

## Rules

1. Car rule (`docs/design/requirements-system.md`): one chip proves an IP version for every family, one chip a family's shared glue; exhaustive tests need the owner's accepted rationale. Evidence follows files, not images; three axes (IP, die, series), tiers `BRD`/`HIL`/`DBG`/`FLSH`/`INH` (`docs/design/maturity.md`).
2. Debt (`docs/DEBT.md`) = owed work on what ships, tagged, with its recorded origin. A
   feature not built or a silicon limit goes to `docs/CHIPS.md`; nothing names a user
   firmware (toaster rule, absolute). An item leaves only when hardware proves the fix;
   one no bench can reach stays in §4 (`rig`).
3. Safety (G5, `docs/policies/safety.md`): no public `unsafe fn` except `Peripherals::steal` behind `rtic`. No unsafe wrapper around a register write: a user who needs registers takes the metapac. Only the token-owning module touches its registers, and metapac imports stay confined. `tools/check_safe_api.py` holds it.
4. One `shall` per requirement; module trailer names its IDs.
5. Reruns: reqtool alone says what a change owes. No bench run for a merge, a conflict fix, a pin or lock-file bump, or a document or comment change; rerun only a bench whose own code path changed.
6. Rig: flash or run a board only under the owner's grant for that board, given once per board and valid for every thread.
7. Decisions: when the owner states a decision, record it as an ADR (`docs/decisions/AGENTS.md`) in the same change, even inside a code task. Code, comments and docs state the rule, never who decided it or when.
8. `DEBT.md`/`resolved/` = pointer indexes, no prose.
9. Nothing documented only in commit/PR/chat.
10. Docs: human vs agent classes; 20000 B always-loaded, 40000 B otherwise (`docs/policies/documentation.md`); split, never compress; a split is linked, never orphaned.
11. Housekeeping forced: `state.md` due date or +10 docs -> `tools/housekeeping.py`, record pass.
12. Root: README, AGENTS, CLAUDE, THIRD-PARTY, licences, manifests, and uncommitted temporary `.md` only; docs live in `docs/`.
13. Don't touch `flash-backups/`, `requirements/evidence/`.
14. `review/` (owner's reviews + drafts, uncommitted): read only; no write of any kind without the owner's explicit permission.

## Style

rustfmt; one commit per item, detailed body; dates YYYY-MM-DD; British prose only in
human docs.
