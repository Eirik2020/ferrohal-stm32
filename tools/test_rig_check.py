"""Tests for tools/rig_check.py: python3 -I tools/test_rig_check.py

PROTOTYPE (audit step 6). The wiring check must name the link at fault: an
open link, two swapped jumpers, a pin something else holds (which it must
never drive), and a slave that does not answer. The identity check must refuse
a die that is not the board file's and name both. These run against a model
of the F4's GPIO ports wired as each test says; nothing here needs a board.
"""

import io
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import rig_check as rc  # noqa: E402

BOARD = HERE.parent / "hil" / "boards" / "stm32f4-discovery.toml"


class FakeBoard:
    """GPIO A..I of an F4 behind a fake GDB session. `nets` are groups of pins
    joined by a jumper; `held` pins are tied to a level off the chip."""

    def __init__(self, nets=(), held=None):
        self.mem = {}
        self.nets = [set(n) for n in nets]
        self.held = dict(held or {})
        self.ops = 0
        self.driven = []  # (pin, level) every time a pin became an output

    def _port(self, address):
        off = address - 0x4002_0000
        if 0 <= off < 9 * 0x400:
            return off // 0x400, off % 0x400
        return None, None

    def _out(self, port, n):
        moder = self.mem.get(0x4002_0000 + port * 0x400, 0)
        return (moder >> 2 * n & 3) == rc.OUTPUT

    def _level(self, pin):
        port, n = rc.parse_pin(pin)
        net = next((x for x in self.nets if pin in x), {pin})
        for p in net:
            pp, pn = rc.parse_pin(p)
            if self._out(pp, pn):
                return self.mem.get(0x4002_0000 + pp * 0x400 + 0x14, 0) >> pn & 1
        for p in net:
            if p in self.held:
                return self.held[p]
        pull = self.mem.get(0x4002_0000 + port * 0x400 + rc.PUPDR, 0) >> 2 * n & 3
        return 1 if pull == rc.PULLUP else 0

    def read32(self, address):
        self.ops += 1
        port, off = self._port(address)
        if port is not None and off == rc.IDR:
            letter = "ABCDEFGHI"[port]
            return sum(self._level(f"P{letter}{n}") << n for n in range(16))
        return self.mem.get(address, 0)

    def write32(self, address, value):
        self.ops += 1
        port, off = self._port(address)
        if port is not None and off == rc.BSRR:
            odr = 0x4002_0000 + port * 0x400 + 0x14
            v = self.mem.get(odr, 0)
            v = (v | (value & 0xFFFF)) & ~(value >> 16)
            self.mem[odr] = v
            return
        if port is not None and off == rc.MODER:
            old = self.mem.get(address, 0)
            letter = "ABCDEFGHI"[port]
            for n in range(16):
                if (value >> 2 * n & 3) == rc.OUTPUT and (old >> 2 * n & 3) != rc.OUTPUT:
                    pin = f"P{letter}{n}"
                    level = self.mem.get(0x4002_0000 + port * 0x400 + 0x14, 0) >> n & 1
                    self.driven.append((pin, level))
        self.mem[address] = value

    def cmd(self, payload):
        address, length = (int(x, 16) for x in payload[1:].split(","))
        self.ops -= length // 4 - 1  # one round trip, however long
        return b"".join(self.read32(address + i).to_bytes(4, "little")
                        for i in range(0, length, 4)).hex()


def board_with(links):
    text = BOARD.read_text().split("# Each jumper between two header pins")[0]
    for name, a, b in links:
        text += f'[[link]]\nname = "{name}"\nfrom = {{ pin = "{a}" }}\nto = {{ pin = "{b}" }}\n\n'
    d = tempfile.mkdtemp()
    path = Path(d) / "b.toml"
    path.write_text(text)
    return rc.load_board(path)


LINKS = [("D1", "PC10", "PC11"), ("D2", "PC12", "PD2"), ("D3", "PD8", "PD9")]


def run_links(board, fake):
    gpio = rc.Gpio(fake, 0x413)
    gpio.enable([p for k in board.links for p in (k.drive, k.sense)])
    lines = []
    return rc.check_links(gpio, board, lines.append), lines


class Links(unittest.TestCase):
    def test_all_fitted(self):
        b = board_with(LINKS)
        faults, lines = run_links(b, FakeBoard(nets=[("PC10", "PC11"), ("PC12", "PD2"), ("PD8", "PD9")]))
        self.assertEqual(faults, [])
        self.assertEqual(sum("ok" in x for x in lines), 3)

    def test_open_link_is_named(self):
        b = board_with(LINKS)
        faults, _ = run_links(b, FakeBoard(nets=[("PC10", "PC11"), ("PD8", "PD9")]))
        self.assertTrue(faults)
        self.assertTrue(all(f.startswith("link D2 ") for f in faults), faults)
        self.assertIn("PD2 read low while PC12 drove high", faults[0])

    def test_swapped_jumpers_are_named(self):
        b = board_with(LINKS)
        # PC10 reaches PD2 and PC12 reaches PC11: the two sense ends crossed.
        faults, _ = run_links(b, FakeBoard(nets=[("PC10", "PD2"), ("PC12", "PC11"), ("PD8", "PD9")]))
        text = "\n".join(faults)
        self.assertIn("link D1", text)
        self.assertIn("PD2 (link D2) followed PC10 high", text)
        self.assertIn("PC11 (link D1) followed PC12 high", text)
        self.assertNotIn("link D3", text)

    def test_held_pin_is_named_and_never_driven(self):
        b = board_with(LINKS + [("LL", "PC8", "PE11")])
        fake = FakeBoard(nets=[("PC10", "PC11"), ("PC12", "PD2"), ("PD8", "PD9")], held={"PC8": 1})
        faults, _ = run_links(b, fake)
        self.assertTrue(any("PC8 is held high" in f for f in faults), faults)
        self.assertNotIn("PC8", [pin for pin, _ in fake.driven])
        self.assertTrue(all(f.startswith("link LL") for f in faults), faults)

    def test_round_trips_are_few(self):
        b = board_with(LINKS)
        fake = FakeBoard(nets=[("PC10", "PC11"), ("PC12", "PD2"), ("PD8", "PD9")])
        run_links(b, fake)
        self.assertLess(fake.ops, 60)


class Slaves(unittest.TestCase):
    def test_absent_slave_is_named(self):
        b = rc.load_board(BOARD)
        fake = FakeBoard()  # nothing pulls SDA low: no ACK
        gpio = rc.Gpio(fake, 0x413)
        gpio.enable(["PB6", "PB9", "PD4"])
        faults = rc.check_slaves(gpio, b, lambda _: None)
        self.assertEqual(len(faults), 1)
        self.assertIn("slave cs43l22", faults[0])
        self.assertIn("no ACK at address 0x4a", faults[0])


class Identity(unittest.TestCase):
    def setUp(self):
        self.saved = rc.attached_probes, rc.read_words

    def tearDown(self):
        rc.attached_probes, rc.read_words = self.saved

    def fake(self, probes, uid_words, idcode=0x101F6413):
        rc.attached_probes = lambda timeout=15: probes
        rc.read_words = lambda board, address, count, timeout=20: (
            uid_words if address == board.uid_address else [idcode])

    def test_match(self):
        b = rc.load_board(BOARD)
        self.fake([b.probe], [0x00410039, 0x32355107, 0x36363436])
        lines = []
        self.assertTrue(rc.identity(b, lines.append))

    def test_other_die_is_refused_and_both_named(self):
        b = rc.load_board(BOARD)
        self.fake([b.probe], [1, 2, 3])
        lines = []
        self.assertFalse(rc.identity(b, lines.append))
        self.assertIn("uid=000000030000000200000001", lines[0])
        self.assertIn(f"expects stm32f4-discovery, uid={b.uid}", lines[0])

    def test_missing_probe_is_refused(self):
        b = rc.load_board(BOARD)
        self.fake(["0483:374b:OTHER"], None)
        lines = []
        self.assertFalse(rc.identity(b, lines.append))
        self.assertIn("is not attached", lines[0])


class BoardFile(unittest.TestCase):
    def test_pin_in_two_links_is_refused(self):
        with self.assertRaises(ValueError):
            board_with([("A", "PC10", "PC11"), ("B", "PC11", "PD2")])

    def test_rom_serial(self):
        # hil/run-discovery.sh's rom_serial, by hand: 0x00410039 + 0x36363436, 0x3235.
        self.assertEqual(rc.rom_dfu_serial("363634363235510700410039"), "3677346F3235")


class Guard(unittest.TestCase):
    def setUp(self):
        self.saved = rc.identity, rc.wiring

    def tearDown(self):
        rc.identity, rc.wiring = self.saved

    def verdict(self, ident, cmd, faults):
        rc.identity = lambda board, out=print: ident
        rc.wiring = lambda board, out=print, only=None: faults
        out = io.StringIO()
        code = rc.guard(rc.load_board(BOARD), cmd, lambda x: out.write(x + "\n"))
        return code, out.getvalue()

    def test_identity_refusal_runs_nothing(self):
        code, text = self.verdict(False, ["sh", "-c", "exit 9"], [])
        self.assertEqual(code, rc.IDENTITY_REFUSED)
        self.assertNotIn("bench exited", text)

    def test_pass_skips_wiring(self):
        code, text = self.verdict(True, ["true"], ["never consulted"])
        self.assertEqual(code, rc.OK)

    def test_fail_with_good_wiring_stands(self):
        code, _ = self.verdict(True, ["false"], [])
        self.assertEqual(code, rc.BENCH_FAIL)

    def test_fail_with_bad_wiring_is_a_rig_fault(self):
        code, text = self.verdict(True, ["false"], ["link D1 ... open"])
        self.assertEqual(code, rc.RIG_FAULT)
        self.assertIn("not requirement failures", text)


if __name__ == "__main__":
    unittest.main(verbosity=1)
