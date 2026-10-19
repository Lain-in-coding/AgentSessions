import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import math
import random
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace
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
                        argv = ["synthetic-python", str(Path(validator["path"])), "run", "--workspace", arm["source_root"]]
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


def synthetic_official_report(arm, identity, *, scale=1.0, tag="sample", capture_start=None):
    origin = capture_start or datetime(2026, 10, 19, tzinfo=timezone.utc)
    observations = []
    ordinal = 0
    metrics = {}
    for metric, (operation, state, count) in STATS.RELEASE_WORKLOADS.items():
        entries = []
        for index in range(1, count + 1):
            argv = ["binary", "--version"] if operation == "startup_warm" else (
                [f"scratch/anchor/startup-cold-{index - 1:03d}.exe", "--version"] if operation == "startup_cold" else
                ["binary", "--db", "scratch/anchor/sync-trial-002/catalog.db", "--output", "json", operation, "synthetic-input"])
            ordinal += 1
            started = origin + timedelta(seconds=1, milliseconds=ordinal * 20)
            finished = started + timedelta(milliseconds=10)
            entry = {"workload": "anchor", "operation": operation,
                     "phase": "startup" if operation.startswith("startup_") else "post_shrink", "role": "measured",
                     "sample_index": index, "legacy_sample_index": index, "legacy_metric": metric,
                     "outcome": "success", "exit_code": 0, "argv": argv,
                     "process_wall_ms": 10.0 * scale, "legacy_duration_ms": 10000.0,
                     "started_at_utc": started.isoformat(), "completed_at_utc": finished.isoformat()}
            entries.append(entry)
        observations.extend(entries)
        metrics[metric] = {"name": metric, "unit": "ms", "required_sample_count": count,
                           "state": "cold_best_effort" if state == "cold" else "warm",
                           "raw_samples": [entry["legacy_duration_ms"] for entry in entries]}
    return {"profile": "full", "evidence_status": "locally_verified", "tag": tag,
            "dataset": {"dataset_hash": "a" * 64, "query_set": ["synthetic-input"], "file_count": 20},
            "metrics": metrics, "baseline": {"anchor": {"observations": observations},
                "build_receipt": {"release_profile": {"opt_level": "3", "test": False, "debug_assertions": False}}}}


def attach_synthetic_release_reports(campaign):
    reference = campaign["qualification_receipts"]["release"]
    receipt = STATS._proof_json(reference)
    for sample in receipt["samples"]:
        arm = campaign["records"][sample["pair_index"]][sample["arm"]]
        proof = STATS._proof_json(sample["evidence"])
        order = campaign["records"][sample["pair_index"]]["order"]
        capture_start = datetime(2026, 10, 19, tzinfo=timezone.utc) + timedelta(seconds=sample["pair_index"] * 30 + order.index(sample["arm"]) * 15)
        proof["capture"]["started_at"], proof["capture"]["finished_at"] = capture_start.isoformat(), (capture_start + timedelta(seconds=10)).isoformat()
        report = synthetic_official_report(arm, campaign["identity"], tag=f"{sample['pair_index']}/{sample['arm']}", capture_start=capture_start)
        proof["official_report"] = write_proof(Path(arm["source_root"]) / "official-release.json", report)
        proof_report = STATS._proof_json(proof["report"])
        proof_report["capture_sha256"] = hashlib.sha256(json.dumps(proof["capture"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
        proof["report"] = write_proof(Path(proof["report"]["path"]), proof_report)
        sample["evidence"] = write_proof(Path(sample["evidence"]["path"]), proof)
    campaign["qualification_receipts"]["release"] = write_proof(Path(reference["path"]), receipt)
    campaign["protocol_evidence"] = {"path": str(Path(reference["path"]).with_name("protocol.json")), "sha256": "1" * 64}


class ReleaseClusterTests(unittest.TestCase):
    def test_paired_groups_preserve_scale_and_run_unit(self):
        before = [[1.0] * 20, [2.0] * 20, [10.0] * 20]
        after = [[value / 2 for value in group] for group in before]
        result = STATS.paired_cluster_bootstrap(before, after, seed=7, resamples=100)
        self.assertEqual(result["resampling_unit"], "paired_run")
        self.assertEqual(result["sample_count"], 3)
        self.assertEqual(result["invocation_count_per_arm"], 60)
        for name in ("p50", "p95"):
            self.assertEqual(result[name]["ratio"], 0.5)
            self.assertEqual(result[name]["ci95"], [0.5, 0.5])
        self.assertEqual(result, STATS.paired_cluster_bootstrap(before, after, seed=7, resamples=100))

    def test_cluster_bootstrap_matches_explicit_run_resampling(self):
        before = [[1.0, 2.0], [3.0, 40.0], [5.0, 6.0]]
        after = [[1.0, 2.0], [8.0, 50.0], [2.0, 4.0]]
        result = STATS.paired_cluster_bootstrap(before, after, seed=19, resamples=100)
        rng = random.Random(19)
        ratios = {"p50": [], "p95": []}
        for _ in range(100):
            indices = [rng.randrange(3) for _ in range(3)]
            left = [value for index in indices for value in before[index]]
            right = [value for index in indices for value in after[index]]
            for name, percentile in (("p50", 50), ("p95", 95)):
                ratios[name].append(STATS.nearest_rank(right, percentile) / STATS.nearest_rank(left, percentile))
        for name, values in ratios.items():
            self.assertEqual(result[name]["ci95"], [STATS.nearest_rank(values, 2.5), STATS.nearest_rank(values, 97.5)])
        for left, right in (([], []), ([[1]], []), ([[1, 2]], [[1]])):
            with self.subTest(left=left, right=right), self.assertRaises(ValueError):
                STATS.paired_cluster_bootstrap(left, right, seed=1, resamples=100)


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
        proof["capture"]["argv"] = ["python", str(canonical), "run", "--workspace", proof["source_root"]]
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
        proof["capture"]["argv"] = ["python", str(canonical), "run", "--workspace", proof["source_root"]]
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

    def test_complete_raw_release_workloads_enable_qualified_synthetic_campaign(self):
        campaign = campaign_fixture()
        attach_synthetic_release_reports(campaign)
        with (
            patch.object(STATS, "_release_protocol", return_value={}),
            patch.object(STATS, "_official_release_valid", side_effect=lambda proof, identity, **kwargs: STATS._proof_json(proof["official_report"])),
        ):
            result = STATS.compare_campaign(campaign)
        self.assertTrue(result["qualified"], result["reasons"])
        self.assertTrue(result["adopted"])
        for workload in STATS.RELEASE_WORKLOADS:
            measurement = result["metrics"]["release_" + workload]
            self.assertEqual(measurement["resampling_unit"], "paired_run")
            self.assertEqual(measurement["sample_count"], 100)
            self.assertEqual(measurement["p50"]["baseline"], 10.0)
            self.assertEqual(measurement["p95"]["ratio"], 1.0)
        self.assertNotIn("release", result["metrics"])
        self.assertNotIn("qualification_release_cli_workload_timing_unverified", result["reasons"])

    def test_release_tail_regression_is_detected_without_harness_duration(self):
        campaign = campaign_fixture("pilot")
        attach_synthetic_release_reports(campaign)
        receipt = STATS._proof_json(campaign["qualification_receipts"]["release"])
        for sample in receipt["samples"]:
            if sample["arm"] != "candidate":
                continue
            proof = STATS._proof_json(sample["evidence"])
            report = STATS._proof_json(proof["official_report"])
            entries = [entry for entry in report["baseline"]["anchor"]["observations"] if entry["legacy_metric"] == "search_latency_ms"]
            for entry in entries[-6:]:
                entry["process_wall_ms"] = 11.0
            proof["official_report"] = write_proof(Path(proof["official_report"]["path"]), report)
            sample["evidence"] = write_proof(Path(sample["evidence"]["path"]), proof)
        campaign["qualification_receipts"]["release"] = write_proof(Path(campaign["qualification_receipts"]["release"]["path"]), receipt)
        with (
            patch.object(STATS, "_release_protocol", return_value={}),
            patch.object(STATS, "_official_release_valid", side_effect=lambda proof, identity, **kwargs: STATS._proof_json(proof["official_report"])),
        ):
            result = STATS.compare_campaign(campaign)
        self.assertTrue(result["qualification_receipts"]["release"])
        self.assertFalse(result["gates"]["release_search_latency_ms_p95"]["passed"])
        self.assertTrue(result["gates"]["release_search_latency_ms_p50"]["passed"])
        self.assertAlmostEqual(result["metrics"]["release_search_latency_ms"]["p95"]["ratio"], 1.1)

    def test_raw_release_coverage_and_capture_boundaries_reject_mutation(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        original = synthetic_official_report(arm, campaign["identity"])
        proof = {"capture": {"started_at": "2026-10-19T00:00:00Z", "finished_at": "2026-10-19T00:00:10Z"}}
        self.assertEqual(STATS._release_workload_samples(original, proof)["search_latency_ms"], [10.0] * 100)
        for mutate in (
            lambda report: report["baseline"]["anchor"]["observations"].pop(),
            lambda report: report["baseline"]["anchor"]["observations"][0].update(role="warmup"),
            lambda report: report["baseline"]["anchor"]["observations"][0].update(phase="warm"),
            lambda report: report["baseline"]["anchor"]["observations"][0].update(sample_index=2),
            lambda report: report["baseline"]["anchor"]["observations"][0].update(exit_code=1),
            lambda report: report["baseline"]["anchor"]["observations"][0].update(process_wall_ms=True),
            lambda report: report["baseline"]["anchor"]["observations"][0].update(completed_at_utc="2026-10-19T00:00:11Z"),
            lambda report: report["metrics"]["search_latency_ms"]["raw_samples"].__setitem__(0, 2.0),
            lambda report: report["metrics"]["cli_startup_cold_latency_ms"].update(state="warm"),
        ):
            report = copy.deepcopy(original)
            mutate(report)
            with self.assertRaises((ValueError, TypeError)):
                STATS._release_workload_samples(report, proof)

    def test_release_protocol_binds_hash_source_toolchain_and_arm_allocations(self):
        campaign = campaign_fixture("pilot")
        identity = campaign["identity"]
        chain = {key: {"version": identity[key + "_version"], "sha256": identity[key + "_sha256"]} for key in ("rustc", "cargo")}
        chain.update(msvc={
            "compiler": {"version": identity["msvc_compiler_version"], "sha256": identity["msvc_compiler_sha256"]},
            "linker": {"version": identity["msvc_linker_version"], "sha256": identity["msvc_linker_sha256"]}}, target=identity["host"])
        run_root = Path(campaign["records"][0]["baseline"]["source_root"]).parents[1] / "approved-run"
        for record in campaign["records"]:
            for name in ("baseline", "candidate"):
                arm = record[name]
                arm_proof = STATS._proof_json(arm["evidence"])
                arm_root = run_root / "pairs" / f"pair-{record['pair_index']:04d}-attempt-{record['attempt']:02d}" / name
                target = arm_root / "targets"
                target.mkdir(parents=True)
                arm_proof["target_root"] = str(target)
                arm["evidence"] = write_proof(arm_root / "arm.json", arm_proof)
        protocol = {"run_root": str(run_root), "source": {"commit": identity["source_commit"], "tree": identity["source_tree"],
                              "lock_sha256": identity["lock_sha256"], "resources_sha256": identity["resources_sha256"]},
                    "toolchain": chain, "jobs": 2, "test_threads": 2,
                    "phase": campaign["phase"], "pairs": campaign["pairs_required"],
                    "seed": campaign["seed"], "order": campaign["schedule"],
                    "configs": {"baseline": {"name": "baseline", "tool": {"kind": "native"}},
                                "candidate": {"name": "candidate", "tool": {"kind": "sccache"}}},
                    "allocations": [{"pair_index": record["pair_index"], "attempt": record["attempt"],
                        "baseline_source_root": record["baseline"]["source_root"],
                        "candidate_source_root": record["candidate"]["source_root"]} for record in campaign["records"]]}
        fake = SimpleNamespace(_validate_protocol=Mock(), canonical_sha256=Mock(side_effect=lambda value: campaign["protocol_sha256"] if value == protocol else campaign["records"][0][value["name"]]["config_sha256"]))
        reference = write_proof(Path(campaign["records"][0]["baseline"]["source_root"]) / "approved-protocol.json", protocol)
        with patch.object(STATS, "_runner_module", return_value=fake):
            result = STATS._release_protocol(reference, identity, campaign["protocol_sha256"], campaign["records"], campaign=campaign)
            self.assertEqual(result, protocol)
            for field, value in (
                ("phase", "formal"), ("pairs_required", 100), ("seed", campaign["seed"] + 1),
                ("schedule", [list(reversed(order)) for order in campaign["schedule"]]), ("comparison_kind", "native"),
                ("job_names", list(reversed(JOBS))), ("seed", float(campaign["seed"])),
                ("pairs_required", 5.0),
            ):
                altered = copy.deepcopy(campaign)
                altered[field] = value
                with self.subTest(field=field, value=value), self.assertRaisesRegex(ValueError, "campaign phase/count/seed/order/kind"):
                    STATS._release_protocol(reference, identity, campaign["protocol_sha256"], campaign["records"], campaign=altered)
            with self.assertRaisesRegex(ValueError, "fixed paired allocation coverage"):
                STATS._release_protocol(reference, identity, campaign["protocol_sha256"], campaign["records"][:-1], campaign=campaign)
            reordered = copy.deepcopy(campaign["records"])
            reordered[0]["order"] = list(reversed(reordered[0]["order"]))
            with self.assertRaisesRegex(ValueError, "pair order differs"):
                STATS._release_protocol(reference, identity, campaign["protocol_sha256"], reordered, campaign=campaign)
            changed = copy.deepcopy(campaign["records"])
            changed[0]["candidate"]["source_root"] = str(Path(changed[0]["candidate"]["source_root"]) / "other")
            with self.assertRaisesRegex(ValueError, "source/config differs"):
                STATS._release_protocol(reference, identity, campaign["protocol_sha256"], changed, campaign=campaign)
            changed = copy.deepcopy(campaign["records"])
            stale_arm = STATS._proof_json(changed[0]["baseline"]["evidence"])
            changed[0]["baseline"]["evidence"] = write_proof(run_root / "stale-arm.json", stale_arm)
            with self.assertRaisesRegex(ValueError, "receipt/target differs"):
                STATS._release_protocol(reference, identity, campaign["protocol_sha256"], changed, campaign=campaign)
            changed = copy.deepcopy(campaign["records"])
            old_evidence = changed[0]["baseline"]["evidence"]
            stale_arm = STATS._proof_json(old_evidence)
            stale_arm["target_root"] = str(run_root)
            changed[0]["baseline"]["evidence"] = write_proof(Path(old_evidence["path"]), stale_arm)
            with self.assertRaisesRegex(ValueError, "receipt/target differs"):
                STATS._release_protocol(reference, identity, campaign["protocol_sha256"], changed, campaign=campaign)
            Path(reference["path"]).write_bytes(b"modified protocol")
            with self.assertRaisesRegex(ValueError, "hash changed"):
                STATS._release_protocol(reference, identity, campaign["protocol_sha256"], campaign["records"], campaign=campaign)

    def test_release_rejects_same_report_inline_times_and_reversed_capture(self):
        for mutation in ("inline", "reused", "reversed"):
            campaign = campaign_fixture("pilot")
            attach_synthetic_release_reports(campaign)
            reference = campaign["qualification_receipts"]["release"]
            receipt = STATS._proof_json(reference)
            if mutation == "inline":
                receipt["samples"][0]["duration_seconds"] = 1.0
            else:
                sample = receipt["samples"][1]
                proof = STATS._proof_json(sample["evidence"])
                if mutation == "reused":
                    other = STATS._proof_json(receipt["samples"][0]["evidence"])
                    proof["official_report"] = other["official_report"]
                    proof["capture"]["started_at"] = other["capture"]["started_at"]
                    proof["capture"]["finished_at"] = other["capture"]["finished_at"]
                else:
                    proof["capture"]["started_at"] = "2026-10-18T23:59:59Z"
                    proof["capture"]["finished_at"] = "2026-10-19T00:01:00Z"
                command_report = STATS._proof_json(proof["report"])
                command_report["capture_sha256"] = hashlib.sha256(json.dumps(proof["capture"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
                proof["report"] = write_proof(Path(proof["report"]["path"]), command_report)
                sample["evidence"] = write_proof(Path(sample["evidence"]["path"]), proof)
            candidate_reference = write_proof(Path(reference["path"]).with_name(mutation + ".json"), receipt)
            with (
                patch.object(STATS, "_release_protocol", return_value={}),
                patch.object(STATS, "_official_release_valid", side_effect=lambda proof, identity, **kwargs: STATS._proof_json(proof["official_report"])),
                self.assertRaises(ValueError),
            ):
                STATS._qualification_receipt(candidate_reference, "release", campaign["identity"], campaign["protocol_sha256"], campaign["records"], 5, protocol_reference=campaign["protocol_evidence"])

    def test_frozen_validator_requires_manifest_identity_before_execution(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        receipt = STATS._proof_json(campaign["qualification_receipts"]["release"])
        proof = STATS._proof_json(receipt["samples"][0]["evidence"])
        root = Path(arm["source_root"])
        canonical = root / "scripts/evidence/core_beta_benchmark.py"
        validator = write_proof(canonical, b"# synthetic pinned official validator, never executed\n")
        manifest_entry = {"path": "scripts/evidence/core_beta_benchmark.py", "sha256": validator["sha256"], "bytes": canonical.stat().st_size}
        source_receipt = write_proof(root / "source-manifest.json", {"source_identity": {"files": [manifest_entry]}})
        protocol = {"source": {"receipt_path": source_receipt["path"], "receipt_sha256": source_receipt["sha256"]}}
        proof["validator"] = validator
        proof["capture"]["argv"] = ["python", str(canonical), "run", "--workspace", proof["source_root"]]
        proof["official_report"] = write_proof(root / "official.json", {"synthetic": True})
        spec = importlib.util.spec_from_file_location("unmocked_frozen_stats", MODULE_PATH)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        fake = SimpleNamespace(_verify_source=Mock(), _manifest=Mock(return_value=[manifest_entry]))
        rejecting_validator = SimpleNamespace(validate_report=Mock(side_effect=ValueError("official full validation rejects synthetic")))
        key = (str(canonical), validator["sha256"])
        with patch.object(module, "_runner_module", return_value=fake), patch.dict(module._RELEASE_VALIDATORS, {key: rejecting_validator}):
            with self.assertRaisesRegex(ValueError, "official full validation rejects synthetic"):
                module._official_release_valid(proof, campaign["identity"], protocol=protocol, arm_evidence=arm["evidence"])
            self.assertEqual(rejecting_validator.validate_report.call_count, 1)
            unrelated = write_proof(root / "unrelated/core_beta_benchmark.py", canonical.read_bytes())
            proof["validator"] = unrelated
            with self.assertRaisesRegex(ValueError, "differs from approved frozen source"):
                module._official_release_valid(proof, campaign["identity"], protocol=protocol, arm_evidence=arm["evidence"])
            self.assertEqual(rejecting_validator.validate_report.call_count, 1)

    def test_manifest_approved_validator_imports_common_api_and_rechecks_files(self):
        """Synthetic validator proves the import boundary, never release performance."""
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        root = Path(arm["source_root"])
        source = root / "scripts/evidence/core_beta_benchmark.py"
        validator = write_proof(source, b"""import json
from pathlib import Path
calls = []
def validate_report(path, expected_profile=None):
    calls.append((path, expected_profile))
    report = json.loads(path.read_text(encoding='utf-8'))
    if expected_profile != 'full' or report.get('profile') != expected_profile:
        raise ValueError('synthetic validator profile mismatch')
    if report.get('mutate_validator'):
        Path(__file__).write_text('# changed validator', encoding='utf-8')
    if report.get('mutate_report'):
        path.write_text('{}', encoding='utf-8')
    return report
""")
        manifest = [{"path": "scripts/evidence/core_beta_benchmark.py", "sha256": validator["sha256"],
                     "bytes": source.stat().st_size}]
        receipt = write_proof(root / "approved-source.json", {"source_identity": {"files": manifest}})
        protocol = {"source": {"receipt_path": receipt["path"], "receipt_sha256": receipt["sha256"]}}
        identity = campaign["identity"]
        binary = write_proof(root / "suite/baseline-cli.exe", b"synthetic suite binary")
        output = root / "suite/report"
        output.mkdir()
        origin = {"cargo_lock_sha256": identity["lock_sha256"], "harness_sha256": validator["sha256"]}
        report = {
            "profile": "full", "commit": identity["source_commit"],
            "binary": {"binary_hash": binary["sha256"]},
            "baseline": {"source_before": origin, "source_after": copy.deepcopy(origin),
                "started_at_utc": "2026-10-19T00:00:01Z", "completed_at_utc": "2026-10-19T00:00:09Z",
                "run_ordinal": 1, "suite_id": "synthetic-suite",
                "build_receipt": {"source": copy.deepcopy(origin),
                    "configuration": {"target": identity["host"], "rustc": identity["rustc_version"],
                        "rustc_tool_sha256": identity["rustc_sha256"], "cargo_tool_sha256": identity["cargo_sha256"]},
                    "cargo_version": identity["cargo_version"].splitlines()[0],
                    "binary_sha256": binary["sha256"], "binary_size_bytes": Path(binary["path"]).stat().st_size,
                    "release_profile": {"opt_level": "3", "debug_assertions": False, "test": False}}},
        }
        proof = {"source_root": str(root), "validator": validator,
            "official_report": write_proof(output / "official.json", report),
            "release_binary": binary, "artifacts": [binary],
            "capture": {"cwd": str(root), "started_at": "2026-10-19T00:00:00Z", "finished_at": "2026-10-19T00:00:10Z",
                "argv": ["python", str(source), "run", "--workspace", str(root),
                "--output-dir", str(output), "--profile", "full", "--expected-commit", identity["source_commit"],
                "--baseline-run", "1", "--suite-id", "synthetic-suite"]}}
        arm_reference = write_proof(root / "suite/arm.json", {"target_root": str(root / "suite/target")})
        (root / "suite/target").mkdir()
        spec = importlib.util.spec_from_file_location("unmocked_approved_import_stats", MODULE_PATH)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        # Source/Git validation has separate runner coverage. Use its real manifest
        # decoder and actual module import/API here, without injecting a validator.
        runner = module._runner_module()
        with patch.object(runner, "_verify_source") as verify:
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                self.assertEqual(module._official_release_valid(proof, identity, protocol=protocol,
                    arm_evidence=arm_reference), report)
            self.assertEqual(stdout.getvalue(), "")
            self.assertEqual(verify.call_count, 2)
            loaded = module._RELEASE_VALIDATORS[(str(source), validator["sha256"])]
            self.assertEqual(loaded.calls, [(Path(proof["official_report"]["path"]), "full")])
            for mutation in ("unrelated_script", "validation_subcommand"):
                changed = copy.deepcopy(proof)
                if mutation == "unrelated_script":
                    changed["capture"]["argv"][1] = str(root / "unrelated.py")
                    changed["capture"]["argv"].append(str(source))
                else:
                    changed["capture"]["argv"][2] = "validate-report"
                    changed["capture"]["argv"].append("run")
                with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, "direct official release command"):
                    module._official_release_valid(changed, identity, protocol=protocol, arm_evidence=arm_reference)
            changed = copy.deepcopy(proof)
            changed["capture"]["cwd"] = str(output)
            with self.assertRaisesRegex(ValueError, "cwd differs"):
                module._official_release_valid(changed, identity, protocol=protocol, arm_evidence=arm_reference)
            for field, stamp in (("started_at", "2026-10-19T00:00:02Z"), ("finished_at", "2026-10-19T00:00:08Z")):
                changed = copy.deepcopy(proof)
                changed["capture"][field] = stamp
                with self.subTest(field=field), self.assertRaisesRegex(ValueError, "run interval exceeds captured command"):
                    module._official_release_valid(changed, identity, protocol=protocol, arm_evidence=arm_reference)
            for field in ("mutate_report", "mutate_validator"):
                with self.subTest(field=field):
                    altered = copy.deepcopy(report)
                    altered[field] = True
                    proof["official_report"] = write_proof(output / "official.json", altered)
                    with self.assertRaisesRegex(ValueError, "hash changed|changed during validation"):
                        module._official_release_valid(proof, identity, protocol=protocol, arm_evidence=arm_reference)

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
