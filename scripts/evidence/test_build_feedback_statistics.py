import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import math
import os
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
                    output = (json.dumps({"reason": "compiler-artifact", "filenames": [artifact["path"]], "fresh": False, "profile": {"opt_level": "0"}, "executable": None}) + "\n"
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





class ProofStreamingTests(unittest.TestCase):
    def test_hashes_complete_large_file_without_whole_read_and_checks_tail(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "artifact.bin"
            block = b"synthetic allocation " * 65536
            with path.open("wb") as stream:
                for _ in range(4):
                    stream.write(block)
            digest = hashlib.sha256()
            with path.open("rb") as stream:
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(chunk)
            reference = {"path": str(path), "sha256": digest.hexdigest(), "bytes": path.stat().st_size}
            with patch.object(Path, "read_bytes", side_effect=AssertionError("no whole artifact read")):
                self.assertEqual(STATS._proof_file(reference), (path, reference["bytes"]))
                with self.assertRaisesRegex(ValueError, "size changed"):
                    STATS._proof_file({**reference, "bytes": reference["bytes"] + 1})
                with path.open("ab") as stream:
                    stream.write(b"changed tail")
                with self.assertRaisesRegex(ValueError, "hash changed"):
                    STATS._proof_file(reference)

    def test_empty_files_and_exact_integer_size(self):
        with tempfile.TemporaryDirectory() as directory:
            reference = write_proof(Path(directory) / "empty", b"")
            self.assertEqual(STATS._proof_file(reference)[1], 0)
            with self.assertRaisesRegex(ValueError, "size changed"):
                STATS._proof_file({**reference, "bytes": False})

    def test_diagnostic_cross_chunk_unicode_and_between_stream_boundary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            needle = "C:/synthetic/小文件.rs:27:"
            prefix = b"x" * (1024 * 1024 - 2)
            value = prefix + needle.encode("utf-8") + b"tail"
            first = write_proof(root / "stdout", value)
            second = write_proof(root / "stderr", b"")
            with patch.object(Path, "read_bytes", side_effect=AssertionError("no whole log read")):
                self.assertTrue(STATS._proof_text_contains([first, second], [needle]))
                self.assertFalse(STATS._proof_text_contains([first, second], [needle + "missing"]))
            split = len(prefix) + len(needle.encode("utf-8")) - 5
            first = write_proof(root / "stdout", value[:split])
            second = write_proof(root / "stderr", value[split:])
            self.assertTrue(STATS._proof_text_contains([first, second], [needle]))
            with Path(second["path"]).open("ab") as stream:
                stream.write(b"tamper after found needle")
            with self.assertRaisesRegex(ValueError, "hash changed"):
                STATS._proof_text_contains([first, second], [needle])

    def test_duplicate_and_nonfinite_json_still_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "proof.json"
            for content in (b'{"sample": 1, "sample": 2}', b'{"sample": NaN}'):
                reference = write_proof(path, content)
                with self.assertRaises(ValueError):
                    STATS._proof_json(reference)


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


def attach_qualification_cargo(proof, target, source):
    """Synthetic Cargo process output, parsed by the real receipt decoder."""
    artifact = write_proof(target / "synthetic.rmeta", b"synthetic qualified compiler output")
    payload = (json.dumps({"reason": "compiler-artifact", "filenames": [artifact["path"]], "executable": None,
                          "fresh": False, "profile": {"test": False}, "manifest_path": str(source / "Cargo.toml")})
               + '\n{"reason":"build-finished","success":true}\n{legal test tail}\n').encode()
    reference = write_proof(Path(proof["capture"]["stdout_path"]), payload)
    proof["capture"]["stdout_sha256"] = reference["sha256"]
    proof["capture"]["cargo"] = STATS._runner_module().parse_cargo_json_file(Path(reference["path"]), target, source)
    proof["artifacts"] = [file for artifact in proof["capture"]["cargo"]["artifacts"] for file in artifact["files"]]


def bind_synthetic_v2(campaign, *, phase=None, pairs=None):
    identity = campaign["identity"]
    count = pairs if pairs is not None else campaign["pairs_required"]
    active_phase = phase or campaign["phase"]
    campaign.update(schema_version=2, phase=active_phase, pairs_required=count)
    if active_phase == "capacity":
        campaign["records"], campaign["schedule"] = [], []
    else:
        campaign["records"] = campaign["records"][:count]
        campaign["schedule"] = campaign["schedule"][:count]
    first = _ACTIVE_PROOF_ROOTS[-1]
    root = Path(first.name)
    configs = {name: {"layout": "isolated", "incremental": "on", "debug": "full", "tool": {"kind": "native" if campaign["comparison_kind"] == "native" else "synthetic-tool"}}
               for name in ("baseline", "candidate")}
    protocol = {"schema_version": 2, "kind": "build-feedback-protocol", "phase": active_phase, "pairs": count,
        "source": {"commit": identity["source_commit"], "tree": identity["source_tree"],
                   "lock_sha256": identity["lock_sha256"], "resources_sha256": identity["resources_sha256"], "receipt_sha256": "f" * 64},
        "toolchain": {"rustc": {"version": identity["rustc_version"], "sha256": identity["rustc_sha256"]},
                      "cargo": {"path": "synthetic-cargo", "version": identity["cargo_version"], "sha256": identity["cargo_sha256"]},
                      "msvc": {"compiler": {"version": identity["msvc_compiler_version"], "sha256": identity["msvc_compiler_sha256"]},
                               "linker": {"version": identity["msvc_linker_version"], "sha256": identity["msvc_linker_sha256"]}},
                      "target": identity["host"]},
        "jobs": 2, "test_threads": 2, "seed": campaign["seed"], "order": campaign["schedule"], "configs": configs,
        "run_root": str(root), "formal_registration": None}
    if active_phase == "formal":
        registration = {"schema_version": 2, "kind": "build-feedback-formal-registration", "approved": True,
                        "pairs": count, "seed": campaign["seed"], "order": campaign["schedule"],
                        "run_root": str(root), "stopping_rule": "fixed-n-no-adaptive-additions", "formal_data_collected": False}
        protocol["formal_registration"] = write_proof(root / "registration.json", registration)
    campaign["protocol_evidence"] = write_proof(root / "v2-protocol.json", protocol)
    # These version-shape fixtures mock the protocol canonical hash. Match the
    # producer's v2 arm contract, including its source/toolchain proof links.
    for record in campaign["records"]:
        for name in ("baseline", "candidate"):
            arm = record[name]
            raw = STATS._proof_json(arm["evidence"])
            raw.update(schema_version=2, protocol_sha256=campaign["protocol_sha256"],
                       source_receipt_sha256=protocol["source"]["receipt_sha256"], toolchain_sha256=campaign["protocol_sha256"])
            raw["config"] = configs[name]
            arm["config_sha256"] = hashlib.sha256(json.dumps(configs[name], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
            for phase_name, field in (("primary", "jobs"), ("feedback-warm", "feedback_warm_jobs"), ("edit", "edit_jobs")):
                for capture in raw[field]:
                    fixed = next(job for job in STATS._runner_module().JOBS if job["name"] == capture["job"])
                    selected = Path(raw["target_root"]) / "primary" / fixed["name"]
                    capture["argv"] = STATS._runner_module().job_command(protocol, fixed, selected)
                    capture["cwd"] = raw["source_root"]
            arm["evidence"] = write_proof(Path(arm["evidence"]["path"]), raw)
    edit = campaign["qualification_receipts"].get("edit")
    if edit is not None and campaign["records"]:
        receipt = STATS._proof_json(edit)
        receipt["samples"] = [sample for sample in receipt["samples"] if sample["pair_index"] < count]
        for sample in receipt["samples"]:
            sample["evidence"] = campaign["records"][sample["pair_index"]][sample["arm"]]["evidence"]
        campaign["qualification_receipts"]["edit"] = write_proof(Path(edit["path"]), receipt)
    return protocol


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
            self.assertFalse((source.parent / "__pycache__").exists(), "approved validator import must not write into frozen source")
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
            # V2 follows the exact independent allocation understood by the
            # frozen official producer. The validator import/report API is real;
            # only the synthetic fixture's Git/source setup is mocked above.
            for phase in ("pilot", "capacity"):
                pair_index, attempt = (None, None) if phase == "capacity" else (0, 0)
                suite_name = f"asg-build-feedback-{phase}-{pair_index if pair_index is not None else 'capacity'}-{attempt if attempt is not None else 0}-baseline"
                suite = root / "evidence-output" / "build-feedback" / suite_name
                exact_output = suite / "report"
                exact_output.mkdir(parents=True)
                declared_binary = write_proof(suite / "baseline-cli.exe", b"synthetic suite binary")
                exact_report = copy.deepcopy(report)
                exact_report["baseline"]["suite_id"] = suite.name
                declared_report = write_proof(exact_output / "official.json", exact_report)
                allocated = {"pair_index": pair_index, "attempt": attempt, "arm": "baseline",
                             "source_root": str(root), "release_suite_root": str(suite)}
                v2_protocol = {**protocol, "schema_version": 2, "phase": phase,
                               "qualification_allocations": [allocated]}
                if phase == "capacity":
                    v2_protocol["capacity_probe"] = {"kind": "full-arm-and-qualification", "arm": "baseline"}
                v2_proof = copy.deepcopy(proof)
                v2_proof.update(pair_index=pair_index, attempt=attempt, arm="baseline",
                                official_report=declared_report, release_binary=declared_binary, artifacts=[declared_binary])
                args = v2_proof["capture"]["argv"]
                args[args.index("--output-dir") + 1] = str(exact_output)
                args[args.index("--suite-id") + 1] = suite.name
                with self.subTest(phase=phase):
                    self.assertEqual(module._official_release_valid(v2_proof, identity, protocol=v2_protocol,
                        arm_evidence=arm_reference), exact_report)
                    wrong_suite = copy.deepcopy(v2_proof)
                    wrong_suite["official_report"] = write_proof(exact_output / "wrong-suite.json", report)
                    wrong_suite["capture"]["argv"][args.index("--suite-id") + 1] = report["baseline"]["suite_id"]
                    with self.assertRaisesRegex(ValueError, "independently declared source output"):
                        module._official_release_valid(wrong_suite, identity, protocol=v2_protocol, arm_evidence=arm_reference)
                    for field in ("pair_index", "attempt", "arm"):
                        changed = copy.deepcopy(v2_proof)
                        changed.pop(field)
                        with self.subTest(missing=field), self.assertRaisesRegex(ValueError, "ordinals missing"):
                            module._official_release_valid(changed, identity, protocol=v2_protocol, arm_evidence=arm_reference)
                    for value in (False, 0 if phase == "capacity" else None, "0"):
                        changed = copy.deepcopy(v2_proof)
                        changed["pair_index"] = value
                        with self.subTest(ordinal=value), self.assertRaisesRegex(ValueError, "ordinals"):
                            module._official_release_valid(changed, identity, protocol=v2_protocol, arm_evidence=arm_reference)
                    for mutation in ({"source_root": str(root / "other-source")},
                                     {"release_suite_root": str(root / "suite")}, {"unexpected": True}):
                        changed = copy.deepcopy(v2_protocol)
                        changed["qualification_allocations"] = [{**allocated, **mutation}]
                        with self.subTest(allocation=mutation), self.assertRaisesRegex(ValueError, "allocation|independently declared"):
                            module._official_release_valid(v2_proof, identity, protocol=changed, arm_evidence=arm_reference)
                    changed = copy.deepcopy(v2_protocol)
                    changed["qualification_allocations"] = [allocated, allocated]
                    with self.assertRaisesRegex(ValueError, "allocation does not match"):
                        module._official_release_valid(v2_proof, identity, protocol=changed, arm_evidence=arm_reference)
                    unrelated = copy.deepcopy(proof)
                    unrelated.update(pair_index=pair_index, attempt=attempt, arm="baseline")
                    with self.assertRaisesRegex(ValueError, "independently declared source output"):
                        module._official_release_valid(unrelated, identity, protocol=v2_protocol, arm_evidence=arm_reference)
                    nested_output = exact_output / "extra"
                    nested_output.mkdir()
                    changed = copy.deepcopy(v2_proof)
                    changed["official_report"] = write_proof(nested_output / "official.json", exact_report)
                    changed["capture"]["argv"][args.index("--output-dir") + 1] = str(nested_output)
                    with self.assertRaisesRegex(ValueError, "independently declared source output"):
                        module._official_release_valid(changed, identity, protocol=v2_protocol, arm_evidence=arm_reference)
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


class BoundedEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.projection_patch = patch.object(STATS, "_verify_runner_campaign_projection")
        self.projection_patch.start()
        self.addCleanup(self.projection_patch.stop)

    def tearDown(self):
        while _ACTIVE_PROOF_ROOTS:
            _ACTIVE_PROOF_ROOTS.pop().cleanup()

    def test_cargo_test_tail_braces_share_runner_semantics(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        capture = raw["jobs"][0]
        path = Path(capture["stdout_path"])
        original = path.read_bytes()
        tails = ((b'{test text after Cargo}\n', True),
                 (b'{"reason":"build-finished","success":true}\n', False),
                 (b'{"reason":"compiler-artifact","filenames":[]}\n', False),
                 (b'\xff', False))
        for tail, accepted in tails:
            path.write_bytes(original + tail)
            capture["stdout_sha256"] = hashlib.sha256(original + tail).hexdigest()
            arm["evidence"] = write_proof(Path(arm["evidence"]["path"]), raw)
            if accepted:
                self.assertTrue(STATS._arm_proof_valid(arm, "baseline", campaign["identity"]))
            else:
                try:
                    result = STATS._arm_proof_valid(arm, "baseline", campaign["identity"])
                except (ValueError, UnicodeError):
                    result = False
                self.assertFalse(result)
        path.write_bytes(b'{bad before Cargo}\n' + original)
        capture["stdout_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        arm["evidence"] = write_proof(Path(arm["evidence"]["path"]), raw)
        self.assertFalse(STATS._arm_proof_valid(arm, "baseline", campaign["identity"]))

    def test_capacity_is_never_paired_performance_or_adoption(self):
        campaign = campaign_fixture("pilot")
        protocol = bind_synthetic_v2(campaign, phase="capacity", pairs=0)
        campaign["capacity_observation"] = {"status": "capacity_limited", "peak_owned_output_bytes": 1234}
        runner = STATS._runner_module()
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "canonical_sha256", return_value=campaign["protocol_sha256"]):
            result = STATS.compare_campaign(campaign)
        self.assertFalse(result["qualified"])
        self.assertEqual(result["completed_valid_pairs"], 0)
        self.assertFalse(result["performance_evidence"])
        self.assertNotIn("adopted", result)
        self.assertEqual(result["capacity_observation"]["status"], "capacity_limited")
        self.assertEqual(protocol["pairs"], 0)

    def test_v2_arm_job_label_cannot_substitute_command_or_source(self):
        campaign = campaign_fixture("pilot")
        protocol = bind_synthetic_v2(campaign)
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        runner = STATS._runner_module()
        with patch.object(STATS, "_arm_binding_valid", return_value=True), patch.object(runner, "canonical_sha256", return_value=campaign["protocol_sha256"]):
            self.assertTrue(STATS._arm_proof_valid(arm, "baseline", campaign["identity"], protocol=protocol, pair_index=0, attempt=0))
            for field, value in (("argv", ["python", "unrelated.py"]), ("cwd", str(Path(arm["source_root"]) / "elsewhere"))):
                changed = copy.deepcopy(raw)
                changed["edit_jobs"][0][field] = value
                arm["evidence"] = write_proof(Path(arm["evidence"]["path"]), changed)
                self.assertFalse(STATS._arm_proof_valid(arm, "baseline", campaign["identity"], protocol=protocol, pair_index=0, attempt=0))

    def test_v2_qualification_binds_compiler_source_scope_and_target(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        run_root = Path(_ACTIVE_PROOF_ROOTS[-1].name) / "approved-run"
        arm_root = run_root / "pairs" / "pair-0000-attempt-00" / "baseline"
        (arm_root / "targets").mkdir(parents=True)
        raw["target_root"] = str(arm_root / "targets")
        arm["evidence"] = write_proof(arm_root / "arm.json", raw)
        protocol = {"schema_version": 2, "phase": "pilot", "run_root": str(run_root),
                    "configs": {"baseline": raw["config"]},
                    "allocations": [{"pair_index": 0, "attempt": 0, "baseline_source_root": arm["source_root"]}],
                    "toolchain": {"cargo": {"path": "approved-cargo"}}}
        for name in ("clippy", "optional_features"):
            receipt = STATS._proof_json(campaign["qualification_receipts"][name])
            proof = STATS._proof_json(receipt["samples"][0]["evidence"])
            base = ["approved-cargo", "clippy" if name == "clippy" else "test", "--workspace", "--all-targets"]
            if name == "optional_features":
                base.append("--all-features")
            target = Path(raw["target_root"]) / "qualification" / name
            attach_qualification_cargo(proof, target, Path(arm["source_root"]))
            base.extend(["--locked", "--offline", "-j2", "--message-format=json", "--target-dir", str(target)])
            base.extend(["--", "-D", "warnings"] if name == "clippy" else ["--", "--test-threads=2"])
            original_report = STATS._proof_json(proof["report"])
            def candidate(argv, cwd, proof=proof, original_report=original_report):
                changed = copy.deepcopy(proof)
                changed["capture"].update(argv=argv, cwd=cwd)
                report = copy.deepcopy(original_report)
                report["argv"] = argv
                report["stdout_sha256"] = changed["capture"]["stdout_sha256"]
                report["artifacts"] = changed["artifacts"]
                report["capture_sha256"] = hashlib.sha256(json.dumps(changed["capture"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
                changed["report"] = write_proof(Path(proof["report"]["path"]), report)
                return changed
            accepted = candidate(base, arm["source_root"])
            self.assertTrue(STATS._command_proof_valid(accepted, name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
            narrow = [part for part in base if part != "--workspace"]
            wrong_target = [str(target.parent / "wrong") if part == str(target) else part for part in base]
            for argv, cwd in ((["unrelated-exe", *base[1:]], arm["source_root"]),
                              (base, str(Path(arm["source_root"]) / "unrelated")),
                              (narrow, arm["source_root"]), (wrong_target, arm["source_root"])):
                changed = candidate(argv, cwd)
                self.assertFalse(STATS._command_proof_valid(changed, name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))

    def test_full_capacity_qualification_keeps_null_ordinals_and_exact_arm(self):
        """Synthetic commands retain real hash/report/source/arm binding checks."""
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        run_root = Path(_ACTIVE_PROOF_ROOTS[-1].name) / "full-capacity-run"
        arm_root = run_root / "capacity"
        (arm_root / "targets").mkdir(parents=True)
        raw.update(schema_version=2, phase="capacity", performance_evidence=False, pair_index=None, attempt=None,
                   completed_valid_pairs=0, target_root=str(arm_root / "targets"))
        arm["evidence"] = write_proof(arm_root / "arm.json", raw)
        protocol = {"schema_version": 2, "phase": "capacity", "pairs": 0, "allocations": [],
                    "run_root": str(run_root), "configs": {"baseline": raw["config"]},
                    "capacity_probe": {"kind": "full-arm-and-qualification", "stage": "expanded",
                                       "arm": "baseline", "source_root": arm["source_root"]},
                    "toolchain": {"cargo": {"path": "approved-cargo"}}}
        diagnostic = Path(raw["target_root"]) / "diagnostic.exe"
        write_proof(diagnostic, b"synthetic emitted diagnostic executable")
        emitted_diagnostic = STATS._runner_module()._artifact_identity(diagnostic)
        raw["jobs"][0]["cargo"]["artifacts"].append({"files": [emitted_diagnostic]})
        arm["evidence"] = write_proof(arm_root / "arm.json", raw)
        source_file = Path(arm["source_root"]) / "source.rs"
        source_receipt = write_proof(arm_root / "approved-source.json", {"source_identity": {"files": [
            {"path": "source.rs", "sha256": hashlib.sha256(source_file.read_bytes()).hexdigest(), "bytes": source_file.stat().st_size}]}})
        protocol["source"] = {"receipt_path": source_receipt["path"], "receipt_sha256": source_receipt["sha256"]}
        diagnostic_argv = [str(diagnostic), "synthetic-diagnostic"]
        policy = SimpleNamespace(validate_policy=lambda protocol: ({}, {"diagnostic_commands": {arm["config_sha256"]: diagnostic_argv}}))
        policy_patch = patch.object(STATS, "_retention_module", return_value=policy)
        policy_patch.start()
        self.addCleanup(policy_patch.stop)
        for name in ("clippy", "optional_features", "diagnostics", "release"):
            receipt = STATS._proof_json(campaign["qualification_receipts"][name])
            original = STATS._proof_json(receipt["samples"][0]["evidence"])
            original.update(pair_index=None, attempt=None)
            original["capture"]["cwd"] = arm["source_root"]
            if name in ("clippy", "optional_features"):
                argv = ["approved-cargo", "clippy" if name == "clippy" else "test", "--workspace", "--all-targets"]
                if name == "optional_features":
                    argv.append("--all-features")
                argv.extend(["--locked", "--offline", "-j2", "--message-format=json", "--target-dir",
                             str(arm_root / "targets" / "qualification" / name)])
                argv.extend(["--", "-D", "warnings"] if name == "clippy" else ["--", "--test-threads=2"])
                original["capture"]["argv"] = argv
                attach_qualification_cargo(original, arm_root / "targets" / "qualification" / name, Path(arm["source_root"]))
            if name == "diagnostics":
                original["capture"]["argv"] = diagnostic_argv
            original_report = STATS._proof_json(original["report"])

            def candidate(mutation=None, *, report_mutation=None, original=original, original_report=original_report):
                proof = copy.deepcopy(original)
                if mutation is not None:
                    proof.update(mutation)
                report = copy.deepcopy(original_report)
                report.update({field: proof[field] for field in ("pair_index", "attempt", "arm", "source_root", "config_sha256") if field in proof})
                report["argv"] = proof["capture"]["argv"]
                report["stdout_sha256"] = proof["capture"]["stdout_sha256"]
                report["artifacts"] = proof["artifacts"]
                report["capture_sha256"] = hashlib.sha256(json.dumps(proof["capture"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
                if report_mutation is not None:
                    report_mutation(report)
                proof["report"] = write_proof(Path(original["report"]["path"]), report)
                return proof

            with self.subTest(name=name):
                self.assertTrue(STATS._command_proof_valid(candidate(), name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
                for mutation in ({"pair_index": 0}, {"attempt": 0}, {"pair_index": False}, {"attempt": False},
                                 {"arm": "candidate"}, {"source_root": str(run_root / "other-source")}, {"config_sha256": "0" * 64}):
                    with self.subTest(mutation=mutation):
                        self.assertFalse(STATS._command_proof_valid(candidate(mutation), name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
                for field in ("pair_index", "attempt", "arm"):
                    changed = candidate()
                    changed.pop(field)
                    self.assertFalse(STATS._command_proof_valid(changed, name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
                for field in ("pair_index", "attempt"):
                    proof = candidate(report_mutation=lambda report, field=field: report.pop(field))
                    self.assertFalse(STATS._command_proof_valid(proof, name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))

    def test_v2_qualification_rejects_non_cargo_logs_and_mismatched_emitted_files(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        run_root = Path(_ACTIVE_PROOF_ROOTS[-1].name) / "qualified-run"
        arm_root = run_root / "pairs" / "pair-0000-attempt-00" / "baseline"
        (arm_root / "targets").mkdir(parents=True)
        raw["target_root"] = str(arm_root / "targets")
        arm["evidence"] = write_proof(arm_root / "arm.json", raw)
        protocol = {"schema_version": 2, "phase": "pilot", "run_root": str(run_root), "configs": {"baseline": raw["config"]},
                    "allocations": [{"pair_index": 0, "attempt": 0, "baseline_source_root": arm["source_root"]}],
                    "toolchain": {"cargo": {"path": "approved-cargo"}}}
        for name in ("clippy", "optional_features"):
            receipt = STATS._proof_json(campaign["qualification_receipts"][name])
            original = STATS._proof_json(receipt["samples"][0]["evidence"])
            target = arm_root / "targets" / "qualification" / name
            argv = ["approved-cargo", "clippy" if name == "clippy" else "test", "--workspace", "--all-targets"]
            if name == "optional_features":
                argv.append("--all-features")
            argv.extend(["--locked", "--offline", "-j2", "--message-format=json", "--target-dir", str(target)])
            argv.extend(["--", "-D", "warnings"] if name == "clippy" else ["--", "--test-threads=2"])
            original["capture"].update(argv=argv, cwd=arm["source_root"])
            attach_qualification_cargo(original, target, Path(arm["source_root"]))
            original_report = STATS._proof_json(original["report"])
            original_log = Path(original["capture"]["stdout_path"]).read_bytes()

            def candidate(mutation=None, *, output=original_log, original=original, original_report=original_report):
                proof = copy.deepcopy(original)
                proof["capture"]["stdout_sha256"] = write_proof(Path(proof["capture"]["stdout_path"]), output)["sha256"]
                if mutation is not None:
                    mutation(proof)
                report = copy.deepcopy(original_report)
                report.update(argv=proof["capture"]["argv"], stdout_sha256=proof["capture"]["stdout_sha256"], artifacts=proof["artifacts"],
                              capture_sha256=STATS._runner_module().canonical_sha256(proof["capture"]))
                proof["report"] = write_proof(Path(proof["report"]["path"]), report)
                return proof

            with self.subTest(name=name):
                self.assertTrue(STATS._command_proof_valid(candidate(), name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
                for output in (b"pretend success\n", b'{"reason":"build-finished","success":true}\n'):
                    with self.subTest(output=output):
                        self.assertFalse(STATS._command_proof_valid(candidate(output=output), name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
                self.assertFalse(STATS._command_proof_valid(candidate(lambda proof: proof["capture"].pop("cargo")), name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
                unrelated = write_proof(target / "unrelated.rmeta", b"not emitted by this command")
                self.assertFalse(STATS._command_proof_valid(candidate(lambda proof, unrelated=unrelated: proof.update(artifacts=[unrelated])), name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
                proof = candidate()
                artifact = proof["artifacts"][0]
                artifact_path = Path(artifact["path"])
                retained = {"seal": {"target": str(arm_root / "targets"), "tree": {"entries": [
                    {"relative": artifact_path.relative_to(arm_root / "targets").as_posix(), "directory": False,
                     "sha256": artifact["sha256"], "bytes": artifact_path.stat().st_size}]},
                    "qualifications": {name: {"reference": {"synthetic": "already-verified"}, "proof": proof}}}}
                artifact_path.unlink()
                with patch.object(STATS._runner_module(), "parse_cargo_json_file", side_effect=AssertionError("retained bytes cannot be replayed")):
                    self.assertTrue(STATS._command_proof_valid(proof, name, arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol,
                        retained=retained, evidence_reference={"synthetic": "already-verified"}))

    def test_v2_diagnostics_require_approved_emitted_executable_and_frozen_source(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        run_root = Path(_ACTIVE_PROOF_ROOTS[-1].name) / "diagnostic-run"
        arm_root = run_root / "pairs" / "pair-0000-attempt-00" / "baseline"
        (arm_root / "targets").mkdir(parents=True)
        raw["target_root"] = str(arm_root / "targets")
        executable = arm_root / "targets" / "diagnostic.exe"
        write_proof(executable, b"synthetic emitted executable")
        emitted = STATS._runner_module()._artifact_identity(executable)
        raw["jobs"][0]["cargo"]["artifacts"].append({"files": [emitted]})
        arm["evidence"] = write_proof(arm_root / "arm.json", raw)
        source = Path(arm["source_root"]) / "source.rs"
        manifest = [{"path": "source.rs", "sha256": hashlib.sha256(source.read_bytes()).hexdigest(), "bytes": source.stat().st_size}]
        source_reference = write_proof(run_root / "approved-source.json", {"source_identity": {"files": manifest}})
        protocol = {"schema_version": 2, "phase": "pilot", "run_root": str(run_root), "configs": {"baseline": raw["config"]},
                    "allocations": [{"pair_index": 0, "attempt": 0, "baseline_source_root": arm["source_root"]}],
                    "source": {"receipt_path": source_reference["path"], "receipt_sha256": source_reference["sha256"]}}
        receipt = STATS._proof_json(campaign["qualification_receipts"]["diagnostics"])
        original = STATS._proof_json(receipt["samples"][0]["evidence"])
        argv = [str(executable), "diagnostic-probe"]
        original["capture"].update(argv=argv, cwd=arm["source_root"])
        original["artifacts"] = [emitted]
        original_report = STATS._proof_json(original["report"])

        def candidate(command=argv, *, check_source=source):
            proof = copy.deepcopy(original)
            proof["capture"]["argv"] = command
            output = f"{check_source.as_posix()}:1: synthetic diagnostic\n".encode()
            proof["capture"]["stdout_sha256"] = write_proof(Path(proof["capture"]["stdout_path"]), output)["sha256"]
            report = copy.deepcopy(original_report)
            report.update(argv=command, stdout_sha256=proof["capture"]["stdout_sha256"], artifacts=proof["artifacts"],
                          capture_sha256=STATS._runner_module().canonical_sha256(proof["capture"]))
            report["checks"][0].update(source={"path": str(check_source), "sha256": hashlib.sha256(check_source.read_bytes()).hexdigest()},
                                       actual_file=str(check_source))
            proof["report"] = write_proof(Path(proof["report"]["path"]), report)
            return proof

        approval = {"diagnostic_commands": {arm["config_sha256"]: argv}}
        helper = SimpleNamespace(validate_policy=lambda approved: ({}, approval))
        with patch.object(STATS, "_retention_module", return_value=helper):
            self.assertTrue(STATS._command_proof_valid(candidate(), "diagnostics", arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
            self.assertFalse(STATS._command_proof_valid(candidate(["unrelated.exe", "diagnostic-probe"]), "diagnostics", arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
            outside = run_root / "outside.rs"
            outside.write_bytes(b"foreign source")
            self.assertFalse(STATS._command_proof_valid(candidate(check_source=outside), "diagnostics", arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
            source.write_bytes(b"changed frozen source")
            self.assertFalse(STATS._command_proof_valid(candidate(), "diagnostics", arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))
            unrecorded = arm_root / "targets" / "unrecorded.exe"
            unrecorded.write_bytes(b"unrecorded")
            approval["diagnostic_commands"][arm["config_sha256"]] = [str(unrecorded), "diagnostic-probe"]
            self.assertFalse(STATS._command_proof_valid(candidate([str(unrecorded), "diagnostic-probe"]), "diagnostics", arm, campaign["identity"], campaign["protocol_sha256"], protocol=protocol))

    def test_full_capacity_binding_and_retained_pair_boundary(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        run_root = Path(_ACTIVE_PROOF_ROOTS[-1].name) / "full-capacity-run"
        arm_root = run_root / "capacity"
        (arm_root / "targets").mkdir(parents=True)
        raw.update(schema_version=2, phase="capacity", performance_evidence=False, pair_index=None, attempt=None,
                   completed_valid_pairs=0, target_root=str(arm_root / "targets"))
        arm["evidence"] = write_proof(arm_root / "arm.json", raw)
        protocol = {"schema_version": 2, "phase": "capacity", "pairs": 0, "allocations": [],
                    "run_root": str(run_root), "configs": {"baseline": raw["config"]},
                    "capacity_probe": {"kind": "full-arm-and-qualification", "stage": "expanded",
                                       "arm": "baseline", "source_root": arm["source_root"]}}
        self.assertTrue(STATS._arm_binding_valid(arm, raw, "baseline", protocol, None, None))
        for mutation in ({"phase": "pilot"}, {"performance_evidence": True}, {"completed_valid_pairs": False},
                         {"completed_valid_pairs": 1}, {"pair_index": 0}, {"attempt": False},
                         {"source_root": str(run_root / "other-source")},
                         {"config": {"debug": "other"}}):
            changed = {**raw, **mutation}
            changed_arm = {**arm, "evidence": write_proof(arm_root / "arm.json", changed)}
            with self.subTest(mutation=mutation):
                self.assertFalse(STATS._arm_binding_valid(changed_arm, changed, "baseline", protocol, None, None))
        for field in ("pair_index", "attempt"):
            changed = {key: value for key, value in raw.items() if key != field}
            changed_arm = {**arm, "evidence": write_proof(arm_root / "arm.json", changed)}
            self.assertFalse(STATS._arm_binding_valid(changed_arm, changed, "baseline", protocol, None, None))
        arm["evidence"] = write_proof(arm_root / "arm.json", raw)
        for pair_index, attempt in ((0, None), (None, 0), (False, None), (None, False)):
            self.assertFalse(STATS._arm_binding_valid(arm, raw, "baseline", protocol, pair_index, attempt))
        for mutation in ({"pairs": False}, {"pairs": 1}, {"allocations": [{}]}, {"schema_version": 1}):
            self.assertFalse(STATS._arm_binding_valid(arm, raw, "baseline", {**protocol, **mutation}, None, None))
        other_root = run_root / "other"
        (other_root / "targets").mkdir(parents=True)
        relocated = {**raw, "target_root": str(other_root / "targets")}
        changed_arm = {**arm, "evidence": write_proof(other_root / "arm.json", relocated)}
        self.assertFalse(STATS._arm_binding_valid(changed_arm, relocated, "baseline", protocol, None, None))
        retained = {**arm, "retained_evidence": {"verified": True}}
        with patch.object(STATS, "_retention_module", side_effect=AssertionError("must not enter paired retention verifier")), \
             self.assertRaisesRegex(ValueError, "bound v2 paired allocation"):
            STATS._retained_arm_result(retained, "baseline", protocol, None, None)

    def test_v2_formal_count_locked_to_protocol_and_registration(self):
        campaign = campaign_fixture()
        protocol = bind_synthetic_v2(campaign, pairs=12)
        runner = STATS._runner_module()
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "canonical_sha256", return_value=campaign["protocol_sha256"]):
            self.assertEqual(STATS._campaign_protocol(campaign["protocol_evidence"], campaign["identity"], campaign["protocol_sha256"], campaign)["pairs"], 12)
            changed = copy.deepcopy(campaign)
            changed["pairs_required"] = 20
            result = STATS.compare_campaign(changed)
            self.assertFalse(result["adopted"])
            self.assertIn("v2_protocol_missing_or_invalid", result["reasons"])
            plan = STATS._proof_json(protocol["formal_registration"])
            for mutation in ({"pairs": 20}, {"approved": False}, {"stopping_rule": "until-significant"}):
                protocol["formal_registration"] = write_proof(Path(protocol["formal_registration"]["path"]), {**plan, **mutation})
                reference = write_proof(Path(campaign["protocol_evidence"]["path"]), protocol)
                with self.assertRaisesRegex(ValueError, "registration"):
                    STATS._campaign_protocol(reference, campaign["identity"], campaign["protocol_sha256"], campaign)

    def test_v2_complete_pilot_keeps_formal_n_pending_and_uncertainty_gates(self):
        campaign = campaign_fixture("pilot")
        protocol = bind_synthetic_v2(campaign)
        runner = STATS._runner_module()
        # These synthetic proofs isolate fixed-N forecasting from live source
        # setup; runtime source/layout checks have their own explicit regression.
        with patch.object(runner, "_validate_protocol"), patch.object(runner, "canonical_sha256", return_value=campaign["protocol_sha256"]), \
             patch.object(STATS, "_arm_binding_valid", return_value=True):
            result = STATS.compare_campaign(campaign)
        self.assertEqual(result["completed_valid_pairs"], 5)
        self.assertIsNone(result["forecast"]["formal_pairs"])
        self.assertIsNone(result["forecast"]["projected_compute_hours"])
        self.assertEqual(result["forecast"]["formal_pair_selection"], "approve-and-freeze-after-pilot-before-formal")
        self.assertNotIn("adopted", result)
        self.assertFalse(result["qualified"])
        self.assertIn("qualification_release_cli_workload_timing_unverified", result["reasons"])
        self.assertEqual(protocol["pairs"], 5)

    def test_arm_binding_rejects_protocol_config_and_source_substitution(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        root = Path(_ACTIVE_PROOF_ROOTS[-1].name) / "approved-run"
        local = root / "pairs" / "pair-0000-attempt-00" / "baseline"
        (local / "targets").mkdir(parents=True)
        raw["target_root"] = str(local / "targets")
        arm["evidence"] = write_proof(local / "arm.json", raw)
        protocol = {"run_root": str(root), "configs": {"baseline": raw["config"]},
                    "allocations": [{"pair_index": 0, "attempt": 0, "baseline_source_root": arm["source_root"]}]}
        self.assertTrue(STATS._arm_binding_valid(arm, raw, "baseline", protocol, 0, 0))
        altered = copy.deepcopy(protocol)
        altered["configs"]["baseline"] = {"debug": "different"}
        self.assertFalse(STATS._arm_binding_valid(arm, raw, "baseline", altered, 0, 0))
        altered = copy.deepcopy(protocol)
        altered["allocations"][0]["baseline_source_root"] = str(root / "other-source")
        self.assertFalse(STATS._arm_binding_valid(arm, raw, "baseline", altered, 0, 0))
        self.assertFalse(STATS._arm_binding_valid(arm, raw, "baseline", protocol, 1, 0))

    def test_v2_requires_pinned_protocol_even_for_pilot_and_capacity(self):
        for phase in ("pilot", "capacity"):
            campaign = campaign_fixture("pilot")
            campaign.update(schema_version=2, phase=phase)
            result = STATS.compare_campaign(campaign)
            self.assertFalse(result["qualified"])
            self.assertIn("v2_protocol_missing_or_invalid", result["reasons"])

    def test_missing_live_artifact_and_forged_retained_flags_stay_rejected(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        Path(raw["jobs"][0]["cargo"]["artifacts"][0]["files"][0]["path"]).unlink()
        with self.assertRaises(OSError):
            STATS._arm_proof_valid(arm, "baseline", campaign["identity"])
        arm["retained_evidence"] = {"verified": True}
        with self.assertRaisesRegex(ValueError, "bound v2"):
            STATS._arm_proof_valid(arm, "baseline", campaign["identity"])
        protocol = {"schema_version": 2, "phase": "pilot", "source": {"receipt_sha256": "e" * 64}, "toolchain": {}}
        runner = STATS._runner_module()
        raw.update(schema_version=2, protocol_sha256=runner.canonical_sha256(protocol), source_receipt_sha256="e" * 64,
                   toolchain_sha256=runner.canonical_sha256({}))
        arm["evidence"] = write_proof(Path(arm["evidence"]["path"]), raw)
        helper = SimpleNamespace(verify_retained_arm=Mock(side_effect=ValueError("unapproved seal")))
        with patch.object(STATS, "_retention_module", return_value=helper), self.assertRaisesRegex(ValueError, "unapproved seal"):
            STATS._arm_proof_valid(arm, "baseline", campaign["identity"], protocol=protocol, pair_index=0, attempt=0)

    def test_retained_command_matches_verified_seal_and_rechecks_kept_logs(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        receipt = STATS._proof_json(campaign["qualification_receipts"]["clippy"])
        reference = receipt["samples"][0]["evidence"]
        proof = STATS._proof_json(reference)
        artifact = proof["artifacts"][0]
        path = Path(artifact["path"])
        retained = {"seal": {"target": str(path.parent),
            "tree": {"entries": [{"relative": path.name, "directory": False, "sha256": artifact["sha256"], "bytes": path.stat().st_size}]},
            "qualifications": {"clippy": {"reference": reference, "proof": proof}}}}
        path.unlink()
        self.assertTrue(STATS._command_proof_valid(proof, "clippy", arm, campaign["identity"], campaign["protocol_sha256"], retained=retained, evidence_reference=reference))
        altered = copy.deepcopy(proof)
        altered["capture"]["duration_ms"] += 1
        with self.assertRaisesRegex(ValueError, "prerecycle qualified evidence"):
            STATS._command_proof_valid(altered, "clippy", arm, campaign["identity"], campaign["protocol_sha256"], retained=retained, evidence_reference=reference)
        with Path(proof["capture"]["stdout_path"]).open("ab") as stream:
            stream.write(b"changed retained log tail")
        with self.assertRaisesRegex(ValueError, "hash changed"):
            STATS._command_proof_valid(proof, "clippy", arm, campaign["identity"], campaign["protocol_sha256"], retained=retained, evidence_reference=reference)

    def test_verified_retained_branch_requires_exact_original_projection(self):
        campaign = campaign_fixture("pilot")
        arm = campaign["records"][0]["baseline"]
        raw = STATS._proof_json(arm["evidence"])
        file = raw["jobs"][0]["cargo"]["artifacts"][0]["files"][0]
        target = Path(raw["target_root"])
        original = copy.deepcopy(arm)
        seal = {"arm_projection": original, "arm_reference": arm["evidence"], "target": str(target),
                "tree": {"entries": [{"relative": "synthetic.exe", "directory": False, "sha256": file["sha256"], "bytes": Path(file["path"]).stat().st_size}]}}
        Path(file["path"]).unlink()
        arm["retained_evidence"] = {"path": "synthetic retained link", "sha256": "f" * 64}
        verified = {"arm_receipt": raw, "seal": seal, "disposition": {"status": "complete"}}
        helper = SimpleNamespace(verify_retained_arm=Mock(return_value=verified))
        with patch.object(STATS, "_retention_module", return_value=helper):
            self.assertEqual(STATS._retained_arm_result(arm, "baseline", {"schema_version": 2, "phase": "pilot"}, 0, 0), verified)
            STATS._artifact_proof_valid(file, verified)
            helper.verify_retained_arm.assert_called_once()
            arm["allocated_bytes"] += 1
            with self.assertRaisesRegex(ValueError, "original projection"):
                STATS._retained_arm_result(arm, "baseline", {"schema_version": 2, "phase": "pilot"}, 0, 0)


@unittest.skipUnless(os.name == "nt", "uses NTFS identity for synthetic retained output")

class RawCampaignProjectionTests(unittest.TestCase):
    """Real campaign path/hash/attempt projection; process execution is synthetic."""
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.raw_path = self.root / "campaign.json"
        self.protocol_path = self.root / "protocol.json"
        tool = {"version": "synthetic version", "sha256": "c" * 64}
        self.protocol = {"schema_version": 2, "source": {"commit": "a" * 40, "tree": "b" * 40, "lock_sha256": "d" * 64, "resources_sha256": "e" * 64},
            "baseline": {"allocated_bytes": 0}, "run_root": str(self.root),
            "toolchain": {"rustc": tool, "cargo": tool, "msvc": {"compiler": tool, "linker": tool}, "target": "x86_64-pc-windows-msvc"},
            "phase": "pilot", "pairs": 5, "jobs": 2, "test_threads": 2, "seed": 7,
            "order": [["baseline", "candidate"]] * 5, "configs": {name: {"tool": {"kind": "native"}} for name in ("baseline", "candidate")},
            "formal_registration": None}
        self.protocol_reference = write_proof(self.protocol_path, self.protocol)
        self.runner = STATS._runner_module()
        self.cli = STATS._benchmark_module()
        stack = contextlib.ExitStack()
        self.addCleanup(stack.close)
        stack.enter_context(patch.object(self.runner, "_validate_protocol"))
        stack.enter_context(patch.object(self.cli, "_module", side_effect=lambda name: self.runner if name == "runner" else STATS._retention_module()))
        self.raw = {"schema_version": 2, "kind": "build-feedback-campaign", "protocol_path": str(self.protocol_path),
                    "protocol_sha256": self.runner.canonical_sha256(self.protocol), "phase": "pilot", "planned_pairs": 5,
                    "source_commit": "a" * 40, "source_tree": "b" * 40,
                    "toolchain_sha256": self.runner.canonical_sha256(self.protocol["toolchain"]),
                    "status": "failed", "complete": False, "completed_valid_pairs": 0, "inflight": None,
                    "reasons": ["budget_limited"], "pairs": [{"pair_index": 0, "attempt": 0, "order": self.protocol["order"][0],
                         "status": "budget_limited", "finalization_reasons": ["registered_storage_limit_reached"], "arms": {}, "arm_receipts": {}}]}

    def project(self, raw=None):
        raw = self.raw if raw is None else raw
        raw["integrity_sha256"] = self.runner.canonical_sha256({key: value for key, value in raw.items() if key != "integrity_sha256"})
        write_proof(self.raw_path, raw)
        campaign = self.cli.normalize_runner_campaign(raw, self.protocol, {})
        campaign["protocol_evidence"] = self.protocol_reference
        return campaign

    def test_exact_raw_failed_campaign_reprojects_and_rejects_status_or_attempt_loss(self):
        campaign = self.project()
        STATS._verify_runner_campaign_projection(campaign, self.protocol)
        mutations = []
        changed = copy.deepcopy(campaign)
        changed["records"][0]["status"] = "valid"
        mutations.append(changed)
        changed = copy.deepcopy(campaign)
        changed["records"] = []
        mutations.append(changed)
        changed = copy.deepcopy(campaign)
        changed["admission_reasons"] = []
        mutations.append(changed)
        changed = copy.deepcopy(campaign)
        changed["runner_state"]["status"] = "complete"
        mutations.append(changed)
        changed = copy.deepcopy(campaign)
        changed["records"][0]["finalization_reasons"] = []
        mutations.append(changed)
        changed = copy.deepcopy(campaign)
        changed["records"][0]["pair_index"] = False
        mutations.append(changed)
        for changed in mutations:
            with self.assertRaisesRegex(ValueError, "pinned raw runner projection"):
                STATS._verify_runner_campaign_projection(changed, self.protocol)
        # The public comparison cannot turn this raw failed attempt valid.
        changed = copy.deepcopy(campaign)
        changed["records"] = []
        result = STATS.compare_campaign(changed)
        self.assertFalse(result["qualified"])
        self.assertIn("v2_protocol_missing_or_invalid", result["reasons"])

    def test_successfully_sealed_arms_do_not_override_raw_pair_budget_failure(self):
        captures = [{"job": name, "status": "ok", "returncode": 0, "duration_ms": 1000,
                     "process_tree_capture_complete": True, "external_processes": []} for name in JOBS]
        for name in ("baseline", "candidate"):
            local = self.root / "pairs" / "pair-0000-attempt-00" / name
            allocation = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 8192,
                          "storage_scope": "owned_targets_and_stores"}
            allocation_ref = write_proof(local / "owned-allocation.json", allocation)
            arm = {"status": "ok", "source_commit": self.protocol["source"]["commit"], "source_tree": self.protocol["source"]["tree"],
                   "source_root": str(local / "source"), "target_root": str(local / "targets"),
                   "process_tree_drained": True, "own_edit_restored": True,
                   "primary_duration_ms": 6000, "duration_ms": 18000, "owned_allocated_bytes": 8192,
                   "allocation_complete": True, "owned_allocation_receipt": allocation,
                   "owned_allocation_receipt_sha256": allocation_ref["sha256"], "config": {},
                   "phase_duration_ms": {"primary": 6000, "feedback-warm": 6000, "edit": 6000},
                   "jobs": captures, "feedback_warm_jobs": captures, "edit_jobs": captures}
            ref = write_proof(local / "arm.json", arm)
            self.raw["pairs"][0]["arms"][name] = arm
            self.raw["pairs"][0]["arm_receipts"][name] = ref
        # Logs and signed seal validation have independent regression coverage;
        # mock only those byte-dependent checks to exercise raw pair semantics.
        with patch.object(self.cli, "_runner_artifacts_verified", return_value=True):
            live = self.project()
        retained = {name: {"arm_receipt": arm, "seal": {"arm_projection": live["records"][0][name]}}
                    for name, arm in self.raw["pairs"][0]["arms"].items()}
        self.raw["pairs"][0]["retained_evidence"] = {name: {"path": str(self.root / (name + "-link.json")), "sha256": "e" * 64}
                                                     for name in ("baseline", "candidate")}
        helper = SimpleNamespace(verify_retained_arm=lambda ref, protocol, name, index, attempt: retained[name])
        with patch.object(self.runner, "_retention_module", return_value=helper), \
             patch.object(self.cli, "_module", side_effect=lambda name: self.runner if name == "runner" else helper):
            sealed = self.project()
            self.assertEqual(sealed["records"][0]["status"], "budget_limited")
            STATS._verify_runner_campaign_projection(sealed, self.protocol)
            altered = copy.deepcopy(sealed)
            altered["records"][0]["status"] = "valid"
            altered.pop("admission_reasons", None)
            with self.assertRaisesRegex(ValueError, "pinned raw runner projection"):
                STATS._verify_runner_campaign_projection(altered, self.protocol)

    def test_changed_raw_hash_wrong_path_and_protocol_identity_refuse(self):
        campaign = self.project()
        self.raw_path.write_bytes(b"changed raw state")
        with self.assertRaisesRegex(ValueError, "hash changed"):
            STATS._verify_runner_campaign_projection(campaign, self.protocol)
        campaign = self.project()
        wrong = write_proof(self.root / "copied-campaign.json", self.raw)
        campaign["runner_campaign_evidence"] = wrong
        with self.assertRaisesRegex(ValueError, "approved run_root"):
            STATS._verify_runner_campaign_projection(campaign, self.protocol)
        altered_raw = copy.deepcopy(self.raw)
        altered_raw["source_commit"] = "f" * 40
        campaign = self.project(altered_raw)
        with self.assertRaisesRegex(ValueError, "raw source/protocol identity"):
            STATS._verify_runner_campaign_projection(campaign, self.protocol)

    def test_normalizer_refuses_in_memory_raw_changed_from_exact_receipt(self):
        self.project()
        changed = copy.deepcopy(self.raw)
        changed["reasons"] = []
        # Integrity is self-rehashable; pinning the actual origin still rejects.
        changed["integrity_sha256"] = self.runner.canonical_sha256({key: value for key, value in changed.items() if key != "integrity_sha256"})
        with self.assertRaisesRegex(ValueError, "pinned raw source"):
            self.cli.normalize_runner_campaign(changed, self.protocol, {})

    def test_over_budget_formal_raw_projection_cannot_hide_registered_limit(self):
        registration = {"kind": "build-feedback-formal-registration", "schema_version": 2,
            "approved": True, "formal_data_collected": False, "pairs": 12, "seed": 7,
            "order": [["baseline", "candidate"]] * 12, "run_root": str(self.root),
            "stopping_rule": "fixed-n-no-adaptive-additions", "maximum_compute_seconds": 10,
            "maximum_storage_bytes": 4096}
        self.protocol.update(phase="formal", pairs=12, order=registration["order"], formal_registration=write_proof(self.root / "registration.json", registration))
        self.protocol_reference = write_proof(self.protocol_path, self.protocol)
        raw = copy.deepcopy(self.raw)
        raw.update(phase="formal", planned_pairs=12, protocol_sha256=self.runner.canonical_sha256(self.protocol),
                   registered_compute_seconds=10.0, formal_registration_sha256=self.protocol["formal_registration"]["sha256"])
        campaign = self.project(raw)
        with self.assertRaisesRegex(ValueError, "registered compute limit reached"):
            STATS._verify_runner_campaign_projection(campaign, self.protocol)
        raw["registered_compute_seconds"] = 1.0
        raw["pairs"][0]["arms"] = {"baseline": {"status": "failed", "jobs": [], "edit_jobs": [], "feedback_warm_jobs": [],
                                               "duration_ms": 0, "peak_allocated_bytes": 4096}}
        arm_root = self.root / "pairs" / "pair-0000-attempt-00" / "baseline"
        arm_reference = write_proof(arm_root / "arm.json", raw["pairs"][0]["arms"]["baseline"])
        raw["pairs"][0]["arm_receipts"] = {"baseline": arm_reference}
        campaign = self.project(raw)
        with self.assertRaisesRegex(ValueError, "registered storage limit reached"):
            STATS._verify_runner_campaign_projection(campaign, self.protocol)


class ProducerEvidenceIntegrationTests(unittest.TestCase):
    def test_actual_runner_v2_arm_normalizes_and_passes_seal_arm_gate(self):
        self._check_producer_arm("pilot")

    def test_actual_runner_full_capacity_arm_normalizes_and_reaches_distinct_seal_gate(self):
        self._check_producer_arm("capacity")

    def _check_producer_arm(self, phase):
        """Run the real receipt producer; only Cargo execution/source setup are synthetic.

        Exact module version/proof/layout/parser and NTFS allocation/lease/HMAC
        checks stay unmocked. Full release qualification is deliberately absent,
        so reaching that next seal gate must still refuse the seal.
        """
        runner = STATS._runner_module()
        spec = importlib.util.spec_from_file_location("bounded_producer_cli", Path(__file__).with_name("build_feedback_benchmark.py"))
        cli = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cli)
        helper = STATS._retention_module()
        inventory = runner._inventory_module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            edit_path = source / runner.DEFAULT_EDIT_PATH
            edit_path.parent.mkdir(parents=True)
            original = b"// synthetic runner source for evidence boundary\n"
            edit_path.write_bytes(original)
            source_receipt = write_proof(root / "source-receipt.json", {"synthetic": True})
            baseline_receipt = write_proof(root / "baseline-receipt.json", {"synthetic": True})
            config = {"debug": "full", "incremental": "on", "layout": "isolated", "tool": {"kind": "native"}}
            identity = {
                "source_commit": "a" * 40, "source_tree": "b" * 40, "lock_sha256": "c" * 64, "resources_sha256": "d" * 64,
                "rustc_version": "rustc 1.97.1 (synthetic)", "cargo_version": "cargo 1.97.1 (synthetic)",
                "rustc_sha256": "e" * 64, "cargo_sha256": "f" * 64, "host": "x86_64-pc-windows-msvc",
                "msvc_compiler_version": "synthetic MSVC", "msvc_linker_version": "synthetic linker",
                "msvc_compiler_sha256": "1" * 64, "msvc_linker_sha256": "2" * 64, "jobs": 2, "test_threads": 2,
            }
            run_root = root / "run"
            full_capacity = phase == "capacity"
            arm_root = run_root / "capacity" if full_capacity else run_root / "pairs" / "pair-0000-attempt-00" / "baseline"
            target = arm_root / "targets"
            protocol = {"schema_version": 2, "kind": "build-feedback-protocol", "phase": phase, "pairs": 0 if full_capacity else 5,
                "source": {"root": str(source), "commit": identity["source_commit"], "tree": identity["source_tree"],
                           "lock_sha256": identity["lock_sha256"], "resources_sha256": identity["resources_sha256"],
                           "receipt_path": source_receipt["path"], "receipt_sha256": source_receipt["sha256"]},
                "baseline": {"allocated_bytes": 0, "receipt_path": baseline_receipt["path"], "receipt_sha256": baseline_receipt["sha256"]},
                "toolchain": {"rustc": {"version": identity["rustc_version"], "sha256": identity["rustc_sha256"]},
                              "cargo": {"path": "synthetic-cargo", "version": identity["cargo_version"], "sha256": identity["cargo_sha256"]},
                              "msvc": {"compiler": {"version": identity["msvc_compiler_version"], "sha256": identity["msvc_compiler_sha256"]},
                                       "linker": {"version": identity["msvc_linker_version"], "sha256": identity["msvc_linker_sha256"]}}, "target": identity["host"]},
                "configs": {"baseline": config, "candidate": config}, "jobs": 2, "test_threads": 2,
                "run_root": str(run_root), "seed": 7, "order": [["baseline", "candidate"]] * 5,
                "budget_policy": {"policy_version": 2, "minimum_free_bytes": 40 * STATS.GIB, "external_growth_allowance_bytes": 8 * STATS.GIB},
                "predicted_growth_bytes": STATS.GIB, "allocations": [{"pair_index": 0, "attempt": 0, "baseline_source_root": str(source), "candidate_source_root": str(source)}],
                "edit": {"path": runner.DEFAULT_EDIT_PATH, "before_sha256": hashlib.sha256(original).hexdigest(), "after_sha256": hashlib.sha256(original + runner.EDIT_SUFFIX).hexdigest()}}
            if full_capacity:
                protocol.update(order=[], allocations=[], predicted_growth_bytes=8 * STATS.GIB,
                    capacity_probe={"kind": "full-arm-and-qualification", "stage": "expanded", "arm": "baseline",
                                    "source_root": str(source), "owned_output_cap_bytes": 8 * STATS.GIB})
            protocol["qualification_allocations"] = runner.qualification_allocations(protocol)
            key = root / "local-integrity.key"
            key.write_bytes(b"s" * 32)
            checker = Path(__file__).with_name("build_feedback_retention.py")
            tools = {name: {"path": str(Path(__file__).with_name("build_feedback_" + name + ".py")),
                            "sha256": runner.sha256_file(Path(__file__).with_name("build_feedback_" + name + ".py")),
                            "bytes": Path(__file__).with_name("build_feedback_" + name + ".py").stat().st_size}
                     for name in ("runner", "statistics", "benchmark", "inventory")}
            approval = {"kind": "build-feedback-retention-approval", "schema_version": 2, "approved": True,
                "source_receipt_sha256": source_receipt["sha256"], "baseline_receipt_sha256": baseline_receipt["sha256"],
                "toolchain_sha256": runner.canonical_sha256(protocol["toolchain"]), "configs_sha256": runner.canonical_sha256(protocol["configs"]),
                "run_root": str(run_root), "disposable_target_paths": [str(target)], "protected_roots": [str(source)], "allowed_source_roots": [str(source)],
                "scope": "validated-successful-and-cap-stopped-task-intermediates", "flow": ["verify", "recycle", "match", "purge-exact", "verify-release", "receipt"],
                "protected": ["source", "git-common-directory", "daily-cache", "existing-last-72-hours", "stores", "seeds", "quality-source-failures", "unrelated-recycle-items", "pagefile"],
                "allow_recent_new_outputs": True, "checker": {"path": str(checker), "sha256": runner.sha256_file(checker), "bytes": checker.stat().st_size},
                "verification_tools": tools, "local_integrity_key": {"path": str(key), "sha256": runner.sha256_file(key), "bytes": 32},
                "producer_owned_scratch_cleanup_paths": sorted(
                    str(Path(entry["release_suite_root"]) / "scratch")
                    for entry in protocol["qualification_allocations"])}
            protocol["retention_policy"] = {"mode": "sealed-verified-recycle", "approval": write_proof(root / "approval.json", approval)}
            sample = {"allocated_bytes": 4096, "disk_free_bytes": 104 * STATS.GIB, "budget": {"allowed": True, "reasons": []}}
            job = SimpleNamespace(account=Mock(return_value={"peak_commit_bytes": 4096}), close=Mock())

            def fake_phase(owned_job, approved, selected_config, source_root, targets_root, log_root, phase, environment, monitor, **kwargs):
                artifact = targets_root / "synthetic.exe"
                artifact.write_bytes(b"synthetic compiled artifact\n")
                output = (json.dumps({"reason": "compiler-artifact", "filenames": [str(artifact)], "fresh": False, "profile": {"opt_level": "0"}, "executable": None})
                          + "\n" + json.dumps({"reason": "build-finished", "success": True}) + "\n{test tail after Cargo}\n").encode()
                captures = []
                for fixed in runner.JOBS:
                    selected = targets_root / "primary" / fixed["name"]
                    capture = capture_fixture(log_root / phase / fixed["name"], runner.job_command(approved, fixed, selected), output, duration=10000)
                    capture["cwd"] = str(source_root)
                    capture.update(job=fixed["name"], peak_commit_bytes=4096, resource_samples=[], reasons=[],
                                   cargo=runner.parse_cargo_json_file(Path(capture["stdout_path"]), targets_root, source_root))
                    if full_capacity:
                        # Explicit synthetic OS execution/monitor fixture;
                        # the independent seal's capture/file checks are real.
                        phase_start = datetime(2026, 10, 19, tzinfo=timezone.utc) + timedelta(seconds=10 * ("primary", "feedback-warm", "edit").index(phase))
                        capture.update(started_at=phase_start.isoformat(), finished_at=(phase_start + timedelta(seconds=10)).isoformat(),
                            resource_samples=[{"allocation_kind": "conservative-disk-and-job-write-bound",
                                               "allocated_bytes": 4096, "disk_free_bytes": 104 * STATS.GIB,
                                               "job_write_bound_bytes": 0, "budget_allowed": True, "budget_reasons": []}])
                    runner._write_new(log_root / (phase + "-" + fixed["name"] + ".json"), capture)
                    captures.append(capture)
                return captures, 10000.0

            with contextlib.ExitStack() as stack:
                stack.enter_context(patch.object(runner, "_verify_source"))
                stack.enter_context(patch.object(runner, "_verify_cache_prewarm"))
                stack.enter_context(patch.object(runner, "build_process_snapshot", return_value=[]))
                stack.enter_context(patch.object(runner, "_budget_sample", return_value=sample))
                stack.enter_context(patch.object(runner, "preview_protocol", return_value={"allowed": True}))
                stack.enter_context(patch.object(runner, "_check_review"))
                stack.enter_context(patch.object(runner, "WindowsJob", return_value=job))
                stack.enter_context(patch.object(runner, "_memory_status", return_value={"physical_available": 8 * STATS.GIB, "commit_available": 8 * STATS.GIB}))
                stack.enter_context(patch.object(runner, "tool_environment", return_value={}))
                stack.enter_context(patch.object(runner, "_LiveBudget", return_value=SimpleNamespace(io_origin=0, floor_free=104 * STATS.GIB, _write_bound=Mock(return_value=0))))
                stack.enter_context(patch.object(runner, "_start_sccache", return_value=None))
                stack.enter_context(patch.object(runner, "_run_parallel_phase", side_effect=fake_phase))
                # Real NTFS scans are retained; inject only reported available
                # free space to keep unrelated current machine usage out of this fixture.
                stack.enter_context(patch.object(inventory._WindowsMetadata, "free_bytes", return_value=104 * STATS.GIB))
                stack.enter_context(patch.object(runner, "_retention_module", return_value=helper))
                raw_arm = runner._run_arm(protocol, "baseline", source, arm_root, None)
            self.assertEqual(raw_arm["status"], "ok", raw_arm["reasons"])
            self.assertEqual(raw_arm["schema_version"], 2)
            self.assertEqual(edit_path.read_bytes(), original)
            self.assertIsInstance(raw_arm.get("output_lease"), dict)
            pair_index, attempt = (None, None) if full_capacity else (0, 0)
            if full_capacity:
                self.assertEqual(raw_arm["phase"], "capacity")
                self.assertFalse(raw_arm["performance_evidence"])
                self.assertEqual(raw_arm["completed_valid_pairs"], 0)
                self.assertIsNone(raw_arm["pair_index"])
                self.assertIsNone(raw_arm["attempt"])
                with patch.object(cli, "_module", return_value=runner), patch.object(runner, "_validate_protocol"):
                    arm = cli.normalize_runner_arm(raw_arm, protocol)
                self.assertNotIn("records", arm)
                self.assertFalse((run_root / "campaign.json").exists())
            else:
                raw = {"status": "segment_complete", "complete": False, "inflight": None, "pairs": [{"pair_index": 0, "attempt": 0, "order": protocol["order"][0], "status": "ok", "arms": {"baseline": raw_arm}}]}
                write_proof(run_root / "campaign.json", raw)
                with patch.object(cli, "_module", return_value=runner), patch.object(runner, "_validate_protocol"), patch.object(runner, "_validate_resume"):
                    normalized = cli.normalize_runner_campaign(raw, protocol, {})
                arm = normalized["records"][0]["baseline"]
            self.assertTrue(STATS._arm_proof_valid(arm, "baseline", identity, protocol=protocol, pair_index=pair_index, attempt=attempt))
            self.assertFalse(STATS._arm_proof_valid(arm, "baseline", identity))
            # Exact approved helper reaches the remaining full qualification gate;
            # without release/diagnostic proofs a seal must still refuse.
            with patch.dict(helper._MODULES, {"runner": runner, "statistics": STATS}), patch.object(runner, "_validate_protocol"), patch.object(runner, "_verify_source"), \
                 self.assertRaisesRegex(ValueError, "complete artifact-dependent qualifications"):
                if full_capacity:
                    helper.create_full_capacity_seal(protocol, arm, {}, raw_arm["output_lease"], arm_root / "seal.json")
                else:
                    helper.create_seal(protocol, arm, "baseline", 0, 0, raw_arm["output_lease"], {}, arm_root / "seal.json")
            for mutated in (1, True, 2.0, 3):
                changed = copy.deepcopy(raw_arm)
                changed["schema_version"] = mutated
                arm["evidence"] = write_proof(arm_root / "arm.json", changed)
                self.assertFalse(STATS._arm_proof_valid(arm, "baseline", identity, protocol=protocol, pair_index=pair_index, attempt=attempt))
            for field in ("protocol_sha256", "source_receipt_sha256", "toolchain_sha256"):
                changed = {**raw_arm, field: "0" * 64}
                arm["evidence"] = write_proof(arm_root / "arm.json", changed)
                self.assertFalse(STATS._arm_proof_valid(arm, "baseline", identity, protocol=protocol, pair_index=pair_index, attempt=attempt))


if __name__ == "__main__":
    unittest.main()
