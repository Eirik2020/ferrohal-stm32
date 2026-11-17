"""Tests for tools/record_evidence.py: python3 -I tools/test_record_evidence.py

The recorder must say what each image was built with: a run built with extra
features (run T, `--features fault-injection`) recorded `features: []`, and
reqtool then fingerprinted the default build, which leaves out run T's own
source.
"""

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import record_evidence  # noqa: E402


class Features(unittest.TestCase):
    def logs(self, *texts):
        folder = Path(tempfile.mkdtemp())
        paths = []
        for i, text in enumerate(texts):
            path = folder / f"run-{i}.log"
            path.write_text(text)
            paths.append(path)
        return paths

    def test_a_run_built_with_features_names_them(self):
        logs = self.logs("build features: fault-injection\nPASS a\n")
        self.assertEqual(record_evidence.features_of(logs), ["fault-injection"])

    def test_a_default_build_names_none(self):
        logs = self.logs("build features: none\nPASS a\n", "PASS b\n")
        self.assertEqual(record_evidence.features_of(logs), [])

    def test_runs_of_one_procedure_must_agree(self):
        logs = self.logs("build features: fault-injection\n", "build features: none\n")
        with self.assertRaises(ValueError):
            record_evidence.features_of(logs)

    def test_the_details_carry_the_features(self):
        details = record_evidence.candidate("0" * 64, ["fault-injection"])
        self.assertEqual(details["features"], ["fault-injection"])


if __name__ == "__main__":
    unittest.main()
