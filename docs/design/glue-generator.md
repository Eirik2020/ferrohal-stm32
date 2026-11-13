# The chip-glue generator

Source: ADR-008, [`../decisions/2026-11-13-chip-glue-generator.md`](../decisions/2026-11-13-chip-glue-generator.md).
Plan: [`../plans/2026-11-13-glue-generator.md`](../plans/2026-11-13-glue-generator.md). Maturity of what it
emits: [maturity.md](maturity.md).

## The silicon model

| layer | identified by | holds | example |
|---|---|---|---|
| series | ST's series | code every member shares | STM32F4 |
| die | DBGMCU `DEV_ID` (revision `REV_ID`) | routing, RCC bits, vectors, memory map, IP instances | 0x413: F405, F407, F415, F417 |
| device | product line | which of the die's peripherals are enabled | F405 (no Ethernet) vs F407 |
| package | pin count and type | bonded pins | LQFP64, LQFP100, TFBGA100, LQFP144 |
| part | metapac's name | device + package + memory size | `stm32f405rg` |

- **[Decided]** The user picks a part (its Cargo feature, metapac's name); FerroHAL dedups
  by die. A die table is the union of its supported parts' metadata; two parts of one die
  that disagree on a row fail the generator.
- **[Decided]** The device layer exists now, thin, and is pruned if it stays redundant:
  F405 vs F407 and H743 vs H753 differ only in peripherals FerroHAL has no driver for.
- **[Decided]** A package is the bonded-pin set; `Port::split` follows it, so an unbonded
  pin cannot be named.
- Die IDs: F401xD/E 0x433, F405/F407/F415/F417 0x413, H742/H743/H750/H753 0x450.
- **[Measured 2026-11-13, metapac 21]** What the die fixes and parts agree on: each
  peripheral's address, IP and register block, RCC fields, a pin's alternate function for
  a signal, and the set of DMAMUX requests per signal. What does not: a fixed-request
  DMA stream can serve one signal through two channel selections (the F405's TIM1 CH3,
  TIM8 CH1), so a route's request is part of the route; metapac gives several requests
  one signal name (HRTIM1's five `TIMER`s), so requests compare as sets; a vector's name
  can be the device's (RNG's is `HASH_RNG` on the H753, `RNG` on the H743); the H7's
  `_C` analog pads are not GPIO and are not in a package's pin list.
- **[Measured]** metapac's per-part peripheral list mixes device and package: the H743VI
  lacks CRYP and HASH (its line) and SPI5 (its LQFP100 bonds none of SPI5's pins). The
  emitted glue tells the two apart.

## The generator

- **[Decided]** `tools/glue-gen`, a Rust package of its own: never published, never a build
  dependency. It reads the stm32-metapac version `Cargo.lock` pins, with `metadata`, one
  part per build (metapac selects one chip).
- **[Decided]** Selectors: `all`; `series F4`; `die 0x413` (or a line name, `STM32F405`);
  `part stm32f405rg`. A die selector regenerates every supported part on the die and
  prints them.
- **[Decided]** A part registry in the generator lists the supported parts, each with its
  die and device. Adding a part: a registry row, then regenerate.
- **[Decided]** Offering policy is data in the generator: which DMA controllers routes may
  use (DMA1, DMA2, DMAMUX1; not BDMA), which instances FerroHAL drives (ADC1 and ADC3 on
  the H7, SDMMC1), which IP versions have a driver.
- **[Decided]** Output is checked in: `src/chips/gen/die_*.rs`, `package_*.rs`,
  `<part>.rs`, each headed with the metapac version and "generated, do not edit".
- **[Decided]** It runs on demand, and as a required check on pull requests into `main`
  (regenerate, fail on any difference). It does not run on development-branch pushes.

## Generated documentation

- **[Decided]** Tables that restate the registry, the metadata or the evidence are
  generated, never hand-edited (owner, 2026-11-13): drift is a check failure.
  - `glue-gen` writes `docs/CHIPS.md`'s silicon table (part, series, die, package, core,
    clock as offered) and capability matrix (per part: each driver, the instances its
    die offers, n/a where the part has none).
  - The tier command writes each part's platform tier and capability tiers into
    `docs/CHIPS.md`, and `hil/COVERAGE.md`'s matrix per IP version, die and series, from
    reqtool's evidence.
- **[Decided]** Each generated table sits between `<!-- generated: <tool> -->` markers;
  the prose around it (limits, roadmap, notes) stays hand-written. The merge gate
  regenerates and fails on any difference, as for the glue.

## Hand facts

- **[Decided]** Facts metapac lacks stay hand-written beside the generated files, per die,
  each citing its manual section: clock limits, DMA-reachable RAM, calibration and
  system-memory addresses, USB core kind and FIFO depth, fractional stop bits, ADC
  internal channels, timer trigger routes (metapac's are empty), and the small code that
  goes with them (the MPU walk, the watchdog debug freeze, the revision read).

## Verification

- **[Decided]** The generator checks itself: parts of a die agree; every row names a pin
  its package bonds; a metadata kind it does not know fails loudly.
- **[Decided]** Migration, once: the generated glue compared row by row with the hand glue
  it replaces, which the benches proved; each difference explained before the swap.
- **[Decided]** A glue probe run per die, on target, debugger only (tier `DBG` or better):
  `DEV_ID` against the die table; each peripheral enabled by its generated RCC bit, a
  scratch register written and read back, reset by its reset bit to its reset value;
  each generated vector pended and seen in the bench's handler; each pin's mode and
  alternate function set and read back. Signal-level proof stays with the bench runs.
- **[Decided]** `tools/check_glue.py` retires once the generator is merged into `main`.
