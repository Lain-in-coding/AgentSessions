"""Fixed-N, paired comparisons for local build feedback evidence (stdlib only).

Input is a normalized campaign, not a list of selected timings. All attempts,
including contaminated retries and failed arms, must remain in ``records``.
The runner/CLI owns process and filesystem evidence; this module owns the
statistical and qualification decisions. Ratios are candidate / baseline.
"""
from __future__ import annotations

import codecs
import contextlib
import copy
import hashlib
import importlib.abc
import importlib.util
import io
import json
import math
import os
import random
import stat
import statistics
import sys
from collections import Counter
from collections.abc import Iterable
from datetime import datetime
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


def paired_cluster_bootstrap(
    baseline: Iterable[Iterable[float]], candidate: Iterable[Iterable[float]], *,
    seed: int, resamples: int = BOOTSTRAP_RESAMPLES,
) -> dict[str, Any]:
    """Resample paired runs, preserving every invocation within each run."""
    before = [_samples(group, positive=True) for group in baseline]
    after = [_samples(group, positive=True) for group in candidate]
    if not before or len(before) != len(after) or any(len(left) != len(right) for left, right in zip(before, after)):
        raise ValueError("release requires nonempty equally paired invocation groups")
    if type(seed) is not int or type(resamples) is not int or resamples < 100:
        raise ValueError("seed must be an integer and resamples must be at least 100")

    def grouped_values(groups: list[list[float]]) -> list[tuple[float, list[tuple[int, int]]]]:
        counts: dict[float, dict[int, int]] = {}
        for index, group in enumerate(groups):
            for value, count in Counter(group).items():
                counts.setdefault(value, {})[index] = count
        return [(value, list(counts[value].items())) for value in sorted(counts)]

    def weighted_percentiles(values: list[tuple[float, list[tuple[int, int]]]], weights: list[int], count: int) -> tuple[float, float]:
        p50_rank, p95_rank = math.ceil(count / 2), math.ceil(95 * count / 100)
        accumulated = 0
        p50 = None
        for value, clusters in values:
            accumulated += sum(weights[index] * occurrences for index, occurrences in clusters)
            if p50 is None and accumulated >= p50_rank:
                p50 = value
            if accumulated >= p95_rank:
                if p50 is None:
                    raise ValueError("invalid release weighted median")
                return p50, value
        raise ValueError("release weighted sample coverage changed")

    before_values, after_values = grouped_values(before), grouped_values(after)
    before_flat = sorted(value for group in before for value in group)
    after_flat = sorted(value for group in after for value in group)
    distributions: dict[str, list[float]] = {"p50": [], "p95": []}
    rng = random.Random(seed)
    for _ in range(resamples):
        weights = [0] * len(before)
        for _ in before:
            weights[rng.randrange(len(before))] += 1
        count = sum(weight * len(group) for weight, group in zip(weights, before))
        before_percentiles = weighted_percentiles(before_values, weights, count)
        after_percentiles = weighted_percentiles(after_values, weights, count)
        for index, name in enumerate(("p50", "p95")):
            distributions[name].append(after_percentiles[index] / before_percentiles[index])
    result: dict[str, Any] = {
        "method": "paired_cluster_percentile_bootstrap", "resampling_unit": "paired_run",
        "sample_count": len(before), "invocations_per_pair": [len(group) for group in before],
        "invocation_count_per_arm": len(before_flat), "confidence_level": 0.95,
        "seed": seed, "resamples": resamples, "unit": "ms",
    }
    for name in ("p50", "p95"):
        left, right = _statistic(before_flat, name), _statistic(after_flat, name)
        ratio = right / left
        result[name] = {"baseline": left, "candidate": right, "ratio": ratio,
                        "ci95": [nearest_rank(distributions[name], 2.5), nearest_rank(distributions[name], 97.5)],
                        "speedup_factor": left / right, "reduction_percent": 100 * (1 - ratio)}
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


def _proof_file(reference: Any) -> tuple[Path, int]:
    """Pin a complete local file with bounded hashing memory.

    Metadata checks detect ordinary replacement/writes during the observation;
    they are local verification, not independent cryptographic attestation.
    JSON consumers still materialize their document and are not constant-memory.
    """
    if not isinstance(reference, dict) or not _digest(reference.get("sha256"), 64):
        raise ValueError("missing proof hash")
    value = reference.get("path")
    if not isinstance(value, str) or not value or not Path(value).is_absolute():
        raise ValueError("proof path must be absolute")
    path = Path(value)
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode) or getattr(before, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 1024):
        raise ValueError("proof must be an ordinary file")

    def identity(metadata):
        return (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mtime_ns, metadata.st_ctime_ns)

    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        if identity(os.fstat(stream.fileno())) != identity(before):
            raise ValueError("proof changed before hashing")
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            size += len(block)
            digest.update(block)
        after = os.fstat(stream.fileno())
    if identity(after) != identity(before) or identity(path.lstat()) != identity(after) or size != after.st_size:
        raise ValueError("proof changed during hashing")
    if digest.hexdigest() != reference["sha256"]:
        raise ValueError("proof hash changed")
    if "bytes" in reference and (type(reference["bytes"]) is not int or reference["bytes"] != size):
        raise ValueError("proof size changed")
    return path, size


def _proof_json(reference: Any) -> dict[str, Any]:
    """Load a pinned JSON document; parsed JSON still scales with document size."""
    def unique_keys(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate proof key")
            result[key] = value
        return result

    def reject_constant(value):
        raise ValueError("non-finite proof constant: " + value)

    path, _ = _proof_file(reference)
    with path.open("r", encoding="utf-8") as stream:
        result = json.load(stream, object_pairs_hook=unique_keys, parse_constant=reject_constant)
    _proof_file(reference)
    if not isinstance(result, dict):
        raise TypeError("proof must be an object")
    return result


def _proof_text_contains(references: list[dict[str, Any]], needles: list[str]) -> bool:
    """Scan the complete concatenated diagnostic streams across chunk boundaries."""
    if not needles:
        return True
    paths = [_proof_file(reference)[0] for reference in references]
    decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
    remaining = set(needles)
    overlap = max(map(len, needles)) - 1
    tail = ""

    def consume(value):
        nonlocal tail
        combined = tail + value.replace("\\", "/")
        remaining.difference_update(needle for needle in tuple(remaining) if needle in combined)
        tail = combined[-overlap:] if overlap else ""

    for path in paths:
        with path.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                consume(decoder.decode(block))
    consume(decoder.decode(b"", final=True))
    for reference in references:
        _proof_file(reference)
    return not remaining


def _capture_valid(capture: Any) -> bool:
    if not isinstance(capture, dict) or not (
        capture.get("kind") == "build-feedback-process" and type(capture.get("schema_version")) is int and capture["schema_version"] == 1
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


_RETENTION = None


def _retention_module():
    global _RETENTION
    if _RETENTION is None:
        spec = importlib.util.spec_from_file_location("build_feedback_retained_evidence", Path(__file__).with_name("build_feedback_retention.py"))
        if spec is None or spec.loader is None:
            raise ValueError("approved retention verifier unavailable")
        module = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = module
        spec.loader.exec_module(module)
        _RETENTION = module
    return _RETENTION


def _retained_arm_result(arm: dict[str, Any], name: str, protocol: dict[str, Any] | None,
                         pair_index: int | None, attempt: int | None) -> dict[str, Any] | None:
    reference = arm.get("retained_evidence")
    if reference is None:
        return None
    if (protocol is None or type(protocol.get("schema_version")) is not int or protocol["schema_version"] != 2
            or protocol.get("phase") not in ("pilot", "formal") or type(pair_index) is not int or type(attempt) is not int):
        raise ValueError("retained artifacts require a bound v2 paired allocation")
    result = _retention_module().verify_retained_arm(reference, protocol, name, pair_index, attempt)
    if not isinstance(result, dict) or not isinstance(result.get("arm_receipt"), dict) or not isinstance(result.get("seal"), dict):
        raise TypeError("invalid retained arm verification result")
    projected = {key: value for key, value in arm.items() if key != "retained_evidence"}
    if (result["arm_receipt"] != _proof_json(arm.get("evidence"))
            or result["seal"].get("arm_projection") != projected
            or result["seal"].get("arm_reference") != arm.get("evidence")):
        raise ValueError("sealed arm does not equal the original projection and proof")
    return result


def _artifact_proof_valid(reference: Any, retained: dict[str, Any] | None = None) -> None:
    """Recheck live bytes or the approved local observation of a disposed file."""
    if retained is None or Path(reference.get("path", "")).exists():
        _proof_file(reference)
        return
    seal = retained["seal"]
    path, target = Path(reference.get("path", "")), Path(seal["target"])
    if not path.is_absolute() or not path.is_relative_to(target) or not _digest(reference.get("sha256"), 64):
        raise ValueError("missing artifact is outside the sealed target")
    entries = [entry for entry in seal["tree"]["entries"] if entry.get("relative") == path.relative_to(target).as_posix()]
    if (len(entries) != 1 or entries[0].get("directory") is not False or entries[0].get("sha256") != reference["sha256"]
            or ("bytes" in reference and (type(reference["bytes"]) is not int or reference["bytes"] != entries[0].get("bytes")))):
        raise ValueError("missing artifact differs from the sealed local observation")


def _arm_binding_valid(arm: dict[str, Any], proof: dict[str, Any], name: str, protocol: dict[str, Any],
                       pair_index: int | None, attempt: int | None, *, recycled: bool = False) -> bool:
    if name not in ("baseline", "candidate"):
        return False
    if protocol.get("phase") == "capacity":
        probe = protocol.get("capacity_probe", {})
        if (type(protocol.get("schema_version")) is not int or protocol["schema_version"] != 2
                or probe.get("kind") != "full-arm-and-qualification"
                or probe.get("stage") != "expanded" or name != probe.get("arm")
                or pair_index is not None or attempt is not None
                or type(protocol.get("pairs")) is not int or protocol["pairs"] != 0
                or protocol.get("allocations") != [] or proof.get("phase") != "capacity"
                or not all(field in proof and proof[field] is None for field in ("pair_index", "attempt"))
                or proof.get("performance_evidence") is not False
                or type(proof.get("completed_valid_pairs")) is not int or proof["completed_valid_pairs"] != 0
                or recycled):
            return False
        expected_source = probe.get("source_root")
        expected = Path(protocol["run_root"]) / "capacity"
    else:
        if type(pair_index) is not int or type(attempt) is not int or pair_index < 0 or attempt < 0:
            return False
        allocations = [entry for entry in protocol["allocations"]
                       if type(entry.get("pair_index")) is int and type(entry.get("attempt")) is int
                       and entry["pair_index"] == pair_index and entry["attempt"] == attempt]
        if len(allocations) != 1:
            return False
        expected_source = allocations[0].get(name + "_source_root")
        expected = Path(protocol["run_root"]) / "pairs" / f"pair-{pair_index:04d}-attempt-{attempt:02d}" / name
    config = protocol["configs"][name]
    config_hash = hashlib.sha256(json.dumps(config, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
    return (arm.get("source_root") == proof.get("source_root") == expected_source
            and arm.get("config_sha256") == config_hash and proof.get("config") == config
            and Path(arm["evidence"]["path"]).resolve(strict=True) == expected / "arm.json"
            and Path(proof["target_root"]).resolve(strict=not recycled) == expected / "targets")


def _arm_proof_valid(arm: dict[str, Any], name: str, identity: dict[str, Any], *,
                     protocol: dict[str, Any] | None = None, pair_index: int | None = None,
                     attempt: int | None = None) -> bool:
    proof = _proof_json(arm.get("evidence"))
    version = protocol.get("schema_version", 1) if protocol is not None else 1
    if type(version) is not int or version not in (1, 2) or type(proof.get("schema_version")) is not int or proof["schema_version"] != version:
        return False
    if version == 2:
        assert protocol is not None
        runner = _runner_module()
        if (proof.get("protocol_sha256") != runner.canonical_sha256(protocol)
                or proof.get("source_receipt_sha256") != protocol["source"]["receipt_sha256"]
                or proof.get("toolchain_sha256") != runner.canonical_sha256(protocol["toolchain"])):
            return False
    retained = _retained_arm_result(arm, name, protocol, pair_index, attempt)
    if protocol is not None and not _arm_binding_valid(arm, proof, name, protocol, pair_index, attempt, recycled=retained is not None):
        return False
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
            if version == 2:
                assert protocol is not None
                config = protocol["configs"][name]
                fixed = next(job for job in _runner_module().JOBS if job["name"] == capture["job"])
                mode = "primary" if phase == "primary" or config["incremental"] == "on" else "feedback"
                selected = Path(proof["target_root"]) / mode / (fixed["name"] if config["layout"] == "isolated" else fixed["lane"])
                expected_argv = _runner_module().job_command(protocol, fixed, selected)
                if config["tool"]["kind"] == "mbx":
                    expected_argv = [config["tool"]["path"], *expected_argv[1:]]
                if capture["argv"] != expected_argv or capture["cwd"] != proof["source_root"]:
                    return False
            cargo = capture.get("cargo")
            if not isinstance(cargo, dict) or cargo.get("build_success") is not True or cargo.get("errors") != []:
                return False
            if retained is None:
                stdout_reference = {"path": capture["stdout_path"], "sha256": capture["stdout_sha256"]}
                output_path = Path(capture["stdout_path"])
                decoded = _runner_module().parse_cargo_json_file(output_path, Path(proof["target_root"]), Path(proof["source_root"]))
                _proof_file(stdout_reference)
                if decoded.get("build_success") is not True or decoded.get("errors") != []:
                    return False
                emitted = {file["path"] for artifact in decoded["artifacts"] for file in artifact["files"]}
            else:
                # All log/artifact-dependent checks ran before sealing. The
                # independent helper rechecks retained hashes and disposition;
                # deleted bytes cannot be reread here or declared freshly valid.
                emitted = {file["path"] for artifact in cargo.get("artifacts", []) for file in artifact.get("files", [])}
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
        _artifact_proof_valid(file, retained)
        if retained is None and any(key in file for key in ("device", "file_id", "modified_ns")):
            _runner_module().verify_artifact_receipt(file)
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

class _ApprovedSourceLoader(importlib.abc.SourceLoader):
    """Load only verified source bytes, without consuming or emitting bytecode."""

    def __init__(self, path: Path, content: bytes):
        self.path = path
        self.content = content

    def get_filename(self, fullname: str) -> str:
        return str(self.path)

    def get_data(self, path: str) -> bytes:
        if path != str(self.path):
            raise OSError("only approved validator source may be loaded")
        return self.content


_RELEASE_VALIDATOR = None
_RELEASE_VALIDATORS: dict[tuple[str, str], Any] = {}
_RUNNER = None
RELEASE_WORKLOADS = {
    "cli_startup_warm_latency_ms": ("startup_warm", "warm", 20),
    "cli_startup_cold_latency_ms": ("startup_cold", "cold", 20),
    "search_latency_ms": ("search", "post_shrink", 100),
    "show_latency_ms": ("show", "post_shrink", 100),
    "get_latency_ms": ("get", "post_shrink", 100),
}


def _runner_module():
    global _RUNNER
    if _RUNNER is None:
        spec = importlib.util.spec_from_file_location("build_feedback_release_runner", Path(__file__).with_name("build_feedback_runner.py"))
        if spec is None or spec.loader is None:
            raise ValueError("release source verifier unavailable")
        _RUNNER = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = _RUNNER
        spec.loader.exec_module(_RUNNER)
    return _RUNNER


def _protocol_campaign_binding(protocol: dict[str, Any], identity: Any, protocol_hash: Any,
                               campaign: Any, *, label: str) -> None:
    runner = _runner_module()
    runner._validate_protocol(protocol)
    if runner.canonical_sha256(protocol) != protocol_hash:
        raise ValueError(label + " protocol digest mismatch")
    source, chain = protocol["source"], protocol["toolchain"]
    expected = {
        "source_commit": source["commit"], "source_tree": source["tree"],
        "lock_sha256": source["lock_sha256"], "resources_sha256": source["resources_sha256"],
        "rustc_version": chain["rustc"]["version"], "cargo_version": chain["cargo"]["version"],
        "rustc_sha256": chain["rustc"]["sha256"], "cargo_sha256": chain["cargo"]["sha256"],
        "msvc_compiler_version": chain["msvc"]["compiler"]["version"],
        "msvc_linker_version": chain["msvc"]["linker"]["version"],
        "msvc_compiler_sha256": chain["msvc"]["compiler"]["sha256"],
        "msvc_linker_sha256": chain["msvc"]["linker"]["sha256"],
        "host": chain["target"], "jobs": protocol["jobs"], "test_threads": protocol["test_threads"],
    }
    if not _identity_matches(identity, expected):
        raise ValueError(label + " protocol toolchain/source identity mismatch")
    kind = "native" if all(config["tool"]["kind"] == "native" for config in protocol["configs"].values()) else "tool"
    if not isinstance(campaign, dict) or any(campaign.get(field) != value for field, value in (
        ("phase", protocol["phase"]), ("pairs_required", protocol["pairs"]),
        ("seed", protocol["seed"]), ("schedule", protocol["order"]),
        ("comparison_kind", kind), ("job_names", list(JOB_NAMES)),
    )) or type(campaign.get("pairs_required")) is not int or type(campaign.get("seed")) is not int:
        raise ValueError(label + " campaign phase/count/seed/order/kind differs from protocol")



_BENCHMARK = None


def _benchmark_module():
    global _BENCHMARK
    if _BENCHMARK is None:
        spec = importlib.util.spec_from_file_location("build_feedback_raw_campaign_projection", Path(__file__).with_name("build_feedback_benchmark.py"))
        if spec is None or spec.loader is None:
            raise ValueError("approved runner campaign projector unavailable")
        _BENCHMARK = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = _BENCHMARK
        spec.loader.exec_module(_BENCHMARK)
    return _BENCHMARK


def _verify_runner_campaign_projection(campaign: dict[str, Any], protocol: dict[str, Any]) -> None:
    """Rebuild every v2 attempt/status/sample from the exact pinned raw campaign."""
    reference = campaign.get("runner_campaign_evidence")
    if not isinstance(reference, dict) or set(reference) != {"path", "sha256"}:
        raise ValueError("runner campaign evidence required")
    path, _ = _proof_file(reference)
    expected = Path(protocol["run_root"]) / "campaign.json"
    if path != expected or path.resolve(strict=True) != expected:
        raise ValueError("runner campaign path differs from the approved run_root/campaign.json")
    raw = _proof_json(reference)
    runner = _runner_module()
    if (type(raw.get("schema_version")) is not int or raw["schema_version"] != 2
            or raw.get("kind") != "build-feedback-campaign"
            or raw.get("protocol_path") != campaign["protocol_evidence"]["path"]
            or raw.get("source_commit") != protocol["source"]["commit"]
            or raw.get("source_tree") != protocol["source"]["tree"]
            or raw.get("toolchain_sha256") != runner.canonical_sha256(protocol["toolchain"])):
        raise ValueError("runner campaign raw source/protocol identity mismatch")
    runner._validate_resume(raw, protocol, runner.canonical_sha256(protocol))
    qualifications = campaign.get("qualification_receipts")
    if not isinstance(qualifications, dict):
        raise TypeError("runner qualification references required")
    # Use the same producer projection and current retained/live checks. This
    # does not recurse through compare_campaign or trust normalized status flags.
    module = _benchmark_module()
    expected_projection = module.normalize_runner_campaign(raw, protocol, qualifications)
    expected_projection["protocol_evidence"] = campaign["protocol_evidence"]
    if runner.canonical_sha256(campaign) != runner.canonical_sha256(expected_projection):
        raise ValueError("normalized campaign differs from its pinned raw runner projection")
    if protocol["phase"] == "formal":
        registration = _proof_json(protocol["formal_registration"])
        compute = raw.get("registered_compute_seconds")
        if not isinstance(compute, (int, float)) or isinstance(compute, bool):
            raise ValueError("runner registered compute provenance missing")
        if (not math.isfinite(compute) or compute < 0
                or raw.get("formal_registration_sha256") != protocol["formal_registration"]["sha256"]):
            raise ValueError("runner registered compute provenance missing")
        maximum_compute, maximum_storage = registration.get("maximum_compute_seconds"), registration.get("maximum_storage_bytes")
        if not isinstance(maximum_compute, int) or isinstance(maximum_compute, bool) or not isinstance(maximum_storage, int) or isinstance(maximum_storage, bool):
            raise ValueError("runner registration resource limits missing")
        if maximum_compute <= 0 or maximum_storage <= 0:
            raise ValueError("runner registration resource limits missing")
        if compute >= maximum_compute:
            raise ValueError("runner registered compute limit reached")
        baseline = protocol["baseline"]["allocated_bytes"]
        for pair in raw["pairs"]:
            for arm in pair.get("arms", {}).values():
                peaks = [arm.get("peak_allocated_bytes"), arm.get("allocated_after_bytes"), arm.get("allocated_before_bytes")]
                for field in ("jobs", "feedback_warm_jobs", "edit_jobs"):
                    for capture in arm.get(field, []):
                        peaks.extend(sample.get("allocated_bytes") for sample in capture.get("resource_samples", []))
                if any(value is not None and (type(value) is not int or value < 0) for value in peaks):
                    raise ValueError("runner storage peak evidence invalid")
                if any(value is not None and max(0, value - baseline) >= maximum_storage for value in peaks):
                    raise ValueError("runner registered storage limit reached")
    if _proof_json(reference) != raw:
        raise ValueError("runner campaign changed during comparison")


def _campaign_protocol(reference: Any, identity: Any, protocol_hash: Any,
                       campaign: dict[str, Any]) -> dict[str, Any]:
    protocol = _proof_json(reference)
    _protocol_campaign_binding(protocol, identity, protocol_hash, campaign, label="v2")
    if (type(protocol.get("schema_version")) is not int or protocol["schema_version"] != 2
            or campaign.get("schema_version") != 2):
        raise ValueError("campaign/protocol version mismatch")
    if protocol["phase"] == "capacity":
        if protocol["pairs"] != 0 or campaign.get("records") != []:
            raise ValueError("capacity cannot contain paired samples")
    elif protocol["phase"] == "pilot":
        if protocol["pairs"] != 5 or protocol.get("formal_registration") is not None:
            raise ValueError("v2 Pilot must retain five pairs without formal registration")
    elif protocol["phase"] == "formal":
        registration = _proof_json(protocol.get("formal_registration"))
        if (registration.get("kind") != "build-feedback-formal-registration"
                or type(registration.get("schema_version")) is not int or registration["schema_version"] != 2
                or registration.get("approved") is not True or registration.get("formal_data_collected") is not False
                or type(registration.get("pairs")) is not int or registration["pairs"] != protocol["pairs"]
                or type(registration.get("seed")) is not int or registration["seed"] != protocol["seed"]
                or registration.get("order") != protocol["order"]
                or registration.get("run_root") != protocol["run_root"]
                or registration.get("stopping_rule") != "fixed-n-no-adaptive-additions"):
            raise ValueError("formal fixed N registration does not match protocol")
    else:
        raise ValueError("unsupported v2 campaign phase")
    _verify_runner_campaign_projection(campaign, protocol)
    return protocol


def _release_protocol(reference: Any, identity: dict[str, Any], protocol_hash: str,
                      records: list[dict[str, Any]], *, campaign: Any) -> dict[str, Any]:
    protocol = _proof_json(reference)
    runner = _runner_module()
    _protocol_campaign_binding(protocol, identity, protocol_hash, campaign, label="release")
    if protocol.get("schema_version", 1) == 2:
        _campaign_protocol(reference, identity, protocol_hash, campaign)
    runner_root = Path(protocol["run_root"]).resolve(strict=True)
    if (len(records) != protocol["pairs"]
            or any(type(record.get("pair_index")) is not int or type(record.get("attempt")) is not int
                   or record["attempt"] < 0 for record in records)
            or sorted(record["pair_index"] for record in records) != list(range(protocol["pairs"]))):
        raise ValueError("release fixed paired allocation coverage differs from protocol")
    allocations = {(entry["pair_index"], entry["attempt"]): entry for entry in protocol["allocations"]}
    for record in records:
        if record.get("order") != protocol["order"][record["pair_index"]]:
            raise ValueError("release pair order differs from protocol")
        allocation = allocations.get((record["pair_index"], record["attempt"]))
        if allocation is None:
            raise ValueError("release pair lacks approved source allocation")
        for name in ("baseline", "candidate"):
            arm = record[name]
            config_hash = runner.canonical_sha256(protocol["configs"][name])
            if arm.get("source_root") != allocation[name + "_source_root"] or arm.get("config_sha256") != config_hash:
                raise ValueError("release arm source/config differs from protocol")
            expected_root = runner_root / "pairs" / f"pair-{record['pair_index']:04d}-attempt-{record['attempt']:02d}" / name
            reference = arm.get("evidence")
            arm_proof = _proof_json(reference)
            retained = _retained_arm_result(arm, name, protocol, record["pair_index"], record["attempt"])
            if (Path(reference["path"]).resolve(strict=True) != expected_root / "arm.json"
                    or Path(arm_proof["target_root"]).resolve(strict=retained is None) != expected_root / "targets"
                    or arm_proof.get("source_root") != allocation[name + "_source_root"]
                    or arm_proof.get("arm") != name or runner.canonical_sha256(arm_proof.get("config")) != config_hash):
                raise ValueError("release arm receipt/target differs from approved run allocation")
    return protocol


def _official_release_valid(proof: dict[str, Any], identity: dict[str, Any], *,
                            protocol: dict[str, Any] | None = None, arm_evidence: Any = None,
                            retained: dict[str, Any] | None = None) -> dict[str, Any]:
    """Invoke the exact official validator authorized by the frozen protocol."""
    global _RELEASE_VALIDATOR
    validator, validator_size = _proof_file(proof.get("validator"))
    if protocol is None:
        canonical = Path(__file__).with_name("core_beta_benchmark.py").resolve()
        if validator.resolve() != canonical:
            raise ValueError("release validator is not the unchanged official validator")
        if _RELEASE_VALIDATOR is None:
            spec = importlib.util.spec_from_file_location("build_feedback_official_release_validator", canonical)
            if spec is None or spec.loader is None:
                raise ValueError("official release validator unavailable")
            _RELEASE_VALIDATOR = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(_RELEASE_VALIDATOR)
        module = _RELEASE_VALIDATOR
    else:
        runner = _runner_module()
        root = Path(proof["source_root"]).resolve(strict=True)
        runner._verify_source(root, protocol)
        canonical = root / "scripts/evidence/core_beta_benchmark.py"
        manifest = runner._manifest(_proof_json({"path": protocol["source"]["receipt_path"], "sha256": protocol["source"]["receipt_sha256"]}))
        approved = [entry for entry in manifest if entry["path"] == "scripts/evidence/core_beta_benchmark.py"]
        if (len(approved) != 1 or validator.resolve() != canonical.resolve()
                or proof["validator"]["sha256"] != approved[0]["sha256"] or validator_size != approved[0]["bytes"]):
            raise ValueError("release validator differs from approved frozen source")
        key = (str(canonical), approved[0]["sha256"])
        if key not in _RELEASE_VALIDATORS:
            # The loader lacks bytecode read/write metadata hooks: frozen
            # __pycache__ cannot override approved source or acquire new bytes.
            content = canonical.read_bytes()
            if len(content) != validator_size or hashlib.sha256(content).hexdigest() != approved[0]["sha256"]:
                raise ValueError("approved official release validator changed before import")
            spec = importlib.util.spec_from_file_location("build_feedback_frozen_release_" + key[1], canonical,
                                                        loader=_ApprovedSourceLoader(canonical, content))
            if spec is None or spec.loader is None:
                raise ValueError("approved official release validator unavailable")
            module = importlib.util.module_from_spec(spec)
            sys.modules[spec.name] = module
            try:
                spec.loader.exec_module(module)
            except BaseException:
                sys.modules.pop(spec.name, None)
                raise
            _RELEASE_VALIDATORS[key] = module
        module = _RELEASE_VALIDATORS[key]
    argv = proof["capture"]["argv"]
    # The validator path must be the executed script followed by its run
    # subcommand. Merely finding both anywhere in argv also accepts wrappers,
    # unrelated scripts and a validate-report invocation with trailing flags.
    if (not isinstance(argv, list) or len(argv) < 3 or not isinstance(argv[1], str)
            or Path(argv[1]).resolve() != canonical.resolve() or argv[2] != "run"):
        raise ValueError("direct official release command required")
    report = _proof_json(proof.get("official_report"))
    with contextlib.redirect_stdout(io.StringIO()):
        validated = module.validate_report(Path(proof["official_report"]["path"]), expected_profile="full")
    if validated != report or _proof_json(proof["official_report"]) != report:
        raise ValueError("official release report changed during validation")
    if _proof_file(proof["validator"])[1] != validator_size:
        raise ValueError("official release validator changed during validation")
    baseline = report.get("baseline")
    if not isinstance(baseline, dict):
        raise TypeError("official release originating build proof missing")
    capture_start, capture_end = _utc(proof["capture"]["started_at"]), _utc(proof["capture"]["finished_at"])
    if not capture_start <= _utc(baseline["started_at_utc"]) <= _utc(baseline["completed_at_utc"]) <= capture_end:
        raise ValueError("official release run interval exceeds captured command")
    source, build = baseline["source_before"], baseline["build_receipt"]
    config = build["configuration"]
    if not (report.get("commit") == identity["source_commit"] and source["cargo_lock_sha256"] == identity["lock_sha256"]
            and source["harness_sha256"] == proof["validator"]["sha256"]
            and baseline.get("source_after") == source and build.get("source") == source
            and config["target"] == identity["host"] and config["rustc"] == identity["rustc_version"]
            and config["rustc_tool_sha256"] == identity["rustc_sha256"] and config["cargo_tool_sha256"] == identity["cargo_sha256"]
            and build["cargo_version"] == identity["cargo_version"].splitlines()[0]
            and report.get("binary", {}).get("binary_hash") == build["binary_sha256"]):
        raise ValueError("official release build identity mismatch")
    if (argv.count("--workspace") != 1
            or argv.index("--workspace") + 1 >= len(argv) or argv[argv.index("--workspace") + 1] != proof["source_root"]):
        raise ValueError("official release command does not target this source")
    if proof["capture"].get("cwd") != proof["source_root"]:
        raise ValueError("official release capture cwd differs from arm source")
    if argv.count("--output-dir") != 1 or argv.index("--output-dir") + 1 >= len(argv):
        raise ValueError("official release output directory must be explicit")
    output_directory = Path(argv[argv.index("--output-dir") + 1]).resolve(strict=True)
    if Path(proof["official_report"]["path"]).resolve(strict=True).parent != output_directory:
        raise ValueError("official report does not belong to the captured output directory")
    for flag, value in (("--profile", "full"), ("--expected-commit", identity["source_commit"]),
                        ("--baseline-run", str(baseline["run_ordinal"])), ("--suite-id", baseline["suite_id"])):
        if argv.count(flag) != 1 or argv.index(flag) + 1 >= len(argv) or argv[argv.index(flag) + 1] != value:
            raise ValueError("official release command/report lineage mismatch:" + flag)
    binary = proof.get("release_binary")
    if not isinstance(binary, dict):
        raise TypeError("release binary file reference required")
    binary_path, binary_size = _proof_file(binary)
    if (binary.get("sha256") != build["binary_sha256"] or binary_size != build["binary_size_bytes"]
            or binary not in proof["artifacts"] or binary_path.suffix.lower() != ".exe"):
        raise ValueError("official release binary differs from built artifact")
    if protocol is not None:
        arm = _proof_json(arm_evidence)
        target_root = Path(arm["target_root"]).resolve(strict=retained is None)
        binary_root = output_directory.parent
        # The official harness copies the Cargo-emitted release executable to
        # suite_root/baseline-cli.exe and records its originating artifact.
        if protocol.get("schema_version", 1) == 2:
            if not all(field in proof for field in ("pair_index", "attempt", "arm")):
                raise ValueError("v2 official suite allocation ordinals missing")
            pair_index, attempt, name = proof["pair_index"], proof["attempt"], proof["arm"]
            full_capacity = protocol.get("phase") == "capacity"
            if full_capacity:
                if (pair_index is not None or attempt is not None
                        or protocol.get("capacity_probe", {}).get("kind") != "full-arm-and-qualification"
                        or protocol["capacity_probe"].get("arm") != name):
                    raise ValueError("v2 full capacity release must have null paired ordinals")
            elif type(pair_index) is not int or type(attempt) is not int or pair_index < 0 or attempt < 0:
                raise ValueError("v2 official suite paired ordinals invalid")
            allocations = protocol.get("qualification_allocations")
            if not isinstance(allocations, list):
                raise ValueError("v2 official suite independent allocation missing")
            matches = [entry for entry in allocations if isinstance(entry, dict)
                       and entry.get("pair_index") == pair_index and entry.get("attempt") == attempt
                       and entry.get("arm") == name and entry.get("source_root") == proof["source_root"]]
            if len(matches) != 1 or set(matches[0]) != {"pair_index", "attempt", "arm", "source_root", "release_suite_root"}:
                raise ValueError("v2 official suite allocation does not match this arm")
            allocation = matches[0]
            if full_capacity:
                if allocation["pair_index"] is not None or allocation["attempt"] is not None:
                    raise ValueError("v2 official suite capacity ordinals changed")
            elif type(allocation["pair_index"]) is not int or type(allocation["attempt"]) is not int:
                raise ValueError("v2 official suite paired allocation types changed")
            suite_name = f"asg-build-feedback-{protocol['phase']}-{pair_index if pair_index is not None else 'capacity'}-{attempt if attempt is not None else 0}-{name}"
            suite = root / "evidence-output" / "build-feedback" / suite_name
            if (name not in ("baseline", "candidate") or allocation["release_suite_root"] != str(suite)
                    or binary_root != suite or output_directory != suite / "report" or baseline["suite_id"] != suite.name):
                raise ValueError("v2 official suite differs from independently declared source output")
        elif not output_directory.is_relative_to(target_root.parent):
            raise ValueError("release binary is not the captured official suite copy")
        if binary_path.resolve(strict=True) != binary_root / "baseline-cli.exe":
            raise ValueError("release binary is not the captured official suite copy")
        runner._verify_source(root, protocol)
    profile = build["release_profile"]
    if profile.get("opt_level") != "3" or profile.get("debug_assertions") is not False or profile.get("test") is not False:
        raise ValueError("release profile changed")
    return report


def _utc(value: Any) -> datetime:
    if not isinstance(value, str):
        raise TypeError("release UTC timestamp must be a string")
    stamp = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if stamp.tzinfo is None:
        raise ValueError("release UTC timestamp must be timezone-aware")
    return stamp


def _release_workload_samples(report: dict[str, Any], proof: dict[str, Any]) -> dict[str, list[float]]:
    """Extract only fixed raw CLI invocations from a verified full report."""
    if report.get("profile") != "full" or report.get("evidence_status") != "locally_verified":
        raise ValueError("release_cli_workload_timing_unverified")
    baseline = report.get("baseline")
    if not isinstance(baseline, dict) or not isinstance(baseline.get("anchor"), dict):
        raise TypeError("release_cli_workload_timing_unverified")
    observations = baseline["anchor"].get("observations")
    if not isinstance(observations, list):
        raise TypeError("release_cli_workload_timing_unverified")
    capture_start, capture_end = _utc(proof["capture"]["started_at"]), _utc(proof["capture"]["finished_at"])
    if capture_start > capture_end:
        raise ValueError("release capture timestamp order changed")
    results = {}
    previous_end = capture_start
    for entry in observations:
        if not isinstance(entry, dict):
            raise TypeError("release raw observation must be an object")
        started, finished = _utc(entry.get("started_at_utc")), _utc(entry.get("completed_at_utc"))
        if not previous_end <= started <= finished <= capture_end:
            raise ValueError("release raw observations overlap or exceed capture")
        previous_end = finished
    for metric_name, (operation, state, count) in RELEASE_WORKLOADS.items():
        entries = [entry for entry in observations if isinstance(entry, dict) and entry.get("legacy_metric") == metric_name]
        if len(entries) != count:
            raise ValueError("release raw workload coverage missing:" + metric_name)
        metrics = report.get("metrics", {}).get(metric_name)
        if (not isinstance(metrics, dict) or metrics.get("required_sample_count") != count or metrics.get("unit") != "ms"
                or metrics.get("raw_samples") != [entry.get("legacy_duration_ms") for entry in entries]):
            raise ValueError("release legacy/raw invocation links changed:" + metric_name)
        values = []
        for index, entry in enumerate(entries, 1):
            if (entry.get("workload") != "anchor" or entry.get("operation") != operation or entry.get("role") != "measured"
                    or entry.get("sample_index") != index or entry.get("legacy_sample_index") != index
                    or entry.get("outcome") != "success" or type(entry.get("exit_code")) is not int or entry["exit_code"] != 0):
                raise ValueError("release raw invocation identity failed:" + metric_name)
            phase = "startup" if operation.startswith("startup_") else "post_shrink"
            if entry.get("phase") != phase:
                raise ValueError("release measurement state changed:" + metric_name)
            argv = entry.get("argv")
            if not isinstance(argv, list) or not all(isinstance(arg, str) for arg in argv):
                raise ValueError("release invocation argv missing")
            if operation.startswith("startup_"):
                expected_binary = "binary" if operation == "startup_warm" else f"scratch/anchor/startup-cold-{index - 1:03d}.exe"
                if argv != [expected_binary, "--version"]:
                    raise ValueError("release startup invocation differs")
            elif not (len(argv) >= 7 and argv[0] == "binary" and argv[1] == "--db" and argv[3:6] == ["--output", "json", operation]):
                raise ValueError("release query invocation differs")
            started, finished = _utc(entry.get("started_at_utc")), _utc(entry.get("completed_at_utc"))
            if not capture_start <= started <= finished <= capture_end:
                raise ValueError("release invocation outside captured command")
            value = _number(entry.get("process_wall_ms"), "release process wall", positive=True)
            legacy = _number(entry.get("legacy_duration_ms"), "release legacy duration", positive=True)
            if value > legacy:
                raise ValueError("release process time exceeds enclosing observation")
            values.append(value)
        expected_state = "cold_best_effort" if state == "cold" else "warm"
        if metrics.get("state") != expected_state:
            raise ValueError("release metric state changed")
        results[metric_name] = values
    return results


def _command_proof_valid(proof: dict[str, Any], name: str, arm: dict[str, Any], identity: dict[str, Any], protocol_hash: str, *,
                         retained: dict[str, Any] | None = None, evidence_reference: Any = None,
                         protocol: dict[str, Any] | None = None) -> bool:
    if retained is not None:
        qualified = retained["seal"].get("qualifications", {}).get(name)
        if not isinstance(qualified, dict) or qualified.get("reference") != evidence_reference or qualified.get("proof") != proof:
            raise ValueError("command does not equal prerecycle qualified evidence")
    if not (_bound_proof(proof, identity, protocol_hash) and proof.get("kind") == "build-feedback-qualification-command"
            and proof.get("qualification") == name and proof.get("source_root") == arm.get("source_root")
            and proof.get("config_sha256") == arm.get("config_sha256") and _capture_valid(proof.get("capture"))):
        return False
    capture = proof["capture"]
    if protocol is not None and protocol.get("schema_version") == 2:
        if (not all(field in proof for field in ("pair_index", "attempt", "arm"))
                or capture["cwd"] != arm.get("source_root") or proof.get("source_root") != arm.get("source_root")):
            return False
        raw_arm = _proof_json(arm.get("evidence"))
        if (raw_arm.get("kind") != "build-feedback-arm" or raw_arm.get("arm") != proof["arm"]
                or not _arm_binding_valid(arm, raw_arm, proof["arm"], protocol,
                                          proof["pair_index"], proof["attempt"], recycled=retained is not None)):
            return False
        if name in ("clippy", "optional_features"):
            target = Path(raw_arm["target_root"])
            argv = capture["argv"]
            # Exact workspace scope, process identity and output assignment
            # prevent narrowed jobs from masquerading as full qualification.
            base = [protocol["toolchain"]["cargo"]["path"], "clippy" if name == "clippy" else "test",
                    "--workspace", "--all-targets"]
            if name == "optional_features":
                base.append("--all-features")
            base.extend(["--locked", "--offline", "-j2", "--message-format=json", "--target-dir"])
            if argv[:len(base)] != base or len(argv) <= len(base):
                return False
            assigned = Path(argv[len(base)])
            expected_root = target / "qualification" / name
            if not assigned.is_absolute() or assigned != expected_root:
                return False
            tail = ["--", "-D", "warnings"] if name == "clippy" else ["--", "--test-threads=2"]
            if argv[len(base) + 1:] != tail:
                return False
            if retained is None:
                cargo = _runner_module().parse_cargo_json_file(Path(capture["stdout_path"]), expected_root, Path(raw_arm["source_root"]))
                _proof_file({"path": capture["stdout_path"], "sha256": capture["stdout_sha256"]})
                if _runner_module().canonical_sha256(cargo) != _runner_module().canonical_sha256(capture.get("cargo")):
                    return False
            else:
                # Exact signed prerecycle qualification has already validated
                # the decoder and artifact bytes; only retained logs survive.
                cargo = capture.get("cargo")
            if (not isinstance(cargo, dict) or cargo.get("build_success") is not True or cargo.get("errors") != []
                    or any(entry.get("level") == "error" for entry in cargo.get("diagnostics", []))):
                return False
            emitted = [file for artifact in cargo.get("artifacts", []) for file in artifact.get("files", [])]
            if not emitted or _runner_module().canonical_sha256(emitted) != _runner_module().canonical_sha256(proof.get("artifacts")):
                return False
        if name == "diagnostics":
            try:
                _, approval = _retention_module().validate_policy(protocol)
                approved_argv = _runner_module().approved_diagnostic_argv(
                    protocol, raw_arm, approval=approval, verify_live=retained is None)
            except (OSError, ValueError, KeyError):
                return False
            if approved_argv != capture["argv"]:
                return False
            executable = Path(capture["argv"][0])
            target = Path(raw_arm["target_root"])
            if not executable.is_absolute() or not executable.is_relative_to(target):
                return False
            matches = [file for field in ("jobs", "feedback_warm_jobs", "edit_jobs") for command in raw_arm.get(field, [])
                       for artifact in command.get("cargo", {}).get("artifacts", []) for file in artifact.get("files", [])
                       if file.get("path") == str(executable)]
            if not matches:
                return False
            _artifact_proof_valid(matches[-1], retained)
    artifacts = proof.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        return False
    for artifact in artifacts:
        _artifact_proof_valid(artifact, retained)
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
        if (report.get(field) != proof.get(field)
                or protocol is not None and protocol.get("schema_version") == 2
                and (field not in report or type(report[field]) is not type(proof[field]))):
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
    if name == "diagnostics":
        needles = []
        for check in checks:
            source, _ = _proof_file(check.get("source"))
            if protocol is not None and protocol.get("schema_version") == 2:
                source_root = Path(arm["source_root"])
                manifest = _runner_module()._manifest(_proof_json({"path": protocol["source"]["receipt_path"],
                                                                  "sha256": protocol["source"]["receipt_sha256"]}))
                if not source.is_relative_to(source_root):
                    return False
                approved = [entry for entry in manifest if entry["path"] == source.relative_to(source_root).as_posix()]
                if len(approved) != 1 or approved[0]["sha256"] != check["source"]["sha256"]:
                    return False
            line = check.get("expected_line")
            if type(line) is not int or line <= 0 or check.get("actual_line") != line or check.get("actual_file") != str(source):
                return False
            needles.append(f"{source.as_posix()}:{line}:")
        if not _proof_text_contains([
            {"path": capture[stream + "_path"], "sha256": capture[stream + "_sha256"]}
            for stream in ("stdout", "stderr")
        ], needles):
            return False
    return True


def _qualification_receipt(reference: Any, name: str, identity: dict[str, Any], protocol_hash: str,
                           records: list[dict[str, Any]], required: int, *, protocol_reference: Any = None,
                           campaign: Any = None) -> tuple[dict[str, Any], dict[str, Any]]:
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
    protocol = None
    if isinstance(campaign, dict) and campaign.get("schema_version") == 2:
        protocol = _campaign_protocol(protocol_reference, identity, protocol_hash, campaign)
    if name == "release":
        if protocol_reference is None:
            raise ValueError("release_cli_workload_timing_unverified")
        protocol = _release_protocol(protocol_reference, identity, protocol_hash, records, campaign=campaign)
    measured: dict[tuple[int, int, str], Any] = {}
    release_compatibility = None
    report_hashes: set[str] = set()
    release_intervals: dict[tuple[int, int, str], tuple[datetime, datetime]] = {}
    for sample in samples:
        if not isinstance(sample, dict) or type(sample.get("pair_index")) is not int or type(sample.get("attempt")) is not int:
            raise ValueError("invalid qualification sample")
        key = (sample["pair_index"], sample["attempt"], sample.get("arm"))
        if not isinstance(key[2], str) or key not in expected or key in measured:
            raise ValueError("duplicate or stale qualification sample")
        arm = expected[key]
        retained = _retained_arm_result(arm, key[2], protocol, key[0], key[1])
        if name == "edit":
            if sample.get("evidence") != arm.get("evidence") or not _arm_proof_valid(arm, key[2], identity, protocol=protocol, pair_index=key[0], attempt=key[1]):
                raise ValueError("edit proof does not belong to arm")
            proof = _proof_json(sample["evidence"])
            duration = proof["phase_duration_ms"]["edit"] / 1000
        else:
            proof = _proof_json(sample.get("evidence"))
            if any(proof.get(field) != sample.get(field) for field in ("pair_index", "attempt", "arm")):
                raise ValueError("command proof pair identity mismatch")
            if not _command_proof_valid(proof, name, arm, identity, protocol_hash, retained=retained, evidence_reference=sample.get("evidence"), protocol=protocol):
                raise ValueError("command proof missing or unsuccessful")
            if name == "release":
                report = _official_release_valid(proof, identity, protocol=protocol, arm_evidence=arm["evidence"], retained=retained)
                raw = _release_workload_samples(report, proof)
                compatible = {
                    "dataset": report["dataset"], "release_profile": report["baseline"]["build_receipt"]["release_profile"],
                    "states": {metric: report["metrics"][metric]["state"] for metric in RELEASE_WORKLOADS},
                }
                if release_compatibility is not None and compatible != release_compatibility:
                    raise ValueError("release workloads dataset/state/profile changed")
                release_compatibility = compatible
                digest = proof["official_report"]["sha256"]
                if digest in report_hashes:
                    raise ValueError("release official report reused across pairs or arms")
                report_hashes.add(digest)
                if any(field in sample for field in ("duration_seconds", "timings", "workload_samples")):
                    raise ValueError("inline release timings cannot replace raw report observations")
                measured[key] = raw
                release_intervals[key] = (_utc(proof["capture"]["started_at"]), _utc(proof["capture"]["finished_at"]))
                continue
            duration = proof["capture"]["duration_ms"] / 1000
        if "duration_seconds" in sample and sample["duration_seconds"] != duration:
            raise ValueError("sample duration is not the captured wallclock")
        measured[key] = duration
    if {key[2] for key in measured} != {"baseline", "candidate"}:
        raise ValueError("qualification must cover both configurations")
    if not timed:
        return receipt, {}
    if name == "release":
        if "timings" in receipt:
            raise ValueError("inline release timings cannot replace raw report observations")
        previous_end = None
        for record in records:
            for arm_name in record["order"]:
                started, finished = release_intervals[(record["pair_index"], record["attempt"], arm_name)]
                if started > finished or (previous_end is not None and started < previous_end):
                    raise ValueError("release pair capture order differs from frozen schedule")
                previous_end = finished
        return receipt, {metric: {
            arm: [measured[(record["pair_index"], record["attempt"], arm)][metric] for record in records]
            for arm in ("baseline", "candidate")
        } for metric in RELEASE_WORKLOADS}
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
    version = campaign.get("schema_version")
    required = PAIR_COUNTS.get(phase) if isinstance(phase, str) else None
    protocol = None
    protocol_error = None
    if type(version) is int and version == 2:
        try:
            protocol = _campaign_protocol(campaign.get("protocol_evidence"), campaign.get("identity"), campaign.get("protocol_sha256"), campaign)
            required = protocol["pairs"]
        except (OSError, ValueError, TypeError, KeyError, AttributeError) as error:
            required = None
            protocol_error = str(error)
    result: dict[str, Any] = {
        "schema_version": version if type(version) is int and version in (1, 2) else SCHEMA_VERSION, "phase": phase,
        "comparison_kind": campaign.get("comparison_kind"),
        "protocol_sha256": campaign.get("protocol_sha256"),
        "required_pairs": required, "completed_valid_pairs": 0,
        "contaminated_attempts": 0, "failed_attempts": 0,
        "qualified": False, "reasons": [], "metrics": {}, "gates": {},
        "raw_statuses": [], "raw_records": copy.deepcopy(campaign.get("records", [])),
        "forecast": None,
    }
    if phase == "formal" or version != 2 and phase != "pilot":
        result["adopted"] = False
    reasons: list[str] = result["reasons"]

    def reject(reason: str) -> None:
        if reason not in reasons:
            reasons.append(reason)

    if protocol_error is not None:
        reject("v2_protocol_missing_or_invalid")
        result["protocol_evidence_error"] = protocol_error
    if type(version) is int and version == 2 and phase == "capacity":
        reject("capacity_probe_is_not_performance_evidence")
        result["performance_evidence"] = False
        result["capacity_observation"] = copy.deepcopy(campaign.get("capacity_observation"))
        return result
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
    if type(version) is not int or version not in (1, 2):
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
    shape_reasons = {"unsupported_schema", "fixed_pair_count_mismatch", "unknown_comparison_kind", "invalid_protocol_identity", "invalid_seed", "fixed_jobs_mismatch", "invalid_schedule", "unbalanced_schedule", "v2_protocol_missing_or_invalid"}
    if any(reason in shape_reasons for reason in reasons):
        return result
    assert isinstance(identity, dict) and isinstance(job_names, list) and isinstance(required, int)
    assert isinstance(schedule, list) and isinstance(protocol_hash, str) and isinstance(seed, int)
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
                proof_ok = _arm_proof_valid(arm, name, identity, protocol=protocol, pair_index=selected["pair_index"], attempt=selected["attempt"])
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
    retained_count = sum("retained_evidence" in record[name] for record in valid for name in ("baseline", "candidate"))
    if retained_count:
        result["retained_artifact_arms"] = retained_count
        result["artifact_verification_basis"] = "approved local prerecycle observations with rechecked retained proofs"
        result["retention_trust_limit"] = "Deleted artifact bytes cannot be independently reread; the local verification record and retained raw evidence are rechecked."
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
                "formal_pairs": PAIR_COUNTS["formal"] if version == 1 else None,
                "formal_pair_selection": "legacy-fixed100" if version == 1 else "approve-and-freeze-after-pilot-before-formal",
                "mean_pair_seconds": statistics.fmean(pair_seconds),
                "projected_compute_hours": statistics.fmean(pair_seconds) * PAIR_COUNTS["formal"] / 3600 if version == 1 else None,
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
        try:
            options = {"protocol_reference": campaign.get("protocol_evidence"), "campaign": campaign} if name == "release" or version == 2 else {}
            _, timings = _qualification_receipt(reference, name, identity, protocol_hash, valid, required, **options)
            qualification_status[name] = True
            if name == "edit":
                measured = bootstrap(timings["baseline"], timings["candidate"])
                result["metrics"][name] = measured
                for estimator in ("p50", "p95"):
                    result["gates"][f"{name}_{estimator}"] = _gate(measured[estimator], 1.05)
            elif name == "release":
                for workload, samples in timings.items():
                    measured = paired_cluster_bootstrap(samples["baseline"], samples["candidate"], seed=seed)
                    key = "release_" + workload
                    result["metrics"][key] = measured
                    for estimator in ("p50", "p95"):
                        result["gates"][f"{key}_{estimator}"] = _gate(measured[estimator], 1.05)
        except (OSError, ValueError, TypeError, KeyError, AttributeError) as error:
            qualification_status[name] = False
            if name == "release":
                reject("qualification_release_cli_workload_timing_unverified")
                result["release_evidence_error"] = str(error)
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
