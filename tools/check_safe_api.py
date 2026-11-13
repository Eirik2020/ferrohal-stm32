"""Checks G5's safe-API rules on the sources, no build needed.

    python3 -I tools/check_safe_api.py

What it enforces (docs/policies/safety.md):

- no `pub unsafe fn` anywhere in `src/` beyond one allowlist entry: the
  `steal` that `src/chips/shared/macros.rs` defines, which must sit behind
  `#[cfg(feature = "rtic")]`;
- the feature table keeps that exception opt-in: `default = []`,
  `rtic = ["rt"]`, and no other feature implies `rtic`;
- metapac imports stay confined to the chip glue, the RTIC `device` module
  and the IP modules that speak registers, so no new module can quietly
  grow a dependency on the PAC.

Exits 1 naming every offender.
"""

import pathlib
import re
import sys
import tomllib

root = pathlib.Path(__file__).resolve().parent.parent

# The one allowed `pub unsafe fn`: file -> item name.
ALLOWED_UNSAFE = {"src/chips/shared/macros.rs": "steal"}

# Where `stm32_metapac` may appear: the RTIC device module, and the
# one-file IP modules. The directory set below covers the rest.
METAPAC_FILES = {"device.rs", "dma_stream.rs", "exti.rs", "iwdg.rs",
                 "timer.rs", "timer_pwm.rs"}
METAPAC_DIRS = {"adc_v2", "adc_v4", "gpio_v2", "i2c_v1", "i2c_v2", "rcc_f4",
                "rcc_h7_rm0433", "sdmmc_v2", "spi_v2", "spi_v4", "usart_v2",
                "usart_v4", "chips"}

offenders: list[str] = []

# No public unsafe fn beyond the allowlist.
for path in sorted(root.glob("src/**/*.rs")):
    relative = path.relative_to(root).as_posix()
    lines = path.read_text().splitlines()
    for index, line in enumerate(lines):
        match = re.match(r"\s*pub unsafe fn (\w+)", line)
        if not match:
            continue
        name = match.group(1)
        if ALLOWED_UNSAFE.get(relative) == name:
            # The allowlist entry must sit behind the feature gate.
            window = lines[max(0, index - 4):index]
            if not any('[cfg(feature = "rtic")]' in item for item in window):
                offenders.append(
                    f"{relative}:{index + 1}: allowlist {name} not behind rtic"
                )
        else:
            offenders.append(f"{relative}:{index + 1}: pub unsafe fn {name}")

# The feature table keeps the exception opt-in.
features = tomllib.loads((root / "Cargo.toml").read_text()).get("features", {})
if features.get("default"):
    offenders.append(f"Cargo.toml: default = {features['default']}")
if features.get("rtic") != ["rt"]:
    offenders.append(f"Cargo.toml: rtic = {features.get('rtic')}")
for name, enables in features.items():
    if name != "rtic" and "rtic" in enables:
        offenders.append(f"Cargo.toml: feature {name} implies rtic")

# Metapac imports confined.
for path in sorted(root.glob("src/**/*.rs")):
    if "stm32_metapac" not in path.read_text():
        continue
    parts = path.relative_to(root).parts
    if parts[1] == "chips":
        continue
    if len(parts) == 2 and parts[1] in METAPAC_FILES:
        continue
    if len(parts) > 2 and parts[1] in METAPAC_DIRS:
        continue
    offenders.append(f"{'/'.join(parts[1:])}: imports stm32_metapac")

if offenders:
    print("FAIL  safe api (G5):")
    for offender in offenders:
        print(f"  {offender}")
    sys.exit(1)

print(
    "pass  safe api (G5): pub unsafe fn only Peripherals::steal behind rtic;"
    " metapac confined to chips/ + the register-speaking modules"
)
