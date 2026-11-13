# FerroHAL: open debt

Debt is work FerroHAL owes on what it already ships: a flaw, an inconsistency, shipped
code with no hardware proof, or a missing check. Finished items move to
[RESOLVED.md](RESOLVED.md). Three things are not debt and live elsewhere (owner,
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

Status 2026-11-13: F401 21 runs, 331 checks; H753 23 runs, 560 checks; `checks/run.sh`
28 steps; reqtool 117 of 122 verified, 0 stale. No item is High.

---

## 1. Library code

- `code` `api` `bench` **Medium: `Adc`, `AdcScan`, `Sdmmc` and `UsbFs` hand nothing
  back.** ADR-002 gives every handle a `free()`; these four have none, so their
  peripherals cannot take a second role. Fix: `free()` on each, with a refusal-run
  reopen. *Spawned:* the drivers predate ADR-002 (`a8a3a05`, `4237048`, `9bd9d1e`,
  2026-11-02), and the `free()` plan's scope listed timers, UART, SPI and I2C only
  (`docs/plans/2026-11-08-free-redesign.md`).
- `code` `api` **Medium: an I2C wiring accepts a push-pull pin.** Both `into_alternate`
  forms return one type ([gpio_v2](../src/gpio_v2/mod.rs)), and push-pull NACKs every
  transfer. Fix: an open-drain type state. *Spawned:* GPIO's single alternate type
  (`af65cad`, 2026-10-31); found when run S's pins were push-pull (`cb5e339`,
  2026-11-08), recorded `f20ccc8`.
- `code` `api` **Medium: the feature hierarchy departs from `docs/design/structure.md`.**
  Chip features turn on their family marker, and two chips at once are caught only by a
  `compile_error!` listing every pair (6 at four chips, 15 at six). Same as review S6.
  *Spawned:* `af65cad` 2026-10-31, two chips; each chip since added a row.
- `code` `tool` **Medium: the routing generator doesn't exist.** Glue rows are
  hand-written; `tools/check_glue.py` reads them as text against metapac, so their
  layout is frozen, and what metapac lacks (USB FIFO depth, clock limits, memory map) is
  checked by hand. *Spawned:* `af65cad` 2026-10-31, hand-written glue for the prototype
  with the generator left open; parked by `docs/design/scope.md`. Planned: ADR-008,
  `docs/plans/2026-11-13-glue-generator.md`.
- `code` `errata` `rig` **Medium: no ES0392 2.2.10 workaround for rev-Y/W H7s.** DMA
  buffers sit in AXI SRAM, where those revisions can corrupt reads;
  `AXI_TARG7_FN_MOD.READ_ISS_OVERRIDE` fixes it and metapac lacks the register. Both H7s
  here are rev V, so no bench can show it (`docs/design/h7-errata.md`). *Spawned:* the
  W11 errata review (`3702336`, 2026-11-09).
- `code` `api` **Low: EXTI owns the whole SYSCFG token.** It needs EXTICR only; the H7's
  I/O compensation cell and the F4's memory remap sit in SYSCFG too. *Spawned:*
  `b55c640` 2026-10-31, EXTI written as SYSCFG's only user.
- `code` `api` **Low: a bonded port still splits into every pin.** `Port::split` hands out
  all sixteen, so an unbonded pin (PB11 on the LQFP64) can be named. Fix: a per-package
  `Parts`. *Spawned:* ports per register map in `af65cad`; W7 (`8c7c909`, 2026-11-09)
  removed phantom ports, not pins.
- `code` **Low: kernel clock is assumed equal to bus clock.** True on the F4, and on the H7
  only because `freeze` forces the muxes; with review D2. *Spawned:* `af65cad`, the
  UART's `Info` derived baud from the bus clock.
- `code` **Low: DMA double buffering and FIFO threshold mode have no caller and no
  test.** Fix: remove them, or give them an internal user and a run. *Spawned:* shipped
  in the DMA layer by `af65cad`; unreachable from outside since W6b2 (`a99d32f`,
  2026-11-09).
- `code` `errata` **Low: `freeze` sets PRFTEN on an F405 of any revision.** ES0182 2.2.1:
  revision A has no prefetch queue. Fix: skip it on revision A (`signature::revision`).
  *Spawned:* F405 glue (`74a8dcd`, 2026-10-31); found checking it against ES0182
  (`6dc314d`).

## 2. API consistency (the 2026-11-11 review)

Recorded by the API review (`cb58dd9`); evidence and fixes in its notes, hub
[`notes/2026-11-11-api-review.md`](notes/2026-11-11-api-review.md), which gives the
order. Re-measure before fixing. Every row is `code`, so each changed file's evidence
reruns. The spawn pattern: each driver landed in its own commit between 2026-10-31 and
2026-11-06 with no shared template, and later drivers reused the first ones' types.

| id | tags | sev | finding | spawned |
|---|---|---|---|---|
| D1 | `code` `api` | M | Instance traits have five or more shapes; nothing records which differences are deliberate. | one per driver: `af65cad` UART, `99e6404` SPI, `4070cd8` timers, `a8a3a05` ADC, `4237048` SDMMC, `cdfe190` I2C |
| D2 | `code` `api` | M | The kernel clock has three types and a `BUS` constant. | `024179f` UART `KERNEL: Bus`; `99e6404` SPI `Kernel` enum |
| D3 | `code` `api` `bench` | M | Neutral drivers expose themselves three ways, so board code names the IP version. | each driver's own re-exports, as above |
| D4 | `code` | L | SPI's traits live in its DMA engine; the polled driver imports them. | `99e6404` before the polled SPI (`66afdaa`, 2026-11-02) |
| D8 | `code` `api` | L | Refusals carry three error families. | `af65cad` `ClockError`, `1e1bbdf` IWDG, `a8a3a05` ADC, `4237048` SDMMC |
| D9 | `code` | L | `usb.rs` hard-codes PA11/PA12 AF10 outside the glue. | `9bd9d1e` 2026-11-02 |
| D10 | `code` | L | USB and SDMMC wirings have no `RESOURCES`. | `9bd9d1e`, `4237048` did not follow `2e7f7fe`'s descriptors |
| D11 | `code` `bench` | M | Two wait mechanisms: `wait::Deadline` and raw spins in nine files. | `af65cad` `STOP_SPINS` spins before `Deadline` |
| D12 | `code` `api` | L | Interrupt service is not uniform. | per driver; `1ca19a9` added a third shape |
| D13 | `code` `api` | L | Vectors come in four shapes; `verify()` on five of nine. | `2e7f7fe` UART, `b55c640` EXTI, `4070cd8` timers |
| N1 | `code` `api` | M | Refusal aliases follow four naming patterns. | `c6c5d23`, `ee05895` 2026-11-06 |
| N2 | `code` `api` | L | Module-prefix stutter is mixed; 17 clippy hits. | per driver from `af65cad` |
| N3 | `code` `api` | M | One quantity, four names: speed, frequency, rate, baudrate. | `ea7a9f8`, `99e6404`, `cdfe190` |
| N4 | `code` `api` | M | `dma::Channel` is an erased stream. | `14f7492` 2026-11-04 `claim_channel` |
| N5 | `docs` `owner` | L | "Handle" has no fixed shape. | `b55c640` `ExtiHandle`, `1e1bbdf` `Watchdog::handle` |
| N6 | `code` | L | Used `_` bindings, mixed aliases, `vdda_mv`, an unused `Block`. | `cdfe190` `Block`; others per driver |
| N7 | `code` | L | Register wrapper `Unit` vs `LineRegs`. | `99e6404` `Unit`; `4bb8398` ADR-004 named `LineRegs` |
| N8 | `code` `api` | L | Routing traits and wirings named outside N2; USB has no wiring. | `ea7a9f8`, `9bd9d1e` |
| N9 | `code` `api` | L | Vector-name constants differ by driver. | `2e7f7fe`, `4070cd8` |
| N10 | `code` `api` | L | DMA fault variant `DmaTransfer`/`DmaOverrun` vs `Dma`. | `42cbcaf` UART, then `99e6404` |
| N12 | `code` `api` | L | Health counters differ in plurality, type and overflow. | `42cbcaf`, `99e6404`, `cdfe190` |
| N13 | `code` `api` | L | RCC error variants differ F4/H7. | `af65cad`, two RCC modules |
| N14 | `code` `api` | L | `free` vs `release` (`Exti`). | `b55c640` `release`; `free` from ADR-002 |
| N15 | `code` `api` | L | `abort` vs `stop` (`Sampler`). | `ea7a9f8` |
| N16 | `code` `api` | L | `mask` vs `listen`; `listen` also means capture. | `b55c640`, then `1ca19a9` |
| N17 | `code` `api` | L | Watchdog's `start` and `WatchdogPeripheral`. | `1e1bbdf` 2026-10-31 |
| N18 | `code` `api` | L | Accessors present on some drivers only. | per driver |
| N19 | `code` `api` | L | `Config` builders and `defmt` derives differ. | per driver |
| S1 | `code` | M | F4 glue repeats per die what H7 shares (owner: after the generator). | `74a8dcd` F405/H743 glue copied per file; `036a7d2` |
| S2 | `code` | L | Timer submodules by `#[path]`; twelve lone `mod.rs`. | `ea7a9f8` |
| S3 | `code` | L | UART engine is one 1,887-line file. | grew from `af65cad` to `4bb8398` |
| S4 | `code` | L | `RESOURCES` placement differs. | `2e7f7fe` vs `cdfe190` |
| S5 | `code` | L | Clock-solver tests in chip files, named by board. | `8ad4510` 2026-11-06 |
| S7 | `code` | L | `Tick::free` repeats `stop`; F4 RCC matches by hand. | ADR-002 `free` (2026-11-10); `af65cad` |
| U1 | `code` `tool` | M | Chip glue repeats across a family's dies (owner: after the generator). | `74a8dcd` |
| U2 | `code` | M | Six RCC types identical in both RCC modules. | `af65cad`, written separately |
| U3 | `code` | M | Four drivers hand-write the one-shot DMA transfer. | `99e6404`, `a8a3a05`, `ea7a9f8` |
| U4 | `code` | M | Four near-identical lane-assembly bodies. | `ee05895` 2026-11-06 |
| U5 | `code` | L | `Resource::Pin` rows written 13 times. | from `2e7f7fe` |
| U6 | `code` | L | DMA and polled SPI repeat clock logic; `spi::Info` = `i2c::Info`. | `66afdaa` copied `99e6404`; `cdfe190` |
| U7 | `code` | L | Three unsafe byte casts exist only for `check_dma_buffer`. | `ea7a9f8`, `a8a3a05` |
| U8 | `code` | L | Wait constants repeated per file. | `af65cad`, `99e6404` |
| U9 | `code` | L | `rcc.enable(x); rcc.reset(x)` eleven times. | `af65cad` onward |
| U10 | `code` | L | `interrupt()` + `verify_vectors` three times. | `4070cd8`, `ea7a9f8` |
| U11 | `bench` | L | Two smoke benches' `usb_irq.rs` differ in 3 of 101 lines. | `ed48419` copied one into the other |
| K1 | `docs` | M | The bootloader path breaks two stated absolutes, unrecorded. | `036a7d2` 2026-11-02 |
| K4 | `docs` | L | Shared-source lists disagree (`naming.md` vs nine). | `af65cad`'s count of five; `naming.md` 2026-11-13 |
| B1 | `bench` `owner` | L | A run letter means a different test on each bench. | `55025e8` the H753 bench numbered its own runs |
| B2 | `bench` `tool` | L | H753 runs generated, F401 runs by hand. | `8b0266b` generator for the H753 only |

## 3. Shipped code without hardware proof: bench work

- `bench` `owner` **Medium: UARTDMA-009 has no method.** No bench makes a DMA stream
  refuse to stop. The owner set it aside until after FerroWasp's move to FerroHAL
  (2026-11-03); that move landed on 2026-11-06. *Spawned:* recording the UART's host
  evidence (`d905095`, 2026-11-03) found no method.
- `bench` **Medium: RESET-004 has no method on the H7.** Run N reaches the ROM, but no
  DFU device appears on CN13; the cause is not yet known (`hil/CAPABILITIES.md` queue
  3). *Spawned:* the coverage pass of 2026-11-13 (`57ae1b3`), measured.
- `tool` **Low: SDMMC's `free()` has only the Lucid's flashed run.** The Lucid bench frees
  and reopens the SDMMC, but reqtool records only `host`, `development` and `product`
  runs, so a `FLSH` run cannot count until plan step 7 gives it a tier; SDMMC's API-002
  stands on the H753 runs that compile it. *Spawned:* `free()` for SDMMC (2026-11-13),
  whose only slot is on a flashed board.
- `bench` **Low: the `usart_v2` parity clear is measured at one speed.** 115200 only; a
  faster line leaves less time before the clear's DR read. *Spawned:* run B's
  measurement when the parity-clear item closed (`f9794ed`, 2026-11-05).
- `bench` **Low: the SPI runs run with the caches off.** The UART runs cover the D-cache;
  the SPI's do not, and `PASS_CYCLES`' cached floor (233 cycles) was measured once.
  *Spawned:* the H753 SPI bench copied a caches-off board's setup (`e6c2362`), and the
  floor's one measurement (`2f09258`), both 2026-10-31.
- `bench` **Low: FIFO error handling is unconfirmed on hardware.** Transmit FIFO errors
  are cleared and ignored, receive ones are `DmaOverrun`; neither has been provoked.
  *Spawned:* `af65cad`, which took the transmit policy from the firmware the prototype
  was written for.
- `bench` `rig` **Low: SPI bit order is unchecked.** A MOSI-to-MISO loop returns the
  bytes either way, so `LsbFirst` is only shown to transfer. Fix: MOSI into an
  LSB-first receiver (a UART RX) over one more link. *Spawned:* `e6c2362` 2026-10-31,
  loopback-only SPI checks.
- `bench` **Low: the bench isn't on FerroForge.** Decided 2026-11-02: add a FerroHAL
  backend to the FerroForge CLI and move the runs onto its tasks, proving the pair.
  *Spawned:* `d773b34` recorded the decision and deferred the move.
- `bench` **Low: a Foxeer smoke image that fails before USB says nothing.** It restarts
  into DFU. *Spawned:* `926b210` 2026-11-02, a bench that reports over USB only.
- `bench` `tool` **Low: `hil/pico-i2c-sim.sh` with two probes needs both `F401_PROBE` and
  `H753_PROBE`.** `PROBE_RS_PROBE` would send both runs to one probe. *Spawned:* probe
  selection by serial (W8, `861f8b8`, 2026-11-09); recorded `f20ccc8`.
- `tool` **Low: the no-panic check proves only the paths it calls.** Nothing checks that
  every public function is in `checks/no-panic`. *Spawned:* `af65cad`, a hand-written
  call list.

## 4. Shipped code without hardware proof: rig work

- `rig` **Medium: USB-F4-001: the F4's OTG_FS core has no evidence.** The Nucleo-F401RE
  has no USB connector; the F407 Discovery needs a cable the rig lacks (queue 1).
  *Spawned:* the car-rule review split the F4 core out of USB-001 (`b15b497`,
  2026-11-13).
- `rig` **Medium: the H753 bench reaches the Zio headers only.** UART8 TX, PA8, PA9,
  PB14, PG9 and 22 of 46 TX/RX entries are unreachable without morpho headers; UART5's
  PB13 needs SB7. Their rows are glue-checked only. *Spawned:* `55025e8` 2026-10-31,
  the bench wired on Zio.
- `rig` **Low: SDMMC-003/004 and SDMMC's error paths have no Nucleo bench.** A CRC failure
  and a card pulled mid-transfer are untested; only the Lucid smoke board has a slot
  (queue 2). *Spawned:* SDMMC proven on the Lucid alone (`1b42fe4`, 2026-11-02).
- `rig` `bench` **Low: `timer_v1` complementary outputs (CHxN) are unproven.** The F401's
  only break timer, TIM1, is also its only timer whose update reaches DMA2, so the
  sampler cannot read CHxN there; `timer_v3` is proven on the H753. *Spawned:*
  `ea7a9f8` 2026-10-31, samplers need DMA2 on the F4.
- `rig` **Low: HSE crystal mode is untested on both Nucleos.** Both run HSE bypass from
  the ST-LINK's MCO. *Spawned:* `af65cad`, the benches clock from the ST-LINK.
- `rig` **Low: F401 USART2 runs through the ST-LINK,** so its break, baud and parity
  changes are unchecked; SB13/SB14 off and SB62/SB63 on would give a direct link.
  *Spawned:* `af65cad`.
- `rig` **Low: clock checks are indirect.** `freeze`'s record, working baud rates and the
  host echo; nothing measures MCO. *Spawned:* `af65cad`.
- `rig` **Low: die 0x413 (F405, F407) has no debugger bench.** Its glue rests on the
  Foxeer's flashed runs (`FLSH`); the generator's glue probe needs `DBG` or better, which
  the F407 Discovery gives once wired with a cable. Owner accepted, 2026-11-13; rig parts
  ordered. *Spawned:* ADR-009's die axis (2026-11-13), which made the gap visible.
- `rig` **Lowest: the rig cannot vary what several ES0392 rules depend on:** the HCLK/8
  SysTick path, WWDG, option bytes, flash ECC, another silicon revision. *Spawned:* the
  W11 audit (`ff3af8d`, 2026-11-10).
