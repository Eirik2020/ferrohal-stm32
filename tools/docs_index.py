#!/usr/bin/env python3
"""Generates docs/index.json and docs/INDEX.md: the machine-readable document list.

Agents should find a document from metadata, not by scanning the tree. This is that
list: path, class (agent or human), title, one-line purpose, size. `--check` fails when
the list is stale, so it cannot drift from the documents - the same contract as a lock
file. `--json` prints it without writing, for tools and a future MCP server.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_docs  # noqa: E402  (budgets, token caps and exemptions in one place)

ROOT = check_docs.ROOT
JSON_PATH = ROOT / "docs/index.json"
MD_PATH = ROOT / "docs/INDEX.md"
TITLE = re.compile(r"^# (.+)$", re.M)
STATUS = re.compile(r"^ADR-(\d{3}) - Status: (\w+)", re.M)
MARKUP = re.compile(r"[`*_\[\]()]|^[-|>#]+ ")


def purpose(path: Path) -> str:
    lines = path.read_text(encoding="utf-8").splitlines()
    seen_title = False
    for line in lines:
        text = line.strip()
        if not seen_title:
            seen_title = text.startswith("# ")
            continue
        if not text or text.startswith(("#", "|", "-", ">", "```")):
            continue
        text = MARKUP.sub("", text).strip()
        if text:
            return text[:50]
    return ""


def describe() -> dict:
    documents = []
    for path in check_docs.documents():
        relative = path.relative_to(ROOT).as_posix()
        if relative == "docs/INDEX.md":
            # The generated index excludes itself: listing it would make every write
            # change the file it is compared against.
            continue
        agent = any(
            re.search(pattern, relative) for pattern, _, _ in check_docs.TOKENS
        )
        title = TITLE.search(path.read_text(encoding="utf-8"))
        size = path.stat().st_size
        entry = {
            "path": relative,
            "class": "agent" if agent else "human",
            "title": title.group(1).strip() if title else path.stem,
            "purpose": purpose(path),
            "bytes": size,
            "tokens": (size + 3) // 4,
        }
        status = STATUS.search(path.read_text(encoding="utf-8"))
        if status:
            entry["adr"] = f"ADR-{status.group(1)}"
            entry["status"] = status.group(2)
        documents.append(entry)

    return {
        "docs_schema": 1,
        "counts": {
            "documents": len(documents),
            "agent": sum(1 for doc in documents if doc["class"] == "agent"),
            "agent_tokens": sum(
                doc["tokens"] for doc in documents if doc["class"] == "agent"
            ),
        },
        "documents": documents,
    }


def markdown(data: dict) -> str:
    lines = [
        "# Document index (generated)",
        "",
        "Metadata only - read a document after choosing it, not before. "
        f"{data['counts']['documents']} documents, "
        f"~{data['counts']['agent_tokens']} agent tokens. "
        "Regenerate: `python3 -I tools/docs_index.py`; `--check` fails when stale.",
        "",
        "| class | path | size | purpose |",
        "|---|---|---|---|",
    ]
    ordered = sorted(data["documents"], key=lambda doc: (doc["class"], doc["path"]))
    for doc in ordered:
        size = f"{doc['tokens']} tok" if doc["class"] == "agent" else f"{doc['bytes']} B"
        lines.append(f"| {doc['class'][0]} | {doc['path']} | {size} | {doc['purpose']} |")
    return "\n".join(lines) + "\n"


def main() -> int:
    data = describe()
    body = markdown(data)

    if "--json" in sys.argv:
        print(json.dumps(data))
        return 0

    if "--check" in sys.argv:
        if not JSON_PATH.is_file() or not MD_PATH.is_file():
            print("stale  docs/index.json or docs/INDEX.md is missing; run tools/docs_index.py")
            return 1
        if json.loads(JSON_PATH.read_text(encoding="utf-8")) != data:
            print("stale  docs/index.json differs; run python3 -I tools/docs_index.py")
            return 1
        if MD_PATH.read_text(encoding="utf-8") != body:
            print("stale  docs/INDEX.md differs; run python3 -I tools/docs_index.py")
            return 1
        print(f"pass  docs index current ({data['counts']['documents']} documents)")
        return 0

    JSON_PATH.write_text(json.dumps(data, indent=1) + "\n", encoding="utf-8")
    MD_PATH.write_text(body, encoding="utf-8")
    print(
        f"wrote docs/index.json and docs/INDEX.md "
        f"({data['counts']['documents']} documents, "
        f"~{data['counts']['agent_tokens']} agent tokens)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
