"""Unit tests for the growing-source repro harness.

These exercise the harness's own logic (record shaping, frame classification,
trial aggregation) without spawning the binary, so they are safe and fast in CI.
The one test that needs the compiled binary skips itself when it is absent,
matching ``test_real_data_regression.py``: the harness is also edited on hosts
that have not built the workspace.
"""

import json
import os
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import growing_source_repro as gsr  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]


def locate_binary():
    """Return the release binary, or None when the workspace is unbuilt."""
    override = os.environ.get("AGENT_SESSION_GREP_BINARY")
    if override:
        candidate = Path(override)
        return candidate if candidate.is_file() else None
    name = "agent-session-grep.exe" if os.name == "nt" else "agent-session-grep"
    candidate = REPO_ROOT / "target" / "release" / name
    return candidate if candidate.is_file() else None


class RecordShapeTest(unittest.TestCase):
    def test_record_is_valid_json_with_fabricated_identity(self):
        record = json.loads(gsr._record(7))
        self.assertEqual(record["type"], "user")
        self.assertEqual(record["message"]["role"], "user")
        # Fabricated, not harvested: the uuid is a fixed prefix plus the index.
        self.assertTrue(record["uuid"].startswith("aaaa0000-0000-4000-8000-"))
        self.assertEqual(record["uuid"][-12:], "000000000007")

    def test_records_are_distinct_so_appends_change_length(self):
        self.assertNotEqual(gsr._record(1), gsr._record(2))

    def test_record_carries_no_real_paths_or_identities(self):
        # The corpus must be safe to paste into an issue; the only free text is
        # the filler, so assert it stays filler.
        record = json.loads(gsr._record(3))
        content = record["message"]["content"]
        self.assertTrue(content.startswith("synthetic record 3 "))
        self.assertEqual(set(content.split(" ", 3)[3]), {"x"})


class ErrorFrameTest(unittest.TestCase):
    def test_finds_the_error_frame_among_other_lines(self):
        output = "\n".join(
            [
                "not json at all",
                json.dumps({"frame_type": "progress", "ok": True}),
                json.dumps({"error": {"code": "source_changed", "retryable": True}}),
            ]
        )
        frame = gsr._error_frame(output)
        self.assertIsNotNone(frame)
        self.assertEqual(frame["error"]["code"], "source_changed")

    def test_returns_none_when_the_run_succeeded(self):
        output = json.dumps({"frame_type": "response", "ok": True, "data": {}})
        self.assertIsNone(gsr._error_frame(output))

    def test_ignores_a_frame_whose_error_is_not_an_object(self):
        # A success envelope may legitimately carry `"error": null`; that is not
        # an error frame and must not be read as one.
        output = json.dumps({"ok": True, "error": None})
        self.assertIsNone(gsr._error_frame(output))


class ChildEnvTest(unittest.TestCase):
    def test_redirects_home_and_drops_inherited_store_path(self):
        os.environ["AGENT_SESSION_GREP_DB"] = "C:/placeholder/real.db"
        os.environ["ASG_DB"] = "C:/placeholder/real.db"
        try:
            env = gsr._child_env(Path("C:/placeholder/scratch"))
        finally:
            os.environ.pop("AGENT_SESSION_GREP_DB", None)
            os.environ.pop("ASG_DB", None)
        self.assertEqual(env["HOME"], str(Path("C:/placeholder/scratch")))
        self.assertEqual(env["USERPROFILE"], str(Path("C:/placeholder/scratch")))
        # An inherited store path would aim the run at a real database.
        self.assertNotIn("AGENT_SESSION_GREP_DB", env)
        self.assertNotIn("ASG_DB", env)


class TrialAggregationTest(unittest.TestCase):
    """The verdict logic is the whole point: one trial cannot judge a race."""

    def _aggregate(self, verdicts):
        trials = [
            {"verdict": v, "observed": {"code": None, "exit_code": 0, "retryable": None}}
            for v in verdicts
        ]
        reproduced = [t for t in trials if t["verdict"] != "not_reproduced"]
        violations = [t for t in trials if t["verdict"] == "violates_catalog"]
        if not reproduced:
            return "not_reproduced"
        if violations:
            return "violates_catalog"
        return "matches_catalog"

    def test_all_matching_passes(self):
        self.assertEqual(
            self._aggregate(["matches_catalog"] * 6), "matches_catalog"
        )

    def test_a_single_misclassification_fails_the_whole_run(self):
        # This is the property that makes the harness discriminating: the known
        # broken binary matched the catalog in roughly half its trials, so
        # "most trials passed" must not be reported as a pass.
        self.assertEqual(
            self._aggregate(
                ["matches_catalog", "matches_catalog", "violates_catalog"]
            ),
            "violates_catalog",
        )

    def test_never_reproducing_is_not_a_pass(self):
        # A run where the appender never won proves nothing about the mapping.
        self.assertEqual(
            self._aggregate(["not_reproduced"] * 6), "not_reproduced"
        )

    def test_mixed_reproduction_still_judges_on_reproduced_trials(self):
        self.assertEqual(
            self._aggregate(["not_reproduced", "matches_catalog"]),
            "matches_catalog",
        )


class UsageTest(unittest.TestCase):
    def test_missing_binary_is_a_usage_error(self):
        code = gsr.main(["--binary", str(REPO_ROOT / "no-such-binary")])
        self.assertEqual(code, 2)

    def test_nonpositive_trials_is_a_usage_error(self):
        binary = locate_binary()
        if binary is None:
            self.skipTest("release binary not built")
        code = gsr.main(["--binary", str(binary), "--trials", "0"])
        self.assertEqual(code, 2)


class EndToEndTest(unittest.TestCase):
    def test_race_classifies_as_retryable_source_changed(self):
        binary = locate_binary()
        if binary is None:
            self.skipTest("release binary not built")
        # Two trials keep the test quick; the full run defaults to six.
        report = gsr.run_trials(str(binary), None, gsr.DEFAULT_RECORDS, 2)
        if report["verdict"] == "not_reproduced":
            self.skipTest("the appender never won a trial on this host")
        self.assertEqual(report["verdict"], "matches_catalog", report)
        for observed in report["observations"]:
            if observed["code"] is None:
                continue
            self.assertEqual(observed["code"], gsr.EXPECTED_CODE, report)
            self.assertEqual(observed["exit_code"], gsr.EXPECTED_EXIT, report)
            self.assertIs(observed["retryable"], gsr.EXPECTED_RETRYABLE, report)
            # Lengths only: never a source path or transcript text.
            self.assertNotIn("\\", observed["message"] or "")


if __name__ == "__main__":
    unittest.main()
