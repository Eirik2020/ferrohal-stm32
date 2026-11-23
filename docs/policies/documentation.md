# Documentation policy

Normative. Two classes:

- **Human-readable** (prose, citations, people read them): `requirements/**` (`goals.md`,
  `benches.md`, `evidence/`), `src/requirements/*.yaml`,
  `docs/DEBT.md`, `docs/resolved/*.md`, `docs/CHIPS.md`, `README.md`, `THIRD-PARTY.md`, `hil/*.md`, `hil/*/README.md`.
- Agent-facing (token-optimised, [agent-docs.md](agent-docs.md) style): `AGENTS.md`,
  `docs/{policies,decisions,notes,housekeeping,design}/**`.
- **Plans are human-readable**: `docs/plans/**` uses prose. A finished plan is deleted: its
  useful facts move into the documents that own them first, and its commits and PR keep
  the record (owner, 2026-11-13).

## Where behaviour lives

| statement | home |
|---|---|
| RM/DS/UM/AN fact shaping code | IP module doc, cited by doc+section (`RM0433 §47.4.9 Table 389`, `ES0299 2.10.1`, `DS12110`, `AN2606`, UM1724/UM2407 in `hil/WIRING.md`) |
| vendor fact binding the caller | that item's `# Safety`/limits text |
| bench measurement of the device | `docs/resolved/` (dated, measured), run module doc, `requirements/benches.md`, `requirements/evidence/`; driver module doc once it changes a contract |
| bench fact about the instrument | that instrument's crate docs |
| tooling workaround | script comment + `requirements/benches.md`; `DEBT.md` pointer only |
| open debt: owed work on what ships | `docs/DEBT.md`: tags, severity, what is wrong and the fix, the recorded origin (*Spawned:*, never reconstructed) |
| silicon limit or documented contract | `docs/CHIPS.md` Limits, and the item's own doc |
| feature not built | `docs/ROADMAP.md` (the roadmap), never `DEBT.md` |
| what a bench proves | `hil/COVERAGE.md` |
| a user firmware's need | that firmware's repository, never here (toaster rule) |
| review or audit finding | the review's note (evidence, proposed fix); `docs/DEBT.md` a tagged row pointing at it, added when the review lands: DEBT is the one ledger of open work |
| requirement | its IP's catalog, `src/requirements/<ip>.yaml`, one `shall`, verified by a `benches.md#anchor` or host test |

Measurement wins for what the code does; record both; divergence -> erratum candidate in
`DEBT.md`. Nothing documented only in commit/PR/chat.

## Requirements

- A file states its requirements; the `Requirements:` trailer names the IDs; `docs/design/requirements-system.md` hashes it.
- One `shall` per statement. A single-part requirement gets its own family (`USART-V4-001`).

## Decision records

Event, not rule. `ADR-NNN - Status:` line; statuses `accepted|promoted|superseded|rejected`.
Promotion: policy -> `docs/policies/`; design -> `docs/design/<topic>.md`; behaviour ->
a catalog in `src/requirements/`. Promoted records carry `Promoted to: [path](path)` (resolved by the checker).
Exactly one `index/<year>.md` row per record; row id+status must match. Superseded/rejected
move verbatim to `archive/<year>/`. Active records capped at 20. Lifecycle:
`docs/decisions/README.md`.

## Decision labels

- A decision is a record under `docs/decisions/`; the label forms that
  `docs/decisions/AGENTS.md` rule 1 names appear nowhere else, in documents or in code
  comments.
- `tools/check_decision_labels.py --diff <base>` fails on a label in a line the change adds,
  moves or re-dates, and prints the rule: write the record or remove the line, do not reword.
  Without `--diff` it lists every label in the tree and passes. Wording that may be a
  reworded label (the owner, then a decision verb) only warns; the owner reviews those.
- CI runs it on every pull request and keeps one comment there with the flagged lines and
  the warnings. The comment is edited, never deleted.

## The repository root

- Owner, 2026-11-13: the root holds only the files a repository is expected to have there
  (`README.md`, `AGENTS.md`, `THIRD-PARTY.md`, the licences, the Cargo and tool
  manifests) and temporary markdown that is never committed. Review records and
  architecture drafts go in `review/`, which git ignores whole and agents may not write
  to without the owner's explicit permission (its local `README.md` and `AGENTS.md`).
  Every other document lives under `docs/` or beside what it describes (`hil/`,
  `requirements/`); the debt registries are `docs/DEBT.md` and `docs/resolved/*.md`.
- `tools/check_docs.py` fails a committed root `.md` outside that list, and treats an
  uncommitted root `.md` as temporary: no budget, no index.
- Agent entry files are the exception: `CLAUDE.md` at the root, and an `AGENTS.md` with a
  `CLAUDE.md` beside it in a folder whose rules bind work in that folder. A `CLAUDE.md`
  holds `@AGENTS.md` and nothing else; it is there because Claude Code loads an attached
  repository's or a subfolder's `AGENTS.md` only through it. Entry files take the
  always-loaded budget and need no inbound link.

## Limits

- Bytes only, set by the owner 2026-11-13: always-loaded files (`AGENTS.md`) 20000 B;
  every other document 40000 B. No line or token caps, no ratchets.
- Over a limit: **split into a second document**, linked from the first; never
  compress, never drop a fact.
- No orphans: every document is linked, or named by its path, from another document,
  the code or a requirement file; entries (`AGENTS.md`, `README.md`, `docs/DEBT.md`,
  `docs/resolved/README.md`, the decision and housekeeping hubs) and the decision indexes aside.
- Checker: `tools/check_docs.py` in `checks/run.sh` (sizes and orphans, both fail);
  workspace: `agent-memory/tools/check_agent_docs.py`.
- Generated `docs/index/` (hub, one file per area, `index.json`): `tools/docs_index.py` (`--check` fails stale).
  Section reads: `tools/docs_read.py <path> [section]`, 400 tokens.

## Housekeeping

`docs/housekeeping/state.md` due date, or +10 documents since the last review ->
`tools/housekeeping.py`, then record the pass in `state.md`.
