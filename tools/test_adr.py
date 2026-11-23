"""Tests for tools/adr.py: python3 -I tools/test_adr.py

A decision record holds one date, at most 3000 bytes and the four sections. A
date inside a path to an existing file is that file's name and is not counted.
Every active index row carries tags from the approved list, and the search by tag
reads the index alone. Each test builds its own tree in a temporary directory.
"""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import adr  # noqa: E402

GOOD = """# ADR-001: One owner per thing ({date})

ADR-001 - Status: accepted

## Context

Why.

## Decision

What.

## Rejected

What not.

## Consequences

What follows.
"""

INDEX_HEAD = """# Decision index, 2026

| Id | Date | Tags | Status | Record |
|---|---|---|---|---|
"""


class Tree:
    """A repository root holding docs/decisions/, its index and its lists."""

    def __init__(self):
        self.root = Path(tempfile.mkdtemp())
        self.folder = self.root / "docs/decisions"
        (self.folder / "index").mkdir(parents=True)
        self.tags(["uart", "i2c", "timer"])
        self.rows = []

    def record(self, name, text=None):
        date = name[:10]
        (self.folder / name).write_text(text if text is not None else GOOD.format(date=date))

    def tags(self, tags):
        (self.folder / adr.TAGS).write_text("# only the owner adds a tag\n" + "\n".join(tags) + "\n")

    def row(self, ident, date, status, tags, title, target):
        self.rows.append(f"| {ident} | {date} | {tags} | {status} | [{title}]({target}) |")
        (self.folder / "index/2026.md").write_text(INDEX_HEAD + "\n".join(self.rows) + "\n")


class Format(unittest.TestCase):
    def setUp(self):
        self.tree = Tree()
        self.name = "2026-11-20-thing.md"

    def failures(self):
        return adr.format_failures(self.tree.root)

    def test_a_record_in_format_passes(self):
        self.tree.record(self.name)
        self.assertEqual(self.failures(), [])

    def test_over_size_fails(self):
        text = GOOD.format(date="2026-11-20").replace("Why.", "w" * 3000)
        self.tree.record(self.name, text)
        found = self.failures()
        self.assertEqual(len(found), 1, found)
        self.assertIn("size:", found[0])

    def test_exactly_3000_bytes_passes(self):
        text = GOOD.format(date="2026-11-20")
        text = text.replace("Why.", "w" * (3000 - len(text.encode()) + 4))
        self.assertEqual(len(text.encode()), 3000)
        self.tree.record(self.name, text)
        self.assertEqual(self.failures(), [])

    def test_a_second_date_fails(self):
        text = GOOD.format(date="2026-11-20").replace("What.", "Built 2026-11-21.")
        self.tree.record(self.name, text)
        found = self.failures()
        self.assertEqual(len(found), 1, found)
        self.assertIn("dates:", found[0])
        self.assertIn("2026-11-21", found[0])

    def test_the_same_date_repeated_passes(self):
        text = GOOD.format(date="2026-11-20").replace("What.", "Since 2026-11-20.")
        self.tree.record(self.name, text)
        self.assertEqual(self.failures(), [])

    def test_a_title_without_the_date_fails(self):
        self.tree.record(self.name, GOOD.format(date="no date"))
        found = self.failures()
        self.assertEqual(len(found), 1, found)
        self.assertIn("title does not carry 2026-11-20", found[0])

    def test_an_extra_section_fails(self):
        text = GOOD.format(date="2026-11-20") + "\n## Evidence\n\nRuns.\n"
        self.tree.record(self.name, text)
        found = self.failures()
        self.assertEqual(len(found), 1, found)
        self.assertIn("sections:", found[0])

    def test_a_missing_section_fails(self):
        text = GOOD.format(date="2026-11-20").replace("## Rejected\n\nWhat not.\n\n", "")
        self.tree.record(self.name, text)
        self.assertIn("sections:", self.failures()[0])

    def test_a_suffixed_or_reordered_section_fails(self):
        for text in [
            GOOD.format(date="2026-11-20").replace("## Decision", "## Decision (by agreement)"),
            GOOD.format(date="2026-11-20").replace("## Rejected", "## X").replace(
                "## Consequences", "## Rejected").replace("## X", "## Consequences"),
        ]:
            self.tree.record(self.name, text)
            self.assertIn("sections:", self.failures()[0], text)

    def test_a_heading_in_a_code_fence_is_not_a_section(self):
        text = GOOD.format(date="2026-11-20").replace("What.", "```\n## Not a section\n```")
        self.tree.record(self.name, text)
        self.assertEqual(self.failures(), [])

    def test_a_date_in_a_link_to_an_existing_file_passes(self):
        notes = self.tree.root / "docs/notes"
        notes.mkdir()
        (notes / "2026-11-08-audit.md").write_text("# Audit\n")
        link = "[`../notes/2026-11-08-audit.md`](../notes/2026-11-08-audit.md)"
        text = GOOD.format(date="2026-11-20").replace("Why.", f"The audit: {link}.")
        self.tree.record(self.name, text)
        self.assertEqual(self.failures(), [])

    def test_a_date_in_a_backticked_root_path_passes(self):
        notes = self.tree.root / "docs/notes"
        notes.mkdir()
        (notes / "2026-11-08-audit.md").write_text("# Audit\n")
        text = GOOD.format(date="2026-11-20").replace(
            "Why.", "See `docs/notes/2026-11-08-audit.md`.")
        self.tree.record(self.name, text)
        self.assertEqual(self.failures(), [])

    def test_a_date_in_a_path_to_no_file_fails(self):
        for wrapped in ("[x](../notes/2026-11-08-gone.md)", "`2026-11-08`",
                        "`../notes/2026-11-08-gone.md`"):
            text = GOOD.format(date="2026-11-20").replace("Why.", f"See {wrapped}.")
            self.tree.record(self.name, text)
            found = self.failures()
            self.assertEqual(len(found), 1, (wrapped, found))
            self.assertIn("2026-11-08", found[0])

    def test_a_date_beside_a_real_link_still_fails(self):
        notes = self.tree.root / "docs/notes"
        notes.mkdir()
        (notes / "2026-11-08-audit.md").write_text("# Audit\n")
        text = GOOD.format(date="2026-11-20").replace(
            "Why.", "Audited 2026-11-08: [audit](../notes/2026-11-08-audit.md).")
        self.tree.record(self.name, text)
        self.assertIn("a second date: 2026-11-08", self.failures()[0])

    def test_only_active_records_are_read(self):
        for name in ("README.md", "AGENTS.md", "CLAUDE.md"):
            self.tree.record(name, "# Not a record\n")
        self.assertEqual(self.failures(), [])


class Tags(unittest.TestCase):
    def setUp(self):
        self.tree = Tree()

    def test_approved_tags_pass(self):
        self.tree.row("ADR-001", "2026-11-20", "accepted", "uart, i2c", "A", "../2026-11-20-a.md")
        self.assertEqual(adr.tag_failures(self.tree.root), [])

    def test_an_unknown_tag_fails(self):
        self.tree.row("ADR-001", "2026-11-20", "accepted", "uart, spi", "A", "../2026-11-20-a.md")
        found = adr.tag_failures(self.tree.root)
        self.assertEqual(len(found), 1, found)
        self.assertIn("tag 'spi' is not in", found[0])

    def test_an_active_row_without_a_tag_fails(self):
        self.tree.row("ADR-001", "2026-11-20", "accepted", "", "A", "../2026-11-20-a.md")
        self.assertIn("has no tag", adr.tag_failures(self.tree.root)[0])

    def test_an_archived_row_without_a_tag_passes(self):
        self.tree.row("ADR-001", "2026-11-20", "obsolete", "", "A",
                      "../archive/2026/2026-11-20-a.md")
        self.assertEqual(adr.tag_failures(self.tree.root), [])

    def test_an_index_without_a_tags_column_fails(self):
        (self.tree.folder / "index/2026.md").write_text(
            "| Id | Date | Area | Status | Record |\n|---|---|---|---|---|\n"
            "| ADR-001 | 2026-11-20 | uart | accepted | [A](../2026-11-20-a.md) |\n"
        )
        self.assertTrue(any("no Tags column" in f for f in adr.tag_failures(self.tree.root)))

    def test_a_malformed_or_doubled_approved_tag_fails(self):
        self.tree.tags(["uart", "UART", "uart"])
        found = adr.tag_failures(self.tree.root)
        self.assertTrue(any("'UART' is not a tag" in f for f in found), found)
        self.assertTrue(any("'uart' is listed twice" in f for f in found), found)


def run(root, *argv):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = adr.main(list(argv), root=root)
    return code, out.getvalue(), err.getvalue()


class Find(unittest.TestCase):
    def setUp(self):
        self.tree = Tree()
        self.tree.row("ADR-001", "2026-11-09", "superseded", "uart", "Old UART",
                      "../archive/2026/2026-11-09-old.md")
        self.tree.row("ADR-002", "2026-11-17", "promoted", "uart", "UART owner",
                      "../2026-11-17-uart.md")
        self.tree.row("ADR-003", "2026-11-19", "accepted", "i2c", "I2C driver",
                      "../2026-11-19-i2c.md")
        self.tree.row("ADR-004", "2026-11-19", "promoted", "timer, uart", "Timer lanes",
                      "../2026-11-19-timer.md")

    def test_find_returns_only_matching_records_active_first(self):
        code, out, _ = run(self.tree.root, "find", "uart")
        self.assertEqual(code, 0)
        lines = out.splitlines()
        self.assertEqual([line.split()[0] for line in lines], ["ADR-002", "ADR-004", "ADR-001"])
        self.assertIn("docs/decisions/2026-11-17-uart.md", lines[0])
        self.assertIn("superseded", lines[2])
        self.assertIn("docs/decisions/archive/2026/2026-11-09-old.md", lines[2])
        self.assertNotIn("I2C", out)

    def test_find_with_two_tags_needs_both(self):
        code, out, _ = run(self.tree.root, "find", "uart", "timer")
        self.assertEqual((code, [line.split()[0] for line in out.splitlines()]), (0, ["ADR-004"]))

    def test_find_ignores_case(self):
        self.assertEqual(run(self.tree.root, "find", "UART")[0], 0)

    def test_an_unknown_tag_exits_2_with_the_valid_tags(self):
        code, out, err = run(self.tree.root, "find", "spi")
        self.assertEqual((code, out), (2, ""))
        self.assertIn("unknown tag: spi", err)
        self.assertIn("valid tags: uart, i2c, timer", err)

    def test_find_opens_no_record(self):
        # None of the linked records exists in this tree: the index alone answers.
        self.assertFalse(any(self.tree.folder.glob("*.md")))
        self.assertEqual(run(self.tree.root, "find", "i2c")[0], 0)

    def test_tags_lists_counts(self):
        code, out, _ = run(self.tree.root, "tags")
        self.assertEqual(code, 0)
        counts = {line.split()[0]: line.split()[1:] for line in out.splitlines()[1:]}
        self.assertEqual(counts, {"uart": ["2", "1"], "i2c": ["1", "0"], "timer": ["1", "0"]})


if __name__ == "__main__":
    unittest.main()
