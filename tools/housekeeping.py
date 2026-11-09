#!/usr/bin/env python3
"""Housekeeping report: text, or `--json` for tools and a future MCP server.

Collects the review data once, then renders either form. The text form forces traversal;
the JSON form is for a tool to consume without parsing prose. Unresolved judgements are
listed as questions for the owner - the report never decides them.
"""

from __future__ import annotations

import datetime
import difflib
import json
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_docs  # noqa: E402

ROOT = check_docs.ROOT
LINK = re.compile(r"\]\(([^)\s#]+\.md)(?:#[^)]*)?\)")
TITLE = re.compile(r"^# (.+)$", re.M)
STATUS = re.compile(
    r"^ADR-(\d{3}) - Status: (accepted|promoted|superseded|rejected)\b", re.M
)
STAMP = re.compile(r"^- (Last review|Next due): (\d{4}-\d{2}-\d{2})$", re.M)

ENTRY = {
    "AGENTS.md",
    "README.md",
    "DEBT.md",
    "RESOLVED.md",
    "docs/INDEX.md",
    "docs/decisions/README.md",
    "docs/housekeeping/README.md",
    "docs/housekeeping/state.md",
}
HUBS = ("docs/decisions/index/",)


def label(relative: str) -> str:
    if relative in check_docs.RATCHET:
        return "ratcheted"
    for pattern, name, _ in check_docs.TOKENS:
        if re.search(pattern, relative):
            return name
    budget = check_docs.budget_for(relative)
    return budget[0] if budget else "document"


def age(path: Path) -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(ROOT), "log", "-1", "--format=%cs", "--", str(path)],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return "unknown"
    return result.stdout.strip() or "unknown"


def collect() -> dict:
    docs = check_docs.documents()
    corpus = []
    for path in sorted(ROOT.rglob("*")):
        if path.suffix not in (".md", ".yaml", ".rs") or not path.is_file():
            continue
        relative = path.relative_to(ROOT).as_posix()
        if relative.startswith(("target/", "requirements/evidence/")):
            continue
        corpus.append((path, path.read_text(encoding="utf-8", errors="ignore")))

    inbound = {path.resolve(): 0 for path in docs}
    for path in docs:
        for target in LINK.findall(path.read_text(encoding="utf-8")):
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            resolved = (path.parent / target).resolve()
            if resolved in inbound:
                inbound[resolved] += 1
    for doc in docs:
        relative = doc.relative_to(ROOT).as_posix()
        for path, text in corpus:
            if path != doc and relative in text:
                inbound[doc.resolve()] += 1

    entries = []
    for path in docs:
        relative = path.relative_to(ROOT).as_posix()
        size = path.stat().st_size
        entries.append(
            {
                "path": relative,
                "label": label(relative),
                "bytes": size,
                "tokens": (size + 3) // 4,
            }
        )

    orphans = [
        path.relative_to(ROOT).as_posix()
        for path in docs
        if not inbound[path.resolve()]
        and path.relative_to(ROOT).as_posix() not in ENTRY
        and not path.relative_to(ROOT).as_posix().startswith(HUBS)
    ]

    titles = {}
    for path in docs:
        found = TITLE.search(path.read_text(encoding="utf-8"))
        titles[path] = found.group(1).strip().lower() if found else path.stem
    duplicates = []
    for index, first in enumerate(docs):
        for second in docs[index + 1:]:
            names = (
                first.relative_to(ROOT).as_posix(),
                second.relative_to(ROOT).as_posix(),
            )
            if names[0] in ENTRY or names[1] in ENTRY:
                continue
            ratio = difflib.SequenceMatcher(None, titles[first], titles[second]).ratio()
            if ratio > 0.75:
                duplicates.append(
                    {"a": names[0], "b": names[1], "ratio": round(ratio, 2)}
                )

    counts = {"accepted": 0, "promoted": 0, "superseded": 0, "rejected": 0}
    unpromoted = []
    decisions = sorted((ROOT / "docs/decisions").glob("*.md"))
    archived = sorted((ROOT / "docs/decisions/archive").glob("*/*.md"))
    for path in decisions + archived:
        found = STATUS.search(path.read_text(encoding="utf-8"))
        if not found:
            continue
        counts[found.group(2)] += 1
        if found.group(2) == "accepted" and path in decisions:
            unpromoted.append(
                {
                    "adr": f"ADR-{found.group(1)}",
                    "path": path.relative_to(ROOT).as_posix(),
                    "since": age(path),
                }
            )

    today = datetime.date.today()
    stale = []
    for path in docs:
        relative = path.relative_to(ROOT).as_posix()
        if not relative.startswith(("docs/plans/", "docs/notes/")):
            continue
        stamp = age(path)
        try:
            days = (today - datetime.date.fromisoformat(stamp)).days
        except ValueError:
            continue
        if days > 90:
            stale.append({"path": relative, "since": stamp, "days": days})

    state = ROOT / "docs/housekeeping/state.md"
    stamp = {}
    review_count = None
    if state.is_file():
        body = state.read_text(encoding="utf-8")
        stamp = dict(STAMP.findall(body))
        found = re.search(r"^- Documents at last review: (\d+)$", body, re.M)
        review_count = int(found.group(1)) if found else None

    questions = [f"{path}: link, merge or delete?" for path in orphans]
    questions += [
        f"{item['adr']} {item['path']}: promote (policy, design or requirement) or archive?"
        for item in unpromoted
    ]
    questions += [
        f"{item['path']} untouched {item['days']} days: merge, archive or refresh?"
        for item in stale
    ]

    return {
        "docs_schema": 1,
        "date": today.isoformat(),
        "state": {
            "last_review": stamp.get("Last review"),
            "next_due": stamp.get("Next due"),
            "documents_at_last_review": review_count,
            "documents_now": len(docs),
        },
        "documents": entries,
        "orphans": orphans,
        "duplicates": duplicates,
        "decisions": {"counts": counts, "unpromoted": unpromoted, "active_cap": 20},
        "stale": stale,
        "ratchets": [
            {
                "path": relative,
                "bytes": (ROOT / relative).stat().st_size
                if (ROOT / relative).is_file()
                else 0,
                "cap": cap,
            }
            for relative, cap in check_docs.RATCHET.items()
        ],
        "questions": questions,
    }


def render(data: dict) -> None:
    state = data["state"]
    print(f"housekeeping report, {data['date']}")
    print(
        f"  last review {state['last_review']}, next due {state['next_due']}: "
        f"{state['documents_now']} documents now, "
        f"{state['documents_at_last_review']} at the last review"
    )

    print("\ndocuments by category")
    for doc in data["documents"]:
        print(f"  {doc['bytes']:>6} B  {doc['label']:<16} {doc['path']}")

    print("\norphans (nothing links here; entries and hubs excluded)")
    for path in data["orphans"] or ["none"]:
        print(f"  {path}")

    print("\nduplicate-title hints (similarity over 0.75; read both before merging)")
    for pair in data["duplicates"]:
        print(f"  {pair['ratio']:.2f}  {pair['a']}  <->  {pair['b']}")
    if not data["duplicates"]:
        print("  none")

    counts = data["decisions"]["counts"]
    print("\ndecision records")
    print(
        f"  accepted {counts['accepted']}, promoted {counts['promoted']}, "
        f"superseded {counts['superseded']}, rejected {counts['rejected']} "
        f"(active cap {data['decisions']['active_cap']})"
    )
    for item in data["decisions"]["unpromoted"]:
        print(
            f"  accepted, not promoted: {item['path']} (since {item['since']}) - "
            "still binds (promote) or archive"
        )

    print("\nplans and notes older than 90 days")
    for item in data["stale"]:
        print(f"  {item['path']}: unchanged since {item['since']} ({item['days']} days)")
    if not data["stale"]:
        print("  none")

    print("\nratchets")
    for item in data["ratchets"]:
        print(f"  {item['bytes']:>6} B of {item['cap']} B  {item['path']}")

    print("\nquestions for the owner")
    for question in data["questions"] or ["none"]:
        print(f"  - {question}")

    print("\nnext: work the list, record the pass in docs/housekeeping/state.md")


def main() -> int:
    data = collect()
    if "--json" in sys.argv:
        print(json.dumps(data))
    else:
        render(data)
    return 0


if __name__ == "__main__":
    sys.exit(main())
