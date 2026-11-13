#!/usr/bin/env python3
"""Checks FerroHAL's names against docs/design/naming.md (ADR-003, W14).

    python3 -I tools/check_names.py

- Every requirement ID is <FAMILY>-<NNN> or <FAMILY>-<PART>-<NNN>, upper case.
- Every ID is defined once, in the catalog its family belongs to: FAMILIES below
  lists each catalog's families, and a family belongs to exactly one catalog.
  Every catalog in src/requirements/ is listed, and nothing else is.
- Every ID a `//! Requirements:` trailer names (a line ending in a comma
  continues on the next) is defined.
- Every ID the bench code (hil/) or a procedure (requirements/*.md) names is
  defined, so a renamed or removed requirement leaves no stale tag behind.
- Every Cargo feature is a chip (metapac's name), an internal one (leading
  underscore), a named option, or an IP version: <ip>_v<N> or <ip>_<family>
  (the H7 RCC adds its manual: rcc_h7_rm0433).

Exits non-zero naming each failure; it would have caught the USART-001
collision ADR-003 records.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Catalog (src/requirements/<name>.yaml) -> the families it holds.
FAMILIES: dict[str, list[str]] = {
    "adc": ["ADC"],
    "api": ["API"],
    "dma": ["DMA"],
    "exti": ["EXTI"],
    "glue": ["GLUE"],
    "gpio": ["GPIO"],
    "i2c": ["I2C", "I2C-V1", "I2C-V2"],
    "iwdg": ["IWDG", "IWDG-V2"],
    "rcc": ["RCC"],
    "reset": ["RESET", "RESET-H7"],
    "resource": ["RESOURCE"],
    "sdmmc": ["SDMMC"],
    "signature": ["UID", "REVISION"],
    "spi": ["SPI", "SPIPOLL", "SPI-V2", "SPI-V4"],
    "timer": ["TIMER", "PWM", "AWG"],
    "uart": ["UART", "UARTDMA", "USART-V2", "USART-V4"],
    "usb": ["USB", "USB-F4"],
    "wait": ["WAIT"],
}

# Features that are neither a chip, an internal one nor an IP version.
OPTIONS = {"default", "rt", "rtic", "defmt", "fugit", "usb"}

# ID-shaped tokens that are not requirements.
NOT_IDS = {"SHA-256"}

ID = re.compile(r"^[A-Z][A-Z0-9]*(-[A-Z][A-Z0-9]*)?-\d{3}$")
# An ID where it is mentioned: not part of a longer token.
MENTION = re.compile(r"(?<![A-Za-z0-9_-])([A-Z][A-Z0-9]*(?:-[A-Z][A-Z0-9]*)?-\d{3})(?![0-9])")
DEFINED = re.compile(r"^  - id: (\S+)\s*$", re.M)
TRAILER = re.compile(r"^//! Requirements: (.*)$")
# <ip>_v<N>, <ip>_<family> (the families FerroHAL supports), or the RCC by family and
# manual where one family has several RCC shapes.
IP_FEATURE = re.compile(r"^[a-z][a-z0-9]*_(v\d+|f4|h7|h7_rm\d{4})$")
CHIP = re.compile(r"^stm32[a-z0-9]+$")


def family(ident: str) -> str:
    return ident.rsplit("-", 1)[0]


def trailer_ids(text: str) -> list[str]:
    ids: list[str] = []
    continuing = False
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped.startswith("//!"):
            continuing = False
            continue
        doc = stripped[3:].strip()
        match = TRAILER.match(stripped)
        if match:
            listed = match.group(1)
        elif continuing:
            listed = doc
        else:
            continue
        ids += [part.strip() for part in listed.split(",") if part.strip()]
        continuing = listed.rstrip().endswith(",")
    return ids


def features() -> list[str]:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    section = text.split("[features]", 1)[1]
    section = re.split(r"(?m)^\[", section, maxsplit=1)[0]
    return re.findall(r"(?m)^([A-Za-z0-9_-]+)\s*=", section)


def main() -> int:
    failures: list[str] = []

    owner: dict[str, str] = {}
    for catalog, families in FAMILIES.items():
        for name in families:
            if name in owner:
                failures.append(f"family {name} is listed for {owner[name]} and {catalog}")
            owner[name] = catalog

    catalogs = sorted((ROOT / "src/requirements").glob("*.yaml"))
    listed = {path.stem for path in catalogs}
    for missing in sorted(set(FAMILIES) - listed):
        failures.append(f"src/requirements/{missing}.yaml is listed in FAMILIES but missing")
    for extra in sorted(listed - set(FAMILIES)):
        failures.append(f"src/requirements/{extra}.yaml holds families FAMILIES does not list")

    defined: dict[str, str] = {}
    for path in catalogs:
        relative = path.relative_to(ROOT).as_posix()
        for ident in DEFINED.findall(path.read_text(encoding="utf-8")):
            if not ID.match(ident):
                failures.append(f"{relative}: {ident} is not <FAMILY>-<NNN> or <FAMILY>-<PART>-<NNN>")
            if ident in defined:
                failures.append(f"{ident} is defined in {defined[ident]} and {relative}")
                continue
            defined[ident] = relative
            home = owner.get(family(ident))
            if home is None:
                failures.append(f"{relative}: {ident}'s family {family(ident)} is in no catalog's list")
            elif home != path.stem:
                failures.append(f"{relative}: {ident} belongs in src/requirements/{home}.yaml")

    for path in sorted((ROOT / "src").rglob("*.rs")):
        relative = path.relative_to(ROOT).as_posix()
        for ident in trailer_ids(path.read_text(encoding="utf-8")):
            if ident not in defined:
                failures.append(f"{relative}: its trailer names {ident}, which no catalog defines")

    named = sorted(
        path
        for path in list((ROOT / "hil").rglob("*.rs")) + list((ROOT / "requirements").glob("*.md"))
        if "target" not in path.parts
    )
    for path in named:
        relative = path.relative_to(ROOT).as_posix()
        for ident in sorted(set(MENTION.findall(path.read_text(encoding="utf-8")))):
            if ident not in defined and ident not in NOT_IDS:
                failures.append(f"{relative}: names {ident}, which no catalog defines")

    for feature in features():
        if feature in OPTIONS or feature.startswith("_") or CHIP.match(feature):
            continue
        if not IP_FEATURE.match(feature):
            failures.append(f"Cargo.toml: feature {feature} is not <ip>_v<N> or <ip>_<family>")

    if failures:
        for failure in failures:
            print(f"FAIL  {failure}")
        return 1
    print(
        f"pass  names ({len(defined)} requirements in {len(catalogs)} catalogs, "
        f"{len(owner)} families)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
