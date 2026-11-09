# FerroHAL debt sweep - plan of record

Status: **in progress** - W1-W5b done (watchdog debug freeze, window, measured LSI; one-pulse; fresh-boot second uses; UART control/flag split; masking); W6-W13 pending.

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
`checks/run.sh` (17 -> 19 steps) + reqtool green.

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
| W6 | Six register accessors internal; `Stream::steal`, `Channel::start(&Transfer)`, `Channel::set_idle_buffer`, `reset::enter_bootloader` internal (`hil/common/usb_bench.rs` -> safe `reboot_to_bootloader()`); `Peripherals::steal` behind `rtic` | API-001 |
| W7 | Phantom pins removed per package + bonded-pin table, cross-checked by `tools/check_glue.py` (omissions are reported, not failed) | - |
| W8 | Probe selection by serial (env override + `probe-rs list`); today hardcoded `0483:374b`/`0483:374e` in `hil/run-*.sh:28` | - |
| W9 | Run M race: pre-build before starting the host; host waits for a **fresh** port (disappear then reappear) | - |
| W10 | Bench crates enable `rt` (VTOR; the F4 DFU leave leaves VTOR on the ROM) | - |
| W11 | Finish the ES0392 review (cache, system, flash ECC); discoveries become explicit DEBT items | - |
| W12 | Hardening: deny `clippy::undocumented_unsafe_blocks` + `unsafe_op_in_unsafe_fn`; `checks/safe-api.py`; `checks/no-metapac` compile-fail probes; architecture §5 register-access + documentation rules | G5 + API-001 |
| W13 | Migrate behaviour prose out of DEBT/RESOLVED into owning files (I2C open-drain pins, DW slave semantics -> `hil/pico-i2c-sim/src/lib.rs`, H7-vs-F401 held line -> `src/i2c`, probe-rs reset catch -> scripts + `requirements/benches.md`, H7 doubleword -> `src/reset.rs`) | - |

Detail that must not be lost, the gates and the cross-repo impact:
[2026-11-08-debt-sweep-gates.md](2026-11-08-debt-sweep-gates.md).
