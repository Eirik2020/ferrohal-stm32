#!/usr/bin/env python3
"""Decision records: their format, their tags, and a search by tag.

    python3 -I tools/adr.py find <tag> [<tag>...]   # the records carrying every tag
    python3 -I tools/adr.py tags                    # the approved tags, with counts

Format (docs/decisions/AGENTS.md, rules 2-4), for every active record, a
docs/decisions/*.md other than README.md and the agent entry files:
- at most 3000 bytes;
- one ISO date (YYYY-MM-DD): the date its name opens with, carried by the `# `
  title. Any other date is a breach, except inside a link target or a
  backticked path that names an existing file (a dated note, an archived
  record): that date is the file's name, not a statement in the record;
- its `##` sections are Context, Decision, Rejected and Consequences, all four, in
  that order, and no others. The `ADR-NNN - Status:` and `Promoted to:` lines are
  lines, not sections; tools/check_docs.py requires them.

Tags: every row of docs/decisions/index/<year>.md has a Tags column,
comma-separated, each tag from docs/decisions/tags.txt. An active record's row
carries at least one; an archived record's row may carry none.

The search reads the index files and tags.txt only: never a record, and never
anything under an archive directory. An archived record's row is listed with the
status the row gives, after the active ones.

tools/check_docs.py calls failures() and prints each as a FAIL line.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

DECISIONS = "docs/decisions"
TAGS = "tags.txt"
ENTRY_NAMES = {"README.md", "AGENTS.md", "CLAUDE.md"}

MAX_BYTES = 3000
SECTIONS = ("Context", "Decision", "Rejected", "Consequences")
ACTIVE = ("accepted", "promoted")

DATE = re.compile(r"(?<![0-9])\d{4}-\d{2}-\d{2}(?![0-9])")
TAG = re.compile(r"^[a-z0-9][a-z0-9-]*$")
LINK = re.compile(r"\[(.+?)\]\(([^)\s]+\.md)\)")
FENCE = re.compile(r"^\s*(```|~~~)")
# A link target, or a backticked token: the two places a path is written.
PATH_TOKEN = re.compile(r"\]\(([^)\s]+)\)|`([^`\s]+)`")


# --- format ---------------------------------------------------------------


def records(root: Path) -> list[Path]:
    """The active records: docs/decisions/*.md, entries aside. No subfolder."""
    folder = root / DECISIONS
    return sorted(p for p in folder.glob("*.md") if p.name not in ENTRY_NAMES)


def sections(text: str) -> list[str]:
    """The `##` headings, outside code fences, as written."""
    found, fenced = [], False
    for line in text.splitlines():
        if FENCE.match(line):
            fenced = not fenced
            continue
        if not fenced and line.startswith("## "):
            found.append(line[3:].strip())
    return found


def without_paths(path: Path, text: str, root: Path) -> str:
    """The text with every link target and backticked path to an existing file
    removed, so a date that is part of a file's name is not counted."""

    def drop(match: re.Match) -> str:
        target = (match.group(1) or match.group(2)).split("#")[0]
        if "/" not in target and "." not in target:
            return match.group(0)
        for base in (path.parent, root):
            try:
                if (base / target).exists():
                    return ""
            except OSError:
                pass
        return match.group(0)

    return PATH_TOKEN.sub(drop, text)


def breaches(path: Path, root: Path = ROOT) -> dict[str, str]:
    """The format rules a record breaks: {"size"|"dates"|"sections": why}."""
    raw = path.read_bytes()
    text = raw.decode("utf-8")
    broken: dict[str, str] = {}

    if len(raw) > MAX_BYTES:
        broken["size"] = f"{len(raw)} B, over the {MAX_BYTES} B cap"

    named = DATE.match(path.name)
    dates = sorted(set(DATE.findall(without_paths(path, text, root))))
    title = next((line for line in text.splitlines() if line.startswith("# ")), "")
    if named is None:
        broken["dates"] = "the file name opens with no YYYY-MM-DD date"
    else:
        others = [d for d in dates if d != named.group(0)]
        why = []
        if others:
            why.append(f"a second date: {', '.join(others)}")
        if named.group(0) not in title:
            why.append(f"the title does not carry {named.group(0)}")
        if why:
            broken["dates"] = "; ".join(why)

    found = sections(text)
    if tuple(found) != SECTIONS:
        broken["sections"] = (
            f"## sections are {', '.join(found) or 'none'}; "
            f"want {', '.join(SECTIONS)}"
        )
    return broken


def read_list(path: Path) -> list[str]:
    """A list file's entries: one per line; blank lines and `#` comments skipped."""
    entries = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            entries.append(line)
    return entries


def format_failures(root: Path) -> list[str]:
    failures: list[str] = []
    for path in records(root):
        relative = f"{DECISIONS}/{path.name}"
        for rule, why in breaches(path, root).items():
            failures.append(f"{relative}: {rule}: {why} (docs/decisions/AGENTS.md)")
    return failures


# --- index and tags -------------------------------------------------------


@dataclass(frozen=True)
class Row:
    ident: str
    date: str
    status: str
    tags: tuple[str, ...]
    title: str
    path: str  # relative to the repository root, from the row's link alone
    index: str  # the index file the row is in, relative to the root


def index_files(root: Path) -> list[Path]:
    return sorted((root / DECISIONS / "index").glob("*.md"))


def cells(line: str) -> list[str]:
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def rows(root: Path) -> tuple[list[Row], list[str]]:
    """Every index row, and the problems met reading them. Opens no record."""
    found: list[Row] = []
    problems: list[str] = []
    for index in index_files(root):
        relative = index.relative_to(root).as_posix()
        header: list[str] | None = None
        for line in index.read_text(encoding="utf-8").splitlines():
            if line.startswith("| Id |"):
                header = [cell.lower() for cell in cells(line)]
                if "tags" not in header:
                    problems.append(f"{relative}: the table has no Tags column")
                continue
            if not line.startswith("| ADR-") or header is None:
                continue
            row = dict(zip(header, cells(line)))
            link = LINK.search(row.get("record", ""))
            if link is None:
                problems.append(f"{relative}: row {row.get('id')} has no record link")
                continue
            target = os.path.normpath(os.path.join(os.path.dirname(relative), link.group(2)))
            tags = tuple(t.strip() for t in row.get("tags", "").split(",") if t.strip())
            found.append(
                Row(
                    ident=row.get("id", ""),
                    date=row.get("date", ""),
                    status=row.get("status", "").strip("`* "),
                    tags=tags,
                    title=link.group(1),
                    path=Path(target).as_posix(),
                    index=relative,
                )
            )
    return found, problems


def approved(root: Path) -> tuple[list[str], list[str]]:
    """The approved tags, and the problems with the list itself."""
    path = root / DECISIONS / TAGS
    relative = f"{DECISIONS}/{TAGS}"
    if not path.is_file():
        return [], [f"{relative} is missing"]
    tags = read_list(path)
    problems = [f"{relative}: '{t}' is not a tag (a-z, 0-9, -)" for t in tags if not TAG.match(t)]
    problems += [f"{relative}: '{t}' is listed twice" for t in sorted({t for t in tags if tags.count(t) > 1})]
    return tags, problems


def tag_failures(root: Path) -> list[str]:
    tags, failures = approved(root)
    found, problems = rows(root)
    failures += problems
    allowed = set(tags)
    for row in found:
        where = f"{row.index}: row {row.ident}"
        if not row.tags and row.status in ACTIVE:
            failures.append(f"{where} has no tag; give it one from {DECISIONS}/{TAGS}")
        for tag in row.tags:
            if tag not in allowed:
                failures.append(
                    f"{where}: tag '{tag}' is not in {DECISIONS}/{TAGS}; "
                    "only the owner adds a tag"
                )
        for tag in sorted({t for t in row.tags if row.tags.count(t) > 1}):
            failures.append(f"{where}: tag '{tag}' is given twice")
    return failures


def failures(root: Path = ROOT) -> list[str]:
    """Every format and tag failure, for tools/check_docs.py."""
    return format_failures(root) + tag_failures(root)


# --- command line ---------------------------------------------------------


def find(root: Path, wanted: list[str]) -> list[Row]:
    found, _ = rows(root)
    matches = [row for row in found if set(wanted) <= set(row.tags)]
    return sorted(matches, key=lambda r: (r.status not in ACTIVE, r.ident))


def main(argv: list[str] | None = None, root: Path = ROOT) -> int:
    parser = argparse.ArgumentParser(
        prog="adr.py", description="Search the decision records by tag."
    )
    sub = parser.add_subparsers(dest="command", required=True)
    find_cmd = sub.add_parser("find", help="the records carrying every given tag")
    find_cmd.add_argument("tag", nargs="+")
    sub.add_parser("tags", help="the approved tags, with counts")
    args = parser.parse_args(argv)

    tags, _ = approved(root)
    if args.command == "tags":
        found, _ = rows(root)
        width = max((len(t) for t in tags), default=0)
        print(f"{'tag':<{width}}  active  archived")
        for tag in tags:
            active = sum(1 for r in found if tag in r.tags and r.status in ACTIVE)
            other = sum(1 for r in found if tag in r.tags and r.status not in ACTIVE)
            print(f"{tag:<{width}}  {active:>6}  {other:>8}")
        return 0

    wanted = [t.lower() for t in args.tag]
    unknown = [t for t in wanted if t not in tags]
    if unknown:
        print(
            f"unknown tag: {', '.join(unknown)}\nvalid tags: {', '.join(tags)}",
            file=sys.stderr,
        )
        return 2
    matches = find(root, wanted)
    if not matches:
        print(f"no record carries: {', '.join(wanted)}")
        return 1
    for row in matches:
        print(f"{row.ident}  {row.date}  {row.status:<10}  {row.title}  {row.path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
