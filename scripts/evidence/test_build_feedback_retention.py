"""Meaningful task disposal contract tests; native mutation is separately opt-in."""
import copy
import ctypes
import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("build_feedback_retention_tests", Path(__file__).with_name("build_feedback_retention.py"))
assert SPEC is not None and SPEC.loader is not None
RETENTION = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RETENTION
SPEC.loader.exec_module(RETENTION)


def write(path, payload):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload), encoding="utf-8")
    return {"path": str(path), "sha256": RETENTION.sha256_file(path)}


class RetentionContracts(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.source.mkdir()
        self.target = self.root / "probe" / "targets"
        checker = RETENTION.MODULE_DIR / "build_feedback_retention.py"
        self.checker = {"path": str(checker), "sha256": RETENTION.sha256_file(checker), "bytes": checker.stat().st_size}
        tools = {}
        for name in ("runner", "statistics", "benchmark", "inventory"):
            path = RETENTION.MODULE_DIR / ("build_feedback_" + name + ".py")
            tools[name] = {"path": str(path), "sha256": RETENTION.sha256_file(path), "bytes": path.stat().st_size}
        self.key_path = self.root / "integrity.key"
        self.key_path.write_bytes(b"k" * 32)
        self.protocol = {"schema_version": 2, "phase": "capacity", "run_root": str(self.target.parent),
                         "source": {"root": str(self.source), "commit": "a" * 40, "tree": "b" * 40, "receipt_sha256": "d" * 64},
                         "baseline": {"receipt_sha256": "c" * 64}, "toolchain": {}, "configs": {}}
        approval = {"kind": "build-feedback-retention-approval", "schema_version": 2, "approved": True,
                    "source_receipt_sha256": "d" * 64, "baseline_receipt_sha256": "c" * 64,
                    "toolchain_sha256": RETENTION._canonical({}), "configs_sha256": RETENTION._canonical({}),
                    "run_root": self.protocol["run_root"], "checker": self.checker, "verification_tools": tools,
                    "disposable_target_paths": [str(self.target)], "protected_roots": [str(self.source)],
                    "allowed_source_roots": [str(self.source)], "allow_recent_new_outputs": True,
                    "scope": "validated-successful-and-cap-stopped-task-intermediates",
                    "flow": ["verify", "recycle", "match", "purge-exact", "verify-release", "receipt"],
                    "protected": ["source", "git-common-directory", "daily-cache", "existing-last-72-hours", "stores", "seeds", "quality-source-failures", "unrelated-recycle-items", "pagefile"],
                    "local_integrity_key": {"path": str(self.key_path), "bytes": 32, "sha256": RETENTION.sha256_file(self.key_path)}}
        ref = write(self.root / "approval.json", approval)
        self.protocol["retention_policy"] = {"mode": "sealed-verified-recycle", "approval": ref}
        self.lease = RETENTION.declare_output(self.protocol, self.target, self.root / "lease.json")

    def test_literal_new_target_lease_not_existing_recent_cache(self):
        self.target.mkdir(parents=True)
        self.assertEqual(RETENTION._safe_target(self.protocol, self.target, self.lease), self.target)
        with self.assertRaisesRegex(ValueError, "absent"):
            RETENTION.declare_output(self.protocol, self.target, self.root / "late-lease.json")

    def test_no_v1_or_inline_permission_fallback(self):
        protocol = copy.deepcopy(self.protocol)
        protocol["schema_version"] = 1
        with self.assertRaisesRegex(ValueError, "v2"):
            RETENTION.validate_policy(protocol)
        protocol = copy.deepcopy(self.protocol)
        protocol["retention_policy"]["approval"] = {"approved": True}
        with self.assertRaisesRegex(ValueError, "receipt"):
            RETENTION.validate_policy(protocol)

    def test_checker_and_verification_tools_independently_pinned(self):
        protocol = copy.deepcopy(self.protocol)
        approval_path = self.root / "approval.json"
        approval = json.loads(approval_path.read_text())
        approval["checker"]["path"] = str(self.root / "unrelated.py")
        protocol["retention_policy"]["approval"] = write(approval_path, approval)
        with self.assertRaisesRegex(ValueError, "checker"):
            RETENTION.validate_policy(protocol)
        approval_path = self.root / "approval.json"
        approval = json.loads(approval_path.read_text())
        approval["checker"] = self.checker
        approval["verification_tools"]["statistics"]["sha256"] = "0" * 64
        protocol = copy.deepcopy(self.protocol)
        protocol["retention_policy"]["approval"] = write(approval_path, approval)
        with self.assertRaisesRegex(ValueError, "tool changed"):
            RETENTION.validate_policy(protocol)

    def test_allowlist_lease_and_source_substitution_rejected(self):
        with self.assertRaisesRegex(ValueError, "allowlist"):
            RETENTION._safe_target(self.protocol, self.root / "other" / "targets", self.lease)
        lease_path = self.root / "lease.json"
        lease = json.loads(lease_path.read_text())
        lease["target"] = str(self.source)
        substituted = write(lease_path, RETENTION._signed({key: value for key, value in lease.items() if key != "local_integrity_hmac"}, self.protocol))
        with self.assertRaisesRegex(ValueError, "lease"):
            RETENTION._safe_target(self.protocol, self.target, substituted)

    @unittest.skipUnless(os.name == "nt", "native NTFS allocation required")
    def test_manifest_detects_content_and_hardlinks(self):
        self.target.mkdir(parents=True)
        binary = self.target / "probe.rmeta"
        binary.write_bytes(b"payload" * 1024)
        first = RETENTION.tree_manifest(self.target)
        self.assertGreater(first["allocated_bytes"], 0)
        binary.write_bytes(b"changed" * 1024)
        self.assertNotEqual(first, RETENTION.tree_manifest(self.target))
        internal_alias = self.target / "alias.rmeta"
        os.link(binary, internal_alias)
        # Cargo can publish internally linked aliases. They are safe while
        # every filesystem link is represented inside this disposable tree.
        aliases = [entry for entry in RETENTION.tree_manifest(self.target)["entries"]
                   if entry["identity"] == first["entries"][1]["identity"]]
        self.assertEqual(len(aliases), 2)
        self.assertEqual(RETENTION.tree_manifest(self.target)["allocated_bytes"],
                         first["allocated_bytes"])
        external_alias = self.root / "external-alias.rmeta"
        os.link(binary, external_alias)
        with self.assertRaisesRegex(ValueError, "hardlink"):
            RETENTION.tree_manifest(self.target)

    @unittest.skipUnless(os.name == "nt", "native NTFS allocation required")
    def test_unexpected_source_subtree_is_not_disposable(self):
        for index, name in enumerate(("crates", "CRATES", ".GIT")):
            path = self.target / name
            path.mkdir(parents=True)
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "source directory"):
                RETENTION.tree_manifest(self.target)
            path.rename(self.root / ("protected-" + str(index)))

    @unittest.skipUnless(os.name == "nt", "native NTFS required")
    def test_directory_change_late_in_full_scan_is_refused(self):
        self.target.mkdir(parents=True)
        binary = self.target / "first.rmeta"
        binary.write_bytes(b"first")
        reader = RETENTION._module("inventory")._new_metadata_reader()
        original = reader.read
        def changing(path, **kwargs):
            result = original(path, **kwargs)
            if Path(path) == binary and not (self.target / "late.txt").exists():
                (self.target / "late.txt").write_bytes(b"late")
            return result
        with patch.object(reader, "read", side_effect=changing), patch.object(RETENTION._module("inventory"), "_new_metadata_reader", return_value=reader), self.assertRaisesRegex(ValueError, "directory changed"):
            RETENTION.tree_manifest(self.target)

    @unittest.skipUnless(os.name == "nt", "native NTFS required")
    def test_directory_named_stream_is_refused(self):
        self.target.mkdir(parents=True)
        stream = Path(str(self.target) + ":named")
        stream.write_bytes(b"directory stream")
        with self.assertRaisesRegex(ValueError, "named stream"):
            RETENTION.tree_manifest(self.target)
        # Preserve the fixture stream until TemporaryDirectory owns test teardown.

    @unittest.skipUnless(os.name == "nt", "native NTFS required")
    def test_earlier_file_change_late_in_full_scan_is_refused(self):
        self.target.mkdir(parents=True)
        first, second = self.target / "a.rmeta", self.target / "b.rmeta"
        first.write_bytes(b"original")
        second.write_bytes(b"later")
        reader = RETENTION._module("inventory")._new_metadata_reader()
        original = reader.read
        changed = False
        def late_write(path, **kwargs):
            nonlocal changed
            metadata = original(path, **kwargs)
            if Path(path) == second and not changed:
                first.write_bytes(b"modified")
                changed = True
            return metadata
        with patch.object(reader, "read", side_effect=late_write), patch.object(RETENTION._module("inventory"), "_new_metadata_reader", return_value=reader), self.assertRaisesRegex(ValueError, "file changed during full scan"):
            RETENTION.tree_manifest(self.target)

    def test_required_log_cannot_be_inside_disposable_target(self):
        self.target.mkdir(parents=True)
        log = self.target / "stdout.jsonl"
        log.write_bytes(b"retained raw observations")
        reference = {"stdout_path": str(log), "stdout_sha256": RETENTION.sha256_file(log)}
        with self.assertRaisesRegex(ValueError, "must survive"):
            RETENTION._retained_refs(reference, self.target)

    def test_required_release_binary_cannot_be_inside_disposable_target(self):
        self.target.mkdir(parents=True)
        binary = self.target / "cli.exe"
        binary.write_bytes(b"retained candidate")
        reference = {"release_binary": {"path": str(binary), "sha256": RETENTION.sha256_file(binary)}}
        with self.assertRaisesRegex(ValueError, "must survive"):
            RETENTION._retained_refs(reference, self.target)

    def test_source_receipt_companion_and_lease_retained(self):
        source_receipt = write(self.root / "source.json", {"commit": "a" * 40})
        retained = RETENTION._retained_refs([
            {"receipt_path": source_receipt["path"], "receipt_sha256": source_receipt["sha256"]}, self.lease,
        ], self.target)
        self.assertEqual({entry["path"] for entry in retained}, {source_receipt["path"], self.lease["path"]})
        Path(source_receipt["path"]).write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "hash changed"):
            RETENTION._retained_refs({"receipt_path": source_receipt["path"], "receipt_sha256": source_receipt["sha256"]}, self.target)

    def test_protected_store_from_protocol_cannot_be_disposed(self):
        self.protocol["store_roots"] = [str(self.target)]
        self.lease = RETENTION.declare_output(self.protocol, self.target, self.root / "store-lease.json")
        self.target.mkdir(parents=True)
        with self.assertRaisesRegex(ValueError, "source/Git/cache overlap"):
            RETENTION._safe_target(self.protocol, self.target, self.lease)

    def _approve_fixture(self):
        approval_path = Path(self.protocol["retention_policy"]["approval"]["path"])
        approval = json.loads(approval_path.read_text())
        approval["purpose"] = "synthetic-recycle-lifecycle"
        self.protocol["retention_policy"]["approval"] = write(approval_path, approval)
        self.lease = RETENTION.declare_output(self.protocol, self.target, self.root / "fixture-lease.json")
        self.target.mkdir(parents=True)
        (self.target / "fixture.bin").write_bytes(RETENTION.FIXTURE_BYTES)

    @unittest.skipUnless(os.name == "nt", "native NTFS required")
    def test_exact_tiny_fixture_seal_never_qualifies_as_capacity_or_performance(self):
        self._approve_fixture()
        reference = RETENTION.create_fixture_seal(self.protocol, self.target, self.lease, self.root / "fixture-seal.json")
        seal = RETENTION._verify_seal(reference, self.protocol)
        self.assertEqual(seal["kind"], "build-feedback-recycle-fixture-seal")
        self.assertIs(seal["performance_evidence"], False)
        link = write(self.root / "fixture-link.json", {"seal": reference})
        with self.assertRaisesRegex(ValueError, "capacity seal cannot qualify"):
            RETENTION.verify_retained_arm(link, self.protocol, "baseline", 0, 0)

    @unittest.skipUnless(os.name == "nt", "native NTFS required")
    def test_fixture_refuses_extra_or_changed_files_and_missing_independent_purpose(self):
        self.target.mkdir(parents=True)
        (self.target / "fixture.bin").write_bytes(RETENTION.FIXTURE_BYTES)
        with self.assertRaisesRegex(ValueError, "synthetic lifecycle purpose"):
            RETENTION.create_fixture_seal(self.protocol, self.target, self.lease, self.root / "unapproved-seal.json")
        self.target.rename(self.root / "earlier-target")
        self._approve_fixture()
        extra = self.target / "unexpected.bin"
        extra.write_bytes(b"not disposable")
        with self.assertRaisesRegex(ValueError, "literal tiny"):
            RETENTION.create_fixture_seal(self.protocol, self.target, self.lease, self.root / "extra-seal.json")
        extra.rename(self.root / "retained-extra.bin")
        (self.target / "fixture.bin").write_bytes(b"wrong payload")
        with self.assertRaisesRegex(ValueError, "literal tiny"):
            RETENTION.create_fixture_seal(self.protocol, self.target, self.lease, self.root / "changed-seal.json")

    def test_local_observation_cannot_be_forged_by_rehashing_json(self):
        value = RETENTION._signed({"status": "complete", "count": 1}, self.protocol)
        RETENTION._verify_signed(value, self.protocol)
        value["count"] = 2
        with self.assertRaisesRegex(ValueError, "integrity"):
            RETENTION._verify_signed(value, self.protocol)

    def test_unrelated_bin_item_change_is_refused(self):
        before = [{"parsing_name": "unrelated", "identity": ["v", "i"]}]
        RETENTION._unrelated(before, before + [{"parsing_name": "owned"}])
        with self.assertRaisesRegex(ValueError, "unrelated"):
            RETENTION._unrelated(before, [])

    def test_capacity_quality_failure_not_eligible(self):
        ref = write(self.root / "probe.json", {"status": "capture_failed"})
        with patch.object(RETENTION._module("runner"), "_validate_protocol"), self.assertRaisesRegex(ValueError, "success/cap"):
            RETENTION.create_capacity_seal(self.protocol, ref, self.lease, self.root / "seal.json")

    def _capacity_fixture(self, status="capacity_limited"):
        self.target = self.root / "campaign" / "capacity" / "targets"
        self.protocol["run_root"] = str(self.target.parent.parent)
        approval_path = Path(self.protocol["retention_policy"]["approval"]["path"])
        approval = json.loads(approval_path.read_text())
        approval["run_root"] = self.protocol["run_root"]
        approval["disposable_target_paths"] = [str(self.target)]
        self.protocol["retention_policy"]["approval"] = write(approval_path, approval)
        self.protocol["capacity_probe"] = {"source_root": str(self.source), "owned_output_cap_bytes": 8 * RETENTION.GIB}
        self.protocol["configs"] = {"candidate": {"tool": {"kind": "native"}}}
        self.protocol["budget_policy"] = {"minimum_free_bytes": 40 * RETENTION.GIB}
        approval["configs_sha256"] = RETENTION._canonical(self.protocol["configs"])
        self.protocol["retention_policy"]["approval"] = write(approval_path, approval)
        self.lease = RETENTION.declare_output(self.protocol, self.target, self.root / "capacity-lease.json")
        self.target.mkdir(parents=True)
        logs = self.target.parent / "logs"
        logs.mkdir()
        stdout, stderr = logs / "stdout.jsonl", logs / "stderr.log"
        stdout.write_bytes(b'{"reason":"build-finished","success":true}\n')
        stderr.write_bytes(b"")
        stamp = "2026-10-19T00:00:00+00:00"
        reasons = ["owned_output_cap_reached"] if status == "capacity_limited" else []
        cargo = {"build_success": True, "build_finished": True, "artifacts": [], "diagnostics": [], "errors": []}
        capture = {"kind": "build-feedback-process", "schema_version": 1, "status": status,
                   "reasons": reasons, "argv": ["synthetic.exe", "check"], "cwd": str(self.source), "pid": 1,
                   "started_at": stamp, "finished_at": stamp, "duration_ms": 1.0, "returncode": 7 if reasons else 0,
                   "stdout_path": str(stdout), "stderr_path": str(stderr), "stdout_sha256": RETENTION.sha256_file(stdout),
                   "stderr_sha256": RETENTION.sha256_file(stderr), "memory_limit_bytes": 24 * RETENTION.GIB,
                   "peak_commit_bytes": 123, "process_tree_capture_complete": True, "external_processes": [],
                   "resource_samples": [{"disk_free_bytes": 48 * RETENTION.GIB, "budget_allowed": not reasons,
                                         "budget_reasons": reasons}]}
        if not reasons:
            capture["cargo"] = cargo
        capture_reference = write(logs / "capture.json", capture)
        allocation = write(self.target.parent / "target-allocation.json", {"complete": True, "errors": [], "allocated_bytes": 0})
        output_allocation = write(self.target.parent / "owned-allocation.json", {"complete": True, "errors": [], "allocated_bytes": 4096})
        receipt = {"kind": "build-feedback-capacity-observation", "schema_version": 2, "phase": "capacity", "status": status,
                   "reasons": reasons, "target_root": str(self.target), "source_root": str(self.source),
                   "source_commit": self.protocol["source"]["commit"], "source_tree": self.protocol["source"]["tree"],
                   "source_receipt_sha256": self.protocol["source"]["receipt_sha256"], "toolchain_sha256": RETENTION._canonical(self.protocol["toolchain"]),
                   "protocol_sha256": RETENTION._canonical(self.protocol), "process_tree_drained": True,
                   "source_verified": True, "allocation_complete": True, "capture_complete": True,
                   "performance_evidence": False, "completed_valid_pairs": 0, "jobs": [capture], "capture_receipt": capture_reference,
                   "config": self.protocol["configs"]["candidate"], "owned_output_cap_bytes": 8 * RETENTION.GIB,
                   "disposal_eligible": True, "cap_stop_quality_errors": [], "memory_limit_bytes": 24 * RETENTION.GIB, "peak_commit_bytes": 123,
                   "peak_owned_output_bytes": 8 * RETENTION.GIB if status == "capacity_limited" else 4096,
                   "final_job_write_bound_bytes": 8 * RETENTION.GIB if status == "capacity_limited" else 0,
                   "owned_peak_kind": "max-post-drain-allocation-and-job-write-bound",
                   "started_at": stamp, "finished_at": stamp, "minimum_disk_free_bytes": 48 * RETENTION.GIB,
                   "owned_allocation_receipt": allocation, "target_allocation_receipt": allocation, "owned_allocated_bytes": 0,
                   "task_output_allocation_receipt": output_allocation, "task_output_allocated_bytes": 4096}
        for entry in capture["resource_samples"]:
            entry["job_write_bound_bytes"] = receipt["final_job_write_bound_bytes"]
        capture_reference = write(self.target.parent / "logs" / "capture.json", capture)
        receipt["capture_receipt"] = capture_reference
        reference = write(self.target.parent / "capacity.json", receipt)
        return reference, receipt

    def test_capacity_cap_stop_seal_requires_real_retained_capture_and_guard_proof(self):
        reference, receipt = self._capacity_fixture()
        runner = RETENTION._module("runner")
        # This fixture exercises closure verification, not a measured Cargo run.
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "_verify_source"), patch.object(runner, "capacity_command", return_value=receipt["jobs"][0]["argv"]):
            seal = RETENTION.create_capacity_seal(self.protocol, reference, self.lease, self.root / "capacity-seal.json")
            retained = RETENTION._verify_seal(seal, self.protocol)
            self.assertEqual(retained["probe_receipt"]["status"], "capacity_limited")
            retained_paths = {entry["path"] for entry in retained["retained_files"]}
            self.assertIn(receipt["capture_receipt"]["path"], retained_paths)
            self.assertIn(receipt["task_output_allocation_receipt"]["path"], retained_paths)
            Path(receipt["jobs"][0]["stderr_path"]).write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "hash changed"):
                RETENTION._verify_seal(seal, self.protocol)

    @unittest.skipUnless(os.name == "nt", "native NTFS required")
    def test_capacity_success_rechecks_real_emitted_artifact_bytes(self):
        reference, receipt = self._capacity_fixture("ok")
        runner = RETENTION._module("runner")
        binary = self.target / "leaf.rmeta"
        binary.write_bytes(b"synthetic emitted artifact")
        stdout = Path(receipt["jobs"][0]["stdout_path"])
        stdout.write_text(json.dumps({"reason": "compiler-artifact", "filenames": [str(binary)], "executable": None,
                                    "fresh": False, "profile": {}, "manifest_path": str(self.source / "Cargo.toml")})
                          + '\n{"reason":"build-finished","success":true}\n', encoding="utf-8")
        receipt["jobs"][0]["stdout_sha256"] = RETENTION.sha256_file(stdout)
        receipt["jobs"][0]["cargo"] = runner.parse_cargo_json_file(stdout, self.target, self.source)
        receipt["capture_receipt"] = write(Path(receipt["capture_receipt"]["path"]), receipt["jobs"][0])
        allocated = RETENTION.tree_manifest(self.target)["allocated_bytes"]
        receipt["owned_allocated_bytes"] = allocated
        receipt["owned_allocation_receipt"] = write(Path(receipt["owned_allocation_receipt"]["path"]), {"complete": True, "errors": [], "allocated_bytes": allocated})
        receipt["target_allocation_receipt"] = receipt["owned_allocation_receipt"]
        receipt["task_output_allocated_bytes"] = allocated + 4096
        receipt["peak_owned_output_bytes"] = allocated + 4096
        receipt["task_output_allocation_receipt"] = write(Path(receipt["task_output_allocation_receipt"]["path"]), {"complete": True, "errors": [], "allocated_bytes": receipt["task_output_allocated_bytes"]})
        reference = write(Path(reference["path"]), receipt)
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "_verify_source"), patch.object(runner, "capacity_command", return_value=receipt["jobs"][0]["argv"]):
            seal = RETENTION.create_capacity_seal(self.protocol, reference, self.lease, self.root / "emitted-capacity-seal.json")
            self.assertEqual(RETENTION._verify_seal(seal, self.protocol)["tree"]["allocated_bytes"], allocated)
            binary.write_bytes(b"changed after capture")
            with self.assertRaisesRegex(ValueError, "successful Cargo proof"):
                RETENTION.create_capacity_seal(self.protocol, reference, self.lease, self.root / "changed-capacity-seal.json")

    def test_capacity_cap_stop_rejects_guard_flag_without_observed_guard_sample(self):
        reference, receipt = self._capacity_fixture()
        receipt["jobs"][0]["resource_samples"][0]["budget_reasons"] = []
        receipt["capture_receipt"] = write(Path(receipt["capture_receipt"]["path"]), receipt["jobs"][0])
        reference = write(Path(reference["path"]), receipt)
        runner = RETENTION._module("runner")
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "_verify_source"), patch.object(runner, "capacity_command", return_value=receipt["jobs"][0]["argv"]), self.assertRaisesRegex(ValueError, "unapproved capacity stop"):
            RETENTION.create_capacity_seal(self.protocol, reference, self.lease, self.root / "missing-guard-seal.json")

    def test_capacity_total_output_over_cap_cannot_be_hidden_by_small_target(self):
        reference, receipt = self._capacity_fixture()
        receipt["task_output_allocated_bytes"] = 9 * RETENTION.GIB
        receipt["task_output_allocation_receipt"] = write(Path(receipt["task_output_allocation_receipt"]["path"]), {"complete": True, "errors": [], "allocated_bytes": receipt["task_output_allocated_bytes"]})
        reference = write(Path(reference["path"]), receipt)
        runner = RETENTION._module("runner")
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "_verify_source"), patch.object(runner, "capacity_command", return_value=receipt["jobs"][0]["argv"]), self.assertRaisesRegex(ValueError, "total output allocation"):
            RETENTION.create_capacity_seal(self.protocol, reference, self.lease, self.root / "output-cap-seal.json")

    def test_capacity_success_without_any_emitted_artifact_is_refused(self):
        reference, receipt = self._capacity_fixture("ok")
        runner = RETENTION._module("runner")
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "_verify_source"), patch.object(runner, "capacity_command", return_value=receipt["jobs"][0]["argv"]), self.assertRaisesRegex(ValueError, "successful Cargo proof"):
            RETENTION.create_capacity_seal(self.protocol, reference, self.lease, self.root / "empty-cargo-seal.json")

    def test_capacity_cap_stop_cannot_hide_compiler_error_before_termination(self):
        reference, receipt = self._capacity_fixture()
        stdout = Path(receipt["jobs"][0]["stdout_path"])
        stdout.write_text(json.dumps({"reason": "compiler-message", "message": {"level": "error", "message": "build failed", "spans": []}}) + "\n", encoding="utf-8")
        receipt["jobs"][0]["stdout_sha256"] = RETENTION.sha256_file(stdout)
        receipt["capture_receipt"] = write(Path(receipt["capture_receipt"]["path"]), receipt["jobs"][0])
        reference = write(Path(reference["path"]), receipt)
        runner = RETENTION._module("runner")
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "_verify_source"), patch.object(runner, "capacity_command", return_value=receipt["jobs"][0]["argv"]), self.assertRaisesRegex(ValueError, "quality/capture failure"):
            RETENTION.create_capacity_seal(self.protocol, reference, self.lease, self.root / "error-before-cap-seal.json")

    def test_cap_stop_seals_log_of_unfinished_artifact_without_claiming_bytes(self):
        reference, receipt = self._capacity_fixture()
        capture = receipt["jobs"][0]
        output = Path(capture["stdout_path"])
        missing = self.target / "unfinished.rmeta"
        output.write_text(json.dumps({"reason": "compiler-artifact", "filenames": [str(missing)],
                                      "executable": None, "fresh": False, "profile": {}}) + "\n", encoding="utf-8")
        capture["stdout_sha256"] = RETENTION.sha256_file(output)
        receipt["capture_receipt"] = write(Path(receipt["capture_receipt"]["path"]), capture)
        reference = write(Path(reference["path"]), receipt)
        runner = RETENTION._module("runner")
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "_verify_source"), patch.object(runner, "capacity_command", return_value=capture["argv"]):
            seal_reference = RETENTION.create_capacity_seal(self.protocol, reference, self.lease, self.root / "unfinished-artifact-cap-seal.json")
            seal = RETENTION._verify_seal(seal_reference, self.protocol)
            self.assertEqual(seal["probe_receipt"]["status"], "capacity_limited")
            self.assertFalse(missing.exists())
            self.assertFalse(seal["probe_receipt"]["performance_evidence"])

    def test_capacity_peak_cannot_be_changed_after_observation(self):
        reference, receipt = self._capacity_fixture()
        receipt["peak_owned_output_bytes"] = 4096
        reference = write(Path(reference["path"]), receipt)
        runner = RETENTION._module("runner")
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "_verify_source"), patch.object(runner, "capacity_command", return_value=receipt["jobs"][0]["argv"]), self.assertRaisesRegex(ValueError, "peak differs"):
            RETENTION.create_capacity_seal(self.protocol, reference, self.lease, self.root / "wrong-peak-seal.json")

    def _full_fixture(self, status="ok", *, qualifier_stop=None):
        """Real files/Cargo decoding/NTFS; only source admission and commands are synthetic."""
        runner = RETENTION._module("runner")
        arm_root = self.root / "full-campaign" / "capacity"
        self.target = arm_root / "targets"
        config = {"tool": {"kind": "native"}, "debug": "full", "incremental": "on", "layout": "isolated"}
        self.protocol.update(run_root=str(arm_root.parent), pairs=0, allocations=[], jobs=2, test_threads=2,
                             configs={"baseline": config, "candidate": config},
                             predicted_growth_bytes=8 * RETENTION.GIB,
                             capacity_probe={"kind": "full-arm-and-qualification", "stage": "expanded", "arm": "baseline",
                                             "source_root": str(self.source), "owned_output_cap_bytes": 8 * RETENTION.GIB},
                             budget_policy={"minimum_free_bytes": 40 * RETENTION.GIB, "external_growth_allowance_bytes": 8 * RETENTION.GIB})
        self.protocol["source"].update(lock_sha256="e" * 64, resources_sha256="f" * 64)
        self.protocol["toolchain"] = {"cargo": {"path": str(self.root / "synthetic-cargo.exe"), "version": "cargo 1.97.1 (synthetic)", "sha256": "1" * 64},
                                      "rustc": {"path": str(self.root / "synthetic-rustc.exe"), "version": "rustc 1.97.1 (synthetic)", "sha256": "2" * 64},
                                      "msvc": {"compiler": {"version": "synthetic cl", "sha256": "3" * 64},
                                               "linker": {"version": "synthetic link", "sha256": "4" * 64}}, "target": "x86_64-pc-windows-msvc"}
        suite = self.source / "evidence-output/build-feedback/asg-build-feedback-capacity-capacity-0-baseline"
        self.protocol["qualification_allocations"] = [{"pair_index": None, "attempt": None, "arm": "baseline", "source_root": str(self.source), "release_suite_root": str(suite)}]
        approval_path = Path(self.protocol["retention_policy"]["approval"]["path"])
        approval = json.loads(approval_path.read_text())
        approval.update(run_root=self.protocol["run_root"], disposable_target_paths=[str(self.target)],
                        toolchain_sha256=RETENTION._canonical(self.protocol["toolchain"]), configs_sha256=RETENTION._canonical(self.protocol["configs"]))
        approval["producer_owned_scratch_cleanup_paths"] = [str(suite / "scratch")]
        # Each fixture is freshly admitted after other writers' current bytes.
        for name in approval["verification_tools"]:
            path = RETENTION.MODULE_DIR / ("build_feedback_" + name + ".py")
            approval["verification_tools"][name] = {"path": str(path), "sha256": RETENTION.sha256_file(path), "bytes": path.stat().st_size}
        self.protocol["retention_policy"]["approval"] = write(approval_path, approval)
        self.lease = RETENTION.declare_output(self.protocol, self.target, self.root / "full-lease.json")
        self.target.mkdir(parents=True)
        logs = arm_root / "logs"
        logs.mkdir()
        suite.mkdir(parents=True)
        stamp = "2026-10-20T00:00:00+00:00"
        origin = 1000
        cap = 8 * RETENTION.GIB
        fields = {"primary": "jobs", "feedback-warm": "feedback_warm_jobs", "edit": "edit_jobs"}
        raw = {"kind": "build-feedback-arm", "schema_version": 2, "phase": "capacity", "arm": "baseline", "pair_index": None, "attempt": None,
               "performance_evidence": False, "completed_valid_pairs": 0, "status": status,
               "reasons": [] if status == "ok" else ["owned_output_cap_reached"], "target_root": str(self.target), "source_root": str(self.source),
               "source_commit": self.protocol["source"]["commit"], "source_tree": self.protocol["source"]["tree"],
               "source_receipt_sha256": self.protocol["source"]["receipt_sha256"], "toolchain_sha256": RETENTION._canonical(self.protocol["toolchain"]),
               "protocol_sha256": RETENTION._canonical(self.protocol), "config": config, "output_lease": self.lease, "source_verified": True,
               "process_tree_drained": True, "own_edit_restored": True, "allocation_complete": True, "capture_complete": True, "disposal_eligible": True, "feedback_incremental": "on",
               "started_at": stamp, "finished_at": stamp, "duration_ms": 180.0, "primary_duration_ms": 60.0,
               "feedback_warm_duration_ms": 60.0, "edit_duration_ms": 60.0, "phase_duration_ms": {phase: 60.0 for phase in fields},
               "allocated_before_bytes": origin, "disk_free_before_bytes": 58 * RETENTION.GIB, "minimum_disk_free_bytes": 48 * RETENTION.GIB,
               "final_job_write_bound_bytes": cap if status != "ok" else 1024 ** 2,
               "resource_guard": {"kind": "windows-job-and-cumulative-owned-output-cap", "memory_limit_bytes": 24 * RETENTION.GIB,
                                  "owned_output_cap_bytes": cap, "minimum_free_bytes": 40 * RETENTION.GIB, "external_growth_allowance_bytes": 8 * RETENTION.GIB},
               "jobs": [], "feedback_warm_jobs": [], "edit_jobs": []}
        def capture(path, argv, *, name=None, stopped=False, budget=1024 ** 2):
            path.parent.mkdir(parents=True, exist_ok=True)
            stdout = path.with_suffix(".stdout")
            stderr = path.with_suffix(".stderr")
            stderr.write_bytes(b"")
            out = b'{"reason":"build-finished","success":true}\n'
            artifacts = []
            if argv[0] == self.protocol["toolchain"]["cargo"]["path"]:
                target = Path(argv[argv.index("--target-dir") + 1])
                target.mkdir(parents=True, exist_ok=True)
                artifact = target / "synthetic.rmeta"
                artifact.write_bytes(b"synthetic emitted artifact")
                event = {"reason": "compiler-artifact", "filenames": [str(artifact)], "executable": None, "fresh": False,
                         "profile": {}, "manifest_path": str(self.source / "Cargo.toml")}
                out = (json.dumps(event) + '\n' + ('{"reason":"build-finished","success":true}\n' if not stopped else '')).encode()
            stdout.write_bytes(out)
            reasons = ["owned_output_cap_reached"] if stopped else []
            value = {"kind": "build-feedback-process", "schema_version": 1, "status": "capacity_limited" if stopped else "ok", "reasons": reasons,
                     "returncode": 7 if stopped else 0, "argv": argv, "cwd": str(self.source), "pid": 1234,
                     "started_at": stamp, "finished_at": stamp, "duration_ms": 10.0, "memory_limit_bytes": 24 * RETENTION.GIB,
                     "peak_commit_bytes": 4096, "process_tree_capture_complete": True, "external_processes": [],
                     "stdout_path": str(stdout), "stdout_sha256": RETENTION.sha256_file(stdout), "stderr_path": str(stderr), "stderr_sha256": RETENTION.sha256_file(stderr),
                     "resource_samples": [{"allocated_bytes": origin + budget, "disk_free_bytes": 48 * RETENTION.GIB,
                                            "job_write_bound_bytes": budget, "allocation_kind": "conservative-disk-and-job-write-bound",
                                            "budget_allowed": not stopped, "budget_reasons": reasons}]}
            if name is not None:
                value["job"] = name
            if argv[0] == self.protocol["toolchain"]["cargo"]["path"] and not stopped:
                value["cargo"] = runner.parse_cargo_json_file(stdout, Path(argv[argv.index("--target-dir") + 1]), self.source)
                artifacts = [file for entry in value["cargo"]["artifacts"] for file in entry["files"]]
            return value, artifacts
        for phase, field in fields.items():
            if status != "ok" and phase != "primary":
                continue
            for fixed in runner.JOBS:
                target = self.target / "primary" / fixed["name"]
                value, _ = capture(logs / (phase + "-" + fixed["name"] + ".json"), runner.job_command(self.protocol, fixed, target),
                                   name=fixed["name"], stopped=status != "ok", budget=cap if status != "ok" else 1024 ** 2)
                write(logs / (phase + "-" + fixed["name"] + ".json"), value)
                raw[field].append(value)
        allocation = RETENTION._module("inventory").inventory([], store_roots=[arm_root, suite], discover=False, minimum_free_bytes=40 * RETENTION.GIB)
        allocation["storage_scope"] = "owned_targets_and_stores"
        raw["owned_allocated_bytes"] = allocation["allocated_bytes"]
        raw["owned_allocation_receipt"] = allocation
        raw["owned_allocation_receipt_sha256"] = write(arm_root / "owned-allocation.json", allocation)["sha256"]
        write(arm_root / "arm.json", raw)
        with patch.object(RETENTION._module("benchmark")._module("runner"), "_validate_protocol"):
            arm = RETENTION._module("benchmark").normalize_runner_arm(raw, self.protocol)
        qualifications = {}
        previous = max(raw["final_job_write_bound_bytes"], raw["owned_allocated_bytes"])
        if status == "ok":
            for index, qualification in enumerate(runner.QUALIFICATION_NAMES):
                stopped = qualification == qualifier_stop
                if qualification in ("clippy", "optional_features"):
                    argv = [self.protocol["toolchain"]["cargo"]["path"], "clippy" if qualification == "clippy" else "test", "--workspace", "--all-targets"]
                    if qualification == "optional_features":
                        argv.append("--all-features")
                    argv.extend(["--locked", "--offline", "-j2", "--message-format=json", "--target-dir", str(self.target / "qualification" / qualification)])
                    argv.extend(["--", "-D", "warnings"] if qualification == "clippy" else ["--", "--test-threads=2"])
                else:
                    argv = [str(self.root / "synthetic-diagnostic.exe"), qualification]
                path = arm_root / "qualification-captures" / qualification / "capture.json"
                budget = cap if stopped else previous + 1024 ** 2
                value, artifacts = capture(path, argv, stopped=stopped, budget=budget)
                value.update(qualification=qualification, process_tree_drained=True, source_root=str(self.source), arm="baseline", pair_index=None, attempt=None,
                             config_sha256=arm["config_sha256"], protocol_sha256=RETENTION._canonical(self.protocol),
                             final_job_write_bound_bytes=budget, minimum_disk_free_bytes=48 * RETENTION.GIB,
                             resource_guard={**raw["resource_guard"], "origin_allocated_bytes": origin, "origin_disk_free_bytes": raw["disk_free_before_bytes"],
                                             "prior_write_bound_bytes": previous, "prior_minimum_disk_free_bytes": 48 * RETENTION.GIB})
                value["qualification_budget"] = {**value["resource_guard"], "final_job_write_bound_bytes": budget,
                                                 "minimum_disk_free_bytes": value["minimum_disk_free_bytes"], "extra_owned_roots": []}
                final = RETENTION._module("inventory").inventory([], store_roots=[arm_root, suite], discover=False, minimum_free_bytes=40 * RETENTION.GIB)
                value["owned_allocation_receipt"] = write(path.parent / "owned-allocation.json", final)
                value["owned_allocated_bytes"] = final["allocated_bytes"]
                value["artifacts"] = artifacts
                write(path, value)
                proof = {"kind": "build-feedback-qualification-command", "schema_version": 1, "qualification": qualification, "identity": arm["identity"],
                         "protocol_sha256": RETENTION._canonical(self.protocol), "source_root": str(self.source), "config_sha256": arm["config_sha256"],
                         "pair_index": None, "attempt": None, "arm": "baseline", "capture": value, "artifacts": artifacts}
                report = {"kind": "build-feedback-qualification-report", "schema_version": 1, "qualification": qualification, "identity": arm["identity"],
                          "protocol_sha256": RETENTION._canonical(self.protocol), "status": "passed", "argv": argv, "exit_code": 0,
                          "stdout_sha256": value["stdout_sha256"], "stderr_sha256": value["stderr_sha256"], "capture_sha256": RETENTION._canonical(value),
                          "artifacts": artifacts, "checks": [{"status": "passed"}], "pair_index": None, "attempt": None, "arm": "baseline",
                          "source_root": str(self.source), "config_sha256": arm["config_sha256"]}
                proof["report"] = write(arm_root / "qualification-reports" / (qualification + ".json"), report)
                qualifications[qualification] = write(arm_root / "qualification-proofs" / (qualification + ".json"), proof)
                previous = max(budget, final["allocated_bytes"])
                if stopped:
                    break
        return raw, arm, qualifications

    def _full_seal_context(self):
        from contextlib import ExitStack
        stack = ExitStack()
        stack.enter_context(patch.object(RETENTION._module("runner"), "_validate_protocol"))
        stack.enter_context(patch.object(RETENTION._module("runner"), "_verify_source"))
        stack.enter_context(patch.object(RETENTION._module("statistics"), "_command_proof_valid", return_value=True))
        stack.enter_context(patch.object(RETENTION._module("statistics"), "_official_release_valid", return_value={}))
        stack.enter_context(patch.object(RETENTION._module("statistics"), "_release_workload_samples", return_value={}))
        return stack

    def test_full_capacity_success_seals_actual_eighteen_captures_and_all_qualifiers(self):
        raw, arm, qualifications = self._full_fixture()
        with self._full_seal_context():
            reference = RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "full-seal.json")
        with self._full_seal_context():
            seal = RETENTION._verify_seal(reference, self.protocol)
        self.assertTrue(seal["workload_complete"])
        self.assertTrue(seal["reservation_eligible"])
        self.assertFalse(seal["performance_evidence"])
        self.assertEqual(seal["pair_index"], None)
        self.assertEqual(seal["task_output_allocated_bytes"], RETENTION._proof(seal["task_output_allocation_receipt"])[1]["allocated_bytes"])
        self.assertEqual(RETENTION._proof(arm["evidence"])[1], raw)
        self.assertEqual(set(seal["qualifications"]), {"clippy", "diagnostics", "optional_features", "release"})

    def test_full_capacity_partial_cap_stop_never_supplies_reservation(self):
        raw, arm, qualifications = self._full_fixture("capacity_limited")
        with self._full_seal_context():
            reference = RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "partial-seal.json")
        with self._full_seal_context():
            seal = RETENTION._verify_seal(reference, self.protocol)
        self.assertFalse(seal["workload_complete"])
        self.assertFalse(seal["reservation_eligible"])
        self.assertTrue(seal["disposal_eligible"])
        self.assertEqual(seal["status"], "capacity_limited")
        self.assertEqual(len(raw["jobs"]), 6)
        self.assertEqual(raw["edit_jobs"], [])

    def test_full_capacity_qualifier_cap_stop_is_distinct_from_successful_arm(self):
        _, arm, qualifications = self._full_fixture(qualifier_stop="clippy")
        with self._full_seal_context():
            reference = RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "qualifier-stop-seal.json")
        with self._full_seal_context():
            seal = RETENTION._verify_seal(reference, self.protocol)
        self.assertEqual(seal["arm_receipt"]["status"], "ok")
        self.assertEqual(seal["status"], "capacity_limited")
        self.assertFalse(seal["reservation_eligible"])

    def test_full_capacity_missing_qualification_rejects_inline_success_flags(self):
        _, arm, qualifications = self._full_fixture()
        del qualifications["release"]
        arm.update(workload_complete=True, reservation_eligible=True, disposal_eligible=True)
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "complete artifact-dependent"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "invalid-seal.json")

    def test_full_capacity_complete_allocation_cannot_hide_report_and_log_bytes(self):
        _, arm, qualifications = self._full_fixture()
        inventory = RETENTION._module("inventory")
        with self._full_seal_context(), patch.object(inventory, "inventory", return_value={"complete": True, "errors": [], "allocated_bytes": 9 * RETENTION.GIB}), self.assertRaisesRegex(ValueError, "complete outputs exceed cap"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "over-cap-seal.json")

    def test_full_capacity_cap_stop_rejects_quality_error_and_fake_guard(self):
        raw, arm, qualifications = self._full_fixture("capacity_limited")
        capture = raw["jobs"][0]
        capture["resource_samples"][0].update(budget_allowed=True, budget_reasons=[])
        write(self.target.parent / "logs/primary-check.json", capture)
        arm["evidence"] = write(Path(arm["evidence"]["path"]), raw)
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "guard flag"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "fake-guard-seal.json")
        capture["resource_samples"][0].update(budget_allowed=False, budget_reasons=["owned_output_cap_reached"])
        stdout = Path(capture["stdout_path"])
        stdout.write_text('{"reason":"compiler-message","message":{"level":"error","rendered":"synthetic failure","spans":[]}}\n', encoding="utf-8")
        capture["stdout_sha256"] = RETENTION.sha256_file(stdout)
        write(self.target.parent / "logs/primary-check.json", capture)
        arm["evidence"] = write(Path(arm["evidence"]["path"]), raw)
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "quality failure"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "quality-seal.json")

    def test_full_capacity_qualification_cannot_reset_cumulative_origin(self):
        _, arm, qualifications = self._full_fixture()
        proof = RETENTION._proof(qualifications["clippy"])[1]
        proof["capture"]["resource_guard"]["origin_allocated_bytes"] += 1
        write(self.target.parent / "qualification-captures/clippy/capture.json", proof["capture"])
        qualifications["clippy"] = write(Path(qualifications["clippy"]["path"]), proof)
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "guard reset"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "reset-guard-seal.json")

    def test_full_capacity_pair_ordinals_or_source_path_cannot_be_substituted(self):
        raw, arm, qualifications = self._full_fixture()
        proof = RETENTION._proof(qualifications["clippy"])[1]
        proof["pair_index"] = 0
        qualifications["clippy"] = write(Path(qualifications["clippy"]["path"]), proof)
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "qualification allocation mismatch"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "wrong-pair-seal.json")
        raw["source_root"] = str(self.root / "unapproved-source")
        arm["evidence"] = write(Path(arm["evidence"]["path"]), raw)
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "source/lease"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, {}, self.lease, self.target.parent / "wrong-source-seal.json")

    def test_full_capacity_signed_success_flags_cannot_upgrade_partial_captures(self):
        _, arm, qualifications = self._full_fixture("capacity_limited")
        with self._full_seal_context():
            reference = RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "partial-flags-seal.json")
            seal = RETENTION._proof(reference)[1]
            forged = {key: value for key, value in seal.items() if key not in {"local_integrity_hmac", "closure_sha256"}}
            forged.update(status="ok", workload_complete=True, reservation_eligible=True)
            forged["closure_sha256"] = RETENTION._canonical(forged)
            reference = write(Path(reference["path"]), RETENTION._signed(forged, self.protocol))
            with self.assertRaisesRegex(ValueError, "eligibility mismatch"):
                RETENTION._verify_seal(reference, self.protocol)

    def test_full_capacity_signed_peak_must_replay_original_capture_bounds(self):
        _, arm, qualifications = self._full_fixture()
        with self._full_seal_context():
            reference = RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "peak-seal.json")
            seal = RETENTION._proof(reference)[1]
            forged = {key: value for key, value in seal.items() if key not in {"local_integrity_hmac", "closure_sha256"}}
            forged["observed_peak_bytes"] += 1
            forged["closure_sha256"] = RETENTION._canonical(forged)
            reference = write(Path(reference["path"]), RETENTION._signed(forged, self.protocol))
            with self.assertRaisesRegex(ValueError, "peak changed"):
                RETENTION._verify_seal(reference, self.protocol)

    def test_full_capacity_original_allocation_hash_and_pair_ordinals_are_required(self):
        raw, arm, qualifications = self._full_fixture("capacity_limited")
        raw["owned_allocation_receipt_sha256"] = "0" * 64
        arm["evidence"] = write(Path(arm["evidence"]["path"]), raw)
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "proof hash"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "wrong-allocation-seal.json")

    def test_full_capacity_retained_link_checks_actual_signed_disposition(self):
        _, arm, qualifications = self._full_fixture()
        with self._full_seal_context():
            seal_reference = RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "linked-seal.json")
            seal = RETENTION._verify_seal(seal_reference, self.protocol)
        # Simulate the loss of intermediates using a reversible rename only;
        # the native Shell mutation is tested by the coordinator separately.
        self.target.rename(self.root / "simulated-disposed-targets")
        root = self.target.parent / "disposition"
        item = {"parsing_name": "owned", "path": str(self.root / "bin-owned"), "identity": seal["tree"]["root_identity"]}
        stamp = "2026-10-20T00:00:00+00:00"
        def signed(name, value):
            return write(root / (name + ".json"), RETENTION._signed(value, self.protocol))
        move = signed("move-intent", {"seal": seal_reference, "target": str(self.target), "tree_sha256": RETENTION._canonical(seal["tree"]), "bin_before": [], "at": stamp})
        moved = signed("moved", {"seal": seal_reference, "item": item, "at": stamp})
        purge = signed("purge-intent", {"seal": seal_reference, "item": item, "bin_before": [item], "free_before_bytes": 1_000_000, "at": stamp})
        amount = seal["tree"]["allocated_bytes"]
        disposition = signed("completed", {"kind": "build-feedback-disposition", "schema_version": 1, "status": "complete", "seal": seal_reference,
                             "protocol_sha256": RETENTION._canonical(self.protocol), "target": str(self.target), "item": item,
                             "move_intent": move, "moved": moved, "purge_intent": purge, "source_absent": True, "bin_absent": True,
                             "release_verified": True, "allocated_bytes": amount, "free_after_bytes": 1_000_000 + amount,
                             "observed_free_delta_bytes": amount, "at": stamp})
        link = write(self.root / "capacity-link.json", {"seal": seal_reference, "disposition": disposition})
        with self._full_seal_context():
            result = RETENTION.verify_full_capacity_link(link, self.protocol)
            self.assertEqual(result["seal"], seal)
            self.assertEqual(result["disposition"]["status"], "complete")
            with self.assertRaisesRegex(ValueError, "capacity"):
                RETENTION.verify_retained_arm(link, self.protocol, "baseline", 0, 0)

    def test_full_capacity_pre_seal_command_and_qualification_verifiers_are_not_bypassed(self):
        _, arm, qualifications = self._full_fixture()
        stats = RETENTION._module("statistics")
        with self._full_seal_context(), patch.object(stats, "_command_proof_valid", return_value=False), self.assertRaisesRegex(ValueError, "qualification verification failed"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "false-qualifier-seal.json")
        with self._full_seal_context(), patch.object(stats, "_official_release_valid", side_effect=ValueError("official full report invalid")), self.assertRaisesRegex(ValueError, "official full report invalid"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "false-release-seal.json")

    def test_full_capacity_retained_logs_stay_required_after_sealing(self):
        _, arm, qualifications = self._full_fixture()
        with self._full_seal_context():
            reference = RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "retained-log-seal.json")
            raw = RETENTION._proof(arm["evidence"])[1]
            Path(raw["jobs"][0]["stdout_path"]).write_bytes(b"changed immutable log")
            with self.assertRaisesRegex(ValueError, "proof hash"):
                RETENTION._verify_seal(reference, self.protocol)

    def test_full_capacity_capture_scope_and_timing_cannot_narrow_workload(self):
        raw, arm, qualifications = self._full_fixture("capacity_limited")
        original = copy.deepcopy(raw["jobs"][0])
        for alteration in ({"argv": [part for part in original["argv"] if part != "--workspace"]},
                           {"started_at": "2026-10-21T00:00:00+00:00"}):
            raw["jobs"][0] = {**original, **alteration}
            write(self.target.parent / "logs/primary-check.json", raw["jobs"][0])
            arm["evidence"] = write(Path(arm["evidence"]["path"]), raw)
            with self._full_seal_context(), self.assertRaises(ValueError):
                RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "narrowed-seal.json")

    def test_full_capacity_signed_projection_cannot_change_raw_timings(self):
        _, arm, qualifications = self._full_fixture()
        with self._full_seal_context():
            reference = RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "timing-seal.json")
            seal = RETENTION._proof(reference)[1]
            forged = {key: value for key, value in seal.items() if key not in {"local_integrity_hmac", "closure_sha256"}}
            forged["arm_projection"]["duration_seconds"] = 0.0001
            forged["closure_sha256"] = RETENTION._canonical(forged)
            reference = write(Path(reference["path"]), RETENTION._signed(forged, self.protocol))
            with self.assertRaisesRegex(ValueError, "projection differs"):
                RETENTION._verify_seal(reference, self.protocol)

    def test_full_capacity_seal_must_remain_inside_charged_arm_root(self):
        _, arm, qualifications = self._full_fixture()
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "billed arm root"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.root / "external-full-seal.json")

    def test_full_capacity_qualifier_cargo_artifacts_and_billed_report_scope_are_required(self):
        _, arm, qualifications = self._full_fixture()
        proof = RETENTION._proof(qualifications["clippy"])[1]
        original = copy.deepcopy(proof)
        proof["artifacts"] = []
        qualifications["clippy"] = write(Path(qualifications["clippy"]["path"]), proof)
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "artifacts differ"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "missing-qualification-artifacts-seal.json")
        proof = copy.deepcopy(original)
        proof["report"] = write(self.root / "external-report.json", RETENTION._proof(original["report"])[1])
        qualifications["clippy"] = write(Path(qualifications["clippy"]["path"]), proof)
        with self._full_seal_context(), self.assertRaisesRegex(ValueError, "report outside billed"):
            RETENTION.create_full_capacity_seal(self.protocol, arm, qualifications, self.lease, self.target.parent / "external-report-seal.json")

    def test_capacity_cannot_become_performance_proof(self):
        with patch.object(RETENTION, "_verify_seal", return_value={"kind": "build-feedback-capacity-seal"}):
            ref = write(self.root / "link.json", {"seal": {"path": "ignored"}})
            with self.assertRaisesRegex(ValueError, "capacity"):
                RETENTION.verify_retained_arm(ref, self.protocol, "baseline", 0, 0)

    def test_disposal_resumes_after_move_without_repeating_mutation(self):
        self._exercise_disposal("move")

    def test_disposal_after_purge_without_durable_immediate_baseline_is_inconclusive(self):
        self._exercise_disposal("purge")

    def test_disposal_refuses_missing_recycle_identity(self):
        self._exercise_disposal("missing")

    def test_disposition_refuses_rehashed_complete_flag_and_cross_item_stages(self):
        seal_reference = {"path": str(self.root / "seal.json"), "sha256": "0" * 64}
        seal = {"target": str(self.target), "tree": {"entries": [], "allocated_bytes": 4096, "root_identity": ["v", "owned"]}}
        root = self.target.parent / "disposition"
        item = {"parsing_name": "owned", "path": str(self.root / "bin" / "owned"), "identity": ["v", "owned"]}
        stamp = "2026-10-19T00:00:00+00:00"
        def signed(name, payload):
            return write(root / (name + ".json"), RETENTION._signed(payload, self.protocol))
        move = {"seal": seal_reference, "target": str(self.target), "tree_sha256": RETENTION._canonical(seal["tree"]), "bin_before": [], "at": stamp}
        moved = {"seal": seal_reference, "item": item, "at": stamp}
        purge = {"seal": seal_reference, "item": item, "bin_before": [item], "free_before_bytes": 1000, "at": stamp}
        refs = {"move_intent": signed("move-intent", move), "moved": signed("moved", moved), "purge_intent": signed("purge-intent", purge)}
        result = {"kind": "build-feedback-disposition", "schema_version": 1, "status": "complete", "seal": seal_reference,
                  "protocol_sha256": RETENTION._canonical(self.protocol), "target": str(self.target), "item": item,
                  **refs, "release_verified": True, "source_absent": True, "bin_absent": True,
                  "allocated_bytes": 4096, "free_after_bytes": 5096, "observed_free_delta_bytes": 4096, "at": stamp}
        reference = signed("completed", result)
        self.assertEqual(RETENTION._verify_disposition(reference, seal_reference, seal, self.protocol)["status"], "complete")
        forged = copy.deepcopy(result)
        forged["observed_free_delta_bytes"] = 9000
        reference = write(root / "completed.json", forged)
        with self.assertRaisesRegex(ValueError, "integrity"):
            RETENTION._verify_disposition(reference, seal_reference, seal, self.protocol)
        reference = signed("completed", forged)
        with self.assertRaisesRegex(ValueError, "release accounting"):
            RETENTION._verify_disposition(reference, seal_reference, seal, self.protocol)
        changed = copy.deepcopy(moved)
        changed["item"]["identity"] = ["v", "unrelated"]
        result["moved"] = signed("moved", changed)
        reference = signed("completed", result)
        with self.assertRaisesRegex(ValueError, "target/item/tree"):
            RETENTION._verify_disposition(reference, seal_reference, seal, self.protocol)

    def test_disk_release_inconclusive_never_retries_delete(self):
        self._exercise_disposal("no-release")

    def test_disposal_uses_immediate_free_sample_after_real_intent_allocation(self):
        self._exercise_disposal("own-stage-writes")

    def _exercise_disposal(self, interruption):
        self.target.mkdir(parents=True)
        expected = {"entries": [], "allocated_bytes": 4096, "root_identity": ["volume", "owned"]}
        unrelated = {"parsing_name": "unrelated", "path": str(self.root / "unrelated"), "identity": ["v", "keep"]}
        item = {"parsing_name": "owned", "path": str(self.root / "bin" / "targets"), "identity": ["volume", "owned"]}
        owner = self
        class FakeShell:
            def __init__(self):
                self.bin = [unrelated]
                self.moves = self.purges = 0
                self.crashed = False
                self.free = 10_000
            def items(self):
                return copy.deepcopy(self.bin)
            def capacity(self, target, tree):
                return {"no_eviction_admitted": True}
            def recycle(self, target, *, expected_tree):
                owner.assertEqual(expected_tree, expected)
                self.moves += 1
                item_path = Path(item["path"])
                item_path.parent.mkdir()
                target.rename(item_path)
                if interruption != "missing":
                    self.bin.append(item)
                if interruption == "move" and not self.crashed:
                    self.crashed = True
                    raise OSError("simulated process lost after move")
                return {"pre": [{"path": str(target)}], "post": [{"hresult": 0, "created_parsing_name": "owned"}]}
            def purge(self, selected, *, expected_tree):
                owner.assertEqual(expected_tree, expected)
                self.purges += 1
                self.bin.remove(selected)
                # The fake models a side effect by moving an EMPTY owned fixture
                # out of its bin namespace. Native destructive flow is separate.
                Path(selected["path"]).rename(owner.root / "fake-purged-empty")
                if interruption != "no-release":
                    self.free += 4096
                if interruption == "purge" and not self.crashed:
                    self.crashed = True
                    raise OSError("simulated process lost after purge")
                return {"pre": [{"path": selected["path"]}], "post": [{"hresult": 0, "created_parsing_name": None}]}
        shell = FakeShell()
        actual_metadata = RETENTION._module("inventory")._new_metadata_reader()
        def observed_free(_):
            if interruption != "own-stage-writes":
                return shell.free
            # Real NTFS allocations from every flushed owned stage decrease the
            # available bytes, unlike the old constant-free fake.
            evidence = self.target.parent / "disposition"
            allocated = sum(actual_metadata.read(path).allocated_bytes
                            for path in evidence.iterdir() if path.is_file()) if evidence.exists() else 0
            return shell.free - allocated
        if interruption == "own-stage-writes":
            shell.free = 100_000
        reader = type("Reader", (), {"free_bytes": observed_free})()
        seal = {"target": str(self.target), "tree": expected}
        ref = {"path": str(self.root / "seal.json"), "sha256": "0" * 64}
        with patch.object(RETENTION, "_verify_seal", return_value=seal), patch.object(RETENTION, "tree_manifest", return_value=expected), \
             patch.object(RETENTION._module("runner"), "build_process_snapshot", return_value=[]), \
             patch.object(RETENTION._module("inventory"), "_new_metadata_reader", return_value=reader):
            if interruption in {"move", "purge"}:
                with self.assertRaisesRegex(OSError, "simulated"):
                    RETENTION.dispose_output(ref, self.protocol, self.target.parent / "disposition", backend=shell)
                if interruption == "purge":
                    with self.assertRaisesRegex(ValueError, "inconclusive"):
                        RETENTION.dispose_output(ref, self.protocol, self.target.parent / "disposition", backend=shell)
                    with self.assertRaisesRegex(ValueError, "completed disposition"):
                        RETENTION.dispose_output(ref, self.protocol, self.target.parent / "disposition", backend=shell)
                    self.assertEqual(shell.moves, 1)
                    self.assertEqual(shell.purges, 1)
                    return
                result = RETENTION.dispose_output(ref, self.protocol, self.target.parent / "disposition", backend=shell)
                self.assertEqual(json.loads(Path(result["path"]).read_text())["status"], "complete")
                self.assertEqual(shell.moves, 1)
                self.assertEqual(shell.purges, 1)
                self.assertEqual(shell.items(), [unrelated])
                self.assertEqual(RETENTION.dispose_output(ref, self.protocol, self.target.parent / "disposition", backend=shell), result)
            elif interruption == "missing":
                with self.assertRaisesRegex(ValueError, "exact recycle"):
                    RETENTION.dispose_output(ref, self.protocol, self.target.parent / "disposition", backend=shell)
                self.assertEqual(shell.purges, 0)
            elif interruption == "own-stage-writes":
                result = RETENTION.dispose_output(ref, self.protocol, self.target.parent / "disposition", backend=shell)
                payload = json.loads(Path(result["path"]).read_text())
                self.assertTrue(payload["release_verified"])
                self.assertEqual(payload["observed_free_delta_bytes"], 4096)
                self.assertLess(payload["free_before_purge_bytes"], payload["prepared_free_before_bytes"])
                self.assertEqual(shell.moves, 1)
                self.assertEqual(shell.purges, 1)
            else:
                with self.assertRaisesRegex(ValueError, "inconclusive"):
                    RETENTION.dispose_output(ref, self.protocol, self.target.parent / "disposition", backend=shell)
                with self.assertRaisesRegex(ValueError, "completed disposition"):
                    RETENTION.dispose_output(ref, self.protocol, self.target.parent / "disposition", backend=shell)
                self.assertEqual(shell.moves, 1)
                self.assertEqual(shell.purges, 1)

    @unittest.skipUnless(os.name == "nt", "native callback ABI required")
    def test_shell_callbacks_veto_wrong_item_and_disposition_without_mutation(self):
        self.target.mkdir(parents=True)
        metadata = RETENTION._module("inventory")._new_metadata_reader().read(self.target)
        manifest = {"entries": [{
            "relative": ".", "identity": list(metadata.identity),
            "directory": metadata.directory, "allocated_bytes": metadata.allocated_bytes,
            "bytes": metadata.logical_bytes, "modified": metadata.last_write,
        }], "allocated_bytes": 0, "root_identity": list(metadata.identity)}
        for mode in ("recycle", "permanent", "wrong-item", "bypass", "handler-error", "purge-alias"):
            shell = RETENTION.WindowsRecycleBin.__new__(RETENTION.WindowsRecycleBin)
            operation_state = {"sink": None, "flags": 0}
            shell._item = lambda name: ctypes.c_void_p(2)
            def shell_name(pointer, format, mode=mode):
                selected = pointer == 2 or getattr(pointer, "value", None) == 2
                if not selected:
                    return "created"
                if mode == "wrong-item":
                    return "unexpected"
                if mode == "purge-alias":
                    return "C:\\deleted-original" if format == 0x80058000 else str(self.target)
                return str(self.target)
            shell._name = shell_name
            shell.ole = type("Ole", (), {"CoCreateInstance": lambda _, clsid, outer, context, iid, output: self._set_pointer(output, 1)})()
            def invoke(pointer, index, args, *values, mode=mode, operation_state=operation_state, **kwargs):
                if index == 5:
                    operation_state["flags"] = values[0]
                elif index == 18:
                    operation_state["sink"] = values[1]
                elif index == 21:
                    sink = operation_state["sink"]
                    table = ctypes.cast(sink, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p))).contents
                    flags = 0 if mode in {"permanent", "purge-alias"} else 0x80
                    pre = ctypes.WINFUNCTYPE(ctypes.c_long, ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p)(table[11])
                    pre_result = pre(sink, flags, 2)
                    if pre_result < 0:
                        return pre_result
                    post = ctypes.WINFUNCTYPE(ctypes.c_long, ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_long, ctypes.c_void_p)(table[12])
                    return post(sink, flags, 2, 0, 3 if mode == "recycle" else None)
                elif index == 22:
                    ctypes.cast(values[0], ctypes.POINTER(ctypes.c_int))[0] = 0
                return 0
            inventory = RETENTION._module("inventory")
            reader = inventory._new_metadata_reader()
            scan = OSError("callback observation failed") if mode == "handler-error" else None
            with patch.object(RETENTION, "_call", side_effect=invoke), \
                 patch.object(inventory, "_new_metadata_reader", return_value=reader), \
                 patch.object(reader, "read", side_effect=scan, return_value=metadata):
                if mode in {"wrong-item", "bypass", "handler-error"}:
                    with self.assertLogs(RETENTION._LOGGER, level="ERROR"), self.assertRaises(OSError):
                        shell._operate(str(self.target), recycle=True, expected_path=str(self.target), expected_tree=manifest)
                else:
                    result = shell._operate(
                        str(self.target), recycle=mode == "recycle", expected_path=str(self.target),
                        expected_alias="C:\\deleted-original" if mode == "purge-alias" else None,
                        expected_tree=manifest)
                    self.assertEqual(len(result["pre"]), 1)
                    self.assertEqual(len(result["post"]), 1)
                    self.assertEqual(bool(operation_state["flags"] & 0x00080000), mode == "recycle")

    @staticmethod
    def _set_pointer(pointer, value):
        ctypes.cast(pointer, ctypes.POINTER(ctypes.c_void_p))[0] = value
        return 0

    @unittest.skipUnless(os.name == "nt", "native Shell required")
    def test_native_shell_enumeration_is_readonly(self):
        shell = RETENTION.WindowsRecycleBin()
        try:
            first = shell.items()
            second = shell.items()
            self.assertEqual(first, second)
        finally:
            shell.close()


if __name__ == "__main__":
    unittest.main()
