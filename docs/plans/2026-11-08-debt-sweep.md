# FerroHAL debt sweep - plan of record

Status: **done** - W1-W7 (watchdog debug freeze, window, measured LSI; one-pulse; fresh-boot second uses; UART control/flag split; masking; `steal` behind `rtic`; register accessors and unsafe hatches internal; API-001; package tables and phantom ports), W8 (probe selection by serial), W9 (run M race), W10 (`rt` check), W11 (ES0392 review), W12 (hardening) and W13 (behaviour prose migrated to its owners) are all landed.

## Prerequisites (in order)
1. **AWG-011 PR first**, own branch off `origin/main` (140697c): 3 files, +56/-4 -
   `hil/common/pwm.rs` (+37), F401 `board.rs` RunO pull-up (PC7 held as a pulled-up
   input so a released wire reads a level), two `src/requirements/timer.yaml`
   procedure anchors. Its anchors do not touch the line `PWM-007` will use.
2. Commit or park the AWG-011 leftovers before branching; `ferrosmith-board-schema.md`
   stays **untracked** and out of every PR.
3. Then `debt-sweep` branch off `origin/main`.

## Non-negotiables
See ../policies/safety.md. Summary: G5 (no public `unsafe fn`; single
`Peripherals::steal` behind `rtic` feature); behaviour documented in the owning file;
DEBT/RESOLVED are pointer-only indexes; one commit per item with its requirement and
`checks/run.sh` (21 steps) + reqtool green.

## Items (one commit each)
| # | Work | Requirement |
|---|---|---|
| W1 | DBGMCU watchdog freeze in `Watchdog::start` (F4 `DBGMCU_CR.iwdg` bit 12; H7 `DBGMCU_APB1LFZR1.iwdg1` bit 18) | IWDG-004 |
| W2 | `Watchdog::start_windowed` on `iwdg_v2` only (`winr`) | IWDG-005 |
| W3 | `Watchdog::start_with_lsi(iwdg, timeout_us, lsi: Hertz)`; `start()` stays nominal; board constants from the bench: F401 ~30.0 kHz, H753 ~31.7 kHz | IWDG-006 |
| W4 | `Pwm::start_one_pulse()` (OPM bit) | PWM-007 |
| W4b | Move the six "second use" checks (`run_h.rs:188,193,194`; `run_i.rs:172,177,178`) into fresh-boot runs; delete `tokens!::steal`; DEBT design brief for `free()` | - |
| W5a | UART control/flag split: a `Line` handle owns CR1-CR3, BRR and PRESC; `UartIrq` keeps flag servicing; `split`/`halves`/`UartRx::new`/`send_break`/`set_config`/`regs` move (ADR-004) | - |
| W5b | `Line::mask()/unmask()` toggling IDLEIE, PEIE, EIE | **USART-005** (001-004 taken) |
| W6a | `Peripherals::steal` behind default-off `rtic`; `take` uses an internal `steal_unchecked`; every RTIC bench and no-panic enable `rtic` (policy exception 1) | - |
| W6b1 | Six register accessors internal: `DmaInstance::regs`/`mux`, `PortInstance::regs`, SDMMC `Instance::regs` move to crate-private supertraits; `Channel::regs` and the UART line `regs` removed | - |
| W6b2 | `Stream::steal`, `Channel::start(&Transfer)`, `Channel::set_idle_buffer`, `reset::enter_bootloader` internal; `usb_bench`/no-panic use safe paths; API-001 | API-001 |
| W7 | Phantom pins removed per package + bonded-pin table, cross-checked by `tools/check_glue.py` (omissions are reported, not failed) | - |
| W8 | Probe selection by serial: `hil/probe.sh` resolves `PROBE_RS_PROBE`, then `F401_PROBE`/`H753_PROBE`, then the only probe attached, and lists the candidates when it cannot choose; crate runners drop `--probe`, F401's host echo follows the chosen serial | - |
| W9 | Run M race: `run-m` is built before the host starts, and the host opens only a port that appeared after the previous image's went (inode/realdir signature), so a rerun cannot open the image it is about to replace | - |
| W10 | Every firmware crate linking FerroHAL enables `rt` (or `rtic`, which implies it); `tools/check_firmware_features.py` in `checks/run.sh` names one that doesn't. `rt` is what takes VTOR back after the F4's ROM bootloader jumps | - |
| W11 | ES0392 review finished for cache, system and flash ECC; verdicts in `docs/design/h7-errata.md`, two DEBT items (2.2.10 AXI SRAM rev-Y, flash-ECC constraints) | - |
| W12 | Hardening: deny `clippy::undocumented_unsafe_blocks` + `unsafe_op_in_unsafe_fn`; `checks/safe-api.py`; `checks/no-metapac` compile-fail probes; architecture §5 register-access + documentation rules | G5 + API-001 |
| W13 | Migrate behaviour prose out of DEBT/RESOLVED into owning files (I2C open-drain pins, DW slave semantics -> `hil/pico-i2c-sim/src/lib.rs`, H7-vs-F401 held line -> `src/i2c`, probe-rs reset catch -> scripts + `requirements/benches.md`, H7 doubleword -> `src/reset.rs`) | - |

Detail that must not be lost, the gates and the cross-repo impact:
[2026-11-08-debt-sweep-gates.md](2026-11-08-debt-sweep-gates.md).
