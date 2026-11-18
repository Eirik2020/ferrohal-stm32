# FerroHAL: open debt

Debt is work FerroHAL owes on what it already ships: a flaw, an inconsistency, shipped
code with no hardware proof, or a missing check. Finished items move to
[resolved/](resolved/README.md). Three things are not debt and live elsewhere (owner,
2026-11-13):

- **A feature not built** is the roadmap: [CHIPS.md](CHIPS.md#not-supported-yet).
- **A listed feature exception** (a feature that can panic, G2) is policy:
  [`policies/features.md`](policies/features.md).
- **A limit of the silicon or a documented contract** is a capability fact:
  [CHIPS.md](CHIPS.md#limits).
- **Anything a particular firmware needs** stays in that firmware's repository. The
  toaster rule (`docs/design/scope.md`) is absolute: no item here names a user.

Test coverage per IP version and bench: [`hil/COVERAGE.md`](../hil/COVERAGE.md).

## How an item reads

`tags` **Severity: title.** What is wrong and what fixes it. *Spawned:* the commit,
review or decision that created it, from the record. Where nothing records it, the
item says so; an origin is never reconstructed after the fact.

| tag | the fix needs |
|---|---|
| `code` | library source (`src/`); the changed files' evidence goes stale, so their benches rerun |
| `api` | a public API change |
| `bench` | bench firmware or scripts (`hil/`) |
| `rig` | the physical rig: a board, a wire, a solder bridge, an instrument (upgrades held by the owner, 2026-11-13) |
| `tool` | a checker or tool (`tools/`, `checks/`) |
| `docs` | documentation only |
| `errata` | follows from a vendor erratum |
| `owner` | an owner decision first |

Severity: **High** gives wrong behaviour. **Medium** is a gap a user will hit soon.
**Low** is cleanup. **Lowest** waits on something outside the repository.

Status 2026-11-16 (PR #48): the merge-gate benches pass on F401, H753 and the STM32F4
Discovery; reqtool 128 of 128 verified, 0 stale (SDMMC-003/004 on the Lucid, `FLSH`). Since
then #52 (ADR-011) and #53 (ADR-012) changed shared files, so the F4 and H7 benches owe a
rerun (`tools/stale_runs.py` lists it). No item is High.

---

## 1. Library code

- `code` `api` **Medium: the feature hierarchy departs from `docs/design/structure.md`.**
  Chip features turn on their family marker, and two chips at once are caught only by a
  `compile_error!` listing every pair (10 at five chips, 15 at six). Same as review S6.
  *Spawned:* `af65cad` 2026-10-31, two chips; each chip since added a row.
- `code` `errata` `rig` **Medium: no ES0392 2.2.10 workaround for rev-Y/W H7s.** DMA
  buffers sit in AXI SRAM, where those revisions can corrupt reads;
  `AXI_TARG7_FN_MOD.READ_ISS_OVERRIDE` fixes it and metapac lacks the register. Both H7s
  here are rev V, so no bench can show it (`docs/design/h7-errata.md`). *Spawned:* the
  W11 errata review (`3702336`, 2026-11-09).
- `code` **Low: kernel clock is assumed equal to bus clock.** True on the F4, and on the H7
  only because `freeze` forces the muxes; with review D2. *Spawned:* `af65cad`, the
  UART's `Info` derived baud from the bus clock.
- `code` `bench` **Medium: above 1 Mbaud, `usart_v2` places a lone parity error late or
  loses it.** F401 run B, 8N2 into 8O1 (one bad byte, then a good frame), both
  directions, measured 2026-11-15: at 115200 and 1 Mbaud the error is seen once and
  falls right after the bad byte; at 2.625 and 3.5 Mbaud it is seen once but reported
  one byte late; at 5.25 Mbaud no vector sees PE and nothing is reported, though even
  parity into odd (every byte bad) is reported at every rate. Cause not known: whether
  PE never rises for a lone byte at USARTDIV 1, or something clears it first (any SR
  read followed by DMA's DR read does), and why the vector runs a byte late at 2.6 Mbaud.
  Run B checks 115200 and 1 Mbaud only. *Spawned:* the one-speed item's measurement
  (2026-11-15), which ran the clear at five rates.

- `code` `bench` **Medium: the F767's SPI5 receives 0xF0 at its slowest clock.** F767 run F,
  link S4 (PF9 to PF8), measured twice on 2026-11-18: at the suite's normal 6.75 MHz every
  SPI5 check passes, but at its slowest setting (APB2 108 MHz / 256, 421.9 kHz) the first
  byte comes back 0xF0 whatever was sent (21 and 76), in both the edge-count and the
  SCK-timing checks. SPI1 and SPI4, on the same APB2 and the same slowest setting, pass.
  Cause not known: spi_v3 at the largest prescaler on this instance, the FIFO's state after
  the preceding checks, or the link. *Spawned:* F767 run F's first pass (2026-11-18).

## 2. API consistency (the 2026-11-11 review)

Recorded by the API review (`cb58dd9`); evidence and fixes in its notes, hub
[`notes/2026-11-11-api-review.md`](notes/2026-11-11-api-review.md), which gives the
order. Re-measure before fixing. Every row is `code`, so each changed file's evidence
reruns. The spawn pattern: each driver landed in its own commit between 2026-10-31 and
2026-11-06 with no shared template, and later drivers reused the first ones' types.

D12, D13, N7, N9, N10, N12, S3, U3 and U10 are fixed in code by the UART single-owner
redesign and the `Service` trait ([ADR-011](decisions/2026-11-17-uart-single-owner.md));
each leaves for [resolved/](resolved/README.md) when the F401 and H753 benches rerun on that code.

| id | tags | sev | finding | spawned |
|---|---|---|---|---|
| D12 | `code` `api` | L | Interrupt service is not uniform. | per driver; `1ca19a9` added a third shape |
| D13 | `code` `api` | L | Vectors come in four shapes; `verify()` on five of nine. | `2e7f7fe` UART, `b55c640` EXTI, `4070cd8` timers |
| N7 | `code` | L | Register wrapper `Unit` vs `LineRegs`. | `99e6404` `Unit`; `4bb8398` ADR-004 named `LineRegs` |
| N9 | `code` `api` | L | Vector-name constants differ by driver. | `2e7f7fe`, `4070cd8` |
| N10 | `code` `api` | L | DMA fault variant `DmaTransfer`/`DmaOverrun` vs `Dma`. | `42cbcaf` UART, then `99e6404` |
| N12 | `code` `api` | L | Health counters differ in plurality, type and overflow. | `42cbcaf`, `99e6404`, `cdfe190` |
| S3 | `code` | L | UART engine is one 1,887-line file. | grew from `af65cad` to `4bb8398` |
| U3 | `code` | M | Four drivers hand-write the one-shot DMA transfer. | `99e6404`, `a8a3a05`, `ea7a9f8` |
| U10 | `code` | L | `interrupt()` + `verify_vectors` three times. | `4070cd8`, `ea7a9f8` |

## 3. Shipped code without hardware proof: bench work

- `tool` **Low: SDMMC's `free()` has only the Lucid's flashed run.** The Lucid bench frees
  and reopens the SDMMC, and its runs are `FLSH` evidence since the tiers (ADR-009), but
  API-002 names no Lucid procedure: an entry would re-owe the Lucid on every change to
  any driver declaring API-002, against the merge-gate rule. SDMMC's API-002 stands on
  the H753 runs that compile it. *Spawned:* `free()` for SDMMC (2026-11-13), whose only
  slot is on a flashed board.
- `bench` **Low: FIFO error handling is unconfirmed on hardware.** Transmit FIFO errors
  are cleared and ignored, receive ones are `Error::Dma`; neither has been provoked.
  *Spawned:* `af65cad`, which took the transmit policy from the firmware the prototype
  was written for.
- `bench` **Low: the bench isn't on FerroForge.** Decided 2026-11-02: add a FerroHAL
  backend to the FerroForge CLI and move the runs onto its tasks, proving the pair.
  *Spawned:* `d773b34` recorded the decision and deferred the move.


## 4. Shipped code without hardware proof: rig work

- `rig` **Medium: the H753 bench reaches the Zio headers only.** UART8 TX, PA8, PA9,
  PB14, PG9 and 22 of 46 TX/RX entries are unreachable without morpho headers; UART5's
  PB13 needs SB7. Their rows are glue-checked only. *Spawned:* `55025e8` 2026-10-31,
  the bench wired on Zio.
- `rig` **Low: SDMMC's error paths have no bench.** A CRC failure and a card pulled
  mid-transfer are untested. SDMMC-003/004 stand on the Lucid's `FLSH` runs (owner,
  2026-11-16); a Nucleo with a slot would make them `HIL` (queue 2). *Spawned:* SDMMC proven on the Lucid alone (`1b42fe4`, 2026-11-02).
- `rig` **Low: HSE crystal mode is untested on both Nucleos.** Both run HSE bypass from
  the ST-LINK's MCO. *Spawned:* `af65cad`, the benches clock from the ST-LINK.
- `rig` **Low: F401 USART2 runs through the ST-LINK,** so its break, baud and parity
  changes are unchecked; SB13/SB14 off and SB62/SB63 on would give a direct link.
  *Spawned:* `af65cad`.
- `rig` **Low: the F401's clock tree is checked only indirectly.** The H753 (run M) and the
  Discovery (run B) measure the CPU clock against the host's USB start-of-frames; the
  Nucleo-F401RE has no USB device, so its clocks rest on `freeze`'s record, baud rates
  and the host echo. Measuring showed the H753's HSE input, the ST-LINK-V3's MCO, 0.4 %
  below 8 MHz, so its check allows 0.5 % (`hil/RIG.md`). *Spawned:* `af65cad`; narrowed
  2026-11-15.
- `rig` **Lowest: the rig cannot vary what several ES0392 rules depend on:** the HCLK/8
  SysTick path, WWDG, option bytes, flash ECC, another silicon revision. Every F4 here
  is past revision A, so ES0182 2.2.2's mapping (`resolved/`) stands on host tests
  alone. *Spawned:* the W11 audit (`ff3af8d`, 2026-11-10).

## 5. CI and merging

- `tool` `owner` **Low: only "Generated glue" blocks a merge into `main`.** The owner's
  ruleset (2026-11-15) requires that one check; "Checks without hardware",
  "Requirements", the five "Bench crate" jobs and "Publication security" run on every
  pull request but only report, so a red one does not stop a merge. Fix: add them to
  the ruleset's required checks (with GitHub Actions as their source), and consider
  "Require branches to be up to date before merging", so the checks see the code as it
  lands. *Spawned:* the owner's choice of one required check for now (2026-11-15).
- `tool` **Low: a required check that runs on pull requests only blocks direct pushes to
  `main`.** "Generated glue" runs on pull requests into `main` (ADR-008), so a direct push
  never gets its result and the ruleset refuses it; changes reach `main` by pull request
  alone. Fine while the owner merges by pull request; a hotfix pushed straight to `main`
  would need the ruleset's bypass list. *Spawned:* the ruleset of 2026-11-15 with the
  gate's pull-request-only trigger.

