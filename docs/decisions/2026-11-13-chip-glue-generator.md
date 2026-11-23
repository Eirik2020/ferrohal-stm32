# ADR-008: Generate the chip glue from metapac (2026-11-13)

ADR-008 - Status: promoted

Promoted to: [glue-generator.md](../design/glue-generator.md) (design).

## Context

About 3,100 lines of chip glue (four part files, shared macros, shared H7 facts) are
copied by hand from stm32-metapac's metadata and checked by `tools/check_glue.py`, which
parses the rows as text. The Python check is as likely to fail as a generator would, so
the hand copy is replaced with a Rust generator, verified by analysis (probing memory on
target) and bench runs. `docs/design/scope.md` had parked the generator; this record
unparks it.

## Decision

1. A Rust generator, `tools/glue-gen`: its own package, never published, never a build
   dependency of the library, reading the pinned stm32-metapac with `metadata`.
2. Output is checked in (`src/chips/gen/`), so reqtool hashes it and a metapac bump is a
   reviewable diff. The generator runs on demand, and as a required check on pull
   requests into `main`; not on pushes to development branches.
3. Selectors: everything, a series, a die (a line name accepted for it), a part.
   Regenerating a die regenerates every supported part on it.
4. Silicon is separate from packages: series, die, device (product line), package, part.
   The user picks a part; FerroHAL dedups by die. The device layer is built now, thin,
   and pruned later if it stays redundant.
5. Facts metapac lacks stay hand-written beside the generated files.
6. Verification: the generator's self-checks, a one-time row-by-row comparison with the
   hand glue it replaces, a glue probe run per die on target, and the bench runs.
   `tools/check_glue.py` retires once the generator is merged into `main`.
7. The documentation tables come from the same sources, so they cannot drift: the
   generator writes `docs/CHIPS.md`'s silicon and capability tables; the tier command
   writes the tier tables and `hil/COVERAGE.md`'s matrix from the evidence. Both are
   checked at the merge gate with the glue. Prose stays hand-written.

## Rejected

- A build script (embassy-stm32's way): its output is not a file reqtool can hash, every
  user build pays for the metadata, and a metapac bump changes glue silently.
- Keeping `check_glue.py` beside the generator: two readings of one source, either as
  likely to be wrong; the probe and the benches check the generator instead.

## Consequences

- Closes on landing: DEBT's "the routing generator doesn't exist" and the bonded-pin
  item; the chip-pair `compile_error!` becomes generated.
- The instance shape and kernel clock change what the generator emits, so they are done
  with it.
- Die 0x413 (F405/F407) has no debugger bench: its glue rests on flashed runs until the
  F407 Discovery is wired (`docs/DEBT.md`).
