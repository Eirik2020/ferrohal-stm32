# Documentation policy

Normative. Two classes:

- **Human-readable** (prose, citations, people read them): `requirements/**` (`goals.md`,
  `benches.md`, `evidence/`), `src/**/requirements.yaml`, `ferrohal-architecture.md`,
  `DEBT.md`, `RESOLVED.md`, `README.md`, `THIRD-PARTY.md`, `hil/WIRING.md`, `hil/*/README.md`.
- Agent-facing (token-optimised, `agent-docs.md` style): `AGENTS.md`,
  `docs/{policies,decisions,notes,housekeeping,design}/**`.
- **Plans are human-readable**: `docs/plans/**` uses prose and the byte budget below.

## Where behaviour lives

| statement | home |
|---|---|
| RM/DS/UM/AN fact shaping code | IP module doc, cited by doc+section (`RM0433 §47.4.9 Table 389`, `ES0299 2.10.1`, `DS12110`, `AN2606`, UM1724/UM2407 in `hil/WIRING.md`) |
| vendor fact binding the caller | that item's `# Safety`/limits text |
| bench measurement of the device | `RESOLVED.md` (dated, measured), run module doc, `requirements/benches.md`, `requirements/evidence/`; driver module doc once it changes a contract |
| bench fact about the instrument | that instrument's crate docs |
| tooling workaround | script comment + `requirements/benches.md`; `DEBT.md` pointer only |
| open limitation | `DEBT.md`: severity, one sentence, pointer, verification |
| requirement | owning `requirements.yaml`, one `shall`, verified by a `benches.md#anchor` or host test |

Measurement wins for what the code does; record both; divergence -> erratum candidate in
`DEBT.md`. Nothing documented only in commit/PR/chat.

## Requirements

- A file states its requirements; the `Requirements:` trailer names the IDs; architecture §9 hashes it.
- One `shall` per statement. A single-part requirement gets its own family (`USARTV4-001`).

## Decision records

Event, not rule. `ADR-NNN - Status:` line; statuses `accepted|promoted|superseded|rejected`.
Promotion: policy -> `docs/policies/`; design -> `docs/design/<topic>.md`; behaviour ->
`requirements.yaml`. Promoted records carry `Promoted to: [path](path)` (resolved by the checker).
Exactly one `index/<year>.md` row per record; row id+status must match. Superseded/rejected
move verbatim to `archive/<year>/`. Active records capped at 20. Lifecycle:
`docs/decisions/README.md`.

## Limits

- Agent docs - estimated tokens (4 B/token): `AGENTS.md` 1100; policies 750; decision hub 500;
  decision 750; decision index 400; note 850; housekeeping 500; design topic 750.
- Human docs - bytes: plans 6000; benches 12000; README/THIRD-PARTY/WIRING 8000.
- Ratchets (human): `DEBT.md` 24929, `RESOLVED.md` 28955, map 26005. `DEBT.md` and the map
  shrink only; `RESOLVED.md` grows by each resolution, raising its ratchet then.
- Over a limit: **split into a second document**; never compress, never drop a fact.
- Checker: `tools/check_docs.py` in `checks/run.sh`; workspace:
  `agent-memory/tools/check_agent_docs.py`.
- Generated `docs/index.json` + `docs/INDEX.md`: `tools/docs_index.py` (`--check` fails stale).
  Section reads: `tools/docs_read.py <path> [section]`, 400 tokens.

## Housekeeping

`docs/housekeeping/state.md` due date, or +10 documents since the last review ->
`tools/housekeeping.py`, then record the pass in `state.md`.
