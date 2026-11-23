"""Runs a bench script and records where its time went: rig telemetry.

    python3 -I tools/rig_run.py hil/run-h753.sh [runs...]
    python3 -I tools/rig_run.py hil/run-foxeer.sh

Wraps any script under hil/ without changing it: the script's output passes
through line by line, and the wrapper exits with the script's own status. It
only watches: each line is timestamped as it arrives, and after each
`run <x>: exit ...` line the run's log (hil/logs/<bench>-run-<x>.log) is read
for cargo's build time and probe-rs's flash time. Because the run scripts are
untouched, wrapping a run owes no bench a rerun.

One JSON line per run, and one per session (the whole script), is appended to
hil/logs/telemetry/runs.jsonl (ignored; RIG_TELEMETRY overrides the path):
bench, board, probe, run id, branch and commit, the phases (probe selection,
build, flash, execution, teardown), the result (pass, fail, hang, timeout,
interrupted; no-probe for a session that found none), the attempt number at
this commit, and RIG_REQUESTER (who asked for the run, when the caller sets
it). Telemetry never fails a run: any error
writing it is printed as one `telemetry:` line and the script's status stands.
tools/rig_report.py summarises the file.
"""

from __future__ import annotations

import datetime
import json
import os
import re
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_LOG = ROOT / "hil" / "logs" / "telemetry" / "runs.jsonl"
VERSION = 1

# bench -> (board, the probe variable hil/probe.sh reads for it)
BOARDS = {
    "f401": ("nucleo-f401re", "F401_PROBE"),
    "h753": ("nucleo-h753zi", "H753_PROBE"),
    "discovery": ("stm32f4-discovery", "DISCOVERY_PROBE"),
    "foxeer": ("foxeer-f405-v2", None),
    "lucid": ("lucid-h7", None),
    "pico-i2c-sim": ("pico-i2c-sim", None),
}

ANSI = re.compile(r"\x1b\[[0-9;]*m")
RUN_LINE = re.compile(r"^run (\S+): exit (-?\d+), (\d+) passed, (\d+) failed")
BUILD_LINE = re.compile(r"^build: (every image built|some images failed)(?: \(in ([0-9.]+s|[0-9]+m [0-9]+s)\))?")
CARGO_FINISHED = re.compile(r"Finished `[^`]*` profile .* in ([0-9.]+s|[0-9]+m [0-9]+s)")
PROBE_RS_FINISHED = re.compile(r"^\s*Finished in ([0-9.]+)s")
PROBE_PICKED = re.compile(r"^probe: \S+ unset; (\S+) has DEV_ID")
PIN_ABSENT = re.compile(r"^probe: \S+ from hil/rig.env is not attached")
NO_PROBE = re.compile(r"^probe: set \S+ \(or PROBE_RS_PROBE\)")


def seconds(text):
    """cargo's durations: `1.72s` or `1m 23s`."""
    m = re.fullmatch(r"(?:(\d+)m )?([0-9.]+)s", text.strip())
    if not m:
        return None
    return int(m.group(1) or 0) * 60 + float(m.group(2))


def bench_of(script, args):
    """The bench a script drives: run-<bench>.sh, flash-dfu.sh <board>, or the script's stem."""
    name = Path(script).name
    if name.startswith("run-") and name.endswith(".sh"):
        return name[4:-3]
    if name == "flash-dfu.sh" and args:
        for bench, (board, _) in BOARDS.items():
            if board == args[0]:
                return bench
        return args[0]
    return Path(name).stem


def parse_run_log(text):
    """(build seconds, flash seconds, whether flashing finished) from one run's log."""
    build = flash = 0.0
    flashed = False
    for line in ANSI.sub("", text).splitlines():
        m = CARGO_FINISHED.search(line)
        if m:
            build += seconds(m.group(1)) or 0.0
            continue
        m = PROBE_RS_FINISHED.match(line)
        if m:
            flash += float(m.group(1))
            flashed = True
    return build, flash, flashed


def classify(code, failed, flashed):
    """A run's result from its exit status and FAIL count. `timeout` kills with
    124: after the image was flashed, the firmware never finished (hang);
    before, build, attach or flashing stalled (timeout)."""
    if code == 124:
        return "hang" if flashed else "timeout"
    if code in (137, 143):
        return "timeout"
    if code == 0 and failed == 0:
        return "pass"
    return "fail"


def git(*args, cwd=ROOT):
    """git in the repository given, FerroHAL's own by default."""
    try:
        res = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, timeout=5)
        return res.stdout.strip() if res.returncode == 0 else None
    except (OSError, subprocess.SubprocessError):
        return None


def probe_of(bench):
    """(probe, where it came from) as hil/probe.sh would start looking: the
    environment, then hil/rig.env. A DEV_ID scan's pick is read from the
    script's own output instead."""
    if os.environ.get("PROBE_RS_PROBE"):
        return os.environ["PROBE_RS_PROBE"], "env"
    var = BOARDS.get(bench, (None, None))[1]
    if not var:
        return None, None
    if os.environ.get(var):
        return os.environ[var], "env"
    try:
        for line in (ROOT / "hil" / "rig.env").read_text().splitlines():
            if line.startswith(f"{var}="):
                return line.split("=", 1)[1].strip(), "rig.env"
    except OSError:
        pass
    return None, None


def iso(t):
    return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def attempts(path, bench, commit):
    """{run: records so far} for this bench at this commit."""
    counts = {}
    try:
        with open(path) as f:
            for line in f:
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                if r.get("kind") == "run" and r.get("bench") == bench and r.get("commit") == commit:
                    counts[r.get("run")] = counts.get(r.get("run"), 0) + 1
    except OSError:
        pass
    return counts


class Session:
    """Watches one script's output and turns it into records."""

    def __init__(self, script, args, start, log_dir):
        self.bench = bench_of(script, args)
        self.board = BOARDS.get(self.bench, (self.bench, None))[0]
        self.args = args
        self.start = start
        self.log_dir = Path(log_dir)
        # The script's own repository (log_dir is <repo>/hil/logs): a sibling's bench,
        # run through tools/rig.py, records that repository's commit, not FerroHAL's.
        self.repo = self.log_dir.parent.parent
        self.mark = start  # when the previous phase ended
        self.prelude = None
        self.build = None
        self.stream_build = 0.0
        self.picked = None
        self.pin_absent = False
        self.no_probe = False
        self.runs = []
        self.last_run_end = None

    def line(self, text, now):
        text = ANSI.sub("", text.rstrip("\n"))
        m = PROBE_PICKED.match(text)
        if m:
            self.picked = m.group(1)
        self.pin_absent |= bool(PIN_ABSENT.match(text))
        self.no_probe |= bool(NO_PROBE.match(text))
        m = CARGO_FINISHED.search(text)
        if m:
            self.stream_build += seconds(m.group(1)) or 0.0
        m = BUILD_LINE.match(text)
        if m and self.prelude is None:
            self.prelude = now - self.start
            self.build = seconds(m.group(2)) if m.group(2) else None
            self.mark = now
            return
        m = RUN_LINE.match(text)
        if m:
            run, code, passed, failed = m.group(1), int(m.group(2)), int(m.group(3)), int(m.group(4))
            if self.prelude is None:
                self.prelude = 0.0  # no build line: probe selection counts in the first run
            wall = now - self.mark
            try:
                build, flash, flashed = parse_run_log((self.log_dir / f"{self.bench}-run-{run}.log").read_text(errors="replace"))
            except OSError:
                build, flash, flashed = 0.0, 0.0, False
            self.runs.append({
                "run": run,
                "start": iso(self.mark),
                "end": iso(now),
                "wall_s": round(wall, 2),
                "phases": {
                    "build": round(build, 2),
                    "flash": round(flash, 2),
                    "exec": round(max(0.0, wall - build - flash), 2),
                },
                "result": classify(code, failed, flashed),
                "exit": code,
                "passed": passed,
                "failed": failed,
            })
            self.mark = now
            self.last_run_end = now

    def records(self, end, code, interrupted, path):
        commit = git("rev-parse", "--short=12", "HEAD", cwd=self.repo)
        common = {
            "v": VERSION,
            "session": f"{self.bench}-{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime(self.start))}-{os.getpid()}",
            "bench": self.bench,
            "board": self.board,
            "host": socket.gethostname(),
            "branch": git("rev-parse", "--abbrev-ref", "HEAD", cwd=self.repo),
            "commit": commit,
            "dirty": bool(git("status", "--porcelain", "--untracked-files=no", cwd=self.repo)),
            "requester": os.environ.get("RIG_REQUESTER") or None,
        }
        if self.picked:
            probe, source = self.picked, "scan"
        elif self.pin_absent:
            probe, source = None, None
        else:
            probe, source = probe_of(self.bench)
        common["probe"] = probe
        common["probe_source"] = source
        seen = attempts(path, self.bench, commit)
        out = []
        for r in self.runs:
            seen[r["run"]] = seen.get(r["run"], 0) + 1
            out.append({**common, "kind": "run", "run_id": f"{common['session']}-{r['run']}",
                        "attempt": seen[r["run"]], **r})
        wall = end - self.start
        prelude = self.prelude if self.prelude is not None else (0.0 if self.runs else wall)
        build = self.build if self.build is not None else (self.stream_build or None if not self.runs else None)
        phases = {"prelude": round(prelude, 2)}
        if build is not None:
            phases["build"] = round(build, 2)
            phases["probe_select"] = round(max(0.0, prelude - build), 2) if self.runs else None
        phases["runs"] = round(sum(r["wall_s"] for r in self.runs), 2)
        phases["teardown"] = round(end - self.last_run_end, 2) if self.last_run_end else None
        results = [r["result"] for r in self.runs]
        if interrupted:
            result = "interrupted"
        elif code == 0:
            result = "pass"
        elif self.no_probe and not self.runs:
            result = "no-probe"
        elif "hang" in results:
            result = "hang"
        elif "timeout" in results and "fail" not in results:
            result = "timeout"
        else:
            result = "fail"
        out.append({**common, "kind": "session", "run_id": common["session"],
                    "args": self.args, "start": iso(self.start), "end": iso(end),
                    "wall_s": round(wall, 2), "phases": {k: v for k, v in phases.items() if v is not None},
                    "runs": len(self.runs), "result": result, "exit": code})
        return out


def append(path, records):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a") as f:
        for r in records:
            f.write(json.dumps(r, separators=(",", ":")) + "\n")


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__.strip())
        return 0 if argv else 2
    script, args = argv[0], argv[1:]
    path = Path(os.environ.get("RIG_TELEMETRY") or DEFAULT_LOG)
    session = None
    start = time.time()
    try:
        session = Session(script, args, start, Path(script).resolve().parent / "logs")
    except Exception as e:  # telemetry must never stop a run
        print(f"telemetry: not recording ({e})", file=sys.stderr)
    proc = subprocess.Popen([script, *args], stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, errors="replace", bufsize=1)
    interrupted = False
    while True:
        try:
            for text in proc.stdout:
                sys.stdout.write(text)
                sys.stdout.flush()
                if session:
                    try:
                        session.line(text, time.time())
                    except Exception as e:
                        print(f"telemetry: stopped watching ({e})", file=sys.stderr)
                        session = None
            break
        except KeyboardInterrupt:
            # The script has the same signal; let it finish its own cleanup.
            interrupted = True
            proc.send_signal(signal.SIGINT)
    code = proc.wait()
    if session:
        try:
            append(path, session.records(time.time(), code, interrupted, path))
        except Exception as e:
            print(f"telemetry: not written ({e})", file=sys.stderr)
    return code if code >= 0 else 128 - code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
