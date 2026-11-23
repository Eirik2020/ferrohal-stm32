"""Tests for tools/check_decision_labels.py: python3 -I tools/test_check_decision_labels.py

Agents wrote decisions as labels in working documents and code comments rather
than as ADRs. The check finds each label form, leaves measurement tags and the
decision records alone, and in --diff mode fails only on lines a change adds.
The fixtures sit in string literals, which the check never reads in code.
"""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_decision_labels as labels  # noqa: E402


def found(text, path="docs/design/x.md", kind="markdown"):
    return labels.scan_text(path, kind, text)


class Forms(unittest.TestCase):
    def assertLabel(self, text, form):
        matches = found(text)
        self.assertEqual(len(matches), 1, text)
        self.assertTrue(matches[0].label, text)
        self.assertEqual(matches[0].form, form, text)

    def test_each_bracket_form_is_a_label(self):
        for text, form in [
            ("- **[Decided]** The rule.", "Decided"),
            ("- **[Proposal]** The idea.", "Proposal"),
            ("**[Superseded 2026-11-13, ADR-009]** ~~Old.~~", "Superseded"),
            ("- **[Decided 2026-11-13, owner]** The rule.", "Decided"),
            ("- **[Decided, built 2026-11-14]** The probe.", "Decided"),
            ("- **[Decided, done 2026-11-15]** Retired.", "Decided"),
            ("- [decided] lower case is the same label", "Decided"),
        ]:
            self.assertLabel(text, form)

    def test_each_owner_attribution_is_a_label(self):
        for text, form in [
            ("The rule holds (owner, 2026-11-13).", "owner attribution"),
            ("Stamped (owner, 2026-11-15: stamped, not mapped at read time).", "owner attribution"),
            ("Widened 2026-11-19 (owner).", "owner attribution"),
            ("Not now (owner 2026-11-13: not now).", "owner attribution"),
            ("The shape (owner chose the shape).", "owner attribution"),
            ("Deleted (2026-11-13, owner). The sweep", "owner-date attribution"),
            ("closed (2026-11-17; owner's pick A).", "owner-date attribution"),
            ("- Owner 2026-11-11: generator first.", "owner-date attribution"),
        ]:
            self.assertLabel(text, form)

    def test_a_measurement_tag_is_not_a_label(self):
        self.assertEqual(found("- **[Measured 2026-11-13, metapac 21]** 40 ms."), [])

    def test_ordinary_owner_words_are_not_flagged(self):
        for text in [
            "The driver has one owner, and `on_interrupt` takes it.",
            "- `review/` (owner's reviews + drafts, uncommitted): read only.",
            "See (ownership.md) for the rule.",
            "| Vector | Owner | Priority |",
        ]:
            self.assertEqual(found(text), [], text)

    def test_a_reworded_label_is_a_warning_not_a_label(self):
        for text in [
            "The owner decided on 13 November that the gate stays.",
            "This was approved by the owner.",
            "It is the owner's decision to keep it.",
        ]:
            matches = found(text)
            self.assertEqual(len(matches), 1, text)
            self.assertFalse(matches[0].label, text)


class Code(unittest.TestCase):
    def test_rust_comments_are_read_and_strings_are_not(self):
        source = "\n".join(
            [
                '//! [Decided] in a module doc',
                'const A: &str = "[Decided] in a string";',
                'let b = r#"(owner, 2026-11-13) in a raw string"#;',
                "fn f<'a>(x: &'a u8) {} // (owner, 2026-11-13) after a lifetime",
                "/* a block",
                "   [Proposal] inside it */",
                "let c = '\"'; // [Superseded] after a quote char",
            ]
        )
        lines = [m.line for m in found(source, "src/x.rs", "rust")]
        self.assertEqual(lines, [1, 4, 6, 7])

    def test_python_comments_and_docstrings_are_read_and_literals_are_not(self):
        source = "\n".join(
            [
                '"""Module doc (owner, 2026-11-16)."""',
                'PATTERN = "[Decided]"',
                "x = 1  # [Proposal] in a comment",
                "def f():",
                '    """[Decided] in a function doc."""',
                '    return "(owner, 2026-11-13)"',
            ]
        )
        lines = [m.line for m in found(source, "tools/x.py", "python")]
        self.assertEqual(lines, [1, 3, 5])

    def test_shell_comments_are_read(self):
        source = "#!/bin/sh\n# [Decided] here\necho ${#x} # (owner, 2026-11-13)\necho a#b\n"
        lines = [m.line for m in found(source, "tools/x.sh", "shell")]
        self.assertEqual(lines, [2, 3])


class Scope(unittest.TestCase):
    def test_what_is_read(self):
        for path, kind in [
            ("docs/design/naming.md", "markdown"),
            ("hil/WIRING.md", "markdown"),
            ("AGENTS.md", "markdown"),
            ("src/uart/mod.rs", "rust"),
            ("hil/common/tests.rs", "rust"),
            ("examples/nucleo-f401re/src/main.rs", "rust"),
            ("tools/check_docs.py", "python"),
            ("tools/glue-gate.sh", "shell"),
        ]:
            self.assertEqual(labels.kind_of(path), kind, path)

    def test_what_is_not_read(self):
        for path in [
            "docs/decisions/2026-11-13-maturity-tiers.md",
            "docs/decisions/AGENTS.md",
            "docs/index/design.md",
            "requirements/evidence/run.md",
            "docs/notes/archive/old.md",
            "archive/x.md",
            "target/doc/x.md",
            "build.rs",
            "checks/no-panic/src/main.rs",
        ]:
            self.assertIsNone(labels.kind_of(path), path)


def git(repo, *args):
    subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True)


class Diff(unittest.TestCase):
    def setUp(self):
        self.repo = Path(tempfile.mkdtemp())
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "user.email", "test@example.invalid")
        git(self.repo, "config", "user.name", "test")
        self.write("docs/design/a.md", "# A\n\n- **[Decided]** An old label.\n")
        self.write("docs/decisions/2026-11-13-x.md", "# X\n")
        self.write("src/lib.rs", "//! A crate.\nfn f() {}\n")
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "-m", "base")
        git(self.repo, "tag", "base")

    def write(self, path, text):
        file = self.repo / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(text)

    def run_check(self, *args):
        result = subprocess.run(
            [sys.executable, "-I", labels.__file__, "--repo", str(self.repo), *args],
            capture_output=True,
            text=True,
        )
        return result.returncode, result.stdout

    def test_the_inventory_lists_existing_labels_and_passes(self):
        code, out = self.run_check()
        self.assertEqual(code, 0)
        self.assertIn("docs/design/a.md:3: - **[Decided]** An old label.", out)
        self.assertIn("1 decision label(s)", out)

    def test_an_unchanged_tree_passes(self):
        self.assertEqual(self.run_check("--diff", "base")[0], 0)

    def test_only_added_lines_fail(self):
        self.write("docs/design/a.md", "# A\n\nNew line.\n\n- **[Decided]** An old label.\n- New (owner, 2026-11-20).\n")
        self.write("src/lib.rs", "//! A crate.\n// [Proposal] a new comment\nfn f() {}\n")
        code, out = self.run_check("--diff", "base")
        self.assertEqual(code, 1)
        self.assertIn("docs/design/a.md:6: - New (owner, 2026-11-20).", out)
        self.assertIn("src/lib.rs:2: // [Proposal] a new comment", out)
        self.assertNotIn("An old label", out)
        self.assertIn("docs/decisions/AGENTS.md", out)
        self.assertIn("write the record or remove the line, do not reword", out)

    def test_committed_and_untracked_additions_count(self):
        self.write("hil/NOTES.md", "- **[Decided]** committed\n")
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "-m", "change")
        self.write("docs/design/new.md", "- **[Superseded]** untracked\n")
        code, out = self.run_check("--diff", "base")
        self.assertEqual(code, 1)
        self.assertIn("hil/NOTES.md:1:", out)
        self.assertIn("docs/design/new.md:1:", out)

    def test_a_decision_record_may_carry_labels(self):
        self.write("docs/decisions/2026-11-13-x.md", "# X\n\n## Decision (owner, 2026-11-13)\n")
        self.assertEqual(self.run_check("--diff", "base")[0], 0)

    def test_a_prose_attribution_warns_and_passes(self):
        self.write("docs/design/a.md", "# A\n\nThe owner decided this.\n\n- **[Decided]** An old label.\n")
        code, out = self.run_check("--diff", "base")
        self.assertEqual(code, 0)
        self.assertIn("warning: prose attribution", out)

    def test_an_unknown_base_is_an_error(self):
        self.assertEqual(self.run_check("--diff", "no-such-ref")[0], 2)

    def test_markdown_needs_diff(self):
        self.assertEqual(self.run_check("--markdown")[0], 2)


class Comment(unittest.TestCase):
    def match(self, text, line=1, label=True):
        return labels.Match("docs/design/a.md", line, text, "Decided", label)

    def test_the_body_opens_with_the_marker_and_carries_the_rule(self):
        body = labels.markdown([self.match("- **[Decided]** x")])
        self.assertTrue(body.startswith("<!-- decision-labels -->\n"))
        self.assertIn("docs/design/a.md:1: - **[Decided]** x", body)
        self.assertIn("write the record or remove the line, do not reword", body)

    def test_nothing_found_now_or_before_is_an_empty_body(self):
        self.assertEqual(labels.markdown([]), "")

    def test_a_label_gone_on_a_later_push_stays_visible(self):
        first = labels.markdown([self.match("- **[Decided]** x", 4)])
        later = labels.markdown([self.match("The owner decided x.", 4, label=False)], first)
        self.assertIn("none in this pull request now", later)
        self.assertIn("gone now: 1 line(s)", later)
        self.assertIn("docs/design/a.md:4: - **[Decided]** x", later)
        self.assertIn("Possible reworded labels", later)
        # The history survives a further push.
        self.assertIn("docs/design/a.md:4: - **[Decided]** x", labels.markdown([], later))

    def test_a_label_still_present_is_not_history(self):
        first = labels.markdown([self.match("- **[Decided]** x", 4)])
        again = labels.markdown([self.match("- **[Decided]** x", 6)], first)
        self.assertNotIn("gone now", again)

    def test_backticks_in_a_line_cannot_close_the_fence(self):
        body = labels.markdown([self.match("``` [Decided] ```")])
        self.assertIn("````text", body)


if __name__ == "__main__":
    unittest.main()
