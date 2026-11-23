# Errata as data

Source: ADR-010, [`../decisions/2026-11-15-errata-as-data.md`](../decisions/2026-11-15-errata-as-data.md).
The tables (generated): [ES0182](../errata/ES0182.md) (die 0x413), [ES0206](../errata/ES0206.md) (die 0x419),
[ES0287](../errata/ES0287.md) (die 0x431), [ES0298](../errata/ES0298.md) (die 0x421), [ES0305](../errata/ES0305.md) (die 0x441), [ES0372](../errata/ES0372.md) (die 0x463), [ES0325](../errata/ES0325.md) (die 0x458), [ES0290](../errata/ES0290.md) (die 0x449), [ES0299](../errata/ES0299.md) (die 0x433),
[ES0334](../errata/ES0334.md) (die 0x451), [ES0360](../errata/ES0360.md) (die 0x452), [ES0392](../errata/ES0392.md) (die 0x450). The ES0392
review notes: [h7-errata.md](h7-errata.md).

## The data

- `crate::errata` defines `Sheet` (name, revision, and the die's revision
  codes in time order with their markings) and `Erratum` (sheet, section, title,
  affected `REV_ID`s, the sheet's workaround status, FerroHAL's `Handling`,
  requirements). Each die's entries are one table beside its hand facts:
  in its family's folder, `chips/f4/`: `errata_es0182.rs` (die 0x413), `errata_es0206.rs` (die 0x419), `errata_es0287.rs` (die 0x431), `errata_es0298.rs` (die 0x421), `errata_es0299.rs` (die 0x433), `errata_es0305.rs` (die 0x441), `errata_es0325.rs` (die 0x458), `errata_es0372.rs` (die 0x463);
  `chips/f7/`: `errata_es0290.rs` (die 0x449), `errata_es0334.rs` (die 0x451), `errata_es0360.rs` (die 0x452); `chips/h7/`: `errata_es0392.rs` (die 0x450). Exported as `chip::ERRATA` and `chip::ERRATA_SHEET`.
- Transcribed once from each sheet's "Summary of device limitations" with
  every row; titles from the section headings. ES0182 Rev 19 (91 rows, revisions A, Z, 1,
  2, Y, 4/5/6), ES0206 Rev 24 (95 rows, A, Y, 1, 3, 4/5/B), ES0287 Rev 6 (65 rows, A/1/2), ES0298 Rev 8 (83 rows, A/1), ES0305 Rev 15 (72 rows, Z, B, 1/C), ES0372 Rev 8 (71 rows, A/1), ES0325 Rev 7 (58 rows, A/1/Z), ES0290 Rev 7 (35 rows, Z/1), ES0299 Rev 6 (66
  rows, A, 1/Z), ES0334 Rev 9 (65 rows, A, Z/1), ES0360 Rev 5 (26 rows, A/1), ES0392 Rev 15 (155 rows, Y/W, X, V). The F7
  sheets call their rows device limitations; the shape is the same.
- `Handling`: `Workaround` (FerroHAL applies one), `Avoided` (its code never
  meets the condition), `Firmware` (a rule the firmware follows; the text says which),
  `Board` (a package or board fact, no firmware action), `NotReached` (no FerroHAL path
  reaches it), `Open` (affects FerroHAL, unhandled; names the DEBT item), `Unreviewed`
  (no verdict yet). nrfx's matrix has the same split (driver/HAL, code, other, to do).

## Matching

- `Erratum::affects(rev)`: a known revision is affected when listed; one
  newer than the sheet's newest takes the newest's verdict (nrfx's per-erratum default:
  unfixed in the newest means a newer one is affected too); any other unknown revision is
  affected. ST's codes run in time order on these dies (F4 0x1000 to 0x101F, H7 0x1003 to
  0x2003).
- A workaround checks its erratum, never a raw `REV_ID` (Zephyr's
  `== 0x1003` is the pattern this replaces). One that affects every revision needs no
  runtime check and names its entry where it acts.

## Checks and documentation

- A host test per chip renders the die's table to `docs/errata/<sheet>.md`
  between generated markers (`FERROHAL_BLESS_ERRATA=1` rewrites it) and fails on any
  difference; it fails too when a section cited in `src/` has no reviewed entry, a
  `Workaround` entry is cited nowhere, or an `Open` entry's DEBT item is missing.
- The tables list each entry's requirements; their evidence and tiers are
  in `docs/CHIPS.md` and `hil/COVERAGE.md`, not repeated here.
