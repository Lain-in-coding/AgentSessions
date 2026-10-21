#!/usr/bin/env python3
"""Prevalidated task artifacts and exact Windows recycle lifecycle.

Seals are local pre-disposal observations, not independent attestation of
bytes which no longer exist. No generic cleanup or whole-bin operation exists.
"""
from __future__ import annotations

import ctypes
import hashlib
import hmac
import importlib.util
import json
import logging
import os
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

GIB = 1024 ** 3
POLICY = "verify-seal-recycle-v1"
TRUST_LIMIT = "Local pre-disposal verification; deleted artifact bytes cannot be independently reread."
MODULE_DIR = Path(__file__).resolve().parent
FIXTURE_BYTES = b"ASG recycle fixture\n" * 256
_LOGGER = logging.getLogger(__name__)
_MODULES: dict[str, Any] = {}


def _module(name: str):
    if name not in _MODULES:
        spec = importlib.util.spec_from_file_location("retention_" + name, MODULE_DIR / ("build_feedback_" + name + ".py"))
        if spec is None or spec.loader is None:
            raise ValueError("approved helper unavailable")
        module = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = module
        spec.loader.exec_module(module)
        _MODULES[name] = module
    return _MODULES[name]


def _need(condition: bool, reason: str) -> None:
    if not condition:
        raise ValueError(reason)


def _stamp() -> str:
    return datetime.now(timezone.utc).isoformat()


def _canonical(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode()).hexdigest()


def sha256_file(path: Path) -> str:
    return _module("runner").sha256_file(path)


def _read(path: Path) -> dict:
    return _module("runner")._read_json(path)


def _new(path: Path, value: dict) -> dict:
    """Publish only a completely flushed immutable receipt; interrupted temps stay visible."""
    _need(not path.exists(), "immutable receipt already exists")
    temporary = path.with_name(path.name + ".pending-" + uuid.uuid4().hex)
    _module("runner")._write_new(temporary, value)
    # On this Windows-only lifecycle rename fails on an existing destination.
    _need(os.name == "nt", "atomic immutable Windows publication required")
    temporary.rename(path)
    return {"path": str(path.resolve()), "sha256": sha256_file(path)}


def _proof(reference: Any) -> tuple[Path, dict]:
    _need(isinstance(reference, dict) and isinstance(reference.get("path"), str), "missing pinned receipt")
    path = _path(reference["path"])
    return path, _module("statistics")._proof_json(reference)


def _path(value: Any, *, exists: bool = True) -> Path:
    _need(isinstance(value, str) and Path(value).is_absolute(), "absolute dedicated path required")
    path = Path(os.path.abspath(value))
    for ancestor in [path, *path.parents]:
        if ancestor.exists():
            attributes = getattr(ancestor.lstat(), "st_file_attributes", 0)
            _need(not ancestor.is_symlink() and not attributes & 0x400, "reparse/alias disposal forbidden")
    _need(path.resolve(strict=exists) == path, "resolved path changed")
    return path


def _inside(path: Path, root: Path) -> bool:
    return path == root or root in path.parents


def validate_policy(protocol: dict) -> tuple[dict, dict]:
    _need(type(protocol.get("schema_version")) is int and protocol["schema_version"] == 2, "retention requires protocol v2")
    declared_policy = protocol.get("retention_policy")
    if not isinstance(declared_policy, dict) or declared_policy.get("mode") != "sealed-verified-recycle":
        raise ValueError("retention policy required")
    _, approval = _proof(declared_policy.get("approval"))
    _need(approval.get("kind") == "build-feedback-retention-approval" and approval.get("schema_version") == 2
          and approval.get("approved") is True, "independent retention approval required")
    _need(approval.get("source_receipt_sha256") == protocol["source"]["receipt_sha256"]
          and approval.get("baseline_receipt_sha256") == protocol["baseline"]["receipt_sha256"]
          and approval.get("toolchain_sha256") == _canonical(protocol["toolchain"])
          and approval.get("configs_sha256") == _canonical(protocol["configs"]), "retention approval source/baseline mismatch")
    _need(approval.get("run_root") == protocol["run_root"] and approval.get("flow") == ["verify", "recycle", "match", "purge-exact", "verify-release", "receipt"]
          and approval.get("scope") == "validated-successful-and-cap-stopped-task-intermediates", "retention approval flow mismatch")
    _need(approval.get("protected") == ["source", "git-common-directory", "daily-cache", "existing-last-72-hours", "stores", "seeds", "quality-source-failures", "unrelated-recycle-items", "pagefile"], "protected categories missing")
    policy = {"mode": POLICY, "approval": declared_policy["approval"], "checker": approval.get("checker"),
              "dedicated_output_roots": approval.get("disposable_target_paths"), "protected_roots": approval.get("protected_roots"),
              "allow_recent_new_outputs": approval.get("allow_recent_new_outputs")}
    expected_checker = MODULE_DIR / "build_feedback_retention.py"
    checker = policy.get("checker")
    _need(isinstance(checker, dict) and checker.get("path") == str(expected_checker)
          and checker.get("sha256") == sha256_file(expected_checker)
          and checker.get("bytes") == expected_checker.stat().st_size and approval.get("checker") == checker,
          "unapproved retention checker")
    tools = approval.get("verification_tools")
    if not isinstance(tools, dict) or set(tools) != {"runner", "statistics", "benchmark", "inventory"}:
        raise ValueError("verification tool manifest missing")
    for name, entry in tools.items():
        expected = MODULE_DIR / ("build_feedback_" + name + ".py")
        _need(entry == {"path": str(expected), "sha256": sha256_file(expected), "bytes": expected.stat().st_size}, "approved verification tool changed")
    declared = policy["dedicated_output_roots"]
    protected = policy.get("protected_roots")
    _need(isinstance(declared, list) and bool(declared) and len(set(declared)) == len(declared)
          and approval.get("disposable_target_paths") == declared, "exact output allowlist missing")
    if not isinstance(protected, list) or not protected or approval.get("protected_roots") != protected:
        raise ValueError("protected roots missing")
    _need(policy.get("allow_recent_new_outputs") is True and approval.get("allow_recent_new_outputs") is True, "new output exception not approved")
    key_reference = approval.get("local_integrity_key")
    if not isinstance(key_reference, dict) or set(key_reference) != {"path", "sha256", "bytes"}:
        raise ValueError("local seal integrity key required")
    key_path = _path(key_reference["path"])
    _need(key_path.is_absolute() and key_path.is_file() and key_path.stat().st_size == 32
          and key_reference["bytes"] == 32 and sha256_file(key_path) == key_reference["sha256"], "local seal integrity key changed")
    _need(not any(_inside(key_path.resolve(), Path(root).resolve()) for root in declared), "integrity key must survive disposal")
    _need(isinstance(approval.get("allowed_source_roots"), list) and bool(approval["allowed_source_roots"]), "approved source roots missing")
    for value in [*declared, *protected, *approval["allowed_source_roots"]]:
        _path(value, exists=False)
    expected_scratch = sorted(
        str(Path(entry["release_suite_root"]) / "scratch")
        for entry in protocol.get("qualification_allocations", [])
    )
    if protocol.get("qualification_allocations"):
        _need(approval.get("producer_owned_scratch_cleanup_paths") == expected_scratch,
              "producer-owned scratch cleanup approval mismatch")
        for value in expected_scratch:
            scratch = _path(value, exists=False)
            _need(any(_inside(scratch, Path(entry["release_suite_root"]))
                      for entry in protocol["qualification_allocations"]),
                  "producer-owned scratch escaped release suite")
    return policy, approval


def declare_output(protocol: dict, target: Path, lease_path: Path) -> dict:
    policy, _ = validate_policy(protocol)
    target = _path(str(target), exists=False)
    _need(str(target) in policy["dedicated_output_roots"] and not target.exists(), "output must be predeclared while absent")
    _need(not _inside(lease_path.resolve(), target), "lease must survive disposal")
    payload = {"kind": "build-feedback-new-output-lease", "schema_version": 1,
                            "target": str(target), "observed_absent": True, "declared_at": _stamp(),
                            "protocol_sha256": _canonical(protocol), "approval": policy["approval"], "nonce": uuid.uuid4().hex}
    return _new(lease_path, _signed(payload, protocol))


def _safe_target(protocol: dict, target: Path, lease: Any) -> Path:
    policy, _ = validate_policy(protocol)
    target = _path(str(target), exists=False)
    _need(str(target) in policy["dedicated_output_roots"], "target is outside exact allowlist")
    protected = [Path(value).resolve() for value in policy["protected_roots"]]
    protected += [Path(protocol["source"]["root"]).resolve(), MODULE_DIR.parent.parent]
    _, approval = validate_policy(protocol)
    protected += [Path(root).resolve() for root in approval["allowed_source_roots"]]
    protected += [Path(root).resolve() for root in protocol.get("store_roots", [])]
    for config in protocol.get("configs", {}).values():
        tool = config.get("tool", {})
        protected += [Path(tool[key]).resolve() for key in ("store", "prewarm_source_root", "prewarm_receipt_path") if key in tool]
    for value in (protocol.get("source", {}).get("receipt_path"), protocol.get("baseline", {}).get("receipt_path"), approval["local_integrity_key"]["path"]):
        if value is not None:
            protected.append(Path(value).resolve())
    coordinator = MODULE_DIR.parent.parent
    for root in protected:
        if root == coordinator and _inside(target, coordinator / ".trellis" / ".runtime"):
            continue
        _need(not _inside(target, root) and not _inside(root, target), "source/Git/cache overlap")
    _need(".git" not in {part.casefold() for part in target.parts} and target.name.casefold() == "targets", "only dedicated targets directory is disposable")
    _, declaration = _proof(lease)
    _verify_signed(declaration, protocol)
    _need(declaration.get("kind") == "build-feedback-new-output-lease" and declaration.get("observed_absent") is True
          and declaration.get("target") == str(target) and declaration.get("protocol_sha256") == _canonical(protocol)
          and declaration.get("approval") == policy["approval"], "precreation lease mismatch")
    if target.exists():
        declared = datetime.fromisoformat(declaration["declared_at"]).timestamp()
        created = getattr(target.stat(), "st_birthtime", target.stat().st_ctime)
        _need(created + 1 >= declared, "existing recent output is protected")
    return target


def tree_manifest(target: Path) -> dict:
    """Hash approved output data, rejecting unknown aliases/streams/links."""
    target = _path(str(target))
    reader = _module("inventory")._new_metadata_reader()
    entries, identities, allocation = [], set(), 0
    allocation_identities: set[tuple[int, int]] = set()
    directories: list[tuple[Path, Any, list[str]]] = []
    files: list[tuple[Path, Any]] = []
    file_identities: dict[tuple[int, int], list[Path]] = {}
    stack = [target]
    while stack:
        path = stack.pop()
        _path(str(path))
        _need(path.name.casefold() not in {".git", "crates", "docs", "schemas", "skills", "github_src", "spikes", "scripts", ".github", ".agents", ".codex", ".claude", ".trellis"}, "unexpected source directory in disposable output")
        meta = reader.read(path)
        _need(not meta.directory or meta.identity not in identities, "hardlink/identity ambiguity")
        _need(meta.directory or meta.links >= 1, "hardlink/identity ambiguity")
        _need(meta.stream_count == (0 if meta.directory else 1), "named stream disposal unsupported")
        identities.add(meta.identity)
        entry = {"relative": path.relative_to(target).as_posix(), "directory": meta.directory,
                 "identity": list(meta.identity), "allocated_bytes": meta.allocated_bytes,
                 "bytes": meta.logical_bytes, "modified": meta.last_write}
        if meta.directory:
            before = sorted(child.name for child in path.iterdir())
            directories.append((path, meta, before))
            stack.extend(path / name for name in reversed(before))
            _need(sorted(child.name for child in path.iterdir()) == before, "output tree changed during scan")
        else:
            entry["sha256"] = sha256_file(path)
            if meta.identity not in allocation_identities:
                allocation += meta.allocated_bytes
                allocation_identities.add(meta.identity)
            _need(reader.read(path) == meta, "output changed during hashing")
            files.append((path, meta))
            file_identities.setdefault(meta.identity, []).append(path)
        entries.append(entry)
    # Cargo may publish an artifact and an internally linked alias in the same
    # disposable target. That is safe only when every filesystem link is
    # accounted for by a path inside this target. A lower in-target count
    # proves an external hardlink still aliases the payload.
    for identity, paths in file_identities.items():
        metadata = reader.read(paths[0])
        _need(metadata.identity == identity and len(paths) == metadata.links,
              "hardlink/identity ambiguity")
    # Editing a file does not necessarily modify its parent directory stamp.
    # Recheck every earlier observation after the entire tree has been hashed.
    for path, metadata in files:
        _path(str(path))
        _need(reader.read(path) == metadata, "output file changed during full scan")
    for path, metadata, names in directories:
        _path(str(path))
        _need(reader.read(path) == metadata and sorted(child.name for child in path.iterdir()) == names,
              "output directory changed during full scan")
    entries.sort(key=lambda entry: entry["relative"])
    return {"entries": entries, "allocated_bytes": allocation, "root_identity": entries[0]["identity"]}


def _same_tree(expected: dict, observed: dict) -> bool:
    return expected == observed


def _required_reference(reference: Any, target: Path) -> dict:
    path, size = _module("statistics")._proof_file(reference)
    path = _path(str(path))
    _need(not _inside(path, target), "required evidence must survive disposal")
    return {"path": str(path), "sha256": reference["sha256"], "bytes": size}


def _retained_refs(value: Any, target: Path) -> list[dict]:
    found: dict[str, dict] = {}
    def visit(item: Any) -> None:
        if isinstance(item, dict):
            if isinstance(item.get("path"), str) and isinstance(item.get("sha256"), str):
                path = Path(item["path"])
                if path.is_absolute() and not _inside(path.resolve(), target):
                    ref = _required_reference(item, target)
                    found[ref["path"]] = ref
            for stream in ("stdout", "stderr"):
                if stream + "_path" in item:
                    ref = _required_reference({"path": item[stream + "_path"], "sha256": item.get(stream + "_sha256")}, target)
                    found[ref["path"]] = ref
            for key, entry in item.items():
                if key == "receipt_path" or key.endswith("_receipt_path"):
                    ref = _required_reference({"path": entry, "sha256": item.get(key[:-5] + "_sha256")}, target)
                    found[ref["path"]] = ref
                if key in {"report", "official_report", "validator", "release_binary"} and isinstance(entry, dict):
                    ref = _required_reference(entry, target)
                    found[ref["path"]] = ref
            for child in item.values():
                visit(child)
        elif isinstance(item, list):
            for child in item:
                visit(child)
    visit(value)
    return sorted(found.values(), key=lambda ref: ref["path"])


def _arm_identity(protocol: dict) -> dict:
    source, chain = protocol["source"], protocol["toolchain"]
    return {
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


def _qualified_commands(protocol: dict, normalized_arm: dict, arm_name: str, pair_index: int | None,
                        attempt: int | None, qualifications: dict[str, dict]) -> dict:
    """Use the same original command/report/release checks for both lifecycles."""
    stats = _module("statistics")
    _need(set(qualifications) == set(stats.QUALIFICATIONS) - {"edit"},
          "complete artifact-dependent qualifications required before disposal")
    qualified = {}
    for name, ref in qualifications.items():
        _, proof = _proof(ref)
        _need((pair_index is not None or all(key in proof for key in ("pair_index", "attempt", "arm")))
              and (proof.get("pair_index"), proof.get("attempt"), proof.get("arm")) == (pair_index, attempt, arm_name),
              "qualification allocation mismatch")
        _need(stats._command_proof_valid(proof, name, normalized_arm, normalized_arm["identity"],
                                        _canonical(protocol), protocol=protocol), "qualification verification failed")
        if name == "release":
            report = stats._official_release_valid(proof, normalized_arm["identity"], protocol=protocol,
                                                  arm_evidence=normalized_arm["evidence"])
            stats._release_workload_samples(report, proof)
        qualified[name] = {"reference": ref, "proof": proof}
    return qualified


def create_seal(protocol: dict, normalized_arm: dict, arm_name: str, pair_index: int, attempt: int,
                lease: dict, qualifications: dict[str, dict], seal_path: Path) -> dict:
    policy, approval = validate_policy(protocol)
    _need(approval.get("purpose") != "synthetic-recycle-lifecycle", "fixture approval cannot seal Cargo evidence")
    stats, runner = _module("statistics"), _module("runner")
    runner._validate_protocol(protocol)
    _need(protocol.get("phase") in {"pilot", "formal"} and "retained_evidence" not in normalized_arm,
          "only original live paired arms may be sealed")
    _, raw = _proof(normalized_arm.get("evidence"))
    target = _safe_target(protocol, Path(raw["target_root"]), lease)
    _need(type(pair_index) is int and type(attempt) is int and arm_name in ("baseline", "candidate"), "invalid sealed arm allocation")
    expected = Path(protocol["run_root"]) / "pairs" / f"pair-{pair_index:04d}-attempt-{attempt:02d}" / arm_name / "targets"
    _need(target == expected and Path(normalized_arm["evidence"]["path"]) == expected.parent / "arm.json", "seal arm target/receipt assignment changed")
    allocation = [entry for entry in protocol["allocations"] if entry["pair_index"] == pair_index and entry["attempt"] == attempt]
    _need(len(allocation) == 1 and raw["source_root"] == allocation[0][arm_name + "_source_root"]
          and raw["source_root"] in approval["allowed_source_roots"], "seal source allocation changed")
    if "output_lease" in raw:
        _need(raw["output_lease"] == lease, "arm creation lease changed")
    _need(not _inside(seal_path.resolve(), target), "seal must survive disposal")
    expected_identity = _arm_identity(protocol)
    _need(stats._identity_matches(normalized_arm.get("identity"), expected_identity)
          and normalized_arm.get("protocol_sha256") == _canonical(protocol)
          and raw.get("config") == protocol["configs"][arm_name]
          and normalized_arm.get("config_sha256") == _canonical(protocol["configs"][arm_name]), "seal arm protocol/toolchain/config identity mismatch")
    runner._verify_source(Path(raw["source_root"]), protocol)
    _need(all(normalized_arm.get(flag) is True for flag in ("capture_complete", "source_verified", "artifacts_verified", "allocation_complete", "quiet"))
          and normalized_arm.get("storage_scope") == "owned_targets_and_stores"
          and raw.get("capture_complete") is True and raw.get("allocation_complete") is True,
          "arm projected closure incomplete")
    _need(stats._arm_proof_valid(normalized_arm, arm_name, expected_identity, protocol=protocol, pair_index=pair_index, attempt=attempt), "arm prerecycle verification failed")
    qualified = _qualified_commands(protocol, normalized_arm, arm_name, pair_index, attempt, qualifications)
    allocation_reference = {"path": str(target.parent / "owned-allocation.json"), "sha256": raw.get("owned_allocation_receipt_sha256")}
    _required_reference(allocation_reference, target)
    retained = _retained_refs([normalized_arm["evidence"], raw, qualified, protocol["source"], policy["approval"], lease, allocation_reference], target)
    payload = {"kind": "build-feedback-artifact-seal", "schema_version": 1, "phase": protocol["phase"],
               "protocol_sha256": _canonical(protocol), "approval": policy["approval"], "checker": policy["checker"],
               "arm": arm_name, "pair_index": pair_index, "attempt": attempt, "arm_reference": normalized_arm["evidence"],
               "arm_projection": normalized_arm, "arm_receipt": raw, "target": str(target), "lease": lease,
               "qualifications": qualified, "retained_files": retained, "tree": tree_manifest(target),
               "sealed_at": _stamp(), "trust_limit": TRUST_LIMIT}
    runner._verify_source(Path(raw["source_root"]), protocol)
    payload["closure_sha256"] = _canonical(payload)
    return _new(seal_path, _signed(payload, protocol))


def create_capacity_seal(protocol: dict, receipt_reference: dict, lease: dict, seal_path: Path) -> dict:
    policy, approval = validate_policy(protocol)
    _need(approval.get("purpose") != "synthetic-recycle-lifecycle", "fixture approval cannot seal Cargo evidence")
    runner = _module("runner")
    runner._validate_protocol(protocol)
    path, receipt = _proof(receipt_reference)
    _need(protocol.get("phase") == "capacity" and receipt.get("status") in {"ok", "capacity_limited"}, "only verified success/cap-stop probe eligible")
    target = _safe_target(protocol, Path(receipt["target_root"]), lease)
    expected = Path(protocol["run_root"]) / "capacity" / "targets"
    _need(target == expected and path == target.parent / "capacity.json", "capacity receipt allocation mismatch")
    source = Path(protocol["capacity_probe"]["source_root"])
    if "output_lease" in receipt:
        _need(receipt["output_lease"] == lease, "capacity creation lease changed")
    _need(str(source) in approval["allowed_source_roots"] and receipt.get("source_root") == str(source), "capacity source not independently approved")
    _need(receipt.get("schema_version") == 2 and receipt.get("kind") == "build-feedback-capacity-observation"
          and receipt.get("phase") == "capacity" and receipt.get("protocol_sha256") == _canonical(protocol)
          and receipt.get("source_commit") == protocol["source"]["commit"] and receipt.get("source_tree") == protocol["source"]["tree"]
          and receipt.get("source_receipt_sha256") == protocol["source"]["receipt_sha256"]
          and receipt.get("toolchain_sha256") == _canonical(protocol["toolchain"]), "capacity identity mismatch")
    _need(receipt.get("process_tree_drained") is True and receipt.get("source_verified") is True
          and receipt.get("allocation_complete") is True and receipt.get("capture_complete") is True
          and receipt.get("performance_evidence") is False and receipt.get("completed_valid_pairs") == 0, "probe closure incomplete")
    runner._verify_source(source, protocol)
    capture_path, capture = _proof(receipt.get("capture_receipt"))
    _need(capture_path == target.parent / "logs" / "capture.json", "capacity capture allocation mismatch")
    _need(receipt.get("jobs") == [capture] and capture.get("argv") == runner.capacity_command(protocol, target)
          and capture.get("cwd") == str(source) and capture.get("status") == receipt["status"]
          and capture.get("process_tree_capture_complete") is True and capture.get("external_processes") == []
          and capture.get("memory_limit_bytes") == 24 * GIB and type(capture.get("peak_commit_bytes")) is int
          and 0 <= capture["peak_commit_bytes"] <= 24 * GIB, "capacity capture not complete/owned")
    stats = _module("statistics")
    _need(stats._capture_valid({**capture, "status": "ok", "returncode": 0})
          and type(capture.get("returncode")) is int and receipt.get("disposal_eligible") is True
          and receipt.get("config") == protocol["configs"]["candidate"]
          and receipt.get("owned_output_cap_bytes") == protocol["capacity_probe"]["owned_output_cap_bytes"], "capacity capture identity incomplete")
    for stream in ("stdout", "stderr"):
        _required_reference({"path": capture[stream + "_path"], "sha256": capture.get(stream + "_sha256")}, target)
    _need(receipt.get("memory_limit_bytes") == capture["memory_limit_bytes"]
          and receipt.get("peak_commit_bytes") == capture["peak_commit_bytes"]
          and stats._utc(receipt["started_at"]) <= stats._utc(capture["started_at"])
          <= stats._utc(capture["finished_at"]) <= stats._utc(receipt["finished_at"]), "capacity timing/memory projection mismatch")
    samples = capture.get("resource_samples")
    if not isinstance(samples, list) or not samples or not all(isinstance(entry, dict) for entry in samples):
        raise ValueError("capacity resource samples missing")
    floor = protocol["budget_policy"]["minimum_free_bytes"]
    _need(type(receipt.get("minimum_disk_free_bytes")) is int and receipt["minimum_disk_free_bytes"] >= floor
          and all(type(entry.get("disk_free_bytes")) is int and entry["disk_free_bytes"] >= floor for entry in samples), "capacity free-space floor failed")
    if receipt["status"] == "ok":
        cargo = runner.parse_cargo_json_file(Path(capture["stdout_path"]), target, source)
        _need(capture.get("returncode") == 0 and cargo.get("build_success") is True and cargo.get("errors") == []
              and bool(cargo.get("artifacts")) and capture.get("cargo") == cargo, "capacity successful Cargo proof missing")
    else:
        _need(capture.get("reasons") == ["owned_output_cap_reached"] and receipt.get("reasons") == ["owned_output_cap_reached"]
              and any(entry.get("budget_allowed") is False and "owned_output_cap_reached" in entry.get("budget_reasons", []) for entry in samples),
              "unapproved capacity stop reason")
        _need(not runner._file_contains_markers(Path(capture["stderr_path"]), runner.MEMORY_ERROR_MARKERS), "capacity memory failure is protected")
        _need(runner.capacity_cargo_failure(Path(capture["stdout_path"]), target, source) == []
              and receipt.get("cap_stop_quality_errors") == [], "capacity quality/capture failure is protected")
    allocation_path, allocation = _proof(receipt.get("owned_allocation_receipt"))
    _need(allocation_path == target.parent / "target-allocation.json"
          and receipt.get("target_allocation_receipt") == receipt["owned_allocation_receipt"], "capacity target allocation reference mismatch")
    _need(allocation.get("complete") is True and allocation.get("errors") == []
          and allocation.get("allocated_bytes") == receipt.get("owned_allocated_bytes"), "capacity allocation proof mismatch")
    output_allocation_path, output_allocation = _proof(receipt.get("task_output_allocation_receipt"))
    _need(output_allocation_path == target.parent / "owned-allocation.json", "capacity output allocation reference mismatch")
    _need(output_allocation.get("complete") is True and output_allocation.get("errors") == []
          and type(output_allocation.get("allocated_bytes")) is int
          and output_allocation["allocated_bytes"] == receipt.get("task_output_allocated_bytes")
          and allocation["allocated_bytes"] <= output_allocation["allocated_bytes"] <= protocol["capacity_probe"]["owned_output_cap_bytes"], "capacity total output allocation mismatch")
    sample_bounds = [entry.get("job_write_bound_bytes") for entry in samples]
    final_bound = receipt.get("final_job_write_bound_bytes")
    _need(all(type(value) is int and value >= 0 for value in sample_bounds)
          and type(final_bound) is int and final_bound >= 0
          and receipt.get("owned_peak_kind") == "max-post-drain-allocation-and-job-write-bound", "capacity live peak evidence missing")
    expected_peak = max(output_allocation["allocated_bytes"], final_bound, *sample_bounds)
    _need(type(receipt.get("peak_owned_output_bytes")) is int and receipt["peak_owned_output_bytes"] == expected_peak,
          "capacity peak differs from complete owned observation")
    manifest = tree_manifest(target)
    _need(manifest["allocated_bytes"] == receipt.get("owned_allocated_bytes")
          and manifest["allocated_bytes"] <= protocol["capacity_probe"]["owned_output_cap_bytes"], "capacity actual allocation cap changed")
    _need(not _inside(seal_path.resolve(), target), "seal must survive disposal")
    payload = {"kind": "build-feedback-capacity-seal", "schema_version": 1, "phase": "capacity", "target": str(target),
               "protocol_sha256": _canonical(protocol), "approval": policy["approval"], "checker": policy["checker"],
               "probe_reference": receipt_reference, "probe_receipt": receipt, "lease": lease,
               "retained_files": _retained_refs([receipt_reference, receipt, protocol["source"], policy["approval"], lease], target),
               "tree": manifest, "sealed_at": _stamp(), "trust_limit": TRUST_LIMIT}
    runner._verify_source(source, protocol)
    payload["closure_sha256"] = _canonical(payload)
    return _new(seal_path, _signed(payload, protocol))


def _full_capacity_identity(protocol: dict, arm: dict, raw: dict, approval: dict, lease: dict) -> tuple[str, Path]:
    probe = protocol.get("capacity_probe", {})
    _need(protocol.get("phase") == "capacity" and probe.get("kind") == "full-arm-and-qualification"
          and probe.get("stage") == "expanded" and protocol.get("pairs") == 0
          and type(protocol.get("pairs")) is int and protocol.get("allocations") == [], "full unpaired capacity protocol required")
    name = probe.get("arm")
    _need(name in ("baseline", "candidate") and "retained_evidence" not in arm, "original full capacity arm required")
    target = _safe_target(protocol, Path(raw["target_root"]), lease)
    expected = Path(protocol["run_root"]) / "capacity"
    _need(target == expected / "targets" and arm.get("evidence", {}).get("path") == str(expected / "arm.json"),
          "full capacity target/receipt allocation mismatch")
    _need(raw.get("source_root") == probe.get("source_root") and raw["source_root"] in approval["allowed_source_roots"]
          and arm.get("source_root") == raw["source_root"] and raw.get("output_lease") == lease, "full capacity source/lease mismatch")
    _need(type(raw.get("schema_version")) is int and raw["schema_version"] == 2
          and raw.get("kind") == "build-feedback-arm" and raw.get("arm") == name and raw.get("phase") == "capacity"
          and raw.get("performance_evidence") is False and type(raw.get("completed_valid_pairs")) is int
          and raw["completed_valid_pairs"] == 0 and raw.get("protocol_sha256") == _canonical(protocol)
          and raw.get("source_receipt_sha256") == protocol["source"]["receipt_sha256"]
          and raw.get("toolchain_sha256") == _canonical(protocol["toolchain"]), "full capacity raw identity mismatch")
    _need(raw.get("config") == protocol["configs"][name] and arm.get("config_sha256") == _canonical(protocol["configs"][name])
          and raw.get("source_commit") == protocol["source"]["commit"] and raw.get("source_tree") == protocol["source"]["tree"]
          and arm.get("protocol_sha256") == _canonical(protocol)
          and _module("statistics")._identity_matches(arm.get("identity"), _arm_identity(protocol)), "full capacity config/source mismatch")
    _need(raw.get("source_verified") is True and raw.get("process_tree_drained") is True
          and raw.get("allocation_complete") is True, "full capacity source/drain/allocation incomplete")
    _need(raw.get("capture_complete") is True and raw.get("disposal_eligible") is True
          and "pair_index" in raw and raw["pair_index"] is None and "attempt" in raw and raw["attempt"] is None,
          "full capacity original closure or ordinals incomplete")
    if raw.get("edit_jobs"):
        _need(raw.get("own_edit_restored") is True, "full capacity edit restoration missing")
    allocation = _proof({"path": str(expected / "owned-allocation.json"),
                         "sha256": raw.get("owned_allocation_receipt_sha256")})[1]
    _need(allocation == raw.get("owned_allocation_receipt") and allocation.get("complete") is True
          and allocation.get("errors") == [] and allocation.get("storage_scope") == "owned_targets_and_stores"
          and type(allocation.get("allocated_bytes")) is int and allocation["allocated_bytes"] >= 0
          and allocation["allocated_bytes"] == raw.get("owned_allocated_bytes"), "full capacity original allocation changed")
    _module("runner")._verify_source(Path(raw["source_root"]), protocol)
    return name, target


def _capacity_samples(capture: dict, protocol: dict, *, stopped: bool = False) -> tuple[int, int]:
    stats = _module("statistics")
    status = capture.get("status")
    _need(status in ({"ok", "capacity_limited"} if stopped else {"ok"}), "capacity capture failure is protected")
    _need(stats._capture_valid({**capture, "status": "ok", "returncode": 0})
          and type(capture.get("returncode")) is int, "capacity process/log proof incomplete")
    samples = capture.get("resource_samples")
    if not isinstance(samples, list) or not samples:
        raise ValueError("full capacity live resource samples missing")
    peaks, floors = [], []
    guarded = False
    for sample in samples:
        _need(isinstance(sample, dict) and sample.get("allocation_kind") == "conservative-disk-and-job-write-bound",
              "full capacity real resource monitor required")
        _need(all(type(sample.get(key)) is int and sample[key] >= 0
                  for key in ("allocated_bytes", "disk_free_bytes", "job_write_bound_bytes")), "full capacity resource type mismatch")
        _need(sample["disk_free_bytes"] >= protocol["budget_policy"]["minimum_free_bytes"], "full capacity free-space floor failed")
        reasons = sample.get("budget_reasons")
        _need(isinstance(reasons, list), "full capacity guard reasons missing")
        if sample.get("budget_allowed") is True:
            _need(reasons == [], "full capacity allowed sample has failure reasons")
        else:
            _need(stopped and status == "capacity_limited" and sample.get("budget_allowed") is False
                  and set(reasons) <= {"owned_output_cap_reached", "project_review_threshold_reached"}
                  and "owned_output_cap_reached" in reasons, "unapproved full capacity stop reason")
            guarded = True
        peaks.append(sample["job_write_bound_bytes"])
        floors.append(sample["disk_free_bytes"])
    _need(stats._utc(capture["started_at"]) <= stats._utc(capture["finished_at"]), "full capacity capture time reversed")
    if status == "capacity_limited":
        _need(capture.get("reasons") == ["owned_output_cap_reached"] and guarded, "full capacity guard flag lacks observed stop")
    else:
        _need(capture["returncode"] == 0 and capture.get("reasons") == [], "capacity successful capture failed")
    _need(not _module("runner")._file_contains_markers(Path(capture["stderr_path"]), _module("runner").MEMORY_ERROR_MARKERS),
          "capacity memory failure is protected")
    return max(peaks), min(floors)


def _full_capacity_captures(protocol: dict, raw: dict, target: Path, *, stopped: bool) -> tuple[list[dict], list[dict]]:
    runner = _module("runner")
    captures, references = [], []
    incomplete = False
    previous_finish = None
    for phase, field in (("primary", "jobs"), ("feedback-warm", "feedback_warm_jobs"), ("edit", "edit_jobs")):
        records = raw.get(field)
        if not isinstance(records, list) or not all(isinstance(entry, dict) for entry in records):
            raise ValueError("full capacity phase captures missing")
        starts = [_module("statistics")._utc(entry["started_at"]) for entry in records]
        finishes = [_module("statistics")._utc(entry["finished_at"]) for entry in records]
        if records:
            _need(previous_finish is None or min(starts) >= previous_finish, "capacity phase intervals overlap")
            previous_finish = max(finishes)
        names = [entry.get("job") for entry in records]
        ordered = [job["name"] for job in runner.JOBS if job["name"] in names]
        _need(names == ordered and len(names) == len(set(names)), "full capacity launched job order changed")
        _need(not incomplete or not records, "full capacity jobs after incomplete phase")
        if len(records) != len(runner.JOBS):
            _need(stopped, "full capacity eighteen jobs required")
            incomplete = True
        for record in records:
            fixed = next(job for job in runner.JOBS if job["name"] == record["job"])
            config = raw["config"]
            mode = "primary" if phase == "primary" or config["incremental"] == "on" else "feedback"
            assigned = target / mode / (fixed["name"] if config["layout"] == "isolated" else fixed["lane"])
            _need(record.get("argv") == runner.job_command(protocol, fixed, assigned)
                  and record.get("cwd") == raw["source_root"], "full capacity raw command allocation mismatch")
            path = target.parent / "logs" / (phase + "-" + record["job"] + ".json")
            reference = {"path": str(path), "sha256": sha256_file(path)}
            _need(_proof(reference)[1] == record, "full capacity launched capture changed")
            for stream in ("stdout", "stderr"):
                stream_path = _path(record.get(stream + "_path"))
                _need(_inside(stream_path, target.parent / "logs"), "full capacity log allocation mismatch")
            _capacity_samples(record, protocol, stopped=stopped)
            if stopped:
                _need(runner.capacity_cargo_failure(Path(record["stdout_path"]), target, Path(raw["source_root"])) == [],
                      "full capacity quality failure is protected")
            captures.append(record)
            references.append(reference)
        if any(entry.get("status") == "capacity_limited" for entry in records):
            incomplete = True
    _need(bool(captures), "full capacity requires actual launched captures")
    return captures, references


def _capacity_qualification_commands(protocol: dict, arm: dict, name: str, qualifications: dict[str, dict],
                                     *, stopped: bool) -> dict:
    stats = _module("statistics")
    if not stopped:
        result = _qualified_commands(protocol, arm, name, None, None, qualifications)
        for qualification, entry in result.items():
            _full_capacity_qualification_files(protocol, arm, qualification, entry, live=True)
        return result
    _need(set(qualifications) <= set(stats.QUALIFICATIONS) - {"edit"}, "unknown capacity qualification")
    qualified = {}
    terminal = False
    seen = list(qualifications)
    order = list(_module("runner").QUALIFICATION_NAMES)
    _need(set(seen) == set(order[:len(seen)]), "capacity qualification prefix incomplete")
    for qualification in order:
        if qualification not in qualifications:
            continue
        _need(not terminal, "qualification after capacity stop")
        ref = qualifications[qualification]
        _, proof = _proof(ref)
        _need(all(key in proof for key in ("pair_index", "attempt", "arm"))
              and proof["pair_index"] is None and proof["attempt"] is None and proof["arm"] == name
              and proof.get("qualification") == qualification and proof.get("kind") == "build-feedback-qualification-command"
              and proof.get("source_root") == arm["source_root"] and proof.get("config_sha256") == arm["config_sha256"]
              and stats._bound_proof(proof, arm["identity"], _canonical(protocol)), "capacity qualification allocation mismatch")
        capture = proof.get("capture")
        if not isinstance(capture, dict):
            raise TypeError("capacity qualification capture missing")
        _capacity_samples(capture, protocol, stopped=True)
        if capture["status"] == "ok":
            _need(stats._command_proof_valid(proof, qualification, arm, arm["identity"], _canonical(protocol), protocol=protocol),
                  "capacity completed qualification invalid")
            if qualification == "release":
                report = stats._official_release_valid(proof, arm["identity"], protocol=protocol, arm_evidence=arm["evidence"])
                stats._release_workload_samples(report, proof)
        else:
            terminal = True
            _need(capture["cwd"] == arm["source_root"], "capacity stopped qualifier source changed")
            if qualification in ("clippy", "optional_features"):
                base = [protocol["toolchain"]["cargo"]["path"], "clippy" if qualification == "clippy" else "test", "--workspace", "--all-targets"]
                if qualification == "optional_features":
                    base.append("--all-features")
                target = Path(_proof(arm["evidence"])[1]["target_root"]) / "qualification" / qualification
                base.extend(["--locked", "--offline", "-j2", "--message-format=json", "--target-dir", str(target)])
                base.extend(["--", "-D", "warnings"] if qualification == "clippy" else ["--", "--test-threads=2"])
                _need(capture["argv"] == base, "capacity stopped qualifier command changed")
                _need(_module("runner").capacity_cargo_failure(Path(capture["stdout_path"]), target, Path(arm["source_root"])) == [],
                      "capacity qualifier quality failure is protected")
            else:
                # There is no complete diagnostics/release report after forced
                # termination. Preserve it rather than granting generic disposal.
                raise ValueError("incomplete diagnostics/release qualification is protected")
        qualified[qualification] = {"reference": ref, "proof": proof}
        _full_capacity_qualification_files(protocol, arm, qualification, qualified[qualification], live=True)
    return qualified


def _full_capacity_qualification_files(protocol: dict, arm: dict, qualification: str, entry: dict, *, live: bool) -> None:
    raw = _proof(arm["evidence"])[1]
    target = Path(raw["target_root"])
    root = target.parent
    proof_path, proof = _proof(entry["reference"])
    _need(_inside(proof_path, root) and not _inside(proof_path, target), "full capacity qualifier proof outside billed arm root")
    capture = proof["capture"]
    _need(capture.get("qualification") == qualification and capture.get("protocol_sha256") == _canonical(protocol)
          and capture.get("config_sha256") == arm["config_sha256"] and capture.get("source_root") == raw["source_root"]
          and capture.get("arm") == raw["arm"] and "pair_index" in capture and capture["pair_index"] is None
          and "attempt" in capture and capture["attempt"] is None, "full capacity raw qualification identity mismatch")
    log_root = root / "qualification-captures" / qualification
    for stream in ("stdout", "stderr"):
        _need(_inside(_path(capture[stream + "_path"]), log_root), "full capacity qualifier log outside billed allocation")
    if capture["status"] == "ok":
        report_path, _ = _proof(proof.get("report"))
        _need(_inside(report_path, root) and not _inside(report_path, target), "full capacity qualifier report outside billed arm root")
    if live and qualification in ("clippy", "optional_features") and capture["status"] == "ok":
        decoded = _module("runner").parse_cargo_json_file(Path(capture["stdout_path"]), target / "qualification" / qualification,
                                                       Path(raw["source_root"]))
        emitted = [file for artifact in decoded["artifacts"] for file in artifact["files"]]
        _need(decoded.get("build_success") is True and decoded.get("errors") == [] and bool(emitted)
              and decoded == capture.get("cargo") and proof.get("artifacts") == emitted,
              "full capacity qualifier artifacts differ from raw Cargo output")


def _full_capacity_outputs(protocol: dict, raw: dict, qualified: dict, target: Path) -> tuple[list[Path], dict]:
    allocations = [entry for entry in protocol.get("qualification_allocations", [])
                   if entry.get("pair_index") is None and entry.get("attempt") is None and entry.get("arm") == raw["arm"]
                   and entry.get("source_root") == raw["source_root"]]
    _need(len(allocations) == 1, "full capacity qualification allocation missing")
    suite = _path(allocations[0]["release_suite_root"], exists=False)
    expected_name = "asg-build-feedback-capacity-capacity-0-" + raw["arm"]
    _need(suite == Path(raw["source_root"]) / "evidence-output" / "build-feedback" / expected_name,
          "full capacity release suite assignment changed")
    roots = [target.parent, suite]
    # The producer confines all official TEMP scratch to suite/scratch.
    # Require the exact persisted captures rather than caller-inline budgets.
    for qualification, entry in qualified.items():
        capture = entry["proof"]["capture"]
        path = target.parent / "qualification-captures" / qualification / "capture.json"
        _need(_proof({"path": str(path), "sha256": sha256_file(path)})[1] == capture,
              "full capacity qualifier raw capture changed")
    # The arm root includes its scratch directory, logs, reports and targets.
    report = _module("inventory").inventory([], store_roots=[root for root in roots if root.exists()], discover=False,
                                            minimum_free_bytes=protocol["budget_policy"]["minimum_free_bytes"])
    _need(report.get("complete") is True and report.get("errors") == [] and type(report.get("allocated_bytes")) is int
          and report["allocated_bytes"] >= 0, "full capacity final output allocation incomplete")
    _need(report["allocated_bytes"] <= protocol["capacity_probe"]["owned_output_cap_bytes"], "full capacity complete outputs exceed cap")
    return roots, report


def _full_capacity_cumulative_bounds(protocol: dict, raw: dict, captures: list[dict], qualified: dict, target: Path) -> int:
    cap = protocol["capacity_probe"]["owned_output_cap_bytes"]
    guard = raw.get("resource_guard")
    expected = {"kind": "windows-job-and-cumulative-owned-output-cap", "memory_limit_bytes": 24 * GIB,
                "owned_output_cap_bytes": cap, "minimum_free_bytes": protocol["budget_policy"]["minimum_free_bytes"],
                "external_growth_allowance_bytes": protocol["budget_policy"]["external_growth_allowance_bytes"]}
    _need(guard == expected and all(type(raw.get(key)) is int and raw[key] >= 0
          for key in ("allocated_before_bytes", "disk_free_before_bytes", "final_job_write_bound_bytes", "minimum_disk_free_bytes")),
          "full capacity original cumulative guard missing")
    _need(raw["minimum_disk_free_bytes"] >= expected["minimum_free_bytes"], "full capacity original free floor failed")
    observed_floor = min(sample["disk_free_bytes"] for field in ("jobs", "feedback_warm_jobs", "edit_jobs")
                         for capture in raw[field] for sample in capture["resource_samples"])
    previous_floor = raw["minimum_disk_free_bytes"]
    _need(previous_floor <= observed_floor and previous_floor <= raw["disk_free_before_bytes"],
          "full capacity original free minimum changed")
    origin = raw["allocated_before_bytes"]
    previous_finish = max(_module("statistics")._utc(capture["finished_at"]) for field in
                          ("jobs", "feedback_warm_jobs", "edit_jobs") for capture in raw[field])
    previous = max(raw["final_job_write_bound_bytes"], raw.get("owned_allocated_bytes", 0),
                   *[sample["job_write_bound_bytes"] for field in ("jobs", "feedback_warm_jobs", "edit_jobs")
                     for capture in raw[field] for sample in capture["resource_samples"]])
    for qualification in _module("runner").QUALIFICATION_NAMES:
        if qualification not in qualified:
            continue
        capture = qualified[qualification]["proof"]["capture"]
        _need(_module("statistics")._utc(capture["started_at"]) >= previous_finish,
              "capacity qualification intervals overlap")
        previous_finish = _module("statistics")._utc(capture["finished_at"])
        current = capture.get("resource_guard")
        _need(isinstance(current, dict) and all(current.get(key) == value for key, value in expected.items())
              and current.get("origin_allocated_bytes") == origin
              and current.get("origin_disk_free_bytes") == raw["disk_free_before_bytes"]
              and type(current.get("prior_write_bound_bytes")) is int and current["prior_write_bound_bytes"] >= previous
              and type(current.get("prior_minimum_disk_free_bytes")) is int
              and expected["minimum_free_bytes"] <= current["prior_minimum_disk_free_bytes"] <= previous_floor
              and capture.get("process_tree_drained") is True, "capacity qualification cumulative guard reset")
        final = capture.get("final_job_write_bound_bytes")
        _need(type(final) is int and final >= current["prior_write_bound_bytes"], "capacity qualifier final write bound missing")
        _need(capture.get("qualification_budget") == {**current, "final_job_write_bound_bytes": final,
              "minimum_disk_free_bytes": capture.get("minimum_disk_free_bytes"), "extra_owned_roots": []},
              "capacity qualification budget projection changed")
        _need(all(sample["job_write_bound_bytes"] >= current["prior_write_bound_bytes"] for sample in capture["resource_samples"]),
              "capacity qualifier samples lost preceding writes")
        allocation_reference = capture.get("owned_allocation_receipt")
        allocation_path, allocation = _proof(allocation_reference)
        expected_path = target.parent / "qualification-captures" / qualification / "owned-allocation.json"
        _need(allocation_path == expected_path and allocation.get("complete") is True and allocation.get("errors") == []
              and type(allocation.get("allocated_bytes")) is int and allocation["allocated_bytes"] == capture.get("owned_allocated_bytes"),
              "capacity qualification allocation mismatch")
        previous = max(final, allocation["allocated_bytes"],
                       *[sample["job_write_bound_bytes"] for sample in capture["resource_samples"]])
        previous_floor = capture.get("minimum_disk_free_bytes")
        _need(type(previous_floor) is int and expected["minimum_free_bytes"] <= previous_floor
              <= min(current["prior_minimum_disk_free_bytes"], *[sample["disk_free_bytes"] for sample in capture["resource_samples"]]),
              "capacity qualifier minimum free reset")
    return max(previous, *[max(0, sample["allocated_bytes"] - origin) for capture in captures for sample in capture["resource_samples"]])


def create_full_capacity_seal(protocol: dict, normalized_arm: dict, qualifications: dict[str, dict], lease: dict,
                              seal_path: Path) -> dict:
    """Verify an unpaired full workload (or explicitly guarded partial stop)."""
    policy, approval = validate_policy(protocol)
    _need(approval.get("purpose") != "synthetic-recycle-lifecycle", "fixture approval cannot seal Cargo evidence")
    runner, stats = _module("runner"), _module("statistics")
    runner._validate_protocol(protocol)
    _, raw = _proof(normalized_arm.get("evidence"))
    name, target = _full_capacity_identity(protocol, normalized_arm, raw, approval, lease)
    _need(not _inside(seal_path.resolve(), target) and seal_path.resolve().parent == target.parent,
          "full capacity seal must survive disposal inside billed arm root")
    statuses = [raw.get("status")]
    for ref in qualifications.values():
        _, proof = _proof(ref)
        statuses.append(proof.get("capture", {}).get("status"))
    _need(all(status in {"ok", "capacity_limited"} for status in statuses), "full capacity failure is protected")
    stopped = "capacity_limited" in statuses
    captures, references = _full_capacity_captures(protocol, raw, target, stopped=stopped)
    if raw["status"] == "ok":
        _need(stats._arm_proof_valid(normalized_arm, name, _arm_identity(protocol), protocol=protocol),
              "full capacity arm prerecycle verification failed")
    else:
        _need(raw.get("reasons") == ["owned_output_cap_reached"] and any(entry["status"] == "capacity_limited" for entry in captures),
              "full capacity raw stop reason missing")
        _need(qualifications == {}, "qualification cannot follow unfinished capacity arm")
    qualified = _capacity_qualification_commands(protocol, normalized_arm, name, qualifications, stopped=stopped)
    captures.extend(entry["proof"]["capture"] for entry in qualified.values())
    roots, report = _full_capacity_outputs(protocol, raw, qualified, target)
    report_path = target.parent / "full-output-allocation.json"
    allocation_reference = _new(report_path, report)
    peak = max(report["allocated_bytes"], *(max(_capacity_samples(capture, protocol, stopped=stopped)[0],
                   max(sample["allocated_bytes"] - raw["allocated_before_bytes"] for sample in capture["resource_samples"])) for capture in captures))
    peak = max(peak, _full_capacity_cumulative_bounds(protocol, raw, captures, qualified, target))
    _need(stopped or peak < protocol["capacity_probe"]["owned_output_cap_bytes"], "full successful capacity guard exceeded")
    manifest = tree_manifest(target)
    _need(manifest["allocated_bytes"] <= report["allocated_bytes"], "full capacity target exceeds complete output allocation")
    retained = _retained_refs([normalized_arm["evidence"], raw, qualified, protocol["source"], policy["approval"], lease,
                               allocation_reference, references], target)
    payload = {"kind": "build-feedback-full-capacity-seal", "schema_version": 1, "phase": "capacity",
               "protocol_sha256": _canonical(protocol), "approval": policy["approval"], "checker": policy["checker"],
               "performance_evidence": False, "completed_valid_pairs": 0, "pair_index": None, "attempt": None, "arm": name,
               "status": "capacity_limited" if stopped else "ok", "workload_complete": not stopped,
               "reservation_eligible": not stopped, "disposal_eligible": True,
               "arm_reference": normalized_arm["evidence"], "arm_projection": normalized_arm, "arm_receipt": raw,
               "qualifications": qualified, "target": str(target), "lease": lease, "retained_files": retained,
               "task_output_allocation_receipt": allocation_reference, "task_output_allocated_bytes": report["allocated_bytes"],
               "output_roots": [str(root) for root in roots], "observed_peak_bytes": peak,
               "minimum_disk_free_bytes": min(raw["minimum_disk_free_bytes"],
                    *[entry["proof"]["capture"]["minimum_disk_free_bytes"] for entry in qualified.values()],
                    *[sample["disk_free_bytes"] for capture in captures for sample in capture["resource_samples"]]),
               "config_sha256": _canonical(protocol["configs"][name]), "tree": manifest, "sealed_at": _stamp(), "trust_limit": TRUST_LIMIT}
    runner._verify_source(Path(raw["source_root"]), protocol)
    payload["closure_sha256"] = _canonical(payload)
    reference = _new(seal_path, _signed(payload, protocol))
    # Allocation-report and seal publication use space too. They must fit the
    # recorded conservative peak; a larger actual publication fails closed and
    # remains visible, instead of authorizing disposal under an understated cap.
    _verify_seal(reference, protocol)
    return reference


def _verify_full_capacity_closure(seal: dict, protocol: dict) -> None:
    """Replay retained command/resource facts without rereading deleted artifacts."""
    _, approval = validate_policy(protocol)
    raw = seal.get("arm_receipt")
    arm = seal.get("arm_projection")
    if not isinstance(raw, dict) or not isinstance(arm, dict):
        raise TypeError("retained full capacity raw/projection missing")
    name, target = _full_capacity_identity(protocol, arm, raw, approval, seal["lease"])
    _need(_proof(seal["arm_reference"])[1] == raw and arm.get("evidence") == seal["arm_reference"]
          and seal.get("target") == str(target) and seal.get("arm") == name
          and seal.get("config_sha256") == _canonical(protocol["configs"][name]), "retained capacity config/path changed")
    # The signed pre-disposal closure authorizes the artifact flag only. All
    # other projection fields are replayed from the preserved original bytes.
    projection = _module("benchmark")._normalize_runner_arm(raw, protocol, _module("runner"),
                    retained=seal if raw.get("status") == "ok" else None)
    _need(_canonical(projection) == _canonical(arm), "retained full capacity projection differs from original raw facts")
    _need(protocol["capacity_probe"].get("kind") == "full-arm-and-qualification"
          and seal.get("performance_evidence") is False and type(seal.get("completed_valid_pairs")) is int
          and seal["completed_valid_pairs"] == 0 and "pair_index" in seal and seal["pair_index"] is None
          and "attempt" in seal and seal["attempt"] is None, "full capacity cannot claim paired performance")
    qualified = seal.get("qualifications")
    if not isinstance(qualified, dict):
        raise TypeError("retained capacity qualifications missing")
    statuses = [raw.get("status")]
    for entry in qualified.values():
        _need(isinstance(entry, dict) and _proof(entry.get("reference"))[1] == entry.get("proof"),
              "retained capacity qualification changed")
        proof = entry["proof"]
        _full_capacity_qualification_files(protocol, arm, proof.get("qualification"), entry, live=False)
        _need(all(key in proof for key in ("pair_index", "attempt", "arm"))
              and proof["pair_index"] is None and proof["attempt"] is None and proof["arm"] == name
              and proof.get("source_root") == raw["source_root"] and proof.get("config_sha256") == arm["config_sha256"],
              "retained capacity qualification allocation changed")
        statuses.append(proof.get("capture", {}).get("status"))
    _need(all(status in {"ok", "capacity_limited"} for status in statuses), "retained full capacity failure is protected")
    stopped = "capacity_limited" in statuses
    complete = not stopped
    _need(seal.get("status") == ("ok" if complete else "capacity_limited")
          and type(seal.get("workload_complete")) is bool and seal["workload_complete"] == complete
          and type(seal.get("reservation_eligible")) is bool and seal["reservation_eligible"] == complete
          and seal.get("disposal_eligible") is True, "full capacity result eligibility mismatch")
    if complete:
        _need(set(qualified) == set(_module("statistics").QUALIFICATIONS) - {"edit"}
              and raw.get("status") == "ok" and raw.get("capture_complete") is True
              and raw.get("own_edit_restored") is True and all(arm.get(flag) is True for flag in
              ("capture_complete", "source_verified", "artifacts_verified", "allocation_complete", "quiet")),
              "retained full capacity qualifications incomplete")
    elif raw["status"] == "capacity_limited":
        _need(not qualified and raw.get("reasons") == ["owned_output_cap_reached"], "retained partial capacity status changed")
    captures, _ = _full_capacity_captures(protocol, raw, target, stopped=stopped)
    for qualification, entry in qualified.items():
        path = target.parent / "qualification-captures" / qualification / "capture.json"
        capture = entry["proof"]["capture"]
        _need(_proof({"path": str(path), "sha256": sha256_file(path)})[1] == capture,
              "retained qualifier capture changed")
        _capacity_samples(capture, protocol, stopped=stopped)
        captures.append(capture)
    allocation_path, allocation = _proof(seal.get("task_output_allocation_receipt"))
    roots = [str(target.parent), protocol["qualification_allocations"][0]["release_suite_root"]]
    _need(seal.get("output_roots") == roots and allocation_path == target.parent / "full-output-allocation.json"
          and allocation.get("complete") is True and allocation.get("errors") == []
          and type(allocation.get("allocated_bytes")) is int and allocation["allocated_bytes"] >= 0
          and allocation["allocated_bytes"] == seal.get("task_output_allocated_bytes")
          and allocation["allocated_bytes"] <= protocol["capacity_probe"]["owned_output_cap_bytes"],
          "retained full capacity allocation changed")
    captured_roots = {entry.get("path") for entry in allocation.get("roots", [])}
    _need(str(target.parent) in captured_roots and captured_roots <= set(roots), "retained full capacity allocation scope changed")
    peak = max(allocation["allocated_bytes"], _full_capacity_cumulative_bounds(protocol, raw, captures, qualified, target))
    _need(type(seal.get("observed_peak_bytes")) is int and seal["observed_peak_bytes"] == peak
          and (stopped or peak < protocol["capacity_probe"]["owned_output_cap_bytes"]), "retained full capacity peak changed")
    floor = min(raw["minimum_disk_free_bytes"], *[entry["proof"]["capture"]["minimum_disk_free_bytes"] for entry in qualified.values()],
                *[sample["disk_free_bytes"] for capture in captures for sample in capture["resource_samples"]])
    _need(seal.get("minimum_disk_free_bytes") == floor, "retained full capacity minimum free changed")
    if target.exists():
        live = _module("inventory").inventory([], store_roots=[Path(root) for root in roots if Path(root).exists()],
                                              discover=False, minimum_free_bytes=protocol["budget_policy"]["minimum_free_bytes"])
        _need(live.get("complete") is True and live.get("errors") == [] and type(live.get("allocated_bytes")) is int
              and live["allocated_bytes"] <= peak, "full capacity publication allocation exceeds recorded conservative peak")


def verify_full_capacity_link(reference: dict, protocol: dict) -> dict:
    """Capacity links never enter verify_retained_arm or performance samples."""
    _, link = _proof(reference)
    _need(set(link) == {"seal", "disposition"}, "full capacity link must bind seal and disposition")
    seal = _verify_seal(link["seal"], protocol)
    _need(seal.get("kind") == "build-feedback-full-capacity-seal" and protocol.get("phase") == "capacity",
          "full capacity seal required")
    disposition = _verify_disposition(link["disposition"], link["seal"], seal, protocol)
    return {"arm_receipt": seal["arm_receipt"], "seal": seal, "disposition": disposition}


def create_fixture_seal(protocol: dict, target: Path, lease: dict, seal_path: Path) -> dict:
    """Seal only the literal tiny native-lifecycle fixture, never Cargo evidence."""
    policy, approval = validate_policy(protocol)
    _need(approval.get("purpose") == "synthetic-recycle-lifecycle" and protocol.get("phase") == "capacity",
          "independent synthetic lifecycle purpose required")
    target = _safe_target(protocol, target, lease)
    _need(not _inside(seal_path.resolve(), target), "seal must survive disposal")
    manifest = tree_manifest(target)
    _need([entry["relative"] for entry in manifest["entries"]] == [".", "fixture.bin"]
          and manifest["entries"][0]["directory"] is True and manifest["entries"][1]["directory"] is False
          and manifest["entries"][1]["bytes"] == len(FIXTURE_BYTES)
          and manifest["entries"][1]["sha256"] == hashlib.sha256(FIXTURE_BYTES).hexdigest()
          and 0 < manifest["allocated_bytes"] <= 1024 ** 2, "only the literal tiny recycle fixture is eligible")
    payload = {"kind": "build-feedback-recycle-fixture-seal", "schema_version": 1, "phase": "capacity", "target": str(target),
               "protocol_sha256": _canonical(protocol), "approval": policy["approval"], "checker": policy["checker"],
               "performance_evidence": False, "lease": lease, "tree": manifest,
               "retained_files": _retained_refs([policy["approval"], lease], target), "sealed_at": _stamp(), "trust_limit": TRUST_LIMIT}
    payload["closure_sha256"] = _canonical(payload)
    return _new(seal_path, _signed(payload, protocol))


def _integrity(value: dict, protocol: dict) -> str:
    _, approval = validate_policy(protocol)
    key = Path(approval["local_integrity_key"]["path"]).read_bytes()
    _need(len(key) == 32, "local integrity key length changed")
    data = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode()
    return hmac.new(key, data, hashlib.sha256).hexdigest()


def _signed(value: dict, protocol: dict) -> dict:
    return {**value, "local_integrity_hmac": _integrity(value, protocol)}


def _verify_signed(value: dict, protocol: dict) -> None:
    unsigned = {key: item for key, item in value.items() if key != "local_integrity_hmac"}
    actual = value.get("local_integrity_hmac")
    _need(isinstance(actual, str) and hmac.compare_digest(actual, _integrity(unsigned, protocol)), "local observation integrity changed")


def _verify_seal(reference: Any, protocol: dict) -> dict:
    policy, approval = validate_policy(protocol)
    _, seal = _proof(reference)
    kinds = {"build-feedback-artifact-seal", "build-feedback-capacity-seal", "build-feedback-full-capacity-seal"}
    if approval.get("purpose") == "synthetic-recycle-lifecycle":
        kinds = {"build-feedback-recycle-fixture-seal"}
        _need(protocol.get("phase") == "capacity" and seal.get("performance_evidence") is False,
              "synthetic fixture cannot supply performance evidence")
    _need(seal.get("kind") in kinds and type(seal.get("schema_version")) is int and seal["schema_version"] == 1
          and seal.get("phase") == protocol.get("phase"), "invalid seal version/phase")
    _need(seal.get("protocol_sha256") == _canonical(protocol) and seal.get("approval") == policy["approval"]
          and seal.get("checker") == policy["checker"] and seal.get("trust_limit") == TRUST_LIMIT, "seal approval changed")
    _verify_signed(seal, protocol)
    unsigned = {key: value for key, value in seal.items() if key not in {"closure_sha256", "local_integrity_hmac"}}
    _need(seal.get("closure_sha256") == _canonical(unsigned), "seal closure changed")
    _safe_target(protocol, Path(seal["target"]), seal["lease"])
    _need(isinstance(seal.get("retained_files"), list) and bool(seal["retained_files"]), "seal retained proofs missing")
    _need(len({ref["path"] for ref in seal["retained_files"]}) == len(seal["retained_files"]), "duplicate retained proof")
    for ref in seal["retained_files"]:
        _required_reference(ref, Path(seal["target"]))
    if seal["kind"] == "build-feedback-full-capacity-seal":
        _verify_full_capacity_closure(seal, protocol)
    return seal


# Minimal supported COM bindings; no filesystem deletion fallback exists.
class _GUID(ctypes.Structure):
    _fields_ = [("data", ctypes.c_ubyte * 16)]
    def __init__(self, value: str):
        super().__init__((ctypes.c_ubyte * 16).from_buffer_copy(uuid.UUID(value).bytes_le))


def _call(pointer, index: int, args: tuple, *values, result=ctypes.c_long):
    vtable = ctypes.cast(pointer, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p))).contents
    function = ctypes.WINFUNCTYPE(result, ctypes.c_void_p, *args)(vtable[index])
    return function(pointer, *values)


def _hr(value: int) -> None:
    if value < 0:
        raise OSError("Windows Shell HRESULT 0x%08x" % (value & 0xffffffff))


class WindowsRecycleBin:
    """Single-item IFileOperation with vetoable callbacks and shell enumeration."""
    def __init__(self):
        _need(os.name == "nt", "Windows recycle operation required")
        self.ole = ctypes.WinDLL("ole32")
        self.shell = ctypes.WinDLL("shell32")
        self.ole.CoInitializeEx.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        self.ole.CoInitializeEx.restype = ctypes.c_long
        _hr(self.ole.CoInitializeEx(None, 2))
        self.ole.CoCreateInstance.argtypes = [ctypes.POINTER(_GUID), ctypes.c_void_p, ctypes.c_uint32, ctypes.POINTER(_GUID), ctypes.POINTER(ctypes.c_void_p)]
        self.ole.CoCreateInstance.restype = ctypes.c_long
        self.ole.CoTaskMemFree.argtypes = [ctypes.c_void_p]
        self.shell.SHCreateItemFromParsingName.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p, ctypes.POINTER(_GUID), ctypes.POINTER(ctypes.c_void_p)]
        self.shell.SHCreateItemFromParsingName.restype = ctypes.c_long
        self.shell.SHGetKnownFolderItem.argtypes = [ctypes.POINTER(_GUID), ctypes.c_uint32, ctypes.c_void_p, ctypes.POINTER(_GUID), ctypes.POINTER(ctypes.c_void_p)]
        self.shell.SHGetKnownFolderItem.restype = ctypes.c_long
        self.iid_item = _GUID("43826d1e-e718-42ee-bc55-a1e261c37bfe")

    def close(self):
        self.ole.CoUninitialize()

    def _item(self, name: str):
        pointer = ctypes.c_void_p()
        _hr(self.shell.SHCreateItemFromParsingName(name, None, ctypes.byref(self.iid_item), ctypes.byref(pointer)))
        return pointer

    def _name(self, pointer, mode: int) -> str:
        value = ctypes.c_void_p()
        _hr(_call(pointer, 5, (ctypes.c_uint32, ctypes.POINTER(ctypes.c_void_p)), mode, ctypes.byref(value)))
        try:
            return ctypes.wstring_at(value)
        finally:
            self.ole.CoTaskMemFree(value)

    def items(self) -> list[dict]:
        root, iterator = ctypes.c_void_p(), ctypes.c_void_p()
        _hr(self.shell.SHGetKnownFolderItem(ctypes.byref(_GUID("b7534046-3ecb-4c18-be4e-64cd4cb7d6ac")), 0, None, ctypes.byref(self.iid_item), ctypes.byref(root)))
        result = []
        try:
            _hr(_call(root, 3, (ctypes.c_void_p, ctypes.POINTER(_GUID), ctypes.POINTER(_GUID), ctypes.POINTER(ctypes.c_void_p)), None,
                      ctypes.byref(_GUID("94f60519-2850-4924-aa5a-d15e84868039")), ctypes.byref(_GUID("70629033-e363-4a28-a567-0db78006e6d7")), ctypes.byref(iterator)))
            while True:
                item, fetched = ctypes.c_void_p(), ctypes.c_uint32()
                hr = _call(iterator, 3, (ctypes.c_uint32, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint32)), 1, ctypes.byref(item), ctypes.byref(fetched))
                _hr(hr)
                if not fetched.value:
                    break
                try:
                    physical = self._name(item, 0x80058000)
                    metadata = _module("inventory")._new_metadata_reader().read(Path(physical), include_streams=False)
                    result.append({"parsing_name": self._name(item, 0x80028000), "path": physical,
                                   "display_name": self._name(item, 0), "identity": list(metadata.identity)})
                finally:
                    _call(item, 2, (), result=ctypes.c_ulong)
        finally:
            if iterator.value:
                _call(iterator, 2, (), result=ctypes.c_ulong)
            _call(root, 2, (), result=ctypes.c_ulong)
        return sorted(result, key=lambda entry: entry["parsing_name"])

    def capacity(self, target: Path, tree: dict) -> dict:
        """Refuse unknown quota or potential eviction of unrelated recycle items."""
        import winreg
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.GetVolumeNameForVolumeMountPointW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32]
        kernel.GetVolumeNameForVolumeMountPointW.restype = ctypes.c_int
        volume = ctypes.create_unicode_buffer(261)
        _need(bool(kernel.GetVolumeNameForVolumeMountPointW(str(target.anchor), volume, len(volume))), "recycle volume identity unavailable")
        key_name = volume.value.split("{")[-1].split("}")[0]
        registry_path = r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume\{" + key_name + "}"
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, registry_path) as key:
            cap, cap_type = winreg.QueryValueEx(key, "MaxCapacity")
            nuke, nuke_type = winreg.QueryValueEx(key, "NukeOnDelete")
        _need(cap_type == winreg.REG_DWORD and type(cap) is int and cap > 0 and nuke_type == winreg.REG_DWORD and nuke == 0,
              "recycling disabled or quota unknown")
        class Info(ctypes.Structure):
            _fields_ = [("size", ctypes.c_uint32), ("bytes", ctypes.c_int64), ("items", ctypes.c_int64)]
        info = Info()
        info.size = ctypes.sizeof(info)
        self.shell.SHQueryRecycleBinW.argtypes = [ctypes.c_wchar_p, ctypes.POINTER(Info)]
        self.shell.SHQueryRecycleBinW.restype = ctypes.c_long
        _hr(self.shell.SHQueryRecycleBinW(str(target.anchor), ctypes.byref(info)))
        logical = sum(entry["bytes"] for entry in tree["entries"] if not entry["directory"])
        required = max(logical, tree["allocated_bytes"]) + 8 * 1024 * 1024
        _need(info.bytes >= 0 and info.items >= 0 and info.bytes + required < cap * 1024 * 1024,
              "recycle quota may evict unrelated items")
        return {"quota_bytes": cap * 1024 * 1024, "existing_logical_bytes": info.bytes,
                "required_bytes_with_margin": required, "volume": volume.value, "no_eviction_admitted": True}

    def _operate(self, parsing_name: str, *, recycle: bool, expected_path: str,
                 expected_alias: str | None = None, expected_tree: dict | None = None) -> dict:
        operation = ctypes.c_void_p()
        _hr(self.ole.CoCreateInstance(ctypes.byref(_GUID("3ad05575-8857-4850-9277-11b85bdb8e09")), None, 1,
                                      ctypes.byref(_GUID("947aab5f-0a5c-4c13-b4d6-4bf7836fc9f8")), ctypes.byref(operation)))
        item = self._item(parsing_name)
        callbacks: list[Any] = []
        state: dict[str, list[Any]] = {"pre": [], "post": [], "errors": []}
        seen_relatives: dict[str, tuple[int, int]] = {}
        iid_sink = uuid.UUID("04b0f1a7-9490-44bc-96e1-4296a31252e2").bytes_le
        iid_unknown = uuid.UUID("00000000-0000-0000-c000-000000000046").bytes_le
        def callback(args, handler):
            def safe(*values):
                try:
                    return handler(*values)
                except BaseException as error:
                    # An exception must never unwind through a native callback:
                    # ctypes would otherwise return an undefined HRESULT.
                    _LOGGER.exception("Windows Shell callback failed; operation cancelled")
                    state["errors"].append(type(error).__name__ + ": " + str(error))
                    return -2147467260  # E_ABORT; do not continue after unknown effects.
            function = ctypes.WINFUNCTYPE(ctypes.c_long, ctypes.c_void_p, *args)(safe)
            callbacks.append(function)
            return ctypes.cast(function, ctypes.c_void_p).value
        def query(this, iid, out):
            out[0] = None
            if ctypes.string_at(iid, 16) not in {iid_sink, iid_unknown}:
                return -2147467262
            out[0] = this
            return 0
        def pre(this, flags, source):
            physical = self._name(source, 0x80058000)
            selected_parsing_name = self._name(source, 0x80028000)
            roots = [Path(expected_path)]
            if expected_alias is not None:
                roots.append(Path(expected_alias))

            def relative(path: Path) -> str | None:
                for root in roots:
                    path_key, root_key = os.path.normcase(str(path)), os.path.normcase(str(root))
                    if path_key == root_key:
                        return "."
                    prefix = root_key.rstrip("\\/") + os.sep
                    if path_key.startswith(prefix):
                        return path.relative_to(root).as_posix()
                return None

            physical_relative = relative(Path(physical))
            parsing_relative = relative(Path(selected_parsing_name))
            _need(physical_relative is not None and physical_relative == parsing_relative,
                  f"Shell selected unexpected item: physical={physical!r} "
                  f"parsing={selected_parsing_name!r} expected={[str(root) for root in roots]!r}")
            if expected_tree is not None:
                observed_path = Path(selected_parsing_name)
                if not observed_path.exists():
                    # For a recycle-bin item IFileOperation's display path is
                    # the deleted original, while the parsing name remains the
                    # exact $R storage entry. The outer operation was created
                    # from that entry, so reverify its tree through the pinned
                    # storage path when the callback exposes only the display.
                    observed_path = Path(expected_path)
                matches = [entry for entry in expected_tree["entries"]
                           if entry.get("relative") == parsing_relative]
                _need(len(matches) == 1,
                      "Shell source entry is outside or duplicated in sealed tree")
                expected_entry = matches[0]
                meta = _module("inventory")._new_metadata_reader().read(observed_path)
                _need(list(meta.identity) == expected_entry.get("identity")
                      and meta.directory is expected_entry.get("directory")
                      and meta.allocated_bytes == expected_entry.get("allocated_bytes")
                      and meta.logical_bytes == expected_entry.get("bytes")
                      and meta.last_write == expected_entry.get("modified"),
                      "Shell source entry identity/tree changed before delete")
                if not meta.directory:
                    _need(sha256_file(observed_path) == expected_entry.get("sha256"),
                          "Shell source file changed before delete")
                prior = seen_relatives.get(parsing_relative)
                _need(prior is None or prior == meta.identity,
                      "Shell source entry was repeated with a different identity")
                seen_relatives[parsing_relative] = meta.identity
            _need(bool(flags & 0x80) == recycle, "Shell recycle/permanent intent mismatch")
            state["pre"].append({"path": physical, "parsing_name": selected_parsing_name,
                                 "relative": parsing_relative, "flags": flags})
            return 0
        def post(this, flags, source, status, created):
            _hr(status)
            if not recycle:
                _need(not created, "Shell bypassed expected permanent disposition")
            state["post"].append({"flags": flags, "hresult": status,
                                  "created_parsing_name": self._name(created, 0x80028000) if created else None})
            return 0
        vtable = (ctypes.c_void_p * 19)(
            callback((ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)), query),
            callback((), lambda this: 1), callback((), lambda this: 1),
            callback((), lambda this: 0), callback((ctypes.c_long,), lambda *args: 0),
            callback((ctypes.c_uint32, ctypes.c_void_p, ctypes.c_wchar_p), lambda *args: 0),
            callback((ctypes.c_uint32, ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_long, ctypes.c_void_p), lambda *args: 0),
            callback((ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_wchar_p), lambda *args: 0),
            callback((ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_long, ctypes.c_void_p), lambda *args: 0),
            callback((ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_wchar_p), lambda *args: 0),
            callback((ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_long, ctypes.c_void_p), lambda *args: 0),
            callback((ctypes.c_uint32, ctypes.c_void_p), pre),
            callback((ctypes.c_uint32, ctypes.c_void_p, ctypes.c_long, ctypes.c_void_p), post),
            callback((ctypes.c_uint32, ctypes.c_void_p, ctypes.c_wchar_p), lambda *args: 0),
            callback((ctypes.c_uint32, ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_long, ctypes.c_void_p), lambda *args: 0),
            callback((ctypes.c_uint32, ctypes.c_uint32), lambda *args: 0),
            callback((), lambda *args: 0), callback((), lambda *args: 0), callback((), lambda *args: 0))
        sink = (ctypes.POINTER(ctypes.c_void_p) * 1)(ctypes.cast(vtable, ctypes.POINTER(ctypes.c_void_p)))
        try:
            flags = 0x0004 | 0x0010 | 0x0400 | 0x00100000
            if recycle:
                flags |= 0x00080000 | 0x20000000 | 0x4000
            _hr(_call(operation, 5, (ctypes.c_uint32,), flags))
            _hr(_call(operation, 18, (ctypes.c_void_p, ctypes.c_void_p), item, ctypes.cast(sink, ctypes.c_void_p)))
            performed = _call(operation, 21, ())
            aborted = ctypes.c_int()
            _hr(_call(operation, 22, (ctypes.POINTER(ctypes.c_int),), ctypes.byref(aborted)))
            _hr(performed)
            _need(not aborted.value and not state["errors"] and bool(state["pre"]) and bool(state["post"]),
                  "Shell operation aborted/incomplete:" + str(state))
            if recycle and not any(entry.get("created_parsing_name") for entry in state["post"]):
                message = "Shell bypassed expected recycling disposition:" + str(state)
                _LOGGER.error(message)
                raise OSError(message)
            return state
        finally:
            _call(item, 2, (), result=ctypes.c_ulong)
            _call(operation, 2, (), result=ctypes.c_ulong)

    def recycle(self, target: Path, *, expected_tree: dict | None = None) -> dict:
        return self._operate(str(target), recycle=True, expected_path=str(target),
                             expected_alias=str(target), expected_tree=expected_tree)

    def purge(self, item: dict, *, expected_tree: dict | None = None) -> dict:
        return self._operate(item["parsing_name"], recycle=False, expected_path=item["path"],
                             expected_alias=item["display_name"], expected_tree=expected_tree)


def _unrelated(before: list[dict], after: list[dict], ignored: str | None = None) -> None:
    mapping = {item["parsing_name"]: item for item in after}
    _need(all(mapping.get(item["parsing_name"]) == item for item in before if item["parsing_name"] != ignored), "unrelated recycle item changed")


def dispose_output(seal_reference: dict, protocol: dict, disposition_root: Path, *, backend=None) -> dict:
    """Stage files reconcile unknown mutations before any retry."""
    seal = _verify_seal(seal_reference, protocol)
    target = Path(seal["target"])
    _need(_path(str(disposition_root), exists=False) == target.parent / "disposition", "disposition requires exact arm-owned directory")
    _need(not _inside(disposition_root.resolve(), target), "disposition must survive output")
    _need(not _module("runner").build_process_snapshot(), "quiet disposal required")
    disposition_root.mkdir(parents=True, exist_ok=True)
    owned_backend = backend is None
    shell = WindowsRecycleBin() if owned_backend else backend
    reader = _module("inventory")._new_metadata_reader()
    def save(name: str, value: dict) -> dict:
        return _new(disposition_root / (name + ".json"), _signed(value, protocol))
    def stage(name: str) -> dict | None:
        path = disposition_root / (name + ".json")
        if not path.exists():
            return None
        value = _read(path)
        _verify_signed(value, protocol)
        return value
    try:
        final = stage("completed")
        if final is not None:
            _need(final.get("seal") == seal_reference and final.get("status") == "complete" and not target.exists(), "completed disposition mismatch")
            _need(not any(item["identity"] == seal["tree"]["root_identity"] for item in shell.items()), "purged identity reappeared")
            reference = {"path": str(disposition_root / "completed.json"), "sha256": sha256_file(disposition_root / "completed.json")}
            _verify_disposition(reference, seal_reference, seal, protocol)
            return reference
        intent = stage("move-intent")
        if intent is None:
            _need(_same_tree(seal["tree"], tree_manifest(target)), "output changed after seal")
            intent = {"kind": "build-feedback-disposition-intent", "schema_version": 1, "seal": seal_reference,
                      "target": str(target), "tree_sha256": _canonical(seal["tree"]), "bin_before": shell.items(),
                      "capacity": shell.capacity(target, seal["tree"]), "at": _stamp()}
            save("move-intent", intent)
        _need(intent.get("seal") == seal_reference and intent.get("target") == str(target)
              and intent.get("tree_sha256") == _canonical(seal["tree"]), "disposition intent mismatch")
        moved = stage("moved")
        if moved is None:
            operation = None
            if target.exists():
                _need(_same_tree(seal["tree"], tree_manifest(target)), "output changed before recycle")
                shell.capacity(target, seal["tree"])
                _need(not _module("runner").build_process_snapshot(), "quiet recycle required")
                operation = shell.recycle(target, expected_tree=seal["tree"])
            current = shell.items()
            _unrelated(intent["bin_before"], current)
            matches = [item for item in current if item["identity"] == seal["tree"]["root_identity"]
                       and item["parsing_name"] not in {old["parsing_name"] for old in intent["bin_before"]}]
            _need(not target.exists() and len(matches) == 1, "exact recycle identity not found; stop")
            item = matches[0]
            _need(_same_tree(seal["tree"], tree_manifest(Path(item["path"]))), "recycled output tree mismatch")
            moved = {"seal": seal_reference, "item": item, "operation": operation, "reconciled": operation is None, "at": _stamp()}
            save("moved", moved)
        _need(moved.get("seal") == seal_reference and isinstance(moved.get("item"), dict)
              and moved["item"].get("identity") == seal["tree"]["root_identity"], "moved disposition seal/item mismatch")
        current = shell.items()
        _unrelated(intent["bin_before"], current)
        matches = [item for item in current if item == moved["item"]]
        purge_intent = stage("purge-intent")
        if purge_intent is None:
            _need(len(matches) == 1, "verified recycled item missing before purge")
            _need(_same_tree(seal["tree"], tree_manifest(Path(matches[0]["path"]))), "recycled output changed before purge")
            purge_intent = {"seal": seal_reference, "item": matches[0], "bin_before": current,
                            "free_before_bytes": reader.free_bytes(), "at": _stamp()}
            save("purge-intent", purge_intent)
        _need(purge_intent.get("seal") == seal_reference and purge_intent.get("item") == moved["item"], "purge intent changed")
        operation = None
        # No new proof file is written between this observation and mutation.
        # An interrupted purge lacks a durable immediate observation and must
        # remain inconclusive rather than borrowing the earlier intent value.
        purge_free_before = None
        purge_free_before_at = None
        if matches:
            _need(len(matches) == 1 and _same_tree(seal["tree"], tree_manifest(Path(matches[0]["path"]))), "purge item changed")
            _need(not _module("runner").build_process_snapshot(), "quiet purge required")
            purge_free_before = reader.free_bytes()
            purge_free_before_at = _stamp()
            operation = shell.purge(matches[0], expected_tree=seal["tree"])
        after = shell.items()
        _unrelated(purge_intent["bin_before"], after, moved["item"]["parsing_name"])
        _need(not target.exists() and not any(item["identity"] == seal["tree"]["root_identity"] for item in after), "approved item still exists after purge")
        free_after = reader.free_bytes()
        released = None if purge_free_before is None else free_after - purge_free_before
        release_verified = released is not None and released >= seal["tree"]["allocated_bytes"]
        payload = {"kind": "build-feedback-disposition", "schema_version": 1, "seal": seal_reference,
                   "protocol_sha256": _canonical(protocol), "target": str(target), "item": moved["item"],
                   "move_intent": {"path": str(disposition_root / "move-intent.json"), "sha256": sha256_file(disposition_root / "move-intent.json")},
                   "moved": {"path": str(disposition_root / "moved.json"), "sha256": sha256_file(disposition_root / "moved.json")},
                   "purge_intent": {"path": str(disposition_root / "purge-intent.json"), "sha256": sha256_file(disposition_root / "purge-intent.json")},
                   "operation": operation, "reconciled": operation is None, "source_absent": True, "bin_absent": True,
                   "allocated_bytes": seal["tree"]["allocated_bytes"], "free_after_bytes": free_after,
                   "free_before_purge_bytes": purge_free_before, "free_before_purge_at": purge_free_before_at,
                   "prepared_free_before_bytes": purge_intent["free_before_bytes"],
                   "observed_free_delta_bytes": released, "release_verified": release_verified,
                   "status": "complete" if release_verified else "release_inconclusive", "at": _stamp()}
        reference = save("completed", payload)
        _need(payload["status"] == "complete", "disposal complete but disk release is inconclusive; do not retry deletion")
        _verify_disposition(reference, seal_reference, seal, protocol)
        return reference
    finally:
        if owned_backend:
            shell.close()


def _verify_disposition(reference: Any, seal_reference: dict, seal: dict, protocol: dict) -> dict:
    path, result = _proof(reference)
    root = Path(seal["target"]).parent / "disposition"
    _need(path == root / "completed.json" and result.get("kind") == "build-feedback-disposition"
          and result.get("schema_version") == 1 and result.get("status") == "complete"
          and result.get("release_verified") is True and result.get("source_absent") is True
          and result.get("bin_absent") is True and result.get("seal") == seal_reference
          and result.get("target") == seal["target"] and result.get("protocol_sha256") == _canonical(protocol),
          "sealed arm disposition incomplete")
    _verify_signed(result, protocol)
    stages = {}
    for key, filename in (("move_intent", "move-intent"), ("moved", "moved"), ("purge_intent", "purge-intent")):
        stage_path, value = _proof(result.get(key))
        _verify_signed(value, protocol)
        _need(stage_path == root / (filename + ".json") and value.get("seal") == seal_reference,
              "disposition stage seal/path mismatch")
        stages[key] = value
    intent, moved, purge = (stages[key] for key in ("move_intent", "moved", "purge_intent"))
    item = result.get("item")
    _need(isinstance(item, dict) and item.get("identity") == seal["tree"]["root_identity"]
          and moved.get("item") == item and purge.get("item") == item
          and intent.get("target") == seal["target"] and intent.get("tree_sha256") == _canonical(seal["tree"]),
          "disposition target/item/tree mismatch")
    if "free_before_purge_bytes" in result:
        before = result.get("free_before_purge_bytes")
        _need(type(result.get("prepared_free_before_bytes")) is int
              and result["prepared_free_before_bytes"] == purge.get("free_before_bytes"), "disposition preparation baseline changed")
        observed_at = _module("statistics")._utc(result.get("free_before_purge_at"))
        _need(_module("statistics")._utc(purge["at"]) <= observed_at <= _module("statistics")._utc(result["at"]),
              "disposition immediate baseline order mismatch")
    else:
        # Compatibility for sealed prior receipts, never a new purge baseline.
        before = purge.get("free_before_bytes")
    after, delta, allocated = (result.get(key) for key in ("free_after_bytes", "observed_free_delta_bytes", "allocated_bytes"))
    if not all(type(value) is int and value >= 0 for value in (before, after, delta, allocated)):
        raise ValueError("disposition release accounting mismatch")
    if not isinstance(before, int) or not isinstance(after, int):
        raise TypeError("disposition free-space types changed")
    _need(allocated == seal["tree"]["allocated_bytes"] and delta == after - before and delta >= allocated,
          "disposition release accounting mismatch")
    _need(isinstance(intent.get("bin_before"), list) and isinstance(purge.get("bin_before"), list), "disposition bin snapshot missing")
    _unrelated(intent["bin_before"], purge["bin_before"])
    _need(sum(entry == item for entry in purge["bin_before"]) == 1, "approved item missing from purge intent")
    timestamps = [_module("statistics")._utc(value["at"]) for value in (intent, moved, purge, result)]
    _need(timestamps == sorted(timestamps), "disposition stage order mismatch")
    _need(not Path(seal["target"]).exists(), "sealed recycled target reappeared")
    return result


def verify_retained_arm(reference: dict, protocol: dict, arm: str, pair_index: int, attempt: int) -> dict:
    _, link = _proof(reference)
    seal = _verify_seal(link.get("seal"), protocol)
    _need(seal.get("kind") == "build-feedback-artifact-seal" and protocol.get("phase") in {"pilot", "formal"}, "capacity seal cannot qualify")
    _need((seal.get("arm"), seal.get("pair_index"), seal.get("attempt")) == (arm, pair_index, attempt), "sealed arm allocation mismatch")
    disposition = _verify_disposition(link.get("disposition"), link["seal"], seal, protocol)
    _need(_proof(seal["arm_reference"])[1] == seal["arm_receipt"], "retained arm receipt changed")
    return {"arm_receipt": seal["arm_receipt"], "seal": seal, "disposition": disposition}
