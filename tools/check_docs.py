#!/usr/bin/env python3
"""Checks the size of every markdown document in FerroHAL.

Agents write diaries. A document that grows without bound kills an agent's
context window and hides the one fact a reader needed. The rule is: a document
has a budget, and when it needs more it is *split into a second document*, never
compressed and never left to grow (see docs/policies/documentation.md).

Budgets, in bytes and lines:
- always-loaded entry files (AGENTS.md): 6000 B / 120 lines - they are read the
  moment an agent enters the repository, so their budget is the strictest;
- one decision per file (docs/decisions): 4096 B / 100 lines;
- one topic per file (docs/notes, docs/plans, docs/policies): 6000 B / 150 lines;
- procedures (requirements/benches.md): 12000 B / 300 lines;
- topic design documents (docs/design): 8000 B / 200 lines;
- other committed documents: 8000 B / 200 lines.

The registries (DEBT.md, RESOLVED.md) are ratcheted at their present size and
may only shrink. The architecture map dissolved into docs/design/ topics,
notes and ADR-005 on 2026-11-10; its chapters are budgeted like any other
document.

Dates: documents under docs/ must be named YYYY-MM-DD-topic.md.
"""

from __future__ import annotations

import datetime
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (pattern, name, max_bytes, max_lines); first match wins, so specific first.
BUDGETS: list[tuple[str, str, int, int]] = [
    (r"^AGENTS\.md$", "entry file", 6_000, 120),
    (r"^docs/design/[^/]+\.md$", "design chapter", 8_000, 200),
    (r"^docs/housekeeping/[^/]+\.md$", "housekeeping", 4_096, 100),
    (r"^docs/decisions/README\.md$", "decision hub", 4_096, 100),
    (r"^docs/decisions/index/[^/]+\.md$", "decision index", 8_192, 200),
    (r"^docs/decisions/[^/]+\.md$", "decision", 4_096, 100),
    (r"^docs/(notes|plans|policies)/[^/]+\.md$", "topic", 6_000, 150),
    (r"^requirements/benches\.md$", "procedures", 12_000, 300),
    (r"^(README|THIRD-PARTY|hil/(WIRING|CAPABILITIES|RIG))\.md$", "document", 8_000, 200),
    (r"^src/([^/]+/)?requirements\.yaml$", "catalog", 0, 0),  # not markdown
]

# Agent-facing documents are read by agents; they are measured in estimated tokens
# (4 bytes/token) rather than bytes and lines. Human-facing documents below keep byte
# and line budgets.
TOKENS: list[tuple[str, str, int]] = [
    (r"^AGENTS\.md$", "agent entry", 1_100),
    (r"^docs/INDEX\.md$", "docs index", 1_200),
    (r"^docs/policies/[^/]+\.md$", "agent policy", 800),
    (r"^docs/decisions/README\.md$", "decision hub", 500),
    (r"^docs/decisions/index/[^/]+\.md$", "decision index", 400),
    (r"^docs/decisions/[^/]+\.md$", "decision", 750),
    (r"^docs/design/[^/]+\.md$", "design topic", 750),
    (r"^docs/notes/[^/]+\.md$", "agent note", 850),
    (r"^docs/housekeeping/[^/]+\.md$", "agent housekeeping", 500),
]

# Ratcheted: present size, may only shrink until the planned split.
RATCHET: dict[str, int] = {
    "DEBT.md": 23_243,
    "RESOLVED.md": 38_015,
}

# Never part of a commit decision: drafts and evidence.
EXEMPT = (
    r"^ferrosmith-board-schema\.md$",  # untracked draft
    # Review records: kept for the owner to cross-check a review against its
    # commits, then deleted; not part of the documentation set or its index.
    r"^REVIEW-\d{4}-\d{2}-\d{2}(-[a-z0-9-]+)?\.md$",
    r"^requirements/evidence/",
    r"^target/",
    r"^\.git/",
)

DATE_NAME = re.compile(r"^\d{4}-\d{2}-\d{2}-[a-z0-9][a-z0-9-]*\.md$")


def documents() -> list[Path]:
    """Every markdown document the budgets cover, in path order."""
    found = []
    for path in sorted(ROOT.rglob("*.md")):
        relative = path.relative_to(ROOT).as_posix()
        if any(re.search(pattern, relative) for pattern in EXEMPT):
            continue
        found.append(path)
    return found


def budget_for(relative: str) -> tuple[str, int, int] | None:
    for pattern, name, max_bytes, max_lines in BUDGETS:
        if re.search(pattern, relative):
            return name, max_bytes, max_lines
    if relative.endswith(".md"):
        return "document", 8_000, 200
    return None


def main() -> int:
    failures: list[str] = []
    checked = 0
    agent_tokens = 0

    for path in documents():
        relative = path.relative_to(ROOT).as_posix()

        budget = budget_for(relative)
        if budget is None:
            continue
        name, max_bytes, max_lines = budget
        checked += 1

        size = path.stat().st_size
        lines = len(path.read_text(encoding="utf-8").splitlines())

        if relative in RATCHET:
            cap = RATCHET[relative]
            if size > cap:
                failures.append(
                    f"{relative}: {size} B exceeds its ratchet of {cap} B; "
                    "split it, or shrink it by the same amount"
                )
            elif size < cap:
                failures.append(
                    f"{relative}: {size} B is below its ratchet of {cap} B; "
                    "lower the ratchet in this change so it cannot grow back"
                )
            continue

        agent = next(
            (
                (label, cap)
                for pattern, label, cap in TOKENS
                if re.search(pattern, relative)
            ),
            None,
        )
        if agent is not None:
            tokens = (size + 3) // 4
            agent_tokens += tokens
            if tokens > agent[1]:
                failures.append(
                    f"{relative}: ~{tokens} estimated tokens exceeds the "
                    f"{agent[0]} cap of {agent[1]}; rewrite terser or split"
                )
        elif size > max_bytes or lines > max_lines:
            failures.append(
                f"{relative}: {size} B / {lines} lines exceeds "
                f"{name} budget {max_bytes} B / {max_lines} lines; "
                "split it into a second document"
            )

        if (
            relative.startswith(("docs/decisions/", "docs/notes/", "docs/plans/"))
            and not relative.startswith("docs/decisions/index/")
            and relative != "docs/decisions/README.md"
        ):
            stem = relative.rsplit("/", 1)[1]
            if not DATE_NAME.match(stem):
                failures.append(f"{relative}: name it YYYY-MM-DD-topic.md")

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
    linked = re.compile(r"\]\(([^)\s#]+\.md)(?:#[^)]*)?\)")
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
                        "agent_tokens": agent_tokens,
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
                    "agent_tokens": agent_tokens,
                    "failures": [],
                }
            )
        )
        return 0

    print(
        f"pass  doc size limits ({checked} documents, {len(records)} decisions, "
        f"{active_count} active, ~{agent_tokens} agent tokens)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
