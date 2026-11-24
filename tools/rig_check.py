#!/usr/bin/env -S python3 -I
"""rig_check: a board's identity check and wiring check, from its board file.

PROTOTYPE, built for the audit's step 6 on 2026-11-24; not a rule and not yet
the owner of any fact. hil/WIRING.md, each bench's board.rs and
hil/rig.env still are. Nothing here is requirement evidence: a check's
result is rig state.

One TOML file per board on the rig, holding what a bench may rely on and a
check can test: the board's identity (die UID, DEV_ID, probe serial), the
pins held at a level during a check, each link (both pins, header positions,
series resistor, who uses it), each slave (bus, pins, address, the register
that proves it answers) and each USB cable to the host. Only
stm32f4-discovery.toml exists.

## Why TOML

- Python reads it with the standard library (tomllib), as tools/ already
  does for Cargo manifests; YAML needs PyYAML, a package this rig has but CI's
  Python tools do not otherwise need.
- It carries comments, which say where each fact was read or measured; JSON
  cannot.
- Rust reads it with the toml crate, if a bench's pin table is generated
  from it later.

## The checks

tools/rig_check.py, through hil/proto-discovery-rig.sh under rig:

- **identity**: the probe is attached and the die behind it has the file's
  UID and DEV_ID; else it refuses and names both, the found die by the board
  file that holds its UID where one does. Read without halting the core.
- **wiring**: no firmware and no flash. A probe-rs gdb session resets and
  halts the core, and the check drives the pins through the debug port:
  - every listed pin read under its pull-down and its pull-up; a pin that reads
    the same under both is held by something off the list, reported, and never
    driven;
  - per link, its from pin driven high (all others pulled down), then low (all
    others pulled up): the to pin must follow, no other listed pin may;
  - per slave, a bit-banged I2C register read; per USB cable, the host sees the
    probe, or the ROM's DFU device with the UID-derived serial after the check
    jumps the core to the ROM;
  - then the chip is reset and runs what is in flash.
- **guard**: identity, then the bench command with PROBE_RS_PROBE set to the
  file's probe, then the wiring check only if the bench failed. Exit 0 pass,
  1 bench failed with the wiring good, 3 identity refused (nothing ran), 4 rig
  fault: the bench's failures are not requirement failures and nothing may be
  recorded.

## Limits

- A GPIO check proves continuity and shorts between listed pins, not a
  resistor's value, and not a short to a pin the file does not list.
- Each register access is one round trip through the probe (about 50 ms on
  the Discovery's USB/IP probe, measured); the check batches them.
- Slave checks know I2C only; USB checks know a probe and an F4 ROM's DFU.
- Only DEV_IDs 0x413 and 0x433 have a GPIO layout.

    rig_check.py identity <board.toml>          the die behind the probe is the board's
    rig_check.py wiring <board.toml> [--only K]  links, slaves and USB cables, over the probe
    rig_check.py guard <board.toml> -- CMD...    identity, CMD, and after a fail the wiring
    rig_check.py pins <board.toml> PIN...        read-only: each pin under no pull, down, up
    rig_check.py rom-diag <board.toml>           enter the ROM over the probe; what it did

Run them on the rig through `rig` (hil/proto-discovery-rig.sh), which holds the
board's lock.

Exit status: 0 pass; 1 the bench failed with the rig's wiring good; 2 usage or
tool error; 3 identity refused (nothing ran); 4 rig fault (the wiring check
failed, so the bench's failures are not requirement failures).

Every result here is rig state, never requirement evidence.
"""

from __future__ import annotations

import os
import re
import socket
import subprocess
import sys
import time
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

BOARDS_DIR = Path(__file__).resolve().parent.parent / "hil" / "boards"
OK, BENCH_FAIL, USAGE, IDENTITY_REFUSED, RIG_FAULT = 0, 1, 2, 3, 4

# ---------------------------------------------------------------- board file

PIN = re.compile(r"^P([A-K])(\d{1,2})$")


def parse_pin(name: str) -> tuple[int, int]:
    m = PIN.match(name)
    if not m or int(m.group(2)) > 15:
        raise ValueError(f"not a pin name: {name!r} (want P<port><0..15>, as PC10)")
    return ord(m.group(1)) - ord("A"), int(m.group(2))


@dataclass
class Link:
    name: str
    drive: str
    sense: str
    drive_header: str = ""
    sense_header: str = ""
    resistor_ohm: int = 0


@dataclass
class Board:
    path: Path
    board: str
    chip: str
    uid: str
    dev_id: int
    uid_address: int
    probe: str
    holds: list[tuple[str, str]] = field(default_factory=list)
    links: list[Link] = field(default_factory=list)
    slaves: list[dict] = field(default_factory=list)
    usb: list[dict] = field(default_factory=list)


def load_board(path: Path) -> Board:
    data = tomllib.loads(Path(path).read_text())
    if data.get("schema") != 1:
        raise ValueError(f"{path}: schema must be 1")
    ident = data["identity"]
    uid = ident["uid"].lower()
    if not re.fullmatch(r"[0-9a-f]{24}", uid):
        raise ValueError(f"{path}: identity.uid must be 24 hex digits")
    board = Board(
        path=Path(path), board=data["board"], chip=data["chip"], uid=uid,
        dev_id=int(ident["dev_id"]), uid_address=int(ident["uid_address"]),
        probe=ident["probe"],
    )
    for h in data.get("hold", []):
        parse_pin(h["pin"])
        board.holds.append((h["pin"], h["level"]))
    names, pins = set(), {}
    for item in data.get("link", []):
        link = Link(item["name"], item["from"]["pin"], item["to"]["pin"],
                    item["from"].get("header", ""), item["to"].get("header", ""),
                    int(item.get("resistor_ohm", 0)))
        if link.name in names:
            raise ValueError(f"{path}: link {link.name} named twice")
        names.add(link.name)
        for pin in (link.drive, link.sense):
            parse_pin(pin)
            if pin in pins:
                raise ValueError(f"{path}: {pin} is in links {pins[pin]} and {link.name}")
            pins[pin] = link.name
        board.links.append(link)
    board.slaves = list(data.get("slave", []))
    board.usb = list(data.get("usb", []))
    return board


def find_board_by_uid(uid: str, directory: Path) -> str | None:
    """The board file in `directory` whose identity.uid is `uid`, by board name."""
    for other in sorted(directory.glob("*.toml")):
        try:
            data = tomllib.loads(other.read_text())
            if data["identity"]["uid"].lower() == uid:
                return f"{data['board']} ({other.name})"
        except (OSError, KeyError, ValueError):
            continue
    return None

# ---------------------------------------------------------------- probe-rs CLI


def run(cmd, timeout):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def attached_probes(timeout=15) -> list[str]:
    out = run(["probe-rs", "list"], timeout).stdout
    return re.findall(r" -- (\S+) \(", out)


def read_words(board: Board, address: int, count: int, timeout=20) -> list[int] | None:
    out = run(["probe-rs", "read", "--chip", board.chip, "--probe", board.probe,
               "b32", hex(address), str(count)], timeout)
    if out.returncode != 0:
        return None
    words = []
    for line in out.stdout.splitlines():
        line = line.split(":", 1)[-1]
        words += [int(v, 16) for v in line.split()]
    return words[:count] or None


def uid_text(words: list[int]) -> str:
    """rig_status.py's form: word 2, word 1, word 0."""
    return "".join(f"{w:08x}" for w in reversed(words))


def rom_dfu_serial(uid: str) -> str:
    """The serial the F4 ROM's DFU device reports (hil/run-discovery.sh)."""
    w2, w1, w0 = (int(uid[i:i + 8], 16) for i in (0, 8, 16))
    return f"{(w0 + w2) & 0xFFFFFFFF:08X}{w1 >> 16:04X}"

# ---------------------------------------------------------------- identity


def identity(board: Board, out=print) -> bool:
    """True when the probe is attached and the die behind it is the board's."""
    t0 = time.monotonic()
    probes = attached_probes()
    if board.probe not in probes:
        out(f"identity: REFUSED: probe {board.probe} ({board.board}'s, {board.path.name}) "
            f"is not attached; attached: {', '.join(probes) or 'none'}")
        return False
    words = read_words(board, board.uid_address, 3)
    idcode = read_words(board, 0xE0042000, 1)
    if not words:
        out(f"identity: REFUSED: no die answers behind probe {board.probe} "
            f"(unpowered, held in reset, or not this chip)")
        return False
    found = uid_text(words)
    dev = idcode[0] & 0xFFF if idcode else None
    if found != board.uid or (dev is not None and dev != board.dev_id):
        known = find_board_by_uid(found, BOARDS_DIR) or "a board no file in hil/boards/ names"
        out(f"identity: REFUSED: behind probe {board.probe} is die uid={found} "
            f"dev=0x{dev or 0:03x}, which is {known}; {board.path.name} expects "
            f"{board.board}, uid={board.uid} dev=0x{board.dev_id:03x}. Nothing was flashed.")
        return False
    out(f"identity: ok {board.board} uid={found} dev=0x{dev:03x} probe={board.probe} "
        f"({time.monotonic() - t0:.2f} s)")
    return True

# ---------------------------------------------------------------- GDB remote


class Gdb:
    """A `probe-rs gdb` session spoken to directly: memory reads and writes.

    One attach for the whole check; every pin operation is then one round trip.
    """

    def __init__(self, board: Board, reset_halt=True, timeout=20.0):
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        cmd = ["probe-rs", "gdb", "--chip", board.chip, "--probe", board.probe,
               "--gdb-connection-string", f"127.0.0.1:{port}"]
        if reset_halt:
            cmd.append("--reset-halt")
        self.proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + timeout
        while True:
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
                break
            except OSError:
                if self.proc.poll() is not None or time.monotonic() > deadline:
                    self.close()
                    out = self.proc.stdout.read().decode(errors="replace") if self.proc.stdout else ""
                    raise RuntimeError(f"probe-rs gdb did not start: {out.strip()[-400:]}")
                time.sleep(0.05)
        self.buf = b""
        self.ops = 0
        self.sock.settimeout(5)
        try:
            self.cmd("?")
        except Exception:
            self.close()
            raise

    def _read(self) -> bytes:
        while b"#" not in self.buf or len(self.buf) < self.buf.index(b"#") + 3:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise RuntimeError("gdb connection closed")
            self.buf += chunk
        return self.buf

    def cmd(self, payload: str) -> str:
        data = payload.encode()
        packet = b"$" + data + b"#" + f"{sum(data) & 0xFF:02x}".encode()
        self.sock.sendall(packet)
        while True:
            buf = self._read()
            start = buf.find(b"$")
            if start < 0:
                self.buf = b""  # only acks
                continue
            end = buf.index(b"#", start)
            if len(buf) < end + 3:
                continue
            body = buf[start + 1:end]
            self.buf = buf[end + 3:]
            self.sock.sendall(b"+")
            if body.startswith(b"O") and body != b"OK":
                continue  # console output
            self.ops += 1
            return self._unrle(body).decode()

    @staticmethod
    def _unrle(body: bytes) -> bytes:
        out, i = bytearray(), 0
        while i < len(body):
            if body[i:i + 1] == b"*" and out:
                out += out[-1:] * (body[i + 1] - 29)
                i += 2
            else:
                out.append(body[i])
                i += 1
        return bytes(out)

    def registers(self) -> dict[str, tuple[int, int]]:
        """Register name -> (GDB number, bits), from the server's target description."""
        if getattr(self, "_regs", None) is None:
            xml, annex, offset = "", "target.xml", 0
            while True:
                reply = self.cmd(f"qXfer:features:read:{annex}:{offset:x},800")
                xml += reply[1:]
                offset += len(reply) - 1
                if not reply or reply[0] != "m":
                    break
            if os.environ.get("RIG_CHECK_DUMP"):
                Path(os.environ["RIG_CHECK_DUMP"]).write_text(xml)
            regs, number = {}, 0
            for m in re.finditer(r"<reg\s([^>]*)>", xml):
                attrs = {k: a or b for k, a, b in re.findall(r"""(\w+)\s*=\s*(?:"([^"]*)"|'([^']*)')""", m.group(1))}
                if "name" not in attrs:
                    continue
                number = int(attrs.get("regnum", number))
                regs[attrs["name"].lower()] = (number, int(attrs.get("bitsize", 32)))
                number += 1
            self._regs = regs
        return self._regs

    def set_register(self, name: str, value: int):
        number, bits = self.registers()[name]
        reply = self.cmd(f"P{number:x}={value.to_bytes(bits // 8, 'little').hex()}")
        if reply != "OK":
            raise RuntimeError(f"register {name}: {reply!r}")

    def resume(self):
        """Continue; the stop reply is read by interrupt()."""
        data = b"c"
        self.sock.sendall(b"$" + data + b"#" + f"{sum(data) & 0xFF:02x}".encode())

    def interrupt(self):
        self.sock.sendall(b"\x03")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            buf = self._read()
            start = buf.find(b"$")
            if start >= 0 and b"#" in buf[start:]:
                end = buf.index(b"#", start)
                if len(buf) >= end + 3:
                    self.buf = buf[end + 3:]
                    self.sock.sendall(b"+")
                    return
            else:
                self.buf = b""
        raise RuntimeError("no stop reply after the interrupt")

    def read32(self, address: int) -> int:
        reply = self.cmd(f"m{address:x},4")
        if len(reply) != 8:
            raise RuntimeError(f"read 0x{address:08x}: {reply!r}")
        return int.from_bytes(bytes.fromhex(reply), "little")

    def write32(self, address: int, value: int):
        reply = self.cmd(f"M{address:x},4:{(value & 0xFFFFFFFF).to_bytes(4, 'little').hex()}")
        if reply != "OK":
            raise RuntimeError(f"write 0x{address:08x}: {reply!r}")

    def modify32(self, address: int, clear: int, setbits: int):
        self.write32(address, (self.read32(address) & ~clear) | setbits)

    def close(self):
        try:
            self.sock.close()
        except Exception:
            pass
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()

# ---------------------------------------------------------------- GPIO over the probe

# Per DEV_ID: GPIO base and stride, and the clock-enable register for the ports.
GPIO_LAYOUT = {
    0x413: (0x4002_0000, 0x400, 0x4002_3830),  # F405/F407: AHB1ENR, bit = port
    0x433: (0x4002_0000, 0x400, 0x4002_3830),  # F401
}
MODER, OTYPER, PUPDR, IDR, BSRR = 0x00, 0x04, 0x0C, 0x10, 0x18
INPUT, OUTPUT = 0b00, 0b01
NOPULL, PULLUP, PULLDOWN = 0b00, 0b01, 0b10
# Cortex-M debug and system registers.
DHCSR, DCRSR, DCRDR, DEMCR, AIRCR, VTOR = (0xE000EDF0, 0xE000EDF4, 0xE000EDF8, 0xE000EDFC,
                                           0xE000ED0C, 0xE000ED08)


class Gpio:
    """The ports through the probe, with each port's MODER, PUPDR and OTYPER
    shadowed here: a change is one whole-register write, never a
    read-modify-write, and every input is read in one round trip where the
    probe allows. A round trip is the cost (about 50 ms on a USB/IP probe)."""

    def __init__(self, gdb: Gdb, dev_id: int):
        if dev_id not in GPIO_LAYOUT:
            raise RuntimeError(f"no GPIO layout for DEV_ID 0x{dev_id:03x} in this prototype")
        self.g = gdb
        self.base, self.stride, self.enr = GPIO_LAYOUT[dev_id]
        self.shadow: dict[tuple[int, int], int] = {}
        self.one_read = True

    def port_reg(self, port: int, offset: int) -> int:
        return self.base + port * self.stride + offset

    def enable(self, pins):
        ports = {parse_pin(p)[0] for p in pins}
        bits = sum(1 << port for port in ports)
        self.g.write32(self.enr, self.g.read32(self.enr) | bits)
        for port in sorted(ports):  # also proves the clock is on
            for offset in (MODER, OTYPER, PUPDR):
                self.shadow[(port, offset)] = self.g.read32(self.port_reg(port, offset))

    def _field(self, changes: dict[str, int], offset: int, width: int):
        per_port: dict[int, int] = {}
        for pin, value in changes.items():
            port, n = parse_pin(pin)
            mask = (1 << width) - 1
            word = per_port.get(port, self.shadow[(port, offset)])
            per_port[port] = (word & ~(mask << width * n)) | (value << width * n)
        for port, word in per_port.items():
            if word != self.shadow[(port, offset)]:
                self.g.write32(self.port_reg(port, offset), word)
                self.shadow[(port, offset)] = word

    def modes(self, changes: dict[str, int]):
        self._field(changes, MODER, 2)

    def pulls(self, changes: dict[str, int]):
        self._field(changes, PUPDR, 2)

    def open_drain(self, pins, on=True):
        self._field({p: int(on) for p in pins}, OTYPER, 1)

    def set(self, pin, high: bool):
        port, n = parse_pin(pin)
        self.g.write32(self.port_reg(port, BSRR), (1 << n) if high else (1 << (n + 16)))

    def levels(self, pins) -> dict[str, int]:
        ports = sorted({parse_pin(p)[0] for p in pins})
        idr: dict[int, int] = {}
        if self.one_read and len(ports) > 1:
            first, last = self.port_reg(ports[0], IDR), self.port_reg(ports[-1], IDR)
            try:
                reply = self.g.cmd(f"m{first:x},{last - first + 4:x}")
                raw = bytes.fromhex(reply)
                if len(raw) != last - first + 4:
                    raise ValueError(reply[:20])
                for port in ports:
                    at = self.port_reg(port, IDR) - first
                    idr[port] = int.from_bytes(raw[at:at + 4], "little")
            except ValueError:
                self.one_read = False
        for port in ports:
            if port not in idr:
                idr[port] = self.g.read32(self.port_reg(port, IDR))
        return {p: idr[parse_pin(p)[0]] >> parse_pin(p)[1] & 1 for p in pins}

    def get(self, pin) -> int:
        return self.levels([pin])[pin]

    def drive(self, pin, high: bool):
        self.set(pin, high)
        self.modes({pin: OUTPUT})

    def release(self, pin, pull):
        self.modes({pin: INPUT})
        self.pulls({pin: pull})

# ---------------------------------------------------------------- wiring check


def held_pins(gpio: Gpio, pins: list[str]) -> dict[str, str]:
    """Pins something off the list holds: high under the pull-down, or low under
    the pull-up, with nothing driven. Such a pin is never driven by the check."""
    gpio.modes({p: INPUT for p in pins})
    gpio.pulls({p: PULLDOWN for p in pins})
    down = gpio.levels(pins)
    gpio.pulls({p: PULLUP for p in pins})
    up = gpio.levels(pins)
    held = {}
    for p in pins:
        if down[p] and up[p]:
            held[p] = "high"
        elif not down[p] and not up[p]:
            held[p] = "low"
    return held


def check_links(gpio: Gpio, board: Board, out) -> list[str]:
    """Each link: its drive end high then low; its sense end follows, no other listed pin does."""
    faults: dict[str, list[str]] = {k.name: [] for k in board.links}
    listed = [p for link in board.links for p in (link.drive, link.sense)]
    owner = {p: k.name for k in board.links for p in (k.drive, k.sense)}
    held = held_pins(gpio, listed)
    for pin, level in held.items():
        faults[owner[pin]].append(
            f"link {owner[pin]}: {pin} is held {level} by something off the list (it reads "
            f"{level} under both its pulls, nothing driven); never driven by this check")
    for high, pull in ((True, PULLDOWN), (False, PULLUP)):
        gpio.modes({p: INPUT for p in listed})
        gpio.pulls({p: pull for p in listed})
        want, level = (1, "high") if high else (0, "low")
        for link in board.links:
            if link.drive in held:
                continue
            where = f"{link.drive} ({link.drive_header}) -> {link.sense} ({link.sense_header})"
            gpio.drive(link.drive, high)
            got = gpio.levels(listed)
            gpio.modes({link.drive: INPUT})
            if link.sense not in held and got[link.sense] != want:
                faults[link.name].append(
                    f"link {link.name} {where}: {link.sense} read {'high' if got[link.sense] else 'low'} "
                    f"while {link.drive} drove {level} (open: not fitted, or on another pin)")
            for pin in listed:
                if pin in (link.drive, link.sense) or pin in held:
                    continue
                if got[pin] == want:
                    faults[link.name].append(
                        f"link {link.name} {where}: {pin} (link {owner[pin]}) followed {link.drive} "
                        f"{level} (a short, or a jumper on the wrong pin)")
    gpio.pulls({p: NOPULL for p in listed})
    for link in board.links:
        out(f"  link {link.name} {link.drive} ({link.drive_header}) -> {link.sense} "
            f"({link.sense_header})" + (f" {link.resistor_ohm} ohm" if link.resistor_ohm else "")
            + (": ok" if not faults[link.name] else ": FAULT"))
    return [f for k in board.links for f in faults[k.name]]


class I2c:
    """A bit-banged I2C master over the probe: slow (one round trip per edge), but
    the protocol is the Pico simulator's, so the same check answers for it."""

    def __init__(self, gpio: Gpio, scl: str, sda: str):
        self.p, self.scl, self.sda = gpio, scl, sda
        for pin in (scl, sda):
            gpio.set(pin, True)
        gpio.open_drain([scl, sda])
        gpio.pulls({scl: PULLUP, sda: PULLUP})
        gpio.modes({scl: OUTPUT, sda: OUTPUT})

    def line(self, pin, high):
        self.p.set(pin, high)
        if high and pin == self.scl:  # clock stretching: wait for the slave to let go
            for _ in range(50):
                if self.p.get(pin):
                    return
            raise RuntimeError(f"I2C: {pin} held low (SCL stuck)")

    def start(self):
        self.line(self.sda, True)
        self.line(self.scl, True)
        self.line(self.sda, False)
        self.line(self.scl, False)

    def stop(self):
        self.line(self.sda, False)
        self.line(self.scl, True)
        self.line(self.sda, True)

    def write(self, byte) -> bool:
        for i in range(7, -1, -1):
            self.line(self.sda, bool(byte >> i & 1))
            self.line(self.scl, True)
            self.line(self.scl, False)
        self.line(self.sda, True)
        self.line(self.scl, True)
        ack = self.p.get(self.sda) == 0
        self.line(self.scl, False)
        return ack

    def read(self, ack: bool) -> int:
        value = 0
        self.line(self.sda, True)
        for _ in range(8):
            self.line(self.scl, True)
            value = value << 1 | self.p.get(self.sda)
            self.line(self.scl, False)
        self.line(self.sda, not ack)
        self.line(self.scl, True)
        self.line(self.scl, False)
        self.line(self.sda, True)
        return value

    def read_register(self, address, register) -> int | None:
        self.start()
        if not self.write(address << 1):
            self.stop()
            return None
        self.write(register)
        self.start()
        self.write(address << 1 | 1)
        value = self.read(ack=False)
        self.stop()
        return value


def check_slaves(gpio: Gpio, board: Board, out) -> list[str]:
    faults = []
    for s in board.slaves:
        if s.get("bus") != "i2c":
            faults.append(f"slave {s['name']}: bus {s.get('bus')!r} has no check in this prototype")
            continue
        release = s.get("release")
        if release:
            gpio.drive(release["pin"], release["level"] == "high")
        try:
            bus = I2c(gpio, s["scl"], s["sda"])
            value = bus.read_register(int(s["address"]), int(s["register"]))
        except RuntimeError as error:
            value, why = None, str(error)
        else:
            why = f"no ACK at address 0x{int(s['address']):02x}"
        mask = int(s.get("mask", 0xFF))
        where = f"{s['bus']} SCL {s['scl']} SDA {s['sda']} address 0x{int(s['address']):02x}"
        if value is None:
            faults.append(f"slave {s['name']} on {where}: does not answer ({why})")
            out(f"  slave {s['name']} {where}: FAULT")
        elif value & mask != int(s["expect"]) & mask:
            faults.append(f"slave {s['name']} on {where}: register 0x{int(s['register']):02x} "
                          f"read 0x{value:02x}, expected 0x{int(s['expect']):02x} under mask "
                          f"0x{mask:02x} (another device, or the wrong address)")
            out(f"  slave {s['name']} {where}: FAULT")
        else:
            out(f"  slave {s['name']} {where}: ok, register 0x{int(s['register']):02x} = 0x{value:02x}")
        gpio.modes({s["scl"]: INPUT, s["sda"]: INPUT})
        gpio.pulls({s["scl"]: NOPULL, s["sda"]: NOPULL})
        gpio.open_drain([s["scl"], s["sda"]], False)
        if release:
            # Back to the board file's hold for that pin, if it has one.
            for pin, level in board.holds:
                if pin == release["pin"]:
                    gpio.drive(pin, level == "high")
    return faults


def write_core_register(g: Gdb, regsel: int, value: int):
    g.write32(DCRDR, value)
    g.write32(DCRSR, (1 << 16) | regsel)
    for _ in range(20):
        if g.read32(DHCSR) & (1 << 16):  # S_REGRDY
            return
    raise RuntimeError(f"core register {regsel} write did not complete")


def reset_and_halt(g: Gdb):
    g.modify32(DEMCR, 0, 1)                 # VC_CORERESET: halt at the reset vector
    g.write32(AIRCR, 0x05FA_0004)           # SYSRESETREQ
    for _ in range(100):
        try:
            if g.read32(DHCSR) & (1 << 17):  # S_HALT
                break
        except RuntimeError:
            time.sleep(0.01)
    else:
        raise RuntimeError("the core did not halt after the reset")
    g.modify32(DEMCR, 1, 0)


def enter_rom(g: Gdb):
    """From a reset core, as FerroHAL's reset::enter_bootloader does in firmware."""
    reset_and_halt(g)
    g.modify32(0x4002_3844, 0, 1 << 14)     # RCC_APB2ENR.SYSCFGEN
    g.write32(0x4001_3800, 0b01)            # SYSCFG_MEMRMP: system memory at 0
    g.write32(VTOR, 0x1FFF_0000)
    sp, pc = g.read32(0x1FFF_0000), g.read32(0x1FFF_0004)
    regs = g.registers()
    for name in ("msp", "sp"):
        if name in regs:
            g.set_register(name, sp)
    g.set_register("pc", pc & ~1)
    g.set_register("xpsr", 1 << 24)         # Thumb
    g.resume()


def dfu_serials(timeout=10) -> list[str]:
    try:
        out = run(["dfu-util", "-l", "-d", "0483:df11"], timeout).stdout
    except subprocess.TimeoutExpired:
        return []
    return sorted(set(re.findall(r'serial="([^"]+)"', out)))


def check_usb(g: Gdb, gpio: Gpio, board: Board, out, rom_wait=10.0) -> list[str]:
    faults = []
    for cable in board.usb:
        name, expect = cable["name"], cable["expect"]
        if expect == "probe":
            if board.probe in attached_probes():
                out(f"  usb {name}: ok, the host lists probe {board.probe}")
            else:
                faults.append(f"usb {name}: the host does not list probe {board.probe}")
                out(f"  usb {name}: FAULT")
        elif expect == "rom-dfu":
            vbus = cable.get("vbus")
            if vbus:
                gpio.release(vbus, NOPULL)
                if not gpio.get(vbus):
                    faults.append(f"usb {name}: VBUS ({vbus}) reads low: no cable, or an unpowered port")
                    out(f"  usb {name}: FAULT")
                    continue
            serial = rom_dfu_serial(board.uid)
            if serial in dfu_serials():
                faults.append(f"usb {name}: a DFU device {serial} is listed before the check "
                              f"started it; cannot tell this cable")
                continue
            enter_rom(g)
            t0, seen = time.monotonic(), []
            while time.monotonic() - t0 < rom_wait:
                seen = dfu_serials()
                if serial in seen:
                    break
                time.sleep(0.25)
            if serial in seen:
                out(f"  usb {name}: ok, VBUS high and the ROM's DFU device {serial} enumerated "
                    f"in {time.monotonic() - t0:.1f} s")
            else:
                faults.append(f"usb {name}: the ROM's DFU device {serial} did not enumerate within "
                              f"{rom_wait:.0f} s (VBUS was high); listed: {', '.join(seen) or 'none'}")
                out(f"  usb {name}: FAULT")
            g.interrupt()
            reset_and_halt(g)  # out of the ROM
        else:
            faults.append(f"usb {name}: expect {expect!r} has no check in this prototype")
    return faults


def wiring(board: Board, out=print, only=None) -> list[str] | None:
    """The faults found (empty: the wiring is as the file says), or None when the
    check itself could not run."""
    t0 = time.monotonic()
    try:
        g = Gdb(board, reset_halt=True)
    except (RuntimeError, OSError) as error:
        run(["probe-rs", "reset", "--chip", board.chip, "--probe", board.probe], 20)
        out(f"wiring: could not attach: {error}")
        return None
    attach = time.monotonic() - t0
    faults: list[str] = []
    try:
        gpio = Gpio(g, board.dev_id)
        pins = [p for p, _ in board.holds] + [p for k in board.links for p in (k.drive, k.sense)]
        for s in board.slaves:
            pins += [s.get("scl", "PA0"), s.get("sda", "PA0")] + ([s["release"]["pin"]] if s.get("release") else [])
        pins += [c["vbus"] for c in board.usb if c.get("vbus")]
        gpio.enable(pins)
        for pin, level in board.holds:
            gpio.drive(pin, level == "high")
        parts = {"links": check_links, "slaves": check_slaves}
        for key, fn in parts.items():
            if only in (None, key):
                t = time.monotonic()
                faults += fn(gpio, board, out)
                out(f"  ({key}: {time.monotonic() - t:.2f} s)")
        if only in (None, "usb"):
            t = time.monotonic()
            faults += check_usb(g, gpio, board, out)
            out(f"  (usb: {time.monotonic() - t:.2f} s)")
    except (RuntimeError, OSError) as error:
        out(f"wiring: the check stopped: {error!r}")
        return None
    finally:
        ops = g.ops
        g.close()
        # Out of the check's pin states: reset and run whatever is in flash.
        run(["probe-rs", "reset", "--chip", board.chip, "--probe", board.probe], 20)
    out(f"wiring: {'ok' if not faults else 'FAULT'} {board.board}: {len(board.links)} links, "
        f"{len(board.slaves)} slaves, {len(board.usb)} cables; {ops} probe round trips; "
        f"attach {attach:.2f} s, total {time.monotonic() - t0:.2f} s; nothing flashed")
    for fault in faults:
        out(f"RIG FAULT {fault}")
    return faults

# ---------------------------------------------------------------- the hook


def guard(board: Board, cmd: list[str], out=print) -> int:
    """identity -> CMD -> (on a fail) wiring -> verdict."""
    if not identity(board, out):
        out("guard: REFUSED before flashing: the board behind the probe is not the file's")
        return IDENTITY_REFUSED
    env = dict(os.environ, PROBE_RS_PROBE=board.probe)
    t0 = time.monotonic()
    code = subprocess.run(cmd, env=env).returncode
    out(f"guard: bench exited {code} after {time.monotonic() - t0:.1f} s")
    if code == 0:
        out("guard: bench passed; the wiring check runs after a fail only")
        return OK
    faults = wiring(board, out)
    if faults is None:
        out("guard: RIG FAULT: the wiring check could not run; the bench's failures are "
            "not requirement failures and nothing may be recorded")
        return RIG_FAULT
    if faults:
        out(f"guard: RIG FAULT: {len(faults)} wiring fault(s) above; the bench's failures are "
            f"not requirement failures and nothing may be recorded")
        return RIG_FAULT
    out("guard: the wiring is as the board file says, so the bench's failures stand as its own")
    return BENCH_FAIL


def read_pins(board: Board, pins: list[str], out=print) -> bool:
    """Read-only: each pin as an input with no pull, its pull-down and its pull-up.
    Nothing is driven. For a pin something off the list holds."""
    g = Gdb(board, reset_halt=True)
    try:
        gpio = Gpio(g, board.dev_id)
        gpio.enable(pins + [p for p, _ in board.holds])
        for pin, level in board.holds:
            gpio.drive(pin, level == "high")
        for pin in pins:
            got = {}
            for name, pull in (("none", NOPULL), ("down", PULLDOWN), ("up", PULLUP)):
                gpio.release(pin, pull)
                got[name] = gpio.get(pin)
            gpio.release(pin, NOPULL)
            out(f"  {pin}: no pull {got['none']}, pull-down {got['down']}, pull-up {got['up']}")
    finally:
        g.close()
        run(["probe-rs", "reset", "--chip", board.chip, "--probe", board.probe], 20)
    return True


def read_core_register(g: Gdb, regsel: int) -> int:
    g.write32(DCRSR, regsel)
    for _ in range(20):
        if g.read32(DHCSR) & (1 << 16):
            return g.read32(DCRDR)
    raise RuntimeError(f"core register {regsel} read did not complete")


def rom_diag(board: Board, out=print) -> bool:
    """Enter the ROM over the probe and look at what it did, for a CN5 check
    that did not see the DFU device."""
    g = Gdb(board, reset_halt=True)
    try:
        out(f"  registers: {sorted(g.registers())}")
        enter_rom(g)
        for t in (0.2, 1.0, 3.0):
            time.sleep(t)
            if t == 3.0:
                g.interrupt()
            else:
                out(f"  after {t} s: dfu {', '.join(dfu_serials()) or 'none'}")
                continue
            dhcsr = g.read32(DHCSR)
            line = (f"  after {t} s: DHCSR=0x{dhcsr:08x} halted={dhcsr >> 17 & 1} "
                    f"lockup={dhcsr >> 19 & 1} RCC_CR=0x{g.read32(0x4002_3800):08x} "
                    f"RCC_AHB2ENR=0x{g.read32(0x4002_3834):08x} VTOR=0x{g.read32(VTOR):08x}")
            if dhcsr >> 17 & 1:
                line += f" PC=0x{read_core_register(g, 15):08x}"
            out(line)
        out(f"  dfu: {', '.join(dfu_serials()) or 'none'}")
        reset_and_halt(g)
    finally:
        g.close()
        run(["probe-rs", "reset", "--chip", board.chip, "--probe", board.probe], 20)
    return True


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(line_buffering=True)  # in order with a bench's own output
    if len(argv) >= 2 and argv[0] == "rom-diag":
        board = load_board(Path(argv[1]))
        return OK if identity(board) and rom_diag(board) else IDENTITY_REFUSED
    if len(argv) >= 3 and argv[0] == "pins":
        board = load_board(Path(argv[1]))
        return OK if identity(board) and read_pins(board, argv[2:]) else IDENTITY_REFUSED
    if len(argv) < 2 or argv[0] not in ("identity", "wiring", "guard"):
        print(__doc__.strip())
        return USAGE
    try:
        board = load_board(Path(argv[1]))
    except (OSError, KeyError, ValueError) as error:
        print(f"rig_check: board file: {error}")
        return USAGE
    if argv[0] == "identity":
        return OK if identity(board) else IDENTITY_REFUSED
    if argv[0] == "wiring":
        only = argv[3] if len(argv) > 3 and argv[2] == "--only" else None
        if not identity(board):
            return IDENTITY_REFUSED
        faults = wiring(board, only=only)
        return USAGE if faults is None else (RIG_FAULT if faults else OK)
    if "--" not in argv:
        print("rig_check: guard needs -- and the bench command")
        return USAGE
    return guard(board, argv[argv.index("--") + 1:])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
