"""Tests for the Python 3.10 lockfile fallback and evidence sealing."""
import json
import tempfile
import unittest
from pathlib import Path

import evidence as ev

SAMPLE = '''version = 4

[[package]]
name = "rusqlite"
version = "0.40.2"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"
dependencies = [
 "libsqlite3-sys",
]

[[package]]
name = "libsqlite3-sys"
version = "0.38.2"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "def"

[[package]]
name = "other"
version = "1"
'''


class FallbackLockTests(unittest.TestCase):
    def test_fallback_matches_expected_selection(self):
        with tempfile.TemporaryDirectory() as name:
            lock = Path(name) / "Cargo.lock"
            lock.write_text(SAMPLE, encoding="utf-8")
            selected = ev.sqlite_locked_selection(lock)
        self.assertEqual([p["name"] for p in selected], ["libsqlite3-sys", "rusqlite"])
        self.assertEqual(selected[1]["checksum"], "abc")

    def test_real_workspace_lock_selects_two_packages(self):
        workspace = Path(__file__).resolve().parents[5]
        selected = ev.sqlite_locked_selection(workspace / "Cargo.lock")
        self.assertEqual({p["name"] for p in selected}, {"libsqlite3-sys", "rusqlite"})
        self.assertEqual({p["version"] for p in selected}, {"0.40.2", "0.38.2"})

    def test_seal_roundtrip_and_tamper(self):
        report = {"a": 1, "b": [True, None]}
        sealed = ev.seal(dict(report))
        ev.verify_seal(json.loads(json.dumps(sealed)))
        tampered = dict(sealed)
        tampered["a"] = 2
        with self.assertRaises(ValueError):
            ev.verify_seal(tampered)


if __name__ == "__main__":
    unittest.main()
