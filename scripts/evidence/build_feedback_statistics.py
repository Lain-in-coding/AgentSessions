"""Fixed-N, paired comparisons for local build feedback evidence (stdlib only).

Input is a normalized campaign, not a list of selected timings. All attempts,
including contaminated retries and failed arms, must remain in ``records``.
The runner/CLI owns process and filesystem evidence; this module owns the
statistical and qualification decisions. Ratios are candidate / baseline.
"""
from __future__ import annotations

import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import math
import random
import statistics
from collections import Counter
from collections.abc import Iterable
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
PAIR_COUNTS = {"pilot": 5, "formal": 100}
BOOTSTRAP_RESAMPLES = 5000
GIB = 1024 ** 3
IDENTITY_FIELDS = (
    "source_commit", "source_tree", "lock_sha256", "resources_sha256",
    "rustc_version", "cargo_version", "rustc_sha256", "cargo_sha256", "host",
    "msvc_compiler_version", "msvc_linker_version", "msvc_compiler_sha256", "msvc_linker_sha256",
    "jobs", "test_threads",
)
QUALIFICATIONS = ("edit", "clippy", "diagnostics", "optional_features", "release")
JOB_NAMES = ("check", "domain-ports", "cli", "sqlite", "application", "clippy")
PAIR_STATUSES = {
    "valid", "contaminated", "failed", "timeout", "memory_limited",
    "budget_limited", "blocked", "incomplete", "interrupted", "capture_failed",
}


def _number(value: Any, label: str, *, positive: bool = False) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise TypeError(f"{label}: expected a finite number")
    try:
        result = float(value)
    except OverflowError as error:
        raise ValueError(f"{label}: expected a finite number") from error
    if not math.isfinite(result) or result < 0 or (positive and result == 0):
        raise ValueError(f"{label}: expected a {'positive' if positive else 'non-negative'} finite number")
    return result


def _samples(values: Iterable[float], *, positive: bool = False) -> list[float]:
    result = [_number(value, "sample", positive=positive) for value in values]
    if not result:
        raise ValueError("cannot summarize empty samples")
    return result


def nearest_rank(values: Iterable[float], percentile: float) -> float:
    ordered = sorted(_samples(values))
    if isinstance(percentile, bool) or not isinstance(percentile, (int, float)):
        raise TypeError("percentile must be in (0, 100]")
    if not math.isfinite(percentile) or not 0 < percentile <= 100:
        raise ValueError("percentile must be in (0, 100]")
    return ordered[math.ceil(percentile * len(ordered) / 100) - 1]


def summarize(values: Iterable[float]) -> dict[str, float | int]:
    samples = _samples(values)
    return {
        "count": len(samples), "min": min(samples), "max": max(samples),
        "p50": nearest_rank(samples, 50), "p95": nearest_rank(samples, 95),
        "mean": statistics.fmean(samples),
        "sample_stddev": statistics.stdev(samples) if len(samples) > 1 else 0.0,
    }


def _statistic(ordered: list[float], name: str) -> float:
    if name == "p50":
        return ordered[math.ceil(len(ordered) / 2) - 1]
    if name == "p95":
        return ordered[math.ceil(95 * len(ordered) / 100) - 1]
    if name == "mean":
        return statistics.fmean(ordered)
    raise ValueError(f"unknown statistic: {name}")


def paired_bootstrap(
    baseline: Iterable[float], candidate: Iterable[float], *, seed: int,
    resamples: int = BOOTSTRAP_RESAMPLES,
    estimators: tuple[str, ...] = ("p50", "p95"),
) -> dict[str, Any]:
    """Percentile 95% CIs for ratios of statistics, resampling paired indices.

    ``resamples`` is configurable for mathematical probes only; campaign
    comparisons always use the frozen BOOTSTRAP_RESAMPLES constant.
    """
    before = _samples(baseline, positive=True)
    after = _samples(candidate)
    if len(before) != len(after):
        raise ValueError("baseline and candidate must have equal paired sample counts")
    if type(seed) is not int or type(resamples) is not int or resamples < 100:
        raise ValueError("seed must be an integer and resamples must be at least 100")
    if not estimators or len(set(estimators)) != len(estimators):
        raise ValueError("estimators must be non-empty and unique")
    before_sorted, after_sorted = sorted(before), sorted(after)
    ratios: dict[str, list[float]] = {name: [] for name in estimators}
    for name in estimators:
        _statistic(before_sorted, name)
    rng = random.Random(seed)
    for _ in range(resamples):
        indices = [rng.randrange(len(before)) for _ in before]
        boot_before = sorted(before[index] for index in indices)
        boot_after = sorted(after[index] for index in indices)
        for name in estimators:
            ratios[name].append(_statistic(boot_after, name) / _statistic(boot_before, name))
    result: dict[str, Any] = {
        "sample_count": len(before), "method": "paired_percentile_bootstrap",
        "confidence_level": 0.95, "seed": seed, "resamples": resamples,
    }
    for name in estimators:
        before_value = _statistic(before_sorted, name)
        after_value = _statistic(after_sorted, name)
        ratio = after_value / before_value
        result[name] = {
            "baseline": before_value, "candidate": after_value,
            "ratio": ratio, "ci95": [nearest_rank(ratios[name], 2.5), nearest_rank(ratios[name], 97.5)],
            "speedup_factor": before_value / after_value if after_value else None,
            "reduction_percent": 100 * (1 - ratio),
        }
    return result


def store_reservation_bytes(peak_allocated_bytes: int) -> int:
    """ceil(1.25 * measured pilot peak in GiB + 1) GiB, using exact integers."""
    if type(peak_allocated_bytes) is not int or peak_allocated_bytes < 0:
        raise ValueError("pilot peak must be a non-negative integer allocation")
    return ((5 * peak_allocated_bytes + 4 * GIB - 1) // (4 * GIB) + 1) * GIB


def _digest(value: Any, length: int) -> bool:
    return isinstance(value, str) and len(value) == length and all(char in "0123456789abcdef" for char in value)


def _valid_identity(identity: Any) -> bool:
    if not isinstance(identity, dict) or any(name not in identity for name in IDENTITY_FIELDS):
        return False
    if any(not _digest(identity[name], 40) for name in ("source_commit", "source_tree")):
        return False
    if any(not _digest(identity[name], 64) for name in (
        "lock_sha256", "resources_sha256", "rustc_sha256", "cargo_sha256",
        "msvc_compiler_sha256", "msvc_linker_sha256",
    )):
        return False
    if any(not isinstance(identity[name], str) or not identity[name].startswith(prefix)
           for name, prefix in (("rustc_version", "rustc 1.97.1 "), ("cargo_version", "cargo 1.97.1 "))):
        return False
    if any(not isinstance(identity[name], str) or not identity[name].strip() for name in ("msvc_compiler_version", "msvc_linker_version")):
        return False
    return (
        identity["host"] == "x86_64-pc-windows-msvc"
        and type(identity["jobs"]) is int and identity["jobs"] == 2
        and type(identity["test_threads"]) is int and identity["test_threads"] == 2
    )


def _identity_matches(actual: Any, expected: dict[str, Any]) -> bool:
    return _valid_identity(actual) and all(actual[name] == expected[name] for name in IDENTITY_FIELDS)


def _gate(entry: dict[str, Any], limit: float) -> dict[str, Any]:
    upper = entry["ci95"][1]
    return {"limit_ratio": limit, "upper_ci95": upper, "passed": upper <= limit}


def _proof_file(reference: Any) -> tuple[Path, bytes]:
    """Read an explicitly pinned local proof; never resolve a relative path."""
    if not isinstance(reference, dict) or not _digest(reference.get("sha256"), 64):
        raise ValueError("missing proof hash")
    value = reference.get("path")
    if not isinstance(value, str) or not value or not Path(value).is_absolute():
        raise ValueError("proof path must be absolute")
    path = Path(value)
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != reference["sha256"]:
        raise ValueError("proof hash changed")
    if "bytes" in reference and (type(reference["bytes"]) is not int or reference["bytes"] != len(data)):
        raise ValueError("proof size changed")
    return path, data


def _proof_json(reference: Any) -> dict[str, Any]:
    def unique_keys(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate proof key")
            result[key] = value
        return result

    def reject_constant(value):
        raise ValueError("non-finite proof constant: " + value)

    _, data = _proof_file(reference)
    result = json.loads(data, object_pairs_hook=unique_keys, parse_constant=reject_constant)
    if not isinstance(result, dict):
        raise TypeError("proof must be an object")
    return result


def _capture_valid(capture: Any) -> bool:
    if not isinstance(capture, dict) or not (
        capture.get("kind") == "build-feedback-process" and capture.get("schema_version") == 1
        and capture.get("status") == "ok" and type(capture.get("returncode")) is int
        and capture["returncode"] == 0 and capture.get("process_tree_capture_complete") is True
        and capture.get("external_processes") == []
        and isinstance(capture.get("argv"), list) and bool(capture["argv"])
        and all(isinstance(arg, str) and bool(arg) for arg in capture["argv"])
        and isinstance(capture.get("cwd"), str) and Path(capture["cwd"]).is_absolute()
        and type(capture.get("pid")) is int and capture["pid"] > 0
        and all(isinstance(capture.get(key), str) and capture[key] for key in ("started_at", "finished_at"))
        and type(capture.get("memory_limit_bytes")) is int and capture["memory_limit_bytes"] == 24 * GIB
        and type(capture.get("peak_commit_bytes")) is int and 0 <= capture["peak_commit_bytes"] <= 24 * GIB
    ):
        return False
    _number(capture.get("duration_ms"), "proof duration", positive=True)
    for stream in ("stdout", "stderr"):
        _proof_file({"path": capture.get(stream + "_path"), "sha256": capture.get(stream + "_sha256")})
    return True


def _bound_proof(proof: dict[str, Any], identity: dict[str, Any], protocol_hash: str) -> bool:
    return (type(proof.get("schema_version")) is int and proof["schema_version"] == 1
            and proof.get("protocol_sha256") == protocol_hash
            and _identity_matches(proof.get("identity"), identity))


def _arm_proof_valid(arm: dict[str, Any], name: str, identity: dict[str, Any]) -> bool:
    proof = _proof_json(arm.get("evidence"))
    if not (proof.get("kind") == "build-feedback-arm" and proof.get("arm") == name
            and proof.get("status") == "ok" and proof.get("source_commit") == identity["source_commit"]
            and proof.get("source_tree") == identity["source_tree"]
            and proof.get("source_root") == arm.get("source_root")
            and proof.get("source_verified") is True and proof.get("own_edit_restored") is True
            and proof.get("process_tree_drained") is True and proof.get("feedback_incremental") == "on"):
        return False
    config_hash = hashlib.sha256(json.dumps(proof.get("config"), sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
    if config_hash != arm.get("config_sha256"):
        return False
    latest_artifacts = {}
    for phase, field in (("primary", "jobs"), ("feedback-warm", "feedback_warm_jobs"), ("edit", "edit_jobs")):
        captures = proof.get(field)
        if not isinstance(captures, list) or [entry.get("job") for entry in captures if isinstance(entry, dict)] != list(JOB_NAMES):
            return False
        _number(proof.get("phase_duration_ms", {}).get(phase), "phase wallclock", positive=True)
        for capture in captures:
            if not _capture_valid(capture):
                return False
            cargo = capture.get("cargo")
            if not isinstance(cargo, dict) or cargo.get("build_success") is not True or cargo.get("errors") != []:
                return False
            _, output = _proof_file({"path": capture["stdout_path"], "sha256": capture["stdout_sha256"]})
            messages = [json.loads(line) for line in output.splitlines() if line.strip().startswith(b"{")]
            if not any(message.get("reason") == "build-finished" and message.get("success") is True for message in messages):
                return False
            emitted = {path for message in messages if message.get("reason") == "compiler-artifact"
                       for path in [*message.get("filenames", []), message.get("executable")] if isinstance(path, str)}
            artifacts = cargo.get("artifacts")
            if not isinstance(artifacts, list) or not artifacts:
                return False
            for artifact in artifacts:
                if not isinstance(artifact, dict) or not isinstance(artifact.get("files"), list) or not artifact["files"]:
                    return False
                for file in artifact["files"]:
                    if not isinstance(file, dict) or file.get("path") not in emitted:
                        return False
                    latest_artifacts[file["path"]] = file
    if not latest_artifacts:
        return False
    for file in latest_artifacts.values():
        _proof_file(file)
    target = Path(proof["target_root"])
    allocation = proof.get("owned_allocation_receipt")
    allocation_proof = _proof_json({"path": str(target.parent / "owned-allocation.json"),
                                   "sha256": proof.get("owned_allocation_receipt_sha256")})
    if not (allocation == allocation_proof and allocation_proof.get("complete") is True
            and allocation_proof.get("errors") == [] and allocation_proof.get("storage_scope") == "owned_targets_and_stores"
            and type(allocation_proof.get("allocated_bytes")) is int
            and allocation_proof["allocated_bytes"] == arm.get("allocated_bytes") == proof.get("owned_allocated_bytes")):
        return False
    for capture in proof["jobs"]:
        job = arm["jobs"].get(capture["job"])
        if not isinstance(job, dict) or job.get("duration_seconds") != capture["duration_ms"] / 1000:
            return False
    feedback = arm.get("feedback_phases")
    if not isinstance(feedback, dict) or set(feedback) != {"feedback-warm", "edit"}:
        return False
    for phase, field in (("feedback-warm", "feedback_warm_jobs"), ("edit", "edit_jobs")):
        entry = feedback[phase]
        if not isinstance(entry, dict) or entry.get("duration_seconds") != proof["phase_duration_ms"][phase] / 1000:
            return False
        projected = {capture["job"]: {"status": "success", "exit_code": 0, "duration_seconds": capture["duration_ms"] / 1000}
                     for capture in proof[field]}
        if entry.get("jobs") != projected:
            return False
    return (proof["primary_duration_ms"] == proof["phase_duration_ms"]["primary"]
            and proof["primary_duration_ms"] / 1000 == arm["duration_seconds"]
            and proof["edit_duration_ms"] == proof["phase_duration_ms"]["edit"]
            and proof["feedback_warm_duration_ms"] == proof["phase_duration_ms"]["feedback-warm"])

_RELEASE_VALIDATOR = None


def _official_release_valid(proof: dict[str, Any], identity: dict[str, Any]) -> dict[str, Any]:
    """Run only the checked-in pure validator, never a receipt-selected script."""
    global _RELEASE_VALIDATOR
    validator, data = _proof_file(proof.get("validator"))
    canonical = Path(__file__).with_name("core_beta_benchmark.py").resolve()
    if validator.resolve() != canonical or data != canonical.read_bytes():
        raise ValueError("release validator is not the unchanged official validator")
    if _RELEASE_VALIDATOR is None:
        spec = importlib.util.spec_from_file_location("build_feedback_official_release_validator", canonical)
        if spec is None or spec.loader is None:
            raise ValueError("official release validator unavailable")
        _RELEASE_VALIDATOR = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(_RELEASE_VALIDATOR)
    report = _proof_json(proof.get("official_report"))
    # The path + expected_profile API is shared by both coordinator and frozen
    # Beta validators. Never catch a validator TypeError as API negotiation.
    with contextlib.redirect_stdout(io.StringIO()):
        validated = _RELEASE_VALIDATOR.validate_report(Path(proof["official_report"]["path"]), expected_profile="full")
    if validated != report or _proof_json(proof["official_report"]) != report:
        raise ValueError("official release report changed during validation")
    baseline = report.get("baseline")
    if not isinstance(baseline, dict):
        raise TypeError("official release originating build proof missing")
    source, build = baseline["source_before"], baseline["build_receipt"]
    config = build["configuration"]
    if not (report.get("commit") == identity["source_commit"] and source["cargo_lock_sha256"] == identity["lock_sha256"]
            and source["harness_sha256"] == proof["validator"]["sha256"]
            and config["target"] == identity["host"] and config["rustc"] == identity["rustc_version"]
            and config["rustc_tool_sha256"] == identity["rustc_sha256"] and config["cargo_tool_sha256"] == identity["cargo_sha256"]
            and build["cargo_version"] == identity["cargo_version"].splitlines()[0]
            and any(file.get("sha256") == build["binary_sha256"] for file in proof["artifacts"])):
        raise ValueError("official release build identity mismatch")
    argv = proof["capture"]["argv"]
    if "run" not in argv or "--workspace" not in argv or argv[argv.index("--workspace") + 1] != proof["source_root"]:
        raise ValueError("official release command does not target this source")
    return build["release_profile"]



def _command_proof_valid(proof: dict[str, Any], name: str, arm: dict[str, Any], identity: dict[str, Any], protocol_hash: str) -> bool:
    if not (_bound_proof(proof, identity, protocol_hash) and proof.get("kind") == "build-feedback-qualification-command"
            and proof.get("qualification") == name and proof.get("source_root") == arm.get("source_root")
            and proof.get("config_sha256") == arm.get("config_sha256") and _capture_valid(proof.get("capture"))):
        return False
    capture = proof["capture"]
    artifacts = proof.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        return False
    for artifact in artifacts:
        _proof_file(artifact)
    report = _proof_json(proof.get("report"))
    if not (_bound_proof(report, identity, protocol_hash) and report.get("kind") == "build-feedback-qualification-report"
            and report.get("qualification") == name and report.get("status") == "passed"
            and report.get("argv") == capture["argv"] and type(report.get("exit_code")) is int and report["exit_code"] == 0
            and all(report.get(stream + "_sha256") == capture[stream + "_sha256"] for stream in ("stdout", "stderr"))):
        return False
    capture_hash = hashlib.sha256(json.dumps(capture, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
    if report.get("capture_sha256") != capture_hash or report.get("artifacts") != artifacts:
        return False
    for field in ("pair_index", "attempt", "arm", "source_root", "config_sha256"):
        if report.get(field) != proof.get(field):
            return False
    checks = report.get("checks")
    if not isinstance(checks, list) or not checks or not all(isinstance(check, dict) and check.get("status") == "passed" for check in checks):
        return False
    if name in ("clippy", "optional_features"):
        argv = capture["argv"]
        if not (len(argv) > 1 and argv[1] in ("clippy", "test") and "--all-targets" in argv
                and "--locked" in argv and "--offline" in argv and "-j2" in argv):
            return False
        if name == "clippy" and (argv[1] != "clippy" or any(value in argv for value in ("--all-features", "--no-default-features", "--features"))):
            return False
        if name == "optional_features" and "--all-features" not in argv:
            return False
    if name == "release":
        validator, _ = _proof_file(proof.get("validator"))
        if validator.name != "core_beta_benchmark.py" or str(validator) not in capture["argv"]:
            return False
        if report.get("validator") != "core_beta_benchmark" or report.get("build_environment_validated") is not True or report.get("release_profile_unchanged") is not True:
            return False
        _official_release_valid(proof, identity)
    if name == "diagnostics":
        _, stdout = _proof_file({"path": capture["stdout_path"], "sha256": capture["stdout_sha256"]})
        _, stderr = _proof_file({"path": capture["stderr_path"], "sha256": capture["stderr_sha256"]})
        output = (stdout + stderr).decode("utf-8", errors="replace").replace("\\", "/")
        for check in checks:
            source, _ = _proof_file(check.get("source"))
            line = check.get("expected_line")
            if type(line) is not int or line <= 0 or check.get("actual_line") != line or check.get("actual_file") != str(source):
                return False
            if f"{source.as_posix()}:{line}:" not in output:
                return False
    return True


def _qualification_receipt(reference: Any, name: str, identity: dict[str, Any], protocol_hash: str,
                           records: list[dict[str, Any]], required: int) -> tuple[dict[str, Any], dict[str, list[float]]]:
    """Validate every paired command link before considering any pass flag."""
    receipt = _proof_json(reference)
    if not (_bound_proof(receipt, identity, protocol_hash) and receipt.get("kind") == "build-feedback-qualification"
            and receipt.get("qualification") == name and receipt.get("status") == "passed" and receipt.get("complete") is True):
        raise ValueError("qualification identity mismatch")
    samples = receipt.get("samples")
    expected = {(record["pair_index"], record["attempt"], arm): record[arm]
                for record in records for arm in ("baseline", "candidate")}
    timed = name in ("edit", "release")
    if len(records) != required or not isinstance(samples, list) or (timed and len(samples) != 2 * required) or not samples:
        raise ValueError("fixed paired proof count missing")
    measured = {}
    for sample in samples:
        if not isinstance(sample, dict) or type(sample.get("pair_index")) is not int or type(sample.get("attempt")) is not int:
            raise ValueError("invalid qualification sample")
        key = (sample["pair_index"], sample["attempt"], sample.get("arm"))
        if not isinstance(key[2], str) or key not in expected or key in measured:
            raise ValueError("duplicate or stale qualification sample")
        arm = expected[key]
        if name == "edit":
            if sample.get("evidence") != arm.get("evidence") or not _arm_proof_valid(arm, key[2], identity):
                raise ValueError("edit proof does not belong to arm")
            proof = _proof_json(sample["evidence"])
            duration = proof["phase_duration_ms"]["edit"] / 1000
        else:
            proof = _proof_json(sample.get("evidence"))
            if any(proof.get(field) != sample.get(field) for field in ("pair_index", "attempt", "arm")):
                raise ValueError("command proof pair identity mismatch")
            if not _command_proof_valid(proof, name, arm, identity, protocol_hash):
                raise ValueError("command proof missing or unsuccessful")
            if name == "release":
                # Official report validation is necessary, but the enclosing
                # harness duration includes setup/build work. The current
                # interchange cannot bind the accepted paired CLI-workload
                # measure; do not silently substitute harness wallclock.
                raise ValueError("release_cli_workload_timing_unverified")
            duration = proof["capture"]["duration_ms"] / 1000
        if "duration_seconds" in sample and sample["duration_seconds"] != duration:
            raise ValueError("sample duration is not the captured wallclock")
        measured[key] = duration
    if {key[2] for key in measured} != {"baseline", "candidate"}:
        raise ValueError("qualification must cover both configurations")
    if not timed:
        return receipt, {}
    timings = {arm: [measured[(record["pair_index"], record["attempt"], arm)] for record in records]
               for arm in ("baseline", "candidate")}
    if "timings" in receipt and receipt["timings"] != timings:
        raise ValueError("inline timings disagree with captured proof")
    return receipt, timings



def compare_campaign(campaign: dict[str, Any]) -> dict[str, Any]:
    """Return a fail-closed decision and transparent summaries for all attempts.

    See BUILD-FEEDBACK.md for the normalized interchange schema. A pilot never
    contains ``adopted``. Formal adoption requires all timing/storage gates and
    separately bound qualification receipts, not just successful timings.
    """
    if not isinstance(campaign, dict):
        raise TypeError("campaign must be an object")
    phase = campaign.get("phase")
    required = PAIR_COUNTS.get(phase) if isinstance(phase, str) else None
    result: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION, "phase": phase,
        "comparison_kind": campaign.get("comparison_kind"),
        "protocol_sha256": campaign.get("protocol_sha256"),
        "required_pairs": required, "completed_valid_pairs": 0,
        "contaminated_attempts": 0, "failed_attempts": 0,
        "qualified": False, "reasons": [], "metrics": {}, "gates": {},
        "raw_statuses": [], "raw_records": copy.deepcopy(campaign.get("records", [])),
        "forecast": None,
    }
    if phase != "pilot":
        result["adopted"] = False
    reasons: list[str] = result["reasons"]

    def reject(reason: str) -> None:
        if reason not in reasons:
            reasons.append(reason)

    admission_reasons = campaign.get("admission_reasons", [])
    if not isinstance(admission_reasons, list) or not all(isinstance(reason, str) for reason in admission_reasons):
        reject("invalid_admission_reasons")
    else:
        for reason in admission_reasons:
            reject(reason)
    bootstrap_cache: dict[tuple[Any, ...], dict[str, Any]] = {}

    def bootstrap(before, after, **options):
        key = (tuple(before), tuple(after), tuple(options.get("estimators", ("p50", "p95"))))
        if key not in bootstrap_cache:
            bootstrap_cache[key] = paired_bootstrap(before, after, seed=seed, **options)
        return copy.deepcopy(bootstrap_cache[key])

    identity, protocol_hash = campaign.get("identity"), campaign.get("protocol_sha256")
    job_names, schedule, seed = campaign.get("job_names"), campaign.get("schedule"), campaign.get("seed")
    if type(campaign.get("schema_version")) is not int or campaign.get("schema_version") != SCHEMA_VERSION:
        reject("unsupported_schema")
    if required is None or type(campaign.get("pairs_required")) is not int or campaign.get("pairs_required") != required:
        reject("fixed_pair_count_mismatch")
    if campaign.get("comparison_kind") not in ("native", "tool"):
        reject("unknown_comparison_kind")
    if not _valid_identity(identity) or not _digest(protocol_hash, 64):
        reject("invalid_protocol_identity")
    if type(seed) is not int or seed < 0:
        reject("invalid_seed")
    if not isinstance(job_names, list) or job_names != list(JOB_NAMES):
        reject("fixed_jobs_mismatch")
    orders = (["baseline", "candidate"], ["candidate", "baseline"])
    if not isinstance(schedule, list) or required is None or len(schedule) != required or any(order not in orders for order in schedule):
        reject("invalid_schedule")
    elif abs(schedule.count(orders[0]) - schedule.count(orders[1])) > 1:
        reject("unbalanced_schedule")
    records = copy.deepcopy(campaign.get("records"))
    if not isinstance(records, list):
        reject("invalid_records")
        return result
    shape_reasons = {"unsupported_schema", "fixed_pair_count_mismatch", "unknown_comparison_kind", "invalid_protocol_identity", "invalid_seed", "fixed_jobs_mismatch", "invalid_schedule", "unbalanced_schedule"}
    if any(reason in shape_reasons for reason in reasons):
        return result
    assert isinstance(identity, dict) and isinstance(job_names, list) and isinstance(required, int)
    assert isinstance(schedule, list) and isinstance(protocol_hash, str)
    grouped: dict[int, list[dict[str, Any]]] = {}
    for record in records:
        if not isinstance(record, dict):
            reject("invalid_record")
            continue
        index, attempt, status = record.get("pair_index"), record.get("attempt"), record.get("status")
        result["raw_statuses"].append({
            "pair_index": index, "attempt": attempt, "status": status,
            "reason": record.get("reason"),
            "baseline": record.get("baseline", {}).get("status") if isinstance(record.get("baseline"), dict) else None,
            "candidate": record.get("candidate", {}).get("status") if isinstance(record.get("candidate"), dict) else None,
        })
        if type(index) is not int or not 0 <= index < required or type(attempt) is not int or attempt < 0:
            reject("invalid_pair_or_attempt")
            continue
        if not isinstance(status, str) or status not in PAIR_STATUSES:
            reject("unknown_pair_status")
            continue
        grouped.setdefault(index, []).append(record)
        if record.get("order") != schedule[index]:
            reject("pair_order_mismatch")
        if status == "contaminated":
            result["contaminated_attempts"] += 1
            if not isinstance(record.get("reason"), str) or not record["reason"]:
                reject("contamination_reason_missing")
        elif status != "valid":
            result["failed_attempts"] += 1
            reject("retained_failed_attempt")
    valid: list[dict[str, Any]] = []
    for index in range(required):
        attempts = sorted(grouped.get(index, []), key=lambda record: record["attempt"])
        if not attempts:
            reject("incomplete_pairs")
            continue
        if [record["attempt"] for record in attempts] != list(range(len(attempts))):
            reject("missing_or_duplicate_attempts")
        successes = [record for record in attempts if record["status"] == "valid"]
        if len(successes) != 1:
            reject("incomplete_pairs" if not successes else "duplicate_valid_pair")
            continue
        selected = successes[0]
        if selected is not attempts[-1] or any(record["status"] != "contaminated" for record in attempts[:-1]):
            reject("invalid_retry_history")
        arm_ok = True
        for name in ("baseline", "candidate"):
            arm = selected.get(name)
            if not isinstance(arm, dict) or arm.get("status") != "success":
                reject("invalid_arm_status")
                arm_ok = False
                continue
            if arm.get("protocol_sha256") != protocol_hash or not _identity_matches(arm.get("identity"), identity):
                reject("arm_identity_mismatch")
                arm_ok = False
            if any(arm.get(flag) is not True for flag in (
                "capture_complete", "source_verified", "artifacts_verified", "allocation_complete", "quiet",
            )):
                reject("incomplete_arm_evidence")
                arm_ok = False
            if arm.get("storage_scope") != "owned_targets_and_stores":
                reject("incomplete_storage_scope")
                arm_ok = False
            try:
                proof_ok = _arm_proof_valid(arm, name, identity)
            except (OSError, ValueError, TypeError, KeyError, AttributeError):
                proof_ok = False
            if not proof_ok:
                reject("incomplete_arm_proof")
                arm_ok = False
            jobs = arm.get("jobs")
            if not isinstance(jobs, dict) or set(jobs) != set(job_names):
                reject("arm_jobs_mismatch")
                arm_ok = False
                continue
            try:
                _number(arm.get("duration_seconds"), "arm duration", positive=True)
                _number(arm.get("elapsed_seconds", arm["duration_seconds"]), "elapsed duration", positive=True)
                if type(arm.get("allocated_bytes")) is not int or arm["allocated_bytes"] < 0:
                    raise ValueError("allocation must be a non-negative integer")
                for job in jobs.values():
                    if not isinstance(job, dict) or job.get("status") != "success" or type(job.get("exit_code")) is not int or job["exit_code"] != 0:
                        raise ValueError("job failed or incomplete")
                    _number(job.get("duration_seconds"), "job duration", positive=True)
            except (ValueError, TypeError):
                reject("invalid_job_or_measurement")
                arm_ok = False
        selected["_comparison_arm_evidence_valid"] = arm_ok
        measurable = True
        for name in ("baseline", "candidate"):
            arm = selected.get(name)
            if not isinstance(arm, dict) or not isinstance(arm.get("jobs"), dict) or set(arm["jobs"]) != set(job_names):
                measurable = False
                break
            try:
                _number(arm.get("duration_seconds"), "summary duration", positive=True)
                _number(arm.get("elapsed_seconds", arm["duration_seconds"]), "summary elapsed", positive=True)
                if type(arm.get("allocated_bytes")) is not int or arm["allocated_bytes"] < 0:
                    raise ValueError("invalid allocation")
                for job_name in job_names:
                    job = arm["jobs"][job_name]
                    if not isinstance(job, dict):
                        raise TypeError("invalid job")
                    _number(job.get("duration_seconds"), "summary job", positive=True)
            except (ValueError, TypeError):
                measurable = False
                reject("invalid_job_or_measurement")
                break
        if measurable:
            valid.append(selected)
    result["completed_valid_pairs"] = sum(record["_comparison_arm_evidence_valid"] for record in valid)
    result["timed_pairs"] = len(valid)
    result["attempt_status_counts"] = dict(Counter(record.get("status") for record in records if isinstance(record, dict) and isinstance(record.get("status"), str)))
    if result["completed_valid_pairs"] != required:
        reject("incomplete_pairs")
    if valid:
        metrics = result["metrics"]
        before = [record["baseline"]["duration_seconds"] for record in valid]
        after = [record["candidate"]["duration_seconds"] for record in valid]
        metrics["primary"] = bootstrap(before, after)
        metrics["primary"]["baseline_summary"] = summarize(before)
        metrics["primary"]["candidate_summary"] = summarize(after)
        for job_name in job_names:
            metrics[job_name] = bootstrap(
                [record["baseline"]["jobs"][job_name]["duration_seconds"] for record in valid],
                [record["candidate"]["jobs"][job_name]["duration_seconds"] for record in valid],
            )
        before_storage = [record["baseline"]["allocated_bytes"] for record in valid]
        after_storage = [record["candidate"]["allocated_bytes"] for record in valid]
        if any(value <= 0 for value in before_storage):
            reject("zero_storage_baseline")
        else:
            metrics["storage"] = bootstrap(before_storage, after_storage, estimators=("p50", "p95"))
        if "storage" in metrics:
            metrics["storage"]["evidence_complete"] = all(record["_comparison_arm_evidence_valid"] for record in valid)
        gates = result["gates"]
        limit = 0.85 if campaign["comparison_kind"] == "tool" else 1.05
        gates["primary_p50"] = _gate(metrics["primary"]["p50"], limit)
        gates["primary_p95"] = _gate(metrics["primary"]["p95"], 1.05)
        for job_name in job_names:
            for estimator in ("p50", "p95"):
                gates[f"{job_name}_{estimator}"] = _gate(metrics[job_name][estimator], 1.05)
        feedback_complete = all(record["_comparison_arm_evidence_valid"] for record in valid)
        if feedback_complete:
            for feedback_phase in ("feedback-warm", "edit"):
                metrics[feedback_phase + "_phase"] = bootstrap(
                    [record["baseline"]["feedback_phases"][feedback_phase]["duration_seconds"] for record in valid],
                    [record["candidate"]["feedback_phases"][feedback_phase]["duration_seconds"] for record in valid])
                for estimator in ("p50", "p95"):
                    gates[f"{feedback_phase}_phase_{estimator}"] = _gate(metrics[feedback_phase + "_phase"][estimator], 1.05)
                for job_name in job_names:
                    key = f"{feedback_phase}_{job_name}"
                    metrics[key] = bootstrap(
                        [record["baseline"]["feedback_phases"][feedback_phase]["jobs"][job_name]["duration_seconds"] for record in valid],
                        [record["candidate"]["feedback_phases"][feedback_phase]["jobs"][job_name]["duration_seconds"] for record in valid])
                    for estimator in ("p50", "p95"):
                        gates[f"{key}_{estimator}"] = _gate(metrics[key][estimator], 1.05)
        if campaign["comparison_kind"] == "native" and "storage" in metrics:
            gates["storage_savings"] = _gate(metrics["storage"]["p50"], 0.9)
            gates["storage_p95_noninferiority"] = _gate(metrics["storage"]["p95"], 1.05)
        if len(valid) == required and phase == "pilot":
            pair_seconds = [sum(record[name].get("elapsed_seconds", record[name]["duration_seconds"]) for name in ("baseline", "candidate")) for record in valid]
            result["forecast"] = {
                "basis": "five_complete_pilot_pairs" if result["completed_valid_pairs"] == required else "five_timed_pilot_pairs_with_incomplete_evidence",
                "evidence_complete": result["completed_valid_pairs"] == required,
                "formal_pairs": PAIR_COUNTS["formal"],
                "mean_pair_seconds": statistics.fmean(pair_seconds),
                "projected_compute_hours": statistics.fmean(pair_seconds) * PAIR_COUNTS["formal"] / 3600,
                "excludes": ["dependency_fetch", "cold_store_seed", "quiet_time_wait", "retries", "qualification_jobs"],
                "pilot_peak_allocated_bytes": max(before_storage + after_storage),
                "store_reservation_bytes": None,
                "store_reservation_reason": "separate_measured_store_peak_required",
            }
    store_peak = campaign.get("pilot_store_peak_allocated_bytes")
    if result["forecast"] is not None and type(store_peak) is int and store_peak >= 0:
        result["forecast"]["pilot_store_peak_allocated_bytes"] = store_peak
        result["forecast"]["store_reservation_bytes"] = store_reservation_bytes(store_peak)
        result["forecast"].pop("store_reservation_reason")
    receipts = campaign.get("qualification_receipts", {})
    qualification_status: dict[str, bool] = {}
    for name in QUALIFICATIONS:
        reference = receipts.get(name) if isinstance(receipts, dict) else None
        if name == "release":
            reject("qualification_release_cli_workload_timing_unverified")
        try:
            _, timings = _qualification_receipt(reference, name, identity, protocol_hash, valid, required)
            qualification_status[name] = True
            if name in ("edit", "release"):
                measured = bootstrap(timings["baseline"], timings["candidate"])
                result["metrics"][name] = measured
                for estimator in ("p50", "p95"):
                    result["gates"][f"{name}_{estimator}"] = _gate(measured[estimator], 1.05)
        except (OSError, ValueError, TypeError, KeyError, AttributeError):
            qualification_status[name] = False
            reject(f"qualification_{name}_missing_or_invalid")
    result["qualification_receipts"] = qualification_status
    for name, gate in result["gates"].items():
        if not gate["passed"]:
            reject(f"gate_{name}_failed_or_uncertain")
    result["qualified"] = not reasons and result["completed_valid_pairs"] == required and all(qualification_status.values())
    if phase == "pilot":
        result["qualified"] = False
        reject("pilot_only")
    else:
        result["adopted"] = result["qualified"]
    return result
