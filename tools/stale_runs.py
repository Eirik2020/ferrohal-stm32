"""Names the bench runs a change owes: the runs whose evidence is not current.

    python3 -I tools/stale_runs.py [--base <rev>] [--shell]

Two sources, joined:

- reqtool's own view (`reqtool check --json`): every requirement entry that is
  not verified - stale, unverified or failed - names the procedure that verifies
  it, and each procedure in requirements/benches*.md names its runs
  (`hil/run-<bench>.sh <runs>`). reqtool judges a requirement by the files that
  declare it, so a change to one IP leaves the others verified; an entry whose
  board is itself still verified (it fell only because a file another chip
  compiles changed) owes nothing on that bench.
- the bench code itself, which declares no requirement, so reqtool never marks
  anything stale when only it changes: a run whose own source changed since
  <base> (default origin/main), or the working tree's uncommitted changes, is
  owed; a change to what every run of a bench compiles or runs through (its
  crate's board.rs, manifest, memory.x, build.rs, .cargo, hil/common/, its run
  script, hil/probe.sh) owes every run of that bench.

Prints one line per debugger bench with the runs owed, in each script's order,
and notes for what no debugger bench runs: flashed benches (run only when what
they alone reach changed, requirements-system.md) and host procedures
(checks/run.sh, tools/glue-gate.sh). --shell prints the bench commands alone.
reqtool is ../reqtool's build, as for record_evidence.py, or REQTOOL.
Nothing is run and nothing is written.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
import record_evidence  # noqa: E402  (procedures(), BENCH, FLASHED, REQTOOL)

NOT_CURRENT = {"stale", "unverified", "failed"}


def script_order(bench):
    """The runs of hil/run-<bench>.sh in its default order."""
    text = (ROOT / "hil" / f"run-{bench}.sh").read_text()
    m = re.search(r"for run in \$\{\*:-([a-z ]+)\}", text)
    return m.group(1).split() if m else []


def changed_files(base):
    """Files changed since the merge base with <base>, plus uncommitted ones."""
    out = set()
    for cmd in (["git", "diff", "--name-only", f"{base}...HEAD"], ["git", "diff", "--name-only", "HEAD"]):
        res = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        if res.returncode != 0:
            sys.exit(f"stale_runs: {' '.join(cmd)} failed: {res.stderr.strip()}")
        out.update(line for line in res.stdout.splitlines() if line)
    return out


def owed_by_code(files, runs_of):
    """{bench: runs} owed because the bench code itself changed."""
    owed = {bench: set() for bench in record_evidence.BENCH}
    for bench, crate in record_evidence.BENCH.items():
        whole = (
            f"hil/run-{bench}.sh",
            "hil/probe.sh",
            "hil/rig.env",
        )
        for f in files:
            if f in whole or f.startswith("hil/common/"):
                owed[bench].update(runs_of[bench])
            elif f.startswith(f"hil/{crate}/"):
                letters = {
                    f"hil/{crate}/src/bin/{binary.replace('-', '_')}.rs": letter
                    for letter, binary in record_evidence.run_binaries(crate).items()
                }
                if f in letters:
                    owed[bench].add(letters[f])
                else:
                    owed[bench].update(runs_of[bench])
            elif f.startswith("tools/glue-gen/") and "probe" in runs_of[bench]:
                owed[bench].add("probe")
    return owed


def main():
    args = sys.argv[1:]
    base = args[args.index("--base") + 1] if "--base" in args else "origin/main"
    shell = "--shell" in args

    procedures = {f"{file}#{heading}": (bench, runs) for file, heading, bench, runs in record_evidence.procedures()}
    runs_of = {bench: [r for r in script_order(bench)] for bench in record_evidence.BENCH}

    reqtool = os.environ.get("REQTOOL", str(record_evidence.REQTOOL))
    res = subprocess.run([reqtool, "check", "--json"], cwd=ROOT, capture_output=True, text=True)
    if res.returncode != 0:
        sys.exit(f"stale_runs: reqtool check failed: {res.stderr.strip()[-300:]}")
    status = json.loads(res.stdout)

    owed = {bench: set() for bench in record_evidence.BENCH}
    why = {bench: set() for bench in record_evidence.BENCH}
    flashed, host, unmapped = set(), set(), set()
    for requirement in status["requirements"]:
        for entry in requirement["entries"]:
            if entry["status"] not in NOT_CURRENT:
                continue
            procedure = entry.get("method", {}).get("procedure")
            if entry["level"] != "target":
                host.add(procedure or requirement["id"])
                continue
            if procedure not in procedures:
                unmapped.add(f"{requirement['id']} ({procedure})")
                continue
            # An entry falls stale on every bench when a file only another
            # chip compiles changed (an H7-only driver re-owes the F401's
            # entry too); rerunning a bench whose own board status is still
            # verified cannot cover that file, so only those that fell run.
            boards = entry.get("boards", [])
            if boards and all(b["status"] == "verified" for b in boards):
                continue
            bench, runs = procedures[procedure]
            if bench in record_evidence.FLASHED:
                flashed.add(procedure.split("#", 1)[1])
            else:
                owed[bench].update(runs)
                why[bench].add(requirement["id"])

    for bench, runs in owed_by_code(changed_files(base), runs_of).items():
        if runs - owed[bench]:
            why[bench].add("bench code")
        owed[bench].update(runs)

    for bench in record_evidence.BENCH:
        runs = [r for r in runs_of[bench] if r in owed[bench]]
        runs += sorted(owed[bench] - set(runs))
        if shell:
            if runs:
                print(f"hil/run-{bench}.sh {' '.join(runs)}")
            continue
        print(f"{bench}: {' '.join(runs) if runs else 'nothing owed'}"
              + (f"  ({len(runs_of[bench])} in a full pass; for {', '.join(sorted(why[bench])[:6])}"
                 + (", ..." if len(why[bench]) > 6 else "") + ")" if runs else ""))
    if shell:
        return
    if flashed:
        print("flashed benches owe (run only if what they alone reach changed): " + "; ".join(sorted(flashed)))
    if host:
        print("host procedures owe (checks/run.sh, tools/glue-gate.sh): " + "; ".join(sorted(host)))
    if unmapped:
        print("no procedure with a run command: " + "; ".join(sorted(unmapped)))


if __name__ == "__main__":
    main()
