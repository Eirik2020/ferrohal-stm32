# The glue generator, and the work before it - plan of record

Status: **agreed 2026-11-13; steps 1 and 2 of the order built (PR #43), their hardware runs
pending; the generator's step 1 built (`tools/glue-gen`), step 2 next.** Steps 1 and 2 of the
order and the generator's step 1 merged (#43, verified 2026-11-13); generator step 2 built. The owner chose
2026-11-13: handles generic over their whole wiring (`Uart<W>`), and `free()` the inverse
of `new` for every driver. Decisions: ADR-008 (the generator) and ADR-009
(maturity). Design: `docs/design/glue-generator.md` and `docs/design/maturity.md`.

## Order

The owner agreed the order on 2026-11-13. The first two items don't touch the glue, so
they go first; the third changes exactly what the generator emits, so it travels with
it.

1. **`free()` for `Adc`, `AdcScan`, `Sdmmc` and `UsbFs`** (DEBT §1), as the inverse of
   `new` (owner, 2026-11-13: `free()` hands back everything the constructor took). Each
   frees and reopens on the bench that reaches it: ADC on both Nucleos, USB on the H753,
   SDMMC on the Lucid. Gate: `checks/run.sh`, F401 and H753 suites.
2. **One DMA-buffer error** (review D6 and D7). A shared `BufferError` that each driver's
   `ConfigError` wraps, so the timers no longer refuse with the UART's error. F401 run V
   and H753 run U change with it (B3).
3. **The generator, with one instance shape** (review D1 to D3, D2), in the steps below.

## The generator's steps

One pull request, one commit per step, each gated by `checks/run.sh` and, from step 4,
both Nucleo suites.

1. **The tool and the registry.** `tools/glue-gen` reads the pinned metapac's metadata
   for one part per build and holds the part registry (the four parts FerroHAL supports
   today, each with its die and device) and the offering policy. Selectors `all`,
   `series`, `die`, `part`. **Built 2026-11-13:** `glue-dump` prints one part's
   metadata as JSON, every RCC field resolved to its bit; `glue-gen parts|dump
   <selector>` checks the tool's metapac is the library's, the registry's consistency,
   and writes `tools/glue-gen/target/parts/<part>.json`. Offering policy not yet. Noted:
   metapac names the H7's SPI IP `v4_i2s`.
2. **The silicon model.** Die tables as the union of their parts, with the consistency
   check; device and package layers; per-package pin sets, which `Port::split` follows.
   **Built 2026-11-13:** `glue-gen model <selector>` merges every registered part of each
   die the selector touches and lists every conflict; the four parts model cleanly (die
   0x450: 109 peripherals, 929 pin rows, 143 DMA routes). `Port::split` follows the
   package when the glue is emitted (step 4).
3. **The instance shape** (D1 to D3, D2). One instance trait shape and one kernel-clock
   type, decided before anything is emitted, so the rows are written once. UART, SPI,
   I2C and the timer lanes keep their typed pins and stream tokens, and `free()`
   returns them (free.md rule 0); how a handle carries them (generic over its wiring,
   or over each part) is the owner's choice at this step.
4. **Emit and compare.** The generator writes `src/chips/gen/`; the generated glue is
   compared row by row with today's hand glue, and every difference is explained. Then
   the part files switch to the generated glue plus their hand facts, and the generator
   also writes `docs/CHIPS.md`'s silicon and capability tables, between generated
   markers, replacing the hand tables.
5. **The glue probe.** A run per Nucleo, debugger only: the die ID, each peripheral's
   enable and reset bits, each vector, each pin's mode and alternate function. It is the
   die axis's evidence for 0x433 and 0x450.
6. **The merge gate.** A required check on pull requests into `main` regenerates and fails
   on any difference; `tools/check_glue.py` and its `checks/run.sh` step go.
7. **Tiers.** reqtool records each run's tier as its environment (a reqtool change, pinned
   by revision here); a command computes each part's platform tier and capability table
   into `docs/CHIPS.md`, and `hil/COVERAGE.md`'s matrix, both checked at the merge gate.

## What it closes

DEBT's "the routing generator doesn't exist" and "a bonded port still splits into every
pin"; review findings S1, U1, D1 to D3, D2, D9 and D10; the chip-pair `compile_error!`,
which the generator emits instead.

## Known gap

Die 0x413 (the F405's) has only flashed runs, the Foxeer's; its glue probe waits for the
F407 Discovery to be wired with a cable. The owner accepted this on 2026-11-13; it is in
`docs/DEBT.md` §4, and the owner has ordered parts for the rig.
