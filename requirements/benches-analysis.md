# Bench Procedures: the host analyses

The checks of [benches.md](benches.md) that run on the host, not a board, split
out to keep that file in its budget; the same rules apply.

## No reachable panic

`checks/run.sh`'s `no reachable panic` step builds `checks/no-panic` per chip in
release. It calls every public function with optimiser-opaque inputs, and its
panic handler calls an undefined symbol, so the image links only if no panic
path survives. Pass: the step passes for every chip. A new public function is
added to that crate [API-001].

## Chip glue against metapac

`python3 -I tools/check_glue.py`, which compares each chip's routing rows with
stm32-metapac's metadata. Pass: it exits zero. It reads the rows as text, so their
layout must stay as it is.

## One I2C API on every chip

`checks/run.sh`'s `no reachable panic` step builds `checks/no-panic`, one source,
for each of the four chips: F4s on `i2c_v1`, H7s on `i2c_v2`. That source opens the
bus as `ferrohal_stm32::i2c::I2c::new` and runs every operation of the embedded-hal
bus trait through it, so it compiles only if every chip reaches the same driver API
by the same name. Pass: the step passes for every chip [I2C-001].
