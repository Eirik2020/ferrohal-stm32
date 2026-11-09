#!/usr/bin/env python3
"""Prints one document, or one section of it, with a token cap.

Section-addressed reads are how an agent gets one fact without loading a whole file:
    python3 -I tools/docs_read.py ferrohal-architecture.md 5
    python3 -I tools/docs_read.py docs/policies/safety.md "Exception 1"
Output is capped at 400 estimated tokens; anything longer is truncated with a note.
Paths outside the repository are refused - the repository is the root this tool serves.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CAP_TOKENS = 400
HEADING = re.compile(r"^(#+)\s+(.*)$")


def section(lines: list[str], wanted: str) -> list[str] | None:
    for index, line in enumerate(lines):
        found = HEADING.match(line)
        if not found:
            continue
        heading = found.group(2)
        hit = wanted.lower() in heading.lower()
        if not hit and wanted.isdigit():
            hit = re.search(rf"§?\s*{wanted}\.", heading) is not None
        if not hit:
            continue
        level = len(found.group(1))
        body = [line]
        for following in lines[index + 1:]:
            next_heading = HEADING.match(following)
            if next_heading and len(next_heading.group(1)) <= level:
                break
            body.append(following)
        return body
    return None


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: docs_read.py <path> [section]", file=sys.stderr)
        return 2

    path = (ROOT / sys.argv[1]).resolve()
    if ROOT not in path.parents:
        print(f"refused  {sys.argv[1]} is outside the repository", file=sys.stderr)
        return 2

    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        print(f"missing  {error}", file=sys.stderr)
        return 2

    if len(sys.argv) > 2:
        found = section(lines, sys.argv[2])
        if found is None:
            print(f"no section matching '{sys.argv[2]}'", file=sys.stderr)
            return 2
        lines = found

    text = "\n".join(lines)
    tokens = (len(text.encode()) + 3) // 4
    if tokens > CAP_TOKENS:
        cut = text.encode()[: CAP_TOKENS * 4].decode(errors="ignore")
        text = cut.rsplit("\n", 1)[0] + "\n... (truncated; read a section instead)"
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
