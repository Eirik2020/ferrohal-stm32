"""Tests for tools/rig_run.py and tools/rig_report.py: python3 -I tools/test_rig_telemetry.py

The wrapper must pass a script's status through untouched, never fail a run
because telemetry could not be written, and split each run's time into build,
flash and execution from the log the run script already writes. The report
must count results, percentiles, flaky runs and busy time correctly.
"""

import datetime
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
import rig_report  # noqa: E402
import rig_run  # noqa: E402

UTC = datetime.timezone.utc

# A run log as hil/run-h753.sh leaves it, probe-rs's colour codes included.
RUN_LOG = (
    "    Finished `release` profile [optimized + debuginfo] target(s) in 1.72s\n"
    "     Running `probe-rs run --chip STM32H753ZI target/thumbv7em-none-eabihf/release/run-a`\n"
    "     \x1b[38;5;10m\x1b[1mFinished\x1b[0m in 2.56s\n"
    "[INFO ] PASS USART6 echo [UART-001]\n"
)

# A stand-in bench: the same lines a run script prints, and its run logs.
FAKE_BENCH = """#!/usr/bin/env bash
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/logs"
echo "probe: H753_PROBE unset; 0483:374e:SERIAL1 has DEV_ID 0x450" >&2
echo "build: every image built (in 1m 2s)"
printf '%s' "$RUN_A" > "$here/logs/h753-run-a.log"
echo "run a: exit 0, 1 passed, 0 failed"
printf '    Finished `release` profile x in 0.1s\\n' > "$here/logs/h753-run-b.log"
echo "run b: exit 124, 0 passed, 0 failed"
exit "${FAKE_EXIT:-1}"
"""


def fake_bench():
    folder = Path(tempfile.mkdtemp()) / "hil"
    folder.mkdir()
    script = folder / "run-h753.sh"
    script.write_text(FAKE_BENCH)
    script.chmod(script.stat().st_mode | stat.S_IEXEC)
    return script


def wrap(script, telemetry, **env):
    return subprocess.run(
        [sys.executable, "-I", str(HERE / "rig_run.py"), str(script)],
        capture_output=True, text=True,
        env={**os.environ, "RIG_TELEMETRY": str(telemetry), "RUN_A": RUN_LOG, **env},
    )


class Parsing(unittest.TestCase):
    def test_cargo_durations(self):
        self.assertEqual(rig_run.seconds("1.72s"), 1.72)
        self.assertEqual(rig_run.seconds("1m 23s"), 83)

    def test_a_run_log_splits_build_and_flash(self):
        self.assertEqual(rig_run.parse_run_log(RUN_LOG), (1.72, 2.56, True))

    def test_results(self):
        self.assertEqual(rig_run.classify(0, 0, True), "pass")
        self.assertEqual(rig_run.classify(0, 1, True), "fail")
        self.assertEqual(rig_run.classify(124, 0, True), "hang")
        self.assertEqual(rig_run.classify(124, 0, False), "timeout")

    def test_benches(self):
        self.assertEqual(rig_run.bench_of("hil/run-h753.sh", []), "h753")
        self.assertEqual(rig_run.bench_of("hil/flash-dfu.sh", ["lucid-h7", "16*128Kg"]), "lucid")
        self.assertEqual(rig_run.bench_of("hil/pico-i2c-sim.sh", []), "pico-i2c-sim")


class Wrapper(unittest.TestCase):
    def test_records_runs_and_session_and_keeps_the_status(self):
        script = fake_bench()
        telemetry = script.parent / "t" / "runs.jsonl"
        res = wrap(script, telemetry, FAKE_EXIT="3")
        self.assertEqual(res.returncode, 3)
        self.assertIn("run a: exit 0", res.stdout)
        records = [json.loads(line) for line in telemetry.read_text().splitlines()]
        a, b, session = records
        self.assertEqual((a["kind"], a["run"], a["result"], a["board"]), ("run", "a", "pass", "nucleo-h753zi"))
        self.assertEqual((a["phases"]["build"], a["phases"]["flash"]), (1.72, 2.56))
        self.assertEqual(a["probe"], "0483:374e:SERIAL1")
        self.assertEqual(a["probe_source"], "scan")
        self.assertEqual(b["result"], "timeout")
        self.assertEqual((session["kind"], session["runs"], session["result"]), ("session", 2, "timeout"))
        self.assertEqual(session["phases"]["build"], 62)

    def test_attempts_count_per_commit(self):
        script = fake_bench()
        telemetry = script.parent / "runs.jsonl"
        wrap(script, telemetry)
        wrap(script, telemetry)
        runs = [json.loads(line) for line in telemetry.read_text().splitlines()]
        self.assertEqual([r["attempt"] for r in runs if r["kind"] == "run"], [1, 1, 2, 2])

    def test_a_session_with_no_probe_says_so(self):
        script = fake_bench()
        script.write_text("#!/usr/bin/env bash\n"
                          "echo 'probe: H753_PROBE=X from hil/rig.env is not attached; finding the die instead' >&2\n"
                          "echo 'probe: set H753_PROBE (or PROBE_RS_PROBE) to one of these,' >&2\nexit 1\n")
        telemetry = script.parent / "runs.jsonl"
        self.assertEqual(wrap(script, telemetry).returncode, 1)
        (session,) = [json.loads(line) for line in telemetry.read_text().splitlines()]
        self.assertEqual((session["result"], session["probe"], session["runs"]), ("no-probe", None, 0))

    def test_an_unwritable_log_never_fails_the_run(self):
        script = fake_bench()
        blocker = script.parent / "file"
        blocker.write_text("")
        res = wrap(script, blocker / "runs.jsonl", FAKE_EXIT="0")
        self.assertEqual(res.returncode, 0)
        self.assertIn("telemetry: not written", res.stderr)


def record(kind, bench, run, start, wall, result="pass", commit="c1", phases=None, attempt=1):
    t0 = datetime.datetime(2026, 11, 17, tzinfo=UTC) + datetime.timedelta(minutes=start)
    r = {"kind": kind, "bench": bench, "board": rig_run.BOARDS[bench][0], "run": run, "commit": commit,
         "start": t0.isoformat(), "end": (t0 + datetime.timedelta(seconds=wall)).isoformat(),
         "wall_s": wall, "result": result, "attempt": attempt,
         "phases": phases or {"build": 0.0, "flash": wall / 2, "exec": wall / 2}}
    r["_start"], r["_end"] = rig_report.parse_time(r["start"]), rig_report.parse_time(r["end"])
    return r


class Report(unittest.TestCase):
    def setUp(self):
        self.start = datetime.datetime(2026, 11, 17, tzinfo=UTC)
        self.end = self.start + datetime.timedelta(hours=10)
        self.records = [
            record("session", "h753", None, 0, 600, phases={"build": 60, "probe_select": 1}),
            record("run", "h753", "a", 1, 100),
            record("run", "h753", "b", 3, 300, result="fail"),
            record("run", "h753", "b", 9, 200, attempt=2),
            record("session", "f401", None, 5, 120),
            record("run", "f401", "a", 5, 60),
            record("session", "f401", None, 300, 60),
        ]

    def test_boards_rates_and_percentiles(self):
        s = rig_report.summarise(self.records, self.start, self.end)
        h7 = s["boards"]["nucleo-h753zi"]
        self.assertEqual((h7["runs"], h7["passed"], h7["p95_s"], h7["total_s"]), (3, 2, 300, 600))
        self.assertEqual(h7["phases_s"]["flash"], 300)
        self.assertEqual(s["flaky"], ["h753 b at c1: fail, pass"])
        self.assertEqual(s["retries"], 1)

    def test_busy_merges_overlapping_sessions(self):
        s = rig_report.summarise(self.records, self.start, self.end)
        self.assertEqual(s["busy_s"], 660)  # 0 to 10 min (the F401 overlaps it), 300 to 301 min
        self.assertEqual(s["longest_idle_s"], 10 * 3600 - 301 * 60)

    def test_a_board_filter_and_an_empty_window(self):
        s = rig_report.summarise(self.records, self.start, self.end, board="f401")
        self.assertEqual(list(s["boards"]), ["nucleo-f401re"])
        later = self.end + datetime.timedelta(days=1)
        empty = rig_report.summarise(self.records, self.end, later)
        self.assertIn("No runs recorded", rig_report.render(empty))

    def test_the_default_output_is_short(self):
        text = rig_report.render(rig_report.summarise(self.records, self.start, self.end))
        self.assertLessEqual(len(text.splitlines()), 8)
        self.assertIn("nucleo-h753zi", text)

    def test_since(self):
        now = datetime.datetime(2026, 11, 17, 12, tzinfo=UTC)
        self.assertEqual(rig_report.since_of("24h", now), now - datetime.timedelta(days=1))
        self.assertEqual(rig_report.since_of("2026-11-10", now), datetime.datetime(2026, 11, 10, tzinfo=UTC))

    def test_torn_lines_are_skipped(self):
        path = Path(tempfile.mkdtemp()) / "runs.jsonl"
        path.write_text('{"kind": "run"\nnot json\n')
        self.assertEqual(rig_report.load(path), [])


if __name__ == "__main__":
    unittest.main()
