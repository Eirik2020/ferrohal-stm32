"""Checks that every firmware crate linking FerroHAL enables `rt`.

    python3 -I tools/check_firmware_features.py

`rt` turns on `cortex-m-rt`'s `set-vtor`, so a firmware's reset handler takes
VTOR back to its own vector table. The F4's ROM bootloader leaves for the
application by jumping there, with no reset, so without it VTOR still points at
the ROM and an interrupt runs the ROM's handler: the Foxeer's USB never
enumerated after a DFU `leave` until this feature was on. A probe flash resets
the chip, which hides the fault, so the bench must not depend on how it was
flashed.

Every `hil/*/Cargo.toml` and `checks/*/Cargo.toml` that names `ferrohal-stm32`
as a dependency must enable `rt` on it, directly or through `rtic` (the crate's
`rtic = ["rt"]`). Crates with no such dependency, the RP2040 simulator for one,
are listed as skipped. Exits 1 naming the crate that lacks it.
"""

import pathlib
import sys
import tomllib

root = pathlib.Path(__file__).resolve().parent.parent
missing: list[str] = []
enabled: list[str] = []
skipped: list[str] = []

for manifest in sorted(
    list(root.glob("hil/*/Cargo.toml")) + list(root.glob("checks/*/Cargo.toml"))
):
    crate = str(manifest.parent.relative_to(root))
    data = tomllib.loads(manifest.read_text())
    dependency = data.get("dependencies", {}).get("ferrohal-stm32")
    if dependency is None:
        skipped.append(crate)
        continue
    features = dependency.get("features", [])
    # `rtic` implies `rt` in FerroHAL's own feature table.
    if "rt" in features or "rtic" in features:
        enabled.append(crate)
    else:
        missing.append(f"{crate}: features = {features}")

if missing:
    print("FAIL  a firmware crate links FerroHAL without rt (or rtic, which implies it):")
    for crate in missing:
        print(f"  {crate}")
    sys.exit(1)

print(
    f"pass  firmware crates enable rt ({len(enabled)}: {', '.join(enabled)}; "
    f"no FerroHAL dependency: {', '.join(skipped)})"
)
