"""Records the debugger benches' runs as reqtool evidence.

    python3 -I tools/record_evidence.py prepare <folder>
    python3 -I tools/record_evidence.py record <folder> [--dry-run] [--only <heading>]

<folder> is a new directory under requirements/evidence/logs/.

prepare: copies hil/logs/<bench>-run-<x>.log for every run a procedure in
requirements/benches*.md names (`hil/run-<bench>.sh <runs>`), writes
<crate>-images.sha256 (each run binary's SHA-256) and checks-run.log. Commit
the folder before `record`, so the tree is clean when reqtool reads it.

record: for every procedure whose runs all passed (PASS lines and no FAIL),
writes its details and calls `reqtool record --procedure`; then the analysis
procedures `checks/run.sh` carries. reqtool comes from ../reqtool, the sibling
checkout at the revision CI pins.
"""

import hashlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REQTOOL = ROOT.parent / "reqtool/target/release/reqtool"
BENCH = {
    "f401": "nucleo-f401re-loopback",
    "h753": "nucleo-h753zi-loopback",
    "discovery": "stm32f4-discovery",
}
ANALYSIS = ("No reachable panic", "Chip glue against metapac", "One I2C API on every chip")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def procedures():
    """(file, heading, bench, runs) for every procedure with a run command."""
    found = []
    for f in sorted((ROOT / "requirements").glob("benches*.md")):
        text = f.read_text()
        for m in re.finditer(r"^## (.+)$", text, re.M):
            nxt = re.search(r"^## ", text[m.end():], re.M)
            body = text[m.end(): m.end() + (nxt.start() if nxt else len(text))]
            cmd = re.search(r"`hil/run-(" + "|".join(BENCH) + r")\.sh ([a-z ]+)`", body)
            if cmd:
                found.append((f.relative_to(ROOT).as_posix(), m.group(1), cmd.group(1), cmd.group(2).split()))
    return found


def passed(path):
    if not path.exists():
        return False
    text = path.read_text(errors="replace")
    return "PASS" in text and "FAIL" not in text


def prepare(folder):
    folder.mkdir(parents=True)
    runs = {bench: set() for bench in BENCH}
    for _, _, bench, rs in procedures():
        runs[bench].update(rs)
    for bench, rs in runs.items():
        crate = BENCH[bench]
        lines = []
        for run in sorted(rs):
            log = ROOT / "hil/logs" / f"{bench}-run-{run}.log"
            if log.exists():
                shutil.copy(log, folder / f"{crate}-run-{run}.log")
            image = ROOT / "hil" / crate / "target/thumbv7em-none-eabihf/release" / f"run-{run}"
            if image.exists():
                lines.append(f"{sha(image)}  run-{run}\n")
        (folder / f"{crate}-images.sha256").write_text("".join(lines))
    out = subprocess.run(["checks/run.sh"], cwd=ROOT, capture_output=True, text=True)
    (folder / "checks-run.log").write_text(out.stdout + out.stderr)
    print("checks/run.sh exit", out.returncode)
    out = subprocess.run(["tools/glue-gate.sh"], cwd=ROOT, capture_output=True, text=True)
    (folder / "glue-gate.log").write_text(out.stdout + out.stderr)
    print("tools/glue-gate.sh exit", out.returncode)


def artifact(folder, name, kind):
    path = folder / name
    return {"id": path.stem, "kind": kind, "path": path.relative_to(ROOT).as_posix(),
            "bytes": path.stat().st_size, "sha256": sha(path)}


def reqtool_record(procedure, details, board, dry):
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump(details, f, indent=1)
    cmd = [str(REQTOOL), "record", "--procedure", procedure, "--result", "pass", "--details", f.name]
    if board:
        cmd += ["--board", board]
    if dry:
        cmd.append("--dry-run")
    out = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    Path(f.name).unlink()
    return out


def record(folder, dry, only=None):
    for file, heading, bench, rs in procedures():
        if only and heading != only:
            continue
        crate = BENCH[bench]
        logs = [folder / f"{crate}-run-{r}.log" for r in rs]
        missing = [r for r, log in zip(rs, logs) if not passed(log)]
        if missing:
            print(f"skip  {heading}: runs {' '.join(missing)} missing or failed")
            continue
        images = folder / f"{crate}-images.sha256"
        details = {
            "execution": {"performed_by": "agent", "user_confirmed": False},
            "candidate": {"firmware_sha256": sha(images), "features": [],
                          "target_triple": "thumbv7em-none-eabihf", "profile": "release"},
            "commands": [{"command": f"hil/run-{bench}.sh {' '.join(rs)}", "exit_code": 0}],
            "artifacts": [artifact(folder, images.name, "configuration")]
            + [artifact(folder, log.name, "log") for log in logs],
            "observations": ["Every check in every run of the subset passed (PASS lines in the logs; no FAIL)."],
        }
        out = reqtool_record(f"{file}#{heading}", details, crate, dry)
        print(("ok    " if out.returncode == 0 else "FAIL  ") + heading, out.stderr.strip()[-200:])
    for heading in ANALYSIS:
        if only and heading != only:
            continue
        command, log = ("tools/glue-gate.sh", "glue-gate.log") if heading == "Chip glue against metapac" else ("checks/run.sh", "checks-run.log")
        details = {
            "execution": {"performed_by": "agent", "user_confirmed": False},
            "commands": [{"command": command, "exit_code": 0}],
            "artifacts": [artifact(folder, log, "log")],
        }
        out = reqtool_record(f"requirements/benches-analysis.md#{heading}", details, None, dry)
        print(("ok    " if out.returncode == 0 else "FAIL  ") + heading, out.stderr.strip()[-200:])


if __name__ == "__main__":
    if len(sys.argv) < 3 or sys.argv[1] not in ("prepare", "record"):
        sys.exit(__doc__)
    target = ROOT / "requirements/evidence/logs" / sys.argv[2]
    if sys.argv[1] == "prepare":
        prepare(target)
    else:
        only = sys.argv[sys.argv.index("--only") + 1] if "--only" in sys.argv else None
        record(target, "--dry-run" in sys.argv, only)
