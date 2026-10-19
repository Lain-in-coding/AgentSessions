import contextlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

MODULE_PATH = Path(__file__).with_name("build_feedback_benchmark.py")
SPEC = importlib.util.spec_from_file_location("build_feedback_benchmark", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
CLI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLI)


def invoke(args):
    stdout, stderr = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
        code = CLI.main(args)
    return code, stdout.getvalue(), stderr.getvalue()


class BuildFeedbackCliTests(unittest.TestCase):
    def test_help_loads_no_runner_or_windows_api(self):
        with patch.object(CLI, "_module", side_effect=AssertionError("unexpected import")), self.assertRaises(SystemExit) as stopped:
            invoke(["--help"])
        self.assertEqual(stopped.exception.code, 0)

    def test_inventory_requires_explicit_roots_and_honest_completion(self):
        code, _, stderr = invoke(["inventory"])
        self.assertEqual(code, 2)
        self.assertIn("explicit root", stderr)
        fake = SimpleNamespace(inventory=Mock(return_value={"schema_version": 1, "complete": False, "errors": ["read_failed"]}))
        with patch.object(CLI, "_module", return_value=fake):
            code, stdout, _ = invoke(["inventory", "--root", "synthetic-target", "--store-root", "synthetic-store", "--no-discover"])
        self.assertEqual(code, 2)
        self.assertFalse(json.loads(stdout)["complete"])
        fake.inventory.assert_called_once_with([Path("synthetic-target")], store_roots=[Path("synthetic-store")], discover=False)

    def test_preview_is_read_only_and_returns_blocked_budget(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            protocol, inventory = root / "protocol.json", root / "inventory.json"
            protocol.write_text('{"schema_version": 1}', encoding="utf-8")
            inventory.write_text('{"complete": false}', encoding="utf-8")
            before = {path.name: path.read_bytes() for path in root.iterdir()}
            fake = SimpleNamespace(preview_protocol=Mock(return_value={"allowed": False, "reasons": ["inventory_incomplete"]}))
            with patch.object(CLI, "_module", return_value=fake):
                code, stdout, stderr = invoke(["preview", "--protocol", str(protocol), "--inventory", str(inventory)])
            self.assertEqual(code, 2)
            self.assertEqual(stderr, "")
            self.assertEqual(json.loads(stdout)["reasons"], ["inventory_incomplete"])
            self.assertEqual(before, {path.name: path.read_bytes() for path in root.iterdir()})

    def test_run_forwards_only_explicit_review_and_segment_limit(self):
        with tempfile.TemporaryDirectory() as directory:
            protocol = Path(directory) / "protocol.json"
            review = Path(directory) / "review.json"
            for path in (protocol, review):
                path.write_text("{}", encoding="utf-8")
            fake = SimpleNamespace(run_protocol=Mock(return_value={"complete": False, "status": "paused"}))
            with patch.object(CLI, "_module", return_value=fake):
                code, stdout, _ = invoke(["run", "--protocol", str(protocol), "--review-receipt", str(review), "--pair-limit", "5"])
            self.assertEqual(code, 2)
            self.assertEqual(json.loads(stdout)["status"], "paused")
            fake.run_protocol.assert_called_once_with(protocol, review_receipt_path=review, pair_limit=5)
            with patch.object(CLI, "_module", side_effect=AssertionError("must not start runner")):
                code, _, _ = invoke(["run", "--protocol", str(protocol), "--pair-limit", "100"])
            self.assertEqual(code, 2)

    def test_existing_output_is_preserved_before_any_work(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "previous.json"
            output.write_text("previous evidence", encoding="utf-8")
            with patch.object(CLI, "_module", side_effect=AssertionError("must not start scan")):
                code, _, stderr = invoke(["inventory", "--root", "synthetic-target", "--output", str(output)])
            self.assertEqual(code, 2)
            self.assertIn("already exists", stderr)
            self.assertEqual(output.read_text(encoding="utf-8"), "previous evidence")

    def test_json_duplicate_keys_and_nonfinite_values_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "invalid.json"
            for text, error in (('{"complete": true, "complete": false}', ValueError),
                                ('{"duration": NaN}', ValueError), ('[]', TypeError)):
                path.write_text(text, encoding="utf-8")
                with self.subTest(text=text), self.assertRaises(error):
                    CLI.read_json(path)
            with patch.object(CLI, "_module", side_effect=AssertionError("invalid JSON must not load runner")):
                code, stdout, stderr = invoke(["preview", "--protocol", str(path), "--inventory", str(path)])
            self.assertEqual(code, 2)
            self.assertEqual(stdout, "")
            self.assertEqual(json.loads(stderr)["reason"], "receipt must be a JSON object")

    def test_compare_hashes_exact_campaign_and_returns_nonzero_when_unqualified(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "campaign.json"
            path.write_text('{"schema_version":1,"phase":"pilot"}', encoding="utf-8")
            fake = SimpleNamespace(compare_campaign=Mock(return_value={"qualified": False, "phase": "pilot", "reasons": ["pilot_only"]}))
            with patch.object(CLI, "_module", return_value=fake):
                code, stdout, stderr = invoke(["compare", "--campaign", str(path)])
            self.assertEqual(code, 2)
            self.assertEqual(stderr, "")
            result = json.loads(stdout)
            self.assertEqual(result["campaign_sha256"], CLI.sha256_file(path))
            self.assertNotIn("adopted", result)


    def test_runner_campaign_requires_protocol(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "campaign.json"
            path.write_text('{"kind":"build-feedback-campaign"}', encoding="utf-8")
            with patch.object(CLI, "_module", side_effect=AssertionError("cannot import stats without protocol")):
                code, _, stderr = invoke(["compare", "--campaign", str(path)])
            self.assertEqual(code, 2)
            self.assertIn("requires --protocol", stderr)

    def test_runner_normalization_preserves_attempts_and_missing_store_proof(self):
        jobs = ["check", "domain-ports", "cli", "sqlite", "application", "clippy"]
        tool_identity = {"version": "synthetic version", "sha256": "c" * 64}
        protocol = {
            "source": {"commit": "a" * 40, "tree": "b" * 40, "lock_sha256": "d" * 64, "resources_sha256": "e" * 64},
            "toolchain": {"rustc": tool_identity, "cargo": tool_identity, "msvc": {"compiler": tool_identity, "linker": tool_identity}, "target": "x86_64-pc-windows-msvc"},
            "phase": "pilot", "pairs": 5, "jobs": 2, "test_threads": 2, "seed": 7,
            "order": [["baseline", "candidate"]] * 5,
            "configs": {"baseline": {"tool": {"kind": "native"}}, "candidate": {"tool": {"kind": "native"}}},
        }
        arm = {
            "status": "ok", "source_commit": protocol["source"]["commit"], "source_tree": protocol["source"]["tree"],
            "process_tree_drained": True, "own_edit_restored": True,
            "primary_duration_ms": 6000, "duration_ms": 12000, "target_allocated_bytes": 4096,
            "jobs": [{"job": name, "status": "ok", "returncode": 0, "duration_ms": 1000,
                      "process_tree_capture_complete": True, "external_processes": [],
                      "cargo": {"build_success": True, "errors": []}} for name in jobs],
        }
        raw = {"status": "segment_complete", "complete": False, "pairs": [
            {"pair_index": 0, "attempt": 0, "order": protocol["order"][0], "status": "contaminated", "arms": {}},
            {"pair_index": 0, "attempt": 1, "order": protocol["order"][0], "status": "ok", "arms": {"baseline": arm, "candidate": arm}},
        ]}
        fake = SimpleNamespace(canonical_sha256=Mock(return_value="f" * 64), _validate_resume=Mock(), _validate_protocol=Mock(), JOBS=[{"name": name} for name in jobs])
        with patch.object(CLI, "_module", return_value=fake):
            normalized = CLI.normalize_runner_campaign(raw, protocol, {})
        fake._validate_resume.assert_called_once_with(raw, protocol, "f" * 64)
        self.assertEqual(len(normalized["records"]), 2)
        self.assertEqual(normalized["records"][0]["status"], "contaminated")
        self.assertEqual(normalized["records"][1]["status"], "valid")
        self.assertFalse(normalized["records"][1]["candidate"]["allocation_complete"])
        self.assertEqual(normalized["records"][1]["candidate"]["elapsed_seconds"], 12)
        self.assertIn("runner_campaign_incomplete", normalized["admission_reasons"])
        arm["owned_allocated_bytes"] = 8192
        arm["allocation_complete"] = True
        arm["edit_jobs"] = arm["jobs"]
        with patch.object(CLI, "_module", return_value=fake):
            covered = CLI.normalize_runner_campaign(raw, protocol, {})
        covered_arm = covered["records"][1]["candidate"]
        self.assertEqual(covered_arm["allocated_bytes"], 8192)
        self.assertFalse(covered_arm["capture_complete"])
        self.assertFalse(covered_arm["allocation_complete"])
        arm["feedback_warm_jobs"] = arm["jobs"]
        with tempfile.TemporaryDirectory() as directory:
            arm_root = Path(directory)
            arm["target_root"] = str(arm_root / "targets")
            allocation = {"schema_version": 1, "complete": True, "errors": [],
                          "storage_scope": "owned_targets_and_stores", "allocated_bytes": 8192}
            arm["owned_allocation_receipt"] = allocation
            allocation_path = arm_root / "owned-allocation.json"
            allocation_path.write_text(json.dumps(allocation), encoding="utf-8")
            arm["owned_allocation_receipt_sha256"] = CLI.sha256_file(allocation_path)
            with patch.object(CLI, "_module", return_value=fake), patch.object(CLI, "_runner_artifacts_verified", return_value=True) as artifacts:
                complete = CLI.normalize_runner_campaign(raw, protocol, {})
            self.assertEqual(artifacts.call_count, 2)
            self.assertEqual([len(call.args[0]) for call in artifacts.call_args_list], [18, 18])
            complete_arm = complete["records"][1]["candidate"]
            self.assertTrue(complete_arm["allocation_complete"])
            self.assertTrue(complete_arm["capture_complete"])
            arm["feedback_warm_jobs"] = arm["jobs"][:-1]
            with patch.object(CLI, "_module", return_value=fake):
                missing_warm = CLI.normalize_runner_campaign(raw, protocol, {})
            self.assertFalse(missing_warm["records"][1]["candidate"]["capture_complete"])
            arm["feedback_warm_jobs"] = arm["jobs"]
            allocation_path.unlink()
            with patch.object(CLI, "_module", return_value=fake):
                missing_receipt = CLI.normalize_runner_campaign(raw, protocol, {})
            self.assertFalse(missing_receipt["records"][1]["candidate"]["allocation_complete"])


    def test_owned_allocation_file_and_payload_must_match_recorded_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            arm_root = Path(directory)
            receipt_path = arm_root / "owned-allocation.json"
            allocation = {"schema_version": 1, "complete": True, "errors": [],
                          "storage_scope": "owned_targets_and_stores", "allocated_bytes": 8192}
            arm = {"target_root": str(arm_root / "targets"), "allocation_complete": True,
                   "owned_allocated_bytes": 8192, "owned_allocation_receipt": allocation}
            receipt_path.write_text(json.dumps(allocation), encoding="utf-8")
            arm["owned_allocation_receipt_sha256"] = CLI.sha256_file(receipt_path)
            self.assertTrue(CLI._runner_allocation_verified(arm))
            receipt_path.write_text("{}", encoding="utf-8")
            self.assertFalse(CLI._runner_allocation_verified(arm))
            arm["owned_allocation_receipt_sha256"] = CLI.sha256_file(receipt_path)
            self.assertFalse(CLI._runner_allocation_verified(arm))
            receipt_path.write_text(json.dumps(allocation), encoding="utf-8")
            arm["owned_allocation_receipt_sha256"] = CLI.sha256_file(receipt_path)
            arm["owned_allocated_bytes"] = 4096
            self.assertFalse(CLI._runner_allocation_verified(arm))
            arm["owned_allocated_bytes"] = 8192
            receipt_path.unlink()
            self.assertFalse(CLI._runner_allocation_verified(arm))

    def test_artifact_check_rechecks_current_logs_and_invocation_file_receipts(self):
        with tempfile.TemporaryDirectory() as directory:
            stdout, stderr = Path(directory) / "stdout.log", Path(directory) / "stderr.log"
            stdout.write_text("synthetic Cargo JSON", encoding="utf-8")
            stderr.write_bytes(b"")
            file_receipt = {"path": "synthetic-artifact", "sha256": "a" * 64}
            measurement = {
                "stdout_path": str(stdout), "stderr_path": str(stderr),
                "stdout_sha256": CLI.sha256_file(stdout), "stderr_sha256": CLI.sha256_file(stderr),
                "cargo": {"build_success": True, "errors": [], "artifacts": [{"files": [file_receipt]}]},
            }
            runner = SimpleNamespace(verify_artifact_receipt=Mock())
            self.assertTrue(CLI._runner_artifacts_verified([measurement], runner))
            runner.verify_artifact_receipt.assert_called_once_with(file_receipt)
            newer = dict(measurement)
            newer_receipt = {"path": "synthetic-artifact", "sha256": "b" * 64}
            newer["cargo"] = {"build_success": True, "errors": [], "artifacts": [{"files": [newer_receipt]}]}
            runner.verify_artifact_receipt.reset_mock()
            self.assertTrue(CLI._runner_artifacts_verified([measurement, newer], runner))
            runner.verify_artifact_receipt.assert_called_once_with(newer_receipt)
            stdout.write_text("changed after invocation", encoding="utf-8")
            self.assertFalse(CLI._runner_artifacts_verified([measurement], runner))
            measurement["stdout_sha256"] = CLI.sha256_file(stdout)
            runner.verify_artifact_receipt.side_effect = ValueError("artifact changed")
            self.assertFalse(CLI._runner_artifacts_verified([measurement], runner))

    def test_arm_proof_hash_only_accepts_matching_on_disk_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            arm = {"target_root": str(root / "targets"), "status": "ok"}
            self.assertIsNone(CLI._runner_arm_reference(arm))
            receipt = root / "arm.json"
            receipt.write_text(json.dumps(arm), encoding="utf-8")
            reference = CLI._runner_arm_reference(arm)
            self.assertEqual(reference, {"path": str(receipt), "sha256": CLI.sha256_file(receipt)})
            receipt.write_text(json.dumps({**arm, "status": "failed"}), encoding="utf-8")
            self.assertIsNone(CLI._runner_arm_reference(arm))

    def test_missing_protocol_does_not_import_or_run_process_capture(self):
        with patch.object(CLI, "_module", side_effect=AssertionError("must not load runner")):
            code, _, stderr = invoke(["run", "--protocol", "missing-synthetic-protocol.json"])
        self.assertEqual(code, 2)
        self.assertTrue(stderr)


if __name__ == "__main__":
    unittest.main()
