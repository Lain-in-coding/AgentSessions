#!/usr/bin/env python3
"""Schema gate controls; no CLI binary or network is needed by these unit tests."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from referencing.exceptions import Unresolvable

SPEC = importlib.util.spec_from_file_location("verify_robot_schema", Path(__file__).with_name("verify-robot-schema.py"))
assert SPEC and SPEC.loader
schema_gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(schema_gate)


def response() -> dict:
    return {
        "schema_version": "1.1", "frame_type": "response", "command": "search",
        "request_id": "schema-test", "ok": True, "outcome": "success",
        "data": {"hits": [], "generation": 1, "truncation": {"truncated": False, "reason": None}},
        "retrieval_mode": "lexical",
        "redaction": {"mode": "default", "status": "none", "ruleset_version": "test", "redacted_count": 0},
        "warnings": [], "page": {"next_cursor": None, "has_more": False},
        "meta": {"duration_ms": 0, "generation": 1},
    }


def progress(kind: str = "progress") -> dict:
    return {"schema_version": "1.1", "frame_type": kind, "command": "search",
            "request_id": "schema-test", "message": "synthetic"}


class RobotSchemaGateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.validator = schema_gate.load_validator()

    def validate(self, *frames: dict, outcome: str = "success") -> list[dict]:
        return schema_gate.validate_output(self.validator, "\n".join(json.dumps(frame, ensure_ascii=False) for frame in frames), "schema-test", outcome, "search")

    def test_valid_response_and_progress_stream(self) -> None:
        self.assertEqual(len(self.validate(progress(), response())), 2)
        self.assertEqual(len(self.validate(response())), 1)

    def test_error_and_partial_outcomes(self) -> None:
        partial = response()
        partial["outcome"] = "partial"
        self.validate(partial, outcome="partial")
        error = {key: value for key, value in response().items()
                 if key not in {"data", "retrieval_mode", "redaction"}}
        error.update(frame_type="error", ok=False, outcome="failure",
                     error={"code": "invalid_request", "message": "synthetic", "retryable": False, "details": {}})
        self.validate(error, outcome="failure")

    def test_schema_rejects_wrong_version_missing_required_and_wrong_type(self) -> None:
        for mutation in ("version", "required", "type", "nested", "extra"):
            with self.subTest(mutation=mutation):
                frame = response()
                if mutation == "version": frame["schema_version"] = "1.0"
                if mutation == "required": del frame["redaction"]
                if mutation == "type": frame["page"]["has_more"] = "false"
                if mutation == "nested": frame["data"]["hits"] = [{"id": "msg_v1_example"}]
                if mutation == "extra": frame["unexpected"] = True
                with self.assertRaisesRegex(schema_gate.VerificationError, "violates Robot"):
                    self.validate(frame)

    def test_unicode_separators_inside_json_strings_are_not_record_boundaries(self) -> None:
        for separator in ("\u0085", "\u2028", "\u2029"):
            with self.subTest(separator=repr(separator)):
                frame = response()
                frame["warnings"] = ["before" + separator + "after"]
                self.assertEqual(self.validate(frame), [frame])

    def test_changed_command_cannot_bypass_the_search_schema(self) -> None:
        frame = response()
        frame["command"] = "context"
        frame["data"]["hits"] = [{"id": "msg_v1_example", "score": "not-a-number"}]
        with self.assertRaisesRegex(schema_gate.VerificationError, "command"):
            self.validate(frame)

    def test_search_payload_accepts_only_declared_effective_modes(self) -> None:
        for mode in ("lexical", "semantic", "hybrid", "lexical_fallback"):
            frame = response()
            frame["data"]["retrieval_mode"] = mode
            self.validate(frame)
        frame["data"]["retrieval_mode"] = "not-a-mode"
        with self.assertRaisesRegex(schema_gate.VerificationError, "violates Robot"):
            self.validate(frame)

    def test_protocol_rejects_nonjson_missing_or_multiple_terminals(self) -> None:
        for output in ("", "human noise", json.dumps(progress()),
                       json.dumps(response()) + "\n" + json.dumps(response()),
                       json.dumps(response()) + "\n" + json.dumps(progress())):
            with self.subTest(output=output):
                with self.assertRaises(schema_gate.VerificationError):
                    schema_gate.validate_output(self.validator, output, "schema-test", "success", "search")

    def test_nonfinite_numbers_are_not_json(self) -> None:
        frame = json.dumps(response()).replace('"duration_ms": 0', '"duration_ms": NaN')
        with self.assertRaisesRegex(schema_gate.VerificationError, "not JSON"):
            schema_gate.validate_output(self.validator, frame, "schema-test", "success", "search")

    def test_request_correlation_and_expected_outcome_are_checked(self) -> None:
        frame = response()
        frame["request_id"] = "another-request"
        with self.assertRaisesRegex(schema_gate.VerificationError, "correlation"):
            self.validate(frame)
        with self.assertRaisesRegex(schema_gate.VerificationError, "outcome"):
            self.validate(response(), outcome="partial")

    def test_diagnostic_is_schema_fixture_only_not_a_live_terminal(self) -> None:
        fixture = progress("diagnostic")
        self.validator.validate(fixture)
        with self.assertRaises(schema_gate.VerificationError):
            self.validate(fixture)
        with self.assertRaises(schema_gate.VerificationError):
            self.validate(fixture, response())

    def test_historical_10_schema_remains_frozen(self) -> None:
        old = json.loads((schema_gate.ROOT / "schemas/robot/v1/envelope.schema.json").read_text(encoding="utf-8"))
        content = json.dumps(old, sort_keys=True, separators=(",", ":")).encode()
        self.assertEqual(hashlib.sha256(content).hexdigest(), "1436884edfe014f57056faf7717619eb19b6be0812f4b81efb8a85daa8d85e2d")

    def test_production_loader_refuses_external_retrieval(self) -> None:
        for uri in ("https://schema.invalid/remote", "file:///synthetic-schema.json"):
            with self.subTest(uri=uri), tempfile.TemporaryDirectory() as directory:
                schema = Path(directory) / "schema.json"
                schema.write_text(json.dumps({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "$id": "https://schema.invalid/local", "$ref": uri,
                }), encoding="utf-8")
                with mock.patch("urllib.request.urlopen", side_effect=AssertionError("retrieval forbidden")) as retrieve:
                    validator = schema_gate.load_validator(schema)
                    with self.assertRaises(Unresolvable):
                        validator.validate({})
                    retrieve.assert_not_called()

    def test_progress_command_must_match_the_terminal_scenario(self) -> None:
        frame = progress()
        frame["command"] = "context"
        with self.assertRaisesRegex(schema_gate.VerificationError, "command"):
            self.validate(frame, response())

    def test_physical_crlf_records_are_accepted(self) -> None:
        frames = schema_gate.validate_output(self.validator,
            json.dumps(progress()) + "\r\n" + json.dumps(response()) + "\r\n",
            "schema-test", "success", "search")
        self.assertEqual(len(frames), 2)

    def test_missing_binary_is_an_error_not_a_skip(self) -> None:
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(schema_gate.subprocess, "run") as run:
            with self.assertRaisesRegex(schema_gate.VerificationError, "binary does not exist"):
                schema_gate.verify(Path(directory) / "missing-asg")
            run.assert_not_called()

    def test_dependency_version_is_pinned(self) -> None:
        with mock.patch.object(schema_gate, "version", return_value="0.0"):
            with self.assertRaisesRegex(schema_gate.VerificationError, "jsonschema==4.26.0"):
                schema_gate.load_validator()

    def test_child_environment_does_not_change_parent_or_inherit_user_asg_settings(self) -> None:
        with tempfile.TemporaryDirectory() as directory, mock.patch.dict(os.environ, {"ASG_UNTRUSTED_TEST_SETTING": "synthetic"}):
            before = os.environ.copy()
            env = schema_gate.isolated_environment(Path(directory))
            self.assertEqual(os.environ, before)
            self.assertNotIn("ASG_UNTRUSTED_TEST_SETTING", env)
            self.assertEqual(env["ASG_CURRENT_REPO"], "")
            for key in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"):
                self.assertTrue(Path(env[key]).is_relative_to(Path(directory)))
                self.assertTrue(Path(env[key]).is_dir())


if __name__ == "__main__":
    unittest.main()
