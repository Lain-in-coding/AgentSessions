#!/usr/bin/env python3
"""Coordinate sealed build-feedback capacity and paired arm lifecycles.

The runner deliberately accepts an explicit lifecycle callback for v2 paired
and expanded-capacity work. This module supplies that callback: it runs all
artifact-dependent qualifications while outputs are live, seals the complete
arm, recycles and purges only the exact approved target, and publishes an
immutable retained-evidence link. It also converts verified full-capacity
observations into measured reservation evidence.

No command deletes an existing cache. Native disposal is limited to a target
created after a signed lease and whose complete tree matches the seal.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import secrets
import subprocess
import sys
from collections.abc import Sequence
from datetime import datetime, timezone
from pathlib import Path
from types import ModuleType
from typing import Any

MODULE_DIR = Path(__file__).resolve().parent
RELEASE_OPTIONS = {
    "disk": "C: local fixed volume",
    "filesystem": "NTFS",
    "antivirus_state": "Windows Security state held unchanged during the arm",
    "sqlite_version": "3.53.2",
    "sqlite_version_source": "locked bundled libsqlite3-sys 0.38.2 source",
}


def _module(name: str, filename: str | None = None) -> ModuleType:
    filename = filename or ("build_feedback_" + name + ".py")
    path = MODULE_DIR / filename
    spec = importlib.util.spec_from_file_location("build_feedback_coordinator_" + name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError("build-feedback module unavailable: " + filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _read(path: str | Path) -> dict[str, Any]:
    return _module("runner")._read_json(Path(path))


def _reference(path: str | Path) -> dict[str, str]:
    value = Path(path).resolve(strict=True)
    if not value.is_file() or value.is_symlink():
        raise ValueError("proof must be an ordinary file")
    return {"path": str(value), "sha256": _module("runner").sha256_file(value)}


def _new(path: Path, value: dict[str, Any]) -> dict[str, str]:
    if path.exists():
        raise ValueError("immutable receipt already exists: " + str(path))
    path.parent.mkdir(parents=True, exist_ok=True)
    _module("runner")._write_new(path, value)
    return _reference(path)


def _canonical(value: Any) -> str:
    return _module("runner").canonical_sha256(value)


def _require(condition: bool, reason: str) -> None:
    if not condition:
        raise ValueError(reason)


def _verify_arm_receipt(arm_receipt: dict[str, Any], arm_record: dict[str, Any]) -> Path:
    reference = arm_receipt
    if not isinstance(reference, dict) or set(reference) != {"path", "sha256"}:
        raise ValueError("arm receipt reference required")
    path = Path(reference["path"])
    if not path.is_absolute() or not path.is_file() or path.is_symlink():
        raise ValueError("arm receipt file required")
    if _module("runner").sha256_file(path) != reference["sha256"] or _read(path) != arm_record:
        raise ValueError("arm receipt changed before finalization")
    return path


def _arm_identity(protocol: dict[str, Any], phase: str, arm: str, pair_index: int | None,
                  attempt: int | None, arm_record: dict[str, Any]) -> tuple[Path, Path]:
    target = Path(arm_record["target_root"])
    if phase == "capacity":
        probe = protocol.get("capacity_probe", {})
        _require(probe.get("kind") == "full-arm-and-qualification"
                 and probe.get("stage") == "expanded"
                 and arm == probe.get("arm")
                 and pair_index is None and attempt is None
                 and arm_record.get("phase") == "capacity"
                 and arm_record.get("performance_evidence") is False
                 and arm_record.get("completed_valid_pairs") == 0,
                 "full capacity arm identity mismatch")
        expected_source = probe.get("source_root")
        expected_target = Path(protocol["run_root"]) / "capacity" / "targets"
    else:
        _require(protocol.get("phase") in {"pilot", "formal"}
                 and type(pair_index) is int and type(attempt) is int,
                 "paired arm allocation required")
        allocations = [entry for entry in protocol.get("allocations", [])
                       if entry.get("pair_index") == pair_index and entry.get("attempt") == attempt]
        _require(len(allocations) == 1, "paired arm allocation missing")
        expected_source = allocations[0].get(arm + "_source_root")
        expected_target = (Path(protocol["run_root"]) / "pairs"
                           / f"pair-{pair_index:04d}-attempt-{attempt:02d}" / arm / "targets")
    _require(arm_record.get("source_root") == expected_source, "arm source allocation mismatch")
    _require(Path(arm_record["target_root"]) == expected_target, "arm target allocation mismatch")
    _require(arm_record.get("config") == protocol["configs"][arm], "arm config changed")
    _require(arm_record.get("protocol_sha256") == _canonical(protocol), "arm protocol changed")
    return target, target.parent


def _qualification_refs(arm_root: Path) -> list[dict[str, str]]:
    refs: list[dict[str, str]] = []
    directory = arm_root / "qualification-captures"
    for name in _module("runner").QUALIFICATION_NAMES:
        path = directory / name / "capture.json"
        if not path.exists():
            break
        refs.append(_reference(path))
    return refs


def _run_qualifications(protocol: dict[str, Any], arm_record: dict[str, Any], arm_root: Path,
                        *, review_receipt_path: str | Path | None,
                        release_options: dict[str, str]) -> dict[str, dict[str, str]]:
    runner = _module("runner")
    refs = _qualification_refs(arm_root)
    if len(refs) > len(runner.QUALIFICATION_NAMES):
        raise ValueError("unknown qualification capture")
    captures = [_read(reference["path"]) for reference in refs]
    _require([capture.get("qualification") for capture in captures]
             == list(runner.QUALIFICATION_NAMES[:len(captures)]),
             "qualification order changed")
    _require(all(capture.get("status") == "ok" for capture in captures),
             "failed qualification is retained and blocks disposal")
    while len(captures) < len(runner.QUALIFICATION_NAMES):
        qualification = runner.QUALIFICATION_NAMES[len(captures)]
        result = runner.capture_qualification(
            protocol,
            arm_record,
            qualification,
            review_receipt_path=review_receipt_path,
            prior_captures=captures,
            release_options=release_options if qualification == "release" else None,
        )
        if result.get("status") != "ok":
            raise ValueError("qualification failed and remains protected: " + qualification)
        captures.append(result)
        refs.append(_reference(arm_root / "qualification-captures" / qualification / "capture.json"))
    _require(len(refs) == len(runner.QUALIFICATION_NAMES), "complete qualifications required")
    return dict(zip(runner.QUALIFICATION_NAMES, refs, strict=True))


def finalize_arm(*, protocol: dict[str, Any], phase: str, arm: str,
                 pair_index: int | None, attempt: int | None,
                 arm_receipt: dict[str, Any], arm_record: dict[str, Any],
                 review_receipt_path: str | Path | None = None,
                 release_options: dict[str, str] | None = None) -> dict[str, str]:
    """Runner callback implementing verify -> seal -> exact recycle lifecycle."""
    _require(phase == protocol.get("phase") or (phase == "capacity" and protocol.get("phase") == "capacity"),
             "finalizer phase changed")
    runner, retention, benchmark = _module("runner"), _module("retention"), _module("benchmark")
    runner._validate_protocol(protocol)
    retention.validate_policy(protocol)
    receipt_path = _verify_arm_receipt(arm_receipt, arm_record)
    target, arm_root = _arm_identity(protocol, phase, arm, pair_index, attempt, arm_record)
    _require(receipt_path == arm_root / "arm.json", "arm receipt path changed")
    link_path = arm_root / "retained-evidence.json"
    if link_path.exists():
        link = _reference(link_path)
        retained = (retention.verify_full_capacity_link(link, protocol)
                    if phase == "capacity"
                    else retention.verify_retained_arm(link, protocol, arm, pair_index, attempt))
        _require(retained.get("arm_receipt") == arm_record, "retained link changed the arm")
        return link

    normalized = benchmark.normalize_runner_arm(arm_record, protocol)
    _require(all(normalized.get(flag) is True for flag in
                 ("capture_complete", "source_verified", "artifacts_verified", "allocation_complete", "quiet")),
             "arm closure incomplete before qualification")
    qualifications = _run_qualifications(
        protocol,
        arm_record,
        arm_root,
        review_receipt_path=review_receipt_path,
        release_options=release_options or RELEASE_OPTIONS,
    )
    # Qualification writes and final allocation do not alter the immutable arm
    # receipt. Re-project to ensure the retained authority still names exactly
    # that receipt after all qualification side effects.
    normalized = benchmark.normalize_runner_arm(arm_record, protocol)
    seal_path = arm_root / "full-capacity-seal.json" if phase == "capacity" else arm_root / "artifact-seal.json"
    if seal_path.exists():
        seal = _reference(seal_path)
        retention._verify_seal(seal, protocol)
    else:
        seal = (retention.create_full_capacity_seal(protocol, normalized, qualifications,
                                                    arm_record["output_lease"], seal_path)
                if phase == "capacity" else
                retention.create_seal(protocol, normalized, arm, pair_index, attempt,
                                      arm_record["output_lease"], qualifications, seal_path))
    _require(target.resolve(strict=False) == Path(arm_record["target_root"]), "sealed target changed")
    disposition = retention.dispose_output(seal, protocol, arm_root / "disposition")
    link = _new(link_path, {"seal": seal, "disposition": disposition})
    retained = (retention.verify_full_capacity_link(link, protocol)
                if phase == "capacity"
                else retention.verify_retained_arm(link, protocol, arm, pair_index, attempt))
    _require(retained.get("arm_receipt") == arm_record, "published link does not bind original arm")
    return link


def make_arm_finalizer(review_receipt_path: str | Path | None = None,
                       release_options: dict[str, str] | None = None):
    def finalizer(**arguments):
        return finalize_arm(**arguments, review_receipt_path=review_receipt_path,
                            release_options=release_options)
    return finalizer


def make_reservation_observation(protocol_path: str | Path, campaign_path: str | Path,
                                 output_path: str | Path) -> dict[str, str]:
    """Pin one verified full-capacity campaign into a producer input receipt."""
    protocol = _read(protocol_path)
    campaign = _read(campaign_path)
    runner, retention = _module("runner"), _module("retention")
    runner._validate_protocol(protocol)
    _require(protocol.get("phase") == "capacity" and runner._full_capacity(protocol),
             "full capacity protocol required")
    _require(campaign.get("complete") is True and campaign.get("status") == "complete"
             and campaign.get("completed_valid_pairs") == 0,
             "complete full capacity campaign required")
    observation = campaign.get("capacity_observation")
    if not isinstance(observation, dict):
        raise TypeError("successful complete full capacity observation required")
    _require(observation.get("kind") == "build-feedback-full-capacity-observation"
             and observation.get("status") == "ok"
             and observation.get("capture_complete") is True
             and observation.get("workload_complete") is True
             and observation.get("reservation_eligible") is True,
             "successful complete full capacity observation required")
    link = observation.get("retained_evidence")
    if not isinstance(link, dict):
        raise TypeError("retained full capacity link required")
    retained = retention.verify_full_capacity_link(link, protocol)
    seal = retained["seal"]
    _require(seal.get("status") == "ok" and seal.get("workload_complete") is True
             and seal.get("reservation_eligible") is True,
             "successful retained full capacity seal required")
    payload = {
        "schema_version": 1,
        "kind": "build-feedback-reservation-observation",
        "protocol": _reference(protocol_path),
        "campaign": _reference(campaign_path),
        "retained_evidence": link,
        "seal": link["seal"],
        "config_sha256": seal["config_sha256"],
        "observed_peak_bytes": seal["observed_peak_bytes"],
        "workload_scope": "full-arm-and-qualification",
        "performance_evidence": False,
        "created_at": datetime.now(timezone.utc).isoformat(),
    }
    return _new(Path(output_path), payload)


def _verified_observation(reference: dict[str, str], template: dict[str, Any]) -> tuple[int, str]:
    observation = _read(reference["path"])
    _require(observation.get("kind") == "build-feedback-reservation-observation",
             "reservation observation kind changed")
    peak = _module("runner")._reservation_observation_peak(
        reference, template, "full-arm-and-qualification")
    return peak, observation["config_sha256"]


def make_reservation_evidence(template_protocol_path: str | Path,
                              baseline_observation: dict[str, str],
                              candidate_observation: dict[str, str],
                              margin_bytes: int, predicted_growth_bytes: int,
                              output_path: str | Path) -> dict[str, str]:
    """Publish max verified peak plus an explicit margin for a Pilot protocol."""
    template = _read(template_protocol_path)
    _require(type(margin_bytes) is int and margin_bytes > 0, "positive reservation margin required")
    _require(type(predicted_growth_bytes) is int and predicted_growth_bytes > 0,
             "positive predicted growth required")
    observations = [baseline_observation, candidate_observation]
    verified = [_verified_observation(reference, template) for reference in observations]
    peaks = [entry[0] for entry in verified]
    configs = [entry[1] for entry in verified]
    expected = {_canonical(config) for config in template["configs"].values()}
    _require(len(set(configs)) == 2 and set(configs) == expected,
             "reservation must cover exactly both verified configurations")
    peak = max(peaks)
    _require(predicted_growth_bytes >= peak + margin_bytes,
             "predicted growth does not cover measured peak plus margin")
    payload = {
        "schema_version": 2,
        "kind": "build-feedback-reservation-evidence",
        "complete": True,
        "source_receipt_sha256": template["source"]["receipt_sha256"],
        "baseline_receipt_sha256": template["baseline"]["receipt_sha256"],
        "toolchain_sha256": _canonical(template["toolchain"]),
        "workload_scope": "full-arm-and-qualification",
        "observed_peak_bytes": peak,
        "margin_bytes": margin_bytes,
        "predicted_growth_bytes": predicted_growth_bytes,
        "configurations": {config: {"observed_peak_bytes": value}
                           for config, value in zip(configs, peaks, strict=True)},
        "observations": observations,
        "created_at": datetime.now(timezone.utc).isoformat(),
    }
    return _new(Path(output_path), payload)


def _tool_manifest() -> dict[str, dict[str, Any]]:
    result = {}
    for name in ("runner", "statistics", "benchmark", "inventory"):
        path = MODULE_DIR / ("build_feedback_" + name + ".py")
        result[name] = {"path": str(path), "sha256": _module("runner").sha256_file(path),
                        "bytes": path.stat().st_size}
    return result


def _diagnostic_commands(configs: dict[str, Any]) -> dict[str, dict[str, Any]]:
    selector = {"artifact_relative_path": "debug/agent-session-grep.exe",
                "args": ["--help"]}
    return {_canonical(config): dict(selector) for config in configs.values()}


def _retention_approval(*, protocol_seed: dict[str, Any], run_root: Path,
                        targets: list[Path], source_roots: list[Path],
                        control_root: Path, diagnostic_commands: dict[str, Any]) -> dict[str, Any]:
    key = control_root / "local-integrity.key"
    if not key.exists():
        key.write_bytes(secrets.token_bytes(32))
    checker = MODULE_DIR / "build_feedback_retention.py"
    protected = []
    for root in source_roots:
        if str(root) not in protected:
            protected.append(str(root))
    return {
        "schema_version": 2,
        "kind": "build-feedback-retention-approval",
        "approved": True,
        "approved_at": datetime.now(timezone.utc).isoformat(),
        "authorization": "user-approved bounded build-feedback experiment",
        "source_receipt_sha256": protocol_seed["source"]["receipt_sha256"],
        "baseline_receipt_sha256": protocol_seed["baseline"]["receipt_sha256"],
        "toolchain_sha256": _canonical(protocol_seed["toolchain"]),
        "configs_sha256": _canonical(protocol_seed["configs"]),
        "run_root": str(run_root),
        "disposable_target_paths": [str(path) for path in targets],
        "scope": "validated-successful-and-cap-stopped-task-intermediates",
        "flow": ["verify", "recycle", "match", "purge-exact", "verify-release", "receipt"],
        "checker": {"path": str(checker), "sha256": _module("runner").sha256_file(checker),
                    "bytes": checker.stat().st_size},
        "verification_tools": _tool_manifest(),
        "protected_roots": protected,
        "allowed_source_roots": protected,
        "allow_recent_new_outputs": True,
        "local_integrity_key": {"path": str(key),
                                "sha256": _module("runner").sha256_file(key), "bytes": 32},
        "protected": ["source", "git-common-directory", "daily-cache",
                      "existing-last-72-hours", "stores", "seeds",
                      "quality-source-failures", "unrelated-recycle-items", "pagefile"],
        "diagnostic_commands": diagnostic_commands,
    }


def prepare_full_capacity(seed_protocol_path: str | Path, prerequisite_observation: str | Path,
                          arm: str, capacity_source_root: str | Path, run_root: str | Path,
                          control_root: str | Path, owned_output_cap_bytes: int) -> dict[str, Any]:
    """Prepare one fresh v2 full-capacity arm and its exact retention authority."""
    runner = _module("runner")
    seed = _read(seed_protocol_path)
    run_root, control_root = Path(run_root).resolve(), Path(control_root).resolve()
    capacity_source_root = Path(capacity_source_root).resolve(strict=True)
    _require(arm in {"baseline", "candidate"}, "full capacity arm required")
    _require(not run_root.exists() and not control_root.exists(), "fresh full capacity roots required")
    prerequisite = _reference(prerequisite_observation)
    candidate = dict(seed["configs"]["candidate"])
    if arm == "baseline":
        candidate = {"debug": "line-tables-only", "incremental": "on",
                     "layout": "3lane", "tool": {"kind": "native"}}
    configs = {
        "baseline": {"debug": "full", "incremental": "on",
                     "layout": "isolated", "tool": {"kind": "native"}},
        "candidate": candidate,
    }
    seed_for_approval = {**seed, "configs": configs}
    control_root.mkdir(parents=True)
    target = run_root / "capacity" / "targets"
    source_roots = [capacity_source_root, Path(seed["source"]["root"]).resolve()]
    approval = _retention_approval(
        protocol_seed=seed_for_approval,
        run_root=run_root,
        targets=[target],
        source_roots=source_roots,
        control_root=control_root,
        diagnostic_commands=_diagnostic_commands(configs),
    )
    approval["purpose"] = "full-capacity-arm"
    approval_ref = _new(control_root / "retention-approval.json", approval)
    inventory_roots = list(seed["inventory_roots"])
    if str(control_root) not in inventory_roots:
        inventory_roots.append(str(control_root))
    store_roots = [str(control_root)]
    for root in seed.get("store_roots", []):
        if str(root) not in store_roots:
            store_roots.append(str(root))
    fields = {
        "protocol_version": 2,
        "phase": "capacity",
        "source_root": seed["source"]["root"],
        "source_commit": seed["source"]["commit"],
        "source_tree": seed["source"]["tree"],
        "source_receipt_path": seed["source"]["receipt_path"],
        "baseline_receipt_path": seed["baseline"]["receipt_path"],
        "run_root": str(run_root),
        "toolchain": seed["toolchain"],
        "candidate_config": candidate,
        "allocations": [],
        "inventory_roots": inventory_roots,
        "store_roots": store_roots,
        "predicted_growth_bytes": owned_output_cap_bytes,
        "capacity_probe": {"kind": "full-arm-and-qualification", "stage": "expanded",
                           "source_root": str(capacity_source_root), "arm": arm,
                           "owned_output_cap_bytes": owned_output_cap_bytes},
        "reservation_provenance": {"kind": "declared-full-arm-cap",
                                   "prerequisite": prerequisite,
                                   "rationale": "The prior leaf fixes a conservative lower bound; this controlled full arm may stop at its declared owned-output cap."},
        "msvc_environment_receipt_path": seed.get("msvc_environment", {}).get("receipt_path"),
        "budget_policy": {"policy_version": 2,
                          "minimum_free_bytes": 40 * runner.GIB,
                          "external_growth_allowance_bytes": 8 * runner.GIB},
        "retention_policy": {"mode": "sealed-verified-recycle",
                             "approval": approval_ref},
        "timeout_seconds": seed["timeout_seconds"],
        "poll_seconds": seed["poll_seconds"],
    }
    protocol = runner.make_protocol(fields)
    protocol_ref = _new(control_root / "protocol.json", protocol)
    return {"protocol": protocol_ref, "approval": approval_ref, "run_root": str(run_root),
            "control_root": str(control_root), "arm": arm}


def prepare_initial_leaf(seed_protocol_path: str | Path, capacity_source_root: str | Path,
                         run_root: str | Path, control_root: str | Path,
                         owned_output_cap_bytes: int = 8 * 1024 ** 3) -> dict[str, Any]:
    """Prepare a fresh current-tool initial leaf under the accepted v2 policy."""
    runner = _module("runner")
    seed = _read(seed_protocol_path)
    run_root, control_root = Path(run_root).resolve(), Path(control_root).resolve()
    capacity_source_root = Path(capacity_source_root).resolve(strict=True)
    _require(not run_root.exists() and not control_root.exists(),
             "fresh leaf run/control roots required")
    _require(owned_output_cap_bytes == runner.INITIAL_CAPACITY_OUTPUT_CAP_BYTES,
             "initial leaf must use the frozen 8 GiB cap")
    control_root.mkdir(parents=True)
    approval = _retention_approval(
        protocol_seed=seed,
        run_root=run_root,
        targets=[run_root / "capacity" / "targets"],
        source_roots=[capacity_source_root, Path(seed["source"]["root"]).resolve()],
        control_root=control_root,
        diagnostic_commands=_diagnostic_commands(seed["configs"]),
    )
    approval["purpose"] = "guarded-initial-leaf-capacity"
    approval_ref = _new(control_root / "retention-approval.json", approval)
    inventory_roots = list(seed["inventory_roots"])
    if str(control_root) not in inventory_roots:
        inventory_roots.append(str(control_root))
    store_roots = [str(control_root)]
    for root in seed.get("store_roots", []):
        if str(root) not in store_roots:
            store_roots.append(str(root))
    fields = {
        "protocol_version": 2,
        "phase": "capacity",
        "source_root": seed["source"]["root"],
        "source_commit": seed["source"]["commit"],
        "source_tree": seed["source"]["tree"],
        "source_receipt_path": seed["source"]["receipt_path"],
        "baseline_receipt_path": seed["baseline"]["receipt_path"],
        "run_root": str(run_root),
        "toolchain": seed["toolchain"],
        "candidate_config": seed["configs"]["candidate"],
        "allocations": [],
        "inventory_roots": inventory_roots,
        "store_roots": store_roots,
        "predicted_growth_bytes": owned_output_cap_bytes,
        "capacity_probe": {"source_root": str(capacity_source_root),
                           "package": "agent-session-grep-domain",
                           "command": "check", "stage": "initial",
                           "owned_output_cap_bytes": owned_output_cap_bytes},
        "reservation_provenance": {"kind": "initial-capacity-cap"},
        "msvc_environment_receipt_path": seed.get("msvc_environment", {}).get("receipt_path"),
        "budget_policy": {"policy_version": 2,
                          "minimum_free_bytes": 40 * runner.GIB,
                          "external_growth_allowance_bytes": 8 * runner.GIB},
        "retention_policy": {"mode": "sealed-verified-recycle",
                             "approval": approval_ref},
        "timeout_seconds": seed["timeout_seconds"],
        "poll_seconds": seed["poll_seconds"],
    }
    protocol = runner.make_protocol(fields)
    protocol_ref = _new(control_root / "protocol.json", protocol)
    return {"protocol": protocol_ref, "approval": approval_ref,
            "run_root": str(run_root), "control_root": str(control_root)}


def make_owner_review(protocol_path: str | Path, preview_path: str | Path,
                      output_path: str | Path, max_allocated_bytes: int,
                      min_disk_free_bytes: int) -> dict[str, str]:
    protocol, preview = _read(protocol_path), _read(preview_path)
    runner = _module("runner")
    expected_hash = runner.canonical_sha256(protocol)
    _require(preview.get("allowed") is True and preview.get("review_required") is True
             and preview.get("reasons") == []
             and preview.get("protocol_sha256") == expected_hash,
             "clean review-required preview needed")
    batch = preview["allocated_bytes"] + protocol["predicted_growth_bytes"]
    _require(type(max_allocated_bytes) is int and max_allocated_bytes >= batch,
             "review allocation ceiling does not cover batch")
    _require(type(min_disk_free_bytes) is int
             and min_disk_free_bytes >= protocol["budget_policy"]["minimum_free_bytes"]
             and preview["disk_free_bytes"] >= min_disk_free_bytes
             + protocol["predicted_growth_bytes"]
             + protocol["budget_policy"]["external_growth_allowance_bytes"],
             "review disk floor does not cover the batch")
    payload = {
        "schema_version": 1,
        "approved": True,
        "protocol_sha256": expected_hash,
        "baseline_receipt_sha256": protocol["baseline"]["receipt_sha256"],
        "max_allocated_bytes": max_allocated_bytes,
        "min_disk_free_bytes": min_disk_free_bytes,
        "preview": _reference(preview_path),
        "authorization": "User directed uninterrupted execution through the final verified result.",
        "approved_at": datetime.now(timezone.utc).isoformat(),
    }
    return _new(Path(output_path), payload)


def create_pilot_sources(source_root: str | Path, allocation_root: str | Path,
                         commit: str) -> dict[str, Any]:
    """Create thirty detached, isolated source worktrees without cleaning failures."""
    source, root = Path(source_root).resolve(), Path(allocation_root).resolve()
    if root.exists():
        raise ValueError("pilot source allocation root must be absent")
    allocations = []
    for pair_index in range(5):
        for attempt in range(3):
            entry: dict[str, Any] = {"pair_index": pair_index, "attempt": attempt}
            for arm in ("baseline", "candidate"):
                path = root / f"pair-{pair_index:04d}-attempt-{attempt:02d}-{arm}"
                completed = subprocess.run(
                    ["git", "-C", str(source), "worktree", "add", "--detach", str(path), commit],
                    capture_output=True, text=True, encoding="utf-8", errors="strict",
                    check=False)
                if completed.returncode != 0:
                    raise ValueError("git worktree allocation failed; partial paths retained: "
                                     + completed.stderr.strip())
                entry[arm + "_source_root"] = str(path.resolve(strict=True))
            allocations.append(entry)
    return {"allocation_root": str(root), "allocations": allocations}


def prepare_pilot(seed_protocol_path: str | Path, reservation_path: str | Path,
                  allocations: list[dict[str, Any]], run_root: str | Path,
                  control_root: str | Path, predicted_growth_bytes: int,
                  margin_bytes: int) -> dict[str, Any]:
    """Prepare the fixed five-pair Pilot bound to measured reservation evidence."""
    runner = _module("runner")
    seed = _read(seed_protocol_path)
    reservation = _reference(reservation_path)
    run_root, control_root = Path(run_root).resolve(), Path(control_root).resolve()
    _require(not run_root.exists() and not control_root.exists(), "fresh Pilot roots required")
    configs = {
        "baseline": {"debug": "full", "incremental": "on",
                     "layout": "isolated", "tool": {"kind": "native"}},
        "candidate": {"debug": "line-tables-only", "incremental": "on",
                      "layout": "3lane", "tool": {"kind": "native"}},
    }
    sources = []
    targets = []
    for entry in allocations:
        pair_index, attempt = entry["pair_index"], entry["attempt"]
        for arm in ("baseline", "candidate"):
            source = Path(entry[arm + "_source_root"]).resolve(strict=True)
            sources.append(source)
            targets.append(run_root / "pairs" / f"pair-{pair_index:04d}-attempt-{attempt:02d}"
                           / arm / "targets")
    sources.append(Path(seed["source"]["root"]).resolve())
    seed_for_approval = {**seed, "configs": configs}
    control_root.mkdir(parents=True)
    approval = _retention_approval(
        protocol_seed=seed_for_approval,
        run_root=run_root,
        targets=targets,
        source_roots=sources,
        control_root=control_root,
        diagnostic_commands=_diagnostic_commands(configs),
    )
    approval["purpose"] = "paired-pilot"
    approval_ref = _new(control_root / "retention-approval.json", approval)
    inventory_roots = list(seed["inventory_roots"])
    if str(control_root) not in inventory_roots:
        inventory_roots.append(str(control_root))
    store_roots = [str(control_root)]
    for root in seed.get("store_roots", []):
        if str(root) not in store_roots:
            store_roots.append(str(root))
    fields = {
        "protocol_version": 2,
        "phase": "pilot",
        "pairs": 5,
        "source_root": str(Path(seed["source"]["root"]).resolve()),
        "source_commit": seed["source"]["commit"],
        "source_tree": seed["source"]["tree"],
        "source_receipt_path": seed["source"]["receipt_path"],
        "baseline_receipt_path": seed["baseline"]["receipt_path"],
        "run_root": str(run_root),
        "toolchain": seed["toolchain"],
        "candidate_config": configs["candidate"],
        "allocations": allocations,
        "inventory_roots": inventory_roots,
        "store_roots": store_roots,
        "predicted_growth_bytes": predicted_growth_bytes,
        "reservation_provenance": {"kind": "measured-capacity", "receipt": reservation,
                                   "margin_bytes": margin_bytes},
        "msvc_environment_receipt_path": seed.get("msvc_environment", {}).get("receipt_path"),
        "budget_policy": {"policy_version": 2,
                          "minimum_free_bytes": 40 * runner.GIB,
                          "external_growth_allowance_bytes": 8 * runner.GIB},
        "retention_policy": {"mode": "sealed-verified-recycle",
                             "approval": approval_ref},
        "timeout_seconds": seed["timeout_seconds"],
        "poll_seconds": seed["poll_seconds"],
    }
    protocol = runner.make_protocol(fields)
    protocol_ref = _new(control_root / "protocol.json", protocol)
    return {"protocol": protocol_ref, "approval": approval_ref, "run_root": str(run_root),
            "control_root": str(control_root)}


def prepare_fixture_protocol(source_protocol_path: str | Path, control_root: str | Path) -> dict[str, Any]:
    """Create an independently scoped synthetic recycle-lifecycle authority."""
    source = _read(source_protocol_path)
    control = Path(control_root).resolve()
    _require(not control.exists(), "fixture control root must be absent")
    run_root = control.parent / (control.name + "-run")
    target = run_root / "capacity" / "targets"
    key = control / "local-integrity.key"
    control.mkdir(parents=True)
    key.write_bytes(secrets.token_bytes(32))
    checker = MODULE_DIR / "build_feedback_retention.py"
    allowed_sources = [source["source"]["root"]]
    approval = {
        "schema_version": 2,
        "kind": "build-feedback-retention-approval",
        "approved": True,
        "approved_at": datetime.now(timezone.utc).isoformat(),
        "authorization": "user-authorized synthetic recycle lifecycle fixture",
        "source_receipt_sha256": source["source"]["receipt_sha256"],
        "baseline_receipt_sha256": source["baseline"]["receipt_sha256"],
        "toolchain_sha256": _canonical(source["toolchain"]),
        "configs_sha256": _canonical(source["configs"]),
        "run_root": str(run_root),
        "disposable_target_paths": [str(target)],
        "scope": "validated-successful-and-cap-stopped-task-intermediates",
        "flow": ["verify", "recycle", "match", "purge-exact", "verify-release", "receipt"],
        "checker": {"path": str(checker), "sha256": _module("runner").sha256_file(checker),
                    "bytes": checker.stat().st_size},
        "verification_tools": _tool_manifest(),
        "protected_roots": allowed_sources,
        "allowed_source_roots": allowed_sources,
        "allow_recent_new_outputs": True,
        "local_integrity_key": {"path": str(key),
                                "sha256": _module("runner").sha256_file(key), "bytes": 32},
        "protected": ["source", "git-common-directory", "daily-cache",
                      "existing-last-72-hours", "stores", "seeds",
                      "quality-source-failures", "unrelated-recycle-items", "pagefile"],
        "purpose": "synthetic-recycle-lifecycle",
    }
    approval_path = control / "retention-approval.json"
    approval_ref = _new(approval_path, approval)
    protocol = dict(source)
    protocol["run_root"] = str(run_root)
    protocol["retention_policy"] = {"mode": "sealed-verified-recycle",
                                    "approval": approval_ref}
    protocol_path = control / "fixture-protocol.json"
    protocol_ref = _new(protocol_path, protocol)
    _module("retention").validate_policy(protocol)
    return {"protocol": protocol_ref, "control_root": str(control), "run_root": str(run_root),
            "target": str(target), "integrity_key": str(key)}


def run_fixture(protocol_path: str | Path, result_path: str | Path) -> dict[str, str]:
    """Exercise one literal owned Windows recycle/purge transaction."""
    protocol = _read(protocol_path)
    retention = _module("retention")
    retention.validate_policy(protocol)
    target = Path(protocol["retention_policy"]["approval"]["path"]).parent / "fixture-targets"
    # The prepared protocol normally already names this target. A copied
    # approval cannot redirect it because validate_policy binds exact paths.
    approval = _read(protocol["retention_policy"]["approval"]["path"])
    _require(len(approval["disposable_target_paths"]) == 1, "one fixture target required")
    target = Path(approval["disposable_target_paths"][0])
    _require(not target.exists(), "fixture target must be absent")
    lease_path = Path(protocol["run_root"]) / "leases" / "fixture-output.json"
    lease_path.parent.mkdir(parents=True, exist_ok=True)
    lease = retention.declare_output(protocol, target, lease_path)
    target.mkdir(parents=True)
    (target / "fixture.bin").write_bytes(retention.FIXTURE_BYTES)
    seal_path = target.parent / "fixture-seal.json"
    seal = retention.create_fixture_seal(protocol, target, lease, seal_path)
    disposition = retention.dispose_output(seal, protocol, target.parent / "disposition")
    result = _new(Path(result_path), {"seal": seal, "disposition": disposition,
                                     "performance_evidence": False})
    return result


def seal_leaf_capacity(protocol_path: str | Path, campaign_path: str | Path,
                       observation_path: str | Path) -> dict[str, str]:
    """Seal and dispose one successful initial leaf, then publish its observation."""
    protocol, campaign = _read(protocol_path), _read(campaign_path)
    runner, retention = _module("runner"), _module("retention")
    runner._validate_protocol(protocol)
    _require(protocol.get("phase") == "capacity" and not runner._full_capacity(protocol),
             "initial leaf protocol required")
    receipt_reference = campaign.get("capacity_receipt")
    _, receipt = _module("runner")._proof_reference(receipt_reference, "leaf_capacity_receipt")
    _require(receipt.get("status") == "ok" and receipt.get("capture_complete") is True,
             "successful leaf required")
    target = Path(receipt["target_root"])
    seal_path = target.parent / "capacity-seal.json"
    if seal_path.exists():
        seal = _reference(seal_path)
        retention._verify_seal(seal, protocol)
    else:
        seal = retention.create_capacity_seal(protocol, receipt_reference,
                                              receipt["output_lease"], seal_path)
    disposition_path = target.parent / "disposition"
    completed = disposition_path / "completed.json"
    if completed.exists():
        disposition = _reference(completed)
    else:
        disposition = retention.dispose_output(seal, protocol, disposition_path)
    observation = {
        "schema_version": 1,
        "kind": "build-feedback-reservation-observation",
        "protocol": _reference(protocol_path),
        "campaign": _reference(campaign_path),
        "seal": seal,
        "disposition": disposition,
        "config_sha256": _canonical(protocol["configs"]["candidate"]),
        "observed_peak_bytes": receipt["peak_owned_output_bytes"],
        "workload_scope": "expanded-capacity",
        "performance_evidence": False,
        "created_at": datetime.now(timezone.utc).isoformat(),
    }
    return _new(Path(observation_path), observation)


def run_campaign(protocol_path: str | Path, review_receipt_path: str | Path | None,
                 pair_limit: int) -> dict[str, Any]:
    protocol = _read(protocol_path)
    runner = _module("runner")
    runner._validate_protocol(protocol)
    return runner.run_protocol(
        protocol_path,
        review_receipt_path=review_receipt_path,
        pair_limit=pair_limit,
        arm_finalizer=make_arm_finalizer(review_receipt_path),
    )


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    commands = result.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.add_argument("--protocol", required=True)
    run.add_argument("--review")
    run.add_argument("--pair-limit", type=int, default=1)
    seal_leaf = commands.add_parser("seal-leaf")
    seal_leaf.add_argument("--protocol", required=True)
    seal_leaf.add_argument("--campaign", required=True)
    seal_leaf.add_argument("--output", required=True)
    fixture_prepare = commands.add_parser("prepare-fixture")
    fixture_prepare.add_argument("--source-protocol", required=True)
    fixture_prepare.add_argument("--control-root", required=True)
    fixture_run = commands.add_parser("run-fixture")
    fixture_run.add_argument("--protocol", required=True)
    fixture_run.add_argument("--result", required=True)
    full = commands.add_parser("prepare-full")
    full.add_argument("--seed-protocol", required=True)
    full.add_argument("--leaf-observation", required=True)
    full.add_argument("--arm", choices=("baseline", "candidate"), required=True)
    full.add_argument("--capacity-source-root", required=True)
    full.add_argument("--run-root", required=True)
    full.add_argument("--control-root", required=True)
    full.add_argument("--owned-output-cap-bytes", type=int, required=True)
    leaf = commands.add_parser("prepare-leaf")
    leaf.add_argument("--seed-protocol", required=True)
    leaf.add_argument("--capacity-source-root", required=True)
    leaf.add_argument("--run-root", required=True)
    leaf.add_argument("--control-root", required=True)
    leaf.add_argument("--owned-output-cap-bytes", type=int,
                      default=8 * 1024 ** 3)
    review = commands.add_parser("make-owner-review")
    review.add_argument("--protocol", required=True)
    review.add_argument("--preview", required=True)
    review.add_argument("--output", required=True)
    review.add_argument("--max-allocated-bytes", type=int, required=True)
    review.add_argument("--min-disk-free-bytes", type=int, required=True)
    observation = commands.add_parser("make-observation")
    observation.add_argument("--protocol", required=True)
    observation.add_argument("--campaign", required=True)
    observation.add_argument("--output", required=True)
    reservation = commands.add_parser("make-reservation")
    reservation.add_argument("--template-protocol", required=True)
    reservation.add_argument("--baseline-observation", required=True)
    reservation.add_argument("--candidate-observation", required=True)
    reservation.add_argument("--margin-bytes", type=int, required=True)
    reservation.add_argument("--predicted-growth-bytes", type=int, required=True)
    reservation.add_argument("--output", required=True)
    sources = commands.add_parser("create-pilot-sources")
    sources.add_argument("--source-root", required=True)
    sources.add_argument("--allocation-root", required=True)
    sources.add_argument("--commit", required=True)
    pilot = commands.add_parser("prepare-pilot")
    pilot.add_argument("--seed-protocol", required=True)
    pilot.add_argument("--reservation", required=True)
    pilot.add_argument("--allocations", required=True)
    pilot.add_argument("--run-root", required=True)
    pilot.add_argument("--control-root", required=True)
    pilot.add_argument("--predicted-growth-bytes", type=int, required=True)
    pilot.add_argument("--margin-bytes", type=int, required=True)
    return result


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    if args.command == "run":
        value = run_campaign(args.protocol, args.review, args.pair_limit)
    elif args.command == "seal-leaf":
        value = seal_leaf_capacity(args.protocol, args.campaign, args.output)
    elif args.command == "prepare-fixture":
        value = prepare_fixture_protocol(args.source_protocol, args.control_root)
    elif args.command == "run-fixture":
        value = run_fixture(args.protocol, args.result)
    elif args.command == "prepare-full":
        value = prepare_full_capacity(
            args.seed_protocol, args.leaf_observation, args.arm, args.capacity_source_root,
            args.run_root, args.control_root, args.owned_output_cap_bytes)
    elif args.command == "prepare-leaf":
        value = prepare_initial_leaf(
            args.seed_protocol, args.capacity_source_root, args.run_root,
            args.control_root, args.owned_output_cap_bytes)
    elif args.command == "make-owner-review":
        value = make_owner_review(
            args.protocol, args.preview, args.output,
            args.max_allocated_bytes, args.min_disk_free_bytes)
    elif args.command == "make-observation":
        value = make_reservation_observation(args.protocol, args.campaign, args.output)
    elif args.command == "make-reservation":
        value = make_reservation_evidence(
            args.template_protocol,
            _reference(args.baseline_observation),
            _reference(args.candidate_observation),
            args.margin_bytes,
            args.predicted_growth_bytes,
            args.output,
        )
    elif args.command == "create-pilot-sources":
        value = create_pilot_sources(args.source_root, args.allocation_root, args.commit)
    elif args.command == "prepare-pilot":
        allocation_data = _read(args.allocations)
        value = prepare_pilot(
            args.seed_protocol, args.reservation, allocation_data["allocations"],
            args.run_root, args.control_root, args.predicted_growth_bytes,
            args.margin_bytes)
    else:
        raise AssertionError(args.command)
    print(json.dumps(value, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
