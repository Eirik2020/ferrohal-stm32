# Housekeeping state

- Last review: 2026-11-23
- Next due: 2026-12-23
- Documents at last review: 99

Cadence: monthly, or immediately when more than ten documents have been added since the
last review. The owner changes the cadence by editing the two dates above.

## Reviews

- 2026-11-23 - forced by the cap (87 documents at the last review, 99 now). No orphans. The
  duplicate hints are all kept: the two `CLAUDE.md` files each import their folder's
  `AGENTS.md`; ADR-017 and the archived ADR-003 are a record and the one it supersedes;
  the errata tables are one per sheet; the two `resolved/` parts and the two bench
  procedure files are splits for the size budget. Decisions: 14 active against a cap of
  20, every one in the record format (`tools/adr.py`); ADR-003 superseded by ADR-017 and
  archived; ADR-013 and ADR-020 are accepted and not promoted, which is the open question
  the report raises. Plans and notes: none older than 90 days; the F411/F722/F745 plan
  is still open. Decision labels: gone from every working document; four dated notes
  keep theirs as history. The workspace pass does not apply: this repository has no
  `agent-memory/`.
- 2026-11-17 - owner-requested sweep after #52 to #55 (+5 documents, none added by
  the pass). README and AGENTS brought level with ADR-011 (one owner, `Service`), ADR-012
  (one folder per IP), the F407VG Discovery bench, `examples/` and rig telemetry; two dead
  links in `requirements/benches.md` and one in `resolved/` fixed; `free-shapes.md`'s UART
  entries, the S2 resolved entry and the API-review hub marked superseded or closed; DEBT's
  status line notes the owed F4/H7 rerun. No orphans. Duplicate hints: ES0182/ES0299 and the two `resolved/` crate-wide parts
  (distinct sheets and date ranges, kept), and the benches pair kept 2026-11-13. The
  ADR-002, ADR-003, ADR-005 and ADR-006 promote-or-archive calls stay the owner's. The
  workspace pass (`agent-memory`) was not run: it is not in this checkout.
- 2026-11-17 - pass forced by the chip work merged with the src layout (+13 documents,
  mostly eight errata sheets, ES0206, ES0287, ES0290, ES0298, ES0305, ES0334, ES0360 and
  ES0372, the F7 plan, and ADR-011 and ADR-012).
  No orphans. Duplicate-title hints: the errata sheets share ST's titles, one document
  per sheet by ADR-010, kept; the two dated `docs/resolved/` files are one log split by
  period, kept; `benches-analysis.md` and `benches-refusals.md` as on 2026-11-13. ADR-002,
  ADR-005, ADR-006 still await the owner's promote-or-archive call. No plans or notes
  older than 90 days. Workspace: `agent-memory` is not in this checkout, not run.

- 2026-11-13 - pass forced by the day's restructuring (+13 documents: `docs/CHIPS.md`,
  `hil/COVERAGE.md`, `docs/policies/features.md`, ADR-007 to ADR-009 with
  `docs/design/glue-generator.md` and `maturity.md`, the generator plan, DEBT and
  RESOLVED moved into `docs/`). No orphans. One duplicate-title hint, `benches-analysis.md`
  and `benches-refusals.md`: different procedures, kept. `hil/CAPABILITIES.md`'s "What it can prove today"
  overlaps `hil/COVERAGE.md` (prose per capability against a matrix per IP version):
  cross-linked, kept. The five finished plans (debt sweep and its gates, `free()`, W14,
  doc sweep) stay: decision records cite them as their plans of record; retiring them
  is the owner's call. Workspace: `agent-memory` check passes (38 documents).

- 2026-11-12 - pass forced by the `free()` redesign (+11 documents since the last
  review, mostly the 2026-11-11 API-review notes and plans): no orphans, no
  duplicate-title hints, no plans or notes older than 90 days. `docs/design/free.md`
  and the teardown brief record the landed shapes, DEBT's `Spi`/`I2c` no-`free()` item
  moved to RESOLVED (ratchets moved with it), and the bench's second-use runs folded
  back into runs H and I. Index regenerated (59 documents). The same four accepted
  records await the owner's promote-or-archive call: ADR-002, ADR-003, ADR-005,
  ADR-006.
- 2026-11-10 - pass forced by the documentation sweep (+22 documents): the
  architecture map dissolved into 12 design topics, 2 notes and ADR-005/ADR-006
  (ADR-006 is the topics' provenance); the rig docs moved to `hil/CAPABILITIES.md`
  and `hil/RIG.md`; DEBT section 6 closed by FerroWasp's adoption and the sections
  renumbered. No orphans, no duplicate-title hints. Four accepted records await the
  owner's promote-or-archive call: ADR-002, ADR-003, ADR-005, ADR-006. Index
  regenerated (48 documents).
- 2026-11-09 - initial pass with the system: policies, decision hub and index, plans and
  notes in place; agent docs rewritten to the token style; generated `docs/INDEX.md` and
  `docs/index.json` (26 documents, ~7.2k agent tokens); no orphans; three active records,
  none promoted to a design topic yet because no topic document exists.
