# Housekeeping state

- Last review: 2026-11-13
- Next due: 2026-12-13
- Documents at last review: 72

Cadence: monthly, or immediately when more than ten documents have been added since the
last review. The owner changes the cadence by editing the two dates above.

## Reviews

- 2026-11-13 - pass forced by the day's restructuring (+13 documents: `docs/CHIPS.md`,
  `hil/COVERAGE.md`, `docs/policies/features.md`, ADR-007 to ADR-009 with
  `docs/design/glue-generator.md` and `maturity.md`, the generator plan, DEBT and
  RESOLVED moved into `docs/`). No orphans. One duplicate-title hint, `benches-analysis.md`
  and `benches-refusals.md`: different procedures, kept. ADR-002, ADR-005 and ADR-006
  stay accepted (owner, 2026-11-13). `hil/CAPABILITIES.md`'s "What it can prove today"
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
