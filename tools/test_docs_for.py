#!/usr/bin/env python3
"""Tests for tools/docs_for.py, each in a throwaway git repository."""
import contextlib
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import docs_for  # noqa: E402

MAP = """# comment
src/uart/** docs/design/uart.md
src/spi/** none
src/chips/** docs/design/glue.md
src/chips/*/errata_*.rs docs/design/errata.md
"""


def run(root: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)


class Repo(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        for f, text in {
            "docs/owners.txt": MAP,
            "docs/design/uart.md": "uart\n",
            "docs/design/glue.md": "glue\n",
            "docs/design/errata.md": "errata\n",
            "src/uart/mod.rs": "a\n",
            "src/spi/mod.rs": "a\n",
            "src/chips/h7/errata_x.rs": "a\n",
        }.items():
            self.write(f, text)
        run(self.root, "init", "-q", "-b", "main")
        run(self.root, "-c", "user.name=t", "-c", "user.email=t@t", "add", "-A")
        run(self.root, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "base")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def write(self, rel: str, text: str) -> None:
        p = self.root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)

    def call(self, *argv: str) -> tuple[int, str]:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = docs_for.main(list(argv), root=self.root, base="main")
        return rc, out.getvalue()

    def test_last_matching_line_wins(self) -> None:
        table = docs_for.rules(self.root)
        self.assertEqual(docs_for.owners("src/chips/h7/errata_x.rs", table), ["docs/design/errata.md"])
        self.assertEqual(docs_for.owners("src/chips/h7/common.rs", table), ["docs/design/glue.md"])
        self.assertEqual(docs_for.owners("src/spi/mod.rs", table), [])
        self.assertIsNone(docs_for.owners("src/crc/mod.rs", table))

    def test_nothing_changed_passes(self) -> None:
        self.assertEqual(self.call("--check")[0], 0)

    def test_code_without_its_page_fails(self) -> None:
        self.write("src/uart/mod.rs", "b\n")
        rc, out = self.call("--check")
        self.assertEqual(rc, 1)
        self.assertIn("docs/design/uart.md owns src/uart/mod.rs", out)

    def test_code_with_its_page_passes(self) -> None:
        self.write("src/uart/mod.rs", "b\n")
        self.write("docs/design/uart.md", "uart, changed\n")
        self.assertEqual(self.call("--check")[0], 0)

    def test_a_stated_reason_passes_and_is_printed(self) -> None:
        self.write("src/uart/mod.rs", "b\n")
        rc, out = self.call("--check", "--none", "a comment only")
        self.assertEqual(rc, 0)
        self.assertIn("Docs: none - a comment only", out)

    def test_an_empty_reason_is_refused(self) -> None:
        self.write("src/uart/mod.rs", "b\n")
        self.assertEqual(self.call("--check", "--none", " ")[0], 2)

    def test_a_path_mapped_to_none_owes_nothing(self) -> None:
        self.write("src/spi/mod.rs", "b\n")
        self.assertEqual(self.call("--check")[0], 0)

    def test_unmapped_code_fails_even_with_a_reason(self) -> None:
        self.write("src/crc/mod.rs", "new\n")
        rc, out = self.call("--check", "--none", "new driver")
        self.assertEqual(rc, 1)
        self.assertIn("src/crc/mod.rs has no line", out)

    def test_map_names_a_missing_page(self) -> None:
        self.write("docs/owners.txt", MAP + "src/x/** docs/design/absent.md\n")
        rc, out = self.call("--map")
        self.assertEqual(rc, 1)
        self.assertIn("absent.md, which does not exist", out)

    def test_map_refuses_an_owner_outside_design(self) -> None:
        self.write("docs/decisions/a.md", "a\n")
        self.write("docs/owners.txt", MAP + "src/x/** docs/decisions/a.md\n")
        rc, out = self.call("--map")
        self.assertEqual(rc, 1)
        self.assertIn("an owner is a page under docs/design/", out)

    def test_map_finds_an_unmapped_tracked_file(self) -> None:
        self.write("src/crc/mod.rs", "new\n")
        run(self.root, "add", "-A")
        rc, out = self.call("--map")
        self.assertEqual(rc, 1)
        self.assertIn("src/crc/mod.rs has no line", out)

    def test_committed_change_on_the_branch_counts(self) -> None:
        run(self.root, "checkout", "-q", "-b", "work")
        self.write("src/uart/mod.rs", "b\n")
        run(self.root, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-am", "x")
        self.assertEqual(self.call("--check")[0], 1)


if __name__ == "__main__":
    unittest.main()
