#!/usr/bin/env python3
"""Names the design pages that own code, from docs/owners.txt.

    python3 -I tools/docs_for.py <file>...     # what to read before changing them
    python3 -I tools/docs_for.py --check       # changed code whose owning page did not change
    python3 -I tools/docs_for.py --check --none "<reason>"   # state why no page changes
    python3 -I tools/docs_for.py --map         # the map itself: every file mapped, every page exists

`--check` compares the branch and the working tree with `origin/main`. The last
matching line of the map wins. A path mapped to `none` owes no page.
"""
from __future__ import annotations

import fnmatch
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAP = "docs/owners.txt"


def rules(root: Path) -> list[tuple[str, list[str]]]:
    out = []
    for line in (root / MAP).read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            glob, *docs = line.split()
            out.append((glob, docs))
    return out


def owners(rel: str, table: list[tuple[str, list[str]]]) -> list[str] | None:
    """The pages owning `rel`; [] for `none`; None when no line matches."""
    found = None
    for glob, docs in table:
        if fnmatch.fnmatchcase(rel, glob):
            found = [d for d in docs if d != "none"]
    return found


def git(root: Path, *args: str) -> list[str]:
    out = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True, check=True)
    return [line for line in out.stdout.splitlines() if line.strip()]


def changed(root: Path, base: str) -> set[str]:
    files = set(git(root, "diff", "--name-only", f"{base}...HEAD"))
    files |= {line[3:] for line in git(root, "status", "--porcelain", "-uall")}
    return files


def map_failures(root: Path) -> list[str]:
    table = rules(root)
    fails = []
    for glob, docs in table:
        if not docs:
            fails.append(f"{MAP}: `{glob}` names no page; write `none` if it has none")
        for d in docs:
            if d != "none" and not (root / d).is_file():
                fails.append(f"{MAP}: `{glob}` names {d}, which does not exist")
            if d != "none" and not d.startswith("docs/design/"):
                fails.append(f"{MAP}: `{glob}` names {d}; an owner is a page under docs/design/")
    for f in git(root, "ls-files", "src"):
        if owners(f, table) is None:
            fails.append(f"{f} has no line in {MAP}")
    return fails


def check(root: Path, base: str, reason: str | None) -> int:
    table = rules(root)
    files = changed(root, base)
    unmapped = [f for f in sorted(files) if f.startswith("src/") and owners(f, table) is None]
    if unmapped:
        for f in unmapped:
            print(f"{f} has no line in {MAP}.")
        print("Add `<glob> <page>...` naming the design page that states its rules, or `<glob> none`.")
        return 1
    owed: dict[str, list[str]] = {}
    for f in sorted(files):
        for d in owners(f, table) or []:
            if d not in files:
                owed.setdefault(d, []).append(f)
    if not owed:
        print("docs_for: every owning page changed with its code, or none is owed.")
        return 0
    if reason:
        for d, fs in owed.items():
            print(f"Docs: none - {reason} ({d}, owner of {', '.join(fs)})")
        return 0
    for d, fs in owed.items():
        print(f"{d} owns {', '.join(fs)}, which changed; the page did not.")
    print('Update each page, or state why it does not change: --check --none "<reason>".')
    return 1


def main(argv: list[str], root: Path = ROOT, base: str = "origin/main") -> int:
    if argv[:1] == ["--map"]:
        fails = map_failures(root)
        for f in fails:
            print(f)
        return 1 if fails else 0
    if argv[:1] == ["--check"]:
        reason = None
        if argv[1:2] == ["--none"]:
            reason = " ".join(argv[2:]).strip()
            if not reason:
                print("--none needs a reason.")
                return 2
        return check(root, base, reason)
    if not argv:
        print(__doc__)
        return 2
    table = rules(root)
    for f in argv:
        p = Path(f)
        rel = str(p.resolve().relative_to(root)) if p.is_absolute() else f
        ds = owners(rel, table)
        if ds is None:
            print(f"{rel}: no line in {MAP}; add one before finishing")
        else:
            print(f"{rel}: {' '.join(ds) or 'none'}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
