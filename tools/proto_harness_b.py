#!/usr/bin/env -S python3 -I
"""Candidate B's host side (PROTOTYPE, audit step 6): flash and run one bench
image with `probe-rs run`, tell it which stages to run, hold every stage to the
limit the image announces, and name the stage left open when it stops a run.

    proto_harness_b.py <board.toml> <elf> [--select "WORD ..."] [--name NAME] [--probe-rs ARG]...

The image prints `stage <name> start <limit_s>`, `stage <name> done` and
`<ID> pass|fail` (hil/proto-discovery-b/src/harness.rs). Deadlines:

- flash: until probe-rs says the flash is done (FLASH_LIMIT s);
- reset: from then until the first stage line (BOOT_LIMIT s): an image dead
  before its first stage is a hang at "reset";
- each stage: its own announced limit; between stages, BOOT_LIMIT.

Prints one summary line (`run <name>: exit N, P passed, F failed`, as the
rig's telemetry reads) and, on a hang, which stage hung, its limit and the
seconds from the start of the run to the report. Exit 0 pass, 1 a check
failed, 124 a hang. Never requirement evidence.
"""

from __future__ import annotations

import os
import re
import selectors
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rig_check  # noqa: E402

FLASH_LIMIT = 30
BOOT_LIMIT = 3
STAGE_START = re.compile(r"stage (\S+) start (\d+)")
STAGE_DONE = re.compile(r"stage (\S+) done")
# A requirement ID, or a prototype probe's own name (S17: section 17's claims).
VERDICT = re.compile(r"\b([A-Z][A-Z0-9]*-[0-9]{3}|S17) (pass|fail)\b")
ANSI = re.compile(r"\x1b\[[0-9;]*m")
FLASHED = re.compile(r"Finished in ([0-9.]+)\s*s")


def run(board, elf: str, select: str, name: str, extra=()) -> int:
    with tempfile.NamedTemporaryFile("w", suffix=".select", delete=False) as f:
        f.write(select)
    cmd = ["probe-rs", "run", "--chip", board.chip, "--probe", board.probe,
           "--semihosting-file", f"select={f.name}", *extra, elf]
    t0 = time.monotonic()
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            start_new_session=True)
    # Raw reads, split here: a buffered reader can hold whole lines that
    # select() then never reports, and the stage they name would be missed.
    fd = proc.stdout.fileno()
    sel = selectors.DefaultSelector()
    sel.register(fd, selectors.EVENT_READ)
    phase, limit, deadline = "flash", FLASH_LIMIT, t0 + FLASH_LIMIT
    flash_s, first_stage_s = None, None
    verdicts: list[tuple[str, str]] = []
    hung = None
    pending = b""
    eof = False
    while not eof:
        now = time.monotonic()
        if now > deadline:
            hung = (phase, limit, now - t0)
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(5)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
            break
        if not sel.select(timeout=0.05):
            continue
        data = os.read(fd, 65536)
        if not data:
            eof = True
            data = b"\n" if pending else b""
        pending += data
        *lines, pending = pending.split(b"\n")
        for raw in lines:
            chunk = raw.decode("utf-8", "replace") + "\n"
            sys.stdout.write(chunk)
            line = ANSI.sub("", chunk)
            now = time.monotonic()
            if flash_s is None and (m := FLASHED.search(line)):
                flash_s = now - t0
                phase, limit, deadline = "reset", BOOT_LIMIT, now + BOOT_LIMIT
            elif m := STAGE_START.search(line):
                first_stage_s = first_stage_s or now - t0
                phase, limit, deadline = m[1], int(m[2]), now + int(m[2])
            elif m := STAGE_DONE.search(line):
                phase, limit, deadline = f"after {m[1]}", BOOT_LIMIT, now + BOOT_LIMIT
            elif m := VERDICT.search(line):
                verdicts.append((m[1], m[2]))
    proc.wait()
    total = time.monotonic() - t0
    os.unlink(f.name)
    passed = sum(v == "pass" for _, v in verdicts)
    failed = sum(v == "fail" for _, v in verdicts)
    timing = (f"flash {flash_s:.2f} s" if flash_s is not None else "flash not seen") + \
        (f", first stage at {first_stage_s:.2f} s" if first_stage_s else "") + f", total {total:.2f} s"
    if hung:
        stage, lim, at = hung
        print(f"HANG in stage {stage} (limit {lim} s), reported {at:.2f} s after the run began; {timing}")
        code = 124
    else:
        code = 0 if proc.returncode == 0 and failed == 0 and passed > 0 else 1
        if proc.returncode != 0 and not phase.startswith("after "):
            print(f"ENDED in stage {phase}: the image stopped there (probe-rs exit {proc.returncode})")
            failed += 1
        print(f"probe-rs exit {proc.returncode}; {timing}")
    print(f"run {name}: exit {code}, {passed} passed, {failed + (1 if hung else 0)} failed")
    return code


def main(argv):
    if len(argv) < 2:
        print(__doc__.strip())
        return 2
    board = rig_check.load_board(Path(argv[0]))
    elf, select, name, extra = argv[1], "", Path(argv[1]).name, []
    rest = argv[2:]
    while rest:
        opt = rest.pop(0)
        if opt == "--select" and rest:
            select = rest.pop(0)
        elif opt == "--name" and rest:
            name = rest.pop(0)
        elif opt == "--probe-rs" and rest:
            extra.append(rest.pop(0))
        else:
            print(f"unknown option {opt}")
            return 2
    sys.stdout.reconfigure(line_buffering=True)
    return run(board, elf, select, name, extra)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
