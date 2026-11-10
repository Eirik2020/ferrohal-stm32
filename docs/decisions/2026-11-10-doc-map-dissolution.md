# ADR-006: Dissolve the architecture map into topic documents (2026-11-10)

ADR-006 - Status: accepted

The map (`ferrohal-architecture.md`, 15 sections) was the W7 answer to a
one-place-for-everything design doc; it floated in the root, held both rules and
records, and outgrew its ratchet once the design topics existed.

## Decision

Dissolve it per the promotion rules it defined for itself: the dated decisions
(section 15) became ADR-005; the design became `docs/design/` topics, one per
subject, section 9 split three ways for the token budgets; the measured facts
(sections 11 and 14) became dated notes; section 12's deferred items are built
(USB, SDMMC) or superseded (ADR-005's all-at-once swap), so only their fate is
noted. The plan of record is
[../plans/2026-11-10-doc-sweep.md](../plans/2026-11-10-doc-sweep.md).

## Consequences

- Every chapter of the dissolution carries `Source: ADR-006`; the UART proposals
  carry `Source: ADR-005`, whose record they are.
- The map's ratchet (25,995 B) is gone; DEBT.md's citations of the map's sections
  now point at the topics; RESOLVED.md's section numbers cite the map as it was.
- DEBT section 6 (the prototype's open questions) closed by FerroWasp's
  adoption; the transition and deferred sections renumbered to 6 and 7.
- The rig documents moved out of `requirements/` (`hil/CAPABILITIES.md`,
  `hil/RIG.md`); `benches.md` stays: 87 `procedure:` anchors resolve there.
- The docs index cap rose 800 -> 1200 tokens: the set is 45 documents now, and
  the index is generated navigation that grows with the set.
