"""Checks G5's safe-API rules on the sources, no build needed.

    python3 -I tools/check_safe_api.py

What it enforces (docs/policies/safety.md):

- no `pub unsafe fn` anywhere in `src/` beyond one allowlist entry: the
  `steal` that `src/chips/shared/macros.rs` defines, which must sit behind
  `#[cfg(feature = "rtic")]`;
- the feature table keeps that exception opt-in: `default = []`,
  `rtic = ["rt"]`, and no other feature implies `rtic`;
- the `rtic` feature adds that one item and nothing else: the only mention of
  `feature = "rtic"` in `src/` is its gate (owner, 2026-11-13: RTIC gets the
  absolute minimum it needs);
- the `device` module RTIC's `device = ...` names exports exactly what RTIC's
  generated code uses: `Interrupt` (and `interrupt`), `NVIC_PRIO_BITS` and
  `Peripherals`;
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

# The rtic feature gates the allowlist entry and nothing else.
gates = []
for path in sorted(root.glob("src/**/*.rs")):
    relative = path.relative_to(root).as_posix()
    for index, line in enumerate(path.read_text().splitlines()):
        if re.search(r'feature\s*=\s*"rtic"', line):
            gates.append((relative, index))
steal_gate = []
for relative, name in ALLOWED_UNSAFE.items():
    lines = (root / relative).read_text().splitlines()
    for index, line in enumerate(lines):
        if re.match(rf"\s*pub unsafe fn {name}\b", line):
            steal_gate += [(relative, i) for i in range(max(0, index - 4), index)]
for relative, index in gates:
    if (relative, index) not in steal_gate:
        offenders.append(f"{relative}:{index + 1}: the rtic feature gates more than steal")

# The device module exports exactly what RTIC's generated code names
# (rtic-macros 2.4.0: codegen/module.rs, codegen/bindings/cortex.rs).
RTIC_NAMES = {"Interrupt", "interrupt", "NVIC_PRIO_BITS", "Peripherals"}
device = (root / "src/device.rs").read_text()
code = "\n".join(l for l in device.splitlines() if not l.lstrip().startswith("//"))
exported: set[str] = set()
for group in re.findall(r"pub use [\w:]+::\{([^}]*)\};", code):
    exported |= {name.strip().split(" as ")[-1] for name in group.split(",") if name.strip()}
exported |= set(re.findall(r"pub use [\w:]+::(\w+);", code))
exported |= set(re.findall(r"pub use [\w:]+ as (\w+);", code))
others = re.findall(r"^\s*pub (?:fn|struct|enum|const|static|mod|type|trait|macro) (\w+)", code, re.M)
if exported != RTIC_NAMES or others:
    offenders.append(
        f"src/device.rs: exports {sorted(exported | set(others))}, RTIC names {sorted(RTIC_NAMES)}"
    )

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
    " rtic gates nothing else; device exports only what RTIC names;"
    " metapac confined to chips/ + the register-speaking modules"
)
