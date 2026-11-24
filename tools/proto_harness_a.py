#!/usr/bin/env -S python3 -I
"""Candidate A's host side (PROTOTYPE, audit step 6): `probe-rs run` on an
embedded-test image, which flashes once and then runs each test after a
reset, holding each test's timeout itself. This wrapper only times it: every
line it passes through carries the seconds since the run began, so when each
test, and each planted hang, was reported is on record.

    proto_harness_a.py <board.toml> <elf> <name> [probe-rs test args...]

Prints `run <name>: exit N, P passed, F failed`, as the rig's telemetry reads.
An outer limit (LIMIT s) stops a run embedded-test itself never ends. Never
requirement evidence.
"""

import os
import re
import selectors
import signal
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rig_check  # noqa: E402

LIMIT = 120
ANSI = re.compile(r"\x1b\[[0-9;]*m")
RESULT = re.compile(r"test result: (\w+)\. (\d+) passed; (\d+) failed")


def main(argv):
    board = rig_check.load_board(Path(argv[0]))
    elf, name, extra = argv[1], argv[2], argv[3:]
    sys.stdout.reconfigure(line_buffering=True)
    cmd = ["probe-rs", "run", "--chip", board.chip, "--probe", board.probe, elf, *extra]
    t0 = time.monotonic()
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            start_new_session=True)
    fd = proc.stdout.fileno()
    sel = selectors.DefaultSelector()
    sel.register(fd, selectors.EVENT_READ)
    pending, text, killed = b"", [], False
    while True:
        if time.monotonic() - t0 > LIMIT:
            os.killpg(proc.pid, signal.SIGTERM)
            killed = True
            break
        if not sel.select(timeout=0.05):
            if proc.poll() is not None:
                break
            continue
        data = os.read(fd, 65536)
        if not data:
            break
        pending += data
        *lines, pending = pending.split(b"\n")
        for raw in lines:
            line = ANSI.sub("", raw.decode("utf-8", "replace"))
            text.append(line)
            print(f"[+{time.monotonic() - t0:6.2f}s] {line}")
    if pending:
        print(f"[+{time.monotonic() - t0:6.2f}s] {ANSI.sub('', pending.decode('utf-8', 'replace'))}")
    try:
        proc.wait(10)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait()
    total = time.monotonic() - t0
    m = next((RESULT.search(x) for x in reversed(text) if RESULT.search(x)), None)
    passed, failed = (int(m[2]), int(m[3])) if m else (0, 0)
    code = 124 if killed else proc.returncode
    print(f"probe-rs exit {proc.returncode}{', killed at the outer limit' if killed else ''}; "
          f"total {total:.2f} s; flashes {sum('Finished in' in x for x in text)}")
    print(f"run {name}: exit {code}, {passed} passed, {failed + (0 if m else 1)} failed")
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
