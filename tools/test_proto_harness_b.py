"""Tests for tools/proto_harness_b.py (PROTOTYPE): python3 -I tools/test_proto_harness_b.py

Candidate B's host must name the stage a hang is in. Lines an image prints in
one burst arrive in one read; a buffered line reader then holds the last of
them where select() cannot see it, and the hang was blamed on an earlier
stage (measured on the Discovery, 2026-11-24). A fake probe-rs prints such a
burst and stops.
"""

import contextlib
import io
import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import proto_harness_b as hb  # noqa: E402
import rig_check  # noqa: E402

BOARD = HERE.parent / "hil" / "boards" / "stm32f4-discovery.toml"

BURST = r"""#!/usr/bin/env bash
printf '     Finished in 0.10s\n'
printf '[INFO ] stage clocks start 2\n[INFO ] stage clocks done\n[INFO ] stage GLUE-001.a start 1\n[INFO ] stage GLUE-001.a done\n[INFO ] GLUE-001 pass (GLUE-001.a)\n[INFO ] stage RCC-003.b start 1\n'
exec sleep 30
"""

PASS = r"""#!/usr/bin/env bash
printf '     Finished in 0.10s\n[INFO ] stage GLUE-001.a start 1\n[INFO ] stage GLUE-001.a done\n[INFO ] GLUE-001 pass (GLUE-001.a)\n'
"""

CRASH = r"""#!/usr/bin/env bash
printf '     Finished in 0.10s\n[INFO ] stage S17.fault start 1\nFirmware exited unexpectedly: Multiple\n'
exit 1
"""


class Host(unittest.TestCase):
    def run_fake(self, script):
        d = tempfile.mkdtemp()
        fake = Path(d) / "probe-rs"
        fake.write_text(script)
        fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
        path = os.environ["PATH"]
        os.environ["PATH"] = f"{d}:{path}"
        out = io.StringIO()
        try:
            with contextlib.redirect_stdout(out):
                code = hb.run(rig_check.load_board(BOARD), "fake.elf", "", "fake")
        finally:
            os.environ["PATH"] = path
        return code, out.getvalue()

    def test_hang_after_a_burst_names_the_last_stage(self):
        code, text = self.run_fake(BURST)
        self.assertEqual(code, 124)
        self.assertIn("HANG in stage RCC-003.b (limit 1 s)", text)
        self.assertIn("run fake: exit 124, 1 passed, 1 failed", text)

    def test_pass(self):
        code, text = self.run_fake(PASS)
        self.assertEqual(code, 0, text)
        self.assertIn("run fake: exit 0, 1 passed, 0 failed", text)

    def test_crash_names_the_open_stage(self):
        code, text = self.run_fake(CRASH)
        self.assertEqual(code, 1)
        self.assertIn("ENDED in stage S17.fault", text)


if __name__ == "__main__":
    unittest.main(verbosity=1)
