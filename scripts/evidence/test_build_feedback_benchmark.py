import contextlib
import copy
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

    def test_runner_compare_pins_exact_protocol_file_for_release_import(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            campaign_path, protocol_path = root / "campaign.json", root / "protocol.json"
            campaign_path.write_text('{"kind":"build-feedback-campaign"}', encoding="utf-8")
            protocol_path.write_text('{"source":{"commit":"synthetic"}}', encoding="utf-8")
            statistics = SimpleNamespace(compare_campaign=Mock(return_value={"qualified": False}))
            normalized = {"phase": "pilot"}
            with patch.object(CLI, "normalize_runner_campaign", return_value=normalized), patch.object(CLI, "_module", return_value=statistics):
                code, _, stderr = invoke(["compare", "--campaign", str(campaign_path), "--protocol", str(protocol_path)])
            self.assertEqual(code, 2)
            self.assertEqual(stderr, "")
            payload = statistics.compare_campaign.call_args.args[0]
            self.assertEqual(payload["protocol_evidence"], {
                "path": str(protocol_path.resolve()), "sha256": CLI.sha256_file(protocol_path),
            })

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


class SingleArmProjectionTests(unittest.TestCase):
    """Real pinned allocation/log/artifact fixtures; no Cargo or performance claim."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.runner = CLI._module("runner")
        self.protocol = {"schema_version": 2, "phase": "capacity", "pairs": 0, "allocations": [],
                         "run_root": str(self.root / "run"), "seed": 7, "order": [], "jobs": 2, "test_threads": 2,
                         "source": {"commit": "a" * 40, "tree": "b" * 40, "lock_sha256": "c" * 64, "resources_sha256": "d" * 64},
                         "toolchain": {"target": "x86_64-pc-windows-msvc"}}
        tool = {"version": "synthetic version", "sha256": "e" * 64}
        self.protocol["toolchain"].update(rustc=tool, cargo=tool, msvc={"compiler": tool, "linker": tool})
        source = self.root / "source"
        source.mkdir()
        self.protocol["capacity_probe"] = {"kind": "full-arm-and-qualification", "stage": "expanded",
                                          "arm": "baseline", "source_root": str(source)}
        config = {"tool": {"kind": "native"}, "debug": "full", "layout": "isolated", "incremental": "on"}
        self.protocol["configs"] = {"baseline": config, "candidate": config}
        arm_root = Path(self.protocol["run_root"]) / "capacity"
        target = arm_root / "targets"
        target.mkdir(parents=True)
        self.arm_path = arm_root / "arm.json"
        artifact = target / "synthetic.rmeta"
        artifact.write_bytes(b"synthetic compiler output")
        file_receipt = self.runner._artifact_identity(artifact)
        allocation = {"schema_version": 1, "complete": True, "errors": [],
                      "storage_scope": "owned_targets_and_stores", "allocated_bytes": 4096}
        allocation_path = arm_root / "owned-allocation.json"
        allocation_path.write_text(json.dumps(allocation), encoding="utf-8")
        self.arm = {"schema_version": 2, "kind": "build-feedback-arm", "arm": "baseline", "phase": "capacity",
                    "performance_evidence": False, "completed_valid_pairs": 0, "pair_index": None, "attempt": None, "status": "ok",
                    "source_root": str(source), "source_commit": self.protocol["source"]["commit"], "source_tree": self.protocol["source"]["tree"],
                    "config": config, "target_root": str(target), "own_edit_restored": True, "process_tree_drained": True,
                    "allocation_complete": True, "owned_allocated_bytes": allocation["allocated_bytes"],
                    "owned_allocation_receipt": allocation, "owned_allocation_receipt_sha256": CLI.sha256_file(allocation_path),
                    "primary_duration_ms": 6000, "duration_ms": 18000,
                    "phase_duration_ms": {"primary": 6000, "feedback-warm": 6000, "edit": 6000}}
        for phase, field in (("primary", "jobs"), ("feedback-warm", "feedback_warm_jobs"), ("edit", "edit_jobs")):
            captures = []
            for job in self.runner.JOBS:
                logs = arm_root / "logs" / phase / job["name"]
                logs.mkdir(parents=True)
                stdout, stderr = logs / "stdout.log", logs / "stderr.log"
                stdout.write_bytes(b"synthetic process log")
                stderr.write_bytes(b"")
                captures.append({"job": job["name"], "status": "ok", "returncode": 0, "duration_ms": 1000,
                                 "process_tree_capture_complete": True, "external_processes": [],
                                 "stdout_path": str(stdout), "stderr_path": str(stderr),
                                 "stdout_sha256": CLI.sha256_file(stdout), "stderr_sha256": CLI.sha256_file(stderr),
                                 "cargo": {"build_success": True, "errors": [], "artifacts": [{"files": [file_receipt]}]}})
            self.arm[field] = captures
        self.arm_path.write_text(json.dumps(self.arm), encoding="utf-8")
        self.validation = patch.object(self.runner, "_validate_protocol")
        self.validation.start()
        self.addCleanup(self.validation.stop)

    def test_single_arm_projection_reuses_original_campaign_projection(self):
        normalized = CLI.normalize_runner_arm(self.arm, self.protocol)
        paired_protocol = {**self.protocol, "phase": "pilot", "pairs": 5, "order": [["baseline", "candidate"]] * 5}
        campaign = {"status": "segment_complete", "complete": False, "inflight": None,
                    "pairs": [{"pair_index": 0, "attempt": 0, "order": paired_protocol["order"][0],
                               "status": "ok", "arms": {"baseline": self.arm}}]}
        campaign_path = Path(self.protocol["run_root"]) / "campaign.json"
        campaign_path.write_text(json.dumps(campaign), encoding="utf-8")
        with patch.object(self.runner, "_validate_resume"):
            projected = CLI.normalize_runner_campaign(campaign, paired_protocol, {})["records"][0]["baseline"]
        normalized["protocol_sha256"] = projected["protocol_sha256"]
        self.assertEqual(normalized, projected)
        self.assertTrue(normalized["capture_complete"])
        self.assertTrue(normalized["allocation_complete"])
        self.assertTrue(normalized["artifacts_verified"])
        self.assertNotIn("runner_campaign_evidence", normalized)
        self.assertNotIn("records", normalized)

    def test_full_capacity_projection_rejects_source_config_path_and_pair_claims(self):
        for mutation in ({"phase": "pilot"}, {"arm": "candidate"}, {"performance_evidence": True},
                         {"completed_valid_pairs": 1}, {"completed_valid_pairs": False},
                         {"pair_index": 0}, {"attempt": 0}, {"pair_index": False}, {"attempt": False},
                         {"source_root": str(self.root / "other-source")}, {"config": {"debug": "changed"}}):
            changed = {**self.arm, **mutation}
            self.arm_path.write_text(json.dumps(changed), encoding="utf-8")
            with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, "single full-capacity arm"):
                CLI.normalize_runner_arm(changed, self.protocol)
        for field in ("pair_index", "attempt"):
            changed = {key: value for key, value in self.arm.items() if key != field}
            self.arm_path.write_text(json.dumps(changed), encoding="utf-8")
            with self.subTest(missing=field), self.assertRaisesRegex(ValueError, "zero paired samples"):
                CLI.normalize_runner_arm(changed, self.protocol)
        self.arm_path.write_text(json.dumps(self.arm), encoding="utf-8")
        for mutation in ({"pairs": 1}, {"pairs": False}, {"allocations": [{}]}):
            with self.subTest(protocol=mutation), self.assertRaisesRegex(ValueError, "zero paired samples"):
                CLI.normalize_runner_arm(self.arm, {**self.protocol, **mutation})
        target = self.root / "unapproved" / "targets"
        target.mkdir(parents=True)
        relocated = {**self.arm, "target_root": str(target)}
        (target.parent / "arm.json").write_text(json.dumps(relocated), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "source/config/output allocation"):
            CLI.normalize_runner_arm(relocated, self.protocol)

    def test_single_arm_requires_exact_raw_bytes_and_preserves_incomplete_phase(self):
        self.arm_path.write_text(json.dumps({**self.arm, "status": "failed"}), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "exact immutable arm receipt"):
            CLI.normalize_runner_arm(self.arm, self.protocol)
        self.arm_path.write_text(json.dumps(self.arm), encoding="utf-8")
        zero_flag = {**self.arm, "performance_evidence": 0}
        self.assertIsNone(CLI._runner_arm_reference(zero_flag))
        with self.assertRaisesRegex(ValueError, "original live evidence"):
            CLI.normalize_runner_arm({**self.arm, "retained_evidence": {"verified": True}}, self.protocol)
        def change_receipt(*_):
            self.arm_path.write_text(json.dumps({**self.arm, "status": "failed"}), encoding="utf-8")
            return True
        with patch.object(CLI, "_runner_artifacts_verified", side_effect=change_receipt), \
             self.assertRaisesRegex(ValueError, "changed during normalization"):
            CLI.normalize_runner_arm(self.arm, self.protocol)
        partial = copy.deepcopy(self.arm)
        partial["edit_jobs"] = partial["edit_jobs"][:-1]
        self.arm_path.write_text(json.dumps(partial), encoding="utf-8")
        self.assertFalse(CLI.normalize_runner_arm(partial, self.protocol)["capture_complete"])
        Path(partial["jobs"][0]["stdout_path"]).write_bytes(b"changed log")
        self.assertFalse(CLI.normalize_runner_arm(partial, self.protocol)["artifacts_verified"])


class BoundedCliTests(unittest.TestCase):
    def test_v2_paired_cli_refuses_before_loading_or_allocating_runner(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "protocol.json"
            path.write_text(json.dumps({"schema_version": 2, "phase": "pilot"}), encoding="utf-8")
            with patch.object(CLI, "_module", side_effect=AssertionError("must refuse before runner import")):
                code, _, stderr = invoke(["run", "--protocol", str(path)])
            self.assertEqual(code, 2)
            self.assertIn("approved arm_finalizer", json.loads(stderr)["reason"])
            self.assertEqual(list(Path(directory).iterdir()), [path])

    def test_v2_capacity_normalizes_without_any_performance_samples(self):
        source = {"commit": "a" * 40, "tree": "b" * 40, "lock_sha256": "c" * 64, "resources_sha256": "d" * 64}
        tool = {"version": "synthetic version", "sha256": "e" * 64}
        protocol = {"schema_version": 2, "phase": "capacity", "pairs": 0, "source": source,
                    "toolchain": {"cargo": tool, "rustc": tool, "msvc": {"compiler": tool, "linker": tool}, "target": "x86_64-pc-windows-msvc"},
                    "jobs": 2, "test_threads": 2, "seed": 7, "order": [],
                    "configs": {name: {"tool": {"kind": "native"}} for name in ("baseline", "candidate")}}
        raw = {"pairs": [], "complete": True, "status": "complete", "inflight": None,
               "capacity_observation": {"status": "capacity_limited", "peak_owned_output_bytes": 4096}}
        fake = SimpleNamespace(_validate_protocol=Mock(), _validate_resume=Mock(), canonical_sha256=Mock(return_value="f" * 64),
                               JOBS=[{"name": name} for name in ("check", "domain-ports", "cli", "sqlite", "application", "clippy")])
        with tempfile.TemporaryDirectory() as directory:
            protocol["run_root"] = directory
            raw_path = Path(directory) / "campaign.json"
            raw_path.write_text(json.dumps(raw), encoding="utf-8")
            with patch.object(CLI, "_module", return_value=fake):
                normalized = CLI.normalize_runner_campaign(raw, protocol, {})
            self.assertEqual(normalized["runner_campaign_evidence"], {"path": str(raw_path), "sha256": CLI.sha256_file(raw_path)})
        self.assertEqual(normalized["schema_version"], 2)
        self.assertEqual(normalized["records"], [])
        self.assertEqual(normalized["pairs_required"], 0)
        self.assertFalse(normalized["performance_evidence"])
        self.assertEqual(normalized["capacity_observation"]["status"], "capacity_limited")

    def test_v2_sealed_reference_map_uses_original_arm_and_exact_projection(self):
        jobs = ["check", "domain-ports", "cli", "sqlite", "application", "clippy"]
        tool = {"version": "synthetic version", "sha256": "c" * 64}
        protocol = {"schema_version": 2, "source": {"commit": "a" * 40, "tree": "b" * 40, "lock_sha256": "d" * 64, "resources_sha256": "e" * 64},
                    "toolchain": {"rustc": tool, "cargo": tool, "msvc": {"compiler": tool, "linker": tool}, "target": "x86_64-pc-windows-msvc"},
                    "phase": "pilot", "pairs": 5, "jobs": 2, "test_threads": 2, "seed": 7, "order": [["baseline", "candidate"]] * 5,
                    "configs": {name: {"tool": {"kind": "native"}} for name in ("baseline", "candidate")}}
        runner = SimpleNamespace(canonical_sha256=Mock(return_value="f" * 64), _validate_resume=Mock(), _validate_protocol=Mock(), JOBS=[{"name": name} for name in jobs])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            protocol["run_root"] = str(root)
            allocation = {"schema_version": 1, "complete": True, "errors": [], "storage_scope": "owned_targets_and_stores", "allocated_bytes": 8192}
            allocation_path = root / "owned-allocation.json"
            allocation_path.write_text(json.dumps(allocation), encoding="utf-8")
            captures = [{"job": name, "status": "ok", "returncode": 0, "duration_ms": 1000,
                         "process_tree_capture_complete": True, "external_processes": []} for name in jobs]
            arm = {"status": "ok", "source_commit": protocol["source"]["commit"], "source_tree": protocol["source"]["tree"],
                   "source_root": str(root / "source"), "process_tree_drained": True, "own_edit_restored": True,
                   "primary_duration_ms": 6000, "duration_ms": 18000, "owned_allocated_bytes": 8192,
                   "allocation_complete": True, "owned_allocation_receipt": allocation,
                   "owned_allocation_receipt_sha256": CLI.sha256_file(allocation_path), "target_root": str(root / "targets"),
                   "phase_duration_ms": {"primary": 6000, "feedback-warm": 6000, "edit": 6000}, "config": {},
                   "jobs": captures, "feedback_warm_jobs": captures, "edit_jobs": captures}
            (root / "arm.json").write_text(json.dumps(arm), encoding="utf-8")
            pair = {"pair_index": 0, "attempt": 0, "order": protocol["order"][0], "status": "ok", "arms": {"baseline": arm}}
            raw = {"status": "complete", "complete": True, "pairs": [pair], "inflight": None}
            (root / "campaign.json").write_text(json.dumps(raw), encoding="utf-8")
            with patch.object(CLI, "_module", return_value=runner), patch.object(CLI, "_runner_artifacts_verified", return_value=True):
                live = CLI.normalize_runner_campaign(raw, protocol, {})
            projection = live["records"][0]["baseline"]
            reference = {"path": str(root / "retained-link.json"), "sha256": "e" * 64}
            pair["retained_evidence"] = {"baseline": reference}
            (root / "campaign.json").write_text(json.dumps(raw), encoding="utf-8")
            retained = {"arm_receipt": arm, "seal": {"arm_projection": projection}}
            helper = SimpleNamespace(verify_retained_arm=Mock(return_value=retained))
            with patch.object(CLI, "_module", side_effect=lambda name: runner if name == "runner" else helper), \
                 patch.object(CLI, "_runner_artifacts_verified", side_effect=AssertionError("deleted artifact must use approved helper")):
                sealed = CLI.normalize_runner_campaign(raw, protocol, {})
                self.assertEqual(sealed["records"][0]["baseline"]["retained_evidence"], reference)
                helper.verify_retained_arm.assert_called_once_with(reference, protocol, "baseline", 0, 0)
                retained["seal"]["arm_projection"] = {**projection, "allocated_bytes": 1}
                with self.assertRaisesRegex(ValueError, "prerecycle sealed projection"):
                    CLI.normalize_runner_campaign(raw, protocol, {})

    def test_v1_normalization_never_accepts_retained_boolean(self):
        tool = {"version": "synthetic version", "sha256": "c" * 64}
        protocol = {"schema_version": 1, "source": {"commit": "a" * 40, "tree": "b" * 40, "lock_sha256": "d" * 64, "resources_sha256": "e" * 64},
                    "toolchain": {"cargo": tool, "rustc": tool, "msvc": {"compiler": tool, "linker": tool}, "target": "x86_64-pc-windows-msvc"},
                    "jobs": 2, "test_threads": 2, "phase": "pilot", "pairs": 5, "seed": 7,
                    "order": [["baseline", "candidate"]] * 5}
        fake = SimpleNamespace(_validate_protocol=Mock(), _validate_resume=Mock(), canonical_sha256=Mock(return_value="f" * 64), JOBS=[])
        raw = {"pairs": [{"pair_index": 0, "attempt": 0, "order": protocol["order"][0], "status": "ok", "arms": {"baseline": {}}, "retained_evidence": {"baseline": {"verified": True}}}]}
        with patch.object(CLI, "_module", return_value=fake), self.assertRaisesRegex(ValueError, "protocol v2"):
            CLI.normalize_runner_campaign(raw, protocol, {})


if __name__ == "__main__":
    unittest.main()
