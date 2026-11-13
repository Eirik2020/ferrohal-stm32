#!/usr/bin/env python3
"""Checks the size of every markdown document in FerroHAL, and that none is
an orphan.

Agents write diaries. A document that grows without bound kills an agent's
context window and hides the one fact a reader needed. The rule is: a document
has a budget, and when it needs more it is *split into a second document*, never
compressed and never left to grow (see docs/policies/documentation.md).

Budgets, in bytes (set by the owner 2026-11-13):
- always-loaded files (AGENTS.md): 20000 B - they are read the moment an agent
  enters the repository, so their budget is the strictest;
- every other document: 40000 B.

No document may be an orphan: each is linked, or named by its path, from another
document, the code or a requirement, unless it is an entry or a hub. A split's
second document is linked from the first.

Dates: documents under docs/ must be named YYYY-MM-DD-topic.md.

Root: a committed markdown file at the repository root must be README.md,
AGENTS.md or THIRD-PARTY.md; any other root .md is temporary, stays uncommitted,
and is neither budgeted nor indexed.
"""

from __future__ import annotations

import datetime
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Always-loaded files; everything else gets DEFAULT_BYTES.
ALWAYS_LOADED: dict[str, int] = {
    "AGENTS.md": 20_000,
}
DEFAULT_BYTES = 40_000

# Agent-facing documents (docs/policies/agent-docs.md style); the rest are human.
# A class of writing, not a budget: both classes share the byte limits above.
AGENT_FACING = (
    r"^AGENTS\.md$",
    r"^docs/(policies|decisions|notes|housekeeping|design|index)/",
)

# Where readers start: nothing needs to link to these.
ENTRY = {
    "AGENTS.md",
    "README.md",
    "docs/DEBT.md",
    "docs/RESOLVED.md",
    "docs/decisions/README.md",
    "docs/housekeeping/README.md",
    "docs/housekeeping/state.md",
}
HUBS = ("docs/decisions/index/",)
LINK = re.compile(r"\]\(([^)\s#]+\.md)(?:#[^)]*)?\)")

# The only markdown a commit may hold at the repository root (owner, 2026-11-13).
# Any other root `.md` is temporary - a review record, a draft - and stays
# uncommitted: no budget, no index.
ROOT_FILES = {"README.md", "AGENTS.md", "THIRD-PARTY.md"}

# Never part of a commit decision: evidence and build output.
EXEMPT = (
    r"^review/",  # the owner's reviews and drafts: uncommitted, not ours to write
    r"^requirements/evidence/",
    r"^target/",
    r"^\.git/",
)

# Written by tools/docs_index.py: budgeted like any agent document, but not counted
# as documents, since nobody adds one by hand.
GENERATED = re.compile(r"^docs/index/")

DATE_NAME = re.compile(r"^\d{4}-\d{2}-\d{2}-[a-z0-9][a-z0-9-]*\.md$")


def committed_root_docs() -> set[str] | None:
    """The root `.md` files git tracks, or None where there is no git."""
    try:
        out = subprocess.run(
            ["git", "-C", str(ROOT), "ls-files", "--", "*.md"],
            capture_output=True, text=True, check=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    return {line for line in out.splitlines() if "/" not in line}


def temporary(relative: str) -> bool:
    """An uncommitted root `.md`: a review record or a draft."""
    if "/" in relative:
        return False
    committed = committed_root_docs()
    return committed is not None and relative not in committed


def documents() -> list[Path]:
    """Every markdown document the budgets cover, in path order."""
    found = []
    for path in sorted(ROOT.rglob("*.md")):
        relative = path.relative_to(ROOT).as_posix()
        if any(re.search(pattern, relative) for pattern in EXEMPT):
            continue
        if temporary(relative):
            continue
        found.append(path)
    return found


def root_files() -> list[str]:
    """Committed root documents outside ROOT_FILES."""
    committed = committed_root_docs() or set()
    return sorted(name for name in committed - ROOT_FILES if (ROOT / name).exists())


def budget_for(relative: str) -> int:
    return ALWAYS_LOADED.get(relative, DEFAULT_BYTES)


def orphans(docs: list[Path]) -> list[str]:
    """Documents nothing links to or names by path, entries and hubs aside.

    A markdown link from another document counts, and so does the document's
    path written anywhere in another document, a requirement file or the code.
    The generated index is left out: it names every path.
    """
    corpus = []
    for path in sorted(ROOT.rglob("*")):
        if path.suffix not in (".md", ".yaml", ".rs") or not path.is_file():
            continue
        relative = path.relative_to(ROOT).as_posix()
        if relative.startswith(("target/", "requirements/evidence/", "docs/index/")):
            continue
        if "/target/" in relative:
            continue
        corpus.append((path, path.read_text(encoding="utf-8", errors="ignore")))

    inbound = {path.resolve(): 0 for path in docs}
    for path in docs:
        for target in LINK.findall(path.read_text(encoding="utf-8")):
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            resolved = (path.parent / target).resolve()
            if resolved in inbound and resolved != path.resolve():
                inbound[resolved] += 1
    for doc in docs:
        relative = doc.relative_to(ROOT).as_posix()
        for path, text in corpus:
            if path != doc and relative in text:
                inbound[doc.resolve()] += 1

    return [
        path.relative_to(ROOT).as_posix()
        for path in docs
        if not inbound[path.resolve()]
        and path.relative_to(ROOT).as_posix() not in ENTRY
        and not path.relative_to(ROOT).as_posix().startswith(HUBS)
    ]


def main() -> int:
    failures: list[str] = []
    checked = 0
    total_bytes = 0

    for path in documents():
        relative = path.relative_to(ROOT).as_posix()
        if not GENERATED.search(relative):
            checked += 1
        size = path.stat().st_size
        total_bytes += size
        cap = budget_for(relative)
        if size > cap:
            failures.append(
                f"{relative}: {size} B exceeds its {cap} B budget; "
                "split it into a second document, linked from this one"
            )

        if (
            relative.startswith(("docs/decisions/", "docs/notes/", "docs/plans/"))
            and not relative.startswith("docs/decisions/index/")
            and relative != "docs/decisions/README.md"
        ):
            stem = relative.rsplit("/", 1)[1]
            if not DATE_NAME.match(stem):
                failures.append(f"{relative}: name it YYYY-MM-DD-topic.md")

    # The root holds only a repository's expected files; documents live in docs/.
    for name in root_files():
        failures.append(
            f"{name}: a committed document at the root; move it under docs/ "
            "(root: " + ", ".join(sorted(ROOT_FILES)) + ")"
        )

    # No orphans: a split's second document is linked from the first.
    for orphan in orphans(
        [path for path in documents() if not GENERATED.search(path.relative_to(ROOT).as_posix())]
    ):
        failures.append(
            f"{orphan}: an orphan - nothing links to it or names its path; "
            "link it from the document it was split from, or from its hub"
        )

    # Housekeeping is forced: due by a date, or when ten documents were added since
    # the last review. See docs/housekeeping/README.md.
    state = ROOT / "docs/housekeeping/state.md"
    if not state.is_file():
        failures.append("docs/housekeeping/state.md is missing")
    else:
        body = state.read_text(encoding="utf-8")
        due = re.search(r"^- Next due: (\d{4}-\d{2}-\d{2})$", body, re.M)
        count = re.search(r"^- Documents at last review: (\d+)$", body, re.M)
        if due is None or count is None:
            failures.append(
                "docs/housekeeping/state.md needs 'Next due:' and "
                "'Documents at last review:'"
            )
        else:
            if datetime.date.today() > datetime.date.fromisoformat(due.group(1)):
                failures.append(
                    f"housekeeping has been due since {due.group(1)}: run "
                    "tools/housekeeping.py and record the pass in "
                    "docs/housekeeping/state.md"
                )
            added = checked - int(count.group(1))
            if added > 10:
                failures.append(
                    f"{added} documents added since the last housekeeping review "
                    "(cap 10): run the pass and record it"
                )

    # Decision-record lifecycle: ids, status lines, promotion targets, archive.
    decisions_dir = ROOT / "docs/decisions"
    records = sorted(
        path for path in decisions_dir.glob("*.md") if path.name != "README.md"
    )
    archived = sorted((decisions_dir / "archive").glob("*/*.md"))
    status = re.compile(
        r"^ADR-(\d{3}) - Status: (accepted|promoted|superseded|rejected)\b", re.M
    )
    promoted = re.compile(r"^Promoted to: .*\]\(([^)]+\.md)\)", re.M)

    ids: dict[str, Path] = {}
    states: dict[Path, tuple[str, str]] = {}
    for record in records + archived:
        relative = record.relative_to(ROOT)
        body = record.read_text(encoding="utf-8")
        found = status.search(body)
        if not found:
            failures.append(f"{relative}: no 'ADR-NNN - Status: ...' line")
            continue
        ident, state = found.group(1), found.group(2)
        if ident in ids:
            failures.append(
                f"{relative}: ADR-{ident} is already used by "
                f"{ids[ident].relative_to(ROOT)}"
            )
        ids[ident] = record
        states[record] = (ident, state)

        if state == "promoted":
            target = promoted.search(body)
            if target is None:
                failures.append(
                    f"{relative}: a promoted record needs "
                    "'Promoted to: [path](path)'"
                )
            elif not (record.parent / target.group(1)).resolve().exists():
                failures.append(
                    f"{relative}: 'Promoted to' target "
                    f"{target.group(1)} does not resolve"
                )
        if record.parent == decisions_dir and state in ("superseded", "rejected"):
            failures.append(f"{relative}: {state} records move to archive/<year>/")
        if record.parent != decisions_dir and state in ("accepted", "promoted"):
            failures.append(
                f"{relative}: archived records are superseded or rejected, "
                f"not {state}"
            )

    # Index rows: the id, status and target must agree with the record, both ways.
    indexes = sorted((decisions_dir / "index").glob("*.md"))
    by_path = {path.resolve(): info for path, info in states.items()}
    listed: set[Path] = set()
    row_link = re.compile(r"\]\(([^)]+\.md)\)")
    for path in indexes:
        relative = path.relative_to(ROOT)
        for line in path.read_text(encoding="utf-8").splitlines():
            if not line.startswith("| ADR-"):
                continue
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            if len(cells) < 5:
                failures.append(f"{relative}: malformed row: {line[:40]}")
                continue
            ident = cells[0]
            state_cell = cells[3].strip("`* ")
            target = row_link.search(cells[-1])
            resolved = (path.parent / target.group(1)).resolve() if target else None
            info = by_path.get(resolved) if resolved else None
            if info is None:
                failures.append(f"{relative}: row {ident} points at no record")
                continue
            if ident != f"ADR-{info[0]}":
                failures.append(f"{relative}: row {ident} points at ADR-{info[0]}")
            if state_cell != info[1]:
                failures.append(
                    f"{relative}: row {ident} says '{state_cell}', "
                    f"the record says '{info[1]}'"
                )
            listed.add(resolved)

    for path in states:
        if path.resolve() not in listed:
            failures.append(
                f"{path.relative_to(ROOT)}: not listed in any decision index"
            )

    active_count = sum(
        1 for path, (_, state) in states.items()
        if path.parent == decisions_dir and state in ("accepted", "promoted")
    )
    if active_count > 20:
        failures.append(
            f"docs/decisions: {active_count} active records, cap is 20; "
            "promote what binds and archive the rest"
        )

    # Design topic documents name the record(s) they distil.
    design_dir = ROOT / "docs/design"
    for path in sorted(design_dir.glob("*.md")) if design_dir.is_dir() else []:
        if path.name == "README.md":
            continue
        body = path.read_text(encoding="utf-8")
        source = re.search(r"^Source: (.+)$", body, re.M)
        cited = re.findall(r"ADR-(\d{3})", source.group(1)) if source else []
        if not cited:
            failures.append(f"{path.relative_to(ROOT)}: no 'Source: ADR-NNN' line")
        for ident in cited:
            if ident not in ids:
                failures.append(f"{path.relative_to(ROOT)}: cites unknown ADR-{ident}")

    # Entry documents point at the hub, never at individual records.
    agents = ROOT / "AGENTS.md"
    if agents.is_file():
        body = agents.read_text(encoding="utf-8")
        if re.search(r"docs/decisions/\d{4}-\d{2}-\d{2}-", body):
            failures.append(
                "AGENTS.md: point at docs/decisions/README.md, not at a record"
            )

    # Every relative markdown link in docs/ and AGENTS.md must resolve.
    linked = LINK
    paths = sorted((ROOT / "docs").rglob("*.md"))
    if agents.is_file():
        paths.append(agents)
    for path in paths:
        for target in linked.findall(path.read_text(encoding="utf-8")):
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            if not (path.parent / target).resolve().exists():
                failures.append(
                    f"{path.relative_to(ROOT)}: link {target} does not resolve"
                )

    if failures:
        if "--json" in sys.argv:
            print(
                json.dumps(
                    {
                        "ok": False,
                        "documents": checked,
                        "decisions": len(records),
                        "active": active_count,
                        "bytes": total_bytes,
                        "failures": failures,
                    }
                )
            )
        for failure in failures:
            print(f"FAIL  {failure}")
        return 1

    if "--json" in sys.argv:
        print(
            json.dumps(
                {
                    "ok": True,
                    "documents": checked,
                    "decisions": len(records),
                    "active": active_count,
                    "bytes": total_bytes,
                    "failures": [],
                }
            )
        )
        return 0

    print(
        f"pass  doc size limits ({checked} documents, {len(records)} decisions, "
        f"{active_count} active, {total_bytes} B, no orphans)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
