# Bench Procedures: the host analyses

The checks of [benches.md](benches.md) that run on the host, not a board, split
out to keep that file in its budget; the same rules apply.

## No reachable panic

`checks/run.sh`'s `no reachable panic` step builds `checks/no-panic` per chip in
release. It calls every public function with optimiser-opaque inputs, and its
panic handler calls an undefined symbol, so the image links only if no panic
path survives. Its `no-panic calls every public function` step
(`tools/check_no_panic_coverage.py`) holds that claim: every `pub fn` reachable
from outside the crate is named in that program, or listed in
`checks/no-panic/uncalled.txt` with a reviewed reason; a method counts by its
name, so two sharing one count together. Pass: both steps pass for every chip.
A new public function is added to that crate [API-001].

## Chip glue against metapac

`tools/glue-gate.sh --no-tiers`, the generated glue's merge gate without the tier
tables (those follow from the evidence, and CI checks them with the full gate): it
regenerates every part's routing module and `docs/CHIPS.md`'s silicon tables from the
pinned stm32-metapac's metadata
(`tools/glue-gen`, which also checks that the parts of a die agree), runs the
generator's tests, and compares with what is checked in. Pass: no difference, and it
exits zero.

## One I2C API on every chip

`checks/run.sh`'s `no reachable panic` step builds `checks/no-panic`, one source,
for each of the four chips: F4s on `i2c_v1`, H7s on `i2c_v2`. That source opens the
bus as `ferrohal_stm32::i2c::I2c::new` and runs every operation of the embedded-hal
bus trait through it, so it compiles only if every chip reaches the same driver API
by the same name. Pass: the step passes for every chip [I2C-001].
