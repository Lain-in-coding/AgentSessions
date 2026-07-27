"""Unit tests for the real-data regression harness.

The tests never touch real transcripts: they synthesise Claude Code fixtures,
so the whole suite is safe to run in CI. Tests that need the compiled binary
skip themselves when it is absent instead of failing, because the harness is
also edited on hosts that have not built the workspace.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import real_data_regression as rdr  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]

# A recognisable token that must never leak into a report.
SECRET_TOKEN = "zqxjkbrw-corpus-secret-token"


def locate_binary():
    """Return the release binary, or None when the workspace is unbuilt."""
    override = os.environ.get("AGENTSESSIONS_BINARY")
    if override:
        candidate = Path(override)
        return candidate if candidate.is_file() else None
    name = "agentsessions.exe" if os.name == "nt" else "agentsessions"
    candidate = REPO_ROOT / "target" / "release" / name
    return candidate if candidate.is_file() else None


def write_fixture(path: Path, session_uuid: str, tag: str) -> None:
    """Write a synthetic 3-record Claude Code transcript.

    Shape: root -> reply -> sidechain probe. The tag keeps each fixture's
    bytes distinct so the content-addressed document ids differ.
    """
    base = session_uuid[:-1]
    records = [
        {
            "type": "user",
            "uuid": f"{base}1",
            "parentUuid": None,
            "sessionId": session_uuid,
            "timestamp": "2026-07-27T01:00:00.000Z",
            "message": {"role": "user", "content": f"{SECRET_TOKEN} root {tag}"},
        },
        {
            "type": "assistant",
            "uuid": f"{base}2",
            "parentUuid": f"{base}1",
            "sessionId": session_uuid,
            "timestamp": "2026-07-27T01:00:01.000Z",
            "message": {"role": "assistant", "content": f"{SECRET_TOKEN} reply {tag}"},
        },
        {
            "type": "user",
            "uuid": f"{base}3",
            "parentUuid": f"{base}2",
            "isSidechain": True,
            "sessionId": session_uuid,
            "timestamp": "2026-07-27T01:00:02.000Z",
            "message": {"role": "user", "content": f"{SECRET_TOKEN} probe {tag}"},
        },
    ]
    path.write_text(
        "\n".join(json.dumps(record) for record in records) + "\n",
        encoding="utf-8",
    )


def sample_report(invariants=None):
    """A canonical, well-formed report built through the real constructor.

    ``invariants`` overrides the default all-passing verdict set, so tests can
    exercise how ``build_report`` derives ``outcome``.
    """
    if invariants is None:
        invariants = [
            rdr.invariant(id_, True, f"{id_} aggregate detail")
            for id_ in rdr.INVARIANT_IDS
        ]
    return rdr.build_report(
        generated_at_utc="2026-07-27T00:00:00Z",
        binary_basename="agentsessions.exe",
        version="0.1.0",
        sha256="0" * 64,
        environment={"os": "Windows", "release": "11", "python": "3.10.11"},
        source_files=2,
        total_bytes=1234,
        totals={"messages": 6, "sessions": 2, "documents": 2, "catalog_entities": 10},
        role_distribution={"user": 4, "assistant": 2},
        evidence_precision={"byte": 6, "line": 0, "record": 0, "unknown": 0},
        invariants=invariants,
    )


class PureFunctionTests(unittest.TestCase):
    """Report construction and invariant judgement need no binary."""

    def test_invariant_ids_are_the_documented_six(self):
        self.assertEqual(
            rdr.INVARIANT_IDS,
            (
                "INV-SYNC-OK",
                "INV-NO-PARSE-LOSS",
                "INV-SESSION-PRESENT",
                "INV-CONTEXT-NONEMPTY",
                "INV-SPAN-COVERAGE",
                "INV-REBUILD-STABLE",
            ),
        )

    def test_outcome_is_failed_when_any_invariant_fails(self):
        # build_report derives outcome from the verdicts: one failure is enough.
        passing = [rdr.invariant(id_, True, "fine") for id_ in rdr.INVARIANT_IDS]
        self.assertEqual(sample_report(passing)["outcome"], "passed")
        mixed = passing[:-1] + [
            rdr.invariant("INV-SPAN-COVERAGE", False, "byte coverage 0.5 of 1.0")
        ]
        self.assertEqual(sample_report(mixed)["outcome"], "failed")

    def test_validate_report_accepts_the_canonical_shape(self):
        self.assertEqual(rdr.validate_report(sample_report()), [])

    def test_validate_report_names_missing_invariants_and_bad_outcome(self):
        report = sample_report()
        report["invariants"] = [entry for entry in report["invariants"] if entry["id"] != "INV-SPAN-COVERAGE"]
        report["outcome"] = "maybe"
        problems = rdr.validate_report(report)
        self.assertIn("missing invariant: INV-SPAN-COVERAGE", problems)
        self.assertIn("outcome must be passed or failed", problems)

    def test_validate_report_names_missing_top_level_fields(self):
        report = sample_report()
        del report["corpus"]
        self.assertIn("missing field: corpus", rdr.validate_report(report))

    def test_report_carries_a_basename_never_a_directory(self):
        # The privacy contract allows the binary's basename only — no directory
        # component may reach the report, so an absolute path must not survive.
        report = sample_report()
        self.assertNotIn(os.sep, report["binary"]["path_basename"])
        self.assertEqual(
            os.path.basename(report["binary"]["path_basename"]),
            report["binary"]["path_basename"],
        )

    def test_markdown_projection_covers_every_invariant(self):
        report = sample_report()
        markdown = rdr.render_markdown(report)
        for invariant_id in rdr.INVARIANT_IDS:
            self.assertIn(invariant_id, markdown)
        self.assertIn(report["outcome"], markdown)


class EndToEndTests(unittest.TestCase):
    """Full harness run against synthetic fixtures and the real binary."""

    @classmethod
    def setUpClass(cls):
        cls.binary = locate_binary()
        if cls.binary is None:
            raise unittest.SkipTest(
                "release binary not found; run "
                "cargo build --locked --release -p agentsessions-cli "
                "or set AGENTSESSIONS_BINARY"
            )

    def setUp(self):
        self.workdir = Path(tempfile.mkdtemp(prefix="rdr-test-"))
        self.addCleanup(shutil.rmtree, self.workdir, ignore_errors=True)
        self.corpus = self.workdir / "corpus"
        self.corpus.mkdir()
        write_fixture(
            self.corpus / "one.jsonl",
            "aaaa1111-2222-4333-8444-555566667771",
            "alpha",
        )
        write_fixture(
            self.corpus / "two.jsonl",
            "bbbb1111-2222-4333-8444-555566667772",
            "beta",
        )

    def run_harness(self, *extra):
        out = self.workdir / "report.json"
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).resolve().parent / "real_data_regression.py"),
                "--binary",
                str(self.binary),
                "--sources",
                str(self.corpus),
                "--out",
                str(out),
                *extra,
            ],
            capture_output=True,
            text=True,
        )
        return completed, out

    def test_synthetic_corpus_passes_every_invariant(self):
        completed, out = self.run_harness()
        self.assertEqual(
            completed.returncode,
            0,
            f"harness failed\nstdout: {completed.stdout}\nstderr: {completed.stderr}",
        )
        report = json.loads(out.read_text(encoding="utf-8"))
        self.assertEqual(report["outcome"], "passed", report["invariants"])
        reported = [entry["id"] for entry in report["invariants"]]
        self.assertEqual(reported, list(rdr.INVARIANT_IDS))
        self.assertEqual(report["corpus"]["source_files"], 2)
        self.assertEqual(report["totals"]["messages"], 6)
        self.assertEqual(report["totals"]["sessions"], 2)
        self.assertEqual(report["evidence_precision"]["unknown"], 0)

    def test_report_never_contains_corpus_text(self):
        _, out = self.run_harness()
        serialized = out.read_text(encoding="utf-8")
        self.assertNotIn(SECRET_TOKEN, serialized)
        # Source paths and provider-native ids must not leak either.
        self.assertNotIn("one.jsonl", serialized)
        self.assertNotIn(str(self.corpus), serialized)
        self.assertNotIn("aaaa1111", serialized)
        markdown = out.with_suffix(".md")
        if markdown.is_file():
            markdown_text = markdown.read_text(encoding="utf-8")
            self.assertNotIn(SECRET_TOKEN, markdown_text)
            self.assertNotIn("aaaa1111", markdown_text)

    def test_dry_run_writes_nothing(self):
        completed, out = self.run_harness("--dry-run")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertFalse(out.exists(), "dry run must not write a report")

    def test_empty_source_selection_is_a_usage_error(self):
        empty = self.workdir / "empty"
        empty.mkdir()
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).resolve().parent / "real_data_regression.py"),
                "--binary",
                str(self.binary),
                "--sources",
                str(empty),
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 2, completed.stdout + completed.stderr)


if __name__ == "__main__":
    unittest.main()
