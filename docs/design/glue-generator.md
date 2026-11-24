# The chip-glue generator

Source: ADR-008, [`../decisions/2026-11-13-chip-glue-generator.md`](../decisions/2026-11-13-chip-glue-generator.md).
Maturity of what it emits:
[maturity.md](maturity.md).

## The silicon model

| layer | identified by | holds | example |
|---|---|---|---|
| series | ST's series | code every member shares | STM32F4 |
| die | DBGMCU `DEV_ID` (revision `REV_ID`) | routing, RCC bits, vectors, memory map, IP instances | 0x413: F405, F407, F415, F417 |
| device | product line | which of the die's peripherals are enabled | F405 (no Ethernet) vs F407 |
| package | pin count and type | bonded pins | LQFP64, LQFP100, TFBGA100, LQFP144 |
| part | metapac's name | device + package + memory size | `stm32f405rg` |

- The user picks a part (its Cargo feature, metapac's name); FerroHAL dedups
  by die.
- Every package of a supported line is a registered part, named by its largest flash size. A part that differs only in flash size is
  a variant (`VARIANTS`): its own Cargo feature, mounting its package's glue. `model`
  dumps variants too, so one that disagrees with its part on any fact fails the
  generator. A line's hand facts are one chip module under its family, named by the
  lines it serves (`chips/f4/stm32f401.rs`, `chips/f7/stm32f76x_f77x.rs`), which mounts
  each part's glue under that part's features (ADR-012).
- The chip selection in `src/lib.rs` is generated (`emit all`,
  ADR-012): the one-chip-feature check from the registry's parts and variants, and each
  line's `chip` mount from the chip file that mounts its parts' glue. Every part of a line
  must be mounted by one file; the merge gate checks `src/lib.rs` with the glue.
- A part's glue is cut by device and package together, not by
  device alone: metapac's peripheral list and pin rows can depend on the package. The
  H743VI's LQFP100 has no SPI5, and only the TFBGA240, which bonds PC2 and PC2_C apart,
  routes ADC1's INP12 to PC2. The die table keeps each cut's peripherals and pin rows,
  and parts of one cut must agree on them. A die table is the union of its supported parts' metadata; two parts of one die
  that disagree on a row fail the generator.
- The device layer exists now, thin, and is pruned if it stays redundant:
  F405 vs F407 and H743 vs H753 differ only in peripherals FerroHAL has no driver for.
- A package is the bonded-pin set; `Port::split` follows it, so an unbonded
  pin cannot be named. The glue emits one parts
  type per bonded port, a field per bonded pin, behind a sealed `gpio::Split`.
- Die IDs: F401xD/E 0x433, F405/F407/F415/F417 0x413, F411xC/E 0x431, F722/F723/F732/F733
  0x452, F745/F746/F756 0x449, F765/F767/F768/F769/F777/F778/F779 0x451,
  H742/H743/H750/H753 0x450.
- **[Measured 2026-11-13, metapac 21]** What the die fixes and parts agree on: each
  peripheral's address, IP and register block, RCC fields, a pin's alternate function for
  a signal, and the set of DMAMUX requests per signal. What does not: a fixed-request
  DMA channel can serve one signal through two requests (the F405's TIM1 CH3,
  TIM8 CH1), so a route's request is part of the route; metapac gives several requests
  one signal name (HRTIM1's five `TIMER`s), so requests compare as sets; a vector's name
  can be the device's (RNG's is `HASH_RNG` on the H753, `RNG` on the H743); the H7's
  `_C` analog pads are not GPIO and are not in a package's pin list.
- **[Measured]** metapac's per-part peripheral list mixes device and package: the H743VI
  lacks CRYP and HASH (its line) and SPI5 (its LQFP100 bonds none of SPI5's pins). The
  emitted glue tells the two apart.

## The generator

- `tools/glue-gen`, a Rust package of its own: never published, never a build
  dependency. It reads the stm32-metapac version `Cargo.lock` pins, with `metadata`, one
  part per build (metapac selects one chip).
- Selectors: `all`; `series F4`; `die 0x413` (or a line name, `STM32F405`);
  `part stm32f405rg`. A die selector regenerates every supported part on the die and
  prints them.
- A part registry in the generator lists the supported parts, each with its
  die and device. Adding a part: a registry row, then regenerate.
- Offering policy is data in the generator (`tools/glue-gen/src/emit.rs`):
  which DMA controllers routes may use (DMA1, DMA2, DMAMUX1; not BDMA), which IP versions
  have a driver, and the instances left out, each with its reason.
- The F7's rows take the F4's macro shapes (fixed-request
  DMA, bus-named RCC registers), its usart_v3 rows included: the usart_v4 module serves
  it (ADR-011). A block on the part that no driver serves shows – (on the part, not
  driven) in the capability table, where it shows n/a for a block the part lacks.
- Where metapac's metadata is incomplete on one device of a
  die, a peripheral FerroHAL does not drive may be listed in `METADATA_GAPS`
  (`tools/glue-gen/src/registry.rs`), and the merge keeps the die's values for it rather
  than fail. The only entry: the STM32H750's HSEM, which metapac 21 gives no register
  block.
- The coverage matrix (`hil/COVERAGE.md`) gives an IP version
  no bench carries no tier (–), even where its requirements are verified on a version
  that shares its driver: the usart_v3 registers have no evidence of their own until
  the F767 bench runs.
- The glue offers everything the silicon has for every IP
  version FerroHAL drives: each instance, pin row and DMA route metapac lists. Left out:
  an instance a driver cannot open without disturbing another (the F4's ADC2 and ADC3,
  reset only by ADC1's ADCRST; the H7's ADC2, sharing ADC12EN; the ADC driver resets on
  open), a block the driver does not carry (LPUART1), and USB_OTG_HS and the F4's SDIO,
  which have no driver.
- Output is checked in, rustfmt'd: one module per part,
  `src/chips/gen/<part>.rs`, headed with the metapac version and "do not edit", which the
  part's chip module mounts beside its hand facts. It is the die table cut to the part's
  device and package; separate die and package files were planned, but Rust builds one
  part at a time, so they would only add a second cut. A die's hand facts are one module
  its parts share (`f4/die_0x413.rs`; the H7's are `h7/common.rs`).
- **[Measured 2026-11-14]** The migration comparison: every hand-written routing row of the
  four proven parts is generated, none differs; the additions are the policy above (H7
  SDMMC2 and SPI6, the basic timers' update DMA) and two tokens the H753's hand list
  missed (DFSDM1, SWPMI1).
- It runs on demand, and as a check on pull requests into `main`
  (`tools/glue-gate.sh`, CI's "Generated glue" job: regenerate, fail on any difference). It
  does not run on development-branch pushes. Making it required is the owner's branch
  protection setting.
- Speed without narrowing: the gate always regenerates every
  part. Each part's metapac dump is cached under a key that covers everything it depends
  on (the generator's lockfile, `glue-dump` and `glue-model`, that part's feature line),
  and a key mismatch redumps; `GLUE_DUMP_CACHE` moves the cache, `off` disables it. CI
  skips the job for a pull request touching none of its inputs (the generator, `src/`, the
  bench crates under `hil/`, `hil/COVERAGE.md`, `requirements/`, the manifests and
  lockfile, `reqtool.toml`, `docs/CHIPS.md`, the workflow), and also runs it on pushes to
  `main`, whose caches are the only ones pull requests share.

## Generated documentation

- Tables that restate the registry, the metadata or the evidence are
  generated, never hand-edited: drift is a check failure.
  - `glue-gen` writes `docs/CHIPS.md`'s silicon table (part, series, die, package, core,
    clock as offered) and capability matrix (per part: each driver, the instances its
    die offers, n/a where the part has none).
  - The tier command writes each part's platform tier and capability tiers into
    `docs/CHIPS.md`, and `hil/COVERAGE.md`'s matrix per IP version, die and series, from
    reqtool's evidence.
- Each generated table sits between `<!-- generated: <tool> -->` markers;
  the prose around it (limits, roadmap, notes) stays hand-written. The merge gate
  regenerates and fails on any difference, as for the glue.

## Hand facts

- Facts metapac lacks stay hand-written beside the generated files, per die,
  each citing its manual section: clock limits, DMA-reachable RAM, calibration and
  system-memory addresses, USB core kind and FIFO depth, fractional stop bits, ADC
  internal channels, timer trigger routes (metapac's are empty), and the small code that
  goes with them (the MPU walk, the watchdog debug freeze, the revision read).

## Verification

- The generator checks itself: parts of a die agree; every row names a pin
  its package bonds; a metadata kind it does not know fails loudly.
- Migration, once: the generated glue compared row by row with the hand glue
  it replaces, which the benches proved; each difference explained before the swap.
- A glue probe per die, from the host through the
  debugger, the core reset and halted, no firmware:
  `glue-gen probe part <part>` with `PROBE_RS_PROBE` set, a `probe` run on each debugger
  bench's script. `DEV_ID` against the die table; for each offered peripheral and bonded
  port, a scratch register (one per IP version, found by name in metapac's register map)
  ignores a write with its RCC enable bit clear, keeps it once set, and returns to its
  earlier value when its reset bit is pulsed; each vector the offered peripherals and DMA
  channels raise pends and clears in the NVIC, so its number exists. Measured: 87 checks on
  die 0x433, 116 on 0x413, 154 on 0x450, all pass. Not probed, by design: a pin's
  alternate function, which AFR accepts on any pin with any value, and a vector being
  raised by its peripheral; the bench runs prove both on the wire.
- `tools/check_glue.py` is retired: `tools/glue-gate.sh`, the merge gate, regenerates
  every part's glue and the CHIPS.md tables and fails on any difference, and the glue
  probe checks the facts on the silicon. The generated `BONDED` tables went with it; the
  per-port parts carry the package.
