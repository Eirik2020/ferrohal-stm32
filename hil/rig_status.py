"""What is on the rig: every attached debug probe and USB serial port, and what
each board says it is and runs (`FERRO-ID v=1`, hil/RIG.md, Identity).

    python3 -I hil/rig_status.py

Over each ST-LINK, without halting the core: the die's IDCODE and UID, and the
identity block every bench image carries at 0x0800_0400 (hil/common/ident.rs).
An erased chip, or an image without the block, shows its die alone. Over each
USB serial port that is not an ST-LINK's: `FERRO-ID?` and the line that comes
back, for the boards with no probe (the Pico, flashed boards).

Read-only: nothing is flashed, reset or halted, and no ST-LINK's serial port is
opened, so a run in progress elsewhere is not disturbed beyond the probe's
shared bus.
"""

import glob
import os
import re
import select
import subprocess
import termios
import time

IDENT = 0x0800_0400
IDENT_LEN = 256

# IDCODE register: the H7's DBGMCU sits elsewhere; every other family here
# has it at 0xE0042000.
IDCODE = (("Cortex-M4", 0xE004_2000), ("Cortex-M7", 0xE004_2000), ("Cortex-M7", 0x5C00_1000))

# By DEV_ID: the part family and where its 96-bit UID lies.
FAMILIES = {
    0x413: ("STM32F405/407", 0x1FFF_7A10, "Cortex-M4"),
    0x433: ("STM32F401xD/E", 0x1FFF_7A10, "Cortex-M4"),
    0x450: ("STM32H742/743/753/750", 0x1FF1_E800, "Cortex-M7"),
    0x451: ("STM32F76x/77x", 0x1FF0_F420, "Cortex-M7"),
    0x469: ("STM32G47x/48x", 0x1FFF_7590, "Cortex-M4"),
}


def probes():
    out = subprocess.run(["probe-rs", "list"], capture_output=True, text=True).stdout
    return re.findall(r"\[\d+\]: (.+?) -- ([0-9a-fA-F]{4}:[0-9a-fA-F]{4}:\w+)", out)


def read(chip, probe, width, address, count):
    """Words (or bytes) from the target, or None."""
    try:
        out = subprocess.run(
            ["probe-rs", "read", "--chip", chip, "--probe", probe, width, hex(address), str(count)],
            capture_output=True, text=True, timeout=30,
        )
    except subprocess.TimeoutExpired:
        return None
    if out.returncode != 0:
        return None
    values = []
    for line in out.stdout.splitlines():
        if re.match(r"^[0-9a-f]{8}:", line):
            values += [int(v, 16) for v in line.split(":", 1)[1].split()]
    return values or None


def probe_report(name, probe):
    idcode, core = None, None
    for chip, address in IDCODE:
        words = read(chip, probe, "b32", address, 1)
        if words and words[0] & 0xFFF in FAMILIES:
            idcode, core = words[0], chip
            break
    if idcode is None:
        return f"{probe}  {name}: no STM32 die answers (unpowered, held in reset, or not listed)"
    dev, rev = idcode & 0xFFF, idcode >> 16
    family, uid_at, core = FAMILIES[dev]
    uid = read(core, probe, "b32", uid_at, 3)
    uid_text = "".join(f"{w:08x}" for w in reversed(uid)) if uid else "?"
    block = read(core, probe, "b8", IDENT, IDENT_LEN)
    line = bytes(block[: block.index(0)] if block and 0 in block else []).decode("ascii", "replace") if block else ""
    die = f"dev=0x{dev:03x} rev=0x{rev:04x} uid={uid_text} ({family})"
    if line.startswith("FERRO-ID "):
        return f"{probe}  {line}  {die}"
    return f"{probe}  {name}: no identity block (erased, or an image without one)  {die}"


def serial_ports():
    for path in sorted(glob.glob("/dev/serial/by-id/*")):
        if "STLink" in path or "STLINK" in path:
            continue
        yield path


def ask(path, wait=1.0):
    """The `FERRO-ID` line the port answers `FERRO-ID?` with, or None."""
    try:
        fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    except OSError as error:
        return f"cannot open: {error.strerror}"
    try:
        attrs = termios.tcgetattr(fd)
        attrs[3] &= ~(termios.ICANON | termios.ECHO)
        termios.tcsetattr(fd, termios.TCSANOW, attrs)
        termios.tcflush(fd, termios.TCIOFLUSH)
        os.write(fd, b"FERRO-ID?\n")
        got, end = b"", time.monotonic() + wait
        while time.monotonic() < end:
            ready, _, _ = select.select([fd], [], [], 0.05)
            if ready:
                got += os.read(fd, 512)
                # Whole lines only: an answer can span several USB packets.
                complete = got[: got.rfind(b"\n") + 1].decode("ascii", "replace")
                for text in complete.splitlines():
                    if text.startswith("FERRO-ID "):
                        return text.strip()
        return None
    finally:
        os.close(fd)


def main():
    found = probes()
    print(f"{len(found)} debug probe(s)")
    for name, probe in found:
        print("  " + probe_report(name, probe))
    ports = list(serial_ports())
    print(f"{len(ports)} USB serial port(s) other than the ST-LINKs'")
    for path in ports:
        answer = ask(path)
        print(f"  {os.path.basename(path)}  {answer or 'nothing answers FERRO-ID?'}")


if __name__ == "__main__":
    main()
