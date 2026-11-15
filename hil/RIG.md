# Rig set-up

What every bench script and firmware crate relies on before a procedure in
[benches.md](../requirements/benches.md) runs. Split out of that file on 2026-11-10, when it reached
its budget; the text is unchanged.

## Which probe

`hil/probe.sh` picks the probe by its serial, as `probe-rs list` prints it after
`--` (`VID:PID:Serial`): `PROBE_RS_PROBE` if already set, else the board's own
variable (`F401_PROBE` for `hil/run-f401.sh`, `H753_PROBE` for `hil/run-h753.sh`,
`DISCOVERY_PROBE` for `hil/run-discovery.sh`),
else the only probe attached, else the one probe whose die has the bench's DEV_ID
(0x433 the F401, 0x450 the H753, 0x413 the Discovery; read without halting). With
none or several matching it lists the probes and exits before building.
`hil/pico-i2c-sim.sh` drives two boards, so it clears `PROBE_RS_PROBE` for each run. The
choice reaches probe-rs through its `PROBE_RS_PROBE` variable, which the crate
runners in `hil/*/.cargo/config.toml` leave unset. `hil/run-f401.sh`'s host echo
runs over the chosen probe's own virtual COM port; `F401_VCP` overrides the path.

## Firmware crates enable `rt`

`python3 -I tools/check_firmware_features.py`, a `checks/run.sh` step. Every crate
under `hil/` and `checks/` that depends on `ferrohal-stm32` must name the crate's
`rt` feature, or `rtic`, which implies it. `rt` is `cortex-m-rt`'s `set-vtor`: the
F4's ROM bootloader leaves for the application by jumping, with no reset, so
without it VTOR still points at the ROM and an interrupt runs the ROM's handler —
the Foxeer's USB never enumerated after a DFU `leave` until the feature was on. A
probe flash resets the chip and hides the fault, so tests can't rely on how the
image arrived. Pass: it exits zero, naming the five firmware crates and the
simulator it skips.

## Reset catch

probe-rs halts a core it sees reset, which would stop a run's second boot from
ever running. The scripts pass `--no-catch-reset` for the runs that reset on
purpose — the watchdog runs J and K, the bootloader run L, and the refusal run
V — and those runs wait after the reset so the host finds RTT again.

## The fitted parts

Every run's log ends with the identity pair — `device_uid` and the die
revision, `DBGMCU_IDC.REV_ID` — so the revision a check ran on is in the
record. The fitted part itself is a board constant, the type each board's
manual and chip module name:

- **Nucleo-F401RE**: STM32F401RET6, die revision 0x1001.
- **Nucleo-H753ZI**: STM32H753ZIT6, die revision 0x2003 (V).
- **Foxeer F405 V2**: STM32F405RGT6, die revision 0x100f.
- **Lucid H743**: STM32H743VIH6 (TFBGA100), die revision 0x2003 (V).

ES0392's per-part rules — 2.2.10's AXI SRAM reads, revision Y/W only, and
2.2.21's 480 MHz — are decided off the revision: both H7s here are V.

## Identity

`python3 -I hil/rig_status.py` lists what is on the rig without disturbing it: every
debug probe with its die (IDCODE, UID) and the identity of the image it runs, and every
USB serial port other than an ST-LINK's with its answer to `FERRO-ID?`. Nothing is
flashed, reset or halted.

The identity is one line, `FERRO-ID v=1` and then `key=value` pairs separated by
spaces: `board`, `part`, `crate`, `run`, `git` (the short commit, `+dirty` when tracked
files differed). Readers ignore keys they do not know; a new key needs no new version,
a changed meaning does.

- **Over the debug probe** (the Nucleos, the STM32F4 Discovery): every bench image
  carries the line NUL-padded to 256 bytes at 0x0800_0400, just after the vector table
  (`hil/common/ident.rs`, each bench's `memory.x` and `build.rs`). The host adds the die
  it reads itself, so an erased chip still shows its IDCODE and UID.
- **Over USB serial** (boards with no probe: the Pico, flashed boards): the firmware
  answers the query `FERRO-ID?` and a newline with the line, adding `dev`, `rev` and
  `uid` it reads itself.

