"""Tests for the study-level validator."""
import json
from pathlib import Path
import tempfile
import unittest

import validate_study as v

RESEARCH = Path(__file__).resolve().parent


class StudyValidatorTests(unittest.TestCase):
    def test_real_source_mode_passes(self):
        v.verify_sources(v.load(RESEARCH / "sources.json"))
        v.verify_environment(v.load(RESEARCH / "environment.json"))
        v.verify_matrix(v.load(RESEARCH / "reuse-matrix.json"), v.load(RESEARCH / "sources.json"))
        v.privacy_scan()

    def test_matrix_tamper_is_rejected(self):
        matrix = v.load(RESEARCH / "reuse-matrix.json")
        sources = v.load(RESEARCH / "sources.json")
        matrix["entries"][0]["product_adoption_approved"] = True
        with self.assertRaises(ValueError):
            v.verify_matrix(matrix, sources)
        matrix = v.load(RESEARCH / "reuse-matrix.json")
        matrix["entries"][0]["classification"] = "maybe"
        with self.assertRaises(ValueError):
            v.verify_matrix(matrix, sources)

    def test_source_range_bounds_are_enforced(self):
        matrix = v.load(RESEARCH / "reuse-matrix.json")
        sources = v.load(RESEARCH / "sources.json")
        matrix["entries"][0]["source"][0]["lines"] = [1, 10 ** 9]
        with self.assertRaises(ValueError):
            v.verify_matrix(matrix, sources)

    def test_privacy_scan_flags_home_paths(self):
        original = v.RESEARCH
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            leak = "path " + "C:\\" + "Users\\" + "someone\\secret"
            (root / "leak.md").write_text(leak, encoding="utf-8")
            v.RESEARCH = root
            try:
                with self.assertRaises(ValueError):
                    v.privacy_scan()
            finally:
                v.RESEARCH = original

    def test_seal_tamper_is_rejected(self):
        report = {"a": 1}
        report["integrity_sha256"] = v.canonical_hash(report)
        v.verify_sealed(report, "unit")
        report["a"] = 2
        with self.assertRaises(ValueError):
            v.verify_sealed(report, "unit")


if __name__ == "__main__":
    unittest.main()
