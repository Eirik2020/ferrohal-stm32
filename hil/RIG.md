# Rig set-up

What every bench script and firmware crate relies on before a procedure in
[benches.md](../requirements/benches.md) runs. Split out of that file on 2026-11-10, when it reached
its budget; the text is unchanged.

## Which probe

`hil/probe.sh` picks the probe by its serial, as `probe-rs list` prints it after
`--` (`VID:PID:Serial`): `PROBE_RS_PROBE` if already set, else the board's own
variable (`F401_PROBE` for `hil/run-f401.sh`, `H753_PROBE` for `hil/run-h753.sh`),
else the only probe attached. With more than one attached and none chosen it lists
them and exits before building, so a shared bench says which board it flashes. The
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
