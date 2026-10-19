import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import math
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

MODULE_PATH = Path(__file__).with_name("build_feedback_statistics.py")
SPEC = importlib.util.spec_from_file_location("build_feedback_statistics", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
STATS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STATS)

JOBS = ["check", "domain-ports", "cli", "sqlite", "application", "clippy"]


def campaign_fixture(phase="formal", kind="tool", ratio=0.8, storage_ratio=0.8):
    """Synthetic local proof fixtures only; never evidence of an actual build."""
    count = {"pilot": 5, "formal": 100}[phase]
    identity = {
        "source_commit": "a" * 40, "source_tree": "b" * 40,
        "lock_sha256": "c" * 64, "resources_sha256": "d" * 64,
        "rustc_sha256": "e" * 64, "cargo_sha256": "f" * 64,
        "msvc_compiler_version": "synthetic MSVC compiler",
        "msvc_linker_version": "synthetic MSVC linker",
        "msvc_compiler_sha256": "3" * 64, "msvc_linker_sha256": "4" * 64,
        "rustc_version": "rustc 1.97.1 (synthetic)",
        "cargo_version": "cargo 1.97.1 (synthetic)",
        "host": "x86_64-pc-windows-msvc", "jobs": 2, "test_threads": 2,
    }
    protocol_hash = "1" * 64
    schedule = [["baseline", "candidate"] if index % 2 == 0 else ["candidate", "baseline"] for index in range(count)]
    records = []
    for index in range(count):
        arms = {}
        for name, scale, storage in (("baseline", 1.0, 1.0), ("candidate", ratio, storage_ratio)):
            arms[name] = {
                "status": "success", "identity": copy.deepcopy(identity),
                "protocol_sha256": protocol_hash,
                "capture_complete": True, "source_verified": True,
                "artifacts_verified": True, "allocation_complete": True, "quiet": True,
                "storage_scope": "owned_targets_and_stores",
                "duration_seconds": 60.0 * scale,
                "allocated_bytes": int(10 * STATS.GIB * storage),
                "jobs": {job: {"status": "success", "exit_code": 0, "duration_seconds": 10.0 * scale} for job in JOBS},
            }
        records.append({"pair_index": index, "attempt": 0, "status": "valid", "order": schedule[index], **arms})
    campaign = {
        "schema_version": 1, "phase": phase, "pairs_required": count,
        "comparison_kind": kind, "identity": identity,
        "protocol_sha256": protocol_hash, "seed": 20261018,
        "job_names": JOBS, "schedule": schedule,
        "records": records, "qualification_receipts": {},
        "pilot_store_peak_allocated_bytes": 10 * STATS.GIB,
    }
    attach_synthetic_proofs(campaign)
    return campaign


_ACTIVE_PROOF_ROOTS: list[tempfile.TemporaryDirectory] = []


def write_proof(path, payload):
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(payload, bytes):
        path.write_bytes(payload)
    else:
        path.write_text(json.dumps(payload, sort_keys=True), encoding="utf-8")
    return {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def capture_fixture(root, argv, output, *, duration=10000):
    stdout = write_proof(root / "stdout.log", output)
    stderr = write_proof(root / "stderr.log", b"")
    return {
        "schema_version": 1, "kind": "build-feedback-process", "status": "ok", "returncode": 0,
        "argv": argv, "cwd": str(root), "pid": 1234, "started_at": "synthetic-start", "finished_at": "synthetic-end",
        "duration_ms": duration, "process_tree_capture_complete": True, "external_processes": [],
        "memory_limit_bytes": 24 * STATS.GIB, "peak_commit_bytes": 4096,
        "stdout_path": stdout["path"], "stdout_sha256": stdout["sha256"],
        "stderr_path": stderr["path"], "stderr_sha256": stderr["sha256"],
    }


def attach_synthetic_proofs(campaign):
    temporary = tempfile.TemporaryDirectory(prefix="build-feedback-proof-test-")
    _ACTIVE_PROOF_ROOTS.append(temporary)
    root = Path(temporary.name)
    identity, protocol_hash = campaign["identity"], campaign["protocol_sha256"]
    validator = write_proof(root / "core_beta_benchmark.py", b"# synthetic validator fixture, never executed\n")
    for record in campaign["records"]:
        for name in ("baseline", "candidate"):
            arm = record[name]
            local = root / f"pair-{record['pair_index']}" / name
            source = write_proof(local / "source.rs", b"// synthetic immutable source\n")
            artifact = write_proof(local / "targets" / "synthetic.exe", b"synthetic artifact bytes")
            allocation = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": arm["allocated_bytes"],
                          "storage_scope": "owned_targets_and_stores"}
            allocation_ref = write_proof(local / "owned-allocation.json", allocation)
            config = {"name": name, "synthetic": True}
            arm["source_root"] = str(local)
            arm["config_sha256"] = hashlib.sha256(json.dumps(config, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
            proof = {"schema_version": 1, "kind": "build-feedback-arm", "arm": name, "status": "ok", "source_root": str(local),
                     "source_commit": identity["source_commit"], "source_tree": identity["source_tree"], "source_verified": True,
                     "own_edit_restored": True, "process_tree_drained": True, "feedback_incremental": "on", "config": config,
                     "target_root": str(local / "targets"), "owned_allocation_receipt": allocation,
                     "owned_allocation_receipt_sha256": allocation_ref["sha256"], "owned_allocated_bytes": arm["allocated_bytes"],
                     "primary_duration_ms": arm["duration_seconds"] * 1000, "edit_duration_ms": 10000,
                     "feedback_warm_duration_ms": 10000,
                     "phase_duration_ms": {"primary": arm["duration_seconds"] * 1000, "feedback-warm": 10000, "edit": 10000}}
            for phase, field in (("primary", "jobs"), ("feedback-warm", "feedback_warm_jobs"), ("edit", "edit_jobs")):
                entries = []
                for job in JOBS:
                    output = (json.dumps({"reason": "compiler-artifact", "filenames": [artifact["path"]]}) + "\n"
                              + json.dumps({"reason": "build-finished", "success": True}) + "\n").encode()
                    duration = arm["jobs"][job]["duration_seconds"] * 1000 if phase == "primary" else 10000
                    capture = capture_fixture(local / phase / job, ["synthetic-cargo", "check", "--locked", "--offline", "-j2"], output, duration=duration)
                    capture.update(job=job, cargo={"build_success": True, "errors": [], "artifacts": [{"files": [artifact]}]})
                    entries.append(capture)
                proof[field] = entries
            arm["feedback_phases"] = {phase: {"duration_seconds": 10.0, "jobs": {
                entry["job"]: {"status": "success", "exit_code": 0, "duration_seconds": entry["duration_ms"] / 1000}
                for entry in proof[field]}} for phase, field in (("feedback-warm", "feedback_warm_jobs"), ("edit", "edit_jobs"))}
            arm["evidence"] = write_proof(local / "arm.json", proof)
    for qualification in STATS.QUALIFICATIONS:
        samples = []
        selected = campaign["records"] if qualification in ("edit", "release") else campaign["records"][:1]
        for record in selected:
            for name in ("baseline", "candidate"):
                arm = record[name]
                sample = {"pair_index": record["pair_index"], "attempt": record["attempt"], "arm": name}
                if qualification == "edit":
                    sample["evidence"] = arm["evidence"]
                else:
                    local = Path(arm["source_root"]) / qualification
                    source = {"path": str(Path(arm["source_root"]) / "source.rs")}
                    source["sha256"] = hashlib.sha256(Path(source["path"]).read_bytes()).hexdigest()
                    artifact = {"path": str(Path(arm["source_root"]) / "targets" / "synthetic.exe")}
                    artifact["sha256"] = hashlib.sha256(Path(artifact["path"]).read_bytes()).hexdigest()
                    argv = ["synthetic-cargo", "clippy" if qualification == "clippy" else "test", "--all-targets", "--locked", "--offline", "-j2"]
                    if qualification == "optional_features":
                        argv.append("--all-features")
                    if qualification == "release":
                        argv = ["synthetic-python", validator["path"]]
                    output = f"{Path(source['path']).as_posix()}:1: synthetic diagnostic\n".encode()
                    capture = capture_fixture(local, argv, output)
                    report = {"schema_version": 1, "kind": "build-feedback-qualification-report", "qualification": qualification,
                              "identity": identity, "protocol_sha256": protocol_hash, "status": "passed", "argv": argv,
                              "exit_code": 0, "stdout_sha256": capture["stdout_sha256"], "stderr_sha256": capture["stderr_sha256"],
                              "checks": [{"status": "passed", "source": source, "expected_line": 1, "actual_line": 1, "actual_file": source["path"]}],
                              "validator": "core_beta_benchmark", "build_environment_validated": True, "release_profile_unchanged": True}
                    report.update(**sample, source_root=arm["source_root"], config_sha256=arm["config_sha256"], artifacts=[artifact],
                                  capture_sha256=hashlib.sha256(json.dumps(capture, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest())
                    proof = {"schema_version": 1, "kind": "build-feedback-qualification-command", "qualification": qualification,
                             "identity": identity, "protocol_sha256": protocol_hash, "source_root": arm["source_root"],
                             "config_sha256": arm["config_sha256"], **sample, "capture": capture, "artifacts": [artifact],
                             "report": write_proof(local / "report.json", report), "validator": validator}
                    sample["evidence"] = write_proof(local / "command.json", proof)
                samples.append(sample)
        receipt = {"schema_version": 1, "kind": "build-feedback-qualification", "qualification": qualification,
                   "identity": identity, "protocol_sha256": protocol_hash, "status": "passed", "complete": True, "samples": samples}
        campaign["qualification_receipts"][qualification] = write_proof(root / f"{qualification}.json", receipt)





class BootstrapTests(unittest.TestCase):
    def test_nearest_rank_is_not_interpolated_median(self):
        self.assertEqual(STATS.nearest_rank([1, 2, 3, 4], 50), 2)
        self.assertEqual(STATS.nearest_rank(range(1, 101), 95), 95)
        self.assertEqual(STATS.nearest_rank([9], 95), 9)

    def test_invalid_numbers_and_percentiles_are_rejected(self):
        for values, error in (([], ValueError), ([math.nan], ValueError), ([math.inf], ValueError),
                              ([-1], ValueError), ([True], TypeError), (["1"], TypeError), ([None], TypeError)):
            with self.subTest(values=values), self.assertRaises(error):
                STATS.summarize(values)
        for percentile, error in ((0, ValueError), (101, ValueError), (math.nan, ValueError), (True, TypeError)):
            with self.subTest(percentile=percentile), self.assertRaises(error):
                STATS.nearest_rank([1, 2], percentile)
        for before, after in (([0], [1]), ([1, 2], [1]), ([1], [math.inf])):
            with self.subTest(before=before, after=after), self.assertRaises(ValueError):
                STATS.paired_bootstrap(before, after, seed=1)

    def test_pair_relationship_is_preserved_under_resampling(self):
        before = [1.0, 20.0, 300.0, 4000.0, 50000.0]
        measured = STATS.paired_bootstrap(before, [value / 2 for value in before], seed=7, resamples=300)
        for estimator in ("p50", "p95"):
            self.assertEqual(measured[estimator]["ratio"], 0.5)
            self.assertEqual(measured[estimator]["ci95"], [0.5, 0.5])
            self.assertEqual(measured[estimator]["speedup_factor"], 2.0)
            self.assertEqual(measured[estimator]["reduction_percent"], 50.0)

    def test_seed_determinism_and_uncertainty(self):
        before = [1, 2, 3, 5, 9]
        after = [3, 1, 5, 1, 11]
        first = STATS.paired_bootstrap(before, after, seed=9, resamples=300)
        second = STATS.paired_bootstrap(before, after, seed=9, resamples=300)
        self.assertEqual(first, second)
        self.assertLess(first["p50"]["ci95"][0], first["p50"]["ci95"][1])

    def test_store_reservation_rounds_actual_allocated_peak(self):
        self.assertEqual(STATS.store_reservation_bytes(0), STATS.GIB)
        self.assertEqual(STATS.store_reservation_bytes(4 * STATS.GIB), 6 * STATS.GIB)
        self.assertEqual(STATS.store_reservation_bytes(4 * STATS.GIB + 1), 7 * STATS.GIB)
        for value in (True, -1, 1.5):
            with self.subTest(value=value), self.assertRaises(ValueError):
                STATS.store_reservation_bytes(value)


class CampaignTests(unittest.TestCase):
    def setUp(self):
        # Statistical pass-path fixtures are synthetic. Official validator
        # rejection is tested separately below, without this mock.
        validator = patch.object(STATS, "_official_release_valid", return_value={"opt_level": "3"})
        validator.start()
        self.addCleanup(validator.stop)

    def tearDown(self):
        while _ACTIVE_PROOF_ROOTS:
            _ACTIVE_PROOF_ROOTS.pop().cleanup()

    def test_complete_tool_timings_do_not_qualify_unverified_release_workload(self):
        result = STATS.compare_campaign(campaign_fixture())
        self.assertFalse(result["qualified"])
        self.assertFalse(result["adopted"])
        self.assertIn("qualification_release_cli_workload_timing_unverified", result["reasons"])
        self.assertTrue(result["gates"]["primary_p50"]["passed"])
        self.assertNotIn("release", result["metrics"])
        self.assertEqual(result["completed_valid_pairs"], 100)
        self.assertEqual(result["metrics"]["primary"]["p50"]["ratio"], 0.8)
        self.assertEqual(len(result["raw_records"]), 100)

    def test_pilot_has_no_adoption_flag_and_forecasts_compute_hours(self):
        result = STATS.compare_campaign(campaign_fixture("pilot"))
        self.assertNotIn("adopted", result)
        self.assertFalse(result["qualified"])
        self.assertIn("pilot_only", result["reasons"])
        self.assertAlmostEqual(result["forecast"]["projected_compute_hours"], 3.0)
        self.assertEqual(result["forecast"]["store_reservation_bytes"], 14 * STATS.GIB)

    def test_native_requires_storage_savings_and_tool_requires_gain(self):
        native = STATS.compare_campaign(campaign_fixture(kind="native", ratio=1.0, storage_ratio=0.95))
        self.assertFalse(native["adopted"])
        self.assertIn("gate_storage_savings_failed_or_uncertain", native["reasons"])
        tool = STATS.compare_campaign(campaign_fixture(ratio=0.9))
        self.assertFalse(tool["adopted"])
        self.assertIn("gate_primary_p50_failed_or_uncertain", tool["reasons"])

    def test_one_missing_pair_cannot_be_formal_evidence(self):
        campaign = campaign_fixture()
        campaign["records"].pop()
        result = STATS.compare_campaign(campaign)
        self.assertFalse(result["adopted"])
        self.assertIn("incomplete_pairs", result["reasons"])

    def test_duplicate_success_does_not_increase_fixed_sample_count(self):
        campaign = campaign_fixture()
        campaign["records"].append(copy.deepcopy(campaign["records"][0]))
        result = STATS.compare_campaign(campaign)
        self.assertFalse(result["adopted"])
        self.assertIn("duplicate_valid_pair", result["reasons"])

    def test_failure_cannot_be_replaced_by_a_successful_retry(self):
        campaign = campaign_fixture()
        failed = copy.deepcopy(campaign["records"][0])
        failed["status"] = "memory_limited"
        failed["reason"] = "job_commit_limit"
        campaign["records"][0]["attempt"] = 1
        campaign["records"].insert(0, failed)
        result = STATS.compare_campaign(campaign)
        self.assertFalse(result["adopted"])
        self.assertIn("retained_failed_attempt", result["reasons"])
        self.assertEqual(result["failed_attempts"], 1)
        self.assertEqual(result["raw_records"][0]["status"], "memory_limited")

    def test_contamination_is_preserved_and_requires_complete_retry(self):
        campaign = campaign_fixture()
        contaminated = copy.deepcopy(campaign["records"][0])
        contaminated["status"] = "contaminated"
        contaminated["reason"] = "external_process_activity"
        campaign["records"][0]["attempt"] = 1
        attach_synthetic_proofs(campaign)
        campaign["records"].insert(0, contaminated)
        result = STATS.compare_campaign(campaign)
        self.assertFalse(result["adopted"])
        self.assertIn("qualification_release_cli_workload_timing_unverified", result["reasons"])
        self.assertNotIn("invalid_retry_history", result["reasons"])
        self.assertEqual(result["completed_valid_pairs"], 100)
        self.assertEqual(result["contaminated_attempts"], 1)
        self.assertEqual(len(result["raw_records"]), 101)
        campaign["records"].pop(1)
        missing = STATS.compare_campaign(campaign)
        self.assertFalse(missing["adopted"])
        self.assertIn("incomplete_pairs", missing["reasons"])

    def test_wrong_source_and_capture_or_job_failure_block_adoption(self):
        for mutate, reason in (
            (lambda arm: arm["identity"].update(source_tree="0" * 40), "arm_identity_mismatch"),
            (lambda arm: arm.update(capture_complete=False), "incomplete_arm_evidence"),
            (lambda arm: arm["jobs"]["check"].update(exit_code=1), "invalid_job_or_measurement"),
        ):
            campaign = campaign_fixture()
            mutate(campaign["records"][0]["candidate"])
            result = STATS.compare_campaign(campaign)
            self.assertFalse(result["adopted"])
            self.assertIn(reason, result["reasons"])

    def test_missing_or_wrong_qualification_receipt_blocks_adoption(self):
        campaign = campaign_fixture()
        reference = campaign["qualification_receipts"]["diagnostics"]
        receipt = STATS._proof_json(reference)
        receipt["samples"].pop()
        campaign["qualification_receipts"]["diagnostics"] = write_proof(Path(reference["path"]), receipt)
        reference = campaign["qualification_receipts"]["release"]
        receipt = STATS._proof_json(reference)
        receipt["protocol_sha256"] = "2" * 64
        campaign["qualification_receipts"]["release"] = write_proof(Path(reference["path"]), receipt)
        campaign["qualification_receipts"].pop("optional_features")
        result = STATS.compare_campaign(campaign)
        self.assertFalse(result["adopted"])
        for name in ("diagnostics", "release", "optional_features"):
            self.assertIn(f"qualification_{name}_missing_or_invalid", result["reasons"])

    def test_feedback_regression_is_independent_of_primary_gain(self):
        campaign = campaign_fixture()
        for record in campaign["records"]:
            arm = record["candidate"]
            proof = STATS._proof_json(arm["evidence"])
            proof["edit_duration_ms"] = proof["phase_duration_ms"]["edit"] = 10600
            arm["feedback_phases"]["edit"]["duration_seconds"] = 10.6
            arm["evidence"] = write_proof(Path(arm["evidence"]["path"]), proof)
        reference = campaign["qualification_receipts"]["edit"]
        receipt = STATS._proof_json(reference)
        for sample in receipt["samples"]:
            sample["evidence"] = campaign["records"][sample["pair_index"]][sample["arm"]]["evidence"]
        campaign["qualification_receipts"]["edit"] = write_proof(Path(reference["path"]), receipt)
        result = STATS.compare_campaign(campaign)
        self.assertFalse(result["adopted"])
        self.assertIn("gate_edit_p95_failed_or_uncertain", result["reasons"])


    def test_partial_storage_evidence_does_not_hide_available_timings(self):
        campaign = campaign_fixture("pilot")
        campaign["records"][0]["candidate"]["allocation_complete"] = False
        original = copy.deepcopy(campaign)
        result = STATS.compare_campaign(campaign)
        self.assertEqual(result["completed_valid_pairs"], 4)
        self.assertEqual(result["timed_pairs"], 5)
        self.assertIn("primary", result["metrics"])
        self.assertFalse(result["qualified"])
        self.assertFalse(result["forecast"]["evidence_complete"])
        self.assertEqual(result["forecast"]["basis"], "five_timed_pilot_pairs_with_incomplete_evidence")
        self.assertEqual(campaign, original)

    def test_missing_store_peak_is_explicit_in_forecast(self):
        campaign = campaign_fixture("pilot")
        campaign.pop("pilot_store_peak_allocated_bytes")
        result = STATS.compare_campaign(campaign)
        self.assertIsNone(result["forecast"]["store_reservation_bytes"])
        self.assertEqual(result["forecast"]["store_reservation_reason"], "separate_measured_store_peak_required")

    def test_malformed_identity_and_status_are_unqualified_not_crashes(self):
        for mutate in (
            lambda campaign: campaign.update(phase=[]),
            lambda campaign: campaign.update(comparison_kind=[]),
            lambda campaign: campaign["identity"].update(msvc_compiler_sha256=None),
            lambda campaign: campaign["records"][0].update(status=[]),
        ):
            campaign = campaign_fixture()
            mutate(campaign)
            result = STATS.compare_campaign(campaign)
            self.assertFalse(result.get("adopted", False))
            self.assertTrue(result["reasons"])

    def test_inline_pass_flags_are_not_proof(self):
        campaign = campaign_fixture()
        campaign["qualification_receipts"] = {name: {
            "schema_version": 1, "status": "passed", "complete": True,
            "identity": campaign["identity"], "protocol_sha256": campaign["protocol_sha256"],
            "checks": [{"status": "passed", "exit_code": 0}],
            "file_line_verified": True, "release_profile_unchanged": True,
            "timings": {"baseline": [10.0] * 100, "candidate": [10.0] * 100},
        } for name in STATS.QUALIFICATIONS}
        result = STATS.compare_campaign(campaign)
        self.assertFalse(result["adopted"])
        self.assertTrue(all(not status for status in result["qualification_receipts"].values()))

    def test_missing_changed_or_stale_command_evidence_blocks_adoption(self):
        campaign = campaign_fixture("pilot")
        original = copy.deepcopy(campaign["qualification_receipts"])
        reference = original["release"]
        receipt = STATS._proof_json(reference)
        command = STATS._proof_json(receipt["samples"][0]["evidence"])
        Path(command["capture"]["stdout_path"]).write_bytes(b"changed log")
        with self.assertRaises(ValueError):
            STATS._qualification_receipt(reference, "release", campaign["identity"], campaign["protocol_sha256"], campaign["records"], 5)
        receipt["samples"][0]["attempt"] = 99
        stale = write_proof(Path(reference["path"]), receipt)
        with self.assertRaises(ValueError):
            STATS._qualification_receipt(stale, "release", campaign["identity"], campaign["protocol_sha256"], campaign["records"], 5)

    def test_edit_job_regression_has_its_own_gate(self):
        campaign = campaign_fixture("pilot")
        for record in campaign["records"]:
            arm = record["candidate"]
            proof = STATS._proof_json(arm["evidence"])
            for entry in proof["edit_jobs"]:
                if entry["job"] == "clippy":
                    entry["duration_ms"] = 12000
            arm["feedback_phases"]["edit"]["jobs"]["clippy"]["duration_seconds"] = 12.0
            arm["evidence"] = write_proof(Path(arm["evidence"]["path"]), proof)
        result = STATS.compare_campaign(campaign)
        self.assertIn("gate_edit_clippy_p95_failed_or_uncertain", result["reasons"])

    def test_qualification_report_and_artifact_hashes_are_rechecked(self):
        campaign = campaign_fixture("pilot")
        reference = campaign["qualification_receipts"]["clippy"]
        receipt = STATS._proof_json(reference)
        proof = STATS._proof_json(receipt["samples"][0]["evidence"])
        Path(proof["report"]["path"]).write_bytes(b"changed report")
        with self.assertRaises(ValueError):
            STATS._qualification_receipt(reference, "clippy", campaign["identity"], campaign["protocol_sha256"], campaign["records"], 5)
        artifact = proof["artifacts"][0]
        Path(artifact["path"]).write_bytes(b"stale artifact")
        with self.assertRaises(ValueError):
            STATS._arm_proof_valid(campaign["records"][0]["baseline"], "baseline", campaign["identity"])

    def test_inline_official_release_flags_do_not_replace_actual_report(self):
        campaign = campaign_fixture("pilot")
        receipt = STATS._proof_json(campaign["qualification_receipts"]["release"])
        proof = STATS._proof_json(receipt["samples"][0]["evidence"])
        # Execute the actual function body even though setUp isolates pass math.
        spec = importlib.util.spec_from_file_location("unmocked_stats", MODULE_PATH)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        canonical = MODULE_PATH.with_name("core_beta_benchmark.py").resolve()
        proof["validator"] = {"path": str(canonical), "sha256": hashlib.sha256(canonical.read_bytes()).hexdigest()}
        proof["official_report"] = write_proof(Path(proof["source_root"]) / "fabricated-official.json", {
            "status": "passed", "build_environment_validated": True, "release_profile_unchanged": True})
        with self.assertRaises(ValueError):
            module._official_release_valid(proof, campaign["identity"])

    def test_official_path_validator_retains_internal_errors_without_retry(self):
        campaign = campaign_fixture("pilot")
        receipt = STATS._proof_json(campaign["qualification_receipts"]["release"])
        proof = STATS._proof_json(receipt["samples"][0]["evidence"])
        spec = importlib.util.spec_from_file_location("unmocked_stats_path_api", MODULE_PATH)
        if spec is None or spec.loader is None:
            raise RuntimeError("statistics module unavailable")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        canonical = MODULE_PATH.with_name("core_beta_benchmark.py").resolve()
        proof["validator"] = {"path": str(canonical), "sha256": hashlib.sha256(canonical.read_bytes()).hexdigest()}
        proof["official_report"] = write_proof(Path(proof["source_root"]) / "internal-error-report.json", {"synthetic": True})
        validator = Mock()
        calls = []

        def validate_report(path, expected_profile=None):
            calls.append((path, expected_profile))
            print("official validator output")
            raise TypeError("invalid report field from official validator")

        validator.validate_report = validate_report
        stdout = io.StringIO()
        with (
            patch.object(module, "_RELEASE_VALIDATOR", validator),
            contextlib.redirect_stdout(stdout),
            self.assertRaisesRegex(TypeError, "invalid report field from official validator"),
        ):
            module._official_release_valid(proof, campaign["identity"])
        self.assertEqual(calls, [(Path(proof["official_report"]["path"]), "full")])
        self.assertEqual(stdout.getvalue(), "")

    def test_release_harness_wallclock_is_not_cli_workload_evidence(self):
        campaign = campaign_fixture("pilot")
        with self.assertRaisesRegex(ValueError, "release_cli_workload_timing_unverified"):
            STATS._qualification_receipt(
                campaign["qualification_receipts"]["release"], "release", campaign["identity"],
                campaign["protocol_sha256"], campaign["records"], 5)
        result = STATS.compare_campaign(campaign)
        self.assertFalse(result["qualification_receipts"]["release"])
        self.assertNotIn("release", result["metrics"])
        self.assertIn("qualification_release_cli_workload_timing_unverified", result["reasons"])

    def test_all_feedback_phases_require_successful_logged_captures(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["candidate"]
        reference = arm["evidence"]
        original = STATS._proof_json(reference)
        for phase in ("jobs", "feedback_warm_jobs", "edit_jobs"):
            with self.subTest(phase=phase):
                proof = copy.deepcopy(original)
                proof[phase][0]["returncode"] = 1
                arm["evidence"] = write_proof(Path(reference["path"]), proof)
                self.assertFalse(STATS._arm_proof_valid(arm, "candidate", campaign["identity"]))
        proof = copy.deepcopy(original)
        proof["feedback_warm_jobs"].pop()
        arm["evidence"] = write_proof(Path(reference["path"]), proof)
        self.assertFalse(STATS._arm_proof_valid(arm, "candidate", campaign["identity"]))

    def test_twenty_pair_health_check_cannot_change_fixed_n(self):
        campaign = campaign_fixture()
        campaign["pairs_required"] = 20
        campaign["records"] = campaign["records"][:20]
        result = STATS.compare_campaign(campaign)
        self.assertFalse(result["adopted"])
        self.assertIn("fixed_pair_count_mismatch", result["reasons"])


if __name__ == "__main__":
    unittest.main()
