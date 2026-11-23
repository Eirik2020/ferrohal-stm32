# Decision records (ADR hub)

Record = event (what, why, rejected, date), not rule. Binds -> promote: policy ->
`docs/policies/`; design -> `docs/design/<topic>.md`; behaviour ->
`src/requirements/<ip>.yaml`. Record keeps `Status: promoted` + `Promoted to: [path](path)`.
`AGENTS.md` points here, at policies and at the design topics - never at a record.

Index: `index/<year>.md`. Policies: `../policies/`. Plans: `../plans/`.

## Lifecycle

- Statuses: `accepted`; `promoted`; `superseded` (names the replacement); `rejected` (kept, so it is not re-argued); `obsolete` (records an event that binds nothing now).
- Every record: `ADR-NNN - Status: ...` after the title; exactly one index row; row id and status must match the record.
- Promoted records: `Promoted to: [path](path)`, resolved by the checker.
- Superseded/rejected/obsolete -> `archive/<year>/` verbatim. Nothing deleted; archived records not loaded by default.
- Active cap 20; over -> promotion/archive pass.
- Promotion = record status edit + target edit; the record keeps the "why".

## Checker

`tools/check_docs.py`: byte budgets, no orphans, status line, promoted target, unique ADR IDs,
archive placement, index agreement both ways, active cap, no dated record in `AGENTS.md`,
every relative `.md` link in `docs/` and `AGENTS.md` resolves. Through `tools/adr.py`: each
record's format ([rules 2-4](AGENTS.md)) and every index row's tags, from
[the approved list](tags.txt).

Find by tag: `python3 -I tools/adr.py find <tag>...`; `python3 -I tools/adr.py tags` lists them.
Reads the index only.

## No script holds

Fidelity (the target really contains the distilled design); one topic per document; moved,
never compressed. Review items - state them in the PR.
