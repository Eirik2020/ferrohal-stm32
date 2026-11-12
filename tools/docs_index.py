#!/usr/bin/env python3
"""Generates docs/index/: the machine-readable document list, one file per area.

Agents should find a document from metadata, not by scanning the tree. This is that
list: path, class (agent or human), title, one-line purpose, size. `--check` fails when
the list is stale, so it cannot drift from the documents - the same contract as a lock
file. `--json` prints it without writing, for tools and a future MCP server.

The list is split so that no index file outgrows its cap: `README.md` is the hub, each
area (a directory under docs/, a top-level directory, or `root` for top-level files) has
its own `<area>.md`, and `index.json` holds everything for tools. All of it sits in
docs/index/, which holds nothing else: a file there that this tool did not write is
stale.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_docs  # noqa: E402  (budgets, token caps and exemptions in one place)

ROOT = check_docs.ROOT
INDEX_DIR = ROOT / "docs/index"
JSON_PATH = INDEX_DIR / "index.json"
HUB = "README.md"
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
            return text[:45]
    return ""


def describe() -> dict:
    documents = []
    for path in check_docs.documents():
        relative = path.relative_to(ROOT).as_posix()
        if check_docs.GENERATED.search(relative):
            # The generated index excludes itself: listing it would make every write
            # change the files it is compared against.
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


def area(relative: str) -> str:
    """The index file a document is listed in."""
    parts = relative.split("/")
    if len(parts) == 1:
        return "root"
    if parts[0] == "docs" and len(parts) > 2:
        return parts[1]
    return parts[0]


def covers(name: str) -> str:
    if name == "root":
        return "top-level files"
    if (ROOT / "docs" / name).is_dir():
        return f"docs/{name}/"
    return f"{name}/"


def row(doc: dict) -> str:
    size = f"{doc['tokens']} tok" if doc["class"] == "agent" else f"{doc['bytes']} B"
    return f"| {doc['class'][0]} | {doc['path']} | {size} | {doc['purpose']} |"


def pages(data: dict) -> dict[str, str]:
    """Every markdown file of the index, by its name in docs/index/."""
    areas: dict[str, list[dict]] = {}
    for doc in sorted(data["documents"], key=lambda doc: (doc["class"], doc["path"])):
        areas.setdefault(area(doc["path"]), []).append(doc)

    hub = [
        "# Document index (generated)",
        "",
        "Metadata only - read after choosing. "
        f"{data['counts']['documents']} documents, "
        f"~{data['counts']['agent_tokens']} agent tokens. "
        "One file per area, in this directory; `index.json` for tools. "
        "Regenerate: `python3 -I tools/docs_index.py`; `--check` fails when stale.",
        "",
        "| area | file | documents | covers |",
        "|---|---|---|---|",
    ]
    result = {}
    for name in sorted(areas):
        docs = areas[name]
        hub.append(f"| {name} | [{name}.md]({name}.md) | {len(docs)} | {covers(name)} |")
        result[f"{name}.md"] = "\n".join(
            [
                f"# Document index: {name} (generated)",
                "",
                f"{len(docs)} documents in {covers(name)}. Hub: [README.md](README.md).",
                "",
                "| class | path | size | purpose |",
                "|---|---|---|---|",
                *(row(doc) for doc in docs),
            ]
        ) + "\n"
    result[HUB] = "\n".join(hub) + "\n"
    return result


def main() -> int:
    data = describe()
    wanted = pages(data)

    if "--json" in sys.argv:
        print(json.dumps(data))
        return 0

    present = (
        {path.name for path in INDEX_DIR.glob("*.md")} if INDEX_DIR.is_dir() else set()
    )

    if "--check" in sys.argv:
        hint = "run python3 -I tools/docs_index.py"
        if not JSON_PATH.is_file():
            print(f"stale  docs/index/index.json is missing; {hint}")
            return 1
        if json.loads(JSON_PATH.read_text(encoding="utf-8")) != data:
            print(f"stale  docs/index/index.json differs; {hint}")
            return 1
        for name in sorted(wanted.keys() | present):
            path = INDEX_DIR / name
            if name not in wanted:
                print(f"stale  docs/index/{name} is not part of the index; {hint}")
                return 1
            if name not in present or path.read_text(encoding="utf-8") != wanted[name]:
                print(f"stale  docs/index/{name} is missing or differs; {hint}")
                return 1
        print(f"pass  docs index current ({data['counts']['documents']} documents)")
        return 0

    INDEX_DIR.mkdir(exist_ok=True)
    JSON_PATH.write_text(json.dumps(data, indent=1) + "\n", encoding="utf-8")
    for name, body in wanted.items():
        (INDEX_DIR / name).write_text(body, encoding="utf-8")
    # An area that lost its last document leaves a file behind: remove it.
    for name in present - wanted.keys():
        (INDEX_DIR / name).unlink()
    print(
        f"wrote docs/index/ ({len(wanted) - 1} areas, "
        f"{data['counts']['documents']} documents, "
        f"~{data['counts']['agent_tokens']} agent tokens)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
