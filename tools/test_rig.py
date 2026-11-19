"""Tests for tools/rig.py: python3 -I tools/test_rig.py

`rig` is the one command agents are allowed to run benches through. It must find
the script in the repository it is started from, pin each board's probe from
hil/rig.env while that probe is attached, refuse a board another `rig` holds
(naming the holder), and pass the script's exit status through.
"""

import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import rig  # noqa: E402

RIG = HERE / "rig.py"

# probe-rs as `probe-rs list` prints with the F401 and H753 attached.
FAKE_PROBE_RS = """#!/usr/bin/env bash
[ "$1" = list ] || exit 1
echo "The following debug probes were found:"
echo "[0]: STLink V2-1 -- 0483:374b:F401SERIAL (ST-LINK)"
echo "[1]: STLink V3 -- 0483:374e:H753SERIAL (ST-LINK)"
"""

RIG_ENV = """# pins
F401_PROBE=0483:374b:F401SERIAL
H753_PROBE=0483:374e:H753SERIAL
DISCOVERY_PROBE=0483:374b:GONE
"""

# A bench script that reports the probe variables it was given, then exits
# with the status in $BENCH_EXIT, after waiting for $BENCH_WAIT to exist.
FAKE_BENCH = """#!/usr/bin/env bash
echo "F401_PROBE=${F401_PROBE:-} H753_PROBE=${H753_PROBE:-} DISCOVERY_PROBE=${DISCOVERY_PROBE:-}"
echo "args: $*"
if [ -n "${BENCH_WAIT:-}" ]; then
    touch "$BENCH_WAIT.started"
    while [ ! -e "$BENCH_WAIT" ]; do sleep 0.05; done
fi
exit "${BENCH_EXIT:-0}"
"""


def executable(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


class BoardsOf(unittest.TestCase):
    def test_board_from_the_script_name(self):
        self.assertEqual(rig.boards_of("run-h753.sh", []), ["h753"])
        self.assertEqual(rig.boards_of("run-f401-spi-sm.sh", []), ["f401"])
        self.assertEqual(rig.boards_of("run-discovery-lane-loop.sh", []), ["discovery"])
        self.assertEqual(rig.boards_of("run-foxeer.sh", ["a"]), ["foxeer"])
        self.assertEqual(rig.boards_of("run-lucid.sh", []), ["lucid"])

    def test_boards_from_the_argument(self):
        self.assertEqual(rig.boards_of("flash-dfu.sh", ["foxeer-f405-v2"]), ["foxeer"])
        self.assertEqual(rig.boards_of("flash-dfu.sh", ["lucid-h7"]), ["lucid"])
        self.assertEqual(rig.boards_of("run-event-adc.sh", ["h753"]), ["h753"])
        self.assertEqual(rig.boards_of("run-event-adc.sh", ["discovery"]), ["discovery"])

    def test_the_simulator_takes_both_benches(self):
        self.assertEqual(rig.boards_of("pico-i2c-sim.sh", ["run"]), ["f401", "h753", "pico-i2c-sim"])

    def test_unknown_script_names_no_board(self):
        self.assertEqual(rig.boards_of("vcp_echo.py", []), [])
        self.assertEqual(rig.boards_of("flash-dfu.sh", []), [])


class Pins(unittest.TestCase):
    def test_only_attached_probes_are_pinned(self):
        listing = FAKE_PROBE_RS.replace('echo "', "").replace('"\n', "\n")
        pins = rig.pins(RIG_ENV, listing)
        self.assertEqual(pins, {"F401_PROBE": "0483:374b:F401SERIAL",
                                "H753_PROBE": "0483:374e:H753SERIAL"})


class Cli(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        t = Path(self.tmp.name)
        self.repo = t / "repo"
        (self.repo / "hil" / "sub").mkdir(parents=True)
        executable(self.repo / "hil" / "run-h753.sh", FAKE_BENCH)
        executable(self.repo / "hil" / "run-f401.sh", FAKE_BENCH)
        executable(self.repo / "hil" / "vcp_echo.py", FAKE_BENCH)
        git = ["git", "-c", "user.name=t", "-c", "user.email=t@t", "-C", str(self.repo)]
        subprocess.run([*git, "init", "-q"], check=True)
        subprocess.run([*git, "add", "."], check=True)
        subprocess.run([*git, "commit", "-qm", "bench"], check=True)
        self.head = subprocess.run([*git, "rev-parse", "--short=12", "HEAD"], check=True,
                                   capture_output=True, text=True).stdout.strip()
        bin_dir = t / "bin"
        bin_dir.mkdir()
        executable(bin_dir / "probe-rs", FAKE_PROBE_RS)
        (t / "rig.env").write_text(RIG_ENV)
        self.env = {
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "HOME": os.environ.get("HOME", "/tmp"),
            "RIG_ENV": str(t / "rig.env"),
            "RIG_LOCK_DIR": str(t / "locks"),
            "RIG_TELEMETRY": str(t / "runs.jsonl"),
            "RIG_REQUESTER": "test",
        }

    def tearDown(self):
        self.tmp.cleanup()

    def rig(self, *args, cwd=None, env=None, wait=True):
        e = {**self.env, **(env or {})}
        p = subprocess.Popen([sys.executable, "-I", str(RIG), *args], cwd=cwd or self.repo,
                             env=e, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        if not wait:
            return p
        out, _ = p.communicate(timeout=60)
        return p.returncode, out

    def test_runs_the_script_with_pins_and_args(self):
        code, out = self.rig("run-h753.sh", "a", "b")
        self.assertEqual(code, 0, out)
        self.assertIn("H753_PROBE=0483:374e:H753SERIAL", out)
        self.assertIn("DISCOVERY_PROBE=\n", out)
        self.assertIn("args: a b", out)

    def test_finds_the_script_from_a_subdirectory_and_with_hil_prefix(self):
        code, out = self.rig("hil/run-f401.sh", cwd=self.repo / "hil" / "sub")
        self.assertEqual(code, 0, out)
        self.assertIn("F401_PROBE=0483:374b:F401SERIAL", out)

    def test_the_environment_wins_over_the_pin(self):
        code, out = self.rig("run-h753.sh", env={"H753_PROBE": "mine"})
        self.assertIn("H753_PROBE=mine", out)

    def test_exit_status_passes_through(self):
        code, out = self.rig("run-h753.sh", env={"BENCH_EXIT": "3"})
        self.assertEqual(code, 3, out)

    def test_unknown_script_is_refused_without_board(self):
        code, out = self.rig("run-nothing.sh")
        self.assertEqual(code, 2, out)
        self.assertIn("no hil/run-nothing.sh", out)
        code, out = self.rig("vcp_echo.py")
        self.assertEqual(code, 2, out)
        self.assertIn("--board", out)
        code, out = self.rig("--board", "f401", "vcp_echo.py")
        self.assertEqual(code, 0, out)

    def test_a_held_board_is_refused_and_names_its_holder(self):
        gate = Path(self.tmp.name) / "gate"
        first = self.rig("run-h753.sh", env={"BENCH_WAIT": str(gate), "RIG_REQUESTER": "thread-one"},
                         wait=False)
        try:
            for _ in range(200):
                if Path(f"{gate}.started").exists():
                    break
                subprocess.run(["sleep", "0.05"])
            code, out = self.rig("run-h753.sh")
            self.assertEqual(code, rig.BUSY, out)
            self.assertIn("h753 is held by thread-one", out)
            code, out = self.rig("locks")
            self.assertIn("h753", out)
            self.assertIn("thread-one", out)
            # Another board is free meanwhile.
            code, out = self.rig("run-f401.sh")
            self.assertEqual(code, 0, out)
        finally:
            gate.touch()
            first.communicate(timeout=60)
        self.assertEqual(first.returncode, 0)
        code, out = self.rig("run-h753.sh")
        self.assertEqual(code, 0, out)

    def test_runs_are_recorded_in_telemetry(self):
        self.rig("run-h753.sh")
        lines = Path(self.env["RIG_TELEMETRY"]).read_text().splitlines()
        session = [json.loads(x) for x in lines if json.loads(x).get("kind") == "session"]
        self.assertTrue(session, lines)
        self.assertEqual(session[-1]["requester"], "test")
        # The bench's own repository, not FerroHAL's, whose rig_run.py recorded it.
        self.assertEqual(session[-1]["commit"], self.head)


if __name__ == "__main__":
    unittest.main()
