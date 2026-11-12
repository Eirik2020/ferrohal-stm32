# Housekeeping state

- Last review: 2026-11-12
- Next due: 2026-12-12
- Documents at last review: 59

Cadence: monthly, or immediately when more than ten documents have been added since the
last review. The owner changes the cadence by editing the two dates above.

## Reviews

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
