# Document index (generated)

Metadata only - read a document after choosing it, not before. 28 documents, ~8351 agent tokens. Regenerate: `python3 -I tools/docs_index.py`; `--check` fails when stale.

| class | path | size | purpose |
|---|---|---|---|
| a | AGENTS.md | 824 tok | STM32 HAL on stm32-metapac. Chips: stm32f401re, stm32f40 |
| a | docs/decisions/2026-11-08-free-teardown-brief.md | 540 tok | ADR-002 - Status: accepted |
| a | docs/decisions/2026-11-08-g5-safe-by-default.md | 603 tok | ADR-001 - Status: promoted |
| a | docs/decisions/2026-11-08-naming-scheme.md | 697 tok | ADR-003 - Status: accepted |
| a | docs/decisions/2026-11-09-uart-capabilities.md | 532 tok | ADR-004 - Status: promoted |
| a | docs/decisions/README.md | 351 tok | Record = event what, why, rejected, date, not rule. Bind |
| a | docs/decisions/index/2026.md | 185 tok | One row per record. The record itself is authoritative f |
| a | docs/design/free.md | 432 tok | There is no Pwm::free, Uart::free or Tick hand-back. Thi |
| a | docs/design/uart-ownership.md | 428 tok | Source: ADR-004, ../decisions/2026-11-09-uart-capabiliti |
| a | docs/housekeeping/README.md | 484 tok | Documents rot by addition. An agent adds a note because  |
| a | docs/housekeeping/state.md | 155 tok | Cadence: monthly, or immediately when more than ten docu |
| a | docs/notes/2026-11-08-naming-audit.md | 615 tok | Audited 2026-11-08/09. Target scheme: ../decisions/2026- |
| a | docs/notes/2026-11-08-rtic-and-hal-comparison.md | 804 tok | Code-only, 2026-11-08/09. Sources: rtic-macros-2.4.0; st |
| a | docs/policies/agent-docs.md | 296 tok | Agent docs = AGENTS.md + docs/{policies,decisions,notes, |
| a | docs/policies/documentation.md | 797 tok | Normative. Two classes: |
| a | docs/policies/safety.md | 608 tok | Normative. Goal G5 requirements/goals.md. Rationale: |
| h | DEBT.md | 24787 B | Open items only. Finished work moves to RESOLVED.mdRESOL |
| h | README.md | 4610 B | A scheduler-agnostic STM32 HAL on stm32-metapachttps://c |
| h | RESOLVED.md | 29454 B | History: items moved out of DEBT.mdDEBT.md once done. Li |
| h | THIRD-PARTY.md | 3052 B | FerroHAL is licensed under MIT OR Apache-2.0 LICENSE-MIT |
| h | docs/plans/2026-11-08-debt-sweep-gates.md | 3051 B | Part of the plan2026-11-08-debt-sweep.md. |
| h | docs/plans/2026-11-08-debt-sweep.md | 3703 B | Status: in progress - W1-W5b and W6a done watchdog debug |
| h | docs/plans/2026-11-08-free-redesign.md | 2755 B | Next project after the sweep, its own PR. steal goes und |
| h | ferrohal-architecture.md | 26005 B | Architecture only. Implementation details are decided wh |
| h | hil/WIRING.md | 4765 B | The two Nucleo benches prove FerroHAL's I2C masters agai |
| h | hil/pico-i2c-sim/README.md | 1483 B | A Raspberry Pi Pico 1 RP2040 standing in for the I2C per |
| h | requirements/benches.md | 9447 B | What each requirement's bench entry runs. A run proves t |
| h | requirements/goals.md | 916 B | Every requirement derives from one of these, or from ano |
