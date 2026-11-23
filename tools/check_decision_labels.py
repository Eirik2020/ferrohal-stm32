#!/usr/bin/env python3
"""Finds decision labels outside the decision records (docs/decisions/AGENTS.md, rule 1).

    python3 -I tools/check_decision_labels.py                      # inventory, exits 0
    python3 -I tools/check_decision_labels.py --diff origin/main   # added lines; exits 1 on any
    python3 -I tools/check_decision_labels.py --diff origin/main --markdown [--previous FILE]

A decision is an ADR in docs/decisions/ and nowhere else. Working documents and
code comments carry no decision label: a bracketed tag opening with Decided,
Proposal, Proposed, Superseded, Accepted or Rejected (any words may follow inside
the brackets), or a parenthesised attribution to the owner (the word owner right
after an opening parenthesis, then a closing one, a comma, a colon, a date or a
verb such as chose), or the owner next to an ISO date in either order. A
bracketed Measured tag is a measurement, not a label.

What is read: tracked Markdown, except docs/decisions/, docs/index/ (generated)
and requirements/evidence/; the comments of the Rust under src/, hil/ and
examples/; the comments and statement strings (docstrings) of tools/ Python; the
comments of tools/ shell. Nothing under any archive directory or target/ is
listed or opened. Only comments are read in code, so the regular expressions and
test fixtures held in string literals here and in the test are not labels; there
is no exemption by file name.

A second class, prose attribution, finds the wording a label turns into when an
agent rewords it rather than writing the record: the owner, then a decision verb
(decided, said, approved, accepted, chose, ...), or the verb, then by the owner.
It warns and never fails, because that wording also describes the owner's role
in rules and policies. It is line-based: a phrase broken across lines is missed.

--diff <base-ref> reads `git diff -U0 $(git merge-base <base-ref> HEAD)`: the
merge base against the working tree, so the committed changes of a pull request
(what CI sees) and an agent's uncommitted edits before it commits (what a local
run sees) are both read, while what landed on the base branch after the branch
point is not. Untracked, unignored files count as added in full. A line counts
when it is added or changed, so a label moved, re-dated or reworded into another
label is flagged too; an unchanged label elsewhere in the file is not.

--markdown writes the body of a pull request comment instead, opening with the
hidden marker line the workflow finds its own comment by. With --previous (that
comment's current body) it keeps the earlier labels the current run no longer
finds, as gone or reworded, so a label reworded away stays visible to the owner.
It prints nothing when nothing was ever found.
"""

from __future__ import annotations

import argparse
import base64
import io
import json
import re
import subprocess
import sys
import tokenize
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

MARKER = "<!-- decision-labels -->"
DATA = re.compile(r"<!-- decision-labels-data: ([A-Za-z0-9+/=]*) -->")

RULE = (
    "A decision is recorded as an ADR in docs/decisions/ and nowhere else "
    "(docs/decisions/AGENTS.md, rule 1). Working documents and code comments carry "
    "no decision label and no owner or date attribution; they state the rule as it "
    "holds now. For each flagged line: write the record or remove the line, do not reword."
)

# --- what is a label ---------------------------------------------------------

LABEL_WORDS = ("Decided", "Proposal", "Proposed", "Superseded", "Accepted", "Rejected")
BRACKET = re.compile(r"\[((?i:" + "|".join(LABEL_WORDS) + r"))\b[^\]\n]*\]")
OWNER_ATTRIBUTION = re.compile(
    r"\(owner(?:\)|\s*[,;:]|\s+\d{4}-\d{2}-\d{2}"
    r"|\s+(?:chose|decided|said|approved|accepted|agreed|asked|ruled|wants|wanted"
    r"|prefers|preferred|confirmed)\b)",
    re.IGNORECASE,
)
# The owner next to an ISO date, either way round, with or without parentheses.
OWNER_DATE = re.compile(
    r"\bowner,?\s+(?:on\s+)?\d{4}-\d{2}-\d{2}\b|\b\d{4}-\d{2}-\d{2}\s*[,;]\s*owner\b",
    re.IGNORECASE,
)

VERBS = (
    r"decided|said|says|approved|accepted|agreed|chose|chosen|ruled|confirmed"
    r"|signed\s+off|rejected|asked|wants|wanted|prefers|preferred|requested|stated"
    r"|settled|picked|set|dropped|added|gave|granted"
)
PROSE = [
    re.compile(r"\bowner\s+(?:has\s+|had\s+|have\s+)?(?:" + VERBS + r")\b", re.IGNORECASE),
    re.compile(
        r"\b(?:decided|approved|accepted|agreed|chosen|confirmed|rejected|requested|ruled)"
        r"\s+(?:on\s+|upon\s+)?by\s+the\s+owner\b",
        re.IGNORECASE,
    ),
    re.compile(
        r"\bper\s+the\s+owner\b|\bat\s+the\s+owner's\s+request\b"
        r"|\bowner's\s+(?:decisions?|ruling|call|choice|approval|sign-off|instruction|pick|option"
        r"|go-ahead|priority)\b",
        re.IGNORECASE,
    ),
]

# --- what is read ------------------------------------------------------------

SKIPPED = ("docs/decisions/", "docs/index/", "requirements/evidence/")
# Passed to git so no archive path is ever listed, let alone opened.
PATHSPEC = ["--", ".", ":(exclude,glob)**/archive/**", ":(exclude,glob)**/target/**"]
RUST_ROOTS = ("src", "hil", "examples")


@dataclass(frozen=True)
class Match:
    path: str
    line: int
    text: str
    form: str  # the label word, "owner attribution", or "prose attribution"
    label: bool  # False: the prose-attribution warning class

    def __str__(self) -> str:
        return f"{self.path}:{self.line}: {self.text}"


def kind_of(path: str) -> str | None:
    """How a repository-relative path is read, or None when it is not."""
    parts = path.split("/")
    if "archive" in parts or "target" in parts or path.startswith(SKIPPED):
        return None
    if path.endswith(".md"):
        return "markdown"
    if path.endswith(".rs") and parts[0] in RUST_ROOTS:
        return "rust"
    if parts[0] == "tools" and path.endswith(".py"):
        return "python"
    if parts[0] == "tools" and path.endswith(".sh"):
        return "shell"
    return None


def _add(found: dict[int, list[str]], line: int, text: str) -> None:
    found.setdefault(line, []).append(text)


RAW_STRING = re.compile(r'b?r(#*)"')
CHAR = re.compile(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\}|.)|[^\\'\n])'")


def rust_comments(text: str) -> dict[int, list[str]]:
    """Line -> the comment text on it: line, doc and (nested) block comments, but
    nothing inside a string, raw string or character literal."""
    found: dict[int, list[str]] = {}
    i, n, line = 0, len(text), 1
    while i < n:
        c = text[i]
        if c == "\n":
            line += 1
            i += 1
        elif text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end < 0 else end
            _add(found, line, text[i:end])
            i = end
        elif text.startswith("/*", i):
            depth, start, i = 1, i, i + 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                elif text[i] == "\n":
                    _add(found, line, text[start:i])
                    line, i, start = line + 1, i + 1, i + 1
                else:
                    i += 1
            _add(found, line, text[start:i])
        elif c in "br" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")) and (
            m := RAW_STRING.match(text, i)
        ):
            close = '"' + m.group(1)
            end = text.find(close, m.end())
            end = n if end < 0 else end + len(close)
            line += text.count("\n", i, end)
            i = end
        elif c == '"':
            i += 1
            while i < n and text[i] != '"':
                if text[i] == "\\":
                    i += 1
                if i < n and text[i] == "\n":
                    line += 1
                i += 1
            i += 1
        elif c == "'":
            m = CHAR.match(text, i)
            i = m.end() if m else i + 1  # no match: a lifetime or a label
        else:
            i += 1
    return found


def python_comments(text: str) -> dict[int, list[str]]:
    """Line -> comment text, and the text of any string that is a statement of its
    own (a docstring), since module docs carry prose as comments do."""
    found: dict[int, list[str]] = {}
    try:
        tokens = list(tokenize.generate_tokens(io.StringIO(text).readline))
    except (tokenize.TokenError, SyntaxError):
        for number, line in enumerate(text.splitlines(), 1):
            if "#" in line:
                _add(found, number, line[line.index("#"):])
        return found
    significant = [t for t in tokens if t.type not in (tokenize.COMMENT, tokenize.NL)]
    starts = (tokenize.NEWLINE, tokenize.INDENT, tokenize.DEDENT)
    for t in tokens:
        if t.type == tokenize.COMMENT:
            _add(found, t.start[0], t.string)
    for k, t in enumerate(significant):
        if t.type != tokenize.STRING:
            continue
        before = significant[k - 1].type if k else tokenize.NEWLINE
        after = significant[k + 1].type if k + 1 < len(significant) else tokenize.NEWLINE
        if before in starts and after in (tokenize.NEWLINE, tokenize.ENDMARKER):
            for offset, part in enumerate(t.string.split("\n")):
                _add(found, t.start[0] + offset, part)
    return found


SHELL_COMMENT = re.compile(r"(?:^|(?<=\s))(#.*)$")


def shell_comments(text: str) -> dict[int, list[str]]:
    """Line -> the text from the first `#` that opens a word. Quotes are not
    tracked, so a `#` word inside a quoted string reads as a comment."""
    found: dict[int, list[str]] = {}
    for number, line in enumerate(text.splitlines(), 1):
        if number == 1 and line.startswith("#!"):
            continue
        m = SHELL_COMMENT.search(line)
        if m:
            _add(found, number, m.group(1))
    return found


def readable(kind: str, text: str) -> dict[int, list[str]]:
    if kind == "markdown":
        return {n: [line] for n, line in enumerate(text.splitlines(), 1)}
    return {"rust": rust_comments, "python": python_comments, "shell": shell_comments}[kind](text)


def scan_text(path: str, kind: str, text: str) -> list[Match]:
    lines = text.splitlines()
    matches: list[Match] = []
    for number, parts in sorted(readable(kind, text).items()):
        shown = lines[number - 1].strip() if number <= len(lines) else " ".join(parts)
        forms: list[str] = []
        for part in parts:
            forms += [m.group(1)[0].upper() + m.group(1)[1:].lower() for m in BRACKET.finditer(part)]
            if OWNER_ATTRIBUTION.search(part):
                forms.append("owner attribution")
            elif OWNER_DATE.search(part):
                forms.append("owner-date attribution")
        if forms:
            matches.append(Match(path, number, shown, forms[0], True))
        elif any(p.search(part) for part in parts for p in PROSE):
            matches.append(Match(path, number, shown, "prose attribution", False))
    return matches


# --- git ---------------------------------------------------------------------


def git(repo: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-c", "core.quotePath=false", *args],
        cwd=repo,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {result.stderr.strip()}")
    return result.stdout


def scan_paths(repo: Path, paths: list[str]) -> list[Match]:
    matches: list[Match] = []
    for path in sorted(set(paths)):
        kind = kind_of(path)
        file = repo / path
        if kind is None or not file.is_file():
            continue
        matches += scan_text(path, kind, file.read_text(encoding="utf-8", errors="replace"))
    return matches


def inventory(repo: Path) -> list[Match]:
    return scan_paths(repo, git(repo, "ls-files", *PATHSPEC).splitlines())


HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


def added_lines(repo: Path, base: str) -> dict[str, set[int] | None]:
    """Path -> the line numbers added since the merge base of `base` and HEAD, in
    the working tree; None for an untracked file, every line of which is new."""
    merge_base = git(repo, "merge-base", base, "HEAD").strip()
    diff = git(repo, "diff", "-U0", "-M", "--no-color", "--no-ext-diff", merge_base, *PATHSPEC)
    added: dict[str, set[int] | None] = {}
    current: str | None = None
    for line in diff.splitlines():
        if line.startswith("+++ "):
            target = line[4:]
            current = target[2:] if target.startswith("b/") else None
        elif current is not None and (m := HUNK.match(line)):
            start, count = int(m.group(1)), int(m.group(2) or 1)
            lines = added.setdefault(current, set())
            assert lines is not None
            lines.update(range(start, start + count))
    for path in git(repo, "ls-files", "--others", "--exclude-standard", *PATHSPEC).splitlines():
        added[path] = None
    return added


def diff_matches(repo: Path, base: str) -> list[Match]:
    added = added_lines(repo, base)
    return [
        m
        for m in scan_paths(repo, list(added))
        if added[m.path] is None or m.line in added[m.path]  # type: ignore[operator]
    ]


# --- reporting ---------------------------------------------------------------


def fence(lines: list[str]) -> str:
    longest = max((len(r) for line in lines for r in re.findall(r"`+", line)), default=0)
    ticks = "`" * max(3, longest + 1)
    return "\n".join([ticks + "text", *lines, ticks])


def decode(previous: str) -> dict[str, list[list]]:
    m = DATA.search(previous or "")
    if not m:
        return {"labels": []}
    try:
        data = json.loads(base64.b64decode(m.group(1)).decode())
    except (ValueError, UnicodeDecodeError):
        return {"labels": []}
    return data if isinstance(data, dict) and isinstance(data.get("labels"), list) else {"labels": []}


def markdown(matches: list[Match], previous: str = "") -> str:
    """The pull request comment body; empty when nothing was found now or before."""
    labels = [m for m in matches if m.label]
    warnings = [m for m in matches if not m.label]
    now = {(m.path, m.text) for m in labels}
    history = [
        entry
        for entry in decode(previous)["labels"]
        if isinstance(entry, list) and len(entry) == 3 and (entry[0], entry[2]) not in now
    ]
    if not labels and not warnings and not history:
        return ""

    out = [MARKER]
    if labels:
        out += [
            f"### Decision labels: {len(labels)} added line(s) fail this check",
            "",
            RULE,
            "",
            fence([str(m) for m in labels]),
        ]
    else:
        out += ["### Decision labels: none in this pull request now"]
    if warnings:
        out += [
            "",
            f"#### Possible reworded labels: {len(warnings)} line(s), a warning only",
            "",
            "These added lines attribute something to the owner in prose. If one is a "
            "decision, it needs an ADR in `docs/decisions/`, not new wording.",
            "",
            fence([str(m) for m in warnings]),
        ]
    if history:
        out += [
            "",
            f"#### Flagged on an earlier push, gone now: {len(history)} line(s)",
            "",
            "Each was removed or reworded. A removed line, or one whose decision became an "
            "ADR, is done; a line that was only reworded still carries the decision outside "
            "its record.",
            "",
            fence([f"{p}:{n}: {t}" for p, n, t in history]),
        ]
    kept = [[m.path, m.line, m.text] for m in labels] + history
    payload = base64.b64encode(json.dumps({"labels": kept}).encode()).decode()
    out += ["", f"<!-- decision-labels-data: {payload} -->"]
    return "\n".join(out) + "\n"


def summary(matches: list[Match]) -> str:
    counts: dict[str, int] = {}
    for m in matches:
        counts[m.form] = counts.get(m.form, 0) + 1
    return ", ".join(f"{form} {n}" for form, n in sorted(counts.items(), key=lambda kv: -kv[1]))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--diff", metavar="BASE_REF", help="flag only lines added since BASE_REF")
    parser.add_argument("--markdown", action="store_true", help="with --diff: a PR comment body")
    parser.add_argument("--previous", metavar="FILE", help="with --markdown: the comment's body")
    parser.add_argument("--repo", type=Path, default=ROOT, help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.markdown and not args.diff:
        parser.error("--markdown needs --diff")
    if args.previous and not args.markdown:
        parser.error("--previous needs --markdown")

    try:
        matches = diff_matches(args.repo, args.diff) if args.diff else inventory(args.repo)
    except RuntimeError as error:
        print(f"FAIL  {error}", file=sys.stderr)
        return 2
    labels = [m for m in matches if m.label]
    warnings = [m for m in matches if not m.label]

    if args.markdown:
        previous = Path(args.previous).read_text() if args.previous else ""
        sys.stdout.write(markdown(matches, previous))
        return 1 if labels else 0

    for m in labels:
        print(m)
    for m in warnings:
        print(f"{m.path}:{m.line}: warning: prose attribution: {m.text}")

    if not args.diff:
        files = len({m.path for m in labels})
        print(f"{len(labels)} decision label(s) in {files} file(s)" + (f": {summary(labels)}" if labels else ""))
        print(f"{len(warnings)} prose attribution warning(s)")
        return 0

    if labels:
        print(f"FAIL  {len(labels)} decision label(s) on lines added since {args.diff}")
        print(f"      {RULE}")
        return 1
    print(
        f"pass  no decision labels on lines added since {args.diff}"
        + (f" ({len(warnings)} prose attribution warning(s))" if warnings else "")
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
