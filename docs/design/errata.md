# Errata as data

Source: ADR-010, [`../decisions/2026-11-15-errata-as-data.md`](../decisions/2026-11-15-errata-as-data.md).
The tables (generated): [ES0182](../errata/ES0182.md) (die 0x413),
[ES0299](../errata/ES0299.md) (die 0x433), [ES0392](../errata/ES0392.md) (die 0x450). The ES0392
review notes: [h7-errata.md](h7-errata.md).

## The data

- **[Decided]** `crate::errata` defines `Sheet` (name, revision, and the die's revision
  codes in time order with their markings) and `Erratum` (sheet, section, title,
  affected `REV_ID`s, the sheet's workaround status, FerroHAL's `Handling`,
  requirements). Each die's entries are one table beside its hand facts:
  `chips/shared/errata_es0182.rs` (die 0x413), `errata_es0299.rs` (die 0x433),
  `errata_es0392.rs` (die 0x450), exported as `chip::ERRATA` and `chip::ERRATA_SHEET`.
- **[Decided]** Transcribed once from each sheet's "Summary of device limitations" with
  every row; titles from the section headings. ES0182 Rev 19 (91 rows, revisions A, Z, 1,
  2, Y, 4/5/6), ES0299 Rev 6 (66 rows, A, 1/Z), ES0392 Rev 15 (155 rows, Y/W, X, V).
- **[Decided]** `Handling`: `Workaround` (FerroHAL applies one), `Avoided` (its code never
  meets the condition), `Firmware` (a rule the firmware follows; the text says which),
  `Board` (a package or board fact, no firmware action), `NotReached` (no FerroHAL path
  reaches it), `Open` (affects FerroHAL, unhandled; names the DEBT item), `Unreviewed`
  (no verdict yet). nrfx's matrix has the same split (driver/HAL, code, other, to do).

## Matching

- **[Decided]** `Erratum::affects(rev)`: a known revision is affected when listed; one
  newer than the sheet's newest takes the newest's verdict (nrfx's per-erratum default:
  unfixed in the newest means a newer one is affected too); any other unknown revision is
  affected. ST's codes run in time order on these dies (F4 0x1000 to 0x101F, H7 0x1003 to
  0x2003).
- **[Decided]** A workaround checks its erratum, never a raw `REV_ID` (Zephyr's
  `== 0x1003` is the pattern this replaces). One that affects every revision needs no
  runtime check and names its entry where it acts.

## Checks and documentation

- **[Decided]** A host test per chip renders the die's table to `docs/errata/<sheet>.md`
  between generated markers (`FERROHAL_BLESS_ERRATA=1` rewrites it) and fails on any
  difference; it fails too when a section cited in `src/` has no reviewed entry, a
  `Workaround` entry is cited nowhere, or an `Open` entry's DEBT item is missing.
- **[Decided]** The tables list each entry's requirements; their evidence and tiers are
  in `docs/CHIPS.md` and `hil/COVERAGE.md`, not repeated here.
