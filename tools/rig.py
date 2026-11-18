#!/usr/bin/env -S python3 -I
"""rig: the one entry point for bench scripts on this rig.

    rig <script> [args...]          run hil/<script> of the repository you are in
    rig --board h753 <script> ...   name the boards of a script rig cannot place
    rig --wait <script> ...         wait for a held board instead of refusing
    rig list                        the repository's hil/ scripts and their boards
    rig locks                       which boards are held, and by whom

The script is looked up as hil/<script> in the nearest directory, from the
current one upwards, that has it, so the same command runs FerroHAL's benches
in ferrohal-stm32 and FerroForge's in ferroforge-stm32. A leading `hil/` is
accepted. Agents are allowed this one command, so a new script needs no new
permission rule.

Before the script starts, rig:

- takes a lock for every board the script drives, from its name (`run-h753*.sh`
  is the H753), its first argument (`flash-dfu.sh lucid-h7`), or `--board`. A
  board another rig holds is refused at once with the holder's name, pid,
  script and start time, exit status 75; `--wait` queues instead. Locks live in
  $RIG_LOCK_DIR (default /tmp/ferro-rig) and are released when rig exits,
  however it exits. Who holds one is RIG_REQUESTER, else the user name.
- exports each board's probe pin from FerroHAL's hil/rig.env (F401_PROBE,
  H753_PROBE, DISCOVERY_PROBE, ...), only while that probe is attached and only
  where the environment leaves the variable unset, so scripts that do not read
  hil/rig.env themselves get the pins too. $RIG_ENV moves the file.

The script runs under tools/rig_run.py, so every run is in the rig telemetry
(hil/RIG.md, Telemetry), and its exit status is rig's.
"""

from __future__ import annotations

import datetime
import fcntl
import getpass
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RIG_RUN = ROOT / "tools" / "rig_run.py"
BUSY = 75  # EX_TEMPFAIL: try again later

# Board key -> the names that mean it in a script's name or first argument.
BOARDS = {
    "f401": ("f401",),
    "h753": ("h753",),
    "discovery": ("discovery",),
    "f767": ("f767",),
    "g474": ("g474",),
    "foxeer": ("foxeer",),
    "lucid": ("lucid",),
    "speedybee": ("speedybee",),
    "pico-i2c-sim": ("pico",),
}
# Scripts that drive more than their name says.
SCRIPT_BOARDS = {
    "pico-i2c-sim.sh": ["f401", "h753", "pico-i2c-sim"],
}
# Scripts whose board is their first argument.
BOARD_ARGUMENT = {"flash-dfu.sh", "run-event-adc.sh"}


def match_boards(text):
    words = re.split(r"[-_.]", text.lower())
    return [b for b, names in BOARDS.items() if any(n in words for n in names)]


def boards_of(script, args):
    """The boards a script drives, in a fixed order, or [] when it cannot be told."""
    name = Path(script).name
    if name in SCRIPT_BOARDS:
        return list(SCRIPT_BOARDS[name])
    if name in BOARD_ARGUMENT:
        return match_boards(args[0]) if args else []
    return match_boards(Path(name).stem)


PIN_LINE = re.compile(r"^([A-Z0-9_]+_PROBE)=(\S+)\s*$")
LISTED = re.compile(r" -- (\S+) \(")


def pins(rig_env_text, probe_list_text):
    """The rig.env pins whose probe `probe-rs list` shows attached."""
    attached = set(LISTED.findall(probe_list_text))
    out = {}
    for line in rig_env_text.splitlines():
        m = PIN_LINE.match(line.strip())
        if m and m.group(2) in attached:
            out[m.group(1)] = m.group(2)
    return out


def find_script(script, start):
    rel = script[4:] if script.startswith("hil/") else script
    if "/" in rel or not rel:
        return None
    for d in [start, *start.parents]:
        candidate = d / "hil" / rel
        if candidate.is_file():
            return candidate
    return None


def lock_dir():
    return Path(os.environ.get("RIG_LOCK_DIR") or "/tmp/ferro-rig")


def holder_of(path):
    try:
        return json.loads(path.read_text() or "{}")
    except (OSError, ValueError):
        return {}


def describe(board, h):
    who = h.get("requester") or "someone"
    return (f"{board} is held by {who} (pid {h.get('pid', '?')}, {h.get('script', '?')} "
            f"{' '.join(h.get('args', []))}, since {h.get('since', '?')})")


def take_locks(boards, me, wait):
    """Opens and locks one file per board; returns the open files, or exits BUSY."""
    d = lock_dir()
    d.mkdir(parents=True, exist_ok=True)
    held = []
    for board in sorted(boards):  # one order for everyone: no deadlock under --wait
        path = d / f"{board}.lock"
        f = open(path, "a+")
        try:
            fcntl.flock(f, fcntl.LOCK_EX | (0 if wait else fcntl.LOCK_NB))
        except BlockingIOError:
            print(f"rig: {describe(board, holder_of(path))}; rerun later or pass --wait",
                  file=sys.stderr)
            f.close()
            for g in held:
                g.close()
            sys.exit(BUSY)
        f.seek(0)
        f.truncate()
        f.write(json.dumps({**me, "board": board}))
        f.flush()
        held.append(f)
    return held


def show_locks():
    d = lock_dir()
    found = False
    for path in sorted(d.glob("*.lock")) if d.is_dir() else []:
        with open(path) as f:
            try:
                fcntl.flock(f, fcntl.LOCK_SH | fcntl.LOCK_NB)
                fcntl.flock(f, fcntl.LOCK_UN)
                continue  # free
            except BlockingIOError:
                pass
        found = True
        print(describe(path.stem, holder_of(path)))
    if not found:
        print("rig: no board is held")
    return 0


def show_list(start):
    for d in [start, *start.parents]:
        hil = d / "hil"
        if hil.is_dir() and any(hil.glob("*.sh")):
            for s in sorted(hil.glob("*.sh")):
                if s.name == "probe.sh":
                    continue
                boards = boards_of(s.name, [])
                note = ", ".join(boards) if boards else (
                    "board from its first argument" if s.name in BOARD_ARGUMENT else "no board (--board)")
                print(f"{s.name:32} {note}")
            return 0
    print("rig: no hil/ with scripts here or above", file=sys.stderr)
    return 2


def probe_list():
    try:
        return subprocess.run(["probe-rs", "list"], capture_output=True, text=True,
                              timeout=30).stdout
    except (OSError, subprocess.SubprocessError):
        return ""


def main(argv):
    boards, wait = [], False
    while argv and argv[0].startswith("--"):
        opt = argv.pop(0)
        if opt == "--wait":
            wait = True
        elif opt == "--board" and argv:
            boards += [b for b in argv.pop(0).split(",") if b]
        elif opt in ("--help",):
            print(__doc__.strip())
            return 0
        else:
            print(f"rig: unknown option {opt}", file=sys.stderr)
            return 2
    if not argv:
        print(__doc__.strip())
        return 2
    cwd = Path.cwd()
    if argv == ["locks"]:
        return show_locks()
    if argv == ["list"]:
        return show_list(cwd)

    script, args = argv[0], argv[1:]
    path = find_script(script, cwd)
    if path is None:
        name = script[4:] if script.startswith("hil/") else script
        print(f"rig: no hil/{name} here or in a directory above {cwd}", file=sys.stderr)
        return 2
    unknown = [b for b in boards if b not in BOARDS]
    if unknown:
        print(f"rig: unknown board {', '.join(unknown)}; boards: {', '.join(BOARDS)}", file=sys.stderr)
        return 2
    boards = boards or boards_of(path.name, args)
    if not boards:
        print(f"rig: cannot tell which boards {path.name} drives; name them with "
              f"--board (one of {', '.join(BOARDS)})", file=sys.stderr)
        return 2

    me = {
        "requester": os.environ.get("RIG_REQUESTER") or getpass.getuser(),
        "pid": os.getpid(),
        "script": str(path.relative_to(path.parent.parent)),
        "args": args,
        "repo": str(path.parent.parent),
        "since": datetime.datetime.now().astimezone().isoformat(timespec="seconds"),
    }
    held = take_locks(boards, me, wait)

    env = dict(os.environ)
    env.setdefault("RIG_REQUESTER", me["requester"])
    try:
        rig_env = Path(os.environ.get("RIG_ENV") or ROOT / "hil" / "rig.env").read_text()
    except OSError:
        rig_env = ""
    if rig_env:
        for var, value in pins(rig_env, probe_list()).items():
            env.setdefault(var, value)

    try:
        proc = subprocess.run([sys.executable, "-I", str(RIG_RUN), str(path), *args],
                              cwd=path.parent.parent, env=env)
        return proc.returncode
    except KeyboardInterrupt:
        return 130
    finally:
        for f in held:
            f.close()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
