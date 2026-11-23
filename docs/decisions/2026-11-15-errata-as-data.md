# ADR-010: Errata as data, matched to the die revision (2026-11-15)

ADR-010 - Status: promoted

Promoted to: [errata.md](../design/errata.md) (design).

## Context

FerroHAL's errata handling was spread over code comments (15 sections of ES0182, ES0299
and ES0392 in 11 files), a hand-written verdict table (`h7-errata.md`, ES0392 only) and
DEBT items, with nothing tying an erratum to the revisions it affects. DEBT's "`freeze`
sets PRFTEN on an F405 of any revision" (ES0182 2.2.1) is that class of bug, and the
review table had drifted: it named DEBT items that no longer existed and driver items
no source file cited.

The pattern is taken from Linux's arm64 errata handling,
verified against `torvalds/linux` at `23c62304dde2`: a match list of affected
silicon per erratum (`cpu_errata.c`), one table matched at boot and logged
(`arm64_errata[]`, "detected: %s"), workarounds that check a capability, never a
revision (`process.c`), and one documented table (`silicon-errata.rst`, where "N/A" means
no config option, not no workaround). Zephyr (`7cd4b562b30a`) and Nordic's nrfx
(`hal_nordic` `a94077e8d576`) were read beside it: Zephyr's STM32 port compares raw
`REV_ID`s in place and cites ES0392 by a section number that has since moved (Rev 8's
2.2.9 is Rev 15's 2.2.10); nrfx generates one revision-matching function per erratum and
decides unknown revisions per erratum by whether the fix exists in the newest known one.

## Decision

1. Every erratum of a supported die's errata sheet is an entry in the library: sheet and
   its revision, section, title, the `REV_ID`s affected, the sheet's workaround status,
   FerroHAL's handling, and the requirements that carry it. Entries sit with the die's
   hand facts; they come from errata sheets, not from metapac.
2. Handling: `Workaround`, `Avoided`, `Firmware`, `Board`, `NotReached`, `Open` (names a
   DEBT item) and `Unreviewed` (transcribed, no verdict yet).
3. A workaround checks its erratum (`affects(rev)`), never a raw revision code. An unknown
   revision newer than the sheet's newest takes the newest's verdict; any other unknown
   revision is affected.
4. A section is cited with its sheet's revision in the entry, so a renumbered section
   cannot silently point elsewhere.
5. A host test per chip renders the die's table into `docs/errata/` and enforces the
   rules: every section cited in `src/` has a reviewed entry, every `Workaround` entry is
   cited, every `Open` entry names a DEBT item that exists.
6. No per-erratum opt-out: the chip feature already selects the die, and switching a
   workaround off would suspend a guarantee (`docs/policies/features.md`).

## Rejected

- A Python checker reading the Rust constants as text (the pattern `check_glue.py`
  retired with).
- An evidence column filled by hand.
- Linux's compile-time switch per erratum.

## Consequences

Evidence is the linked requirements', in the tier tables; it is not repeated in the
errata tables.
