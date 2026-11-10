# Review 2026-11-10: remaining findings and cross-checks

Second part of [REVIEW-2026-11-10.md](REVIEW-2026-11-10.md), split off to keep each
file within its budget.

## Remaining findings

Recorded in `DEBT.md` (`f20ccc8`), not fixed:

- **`hil/run-i2c-sim.sh` with two probes attached** fails unless both `F401_PROBE`
  and `H753_PROBE` are set. Measured: exit 1, with both scripts listing the probes.
  `hil/probe.sh` ranks `PROBE_RS_PROBE` above the board variables, so setting it
  would send both runs to one probe (inferred). The `flash` path discards run T's
  output, so a probe it cannot choose ends in the BOOTSEL hint (inferred).
- **`hil/run-dfu.sh`** picks the board by flash layout. The H743VI and H753ZI both
  list `16*128Kg`, so a Nucleo-H753ZI in DFU could take the Lucid's image. It also
  ignores `dfu-util`'s exit status. Inferred from the script.

Not recorded in `DEBT.md`; left for the owner to decide:

- **Output speed (inferred).** No driver sets OSPEEDR, and of the benches only the
  Lucid's SPI1 SCK calls `set_speed`, so SDMMC at 25 MHz and SPI at 10 MHz probably
  run on low-speed pins. They pass on the benches; nothing has checked them against
  the datasheets' I/O speed tables. It was left out of `DEBT.md` because it is
  unmeasured and `DEBT.md` is at its ratchet.
- **`hil/probe.sh`** indents only the first probe it lists (measured, cosmetic).
- **Boot 1's checks in runs J and K** (fresh start, debug freeze) are printed but not
  carried into the run's exit status, which only the last boot sets. Run K's boot 2
  now carries its failures; `hil/common/watchdog.rs` does not (inferred).
- **The workspace `CLAUDE.md`** still says ferro-wasp doesn't use `ferrohal-stm32`
  yet. That file is outside this repository.

## Checked and found correct

Inferred from code against the cited documents unless marked measured.

- `signature::revision` reads `DBGMCU_IDC.REV_ID`, and ES0392 Table 2 maps 0x2003 to
  revision V. 2.2.10 is absent on V and 2.2.21's workaround is revision V or X, as
  RESOLVED says.
- Watchdog debug freeze: F4 `DBGMCU_APB1FZ` bit 12, H7 `DBGMCU_APB4FZR1` bit 18.
- `clear_tc` writes 0 at SR bit 9, the CTS bit that UART4/5 reserve (RM0090 30.6.1).
- G5: the only public `unsafe fn` is `Peripherals::steal`; `Channel::start`,
  `Channel::alias` and `enter_bootloader` are `pub(crate)`.
- Both `embedded_io::Read::read` impls point to `UartRx::read_within`.
- `check_dma_buffer` on the H7: when unsure, it counts memory as cacheable and
  refuses the buffer (partial overlap, a disabled subregion, a reserved TEX/C/B
  encoding).
- The F4 voltage scale is written with the PLL off, and `freeze` waits for VOSRDY,
  which RM0090 lists for the F405.
- `reboot_to_bootloader` fills its doubleword and cleans it from the D-cache.
- `Pwm::start_one_pulse` latches with UG while the mode is off, then starts.
- All three FerroWasp barometer buses use `into_alternate_open_drain`.
- Measured: the Lucid reports `rev: 0x2003 (V)`, as RESOLVED says.

## How to cross-check

From the repository root, with both Nucleos attached (probe serials from
`probe-rs list`):

    git log --oneline 94eae92..debt-iwdg-window
    checks/run.sh
    reqtool validate
    H753_PROBE=<H753 serial> hil/run-h753.sh k
    F401_PROBE=<F401 serial> H753_PROBE=<H753 serial> hil/run-i2c-sim.sh

Run K should report 9 PASS lines, among them
`fed 5 times 85 ms after each reload, inside the window: no reset [IWDG-007]` and
`a feed before the window opens resets the chip [IWDG-005]`. To see the check catch
the old defect, put back the old driver with `git show 94eae92:src/iwdg.rs > src/iwdg.rs`
and run run K again: it fails IWDG-007. Restore with `git checkout src/iwdg.rs`.

The Nucleo-H753ZI's revision: `probe-rs read --chip STM32H753ZI b32 0x5C001000 1`
should print `20036450`.

Not re-verified after the fix: the full `hil/run-h753.sh` (only run K changed) and
`hil/run-f401.sh` (no F401 code changed). FerroWasp, which pins FerroHAL `b6d8e84`,
does not get the fix until that pin moves; a search of its crates and firmware finds no `start_windowed` call.
