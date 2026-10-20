"""Windows-only, fail-closed build experiment runner (Python 3.10+, stdlib).

Public entry points: JOBS, make_protocol(configuration=None, **fields),
preview_protocol(protocol, inventory_report), run_protocol(protocol_path,
review_receipt_path=None, pair_limit=20, arm_finalizer=None),
capacity_command(protocol,target), capture_qualification(protocol,arm_record,name),
and parse_cargo_json_file(path,target,source).
V2 binds40GiB free floor plus8GiB external allowance and explicit reservations;
V1 retains80GiB/fixed100. Capacity is a declared leaf or full arm with qualifications, never paired data. No command creates a Git worktree,
installs a tool, deletes an output, or changes a shared Cargo configuration.

make_protocol takes explicit source_root/source_commit/source_tree,
source_receipt_path, baseline_receipt_path, run_root, toolchain, inventory_roots,
predicted_growth_bytes, baseline_config/candidate_config and allocations.
Toolchain identities contain cargo/rustc and msvc.compiler/msvc.linker objects
with absolute path, sha256 and full version. Allocations are pre-created frozen
worktrees: {pair_index, attempt, baseline_source_root, candidate_source_root}.
Every attempted arm needs its own worktree; polluted attempts are never reused.
All returned paths and raw diagnostics are machine-local evidence.
"""
from __future__ import annotations

import ctypes
import hashlib
import importlib.util
import io
import json
import math
import os
import random
import socket
import subprocess
import sys
import time
import uuid
from collections.abc import Iterable, Mapping
from ctypes import wintypes
from datetime import datetime, timezone
from functools import lru_cache
from pathlib import Path
from typing import Any

GIB = 1024 ** 3
MEMORY_LIMIT_BYTES = 24 * GIB
BOUNDED_MINIMUM_FREE_BYTES = 40 * GIB
EXTERNAL_GROWTH_ALLOWANCE_BYTES = 8 * GIB
INITIAL_CAPACITY_OUTPUT_CAP_BYTES = 8 * GIB
QUALIFICATION_NAMES = ("clippy", "diagnostics", "optional_features", "release")
JOBS = (
    {"name": "check", "args": ("check", "--workspace", "--all-targets"), "lane": "check-clippy"},
    {"name": "domain-ports", "args": ("test", "-p", "agent-session-grep-domain", "-p", "agent-session-grep-ports", "--all-targets"), "lane": "domain-cli"},
    {"name": "cli", "args": ("test", "-p", "agent-session-grep-cli", "--all-targets"), "lane": "domain-cli"},
    {"name": "sqlite", "args": ("test", "-p", "agent-session-grep-adapters-sqlite", "--all-targets"), "lane": "sqlite-application"},
    {"name": "application", "args": ("test", "-p", "agent-session-grep-application", "--all-targets"), "lane": "sqlite-application"},
    {"name": "clippy", "args": ("clippy", "--workspace", "--all-targets"), "lane": "check-clippy"},
)
EDIT_SUFFIX = b"\n// build-feedback deterministic edit v1\n"
DEFAULT_EDIT_PATH = "crates/agent-session-grep-domain/src/lib.rs"
BUILD_NAMES = frozenset({"cargo.exe", "rustc.exe", "rustdoc.exe", "cargo-clippy.exe", "clippy-driver.exe", "cl.exe", "link.exe", "lld-link.exe", "sccache.exe", "mbx.exe", "ccache.exe", "rust-lld.exe", "cmake.exe", "ninja.exe", "nmake.exe", "msbuild.exe", "cvtres.exe", "rc.exe", "agent-session-grep.exe", "asg.exe", "asg-supervisor.exe"})


class ProtocolError(ValueError):
    """A missing proof is an admission failure, never an inferred default."""


def _require(condition: bool, reason: str) -> None:
    if not condition:
        raise ProtocolError(reason)


def _stamp() -> str:
    return datetime.now(timezone.utc).isoformat()


def canonical_sha256(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode("utf-8")).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _digest(value: Any, label: str, length: int = 64) -> str:
    _require(isinstance(value, str) and len(value) == length and all(c in "0123456789abcdef" for c in value), f"invalid_{label}")
    return value


def _count(value: Any, label: str, positive: bool = False) -> int:
    _require(type(value) is int and value >= (1 if positive else 0), f"invalid_{label}")
    return value


def _read_json(path: Path) -> dict:
    def reject_constant(value: str):
        raise ProtocolError(f"nonfinite_json:{value}")
    def unique_keys(pairs):
        result = {}
        for key, item in pairs:
            _require(key not in result, "duplicate_json_key:" + key)
            result[key] = item
        return result
    value = json.loads(path.read_text(encoding="utf-8"), parse_constant=reject_constant, object_pairs_hook=unique_keys)
    _require(isinstance(value, dict), "json_object_required")
    return value


def _read_small_capture(path: Path, limit_bytes: int = 1024 * 1024) -> bytes:
    """Version/banner capture only; oversized tool output fails with logs kept."""
    with path.open("rb") as handle:
        data = handle.read(limit_bytes + 1)
    _require(len(data) <= limit_bytes, "toolchain_banner_output_too_large")
    return data


def _write_new(path: Path, value: dict) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(value, handle, indent=2, ensure_ascii=False, allow_nan=False)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())


def _write_state(path: Path, value: dict) -> None:
    """Replace only this campaign's known state; immutable raw receipts stay put."""
    sealed = dict(value)
    sealed["integrity_sha256"] = canonical_sha256({k: v for k, v in value.items() if k != "integrity_sha256"})
    temporary = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    _write_new(temporary, sealed)
    os.replace(temporary, path)
    value.clear()
    value.update(sealed)


def _path(value: Any, label: str, *, exists: bool = True) -> Path:
    _require(isinstance(value, (str, os.PathLike)) and bool(str(value)), f"invalid_{label}")
    path = Path(value)
    _require(path.is_absolute(), f"absolute_{label}_required")
    resolved = path.resolve(strict=exists)
    return resolved


def _inside(path: Path, root: Path) -> bool:
    return path == root or root in path.parents


def _separate(paths: list[Path], label: str) -> None:
    for index, path in enumerate(paths):
        for other in paths[:index]:
            _require(not _inside(path, other) and not _inside(other, path), f"overlapping_{label}")


@lru_cache(maxsize=1)
def _inventory_module():
    path = Path(__file__).with_name("build_feedback_inventory.py")
    spec = importlib.util.spec_from_file_location("build_feedback_inventory_runner_dependency", path)
    _require(spec is not None and spec.loader is not None, "inventory_module_unavailable")
    module = importlib.util.module_from_spec(spec)
    # Some stdlib dataclass implementations need the defining module registered.
    import sys
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _manifest(receipt: dict) -> list[dict]:
    entries = receipt.get("source_identity", {}).get("files", receipt.get("files"))
    _require(isinstance(entries, list) and len(entries) > 0, "source_manifest_required")
    result = []
    seen = set()
    for entry in entries:
        _require(isinstance(entry, dict), "invalid_source_manifest")
        name = entry.get("path")
        _require(isinstance(name, str) and bool(name) and "\\" not in name and ":" not in name, "invalid_manifest_path")
        relative = Path(name)
        _require(not relative.is_absolute() and all(part not in ("", ".", "..") for part in name.split("/")), "unsafe_manifest_path")
        _require(name.casefold() not in seen and ".git" not in relative.parts and "target" not in relative.parts, "duplicate_or_output_manifest_path")
        seen.add(name.casefold())
        result.append({"path": name, "sha256": _digest(entry.get("sha256"), "source_hash"), "bytes": _count(entry.get("bytes"), "source_size")})
    return sorted(result, key=lambda entry: entry["path"])


def _git(root: Path, *args: str) -> str:
    environment = dict(os.environ, GIT_OPTIONAL_LOCKS="0")
    completed = subprocess.run(["git", "-C", str(root), *args], capture_output=True, timeout=30, env=environment, check=False)
    _require(completed.returncode == 0, "source_git_identity_unavailable")
    return completed.stdout.decode("utf-8", "strict").strip()


def _coordinator_output_path(path: Path, coordinator: Path) -> bool:
    return any(_inside(path, coordinator / relative) for relative in (Path(".trellis/.runtime"), Path("evidence-output")))


def _dedicated_source(root: Path, live: Path, coordinator: Path) -> None:
    _require(root != live and not _inside(root, live) and not _inside(live, root), "source_roots_overlap_live")
    _require(root != coordinator and not _inside(coordinator, root), "source_roots_overlap_coordinator")
    if _inside(root, coordinator):
        _require(_coordinator_output_path(root, coordinator), "source_must_use_ignored_coordinator_runtime")
    actual = _path(_git(root, "rev-parse", "--show-toplevel"), "dedicated_git_worktree")
    _require(actual == root, "dedicated_git_worktree_required")


def _verify_source(root: Path, protocol: dict, *, edited: bool = False) -> None:
    source = protocol["source"]
    receipt_path = _path(source["receipt_path"], "source_receipt")
    _require(sha256_file(receipt_path) == source["receipt_sha256"], "source_receipt_changed")
    receipt = _read_json(receipt_path)
    manifest = _manifest(receipt)
    _require(canonical_sha256(manifest) == source["manifest_sha256"], "source_manifest_changed")
    _require(_git(root, "rev-parse", "HEAD") == source["commit"], "source_commit_changed")
    _require(_git(root, "rev-parse", "HEAD^{tree}") == source["tree"], "source_tree_changed")
    edit = protocol["edit"]
    for entry in manifest:
        path = root / entry["path"]
        _require(_inside(path.resolve(strict=True), root) and not path.is_symlink(), "source_alias_outside_root")
        expected = entry["sha256"]
        expected_size = entry["bytes"]
        if edited and entry["path"] == edit["path"]:
            expected = edit["after_sha256"]
            expected_size += len(EDIT_SUFFIX)
        stat = path.stat()
        _require(stat.st_nlink == 1, "source_hardlink_not_isolated")
        _require(stat.st_size == expected_size and sha256_file(path) == expected, "source_drift:" + entry["path"])
    # The frozen manifest pins raw checkout bytes. Git's cached stat/porcelain
    # may mark LF restoration dirty under autocrlf; verify actual tracked diff,
    # staging and untracked content instead of rewriting byte-pinned sources.
    _require(not _git(root, "diff", "--cached", "--name-only", "HEAD"), "source_staged_changes")
    _require(not _git(root, "ls-files", "--others", "--exclude-standard"), "source_untracked_changes")
    if protocol.get("schema_version") == 2:
        # Git's ordinary untracked query omits every ignored byte. Fresh trial
        # sources may write only the independently declared official suite.
        # External target/store/seed roots remain outside the source checkout.
        allowed = [Path(entry["release_suite_root"]) for entry in protocol.get("qualification_allocations", [])
                   if entry.get("source_root") == str(root)]
        ignored = _git(root, "ls-files", "--others", "--ignored", "--exclude-standard", "-z")
        for relative in filter(None, ignored.split("\0")):
            original = root / relative
            selected = original.resolve(strict=True)
            _require(not original.is_absolute() or _inside(original, root), "ignored_source_path_outside_root")
            _require(selected == original and not original.is_symlink()
                     and any(_inside(selected, output) for output in allowed), "source_ignored_output_not_declared")
            for ancestor in (original, *original.parents):
                if ancestor == root:
                    break
                _require(not getattr(ancestor.lstat(), "st_file_attributes", 0) & 0x400, "source_ignored_output_reparse")
    changed = _git(root, "-c", "core.autocrlf=true", "-c", "core.safecrlf=false", "diff", "--name-only", "--ignore-submodules=none")
    _require(changed.splitlines() == ([edit["path"]] if edited else []), "source_worktree_not_frozen")
    _require(sha256_file(root / "Cargo.lock") == source["lock_sha256"], "lockfile_changed")


def _identity(value: Any, label: str, required_version: str | None = None) -> dict:
    _require(isinstance(value, dict) and set(value) == {"path", "sha256", "version"}, f"invalid_{label}_identity")
    path = _path(value["path"], label)
    _require(path.is_file() and path.suffix.lower() == ".exe", f"invalid_{label}_executable")
    _digest(value["sha256"], label + "_hash")
    _require(sha256_file(path) == value["sha256"], f"{label}_binary_changed")
    _require(isinstance(value["version"], str) and bool(value["version"].strip()), f"{label}_version_required")
    if required_version:
        _require(value["version"].startswith(required_version + " ") or value["version"].splitlines()[0] == required_version, f"{label}_version_not_1_97_1")
    return {"path": str(path), "sha256": value["sha256"], "version": value["version"]}


def _arm_config(value: Any, label: str) -> dict:
    _require(isinstance(value, dict), f"{label}_config_required")
    _require(not (set(value) - {"debug", "incremental", "layout", "tool"}), f"unknown_{label}_config")
    debug = value.get("debug", "full")
    incremental = value.get("incremental", "on")
    layout = value.get("layout", "isolated")
    _require(debug in ("full", "line-tables-only"), "unsupported_debug")
    _require(incremental in ("on", "one-shot-off"), "unsupported_incremental")
    _require(layout in ("isolated", "3lane"), "unsupported_layout")
    tool = value.get("tool", {"kind": "native"})
    _require(isinstance(tool, dict), "invalid_tool")
    kind = tool.get("kind")
    _require(kind in ("native", "sccache", "mbx"), "unsupported_tool")
    normalized_tool: dict[str, Any]
    if kind == "native":
        _require(set(tool) == {"kind"}, "native_tool_has_cache_options")
        normalized_tool = {"kind": "native"}
    else:
        known = {"kind", "path", "sha256", "version", "store", "store_limit_bytes", "prewarm_source_root", "prewarm_receipt_path", "prewarm_receipt_sha256", "server_port", "views", "native_check_lane", "local_rw_mode"}
        _require(not (set(tool) - known), "unknown_tool_option")
        executable = _identity({key: tool.get(key) for key in ("path", "sha256", "version")}, "cache_tool")
        store = _path(tool.get("store"), "tool_store", exists=False)
        limit = _count(tool.get("store_limit_bytes"), "store_limit_bytes", positive=True)
        prewarm = _path(tool.get("prewarm_source_root"), "prewarm_source_root")
        prewarm_receipt = _path(tool.get("prewarm_receipt_path"), "prewarm_receipt")
        _require(sha256_file(prewarm_receipt) == _digest(tool.get("prewarm_receipt_sha256"), "prewarm_receipt_hash"), "prewarm_receipt_changed")
        normalized_tool = {**executable, "kind": kind, "store": str(store), "store_limit_bytes": limit,
                               "prewarm_source_root": str(prewarm), "prewarm_receipt_path": str(prewarm_receipt), "prewarm_receipt_sha256": tool["prewarm_receipt_sha256"]}
        if kind == "sccache":
            port = _count(tool.get("server_port"), "sccache_port", positive=True)
            _require(1024 <= port <= 65535, "invalid_sccache_port")
            normalized_tool["server_port"] = port
            _require(tool.get("local_rw_mode") == "READ_ONLY", "sccache_warm_store_read_only_required")
            normalized_tool["local_rw_mode"] = "READ_ONLY"
        else:
            _require(tool.get("views") is False and tool.get("native_check_lane") is False, "mbx_views_and_native_lane_must_be_off")
            normalized_tool.update(views=False, native_check_lane=False)
    return {"debug": debug, "incremental": incremental, "layout": layout, "tool": normalized_tool}


def balanced_order(pairs: int, seed: int, *, protocol_version: int = 1) -> list[list[str]]:
    _require(type(protocol_version) is int and protocol_version in (1, 2), "unsupported_protocol")
    _require(type(pairs) is int and (pairs in (5, 100) if protocol_version == 1 else 2 <= pairs <= 1000), "fixed_pairs_required")
    _count(seed, "seed")
    orders = [["baseline", "candidate"] if index % 2 == 0 else ["candidate", "baseline"] for index in range(pairs)]
    random.Random(seed).shuffle(orders)
    return orders


def cache_reservation_bytes(peak_bytes: int) -> int:
    """ceil(1.25 * measured GiB peak + 1) GiB, with integer arithmetic."""
    _count(peak_bytes, "pilot_peak")
    return ((5 * peak_bytes + 4 * GIB - 1) // (4 * GIB) + 1) * GIB


def _proof_reference(value: Any, label: str) -> tuple[dict, dict]:
    _require(isinstance(value, dict) and set(value) == {"path", "sha256"}, "invalid_" + label)
    path = _path(value.get("path"), label)
    _require(path.is_file() and not path.is_symlink(), label + "_file_required")
    digest = _digest(value.get("sha256"), label + "_hash")
    _require(sha256_file(path) == digest, label + "_changed")
    payload = _read_json(path)
    _require(sha256_file(path) == digest, label + "_changed_during_read")
    return {"path": str(path), "sha256": digest}, payload



def _reservation_observation_peak(reference: dict, expected: dict, scope: str) -> int:
    """Verify the originating sealed observation, never status/allocation flags."""
    _, observation = _proof_reference(reference, "reservation_observation")
    protocol_reference = observation.get("protocol")
    _, origin = _proof_reference(protocol_reference, "reservation_observation_protocol")
    _require(origin.get("schema_version") == 2
             and origin.get("phase") in ("capacity", "pilot"), "reservation_observation_phase_invalid")
    _validate_protocol(origin)
    _require(origin["source"]["receipt_sha256"] == expected["source"]["receipt_sha256"]
             and origin["baseline"]["receipt_sha256"] == expected["baseline"]["receipt_sha256"]
             and origin["toolchain"] == expected["toolchain"], "reservation_observation_identity_mismatch")
    helper = _retention_module()
    # The helper verifies its known checker/tool hashes, signed closure and all
    # retained logs/source/receipt bytes. This is a local pre-disposal record;
    # it does not claim independently reread deleted artifacts.
    seal = helper._verify_seal(observation.get("seal"), origin)
    if scope == "expanded-capacity":
        _require(seal.get("kind") == "build-feedback-capacity-seal"
                 and origin["phase"] == "capacity", "reservation_capacity_seal_required")
        raw = seal.get("probe_receipt")
        raw_reference = seal.get("probe_reference")
        _, current = _proof_reference(raw_reference, "reservation_capacity_receipt")
        _require(isinstance(raw, dict) and current == raw and raw.get("status") == "ok"
                 and raw.get("capture_complete") is True and raw.get("allocation_complete") is True,
                 "reservation_observation_failed")
        _require(raw.get("protocol_sha256") == canonical_sha256(origin)
                 and raw.get("source_receipt_sha256") == origin["source"]["receipt_sha256"]
                 and raw.get("toolchain_sha256") == canonical_sha256(origin["toolchain"]),
                 "reservation_observation_identity_mismatch")
        _, allocation = _proof_reference(raw.get("task_output_allocation_receipt"), "reservation_output_allocation")
        _require(allocation.get("complete") is True and allocation.get("errors") == []
                 and allocation.get("allocated_bytes") == raw.get("task_output_allocated_bytes"),
                 "reservation_output_allocation_invalid")
        peak = _count(raw.get("peak_owned_output_bytes"), "reservation_observation_peak", positive=True)
        _require(peak >= allocation["allocated_bytes"], "reservation_peak_below_allocation")
    else:
        full_capacity = _full_capacity(origin)
        _require(scope == "full-arm-and-qualification"
                 and ((origin["phase"] == "pilot" and seal.get("kind") == "build-feedback-artifact-seal")
                      or (full_capacity and seal.get("kind") == "build-feedback-full-capacity-seal")),
                 "reservation_full_arm_seal_required")
        raw, projected = seal.get("arm_receipt"), seal.get("arm_projection")
        _, current = _proof_reference(seal.get("arm_reference"), "reservation_arm_receipt")
        _require(isinstance(raw, dict) and current == raw and isinstance(projected, dict)
                 and raw.get("status") == "ok" and raw.get("schema_version") == 2
                 and raw.get("capture_complete") is True and raw.get("allocation_complete") is True
                 and raw.get("source_verified") is True and raw.get("process_tree_drained") is True
                 and raw.get("protocol_sha256") == canonical_sha256(origin)
                 and raw.get("source_receipt_sha256") == origin["source"]["receipt_sha256"]
                 and raw.get("toolchain_sha256") == canonical_sha256(origin["toolchain"]),
                 "reservation_observation_failed")
        stats = helper._module("statistics")
        _require(set(seal.get("qualifications", {})) == set(stats.QUALIFICATIONS) - {"edit"},
                 "reservation_full_qualifications_required")
        allocation_reference = {"path": str(Path(raw["target_root"]).parent / "owned-allocation.json"),
                                "sha256": raw.get("owned_allocation_receipt_sha256")}
        _, allocation = _proof_reference(allocation_reference, "reservation_arm_allocation")
        _require(allocation == raw.get("owned_allocation_receipt") and allocation.get("complete") is True
                 and allocation.get("errors") == [] and allocation.get("allocated_bytes") == raw.get("owned_allocated_bytes"),
                 "reservation_output_allocation_invalid")
        # Includes the original complete arm and every qualification command's
        # conservative live job-write/global-allocation observation. Require
        # that coverage rather than accepting the small leaf cap as a full arm.
        captures = [entry for field in ("jobs", "feedback_warm_jobs", "edit_jobs") for entry in raw.get(field, [])]
        _require(all([entry.get("job") for entry in raw.get(field, [])] == [job["name"] for job in JOBS]
                     for field in ("jobs", "feedback_warm_jobs", "edit_jobs")), "reservation_full_workloads_required")
        for qualification in seal["qualifications"].values():
            _require(isinstance(qualification, dict) and isinstance(qualification.get("proof"), dict),
                     "reservation_qualification_invalid")
            _, verified = _proof_reference(qualification.get("reference"), "reservation_qualification")
            _require(verified == qualification["proof"], "reservation_qualification_changed")
            captures.append(verified.get("capture"))
        _require(observation.get("config_sha256") == canonical_sha256(raw.get("config")),
                 "reservation_observation_configuration_mismatch")
        if full_capacity:
            _require(seal.get("workload_complete") is True and seal.get("reservation_eligible") is True
                     and seal.get("performance_evidence") is False
                     and seal.get("pair_index") is None and seal.get("attempt") is None,
                     "full_capacity_reservation_closure_incomplete")
        origin_allocated = _count(raw.get("allocated_before_bytes"), "reservation_origin_allocation")
        peaks = [_count(raw.get("owned_allocated_bytes"), "reservation_owned_allocation", positive=True),
                 _count(raw.get("final_job_write_bound_bytes", 0), "reservation_final_write_bound")]
        final_reference = seal.get("task_output_allocation_receipt")
        if final_reference is not None:
            _, final_allocation = _proof_reference(final_reference, "reservation_final_allocation")
            _require(final_allocation.get("complete") is True and final_allocation.get("errors") == [],
                     "reservation_final_allocation_incomplete")
            peaks.append(_count(final_allocation.get("allocated_bytes"), "reservation_final_owned_allocation"))
        for capture in captures:
            _require(isinstance(capture, dict) and stats._capture_valid(capture), "reservation_raw_capture_invalid")
            budget = capture.get("qualification_budget")
            if isinstance(budget, dict):
                peaks.append(_count(budget.get("final_job_write_bound_bytes"), "reservation_qualification_final_write_bound"))
                peaks.append(_count(capture.get("owned_allocated_bytes"), "reservation_qualification_owned_allocation"))
            samples = capture.get("resource_samples")
            _require(isinstance(samples, list) and bool(samples), "reservation_resource_samples_required")
            for sample in samples:
                _require(isinstance(sample, dict) and sample.get("allocation_kind") == "conservative-disk-and-job-write-bound"
                         and sample.get("budget_allowed") is True and sample.get("budget_reasons") == [],
                         "reservation_resource_sample_invalid")
                peaks.append(max(_count(sample.get("job_write_bound_bytes"), "reservation_job_write_bound"),
                                 max(0, _count(sample.get("allocated_bytes"), "reservation_live_allocation") - origin_allocated)))
        peak = max(peaks)
    _require(observation.get("observed_peak_bytes") == peak and type(observation.get("observed_peak_bytes")) is int,
             "reservation_peak_not_from_observation_closure")
    return peak


def _verify_completed_v2_pilot(state: dict, protocol: dict) -> None:
    """Formal registration needs five verified retained pairs, not a summary."""
    _validate_resume(state, protocol, canonical_sha256(protocol))
    _require(protocol.get("schema_version") == 2 and protocol.get("phase") == "pilot"
             and protocol.get("pairs") == 5 and state.get("complete") is True
             and state.get("status") == "complete" and state.get("completed_valid_pairs") == 5,
             "complete_matching_pilot_required")
    helper = _retention_module()
    valid = [pair for pair in state["pairs"] if pair.get("status") == "ok"]
    _require(len(valid) == 5 and {entry["pair_index"] for entry in valid} == set(range(5)),
             "complete_matching_pilot_required")
    for pair in valid:
        links = pair.get("retained_evidence")
        _require(isinstance(links, dict) and set(links) == {"baseline", "candidate"}, "formal_pilot_retained_proofs_required")
        for arm in ("baseline", "candidate"):
            retained = helper.verify_retained_arm(links[arm], protocol, arm, pair["pair_index"], pair["attempt"])
            _require(retained.get("arm_receipt") == pair["arms"][arm], "formal_pilot_arm_mismatch")
            seal = retained.get("seal")
            stats = helper._module("statistics")
            _require(isinstance(seal, dict) and isinstance(seal.get("arm_projection"), dict)
                     and all(seal["arm_projection"].get(flag) is True for flag in
                             ("capture_complete", "source_verified", "artifacts_verified", "allocation_complete", "quiet"))
                     and set(seal.get("qualifications", {})) == set(stats.QUALIFICATIONS) - {"edit"},
                     "formal_pilot_qualification_closure_incomplete")
    # The preregistration caller separately binds source/toolchain/config/B.
    # Quality/performance failures remain in the campaign; only contamination
    # may have been retried, with no discarded successful arm evidence.
    _require(all(pair.get("status") in ("ok", "contaminated") for pair in state["pairs"]),
             "formal_pilot_contains_failed_attempt")


def _budget_keywords(protocol: dict) -> dict:
    """V1 call shape stays unchanged for legacy producers and tests."""
    if protocol.get("schema_version", 1) == 1:
        return {}
    policy = protocol["budget_policy"]
    return {"policy_version": 2, "minimum_free_bytes": policy["minimum_free_bytes"],
            "external_growth_allowance_bytes": policy["external_growth_allowance_bytes"]}


def _disk_floor(protocol: dict) -> int:
    return protocol["budget_policy"]["minimum_free_bytes"] if protocol.get("schema_version", 1) == 2 else 80 * GIB


def _external_allowance(protocol: dict) -> int:
    return protocol["budget_policy"]["external_growth_allowance_bytes"] if protocol.get("schema_version", 1) == 2 else 0


def _disposable_paths(protocol: dict) -> list[str]:
    root = Path(protocol["run_root"])
    if protocol["phase"] == "capacity":
        return [str(root / "capacity" / "targets")]
    return [str(root / "pairs" / f"pair-{entry['pair_index']:04d}-attempt-{entry['attempt']:02d}" / arm / "targets")
            for entry in protocol["allocations"] for arm in ("baseline", "candidate")]


def _full_capacity(protocol: dict) -> bool:
    return protocol.get("schema_version") == 2 and protocol.get("phase") == "capacity" and protocol.get("capacity_probe", {}).get("kind") == "full-arm-and-qualification"


def qualification_allocations(protocol: dict) -> list[dict]:
    """Reconstruct source-relative official suites; caller-selected paths have no authority."""
    allocated = []
    if _full_capacity(protocol):
        probe = protocol["capacity_probe"]
        entries = [(None, None, probe["arm"], probe["source_root"])]
    elif protocol["phase"] == "capacity":
        entries = []
    else:
        entries = [(entry["pair_index"], entry["attempt"], arm, entry[arm + "_source_root"])
                   for entry in protocol["allocations"] for arm in ("baseline", "candidate")]
    for index, attempt, arm, source in entries:
        suite_name = f"asg-build-feedback-{protocol['phase']}-{index if index is not None else 'capacity'}-{attempt if attempt is not None else 0}-{arm}"
        _require(len(suite_name) <= 64, "qualification_suite_id_too_long")
        suite = Path(source) / "evidence-output" / "build-feedback" / suite_name
        for ancestor in (suite, *suite.parents):
            if ancestor.exists():
                _require(not ancestor.is_symlink() and not getattr(ancestor.lstat(), "st_file_attributes", 0) & 0x400,
                         "qualification_suite_reparse_not_allowed")
        allocated.append({"pair_index": index, "attempt": attempt, "arm": arm, "source_root": source,
                          "release_suite_root": str(suite)})
    return allocated


def _add_bounded_contract(protocol: dict, data: dict, live: Path, coordinator: Path, allocated_sources: list[Path]) -> None:
    """Revision bindings are rebuilt, never inferred from inline verified flags."""
    policy = data.get("budget_policy")
    expected_policy = {"policy_version": 2, "minimum_free_bytes": BOUNDED_MINIMUM_FREE_BYTES,
                       "external_growth_allowance_bytes": EXTERNAL_GROWTH_ALLOWANCE_BYTES}
    _require(isinstance(policy, dict) and set(policy) == set(expected_policy) and all(type(policy[k]) is int and policy[k] == v for k, v in expected_policy.items()), "bounded_budget_policy_required")
    protocol["budget_policy"] = dict(expected_policy)
    phase = protocol["phase"]
    _require(not any(Path(entry).resolve() == Path(protocol["run_root"]) for entry in protocol["store_roots"]), "run_root_store_alias_not_allowed")
    probe = data.get("capacity_probe")
    if phase == "capacity":
        if not isinstance(probe, dict):
            raise ProtocolError("capacity_probe_required")
        full = probe.get("kind") == "full-arm-and-qualification"
        required = ({"kind", "source_root", "arm", "stage", "owned_output_cap_bytes"} if full
                    else {"source_root", "package", "command", "stage", "owned_output_cap_bytes"})
        _require(set(probe) == required, "capacity_probe_required")
        if full:
            _require(probe["stage"] == "expanded" and probe["arm"] in ("baseline", "candidate"), "invalid_full_capacity_probe")
        else:
            _require(probe["package"] in ("agent-session-grep-domain", "agent-session-grep-ports") and probe["command"] in ("check", "test"), "single_leaf_capacity_command_required")
            _require(probe["stage"] in ("initial", "expanded"), "invalid_capacity_stage")
        cap = _count(probe["owned_output_cap_bytes"], "owned_output_cap_bytes", positive=True)
        _require(cap == protocol["predicted_growth_bytes"], "capacity_reservation_must_equal_owned_cap")
        if full:
            _require(cap >= INITIAL_CAPACITY_OUTPUT_CAP_BYTES, "declared_full_capacity_cap_at_least_8gib")
        elif probe["stage"] == "initial":
            _require(cap == INITIAL_CAPACITY_OUTPUT_CAP_BYTES, "initial_capacity_cap_must_be_8gib")
        root = _path(probe["source_root"], "capacity_source_root")
        _dedicated_source(root, live, coordinator)
        _require(not any(_inside(root, other) or _inside(other, root) for other in (Path(protocol["source"]["root"]), Path(protocol["run_root"]), *allocated_sources)), "capacity_source_must_be_distinct")
        _require(all(config["tool"]["kind"] == "native" for config in protocol["configs"].values()), "capacity_native_only")
        _verify_source(root, protocol)
        protocol["capacity_probe"] = {**probe, "source_root": str(root)}
    else:
        _require(probe is None, "capacity_fields_outside_capacity")
        protocol["capacity_probe"] = None

    protocol["qualification_allocations"] = qualification_allocations(protocol)

    provenance = data.get("reservation_provenance")
    if not isinstance(provenance, dict):
        raise ProtocolError("reservation_provenance_required")
    if _full_capacity(protocol):
        _require(set(provenance) == {"kind", "prerequisite", "rationale"}
                 and provenance["kind"] == "declared-full-arm-cap"
                 and isinstance(provenance["rationale"], str) and bool(provenance["rationale"].strip()),
                 "declared_full_capacity_provenance_required")
        reference, prerequisite = _proof_reference(provenance["prerequisite"], "full_capacity_leaf_prerequisite")
        leaf_peak = _reservation_observation_peak(reference, protocol, "expanded-capacity")
        _require(protocol["predicted_growth_bytes"] >= cache_reservation_bytes(leaf_peak), "declared_full_capacity_cap_below_verified_leaf_margin")
        _, leaf_protocol = _proof_reference(prerequisite.get("protocol"), "full_capacity_leaf_protocol")
        leaf = leaf_protocol.get("capacity_probe", {})
        _require(leaf_protocol.get("phase") == "capacity" and leaf.get("kind") is None
                 and leaf.get("stage") == "initial" and leaf.get("owned_output_cap_bytes") == INITIAL_CAPACITY_OUTPUT_CAP_BYTES,
                 "full_capacity_requires_successful_initial_leaf")
        protocol["reservation_provenance"] = {"kind": "declared-full-arm-cap", "prerequisite": reference,
                                              "rationale": provenance["rationale"]}
    elif phase == "capacity" and isinstance(probe, dict) and probe["stage"] == "initial":
        _require(provenance == {"kind": "initial-capacity-cap"}, "initial_capacity_reservation_required")
        protocol["reservation_provenance"] = dict(provenance)
    else:
        _require(set(provenance) == {"kind", "receipt", "margin_bytes"} and provenance["kind"] == "measured-capacity", "measured_reservation_required")
        reference, evidence = _proof_reference(provenance["receipt"], "capacity_reservation_evidence")
        _require(type(evidence.get("schema_version")) is int and evidence.get("schema_version") == 2 and evidence.get("kind") == "build-feedback-reservation-evidence" and evidence.get("complete") is True, "complete_reservation_evidence_required")
        _require(evidence.get("source_receipt_sha256") == protocol["source"]["receipt_sha256"] and evidence.get("baseline_receipt_sha256") == protocol["baseline"]["receipt_sha256"] and evidence.get("toolchain_sha256") == canonical_sha256(protocol["toolchain"]), "reservation_evidence_identity_mismatch")
        margin = _count(provenance["margin_bytes"], "reservation_margin_bytes", positive=True)
        peak = _count(evidence.get("observed_peak_bytes"), "reservation_observed_peak_bytes", positive=True)
        _require(protocol["predicted_growth_bytes"] >= peak + margin, "reservation_below_observed_peak_and_margin")
        _require(evidence.get("workload_scope") == ("expanded-capacity" if phase == "capacity" else "full-arm-and-qualification"), "reservation_workload_scope_mismatch")
        observations = evidence.get("observations")
        if not isinstance(observations, list) or not observations:
            raise ProtocolError("raw_reservation_observations_required")
        observed_peaks = [_reservation_observation_peak(observation, protocol, evidence["workload_scope"])
                          for observation in observations]
        _require(peak == max(observed_peaks), "reservation_peak_not_from_observations")
        if phase != "capacity":
            observed_configs = {_proof_reference(observation, "reservation_observation")[1].get("config_sha256")
                                for observation in observations}
            expected_configs = {canonical_sha256(config) for config in protocol["configs"].values()}
            _require(observed_configs == expected_configs, "reservation_must_cover_both_configurations")
        protocol["reservation_provenance"] = {"kind": "measured-capacity", "receipt": reference, "margin_bytes": margin}

    retention = data.get("retention_policy")
    if not isinstance(retention, dict) or set(retention) != {"mode", "approval"} or retention["mode"] != "sealed-verified-recycle":
        raise ProtocolError("retention_policy_required")
    approval_ref, approval = _proof_reference(retention["approval"], "retention_approval")
    _require(type(approval.get("schema_version")) is int and approval.get("schema_version") == 2 and approval.get("kind") == "build-feedback-retention-approval" and approval.get("approved") is True, "retention_approval_required")
    _require(approval.get("source_receipt_sha256") == protocol["source"]["receipt_sha256"] and approval.get("baseline_receipt_sha256") == protocol["baseline"]["receipt_sha256"] and approval.get("toolchain_sha256") == canonical_sha256(protocol["toolchain"]) and approval.get("configs_sha256") == canonical_sha256(protocol["configs"]), "retention_approval_identity_mismatch")
    _require(approval.get("run_root") == protocol["run_root"] and approval.get("disposable_target_paths") == _disposable_paths(protocol), "retention_approval_scope_mismatch")
    for approved_path in _disposable_paths(protocol):
        target = Path(approved_path)
        for ancestor in (target, *target.parents):
            if ancestor.exists():
                _require(not ancestor.is_symlink() and not getattr(ancestor.lstat(), "st_file_attributes", 0) & 0x400, "disposable_path_reparse_not_allowed")
    _require(approval.get("scope") == "validated-successful-and-cap-stopped-task-intermediates" and approval.get("flow") == ["verify", "recycle", "match", "purge-exact", "verify-release", "receipt"], "retention_flow_not_approved")
    checker = approval.get("checker")
    if not isinstance(checker, dict) or set(checker) != {"path", "sha256", "bytes"}:
        raise ProtocolError("approved_retention_checker_required")
    checker_path = _path(checker.get("path"), "retention_checker")
    _require(checker_path.is_file() and not checker_path.is_symlink() and checker_path.stat().st_size == _count(checker.get("bytes"), "checker_bytes", positive=True) and sha256_file(checker_path) == _digest(checker.get("sha256"), "checker_hash"), "retention_checker_changed")
    _require(approval.get("protected") == ["source", "git-common-directory", "daily-cache", "existing-last-72-hours", "stores", "seeds", "quality-source-failures", "unrelated-recycle-items", "pagefile"], "retention_protected_scope_required")
    protocol["retention_policy"] = {"mode": "sealed-verified-recycle", "approval": approval_ref}

    registration = data.get("formal_registration")
    if phase != "formal":
        _require(registration is None, "formal_registration_outside_formal")
        protocol["formal_registration"] = None
        return
    registration_ref, plan = _proof_reference(registration, "formal_registration")
    _require(plan.get("schema_version") == 2 and plan.get("kind") == "build-feedback-formal-registration" and plan.get("approved") is True and plan.get("formal_data_collected") is False, "preregistered_formal_plan_required")
    _require(plan.get("pairs") == protocol["pairs"] and type(plan.get("pairs")) is int and plan.get("seed") == protocol["seed"] and type(plan.get("seed")) is int and plan.get("order") == protocol["order"], "formal_registration_schedule_mismatch")
    _require(plan.get("pilot_receipt_sha256") == protocol["pilot"]["receipt_sha256"] and plan.get("source_receipt_sha256") == protocol["source"]["receipt_sha256"] and plan.get("baseline_receipt_sha256") == protocol["baseline"]["receipt_sha256"] and plan.get("toolchain_sha256") == canonical_sha256(protocol["toolchain"]) and plan.get("configs_sha256") == canonical_sha256(protocol["configs"]), "formal_registration_identity_mismatch")
    _require(plan.get("run_root") == protocol["run_root"] and plan.get("stopping_rule") == "fixed-n-no-adaptive-additions" and isinstance(plan.get("rationale"), str) and bool(plan["rationale"].strip()), "formal_registration_rationale_required")
    approved_at = plan.get("approved_at")
    if not isinstance(approved_at, str):
        raise ProtocolError("formal_registration_timestamp_required")
    try:
        approved_time = datetime.fromisoformat(approved_at)
        protocol_time = datetime.fromisoformat(protocol["created_at"])
        _require(approved_time.tzinfo is not None and protocol_time.tzinfo is not None and approved_time <= protocol_time, "formal_registration_after_protocol")
    except (TypeError, ValueError) as error:
        raise ProtocolError("invalid_formal_registration_timestamp") from error
    _count(plan.get("maximum_compute_seconds"), "registered_maximum_compute_seconds", positive=True)
    _count(plan.get("maximum_storage_bytes"), "registered_maximum_storage_bytes", positive=True)
    _require(plan["maximum_storage_bytes"] >= protocol["predicted_growth_bytes"], "registered_storage_does_not_cover_arm")
    protocol["formal_registration"] = registration_ref


def make_protocol(configuration: Mapping[str, Any] | None = None, **fields: Any) -> dict:
    data = dict(configuration or {})
    _require(not (set(data) & set(fields)), "duplicate_protocol_field")
    data.update(fields)
    version = data.get("protocol_version", 1)
    _require(type(version) is int and version in (1, 2), "unsupported_protocol")
    if version == 1:
        _require(not (set(data) & {"budget_policy", "reservation_provenance", "retention_policy", "formal_registration", "capacity_probe"}), "bounded_fields_require_protocol_v2")
    known = {"source_root", "source_commit", "source_tree", "source_receipt_path", "baseline_receipt_path", "run_root", "toolchain", "baseline_config", "candidate_config", "phase", "seed", "pairs", "jobs", "test_threads", "inventory_roots", "store_roots", "predicted_growth_bytes", "allocations", "edit_path", "timeout_seconds", "poll_seconds", "pilot_receipt_path", "pilot_receipt_sha256", "msvc_environment_receipt_path", "protocol_version", "budget_policy", "reservation_provenance", "retention_policy", "formal_registration", "capacity_probe"}
    _require(not (set(data) - known), "unknown_protocol_fields:" + ",".join(sorted(set(data) - known)))
    source_root = _path(data.get("source_root"), "source_root")
    source_receipt = _path(data.get("source_receipt_path"), "source_receipt")
    receipt = _read_json(source_receipt)
    _require(receipt.get("schema_version") == 1 and receipt.get("live_source_unchanged") is True, "approved_frozen_source_receipt_required")
    commit = _digest(data.get("source_commit"), "source_commit", 40)
    tree = _digest(data.get("source_tree"), "source_tree", 40)
    _require(receipt.get("snapshot_commit") == commit and receipt.get("snapshot_tree") == tree, "snapshot_receipt_identity_mismatch")
    live = _path(receipt.get("live_source"), "live_source")
    run_root = _path(data.get("run_root"), "run_root", exists=False)
    coordinator = Path(__file__).resolve().parents[2]
    _dedicated_source(source_root, live, coordinator)
    _require(not any(_inside(run_root, root) or _inside(root, run_root) for root in (source_root, live)), "run_root_overlaps_source")
    _require(not _inside(coordinator, run_root), "run_root_overlaps_coordinator")
    if _inside(run_root, coordinator):
        _require(_coordinator_output_path(run_root, coordinator), "run_root_must_use_ignored_coordinator_runtime")
    baseline_receipt = _path(data.get("baseline_receipt_path"), "baseline_receipt")
    baseline = _read_json(baseline_receipt)
    _require(baseline.get("schema_version") == 1 and baseline.get("complete") is True and baseline.get("errors") == [], "complete_baseline_receipt_required")
    baseline_bytes = _count(baseline.get("allocated_bytes"), "baseline_allocated_bytes")
    phase = data.get("phase", "pilot")
    _require(phase in (("capacity", "pilot", "formal") if version == 2 else ("pilot", "formal")), "unsupported_phase")
    pairs = 0 if phase == "capacity" else 5 if phase == "pilot" else 100 if version == 1 else _count(data.get("pairs"), "registered_formal_pairs", positive=True)
    _require(data.get("pairs", pairs) == pairs and type(data.get("pairs", pairs)) is int, "fixed_pairs_required")
    if version == 2 and phase == "formal":
        _require(2 <= pairs <= 1000, "formal_pairs_outside_registration_bounds")
    _require(type(data.get("jobs", 2)) is int and type(data.get("test_threads", 2)) is int and data.get("jobs", 2) == 2 and data.get("test_threads", 2) == 2, "fixed_parallelism_required")
    seed = _count(data.get("seed", 20261018), "seed")
    toolchain = data.get("toolchain")
    if not isinstance(toolchain, dict) or set(toolchain) != {"cargo", "rustc", "msvc", "target"}:
        raise ProtocolError("complete_toolchain_required")
    _require(toolchain["target"] == "x86_64-pc-windows-msvc", "unsupported_target")
    msvc = toolchain["msvc"]
    _require(isinstance(msvc, dict) and set(msvc) == {"compiler", "linker"}, "msvc_identity_required")
    normalized_chain = {"cargo": _identity(toolchain["cargo"], "cargo", "cargo 1.97.1"), "rustc": _identity(toolchain["rustc"], "rustc", "rustc 1.97.1"),
                        "msvc": {"compiler": _identity(msvc["compiler"], "msvc_compiler"), "linker": _identity(msvc["linker"], "msvc_linker")}, "target": toolchain["target"]}
    _require(normalized_chain["cargo"]["sha256"] != normalized_chain["rustc"]["sha256"], "direct_toolchain_binaries_required_not_rustup_proxies")
    msvc_environment = None
    if data.get("msvc_environment_receipt_path") is not None:
        environment_path = _path(data["msvc_environment_receipt_path"], "msvc_environment_receipt")
        environment_receipt = _read_json(environment_path)
        _require(environment_receipt.get("schema_version") == 1 and environment_receipt.get("identity") == normalized_chain["msvc"], "msvc_environment_identity_mismatch")
        variables = environment_receipt.get("environment")
        if not isinstance(variables, dict) or not all(isinstance(k, str) and isinstance(v, str) and "\0" not in k + v for k, v in variables.items()):
            raise ProtocolError("invalid_msvc_environment")
        _require(all(variables.get(k) for k in ("LIB", "INCLUDE", "VCToolsInstallDir", "WindowsSdkDir")), "msvc_environment_incomplete")
        _require(set(variables) <= {"INCLUDE", "LIB", "LIBPATH", "VCToolsInstallDir", "VCToolsVersion", "VCINSTALLDIR", "VSINSTALLDIR", "VisualStudioVersion", "WindowsSdkDir", "WindowsSDKVersion", "WindowsSDKLibVersion", "UniversalCRTSdkDir", "UCRTVersion", "VSCMD_ARG_HOST_ARCH", "VSCMD_ARG_TGT_ARCH"}, "msvc_environment_unknown_keys")
        script = environment_receipt.get("script", {})
        _require(sha256_file(_path(script.get("path"), "msvc_environment_script")) == script.get("sha256"), "msvc_environment_script_changed")
        msvc_environment = {"receipt_path": str(environment_path), "receipt_sha256": sha256_file(environment_path)}
    manifest = _manifest(receipt)
    by_path = {entry["path"]: entry for entry in manifest}
    _require("Cargo.lock" in by_path, "frozen_lockfile_required")
    edit_path = data.get("edit_path", DEFAULT_EDIT_PATH)
    _require(isinstance(edit_path, str) and edit_path.startswith("crates/") and edit_path.endswith(".rs") and edit_path in by_path, "deterministic_product_edit_required")
    edit_original = (source_root / edit_path).read_bytes()
    _require(hashlib.sha256(edit_original).hexdigest() == by_path[edit_path]["sha256"], "edit_source_drift")
    resources = [entry for entry in manifest if "resources" in Path(entry["path"]).parts or entry["path"].startswith("schemas/")]
    roots = data.get("inventory_roots")
    if not isinstance(roots, (tuple, list)) or not roots:
        raise ProtocolError("inventory_roots_required")
    roots = [str(_path(root, "inventory_root")) for root in roots]
    stores = [str(_path(root, "store_root", exists=False)) for root in data.get("store_roots", ())]
    configs = {"baseline": _arm_config(data.get("baseline_config", {"tool": {"kind": "native"}}), "baseline"), "candidate": _arm_config(data.get("candidate_config"), "candidate")}
    allocations = data.get("allocations", [])
    _require(isinstance(allocations, list), "invalid_allocations")
    if phase == "capacity":
        _require(not allocations, "capacity_has_no_pair_allocations")
    normalized_allocations: list[dict[str, Any]] = []
    allocated_sources: list[Path] = []
    keys: set[tuple[int, int]] = set()
    for allocation in allocations:
        _require(isinstance(allocation, dict) and set(allocation) == {"pair_index", "attempt", "baseline_source_root", "candidate_source_root"}, "invalid_allocation")
        pair_index = _count(allocation["pair_index"], "pair_index")
        attempt = _count(allocation["attempt"], "attempt")
        _require(pair_index < pairs and attempt < 3 and (pair_index, attempt) not in keys, "duplicate_or_invalid_allocation")
        keys.add((pair_index, attempt))
        entry: dict[str, Any] = {"pair_index": pair_index, "attempt": attempt}
        for arm in ("baseline", "candidate"):
            root = _path(allocation[arm + "_source_root"], "allocation_source_root")
            _require(not any(_inside(root, other) or _inside(other, root) for other in (source_root, live, run_root)), "allocation_overlaps_protected_source")
            _dedicated_source(root, live, coordinator)
            allocated_sources.append(root)
            entry[arm + "_source_root"] = str(root)
        normalized_allocations.append(entry)
    _separate(allocated_sources, "allocated_worktrees")
    for config in configs.values():
        tool = config["tool"]
        if tool["kind"] != "native":
            _require(tool["store"] in stores, "tool_store_missing_from_inventory")
            prewarm = Path(tool["prewarm_source_root"])
            _require(not any(_inside(prewarm, other) or _inside(other, prewarm) for other in (source_root, *allocated_sources, run_root)), "prewarm_worktree_must_be_distinct")
            _dedicated_source(prewarm, live, coordinator)
            store = Path(tool["store"])
            _require(not any(_inside(store, root) or _inside(root, store) for root in [source_root, live, *allocated_sources, run_root, prewarm]), "store_overlaps_source_or_targets")
            _require(not _inside(coordinator, store) and (not _inside(store, coordinator) or _coordinator_output_path(store, coordinator)), "store_overlaps_coordinator_source")
    growth = _count(data.get("predicted_growth_bytes"), "predicted_growth_bytes", positive=True)
    timeout = data.get("timeout_seconds", 1800)
    poll = data.get("poll_seconds", 2)
    _require(type(timeout) in (float, int) and math.isfinite(timeout) and 1 <= timeout <= 14400, "invalid_timeout")
    _require(type(poll) in (float, int) and math.isfinite(poll) and 0.1 <= poll <= 10, "invalid_poll_interval")
    pilot = None
    if phase == "formal":
        pilot_path = _path(data.get("pilot_receipt_path"), "pilot_receipt")
        pilot_hash = _digest(data.get("pilot_receipt_sha256"), "pilot_receipt_hash")
        _require(sha256_file(pilot_path) == pilot_hash, "pilot_receipt_changed")
        pilot_evidence = _read_json(pilot_path)
        _require(pilot_evidence.get("phase") == "pilot" and pilot_evidence.get("complete") is True and pilot_evidence.get("status") == "complete" and pilot_evidence.get("source_tree") == tree and pilot_evidence.get("source_commit") == commit and pilot_evidence.get("planned_pairs") == 5 and pilot_evidence.get("completed_valid_pairs") == 5, "complete_matching_pilot_required")
        _require(pilot_evidence.get("toolchain_sha256") == canonical_sha256(normalized_chain), "pilot_toolchain_mismatch")
        pilot_protocol_path = _path(pilot_evidence.get("protocol_path"), "pilot_protocol")
        pilot_protocol = _read_json(pilot_protocol_path)
        _require(pilot_protocol.get("phase") == "pilot" and pilot_protocol.get("configs") == configs, "pilot_configuration_mismatch")
        _require(pilot_protocol.get("baseline", {}).get("receipt_sha256") == sha256_file(baseline_receipt), "formal_baseline_cannot_rebase_pilot")
        _validate_protocol(pilot_protocol)
        _validate_resume(pilot_evidence, pilot_protocol, canonical_sha256(pilot_protocol))
        peak = _count(pilot_evidence.get("peak_pair_growth_bytes"), "pilot_peak", positive=True)
        reserve_peak = peak
        if version == 2:
            _require(pilot_protocol.get("schema_version") == 2, "formal_v2_requires_matching_v2_pilot")
            _verify_completed_v2_pilot(pilot_evidence, pilot_protocol)
            reserve_peak = _count(pilot_evidence.get("peak_active_arm_growth_bytes"), "pilot_active_arm_peak", positive=True)
        _require(growth >= cache_reservation_bytes(reserve_peak), "forecast_below_measured_pilot_margin")
        pilot = {"receipt_path": str(pilot_path), "receipt_sha256": pilot_hash, "peak_pair_growth_bytes": peak}
    protocol = {"schema_version": version, "kind": "build-feedback-protocol", "created_at": _stamp(), "source": {"root": str(source_root), "commit": commit, "tree": tree, "receipt_path": str(source_receipt), "receipt_sha256": sha256_file(source_receipt), "manifest_sha256": canonical_sha256(manifest), "lock_sha256": by_path["Cargo.lock"]["sha256"], "resources_sha256": canonical_sha256(resources)},
                "baseline": {"receipt_path": str(baseline_receipt), "receipt_sha256": sha256_file(baseline_receipt), "allocated_bytes": baseline_bytes}, "run_root": str(run_root), "toolchain": normalized_chain, "configs": configs,
                "phase": phase, "pairs": pairs, "jobs": 2, "test_threads": 2, "seed": seed, "order": [] if phase == "capacity" else balanced_order(pairs, seed, protocol_version=version), "inventory_roots": roots, "store_roots": stores,
                "predicted_growth_bytes": growth, "allocations": normalized_allocations, "edit": {"kind": "append-comment-v1", "path": edit_path, "before_sha256": by_path[edit_path]["sha256"], "after_sha256": hashlib.sha256(edit_original + EDIT_SUFFIX).hexdigest()},
                "timeout_seconds": timeout, "poll_seconds": poll, "memory_limit_bytes": MEMORY_LIMIT_BYTES, "pilot": pilot}
    if msvc_environment is not None:
        protocol["msvc_environment"] = msvc_environment
    if version == 2:
        _add_bounded_contract(protocol, data, live, coordinator, allocated_sources)
    _verify_source(source_root, protocol)
    for root in allocated_sources:
        _verify_source(root, protocol)
    return protocol


def _validate_protocol(protocol: dict) -> None:
    """Rebuild the protocol from its explicit inputs to reject hand-edited modes."""
    _require(type(protocol.get("schema_version")) is int and protocol.get("schema_version") in (1, 2) and protocol.get("kind") == "build-feedback-protocol", "unsupported_protocol")
    source, baseline = protocol.get("source", {}), protocol.get("baseline", {})
    _require(sha256_file(_path(source.get("receipt_path"), "source_receipt")) == source.get("receipt_sha256"), "source_receipt_changed")
    _require(sha256_file(_path(baseline.get("receipt_path"), "baseline_receipt")) == baseline.get("receipt_sha256"), "baseline_receipt_changed")
    fields = {"source_root": source.get("root"), "source_commit": source.get("commit"), "source_tree": source.get("tree"), "source_receipt_path": source.get("receipt_path"), "baseline_receipt_path": baseline.get("receipt_path"), "run_root": protocol.get("run_root"), "toolchain": protocol.get("toolchain"), "baseline_config": protocol.get("configs", {}).get("baseline"), "candidate_config": protocol.get("configs", {}).get("candidate"),
              **{key: protocol.get(key) for key in ("phase", "seed", "pairs", "jobs", "test_threads", "inventory_roots", "store_roots", "predicted_growth_bytes", "allocations", "timeout_seconds", "poll_seconds")}, "edit_path": protocol.get("edit", {}).get("path")}
    if protocol["schema_version"] == 2:
        fields.update(protocol_version=2, **{key: protocol.get(key) for key in ("budget_policy", "reservation_provenance", "retention_policy", "formal_registration", "capacity_probe")})
    if protocol.get("msvc_environment"):
        bound_env = protocol["msvc_environment"]
        _require(sha256_file(_path(bound_env.get("receipt_path"), "msvc_environment_receipt")) == bound_env.get("receipt_sha256"), "msvc_environment_receipt_changed")
        fields["msvc_environment_receipt_path"] = bound_env["receipt_path"]
    if protocol.get("pilot"):
        fields.update(pilot_receipt_path=protocol["pilot"].get("receipt_path"), pilot_receipt_sha256=protocol["pilot"].get("receipt_sha256"))
    rebuilt = make_protocol(**fields)
    rebuilt["created_at"] = protocol.get("created_at")
    if protocol["schema_version"] == 2 and protocol["phase"] == "formal":
        _, registration = _proof_reference(protocol["formal_registration"], "formal_registration")
        try:
            approved_time = datetime.fromisoformat(registration["approved_at"])
            protocol_time = datetime.fromisoformat(protocol["created_at"])
            _require(approved_time.tzinfo is not None and protocol_time.tzinfo is not None and approved_time <= protocol_time, "formal_registration_after_protocol")
        except (KeyError, TypeError, ValueError) as error:
            raise ProtocolError("invalid_formal_registration_timestamp") from error
    _require(canonical_sha256(rebuilt) == canonical_sha256(protocol) if protocol["schema_version"] == 2 else rebuilt == protocol,
             "protocol_contract_changed")


def _free_bytes() -> int:
    _require(os.name == "nt", "windows_capture_required")
    available, total, free = ctypes.c_uint64(), ctypes.c_uint64(), ctypes.c_uint64()
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetDiskFreeSpaceExW.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(ctypes.c_uint64)]
    kernel.GetDiskFreeSpaceExW.restype = wintypes.BOOL
    _require(bool(kernel.GetDiskFreeSpaceExW("C:\\", ctypes.byref(available), ctypes.byref(total), ctypes.byref(free))), "disk_free_capture_failed")
    return available.value


def preview_protocol(protocol: dict, inventory_report: dict) -> dict:
    _validate_protocol(protocol)
    reasons = []
    if inventory_report.get("schema_version") != 1 or inventory_report.get("complete") is not True or inventory_report.get("errors") != []:
        reasons.append("inventory_incomplete")
    current = inventory_report.get("allocated_bytes")
    free = inventory_report.get("disk_free_bytes", inventory_report.get("free_bytes"))
    if type(current) is not int or current < 0:
        reasons.append("allocated_bytes_unknown")
    if type(free) is not int or free < 0:
        reasons.append("disk_free_bytes_unknown")
    budget = None
    if not reasons:
        budget = _inventory_module().budget_status(protocol["baseline"]["allocated_bytes"], current, free, protocol["predicted_growth_bytes"], **_budget_keywords(protocol))
        reasons.extend(budget["reasons"] if not budget["allowed"] else [])
    baseline = _read_json(Path(protocol["baseline"]["receipt_path"]))
    baseline_roots = baseline.get("roots", [])
    current_roots = inventory_report.get("roots", [])
    if baseline_roots:
        expected = {str(Path(entry["path"]).resolve()) for entry in baseline_roots if isinstance(entry, dict) and isinstance(entry.get("path"), str)}
        current_scope = {str(Path(entry["path"]).resolve()) for entry in current_roots if isinstance(entry, dict) and isinstance(entry.get("path"), str)}
        if not expected <= current_scope:
            reasons.append("initial_inventory_scope_missing")
        # Opaque tool blobs are scanned only for explicitly declared stores.
        # Keeping the same path as a discovery root must not narrow fixed-B scope.
        expected_stores = {str(Path(entry["path"]).resolve()) for entry in baseline_roots
                           if isinstance(entry, dict) and entry.get("kind") == "store"
                           and isinstance(entry.get("path"), str)}
        current_stores = {str(Path(entry["path"]).resolve()) for entry in current_roots
                          if isinstance(entry, dict) and entry.get("kind") == "store"
                          and isinstance(entry.get("path"), str)}
        if not expected_stores <= current_stores:
            reasons.append("initial_store_scope_missing")
    if (protocol["schema_version"] == 2 and inventory_report.get("roots")
            and not any(isinstance(entry, dict) and entry.get("kind") == "store" and Path(entry.get("path", "")).resolve() == Path(protocol["run_root"]) for entry in inventory_report["roots"])):
        reasons.append("retained_evidence_scope_missing")
    if not any(_inside(Path(protocol["run_root"]), Path(root)) for root in protocol["inventory_roots"]):
        reasons.append("trial_run_root_not_in_inventory_scope")
    if protocol["phase"] != "capacity" and not protocol["allocations"]:
        reasons.append("preallocated_sources_required")
    for config in protocol["configs"].values():
        if config["tool"]["kind"] == "sccache" and config["tool"].get("local_rw_mode") != "READ_ONLY":
            reasons.append("sccache_warm_store_read_only_required")
    return {"schema_version": 1, "kind": "build-feedback-preview", "created_at": _stamp(), "protocol_sha256": canonical_sha256(protocol), "inventory_sha256": canonical_sha256(inventory_report), "allowed": not reasons, "review_required": bool(budget and budget["review_required"]), "reasons": sorted(set(reasons)), "budget": budget,
            "allocated_bytes": current, "disk_free_bytes": free, "predicted_growth_bytes": protocol["predicted_growth_bytes"], "run_root": protocol["run_root"], "planned_pairs": protocol["pairs"], "phase": protocol["phase"], "disposable_target_paths": _disposable_paths(protocol)}


def job_command(protocol: dict, job: dict, target: Path) -> list[str]:
    _require(job in JOBS, "unknown_job")
    command = [protocol["toolchain"]["cargo"]["path"], *job["args"], "--locked", "--offline", "-j2", "--message-format=json", "--target-dir", str(target)]
    if job["args"][0] == "test":
        command.extend(["--", "--test-threads=2"])
    elif job["name"] == "clippy":
        command.extend(["--", "-D", "warnings"])
    return command


def tool_environment(protocol: dict, config: dict, *, feedback: bool = False) -> dict[str, str]:
    environment = {key: value for key, value in os.environ.items() if not key.upper().startswith(("CARGO_", "RUST", "SCCACHE_", "MBX_", "CCACHE_")) and key.upper() not in ("CI", "MAKEFLAGS", "MFLAGS", "CFLAGS", "CXXFLAGS", "CL", "_CL_", "LINK", "_LINK_", "CC", "CXX", "AR", "CARGO", "RUSTUP_TOOLCHAIN")}
    cargo_home = os.environ.get("CARGO_HOME")
    if not cargo_home:
        profile = os.environ.get("USERPROFILE")
        if not profile:
            raise ProtocolError("cargo_home_identity_required")
        cargo_home = str(Path(profile) / ".cargo")
    environment["CARGO_HOME"] = str(_path(cargo_home, "cargo_home"))
    environment.update(RUSTC=protocol["toolchain"]["rustc"]["path"], RUSTC_WRAPPER="", RUSTC_WORKSPACE_WRAPPER="", RUSTFLAGS="", CARGO_ENCODED_RUSTFLAGS="", CARGO_BUILD_JOBS="2", CARGO_NET_OFFLINE="true", CARGO_INCREMENTAL="1" if feedback or config["incremental"] == "on" else "0",
                       CARGO_PROFILE_DEV_DEBUG="2" if config["debug"] == "full" else "line-tables-only", CARGO_PROFILE_TEST_DEBUG="2" if config["debug"] == "full" else "line-tables-only")
    compiler = protocol["toolchain"]["msvc"]["compiler"]["path"]
    linker = protocol["toolchain"]["msvc"]["linker"]["path"]
    if protocol.get("msvc_environment"):
        binding = protocol["msvc_environment"]
        env_path = _path(binding["receipt_path"], "msvc_environment_receipt")
        _require(sha256_file(env_path) == binding["receipt_sha256"], "msvc_environment_receipt_changed")
        receipt = _read_json(env_path)
        _require(receipt["identity"] == protocol["toolchain"]["msvc"], "msvc_environment_identity_changed")
        environment.update(receipt["environment"])
    environment.update(CC=compiler, CXX=compiler, CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=linker)
    environment["PATH"] = os.pathsep.join([str(Path(compiler).parent), str(Path(linker).parent), environment.get("PATH", "")])
    tool = config["tool"]
    if tool["kind"] == "sccache":
        environment.update(RUSTC_WRAPPER=tool["path"], SCCACHE_DIR=tool["store"], SCCACHE_CACHE_SIZE=str((1 << 63) - 1), SCCACHE_LOCAL_RW_MODE=tool["local_rw_mode"], SCCACHE_SERVER_PORT=str(tool["server_port"]), SCCACHE_IDLE_TIMEOUT="0")
    elif tool["kind"] == "mbx":
        _require(tool.get("views") is False and tool.get("native_check_lane") is False, "mbx_views_and_native_lane_must_be_off")
        environment.update(CARGO=protocol["toolchain"]["cargo"]["path"], MBX_CACHE_DIR=tool["store"], MBX_SHIMS_DIR=str(Path(tool["store"]) / "shims"),
                           MBX_TARGET_ROOT=str(Path(tool["store"]) / "managed-disabled"), MBX_TARGET_VIEWS="0", MBX_TARGET_LANES="0", MBX_TARGET_SEED="0",
                           MBX_GC_AUTO="0", MBX_INCREMENTAL="1" if feedback or config["incremental"] == "on" else "0", MBX_LEARNED_INCREMENTAL="0",
                           MBX_RESTORE_HARDLINK="0", MBX_DISPLAY="plain", MBX_SUMMARY="off", MBX_SAVINGS="off", MBX_SCHEDULER_CPUS=str(min(12, os.cpu_count() or 1)))
    return environment

# Fixed-width Win32 structures. ctypes handles use pointer-sized HANDLE/SIZE_T.
DWORD = ctypes.c_uint32
ULONG_PTR = ctypes.c_size_t
HANDLE = ctypes.c_void_p


class _BasicLimit(ctypes.Structure):
    _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64), ("LimitFlags", DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t), ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", DWORD), ("Affinity", ULONG_PTR), ("PriorityClass", DWORD), ("SchedulingClass", DWORD)]


class _IOCounters(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount", "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class _ExtendedLimit(ctypes.Structure):
    _fields_ = [("BasicLimitInformation", _BasicLimit), ("IoInfo", _IOCounters), ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t), ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]


class _BasicAccounting(ctypes.Structure):
    _fields_ = [(name, ctypes.c_int64) for name in ("TotalUserTime", "TotalKernelTime", "ThisPeriodTotalUserTime", "ThisPeriodTotalKernelTime")] + [(name, DWORD) for name in ("TotalPageFaultCount", "TotalProcesses", "ActiveProcesses", "TotalTerminatedProcesses")]


class _CompletionPort(ctypes.Structure):
    _fields_ = [("CompletionKey", HANDLE), ("CompletionPort", HANDLE)]


class _StartupInfo(ctypes.Structure):
    _fields_ = [("cb", DWORD), ("lpReserved", wintypes.LPWSTR), ("lpDesktop", wintypes.LPWSTR), ("lpTitle", wintypes.LPWSTR), ("dwX", DWORD), ("dwY", DWORD), ("dwXSize", DWORD), ("dwYSize", DWORD), ("dwXCountChars", DWORD), ("dwYCountChars", DWORD), ("dwFillAttribute", DWORD), ("dwFlags", DWORD), ("wShowWindow", ctypes.c_uint16), ("cbReserved2", ctypes.c_uint16), ("lpReserved2", ctypes.POINTER(ctypes.c_ubyte)), ("hStdInput", HANDLE), ("hStdOutput", HANDLE), ("hStdError", HANDLE)]


class _StartupInfoEx(ctypes.Structure):
    _fields_ = [("StartupInfo", _StartupInfo), ("lpAttributeList", HANDLE)]


class _ProcessInformation(ctypes.Structure):
    _fields_ = [("hProcess", HANDLE), ("hThread", HANDLE), ("dwProcessId", DWORD), ("dwThreadId", DWORD)]


class _ProcessEntry(ctypes.Structure):
    _fields_ = [("dwSize", DWORD), ("cntUsage", DWORD), ("th32ProcessID", DWORD), ("th32DefaultHeapID", ULONG_PTR), ("th32ModuleID", DWORD), ("cntThreads", DWORD), ("th32ParentProcessID", DWORD), ("pcPriClassBase", ctypes.c_int32), ("dwFlags", DWORD), ("szExeFile", ctypes.c_wchar * 260)]


class _FileTime(ctypes.Structure):
    _fields_ = [("low", DWORD), ("high", DWORD)]


class WindowsJob:
    """Own every spawned process before its first instruction, with no breakaway.

    One arm uses one job, including its foreground cache daemon. Process handles
    and completion ports are private, non-inheritable. Only log handles are in
    CreateProcessW's explicit handle list. All descendants are drained before
    returning; kill-on-close also protects abrupt runner death.
    """
    def __init__(self, memory_limit_bytes: int = MEMORY_LIMIT_BYTES):
        _require(os.name == "nt" and ctypes.sizeof(ctypes.c_void_p) == 8, "windows_64bit_job_capture_required")
        _count(memory_limit_bytes, "memory_limit", positive=True)
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        signatures = {
            "CreateJobObjectW": ([HANDLE, wintypes.LPCWSTR], HANDLE),
            "SetInformationJobObject": ([HANDLE, ctypes.c_int, HANDLE, DWORD], wintypes.BOOL),
            "QueryInformationJobObject": ([HANDLE, ctypes.c_int, HANDLE, DWORD, ctypes.POINTER(DWORD)], wintypes.BOOL),
            "AssignProcessToJobObject": ([HANDLE, HANDLE], wintypes.BOOL),
            "IsProcessInJob": ([HANDLE, HANDLE, ctypes.POINTER(wintypes.BOOL)], wintypes.BOOL),
            "OpenProcess": ([DWORD, wintypes.BOOL, DWORD], HANDLE),
            "CloseHandle": ([HANDLE], wintypes.BOOL),
            "TerminateJobObject": ([HANDLE, DWORD], wintypes.BOOL),
            "TerminateProcess": ([HANDLE, DWORD], wintypes.BOOL),
            "CreateIoCompletionPort": ([HANDLE, HANDLE, ULONG_PTR, DWORD], HANDLE),
            "GetQueuedCompletionStatus": ([HANDLE, ctypes.POINTER(DWORD), ctypes.POINTER(ULONG_PTR), ctypes.POINTER(HANDLE), DWORD], wintypes.BOOL),
            "CreateProcessW": ([wintypes.LPCWSTR, wintypes.LPWSTR, HANDLE, HANDLE, wintypes.BOOL, DWORD, HANDLE, wintypes.LPCWSTR, ctypes.POINTER(_StartupInfoEx), ctypes.POINTER(_ProcessInformation)], wintypes.BOOL),
            "InitializeProcThreadAttributeList": ([HANDLE, DWORD, DWORD, ctypes.POINTER(ctypes.c_size_t)], wintypes.BOOL),
            "UpdateProcThreadAttribute": ([HANDLE, DWORD, ULONG_PTR, HANDLE, ctypes.c_size_t, HANDLE, ctypes.POINTER(ctypes.c_size_t)], wintypes.BOOL),
            "DeleteProcThreadAttributeList": ([HANDLE], None),
            "ResumeThread": ([HANDLE], DWORD),
            "WaitForSingleObject": ([HANDLE, DWORD], DWORD),
            "GetExitCodeProcess": ([HANDLE, ctypes.POINTER(DWORD)], wintypes.BOOL),
            "GetProcessTimes": ([HANDLE, ctypes.POINTER(_FileTime), ctypes.POINTER(_FileTime), ctypes.POINTER(_FileTime), ctypes.POINTER(_FileTime)], wintypes.BOOL),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self.kernel, name)
            function.argtypes, function.restype = arguments, result
        self.handle = self.kernel.CreateJobObjectW(None, None)
        _require(bool(self.handle), "create_job_failed")
        self.port = None
        self.processes: list[dict] = []
        self.memory_limit_bytes = memory_limit_bytes
        self.memory_limited = False
        self.peak_commit_bytes = 0
        self.seen_process_ids: set[int] = set()
        try:
            limits = _ExtendedLimit()
            # JOB_MEMORY | KILL_ON_JOB_CLOSE; deliberately no breakaway flag.
            limits.BasicLimitInformation.LimitFlags = 0x200 | 0x2000
            limits.JobMemoryLimit = memory_limit_bytes
            self._check(self.kernel.SetInformationJobObject(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits),), "set_job_limit")
            self.port = self.kernel.CreateIoCompletionPort(HANDLE(-1), None, 0, 1)
            _require(bool(self.port), "create_completion_port_failed")
            associate = _CompletionPort(HANDLE(1), self.port)
            self._check(self.kernel.SetInformationJobObject(self.handle, 7, ctypes.byref(associate), ctypes.sizeof(associate)), "associate_completion_port")
        except BaseException:
            self.close()
            raise

    @staticmethod
    def _check(result: Any, label: str) -> None:
        if not result:
            raise ProtocolError(f"windows_capture_failed:{label}:{ctypes.get_last_error()}")

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, traceback):
        self.close()

    def start(self, argv: list[str], cwd: Path, env: dict[str, str], log_root: Path, label: str) -> dict:
        import msvcrt
        _require(bool(argv) and Path(argv[0]).is_absolute() and Path(argv[0]).is_file(), "absolute_executable_required")
        _require(cwd.is_dir() and log_root.is_dir(), "process_directory_required")
        _require(bool(label) and all(char.isalnum() or char in "-_" for char in label), "invalid_process_label")
        _require(all(isinstance(part, str) and "\0" not in part for part in argv), "invalid_process_argv")
        _require(all(isinstance(key, str) and isinstance(value, str) and key and "=" not in key and "\0" not in key + value for key, value in env.items()), "invalid_process_environment")
        output_path, error_path = log_root / (label + ".stdout"), log_root / (label + ".stderr")
        with output_path.open("xb") as output, error_path.open("xb") as error, open(os.devnull, "rb") as input_file:
            native_handles = [msvcrt.get_osfhandle(handle.fileno()) for handle in (input_file, output, error)]
            attribute_size = ctypes.c_size_t()
            self.kernel.InitializeProcThreadAttributeList(None, 1, 0, ctypes.byref(attribute_size))
            _require(attribute_size.value > 0, "attribute_list_size_unknown")
            attribute_buffer = ctypes.create_string_buffer(attribute_size.value)
            self._check(self.kernel.InitializeProcThreadAttributeList(attribute_buffer, 1, 0, ctypes.byref(attribute_size)), "initialize_handle_list")
            handles = (HANDLE * len(native_handles))(*native_handles)
            information = _ProcessInformation()
            try:
                self._check(self.kernel.UpdateProcThreadAttribute(attribute_buffer, 0, 0x20002, handles, ctypes.sizeof(handles), None, None), "restrict_inherited_handles")
                for handle in native_handles:
                    os.set_handle_inheritable(handle, True)
                startup = _StartupInfoEx()
                startup.StartupInfo.cb = ctypes.sizeof(startup)
                startup.StartupInfo.dwFlags = 0x100 | 1  # std handles + hidden window
                startup.StartupInfo.wShowWindow = 0
                startup.StartupInfo.hStdInput, startup.StartupInfo.hStdOutput, startup.StartupInfo.hStdError = native_handles
                startup.lpAttributeList = ctypes.cast(attribute_buffer, HANDLE)
                command = ctypes.create_unicode_buffer(subprocess.list2cmdline(argv))
                block = ctypes.create_unicode_buffer("\0".join(key + "=" + value for key, value in sorted(env.items(), key=lambda item: item[0].upper())) + "\0\0")
                flags = 0x4 | 0x400 | 0x80000 | 0x08000000  # suspended, unicode, extended startup, no window
                self._check(self.kernel.CreateProcessW(argv[0], command, None, None, True, flags, block, str(cwd), ctypes.byref(startup), ctypes.byref(information)), "create_suspended_process")
                try:
                    self._check(self.kernel.AssignProcessToJobObject(self.handle, information.hProcess), "assign_suspended_process")
                    member = wintypes.BOOL()
                    self._check(self.kernel.IsProcessInJob(information.hProcess, self.handle, ctypes.byref(member)), "verify_job_membership")
                    _require(bool(member.value), "process_job_membership_unknown")
                    _require(self.kernel.ResumeThread(information.hThread) != 0xFFFFFFFF, "resume_process_failed")
                except BaseException:
                    self._check(self.kernel.TerminateProcess(information.hProcess, 0xE001), "terminate_unstarted_process")
                    self.kernel.WaitForSingleObject(information.hProcess, 5000)
                    self.kernel.CloseHandle(information.hProcess)
                    raise
                finally:
                    self.kernel.CloseHandle(information.hThread)
            finally:
                for handle in native_handles:
                    os.set_handle_inheritable(handle, False)
                self.kernel.DeleteProcThreadAttributeList(attribute_buffer)
        process = {"handle": information.hProcess, "pid": int(information.dwProcessId), "argv": argv, "cwd": str(cwd), "stdout_path": str(output_path), "stderr_path": str(error_path), "started_at": _stamp(), "started_monotonic": time.monotonic()}
        self.processes.append(process)
        self.seen_process_ids.add(process["pid"])
        return process

    def poll(self, process: dict) -> int | None:
        code = DWORD()
        self._check(self.kernel.GetExitCodeProcess(process["handle"], ctypes.byref(code)), "process_exit_code")
        return None if code.value == 259 else int(code.value)

    def wait(self, process: dict, seconds: float) -> None:
        result = self.kernel.WaitForSingleObject(process["handle"], max(0, int(seconds * 1000)))
        _require(result in (0, 258), "process_wait_capture_failed")

    def process_wall_ms(self, process: dict) -> float | None:
        created, exited, kernel, user = _FileTime(), _FileTime(), _FileTime(), _FileTime()
        self._check(self.kernel.GetProcessTimes(process["handle"], ctypes.byref(created), ctypes.byref(exited), ctypes.byref(kernel), ctypes.byref(user)), "process_wall_clock")
        start = (created.high << 32) | created.low
        finish = (exited.high << 32) | exited.low
        return (finish - start) / 10000 if finish else None

    def account(self) -> dict:
        limits, accounting = _ExtendedLimit(), _BasicAccounting()
        self._check(self.kernel.QueryInformationJobObject(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits), None), "job_memory_accounting")
        self._check(self.kernel.QueryInformationJobObject(self.handle, 1, ctypes.byref(accounting), ctypes.sizeof(accounting), None), "job_process_accounting")
        _require(limits.JobMemoryLimit == self.memory_limit_bytes and limits.BasicLimitInformation.LimitFlags & 0x2200 == 0x2200, "job_limits_changed")
        self.peak_commit_bytes = max(self.peak_commit_bytes, int(limits.PeakJobMemoryUsed))
        messages = []
        while True:
            message, key, data = DWORD(), ULONG_PTR(), HANDLE()
            result = self.kernel.GetQueuedCompletionStatus(self.port, ctypes.byref(message), ctypes.byref(key), ctypes.byref(data), 0)
            if not result:
                _require(ctypes.get_last_error() == 258, "job_completion_capture_failed")
                break
            messages.append(int(message.value))
            if message.value in (9, 10):  # PROCESS_MEMORY_LIMIT, JOB_MEMORY_LIMIT
                self.memory_limited = True
            if message.value == 6 and data.value:
                self.seen_process_ids.add(int(data.value))
        return {"peak_commit_bytes": self.peak_commit_bytes, "active_processes": int(accounting.ActiveProcesses), "total_processes": int(accounting.TotalProcesses), "memory_limit_bytes": self.memory_limit_bytes, "memory_limited": self.memory_limited, "notifications": messages}

    def members(self) -> set[int]:
        # A fresh bounded buffer handles process growth between size queries.
        for count in (256, 4096, 65536):
            class ProcessList(ctypes.Structure):
                _fields_ = [("assigned", DWORD), ("listed", DWORD), ("ids", ULONG_PTR * count)]
            value = ProcessList()
            result = self.kernel.QueryInformationJobObject(self.handle, 3, ctypes.byref(value), ctypes.sizeof(value), None)
            if result:
                _require(value.listed <= count and value.assigned == value.listed, "incomplete_job_process_list")
                members = {int(value.ids[index]) for index in range(value.listed)}
                self.seen_process_ids.update(members)
                return members
            _require(ctypes.get_last_error() in (122, 234), "job_membership_capture_failed")
        raise ProtocolError("job_process_list_overflow")


    def owns_process_identity(self, identity: dict) -> bool | None:
        """Resolve snapshot races using a handle, creation time and actual job.

        None means the observed process has conclusively exited. Membership
        snapshots cannot prove ownership for descendants born between scans.
        """
        process = self.kernel.OpenProcess(0x1000, False, identity["pid"])
        if not process:
            _require(ctypes.get_last_error() == 87, "build_process_membership_unknown")
            return None
        try:
            created, exited, system, user = _FileTime(), _FileTime(), _FileTime(), _FileTime()
            self._check(self.kernel.GetProcessTimes(process, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(system), ctypes.byref(user)), "build_process_membership_times")
            _require((created.high << 32) | created.low == identity["created_filetime"], "build_process_identity_changed_during_capture")
            if (exited.high << 32) | exited.low:
                return None
            member = wintypes.BOOL()
            self._check(self.kernel.IsProcessInJob(process, self.handle, ctypes.byref(member)), "build_process_membership")
            return bool(member.value)
        finally:
            self.kernel.CloseHandle(process)

    def terminate(self, reason_code: int = 0xE002) -> None:
        self._check(self.kernel.TerminateJobObject(self.handle, reason_code), "terminate_job_tree")
        deadline = time.monotonic() + 5
        while self.account()["active_processes"]:
            _require(time.monotonic() < deadline, "job_tree_drain_failed")
            time.sleep(0.02)
        # ActiveProcesses may reach zero just before every process handle
        # transitions to signaled. The job handle itself is signaled only after
        # all assigned processes have terminated, so wait on that contract
        # before reporting a drained tree to callers.
        remaining_ms = max(0, int((deadline - time.monotonic()) * 1000))
        _require(self.kernel.WaitForSingleObject(self.handle, remaining_ms) == 0,
                 "job_tree_handle_not_signaled")
        for process in self.processes:
            remaining_ms = max(0, int((deadline - time.monotonic()) * 1000))
            _require(self.kernel.WaitForSingleObject(process["handle"], remaining_ms) == 0, "job_process_handle_not_signaled")

    def close(self) -> None:
        if self.handle:
            try:
                if self.account()["active_processes"]:
                    self.terminate()
            finally:
                self.kernel.CloseHandle(self.handle)
                self.handle = None
        for process in self.processes:
            if process["handle"]:
                _require(self.kernel.WaitForSingleObject(process["handle"], 5000) == 0, "process_cleanup_not_complete")
                self.kernel.CloseHandle(process["handle"])
                process["handle"] = None
        if self.port:
            self.kernel.CloseHandle(self.port)
            self.port = None


def build_process_snapshot() -> list[dict]:
    """Read executable path + creation time for every external build process.

    A PID alone is never an identity. Access failure for a matching process is
    unknown capture and blocks admission; a confirmed already-exited PID does
    not become a false active process. No external process is terminated.
    """
    _require(os.name == "nt", "windows_process_capture_required")
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    declarations = {
        "CreateToolhelp32Snapshot": ([DWORD, DWORD], HANDLE), "Process32FirstW": ([HANDLE, ctypes.POINTER(_ProcessEntry)], wintypes.BOOL), "Process32NextW": ([HANDLE, ctypes.POINTER(_ProcessEntry)], wintypes.BOOL),
        "OpenProcess": ([DWORD, wintypes.BOOL, DWORD], HANDLE), "QueryFullProcessImageNameW": ([HANDLE, DWORD, wintypes.LPWSTR, ctypes.POINTER(DWORD)], wintypes.BOOL),
        "GetProcessTimes": ([HANDLE, ctypes.POINTER(_FileTime), ctypes.POINTER(_FileTime), ctypes.POINTER(_FileTime), ctypes.POINTER(_FileTime)], wintypes.BOOL), "CloseHandle": ([HANDLE], wintypes.BOOL),
        "GetExitCodeProcess": ([HANDLE, ctypes.POINTER(DWORD)], wintypes.BOOL),
    }
    for name, (arguments, result) in declarations.items():
        getattr(kernel, name).argtypes, getattr(kernel, name).restype = arguments, result
    handle = kernel.CreateToolhelp32Snapshot(2, 0)
    _require(handle and handle != HANDLE(-1).value, "process_snapshot_failed")
    processes: list[dict] = []
    try:
        entry = _ProcessEntry()
        entry.dwSize = ctypes.sizeof(entry)
        more = kernel.Process32FirstW(handle, ctypes.byref(entry))
        while more:
            if entry.szExeFile.lower() in BUILD_NAMES or entry.szExeFile.lower().startswith(("agent_session_grep_", "agent-session-grep-")):
                process = kernel.OpenProcess(0x1000, False, entry.th32ProcessID)
                if not process:
                    # ERROR_INVALID_PARAMETER proves this process has exited.
                    _require(ctypes.get_last_error() == 87, "build_process_identity_unavailable")
                else:
                    try:
                        image, length = ctypes.create_unicode_buffer(32768), DWORD(32768)
                        available = kernel.QueryFullProcessImageNameW(process, 0, image, ctypes.byref(length))
                        if not available:
                            image_error = ctypes.get_last_error()
                            code = DWORD()
                            # A Toolhelp snapshot can outlive a short Cargo root.
                            # Only a confirmed terminal status permits skipping;
                            # access/query failure remains unknown and blocked.
                            _require(bool(kernel.GetExitCodeProcess(process, ctypes.byref(code))) and code.value != 259, "build_process_image_unknown:" + str(image_error))
                        else:
                            created, exited, system, user = _FileTime(), _FileTime(), _FileTime(), _FileTime()
                            WindowsJob._check(kernel.GetProcessTimes(process, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(system), ctypes.byref(user)), "build_process_creation_time")
                            if not ((exited.high << 32) | exited.low):
                                processes.append({"pid": int(entry.th32ProcessID), "parent_pid": int(entry.th32ParentProcessID), "image": image.value, "created_filetime": (created.high << 32) | created.low})
                    finally:
                        kernel.CloseHandle(process)
            more = kernel.Process32NextW(handle, ctypes.byref(entry))
        _require(ctypes.get_last_error() == 18, "incomplete_process_snapshot")
    finally:
        kernel.CloseHandle(handle)
    return processes


def _external_build_processes(job: WindowsJob) -> list[dict]:
    return [entry for entry in build_process_snapshot() if job.owns_process_identity(entry) is False]


def _artifact_identity(path: Path) -> dict:
    _require(path.is_file() and not path.is_symlink(), "artifact_missing_or_symlink")
    stat = path.stat()
    _require(stat.st_ino != 0, "artifact_file_identity_unknown")
    return {"path": str(path.resolve()), "sha256": sha256_file(path), "bytes": stat.st_size, "device": stat.st_dev, "file_id": stat.st_ino, "modified_ns": stat.st_mtime_ns}


def _parse_cargo_lines(lines: Iterable[str], target_root: Path, source_root: Path, *, verify_artifact_bytes: bool = True) -> dict:
    """Accept artifacts only named by Cargo in this invocation and under target.

    Test text after build-finished is preserved and need not be JSON. Artifacts
    from a fresh warm lane are allowed because Cargo explicitly selected them;
    this never falls back to scanning a target directory for an executable.
    """
    target_root, source_root = target_root.resolve(), source_root.resolve()
    finished, artifacts, diagnostics, errors = None, [], [], []
    for raw in lines:
        if not raw.lstrip().startswith("{"):
            continue
        try:
            message = json.loads(raw)
        except json.JSONDecodeError:
            if finished is None:
                errors.append("malformed_cargo_json")
            continue
        if not isinstance(message, dict) or not isinstance(message.get("reason"), str):
            if finished is None:
                errors.append("unrecognized_cargo_json")
            continue
        if message["reason"] == "build-finished":
            if finished is not None or type(message.get("success")) is not bool:
                errors.append("invalid_build_finished")
            else:
                finished = message["success"]
        elif message["reason"] == "compiler-artifact":
            if finished is not None:
                errors.append("artifact_after_build_finished")
                continue
            filenames = message.get("filenames")
            manifest = message.get("manifest_path")
            if not isinstance(filenames, list) or not all(isinstance(name, str) for name in filenames):
                raise ProtocolError("invalid_artifact_filenames")
            _require(type(message.get("fresh")) is bool and isinstance(message.get("profile"), dict), "invalid_artifact_profile")
            executable = message.get("executable")
            _require(executable is None or isinstance(executable, str), "invalid_artifact_executable")
            names = list(dict.fromkeys(filenames + ([executable] if executable else [])))
            receipts = []
            for name in names:
                path = Path(name)
                _require(path.is_absolute() and _inside(path.resolve(strict=verify_artifact_bytes), target_root), "artifact_outside_invocation_target")
                if verify_artifact_bytes:
                    receipts.append(_artifact_identity(path))
            artifacts.append({"package_id": message.get("package_id"), "manifest_path": manifest, "target": message.get("target"), "profile": message["profile"], "features": message.get("features"), "fresh": message["fresh"], "executable": executable, "files": receipts})
        elif message["reason"] == "compiler-message":
            diagnostic = message.get("message")
            if not isinstance(diagnostic, dict) or not isinstance(diagnostic.get("spans"), list):
                raise ProtocolError("invalid_compiler_diagnostic")
            spans = []
            for span in diagnostic["spans"]:
                _require(isinstance(span, dict) and isinstance(span.get("file_name"), str) and type(span.get("line_start")) is int and type(span.get("column_start")) is int, "invalid_diagnostic_span")
                spans.append({key: span.get(key) for key in ("file_name", "line_start", "line_end", "column_start", "column_end", "is_primary")})
            diagnostics.append({"level": diagnostic.get("level"), "message": diagnostic.get("message"), "code": diagnostic.get("code"), "spans": spans, "rendered": diagnostic.get("rendered")})
    if finished is None:
        errors.append("missing_build_finished")
    return {"build_success": finished is True, "build_finished": finished is not None, "artifacts": artifacts, "diagnostics": diagnostics, "errors": errors}


def parse_cargo_json(stdout: bytes, target_root: Path, source_root: Path) -> dict:
    """Compatibility byte entry point using the same decoder as file proofs."""
    with io.TextIOWrapper(io.BytesIO(stdout), encoding="utf-8", errors="strict") as handle:
        return _parse_cargo_lines(handle, target_root, source_root)


def parse_cargo_json_file(path: Path, target_root: Path, source_root: Path) -> dict:
    """Decode one Cargo line at a time; receipt lists/individual JSON lines grow.

    This avoids a second full log copy. It is not a constant-memory JSON parser:
    the largest line and returned artifacts/diagnostics still occupy memory.
    Test text after build-finished follows exactly the compatibility byte API.
    """
    with path.open("r", encoding="utf-8", errors="strict", newline=None) as handle:
        return _parse_cargo_lines(handle, target_root, source_root)


def capacity_cargo_failure(path: Path, target_root: Path, source_root: Path) -> list[str]:
    """Inspect incomplete cap-stopped logs with the shared decoder.

    Missing build-finished is expected after termination. This entry point
    grants no artifact proof; it rejects recorded quality/protocol failures
    without requiring potentially unfinished target bytes to exist.
    """
    with path.open("r", encoding="utf-8", errors="strict", newline=None) as handle:
        cargo = _parse_cargo_lines(handle, target_root, source_root, verify_artifact_bytes=False)
    failures = [reason for reason in cargo["errors"] if reason != "missing_build_finished"]
    if cargo["build_finished"] and not cargo["build_success"]:
        failures.append("cargo_build_failed_before_capacity_stop")
    if any(entry.get("level") in {"error", "failure-note"} for entry in cargo["diagnostics"]):
        failures.append("compiler_error_before_capacity_stop")
    return list(dict.fromkeys(failures))


MEMORY_ERROR_MARKERS = (b"memory allocation of", b"out of memory", b"not enough memory")


def _file_contains_markers(path: Path, markers: tuple[bytes, ...]) -> bool:
    """Scan bounded blocks, preserving matches split across read boundaries."""
    overlap = max((len(marker) for marker in markers), default=1) - 1
    tail = b""
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(64 * 1024), b""):
            combined = tail + block.lower()
            if any(marker in combined for marker in markers):
                return True
            tail = combined[-overlap:] if overlap else b""
    return False


def verify_artifact_receipt(receipt: dict) -> None:
    path = _path(receipt.get("path"), "artifact")
    current = _artifact_identity(path)
    _require(current == receipt, "artifact_changed_since_invocation")



def _pending_recycle_roots(protocol: dict) -> list[str]:
    """Charge exact signed moved payloads until release is verified.

    Unknown post-move effects require reconciliation; they are never inferred
    as freed bytes. Unrelated recycle items are outside project accounting.
    """
    if protocol.get("schema_version") != 2 or not Path(protocol["run_root"]).exists():
        return []
    helper = _retention_module()
    roots = []
    for target_name in _disposable_paths(protocol):
        target = Path(target_name)
        disposition_root = target.parent / "disposition"
        moved_path = disposition_root / "moved.json"
        if not moved_path.exists():
            if (disposition_root / "move-intent.json").exists() and not target.exists():
                raise ProtocolError("unresolved_recycle_move_requires_reconciliation")
            continue
        moved = _read_json(moved_path)
        helper._verify_signed(moved, protocol)
        seal = helper._verify_seal(moved.get("seal"), protocol)
        _require(seal.get("target") == str(target), "pending_recycle_target_mismatch")
        completed_path = disposition_root / "completed.json"
        if completed_path.exists():
            helper._verify_disposition({"path": str(completed_path), "sha256": sha256_file(completed_path)},
                                       moved["seal"], seal, protocol)
            continue
        item = moved.get("item")
        _require(isinstance(item, dict) and item.get("identity") == seal["tree"]["root_identity"],
                 "pending_recycle_identity_mismatch")
        assert isinstance(item, dict)
        payload = helper._path(item.get("path"))
        _require(helper.tree_manifest(payload) == seal["tree"], "pending_recycle_tree_changed")
        roots.append(str(payload))
    _require(len(roots) == len(set(roots)), "duplicate_pending_recycle_root")
    return roots


def _budget_sample(protocol: dict) -> dict:
    stores = list(protocol["store_roots"])
    if protocol.get("schema_version") == 2:
        # Arbitrarily named raw stdout/receipts and recycled-pending bookkeeping
        # must count even when they are not Cargo-marker discovery artifacts.
        stores.append(protocol["run_root"])
        stores.extend(entry["release_suite_root"] for entry in protocol["qualification_allocations"])
        stores.extend(_pending_recycle_roots(protocol))
    report = _inventory_module().inventory(protocol["inventory_roots"], store_roots=stores, discover=True, **({"minimum_free_bytes": _disk_floor(protocol)} if protocol.get("schema_version") == 2 else {}))
    report["disk_free_bytes"] = _free_bytes()
    if report.get("complete") is not True or report.get("errors") != []:
        receipt_root = Path(protocol["baseline"]["receipt_path"]).parent
        failure_path = receipt_root / ("inventory-admission-failed-" + uuid.uuid4().hex + ".json")
        _write_new(failure_path, report)
        raise ProtocolError("inventory_capture_incomplete:" + str(failure_path))
    _count(report.get("allocated_bytes"), "current_allocated_bytes")
    report["budget"] = _inventory_module().budget_status(protocol["baseline"]["allocated_bytes"], report["allocated_bytes"], report["disk_free_bytes"], 0, **_budget_keywords(protocol))
    registered = _registered_reasons(protocol, report["allocated_bytes"])
    if registered:
        report["budget"]["allowed"] = False
        report["budget"]["reasons"].extend(registered)
    return report



def _memory_status() -> dict:
    class Status(ctypes.Structure):
        _fields_ = [("length", DWORD), ("load", DWORD), *[(name, ctypes.c_uint64) for name in ("physical_total", "physical_available", "commit_limit", "commit_available", "virtual_total", "virtual_available", "extended")]]
    value = Status()
    value.length = ctypes.sizeof(value)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GlobalMemoryStatusEx.argtypes = [ctypes.POINTER(Status)]
    kernel.GlobalMemoryStatusEx.restype = wintypes.BOOL
    _require(bool(kernel.GlobalMemoryStatusEx(ctypes.byref(value))), "system_memory_capture_failed")
    return {name: int(getattr(value, name)) for name in ("physical_total", "physical_available", "commit_limit", "commit_available")}



class _FormalLimits:
    """Registered wall compute and related-growth ceilings across resumes."""
    def __init__(self, protocol: dict, state: dict):
        _, self.registration = _proof_reference(protocol["formal_registration"], "formal_registration")
        self.protocol = protocol
        self.prior_seconds = state.get("registered_compute_seconds", 0.0)
        _require(type(self.prior_seconds) in (int, float) and math.isfinite(self.prior_seconds)
                 and self.prior_seconds >= 0, "registered_compute_accounting_invalid")
        self.started = time.monotonic()

    def elapsed_seconds(self) -> float:
        return self.prior_seconds + max(0.0, time.monotonic() - self.started)

    def reasons(self, allocation: int | None = None) -> list[str]:
        reasons = []
        if self.elapsed_seconds() >= self.registration["maximum_compute_seconds"]:
            reasons.append("registered_compute_limit_reached")
        if allocation is not None and max(0, allocation - self.protocol["baseline"]["allocated_bytes"]) >= self.registration["maximum_storage_bytes"]:
            reasons.append("registered_storage_limit_reached")
        return reasons

    def checkpoint(self, state: dict) -> None:
        state["registered_compute_seconds"] = self.elapsed_seconds()
        state["formal_registration_sha256"] = self.protocol["formal_registration"]["sha256"]


# Owned under the exclusive CampaignLock; never a serialized protocol override.
_FORMAL_LIMITS: dict[str, _FormalLimits] = {}


def _registered_reasons(protocol: dict, allocation: int | None = None) -> list[str]:
    guard = _FORMAL_LIMITS.get(protocol.get("run_root", ""))
    return [] if guard is None else guard.reasons(allocation)


def _checkpoint_formal_state(protocol: dict, state: dict, path: Path) -> None:
    guard = _FORMAL_LIMITS.get(protocol.get("run_root", ""))
    if guard is not None:
        guard.checkpoint(state)
    _write_state(path, state)


class _LiveBudget:
    """Low-cost conservative admission bound, never a full allocation receipt.

    Disk consumption includes unrelated writes. Job I/O is an upper bound on
    writes, rather than NTFS allocation evidence; final quiet handle scans are
    authoritative. The monitor performs no filesystem walk in timed loops.
    """
    def __init__(self, protocol: dict, before: dict, job: WindowsJob, store_bytes: int = 0, config: dict | None = None, *, review_floor_bytes: int | None = None, prior_write_bound_bytes: int = 0, minimum_free_bytes: int | None = None):
        self.protocol, self.before, self.job = protocol, before, job
        self.config = config
        # Pin the admission observation: intervening external writes consume B's
        # remaining allowance before our first owned process starts.
        self.start_free = _count(before.get("disk_free_bytes"), "admission_disk_free_bytes")
        self.floor_free = self.start_free if minimum_free_bytes is None else min(self.start_free, _count(minimum_free_bytes, "prior_minimum_free_bytes"))
        self.prior_write_bound_bytes = _count(prior_write_bound_bytes, "prior_write_bound_bytes")
        self.review_floor_bytes = max(_disk_floor(protocol), _count(_disk_floor(protocol) if review_floor_bytes is None else review_floor_bytes, "review_disk_free_floor"))
        self.io_origin = self._write_bound()
        self.store_bytes = store_bytes
        self.last: dict[str, Any] | None = None

    def _write_bound(self) -> int:
        class Accounting(ctypes.Structure):
            _fields_ = [("basic", _BasicAccounting), ("io", _IOCounters)]
        value = Accounting()
        self.job._check(self.job.kernel.QueryInformationJobObject(self.job.handle, 8, ctypes.byref(value), ctypes.sizeof(value), None), "job_io_accounting")
        # NTFS cluster rounding and file creations may exceed transfer bytes.
        return int(value.io.WriteTransferCount) + int(value.io.WriteOperationCount) * 65536

    def __call__(self) -> dict:
        free = _free_bytes()
        self.floor_free = min(self.floor_free, free)
        physical_growth = max(0, self.start_free - self.floor_free)
        writes = self.prior_write_bound_bytes + max(0, self._write_bound() - self.io_origin)
        growth = max(physical_growth, writes)
        allocated_bound = self.before["allocated_bytes"] + growth
        budget = _inventory_module().budget_status(self.protocol["baseline"]["allocated_bytes"], allocated_bound, free, 0, **_budget_keywords(self.protocol))
        if free < self.review_floor_bytes:
            budget["allowed"] = False
            budget["reasons"].append("budget_review_free_space_floor_exceeded")
        if self.config is not None:
            tool = self.config["tool"]
            if tool["kind"] == "mbx" and self.store_bytes + growth >= tool["store_limit_bytes"]:
                budget["allowed"] = False
                budget["reasons"].append("store_reservation_write_bound_reached")
        if self.protocol.get("schema_version") == 2 and writes >= self.protocol["predicted_growth_bytes"]:
            budget["allowed"] = False
            budget["reasons"].append("owned_output_cap_reached" if self.protocol["phase"] == "capacity" else "arm_reservation_write_bound_reached")
        registered = _registered_reasons(self.protocol, allocated_bound)
        if registered:
            budget["allowed"] = False
            budget["reasons"].extend(registered)
        sample = {"allocated_bytes": allocated_bound, "disk_free_bytes": free, "budget": budget,
                     "allocation_kind": "conservative-disk-and-job-write-bound", "physical_growth_bytes": physical_growth, "job_write_bound_bytes": writes}
        self.last = sample
        return sample

def _check_review(protocol: dict, preview: dict, review_receipt_path: Path | None) -> None:
    _require(preview["allowed"], "admission_denied:" + ",".join(preview["reasons"]))
    registered = _registered_reasons(protocol, preview.get("allocated_bytes"))
    _require(not registered, "admission_denied:" + ",".join(registered))
    if preview["review_required"] or review_receipt_path is not None:
        if review_receipt_path is None:
            raise ProtocolError("budget_review_receipt_required")
        review = _read_json(review_receipt_path)
        _require(review.get("schema_version") == 1 and review.get("approved") is True and review.get("protocol_sha256") == preview["protocol_sha256"], "invalid_budget_review")
        _require(review.get("baseline_receipt_sha256") == protocol["baseline"]["receipt_sha256"], "budget_review_baseline_mismatch")
        _require(type(review.get("max_allocated_bytes")) is int and review["max_allocated_bytes"] >= preview["allocated_bytes"] + protocol["predicted_growth_bytes"], "budget_review_does_not_cover_batch")
        _require(type(review.get("min_disk_free_bytes")) is int and _disk_floor(protocol) <= review["min_disk_free_bytes"] <= preview["disk_free_bytes"] - protocol["predicted_growth_bytes"] - _external_allowance(protocol), "budget_review_free_space_mismatch")



def _daemon_alive_ids(job: WindowsJob, daemon: dict | None) -> set[int]:
    """Foreground Windows executables may include a launcher and server child."""
    if daemon is None:
        return set()
    expected = set(daemon.get("owned_process_ids", [daemon["pid"]]))
    return expected & job.members()

def _wait_capture(job: WindowsJob, process: dict, protocol: dict, *, daemon: dict | None = None, monitor=None, review_cap: int | None = None) -> dict:
    deadline = process["started_monotonic"] + protocol["timeout_seconds"]
    status, reasons, samples, external = "ok", [], [], []
    final_code = None
    try:
        while True:
            accounting = job.account()
            members = job.members()
            outsiders = _external_build_processes(job)
            if outsiders:
                external.extend(outsiders)
                status, reasons = "contaminated", ["external_build_process"]
            if accounting["memory_limited"]:
                status, reasons = "memory_limited", ["job_commit_memory_limit"]
            sample = monitor() if monitor is not None else _budget_sample(protocol)
            samples.append({"at": _stamp(), "allocated_bytes": sample["allocated_bytes"], "disk_free_bytes": sample["disk_free_bytes"], "peak_commit_bytes": accounting["peak_commit_bytes"], "active_processes": accounting["active_processes"],
                            "allocation_kind": sample.get("allocation_kind", "synthetic"), "job_write_bound_bytes": sample.get("job_write_bound_bytes"), "physical_growth_bytes": sample.get("physical_growth_bytes"), "budget_allowed": sample["budget"]["allowed"], "budget_reasons": list(sample["budget"]["reasons"])})
            if not sample["budget"]["allowed"] and status == "ok":
                stop_reasons = [reason for reason in sample["budget"]["reasons"] if reason != "project_review_threshold_reached"]
                status, reasons = ("capacity_limited" if protocol.get("phase") == "capacity" and stop_reasons == ["owned_output_cap_reached"] else "budget_limited"), stop_reasons
            if review_cap is not None and sample["allocated_bytes"] > review_cap:
                status, reasons = "budget_limited", ["budget_review_ceiling_exceeded"]
            if daemon is not None and (job.poll(daemon) is not None or _tcp_listener_pid(daemon["listener_port"]) != daemon["pid"] or not set(daemon.get("owned_process_ids", [daemon["pid"]])) <= members):
                status, reasons = "capture_failed", ["dedicated_cache_server_identity_changed"]
            final_code = job.poll(process)
            if status == "capacity_limited" and final_code is not None and final_code != 0:
                status, reasons = "failed", ["command_nonzero_before_capacity_stop"]
            if status != "ok":
                job.terminate()
                final_code = job.poll(process)
                break
            # The root command cannot finish while an unowned descendant keeps
            # writing. A daemon is the one expected still-active arm member.
            if final_code is not None and members <= _daemon_alive_ids(job, daemon):
                break
            if time.monotonic() >= deadline:
                job.terminate()
                final_code = job.poll(process)
                status, reasons = "timeout", ["process_tree_deadline"]
                break
            remaining = min(protocol["poll_seconds"], max(0, deadline - time.monotonic()))
            if final_code is None:
                job.wait(process, remaining)
            else:
                time.sleep(min(remaining, 0.02))
    except (OSError, ProtocolError) as error:
        job.terminate()
        final_code = job.poll(process)
        status, reasons = "capture_failed", [str(error)]
    accounting = job.account()
    if status in ("capacity_limited", "failed") and (accounting["memory_limited"] or _file_contains_markers(Path(process["stderr_path"]), MEMORY_ERROR_MARKERS)):
        status, reasons = "memory_limited", ["commit_allocation_failed"]
    # A failed allocation is sometimes only observable in stderr/exit code;
    # do not turn an unknown memory failure into successful evidence.
    if status == "ok" and final_code != 0:
        status, reasons = "failed", ["command_nonzero"]
        if accounting["memory_limited"] or _file_contains_markers(Path(process["stderr_path"]), MEMORY_ERROR_MARKERS):
            status, reasons = "memory_limited", ["commit_allocation_failed"]
    wall_ms = job.process_wall_ms(process)
    _require(wall_ms is not None and wall_ms >= 0, "process_wall_clock_unavailable")
    return {"schema_version": 1, "kind": "build-feedback-process", "status": status, "reasons": reasons, "argv": process["argv"], "cwd": process["cwd"], "pid": process["pid"], "started_at": process["started_at"], "finished_at": _stamp(), "duration_ms": wall_ms, "capture_duration_ms": (time.monotonic() - process["started_monotonic"]) * 1000, "returncode": final_code,
            "stdout_path": process["stdout_path"], "stderr_path": process["stderr_path"], "stdout_sha256": sha256_file(Path(process["stdout_path"])), "stderr_sha256": sha256_file(Path(process["stderr_path"])), "peak_commit_bytes": accounting["peak_commit_bytes"], "memory_limit_bytes": accounting["memory_limit_bytes"], "process_tree_capture_complete": status != "capture_failed", "external_processes": external, "resource_samples": samples}


def capture_command(argv: list[str], *, cwd: Path, env: dict[str, str], log_root: Path, label: str, timeout_seconds: float = 30, poll_seconds: float = 0.05, memory_limit_bytes: int = MEMORY_LIMIT_BYTES, monitor=None) -> dict:
    """Native smoke-test entry point; experiments use the same job implementation."""
    with WindowsJob(memory_limit_bytes) as job:
        process = job.start(argv, cwd, env, log_root, label)
        protocol = {"timeout_seconds": timeout_seconds, "poll_seconds": poll_seconds}
        if monitor is None:
            # Synthetic capture has no cache growth; this is never used to admit
            # a Cargo arm or to manufacture a complete inventory receipt.
            monitor = lambda: {"allocated_bytes": 0, "disk_free_bytes": _free_bytes(), "budget": {"allowed": True, "reasons": []}}
        return _wait_capture(job, process, protocol, monitor=monitor)


def _tcp_listener_pid(port: int) -> int | None:
    """Verify the PID that owns the IPv4 loopback listener, not a port guess."""
    _require(os.name == "nt", "tcp_owner_capture_requires_windows")
    library = ctypes.WinDLL("iphlpapi", use_last_error=True)
    library.GetExtendedTcpTable.argtypes = [HANDLE, ctypes.POINTER(DWORD), wintypes.BOOL, DWORD, ctypes.c_int, DWORD]
    library.GetExtendedTcpTable.restype = DWORD
    size = DWORD()
    code = library.GetExtendedTcpTable(None, ctypes.byref(size), False, 2, 3, 0)  # IPv4, OWNER_PID_LISTENER
    _require(code in (0, 122) and size.value >= 4, "tcp_listener_capture_failed")
    for _ in range(3):
        buffer = ctypes.create_string_buffer(size.value)
        code = library.GetExtendedTcpTable(buffer, ctypes.byref(size), False, 2, 3, 0)
        if code == 122:
            continue
        _require(code == 0, "tcp_listener_capture_failed")
        count = DWORD.from_buffer_copy(buffer.raw[:4]).value
        _require(4 + count * 24 <= len(buffer), "tcp_listener_capture_incomplete")
        owners = set()
        for index in range(count):
            row = (DWORD * 6).from_buffer_copy(buffer.raw[4 + index * 24:4 + (index + 1) * 24])
            local_port = socket.ntohs(row[2] & 0xFFFF)
            if local_port == port:
                # A wildcard listener also conflicts with our dedicated port.
                owners.add(int(row[5]))
        _require(len(owners) <= 1, "ambiguous_cache_listener")
        return next(iter(owners), None)
    raise ProtocolError("tcp_listener_capture_changed_repeatedly")


def _verify_cache_prewarm(protocol: dict, config: dict, arm_source: Path) -> None:
    tool = config["tool"]
    if tool["kind"] == "native":
        return
    receipt_path = Path(tool["prewarm_receipt_path"])
    _require(sha256_file(receipt_path) == tool["prewarm_receipt_sha256"], "cache_prewarm_receipt_changed")
    receipt = _read_json(receipt_path)
    _require(receipt.get("schema_version") == 1 and receipt.get("complete") is True and receipt.get("offline") is True and receipt.get("errors") == [], "complete_offline_prewarm_required")
    _require(receipt.get("source_tree") == protocol["source"]["tree"] and receipt.get("source_commit") == protocol["source"]["commit"], "cache_prewarm_source_mismatch")
    _require(receipt.get("source_root") == tool["prewarm_source_root"] and Path(receipt["source_root"]).resolve() != arm_source, "cache_prewarm_must_use_other_worktree")
    _require(receipt.get("tool_sha256") == tool["sha256"] and receipt.get("toolchain_sha256") == canonical_sha256(protocol["toolchain"]) and receipt.get("store") == tool["store"], "cache_prewarm_identity_mismatch")
    duration = receipt.get("cold_store_duration_ms")
    if not isinstance(duration, (float, int)) or isinstance(duration, bool) or not math.isfinite(duration) or duration <= 0:
        raise ProtocolError("cold_store_duration_required")
    jobs = receipt.get("jobs")
    if not isinstance(jobs, list) or [entry.get("job") for entry in jobs if isinstance(entry, dict)] != [entry["name"] for entry in JOBS]:
        raise ProtocolError("six_prewarm_jobs_required")
    _require(all(entry.get("status") == "ok" and entry.get("returncode") == 0 and entry.get("process_tree_capture_complete") is True for entry in jobs), "successful_owned_prewarm_jobs_required")
    _require(receipt.get("jobs_limit") == 2 and receipt.get("test_threads") == 2, "prewarm_parallelism_mismatch")
    _verify_source(Path(tool["prewarm_source_root"]), protocol)
    report = _inventory_module().inventory([], store_roots=[tool["store"]], discover=False, **({"minimum_free_bytes": _disk_floor(protocol)} if protocol.get("schema_version") == 2 else {}))
    _require(report.get("complete") is True and report.get("errors") == [], "store_inventory_incomplete")
    current = _count(report.get("allocated_bytes"), "store_allocated_bytes")
    # The pair forecast includes fresh targets, not just the store. READ_ONLY
    # sccache performs no inserts; MBX writes remain bounded independently by
    # the live write monitor. Both start strictly within their own reservation.
    _require(current < tool["store_limit_bytes"], "store_reservation_blocks_batch")
    if tool["kind"] == "sccache":
        _require(tool["local_rw_mode"] == "READ_ONLY", "sccache_warm_store_read_only_required")
        _require(receipt.get("logical_bytes", 0) < (1 << 63) - 1, "sccache_initial_store_exceeds_lru_ceiling")


def _start_sccache(job: WindowsJob, protocol: dict, config: dict, source: Path, environment: dict, arm_root: Path) -> dict | None:
    tool = config["tool"]
    if tool["kind"] == "native":
        return None
    if tool["kind"] == "mbx":
        # MBX owns a compiler-wrapper tree under each invoking root. It has no
        # separate cache-server listener in this explicit-target trial.
        return None
    _require(tool["kind"] == "sccache" and tool["local_rw_mode"] == "READ_ONLY", "tool_process_attribution_unavailable")
    _require(_tcp_listener_pid(tool["server_port"]) is None, "cache_server_port_in_use")
    # An explicit empty TOML file prevents a user-level remote store/config
    # from silently changing the experiment. All disk options are local env.
    configuration = arm_root / "sccache-local.toml"
    with configuration.open("x", encoding="utf-8") as handle:
        handle.write("# Explicit local experiment; no remote cache backend.\n")
    environment["SCCACHE_CONF"] = str(configuration)
    server_environment = dict(environment, SCCACHE_START_SERVER="1", SCCACHE_NO_DAEMON="1")
    process = job.start([tool["path"]], source, server_environment, arm_root, "cache-server")
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        _require(job.poll(process) is None, "foreground_cache_server_failed")
        owner = _tcp_listener_pid(tool["server_port"])
        if owner is not None:
            _require(owner == process["pid"] and owner in job.members(), "cache_server_not_owned_by_job")
            identities = [entry for entry in build_process_snapshot() if entry["pid"] == owner]
            _require(len(identities) == 1 and Path(identities[0]["image"]).resolve() == Path(tool["path"]).resolve(), "cache_server_image_identity_unknown")
            process["listener_identity"] = identities[0]
            process["owned_process_ids"] = sorted(job.members())
            process["listener_port"] = tool["server_port"]
            return process
        time.sleep(0.1)
    raise ProtocolError("cache_server_listener_not_verified")



def phase_queues(config: dict) -> list[list[dict]]:
    if config["layout"] == "isolated":
        return [[job] for job in JOBS]
    queues: dict[str, list[dict]] = {}
    for job in JOBS:
        queues.setdefault(str(job["lane"]), []).append(job)
    return list(queues.values())


def _completed_capture(job: WindowsJob, process: dict, protocol: dict, status: str, reasons: list[str], samples: list, external: list) -> dict:
    account = job.account()
    final_code = job.poll(process)
    _require(final_code is not None, "process_exit_capture_incomplete")
    if status == "capacity_limited" and (account["memory_limited"] or _file_contains_markers(Path(process["stderr_path"]), MEMORY_ERROR_MARKERS)):
        status, reasons = "memory_limited", ["commit_allocation_failed"]
    if status == "ok" and final_code != 0:
        status, reasons = "failed", ["command_nonzero"]
        if account["memory_limited"] or _file_contains_markers(Path(process["stderr_path"]), MEMORY_ERROR_MARKERS):
            status, reasons = "memory_limited", ["commit_allocation_failed"]
    wall = job.process_wall_ms(process)
    _require(wall is not None and wall >= 0, "process_wall_clock_unavailable")
    return {"schema_version": 1, "kind": "build-feedback-process", "status": status, "reasons": reasons,
            "argv": process["argv"], "cwd": process["cwd"], "pid": process["pid"], "started_at": process["started_at"], "finished_at": _stamp(),
            "duration_ms": wall, "capture_duration_ms": (time.monotonic() - process["started_monotonic"]) * 1000, "returncode": final_code,
            "stdout_path": process["stdout_path"], "stderr_path": process["stderr_path"], "stdout_sha256": sha256_file(Path(process["stdout_path"])),
            "stderr_sha256": sha256_file(Path(process["stderr_path"])), "peak_commit_bytes": account["peak_commit_bytes"],
            "memory_limit_bytes": account["memory_limit_bytes"], "process_tree_capture_complete": status != "capture_failed",
            "external_processes": external, "resource_samples": samples}


def _run_parallel_phase(job: WindowsJob, protocol: dict, config: dict, source: Path, targets_root: Path, log_root: Path,
                        phase: str, environment: dict, monitor, daemon=None, review_cap=None) -> tuple[list[dict], float]:
    """Six simultaneous roots or three serial lane queues in one owning job."""
    queues = phase_queues(config)
    active: dict[int, tuple[dict, dict, Path]] = {}
    all_started: dict[str, tuple[dict, dict, Path]] = {}
    finished: dict[str, dict] = {}
    samples, external = [], []
    start = time.monotonic()
    phase_failure, failure_reasons = "ok", []

    def launch(index: int) -> None:
        fixed = queues[index].pop(0)
        target_mode = "primary" if phase == "primary" or config["incremental"] == "on" else "feedback"
        target = targets_root / target_mode / (fixed["name"] if config["layout"] == "isolated" else fixed["lane"])
        target.mkdir(parents=True, exist_ok=True)
        command = job_command(protocol, fixed, target)
        if config["tool"]["kind"] == "mbx":
            command = [config["tool"]["path"], *command[1:]]
        process = job.start(command, source, environment, log_root, phase + "-" + fixed["name"])
        value = (fixed, process, target)
        active[index] = value
        all_started[fixed["name"]] = value

    try:
        for index in range(len(queues)):
            launch(index)
        while active or not job.members() <= _daemon_alive_ids(job, daemon):
            account = job.account()
            members = job.members()
            outsiders = _external_build_processes(job)
            if outsiders:
                external.extend(outsiders)
                phase_failure, failure_reasons = "contaminated", ["external_build_process"]
            if account["memory_limited"]:
                phase_failure, failure_reasons = "memory_limited", ["job_commit_memory_limit"]
            sample = monitor()
            samples.append({"at": _stamp(), "allocated_bytes": sample["allocated_bytes"], "disk_free_bytes": sample["disk_free_bytes"],
                            "peak_commit_bytes": account["peak_commit_bytes"], "active_processes": account["active_processes"], "active_root_commands": len(active), "allocation_kind": sample.get("allocation_kind", "synthetic"),
                            "job_write_bound_bytes": sample.get("job_write_bound_bytes"), "physical_growth_bytes": sample.get("physical_growth_bytes"), "budget_allowed": sample["budget"]["allowed"], "budget_reasons": list(sample["budget"]["reasons"])})
            if not sample["budget"]["allowed"] and phase_failure == "ok":
                failure_reasons = [reason for reason in sample["budget"]["reasons"] if reason != "project_review_threshold_reached"]
                phase_failure = "capacity_limited" if _full_capacity(protocol) and failure_reasons == ["owned_output_cap_reached"] else "budget_limited"
                if phase_failure == "capacity_limited" and any(job.poll(process) not in (None, 0) for _, process, _ in active.values()):
                    phase_failure, failure_reasons = "failed", ["command_nonzero_before_capacity_stop"]
            if review_cap is not None and sample["allocated_bytes"] > review_cap:
                phase_failure, failure_reasons = "budget_limited", ["budget_review_ceiling_exceeded"]
            if daemon is not None and (job.poll(daemon) is not None or _tcp_listener_pid(daemon["listener_port"]) != daemon["pid"] or not set(daemon.get("owned_process_ids", [daemon["pid"]])) <= members):
                phase_failure, failure_reasons = "capture_failed", ["dedicated_cache_server_identity_changed"]
            if time.monotonic() - start >= protocol["timeout_seconds"]:
                phase_failure, failure_reasons = "timeout", ["process_tree_deadline"]
            if phase_failure != "ok":
                job.terminate()
                break
            completed_lanes = []
            for index, (fixed, process, target) in list(active.items()):
                if job.poll(process) is None:
                    continue
                code = job.poll(process)
                if code != 0:
                    phase_failure, failure_reasons = "failed", ["command_nonzero"]
                    if account["memory_limited"] or _file_contains_markers(Path(process["stderr_path"]), MEMORY_ERROR_MARKERS):
                        phase_failure, failure_reasons = "memory_limited", ["commit_allocation_failed"]
                    job.terminate()
                    break
                finished[fixed["name"]] = {"complete": True}
                del active[index]
                if queues[index]:
                    completed_lanes.append(index)
            if phase_failure != "ok":
                break
            for index in completed_lanes:
                launch(index)
            if active or not job.members() <= _daemon_alive_ids(job, daemon):
                time.sleep(protocol["poll_seconds"])
    except (OSError, ProtocolError, UnicodeError) as error:
        phase_failure, failure_reasons = "capture_failed", [str(error)]
        job.terminate()
    duration = (time.monotonic() - start) * 1000
    records = []
    for fixed in JOBS:
        name = fixed["name"]
        if name not in all_started:
            continue
        _, process, target = all_started[name]
        capture = _completed_capture(job, process, protocol, phase_failure, failure_reasons, list(samples), list(external))
        capture["job"] = name
        if capture["status"] == "capacity_limited":
            try:
                failures = capacity_cargo_failure(Path(capture["stdout_path"]), target, source)
                if failures:
                    capture["status"], capture["reasons"] = "failed", failures
            except (OSError, ProtocolError, UnicodeError) as error:
                capture["status"], capture["reasons"] = "capture_failed", [str(error)]
        if capture["status"] == "ok":
            try:
                cargo = parse_cargo_json_file(Path(capture["stdout_path"]), target, source)
                capture["cargo"] = cargo
                if not cargo["build_success"] or cargo["errors"]:
                    capture["status"], capture["reasons"] = "capture_failed", cargo["errors"] or ["cargo_build_failed"]
            except (OSError, ProtocolError, UnicodeError) as error:
                # Preserve every launched job's immutable receipt even when
                # one invocation cannot validate its artifact/log identity.
                capture["status"], capture["reasons"] = "capture_failed", [str(error)]
        _write_new(log_root / (phase + "-" + name + ".json"), capture)
        records.append(capture)
    return records, duration

def _execution_retention_policy(protocol: dict, arm_finalizer=None) -> None:
    """Real v2 entry must validate the known sibling checker before outputs."""
    _require(arm_finalizer is None or callable(arm_finalizer), "invalid_arm_finalizer")
    if protocol.get("schema_version", 1) == 1:
        _require(arm_finalizer is None, "arm_finalizer_requires_v2_pairs")
        return
    _require(protocol.get("schema_version") == 2, "unsupported_protocol")
    _retention_module().validate_policy(protocol)
    if protocol["phase"] in ("pilot", "formal"):
        _require(callable(arm_finalizer), "v2_paired_run_requires_arm_finalizer_before_allocation")
    else:
        _require(protocol["phase"] == "capacity", "arm_finalizer_requires_v2_pairs")
        if _full_capacity(protocol):
            _require(callable(arm_finalizer), "v2_full_capacity_requires_arm_finalizer_before_allocation")
        else:
            _require(arm_finalizer is None, "arm_finalizer_requires_v2_pairs")


def _declare_trial_output(protocol: dict, target: Path) -> dict:
    """Pin literal absence before target/arm creation; the lease survives reuse."""
    helper = _retention_module()
    policy, _ = helper.validate_policy(protocol)
    target = helper._path(str(target), exists=False)
    _require(str(target) in policy["dedicated_output_roots"] and not target.exists(), "new_target_lease_scope_mismatch")
    lease_root = helper._path(str(Path(protocol["run_root"]) / "leases"), exists=False)
    _require(not _inside(lease_root, target), "output_lease_must_survive_disposal")
    lease_root.mkdir(parents=True, exist_ok=True)
    # A deterministic name refuses a second declaration after unknown effects.
    lease_path = lease_root / ("output-" + hashlib.sha256(str(target).encode("utf-8")).hexdigest() + ".json")
    reference = helper.declare_output(protocol, target, lease_path)
    helper._safe_target(protocol, target, reference)
    return reference


def _run_arm(protocol: dict, arm: str, source: Path, arm_root: Path, review_receipt_path: Path | None) -> dict:
    config = protocol["configs"][arm]
    _require(not arm_root.exists(), "arm_output_identity_already_used")
    targets_root = arm_root / "targets"
    output_lease = _declare_trial_output(protocol, targets_root) if protocol.get("schema_version") == 2 else None
    arm_root.mkdir(parents=True, exist_ok=False)
    targets_root.mkdir()
    log_root = arm_root / "logs"
    log_root.mkdir()
    result = {"schema_version": 1, "kind": "build-feedback-arm", "arm": arm, "status": "running", "reasons": [], "started_at": _stamp(), "source_root": str(source), "source_commit": protocol["source"]["commit"], "source_tree": protocol["source"]["tree"], "config": config, "jobs": [], "feedback_warm_jobs": [], "edit_jobs": [], "target_root": str(targets_root), "peak_commit_bytes": 0}
    if protocol["schema_version"] == 2:
        result.update(schema_version=2, protocol_sha256=canonical_sha256(protocol), source_receipt_sha256=protocol["source"]["receipt_sha256"], toolchain_sha256=canonical_sha256(protocol["toolchain"]), output_lease=output_lease)
    start = time.monotonic()
    edit_original = None
    job, live_monitor = None, None
    review_cap = protocol["baseline"]["allocated_bytes"] + 40 * GIB - 1
    review_floor = _disk_floor(protocol)
    try:
        _verify_source(source, protocol)
        _verify_cache_prewarm(protocol, config, source)
        # No root can be started between known busy-window detection and source
        # admission without being retained as a polluted resource sample.
        _require(not build_process_snapshot(), "quiet_segment_required")
        before = _budget_sample(protocol)
        _check_review(protocol, preview_protocol(protocol, before), review_receipt_path)
        if review_receipt_path is not None:
            review_value = _read_json(review_receipt_path)
            _require(review_value.get("approved") is True and review_value.get("protocol_sha256") == canonical_sha256(protocol) and review_value.get("baseline_receipt_sha256") == protocol["baseline"]["receipt_sha256"], "review_identity_changed")
            review_cap = min(_count(review_value.get("max_allocated_bytes"), "review_allocated_ceiling"), protocol["baseline"]["allocated_bytes"] + 60 * GIB - 1)
            review_floor = _count(review_value.get("min_disk_free_bytes"), "review_disk_free_floor")
        result["allocated_before_bytes"] = before["allocated_bytes"]
        if protocol.get("schema_version") == 2:
            result["disk_free_before_bytes"] = before["disk_free_bytes"]
            result["resource_guard"] = {"kind": "windows-job-and-cumulative-owned-output-cap", "memory_limit_bytes": MEMORY_LIMIT_BYTES,
                                        "owned_output_cap_bytes": protocol["predicted_growth_bytes"],
                                        "minimum_free_bytes": _disk_floor(protocol), "external_growth_allowance_bytes": _external_allowance(protocol)}
        job = WindowsJob()
        memory = _memory_status()
        _require(memory["physical_available"] >= 2 * GIB and memory["commit_available"] >= 2 * GIB, "system_resource_headroom_insufficient")
        environment = tool_environment(protocol, config)
        store_bytes = 0
        if config["tool"]["kind"] != "native":
            store_scan = _inventory_module().inventory([], store_roots=[config["tool"]["store"]], discover=False, **({"minimum_free_bytes": _disk_floor(protocol)} if protocol.get("schema_version") == 2 else {}))
            _require(store_scan.get("complete") is True and store_scan.get("errors") == [], "store_allocation_capture_incomplete")
            store_bytes = _count(store_scan.get("allocated_bytes"), "store_allocated_bytes")
        live_monitor = _LiveBudget(protocol, before, job, store_bytes, config, review_floor_bytes=review_floor)
        if config["tool"]["kind"] == "mbx":
            private_config = log_root / "platform-config"
            (private_config / "mbx").mkdir(parents=True)
            (private_config / "mbx" / "config.toml").write_text("# Explicit isolated local build-cache trial.\n", encoding="utf-8")
            environment["APPDATA"] = str(private_config)
        daemon = _start_sccache(job, protocol, config, source, environment, log_root)
        if daemon is not None:
            result["cache_server_identity"] = daemon["listener_identity"]
        for phase in ("primary", "feedback-warm", "edit"):
            feedback = phase != "primary"
            edited = phase == "edit"
            if feedback:
                private_overrides: dict[str, str] = {key: environment[key] for key in ("SCCACHE_CONF", "APPDATA") if key in environment}
                environment = tool_environment(protocol, config, feedback=True)
                environment.update(private_overrides)
            if edited:
                edit_file = source / protocol["edit"]["path"]
                edit_original = edit_file.read_bytes()
                _require(hashlib.sha256(edit_original).hexdigest() == protocol["edit"]["before_sha256"], "pre_edit_source_changed")
                with edit_file.open("ab") as handle:
                    handle.write(EDIT_SUFFIX)
                    handle.flush()
                    os.fsync(handle.fileno())
                _verify_source(source, protocol, edited=True)
            records = result[{"primary": "jobs", "feedback-warm": "feedback_warm_jobs", "edit": "edit_jobs"}[phase]]
            captures, phase_duration = _run_parallel_phase(job, protocol, config, source, targets_root, log_root, phase, environment, live_monitor, daemon=daemon, review_cap=review_cap)
            records.extend(captures)
            result.setdefault("phase_duration_ms", {})[phase] = phase_duration
            for capture in captures:
                result["peak_commit_bytes"] = max(result["peak_commit_bytes"], capture["peak_commit_bytes"])
            if _full_capacity(protocol):
                failure = next((capture for capture in captures if capture["status"] not in ("ok", "capacity_limited")), None)
                if failure is None:
                    failure = next((capture for capture in captures if capture["status"] == "capacity_limited"), None)
            else:
                failure = next((capture for capture in captures if capture["status"] != "ok"), None)
            if failure is not None:
                result["status"], result["reasons"] = failure["status"], failure["reasons"]
            if len(captures) != len(JOBS) and result["status"] == "running":
                result["status"], result["reasons"] = "capture_failed", ["phase_job_count_incomplete"]
            if result["status"] != "running":
                break
        if result["status"] == "running":
            _verify_source(source, protocol, edited=True)
            result["status"] = "ok"
    except (OSError, ProtocolError, subprocess.SubprocessError, UnicodeError) as error:
        result["status"] = "contaminated" if str(error) == "quiet_segment_required" else "capture_failed"
        result["reasons"] = [str(error)]
    finally:
        if job is not None:
            try:
                account = job.account()
                if account.get("active_processes", 0):
                    job.terminate()
                    account = job.account()
                result["peak_commit_bytes"] = max(result["peak_commit_bytes"], account["peak_commit_bytes"])
                if protocol.get("schema_version") == 2 and live_monitor is not None and hasattr(live_monitor, "io_origin") and hasattr(live_monitor, "floor_free"):
                    result["final_job_write_bound_bytes"] = max(0, live_monitor._write_bound() - live_monitor.io_origin)
                    result["minimum_disk_free_bytes"] = live_monitor.floor_free
                job.close()
                result["process_tree_drained"] = True
            except (OSError, ProtocolError) as error:
                result["status"], result["reasons"] = "capture_failed", [str(error)]
                result["process_tree_drained"] = False
        if edit_original is not None:
            edit_file = source / protocol["edit"]["path"]
            if sha256_file(edit_file) == protocol["edit"]["after_sha256"]:
                edit_file.write_bytes(edit_original)
                result["own_edit_restored"] = True
            else:
                result["status"], result["reasons"] = "capture_failed", ["foreign_source_change_during_edit"]
                result["own_edit_restored"] = False
        try:
            _verify_source(source, protocol)
            result["source_verified"] = True
            after = _budget_sample(protocol)
            result["allocated_after_bytes"] = after["allocated_bytes"]
            if protocol.get("schema_version") == 2:
                result["disk_free_after_bytes"] = after["disk_free_bytes"]
                result["minimum_disk_free_bytes"] = min(result.get("minimum_disk_free_bytes", after["disk_free_bytes"]), after["disk_free_bytes"])
            after_reasons = list(after["budget"]["reasons"] if not after["budget"]["allowed"] else [])
            if after["allocated_bytes"] > review_cap:
                after_reasons.append("budget_review_ceiling_exceeded")
            if after["disk_free_bytes"] < review_floor:
                after_reasons.append("budget_review_free_space_floor_exceeded")
            if after_reasons and result["status"] in (("ok", "capacity_limited") if _full_capacity(protocol) else ("ok",)):
                result["status"], result["reasons"] = "budget_limited", sorted(set(after_reasons))
            result["peak_allocated_bytes"] = max([after["allocated_bytes"], result.get("allocated_before_bytes", 0), *[sample["allocated_bytes"] for entry in result["jobs"] + result["feedback_warm_jobs"] + result["edit_jobs"] for sample in entry["resource_samples"]]])
            result["retained_growth_bytes"] = max(0, after["allocated_bytes"] - result.get("allocated_before_bytes", after["allocated_bytes"]))
            target_report = _inventory_module().inventory([], store_roots=[targets_root], discover=False, **({"minimum_free_bytes": _disk_floor(protocol)} if protocol.get("schema_version") == 2 else {}))
            _require(target_report.get("complete") is True and target_report.get("errors") == [], "target_allocation_capture_incomplete")
            result["target_allocated_bytes"] = _count(target_report.get("allocated_bytes"), "target_allocated_bytes")
            owned_roots = _arm_owned_roots(protocol, result) if _full_capacity(protocol) else [targets_root]
            if config["tool"]["kind"] != "native":
                owned_roots.append(Path(config["tool"]["store"]))
            owned_report = _inventory_module().inventory([], store_roots=owned_roots, discover=False, **({"minimum_free_bytes": _disk_floor(protocol)} if protocol.get("schema_version") == 2 else {}))
            _require(owned_report.get("complete") is True and owned_report.get("errors") == [], "owned_allocation_capture_incomplete")
            result["owned_allocated_bytes"] = _count(owned_report.get("allocated_bytes"), "owned_allocated_bytes")
            if protocol.get("schema_version") == 2:
                if result["owned_allocated_bytes"] > protocol["predicted_growth_bytes"] and result["status"] in ("ok", "capacity_limited"):
                    result["status"], result["reasons"] = "budget_limited", ["arm_reservation_allocation_overrun"]
                if result.get("final_job_write_bound_bytes", 0) >= protocol["predicted_growth_bytes"] and result["status"] == "ok":
                    result["status"], result["reasons"] = "budget_limited", ["unobserved_capacity_guard_overrun"]
            owned_report["storage_scope"] = "owned_targets_and_stores"
            result["owned_allocation_receipt"] = owned_report
            _write_new(arm_root / "owned-allocation.json", owned_report)
            result["owned_allocation_receipt_sha256"] = sha256_file(arm_root / "owned-allocation.json")
            result["source_verified"] = True
            result["allocation_complete"] = True
        except (OSError, ProtocolError, KeyError) as error:
            result["status"], result["reasons"] = "capture_failed", [str(error)]
        result["duration_ms"] = (time.monotonic() - start) * 1000
        result["finished_at"] = _stamp()
        result["primary_duration_ms"] = result.get("phase_duration_ms", {}).get("primary", 0)
        result["edit_duration_ms"] = result.get("phase_duration_ms", {}).get("edit", 0)
        result["feedback_warm_duration_ms"] = result.get("phase_duration_ms", {}).get("feedback-warm", 0)
        result["feedback_incremental"] = "on"
        complete_statuses = ("ok", "capacity_limited") if _full_capacity(protocol) else ("ok",)
        result["capture_complete"] = result["status"] in complete_statuses and result.get("process_tree_drained") is True and result.get("source_verified") is True and result.get("allocation_complete") is True and bool(result["jobs"]) and all(entry["process_tree_capture_complete"] for entry in result["jobs"] + result["feedback_warm_jobs"] + result["edit_jobs"])
        if _full_capacity(protocol):
            result.update(phase="capacity", pair_index=None, attempt=None, performance_evidence=False, completed_valid_pairs=0)
            result["workload_complete"] = result["status"] == "ok" and all(len(result[field]) == len(JOBS) for field in ("jobs", "feedback_warm_jobs", "edit_jobs"))
            result["reservation_eligible"] = False
            result["disposal_eligible"] = result["capture_complete"] and (result["status"] == "capacity_limited" or result["workload_complete"])
        result["artifacts_verified"] = result["status"] == "ok" and all(entry.get("cargo", {}).get("build_success") and not entry.get("cargo", {}).get("errors") for entry in result["jobs"] + result["edit_jobs"])
        result["quiet"] = result["status"] in complete_statuses and all(not entry["external_processes"] for entry in result["jobs"] + result["feedback_warm_jobs"] + result["edit_jobs"])
        if _full_capacity(protocol) and result["status"] in complete_statuses:
            try:
                outsiders = build_process_snapshot()
                if outsiders:
                    result["status"], result["reasons"] = "contaminated", ["external_build_process_after_drain"]
                    result.update(quiet=False, capture_complete=False, disposal_eligible=False, workload_complete=False)
            except (OSError, ProtocolError) as error:
                result["status"], result["reasons"] = "capture_failed", [str(error)]
                result.update(quiet=False, capture_complete=False, disposal_eligible=False, workload_complete=False)
        _write_new(arm_root / "arm.json", result)
    return result


def _qualification_allocation(protocol: dict, arm_record: dict) -> dict:
    _require(protocol.get("schema_version") == 2, "qualification_requires_v2")
    _require(protocol.get("qualification_allocations") == qualification_allocations(protocol),
             "qualification_allocation_contract_changed")
    source, arm = arm_record.get("source_root"), arm_record.get("arm")
    if not isinstance(source, str) or arm not in ("baseline", "candidate"):
        raise ProtocolError("qualification_arm_name_required")
    entries = [entry for entry in protocol["qualification_allocations"] if entry["source_root"] == source and entry["arm"] == arm]
    _require(len(entries) == 1, "qualification_source_allocation_required")
    entry = entries[0]
    expected = Path(protocol["run_root"]) / "capacity" if _full_capacity(protocol) else Path(protocol["run_root"]) / "pairs" / f"pair-{entry['pair_index']:04d}-attempt-{entry['attempt']:02d}" / arm
    _require(arm_record.get("target_root") == str(expected / "targets")
             and arm_record.get("config") == protocol["configs"][arm]
             and arm_record.get("protocol_sha256") == canonical_sha256(protocol), "qualification_arm_identity_changed")
    path = expected / "arm.json"
    _require(canonical_sha256(_read_json(path)) == canonical_sha256(arm_record), "qualification_arm_receipt_changed")
    return entry


def _arm_owned_roots(protocol: dict, arm_record: dict) -> list[Path]:
    target = Path(arm_record["target_root"])
    entries = [entry for entry in protocol.get("qualification_allocations", [])
               if entry["source_root"] == arm_record["source_root"] and entry["arm"] == arm_record["arm"]]
    _require(len(entries) == 1, "qualification_source_allocation_required")
    suite = Path(entries[0]["release_suite_root"])
    roots = [target.parent, suite]
    if arm_record["config"]["tool"]["kind"] != "native":
        roots.append(Path(arm_record["config"]["tool"]["store"]))
    return roots


def release_environment(protocol: dict, arm_record: dict) -> dict[str, str]:
    """Official validator receives approved child-only paths, never profile/wrapper overrides."""
    entry = _qualification_allocation(protocol, arm_record)
    environment = tool_environment(protocol, protocol["configs"][arm_record["arm"]])
    allowed_cargo = {"CARGO_HOME"}
    forbidden_names = {"CC", "CXX", "AR", "CARGO", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "CL", "_CL_", "LINK", "_LINK_",
                       "CRATE_CC_NO_DEFAULTS", "CC_ENABLE_DEBUG_OUTPUT"}
    forbidden_names.update(prefix + "_" + variable for prefix in ("HOST", "TARGET")
                           for variable in ("CC", "CXX", "AR", "CFLAGS", "CXXFLAGS", "LDFLAGS"))
    environment = {key: value for key, value in environment.items()
                   if (not key.upper().startswith(("CARGO_", "RUST", "SCCACHE_", "MBX_", "CCACHE_")) or key.upper() in allowed_cargo)
                   and key.upper() not in forbidden_names
                   and not key.upper().startswith(("LIBSQLITE3", "SQLITE", "SQLCIPHER", "PKG_CONFIG", "CC_", "CXX_", "AR_", "CFLAGS_", "CXXFLAGS_"))}
    environment["PATH"] = os.pathsep.join([str(Path(protocol["toolchain"]["cargo"]["path"]).parent),
                                           str(Path(protocol["toolchain"]["rustc"]["path"]).parent), environment.get("PATH", "")])
    environment.update(CARGO_TARGET_DIR=str(Path(arm_record["target_root"]) / "qualification" / "release"),
                       CARGO_BUILD_TARGET=protocol["toolchain"]["target"], CARGO_BUILD_JOBS="2", CARGO_NET_OFFLINE="true",
                       TEMP=str(Path(entry["release_suite_root"]) / "scratch"), TMP=str(Path(entry["release_suite_root"]) / "scratch"),
                       PYTHONDONTWRITEBYTECODE="1")
    return environment


def qualification_command(protocol: dict, arm_record: dict, qualification: str, *, release_options: dict | None = None,
                          diagnostic_argv: list[str] | None = None) -> list[str]:
    _require(qualification in QUALIFICATION_NAMES, "unknown_qualification")
    allocation = _qualification_allocation(protocol, arm_record)
    target = Path(arm_record["target_root"]) / "qualification" / qualification
    if qualification in ("clippy", "optional_features"):
        command = [protocol["toolchain"]["cargo"]["path"], "clippy" if qualification == "clippy" else "test", "--workspace", "--all-targets"]
        if qualification == "optional_features":
            command.append("--all-features")
        command.extend(["--locked", "--offline", "-j2", "--message-format=json", "--target-dir", str(target)])
        return [*command, "--", "-D", "warnings"] if qualification == "clippy" else [*command, "--", "--test-threads=2"]
    if qualification == "diagnostics":
        approved = approved_diagnostic_argv(protocol, arm_record)
        if diagnostic_argv is not None:
            _require(isinstance(diagnostic_argv, list) and diagnostic_argv == approved,
                     "diagnostic_command_not_independently_approved")
        return approved
    source = Path(arm_record["source_root"])
    validator = source / "scripts" / "evidence" / "core_beta_benchmark.py"
    manifest = _manifest(_read_json(Path(protocol["source"]["receipt_path"])))
    selected = [entry for entry in manifest if entry["path"] == "scripts/evidence/core_beta_benchmark.py"]
    _require(len(selected) == 1 and validator.is_file() and sha256_file(validator) == selected[0]["sha256"]
             and validator.stat().st_size == selected[0]["bytes"], "release_validator_not_frozen")
    if not isinstance(release_options, dict) or set(release_options) != {"disk", "filesystem", "antivirus_state", "sqlite_version", "sqlite_version_source"} or not all(isinstance(value, str) and value.strip() and value != "not_recorded" for value in release_options.values()):
        raise ProtocolError("official_release_environment_metadata_required")
    suite = Path(allocation["release_suite_root"])
    command = [str(Path(sys.executable).resolve()), str(validator), "run", "--profile", "full", "--workspace", str(source),
               "--output-dir", str(suite / "report"), "--baseline-run", "1", "--suite-id", suite.name,
               "--cargo", protocol["toolchain"]["cargo"]["path"], "--rustc", protocol["toolchain"]["rustc"]["path"],
               "--expected-commit", protocol["source"]["commit"]]
    for name, value in release_options.items():
        command.extend(["--" + name.replace("_", "-"), value])
    return command


def approved_diagnostic_argv(protocol: dict, arm_record: dict, *, approval: dict | None = None,
                             verify_live: bool = True) -> list[str]:
    """Resolve an approved exact command or stable Cargo-emitted artifact selector.

    Pair campaigns have one target root per arm, so an approval cannot safely
    repeat a literal absolute path for every allocation. The selector names a
    target-relative artifact and immutable argv tail; the concrete executable
    must still be present in the original Cargo capture and retain its receipt.
    """
    if approval is None:
        _, approval = _retention_module().validate_policy(protocol)
    commands = approval.get("diagnostic_commands")
    key = canonical_sha256(arm_record["config"])
    if not isinstance(commands, dict) or key not in commands:
        raise ProtocolError("diagnostic_command_not_independently_approved")
    selected = commands[key]
    target = Path(arm_record["target_root"])
    if isinstance(selected, list):
        if not (selected and all(isinstance(arg, str) and arg for arg in selected)):
            raise ProtocolError("approved_diagnostic_argv_required")
        argv = [str(arg) for arg in selected]
    elif isinstance(selected, dict):
        relative_value = selected.get("artifact_relative_path")
        arguments = selected.get("args")
        if not (set(selected) == {"artifact_relative_path", "args"}
                and isinstance(relative_value, str) and relative_value
                and isinstance(arguments, list)
                and all(isinstance(arg, str) and arg for arg in arguments)):
            raise ProtocolError("approved_diagnostic_selector_invalid")
        relative = Path(relative_value)
        _require(not relative.is_absolute() and ".." not in relative.parts,
                 "approved_diagnostic_selector_path_invalid")
        executable = target / relative
        _require(_inside(executable, target), "diagnostic_requires_arm_artifact")
        argv = [str(executable), *(str(arg) for arg in arguments)]
    else:
        raise ProtocolError("approved_diagnostic_command_invalid")
    executable = Path(argv[0])
    _require(executable.is_absolute() and _inside(executable, target), "diagnostic_requires_arm_artifact")
    artifacts = [artifact for field in ("jobs", "feedback_warm_jobs", "edit_jobs") for capture in arm_record.get(field, [])
                 for selected in capture.get("cargo", {}).get("artifacts", []) for artifact in selected.get("files", [])]
    matches = [artifact for artifact in artifacts if artifact.get("path") == str(executable)]
    _require(bool(matches), "diagnostic_executable_not_cargo_emitted")
    if verify_live:
        verify_artifact_receipt(matches[-1])
    return argv


def _qualification_prior(protocol: dict, arm_record: dict, directory: Path, prior_captures: Iterable[dict]) -> list[dict]:
    declared = list(prior_captures)
    stored = []
    if directory.exists():
        for path in sorted(directory.glob("*/capture.json")):
            capture = _read_json(path)
            _require(path.parent.name in QUALIFICATION_NAMES, "unknown_existing_qualification")
            stored.append(capture)
    _require(sorted(canonical_sha256(capture) for capture in stored) == sorted(canonical_sha256(capture) for capture in declared),
             "all_previous_qualification_captures_required")
    previous = _count(arm_record.get("final_job_write_bound_bytes"), "prior_arm_write_bound")
    floor = _count(arm_record.get("minimum_disk_free_bytes"), "prior_arm_free_minimum")
    for capture in declared:
        guard = capture.get("resource_guard", {})
        _require(capture.get("protocol_sha256") == canonical_sha256(protocol)
                 and capture.get("source_root") == arm_record["source_root"] and capture.get("arm") == arm_record["arm"]
                 and guard.get("kind") == "windows-job-and-cumulative-owned-output-cap"
                 and guard.get("owned_output_cap_bytes") == protocol["predicted_growth_bytes"]
                 and guard.get("origin_allocated_bytes") == arm_record.get("allocated_before_bytes")
                 and guard.get("origin_disk_free_bytes") == arm_record.get("disk_free_before_bytes")
                 and guard.get("memory_limit_bytes") == MEMORY_LIMIT_BYTES
                 and capture.get("config_sha256") == canonical_sha256(arm_record["config"])
                 and capture.get("process_tree_drained") is True and capture.get("status") == "ok", "qualification_prior_capture_not_verified")
        _require(_count(guard.get("prior_write_bound_bytes"), "prior_write_bound_bytes") >= previous
                 and _count(guard.get("prior_minimum_disk_free_bytes"), "prior_minimum_disk_free_bytes") <= floor,
                 "qualification_prior_cumulative_guard_changed")
        allocation_path, allocation = _proof_reference(capture.get("owned_allocation_receipt"), "qualification_prior_allocation")
        expected_path = directory / capture["qualification"] / "owned-allocation.json"
        _require(Path(allocation_path["path"]) == expected_path and allocation.get("complete") is True and allocation.get("errors") == []
                 and allocation.get("allocated_bytes") == capture.get("owned_allocated_bytes"), "qualification_prior_allocation_invalid")
        previous = max(_count(capture.get("final_job_write_bound_bytes"), "prior_final_write_bound"),
                       _count(capture.get("owned_allocated_bytes"), "prior_owned_allocated_bytes"))
        floor = _count(capture.get("minimum_disk_free_bytes"), "prior_minimum_disk_free_bytes")
        for stream in ("stdout", "stderr"):
            _require(sha256_file(Path(capture[stream + "_path"])) == capture[stream + "_sha256"], "qualification_prior_log_changed")
    return declared


def capture_qualification(protocol: dict, arm_record: dict, qualification: str, *, review_receipt_path: str | Path | None = None,
                          prior_captures: Iterable[dict] = (), release_options: dict | None = None,
                          diagnostic_argv: list[str] | None = None) -> dict:
    """Real guarded qualifier: preserve original admission, cumulative writes and free minima."""
    _validate_protocol(protocol)
    _retention_module().validate_policy(protocol)
    allocation = _qualification_allocation(protocol, arm_record)
    _require(arm_record.get("status") == "ok" and arm_record.get("capture_complete") is True
             and arm_record.get("source_verified") is True and arm_record.get("process_tree_drained") is True,
             "qualification_requires_complete_live_arm")
    source, target = Path(arm_record["source_root"]), Path(arm_record["target_root"])
    review = _path(review_receipt_path, "review_receipt") if review_receipt_path is not None else None
    log_base = target.parent / "qualification-captures"
    prior = _qualification_prior(protocol, arm_record, log_base, prior_captures)
    _require([capture.get("qualification") for capture in prior] == list(QUALIFICATION_NAMES[:len(prior)])
             and len(prior) < len(QUALIFICATION_NAMES) and qualification == QUALIFICATION_NAMES[len(prior)],
             "qualification_order_must_preserve_cumulative_captures")
    _require(protocol.get("msvc_environment") is not None, "verified_msvc_environment_required")
    argv = qualification_command(protocol, arm_record, qualification, release_options=release_options, diagnostic_argv=diagnostic_argv)
    logs = log_base / qualification
    _require(not logs.exists(), "qualification_output_identity_already_used")
    _verify_source(source, protocol)
    _require(not build_process_snapshot(), "quiet_qualification_required")
    before = _budget_sample(protocol)
    review_floor, review_cap = _disk_floor(protocol), protocol["baseline"]["allocated_bytes"] + 40 * GIB - 1
    _require(before["budget"]["allowed"], "qualification_current_budget_refused")
    if review is not None:
        data = _read_json(review)
        _require(data.get("approved") is True and data.get("protocol_sha256") == canonical_sha256(protocol)
                 and data.get("baseline_receipt_sha256") == protocol["baseline"]["receipt_sha256"], "review_identity_changed")
        review_floor = max(review_floor, _count(data.get("min_disk_free_bytes"), "review_disk_free_floor"))
        review_cap = min(_count(data.get("max_allocated_bytes"), "review_allocated_ceiling"), protocol["baseline"]["allocated_bytes"] + 60 * GIB - 1)
    _require(before["allocated_bytes"] <= review_cap and before["disk_free_bytes"] >= review_floor, "qualification_review_ceiling_reached")
    original = {"allocated_bytes": _count(arm_record.get("allocated_before_bytes"), "arm_original_allocated_bytes"),
                "disk_free_bytes": _count(arm_record.get("disk_free_before_bytes"), "arm_original_disk_free_bytes")}
    prior_writes = _count(arm_record.get("final_job_write_bound_bytes"), "arm_final_write_bound")
    minimum_free = min(original["disk_free_bytes"], _count(arm_record.get("minimum_disk_free_bytes"), "arm_minimum_disk_free_bytes"))
    captures = [capture for field in ("jobs", "feedback_warm_jobs", "edit_jobs") for capture in arm_record[field]] + prior
    _require(bool(captures) and all(isinstance(capture.get("resource_samples"), list) and bool(capture["resource_samples"]) for capture in captures),
             "real_qualification_resource_samples_required")
    for capture in captures:
        for sample in capture["resource_samples"]:
            _require(sample.get("allocation_kind") == "conservative-disk-and-job-write-bound", "real_qualification_resource_samples_required")
            prior_writes = max(prior_writes, _count(sample.get("job_write_bound_bytes"), "prior_cumulative_write_bound"))
            minimum_free = min(minimum_free, _count(sample.get("disk_free_bytes"), "prior_capture_disk_free_bytes"))
        if capture in prior:
            prior_writes = max(prior_writes, _count(capture.get("final_job_write_bound_bytes"), "prior_final_write_bound"))
            minimum_free = min(minimum_free, _count(capture.get("minimum_disk_free_bytes"), "prior_minimum_disk_free_bytes"))
    owned = _inventory_module().inventory([], store_roots=_arm_owned_roots(protocol, arm_record), discover=False, minimum_free_bytes=_disk_floor(protocol))
    _require(owned.get("complete") is True and owned.get("errors") == [], "qualification_owned_allocation_incomplete")
    prior_writes = max(prior_writes, _count(owned.get("allocated_bytes"), "qualification_owned_bytes"))
    _require(prior_writes < protocol["predicted_growth_bytes"], "qualification_cumulative_cap_already_reached")
    memory = _memory_status()
    _require(memory["physical_available"] >= 2 * GIB and memory["commit_available"] >= 2 * GIB, "system_resource_headroom_insufficient")
    if qualification == "release":
        environment = release_environment(protocol, arm_record)
        scratch = Path(allocation["release_suite_root"]) / "scratch"
        _require(not scratch.exists(), "release_scratch_identity_already_used")
        scratch.mkdir(parents=True, exist_ok=False)
    else:
        environment = tool_environment(protocol, arm_record["config"], feedback=True)
        if arm_record["config"]["tool"]["kind"] != "native":
            # Qualification is a direct Cargo correctness gate, while cached
            # performance belongs exclusively to the eighteen measured jobs.
            environment = tool_environment(protocol, {**arm_record["config"], "tool": {"kind": "native"}}, feedback=True)
    result = None
    with WindowsJob(MEMORY_LIMIT_BYTES) as job:
        monitor = _LiveBudget(protocol, original, job, config=arm_record["config"], review_floor_bytes=review_floor,
                              prior_write_bound_bytes=prior_writes, minimum_free_bytes=minimum_free)
        first = monitor()
        _require(first["budget"]["allowed"], "qualification_cumulative_budget_refused")
        process = job.start(argv, source, environment, logs, qualification)
        result = _wait_capture(job, process, protocol, monitor=monitor, review_cap=review_cap)
        if job.account()["active_processes"]:
            job.terminate()
        result.update(qualification=qualification, protocol_sha256=canonical_sha256(protocol), source_root=str(source),
                      arm=arm_record["arm"], pair_index=allocation["pair_index"], attempt=allocation["attempt"],
                      config_sha256=canonical_sha256(arm_record["config"]), process_tree_drained=True,
                      final_job_write_bound_bytes=prior_writes + max(0, monitor._write_bound() - monitor.io_origin),
                      minimum_disk_free_bytes=monitor.floor_free, resource_guard={"kind": "windows-job-and-cumulative-owned-output-cap",
                      "memory_limit_bytes": MEMORY_LIMIT_BYTES, "owned_output_cap_bytes": protocol["predicted_growth_bytes"],
                      "minimum_free_bytes": _disk_floor(protocol), "external_growth_allowance_bytes": _external_allowance(protocol),
                      "origin_allocated_bytes": original["allocated_bytes"], "origin_disk_free_bytes": original["disk_free_bytes"],
                      "prior_write_bound_bytes": prior_writes, "prior_minimum_disk_free_bytes": minimum_free})
    result["qualification_budget"] = {**result["resource_guard"],
                                      "final_job_write_bound_bytes": result["final_job_write_bound_bytes"],
                                      "minimum_disk_free_bytes": result["minimum_disk_free_bytes"], "extra_owned_roots": []}
    try:
        if result["status"] == "ok" and qualification in ("clippy", "optional_features"):
            result["cargo"] = parse_cargo_json_file(Path(result["stdout_path"]), target / "qualification" / qualification, source)
            if result["cargo"]["errors"] or not result["cargo"]["build_success"]:
                result["status"], result["reasons"] = "capture_failed", result["cargo"]["errors"] or ["cargo_build_failed"]
        if result["status"] == "capacity_limited" and qualification in ("clippy", "optional_features"):
            failures = capacity_cargo_failure(Path(result["stdout_path"]), target / "qualification" / qualification, source)
            if failures:
                result["status"], result["reasons"] = "failed", failures
        _verify_source(source, protocol)
        outsiders = build_process_snapshot()
        if outsiders:
            result["status"], result["reasons"] = "contaminated", ["external_build_process_after_qualification"]
            result["external_processes"].extend(outsiders)
        final = _inventory_module().inventory([], store_roots=_arm_owned_roots(protocol, arm_record), discover=False, minimum_free_bytes=_disk_floor(protocol))
        _require(final.get("complete") is True and final.get("errors") == [], "qualification_final_allocation_incomplete")
        _write_new(logs / "owned-allocation.json", final)
        result["owned_allocation_receipt"] = {"path": str(logs / "owned-allocation.json"), "sha256": sha256_file(logs / "owned-allocation.json")}
        result["owned_allocated_bytes"] = _count(final.get("allocated_bytes"), "qualification_final_allocated_bytes")
        if result["owned_allocated_bytes"] > protocol["predicted_growth_bytes"] and result["status"] in ("ok", "capacity_limited"):
            result["status"], result["reasons"] = "budget_limited", ["qualification_owned_output_cap_overrun"]
        elif result["status"] == "ok" and result["final_job_write_bound_bytes"] >= protocol["predicted_growth_bytes"]:
            result["status"], result["reasons"] = "budget_limited", ["qualification_unobserved_guard_overrun"]
        result["finished_at"] = _stamp()
    except (OSError, ProtocolError, subprocess.SubprocessError, UnicodeError, KeyError, ValueError) as error:
        result["status"], result["reasons"] = "capture_failed", [str(error)]
    finally:
        result["finished_at"] = _stamp()
        _write_new(logs / "capture.json", result)
    return result


def _run_full_capacity_probe(protocol: dict, probe_root: Path, review_receipt_path: Path | None, arm_finalizer) -> dict:
    _require(_full_capacity(protocol) and callable(arm_finalizer), "full_capacity_finalizer_required")
    probe = protocol["capacity_probe"]
    arm = _run_arm(protocol, probe["arm"], Path(probe["source_root"]), probe_root, review_receipt_path)
    arm_path = probe_root / "arm.json"
    arm_reference = {"path": str(arm_path), "sha256": sha256_file(arm_path)}
    observation = {"schema_version": 2, "kind": "build-feedback-full-capacity-observation", "phase": "capacity", "arm": probe["arm"],
                   "pair_index": None, "attempt": None, "protocol_sha256": canonical_sha256(protocol),
                   "source_receipt_sha256": protocol["source"]["receipt_sha256"], "toolchain_sha256": canonical_sha256(protocol["toolchain"]),
                   "source_root": probe["source_root"], "target_root": arm["target_root"], "config": arm["config"], "arm_reference": arm_reference,
                   "status": arm["status"], "reasons": list(arm["reasons"]), "capture_complete": False, "workload_complete": False,
                   "reservation_eligible": False, "disposal_eligible": False, "performance_evidence": False, "completed_valid_pairs": 0,
                   "started_at": arm["started_at"], "output_lease": arm.get("output_lease")}
    if arm["status"] in ("ok", "capacity_limited") and arm.get("capture_complete") is True:
        try:
            reference = arm_finalizer(protocol=protocol, phase="capacity", arm=probe["arm"], pair_index=None, attempt=None,
                                      arm_receipt=dict(arm_reference), arm_record=dict(arm))
            _require(sha256_file(arm_path) == arm_reference["sha256"] and _read_json(arm_path) == arm,
                     "capacity_finalizer_changed_immutable_arm")
            retained = _retention_module().verify_full_capacity_link(reference, protocol)
            _require(isinstance(retained, dict) and retained.get("arm_receipt") == arm, "full_capacity_seal_does_not_bind_arm")
            seal = retained.get("seal")
            _require(isinstance(seal, dict) and seal.get("kind") == "build-feedback-full-capacity-seal"
                     and seal.get("performance_evidence") is False and seal.get("arm") == probe["arm"]
                     and seal.get("pair_index") is None and seal.get("attempt") is None,
                     "full_capacity_closure_invalid")
            _require(seal.get("status") in ("ok", "capacity_limited") and (arm["status"] == "ok" or seal["status"] == "capacity_limited"),
                     "full_capacity_seal_status_mismatch")
            successful = seal["status"] == "ok"
            _require(seal.get("workload_complete") is successful and seal.get("reservation_eligible") is successful,
                     "full_capacity_reservation_status_mismatch")
            observation["status"] = seal["status"]
            observation["reasons"] = [] if successful else ["owned_output_cap_reached"]
            observation.update(capture_complete=True, workload_complete=successful, reservation_eligible=successful,
                               disposal_eligible=True, retained_evidence=reference,
                               peak_owned_output_bytes=_count(seal.get("observed_peak_bytes"), "full_capacity_sealed_peak", positive=True))
        except (OSError, ProtocolError, ValueError, TypeError, KeyError) as error:
            observation["status"], observation["reasons"] = "capture_failed", [str(error)]
    observation["finished_at"] = _stamp()
    _write_new(probe_root / "capacity.json", observation)
    return observation


def capacity_command(protocol: dict, target: Path) -> list[str]:
    _require(protocol.get("schema_version") == 2 and protocol.get("phase") == "capacity", "capacity_protocol_required")
    probe = protocol["capacity_probe"]
    command = [protocol["toolchain"]["cargo"]["path"], probe["command"], "-p", probe["package"], "--all-targets", "--locked", "--offline", "-j2", "--message-format=json", "--target-dir", str(target)]
    if probe["command"] == "test":
        command.extend(["--", "--test-threads=2"])
    return command


def _run_capacity_probe(protocol: dict, probe_root: Path, review_receipt_path: Path | None) -> dict:
    """One leaf invocation; a bounded capacity observation never qualifies speed."""
    _require(protocol["schema_version"] == 2 and protocol["phase"] == "capacity" and not _full_capacity(protocol), "single_leaf_capacity_protocol_required")
    _require(not probe_root.exists(), "capacity_output_identity_already_used")
    targets, logs = probe_root / "targets", probe_root / "logs"
    output_lease = _declare_trial_output(protocol, targets)
    probe_root.mkdir(parents=True, exist_ok=False)
    targets.mkdir()
    logs.mkdir()
    source = Path(protocol["capacity_probe"]["source_root"])
    config = protocol["configs"]["candidate"]
    result: dict[str, Any] = {"schema_version": 2, "kind": "build-feedback-capacity-observation", "phase": "capacity", "status": "running", "reasons": [], "started_at": _stamp(), "protocol_sha256": canonical_sha256(protocol), "source_root": str(source), "source_commit": protocol["source"]["commit"], "source_tree": protocol["source"]["tree"], "source_receipt_sha256": protocol["source"]["receipt_sha256"], "toolchain_sha256": canonical_sha256(protocol["toolchain"]), "target_root": str(targets), "config": config, "owned_output_cap_bytes": protocol["capacity_probe"]["owned_output_cap_bytes"], "performance_evidence": False, "completed_valid_pairs": 0, "allocation_complete": False, "source_verified": False, "process_tree_drained": False, "jobs": []}
    result["output_lease"] = output_lease
    job, live_monitor = None, None
    before = None
    final_write_bound = 0
    try:
        _verify_source(source, protocol)
        _require(not build_process_snapshot(), "quiet_segment_required")
        before = _budget_sample(protocol)
        _check_review(protocol, preview_protocol(protocol, before), review_receipt_path)
        result["allocated_before_bytes"] = before["allocated_bytes"]
        result["disk_free_before_bytes"] = before["disk_free_bytes"]
        review_floor = _disk_floor(protocol)
        review_cap = protocol["baseline"]["allocated_bytes"] + 40 * GIB - 1
        if review_receipt_path is not None:
            review = _read_json(review_receipt_path)
            review_floor = _count(review.get("min_disk_free_bytes"), "review_disk_free_floor")
            review_cap = min(_count(review.get("max_allocated_bytes"), "review_allocated_ceiling"), protocol["baseline"]["allocated_bytes"] + 60 * GIB - 1)
        memory = _memory_status()
        _require(memory["physical_available"] >= 2 * GIB and memory["commit_available"] >= 2 * GIB, "system_resource_headroom_insufficient")
        job = WindowsJob(MEMORY_LIMIT_BYTES)
        live_monitor = _LiveBudget(protocol, before, job, config=config, review_floor_bytes=review_floor)
        environment = tool_environment(protocol, config, feedback=True)
        process = job.start(capacity_command(protocol, targets), source, environment, logs, "capacity-" + protocol["capacity_probe"]["command"])
        capture = _wait_capture(job, process, protocol, monitor=live_monitor, review_cap=review_cap)
        result["jobs"].append(capture)
        result["status"], result["reasons"] = capture["status"], capture["reasons"]
        if capture["status"] == "capacity_limited":
            failures = capacity_cargo_failure(Path(capture["stdout_path"]), targets, source)
            result["cap_stop_quality_errors"] = failures
            if failures:
                result["status"], result["reasons"] = "failed", failures
        if capture["status"] == "ok":
            cargo = parse_cargo_json_file(Path(capture["stdout_path"]), targets, source)
            capture["cargo"] = cargo
            if not cargo["build_success"] or cargo["errors"]:
                result["status"], result["reasons"] = "capture_failed", cargo["errors"] or ["cargo_build_failed"]
        _write_new(logs / "capture.json", capture)
        result["capture_receipt"] = {"path": str(logs / "capture.json"), "sha256": sha256_file(logs / "capture.json")}
        result["peak_commit_bytes"] = capture["peak_commit_bytes"]
        result["memory_limit_bytes"] = MEMORY_LIMIT_BYTES
    except (OSError, ProtocolError, subprocess.SubprocessError, UnicodeError) as error:
        result["status"] = "contaminated" if str(error) == "quiet_segment_required" else "capture_failed"
        result["reasons"] = [str(error)]
    finally:
        if job is not None:
            try:
                if job.account()["active_processes"]:
                    job.terminate()
                if live_monitor is not None:
                    final_write_bound = max(0, live_monitor._write_bound() - live_monitor.io_origin)
                job.close()
                result["process_tree_drained"] = True
            except (OSError, ProtocolError) as error:
                result["status"], result["reasons"] = "capture_failed", [str(error)]
        try:
            _verify_source(source, protocol)
            result["source_verified"] = True
            report = _inventory_module().inventory([], store_roots=[targets], discover=False, minimum_free_bytes=_disk_floor(protocol))
            _require(report.get("complete") is True and report.get("errors") == [], "capacity_allocation_capture_incomplete")
            target_bytes = _count(report.get("allocated_bytes"), "capacity_target_allocated_bytes")
            _write_new(probe_root / "target-allocation.json", report)
            result["target_allocation_receipt"] = {"path": str(probe_root / "target-allocation.json"), "sha256": sha256_file(probe_root / "target-allocation.json")}
            result["target_allocated_bytes"] = target_bytes
            owned_report = _inventory_module().inventory([], store_roots=[probe_root], discover=False, minimum_free_bytes=_disk_floor(protocol))
            _require(owned_report.get("complete") is True and owned_report.get("errors") == [], "capacity_owned_allocation_capture_incomplete")
            owned = _count(owned_report.get("allocated_bytes"), "capacity_owned_allocated_bytes")
            _write_new(probe_root / "owned-allocation.json", owned_report)
            result["task_output_allocation_receipt"] = {"path": str(probe_root / "owned-allocation.json"), "sha256": sha256_file(probe_root / "owned-allocation.json")}
            result["task_output_allocated_bytes"] = owned
            result["owned_allocation_receipt"] = result["target_allocation_receipt"]
            result["owned_allocated_bytes"] = target_bytes
            result["allocation_complete"] = True
            after = _budget_sample(protocol)
            result["allocated_after_bytes"] = after["allocated_bytes"]
            result["disk_free_after_bytes"] = after["disk_free_bytes"]
            bound = max(final_write_bound, 0 if live_monitor is None or live_monitor.last is None else live_monitor.last["job_write_bound_bytes"])
            result["peak_owned_output_bytes"] = max(owned, bound)
            result["final_job_write_bound_bytes"] = final_write_bound
            result["owned_peak_kind"] = "max-post-drain-allocation-and-job-write-bound"
            result["minimum_disk_free_bytes"] = min(after["disk_free_bytes"], live_monitor.floor_free if live_monitor is not None else after["disk_free_bytes"])
            if owned > result["owned_output_cap_bytes"] and result["status"] in ("ok", "capacity_limited"):
                result["status"], result["reasons"] = "budget_limited", ["owned_output_cap_overrun"]
            elif bound >= result["owned_output_cap_bytes"] and result["status"] == "ok":
                result["status"], result["reasons"] = "budget_limited", ["unobserved_capacity_guard_overrun"]
            elif not after["budget"]["allowed"] and result["status"] == "ok":
                result["status"], result["reasons"] = "budget_limited", after["budget"]["reasons"]
        except (OSError, ProtocolError, KeyError) as error:
            result["status"], result["reasons"] = "capture_failed", [str(error)]
        if result["status"] in ("ok", "capacity_limited"):
            try:
                outsiders = build_process_snapshot()
                if outsiders:
                    result["status"], result["reasons"] = "contaminated", ["external_build_process_after_drain"]
                    result["final_external_processes"] = outsiders
            except (OSError, ProtocolError) as error:
                result["status"], result["reasons"] = "capture_failed", [str(error)]
        result["finished_at"] = _stamp()
        result["capture_complete"] = result["status"] in ("ok", "capacity_limited") and result["process_tree_drained"] and result["source_verified"] and result["allocation_complete"] and all(entry.get("process_tree_capture_complete") for entry in result["jobs"])
        result["artifacts_verified"] = result["status"] == "ok" and bool(result["jobs"]) and all(entry.get("cargo", {}).get("build_success") and not entry.get("cargo", {}).get("errors") for entry in result["jobs"])
        result["disposal_eligible"] = result["capture_complete"] and (result["status"] == "capacity_limited" or result["artifacts_verified"])
        _write_new(probe_root / "capacity.json", result)
    return result


@lru_cache(maxsize=1)
def _retention_module():
    path = Path(__file__).with_name("build_feedback_retention.py")
    _require(path.is_file(), "approved_retention_helper_unavailable")
    spec = importlib.util.spec_from_file_location("build_feedback_runner_retention", path)
    _require(spec is not None and spec.loader is not None, "retention_import_unavailable")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _validate_resume(state: dict, protocol: dict, protocol_hash: str) -> None:
    seal = state.get("integrity_sha256")
    _require(isinstance(seal, str) and seal == canonical_sha256({key: value for key, value in state.items() if key != "integrity_sha256"}), "resume_state_corrupt")
    _require(type(state.get("schema_version")) is int and state.get("schema_version") == protocol.get("schema_version", 1) and state.get("protocol_sha256") == protocol_hash and state.get("phase") == protocol["phase"] and state.get("planned_pairs") == protocol["pairs"], "resume_protocol_mismatch")
    if protocol.get("schema_version") == 2:
        _require(type(state.get("planned_pairs")) is int and type(state.get("completed_valid_pairs")) is int,
                 "resume_pair_counter_types_changed")
    pairs = state.get("pairs")
    if not isinstance(pairs, list):
        raise ProtocolError("resume_pairs_invalid")
    indices, attempts = set(), set()
    for pair in pairs:
        index = _count(pair.get("pair_index"), "resume_pair_index")
        attempt = _count(pair.get("attempt"), "resume_pair_attempt")
        _require(index < protocol["pairs"] and (index, attempt) not in attempts, "duplicate_resume_attempt")
        attempts.add((index, attempt))
        _require(pair.get("order") == protocol["order"][index], "resume_order_changed")
        _require(pair.get("status") in ("ok", "contaminated", "failed", "timeout", "memory_limited", "budget_limited", "capture_failed", "interrupted"), "unknown_resume_status")
        _require(set(pair.get("arms", {})) == set(pair.get("arm_receipts", {})), "resume_arm_receipts_incomplete")
        retained_refs = pair.get("retained_evidence", {})
        _require(isinstance(retained_refs, dict) and set(retained_refs) <= set(pair.get("arms", {})), "resume_retained_scope_invalid")
        if pair.get("status") == "ok":
            _require(index not in indices and set(pair.get("arms", {})) == {"baseline", "candidate"} and all(arm.get("status") == "ok" for arm in pair["arms"].values()), "duplicate_or_incomplete_valid_pair")
            indices.add(index)
        for arm, receipt in pair.get("arm_receipts", {}).items():
            path = _path(receipt.get("path"), "arm_receipt")
            _require(_inside(path, Path(protocol["run_root"])) and sha256_file(path) == receipt.get("sha256"), "resume_arm_receipt_changed")
            _require(_read_json(path) == pair["arms"][arm], "resume_arm_payload_changed")
            retained_reference = pair.get("retained_evidence", {}).get(arm)
            if retained_reference is not None:
                _require(protocol.get("schema_version") == 2 and pair["arms"][arm].get("status") == "ok", "legacy_or_failed_retained_evidence_not_allowed")
                retained = _retention_module().verify_retained_arm(retained_reference, protocol, arm, index, attempt)
                _require(isinstance(retained, dict) and retained.get("arm_receipt") == pair["arms"][arm], "resume_retained_arm_changed")
    _require(indices == set(range(len(indices))), "resume_valid_pair_gap")
    _require(state.get("completed_valid_pairs") == len(indices), "resume_completed_count_changed")
    if protocol["phase"] == "capacity":
        _require(not pairs and state.get("performance_evidence") is False and state.get("completed_valid_pairs") == 0, "capacity_cannot_claim_pairs")
        capacity = state.get("capacity_observation")
        if capacity is not None:
            reference, payload = _proof_reference(state.get("capacity_receipt"), "capacity_receipt")
            expected = Path(protocol["run_root"]) / "capacity" / "capacity.json"
            _require(Path(reference["path"]) == expected and payload == capacity, "resume_capacity_receipt_changed")
            if _full_capacity(protocol):
                probe = protocol["capacity_probe"]
                _require(capacity.get("kind") == "build-feedback-full-capacity-observation"
                         and capacity.get("phase") == "capacity" and capacity.get("performance_evidence") is False
                         and type(capacity.get("completed_valid_pairs")) is int and capacity["completed_valid_pairs"] == 0
                         and "pair_index" in capacity and capacity["pair_index"] is None
                         and "attempt" in capacity and capacity["attempt"] is None
                         and capacity.get("protocol_sha256") == protocol_hash and capacity.get("arm") == probe["arm"]
                         and capacity.get("source_receipt_sha256") == protocol["source"]["receipt_sha256"]
                         and capacity.get("toolchain_sha256") == canonical_sha256(protocol["toolchain"])
                         and capacity.get("source_root") == probe["source_root"]
                         and capacity.get("target_root") == str(expected.parent / "targets")
                         and canonical_sha256(capacity.get("config")) == canonical_sha256(protocol["configs"][probe["arm"]]),
                         "resume_full_capacity_identity_changed")
                if capacity.get("capture_complete") is True:
                    verified = _retention_module().verify_full_capacity_link(capacity.get("retained_evidence"), protocol)
                    _, raw_arm = _proof_reference(capacity.get("arm_reference"), "resume_full_capacity_arm")
                    _require(verified.get("arm_receipt") == raw_arm
                             and capacity.get("arm_reference", {}).get("path") == str(expected.parent / "arm.json")
                             and capacity.get("status") == verified.get("seal", {}).get("status")
                             and capacity.get("workload_complete") is (capacity.get("status") == "ok")
                             and capacity.get("reservation_eligible") is capacity.get("workload_complete")
                             and capacity.get("disposal_eligible") is True
                             and type(capacity.get("peak_owned_output_bytes")) is int
                             and capacity["peak_owned_output_bytes"] == verified.get("seal", {}).get("observed_peak_bytes"),
                             "resume_full_capacity_closure_changed")
        _require(not state.get("complete") or capacity is not None and capacity.get("status") in ("ok", "capacity_limited") and capacity.get("capture_complete") is True, "incomplete_capacity_claims_complete")
    elif state.get("complete") is True or state.get("status") == "complete":
        _require(len(indices) == protocol["pairs"] and state.get("complete") is True and state.get("status") == "complete", "resume_incomplete_campaign_claims_complete")
    if protocol.get("schema_version") == 2 and protocol["phase"] == "formal" and state.get("pairs"):
        compute = state.get("registered_compute_seconds")
        if not isinstance(compute, (int, float)) or isinstance(compute, bool):
            raise ProtocolError("registered_compute_accounting_invalid")
        _require(math.isfinite(compute) and compute >= 0
                 and state.get("formal_registration_sha256") == protocol["formal_registration"]["sha256"],
                 "registered_compute_accounting_invalid")
        retained_seconds = sum((entry.get("duration_ms", 0) / 1000) for pair in pairs
                               for entry in pair.get("arms", {}).values())
        retained_seconds += sum(value / 1000 for pair in pairs for value in pair.get("arm_finalization_duration_ms", {}).values())
        _require(compute >= retained_seconds, "registered_compute_below_raw_durations")
        if state.get("complete") is True:
            _, registration = _proof_reference(protocol["formal_registration"], "formal_registration")
            _require(compute < registration["maximum_compute_seconds"], "registered_compute_limit_reached")
    for receipt in state.get("toolchain_segments", []):
        path = _path(receipt.get("path"), "segment_toolchain_receipt")
        _require(_inside(path, Path(protocol["run_root"])) and sha256_file(path) == receipt.get("sha256"), "resume_segment_toolchain_receipt_changed")
        value = _read_json(path)
        _require(value.get("complete") is True and value.get("protocol_sha256") == protocol_hash, "resume_segment_toolchain_incomplete")


def _verify_segment_toolchain(protocol: dict, run_root: Path) -> dict:
    directory = run_root / "toolchain-segments" / uuid.uuid4().hex
    directory.mkdir(parents=True, exist_ok=False)
    _require(protocol.get("msvc_environment") is not None, "verified_msvc_environment_required")
    environment = tool_environment(protocol, protocol["configs"]["baseline"])
    captures = []
    for name, arguments in (("cargo", ["--version", "--verbose"]), ("rustc", ["--version", "--verbose"]), ("msvc_compiler", []), ("msvc_linker", ["/?"])):
        identity = protocol["toolchain"][name] if name in ("cargo", "rustc") else protocol["toolchain"]["msvc"]["compiler" if name == "msvc_compiler" else "linker"]
        _require(sha256_file(Path(identity["path"])) == identity["sha256"], "segment_toolchain_binary_changed")
        capture = capture_command([identity["path"], *arguments], cwd=Path(protocol["source"]["root"]), env=environment, log_root=directory, label=name, timeout_seconds=10, poll_seconds=0.05)
        output = _read_small_capture(Path(capture["stdout_path"])) + _read_small_capture(Path(capture["stderr_path"]))
        if name in ("cargo", "rustc"):
            observed = output.decode("utf-8", "strict").strip()
            _require(capture["status"] == "ok" and observed == identity["version"].strip(), "segment_toolchain_version_mismatch:" + name)
        else:
            # cl.exe without inputs exits nonzero after displaying its version;
            # linker help may be localized. Exact predeclared banner bytes bind
            # that identity, while resource/pollution failures still block.
            decoded = output.decode("mbcs" if os.name == "nt" else "utf-8", "strict")
            _require(capture["status"] in ("ok", "failed") and identity["version"].strip() in decoded, "segment_msvc_banner_mismatch:" + name)
        captures.append(capture)
    receipt = {"schema_version": 1, "kind": "build-feedback-toolchain-segment", "complete": True, "protocol_sha256": canonical_sha256(protocol), "toolchain_sha256": canonical_sha256(protocol["toolchain"]), "captures": captures, "verified_at": _stamp()}
    _write_new(directory / "toolchain.json", receipt)
    return {"path": str(directory / "toolchain.json"), "sha256": sha256_file(directory / "toolchain.json")}


def _run_protocol_locked(protocol_path: str | Path, *, review_receipt_path: str | Path | None = None, pair_limit: int = 20, initializing: bool = False, arm_finalizer=None) -> dict:
    """Run/resume up to 20 health-segment pairs, preserving a fixed total N.

    No review implies admission only below the soft budget. Only contamination
    can be retried, with at most three predeclared full-pair attempts. A failed,
    timed-out, memory-limited or interrupted attempt remains failing evidence.
    """
    protocol_path = _path(protocol_path, "protocol")
    protocol = _read_json(protocol_path)
    _validate_protocol(protocol)
    _require(os.name == "nt", "windows_capture_required")
    _count(pair_limit, "pair_limit", positive=True)
    _require(pair_limit <= 20, "health_segment_maximum_20_pairs")
    _execution_retention_policy(protocol, arm_finalizer)
    protocol_hash = canonical_sha256(protocol)
    review = _path(review_receipt_path, "review_receipt") if review_receipt_path is not None else None
    root = Path(protocol["run_root"])
    state_path = root / "campaign.json"
    if root.exists() and not initializing:
        _require(state_path.is_file(), "run_root_exists_without_matching_state")
        state = _read_json(state_path)
        _validate_resume(state, protocol, protocol_hash)
        if state.get("inflight") is not None:
            inflight = state["inflight"]
            state["status"] = "interrupted"
            state["reasons"] = ["previous_attempt_incomplete_raw_evidence_preserved"]
            if protocol["phase"] != "capacity":
                state["pairs"].append({"pair_index": inflight["pair_index"], "attempt": inflight["attempt"], "order": protocol["order"][inflight["pair_index"]], "status": "interrupted", "arms": {}, "arm_receipts": {}, "raw_root": inflight["raw_root"]})
            state["inflight"] = None
            _checkpoint_formal_state(protocol, state, state_path)
            return state
        if state["status"] in ("complete", "failed", "interrupted"):
            return state
    else:
        _require(not build_process_snapshot(), "quiet_segment_required")
        preview = preview_protocol(protocol, _budget_sample(protocol))
        _check_review(protocol, preview, review)
        if not initializing:
            root.mkdir(parents=True, exist_ok=False)
        state = {"schema_version": protocol["schema_version"], "kind": "build-feedback-campaign", "protocol_sha256": protocol_hash, "protocol_path": str(protocol_path), "source_commit": protocol["source"]["commit"], "source_tree": protocol["source"]["tree"], "toolchain_sha256": canonical_sha256(protocol["toolchain"]), "phase": protocol["phase"], "planned_pairs": protocol["pairs"], "completed_valid_pairs": 0, "started_at": _stamp(), "status": "ready", "complete": False, "reasons": [], "pairs": [], "inflight": None, "peak_pair_growth_bytes": 0}
        if protocol["phase"] == "capacity":
            state["performance_evidence"] = False
        _checkpoint_formal_state(protocol, state, state_path)
    if protocol.get("schema_version") == 2 and protocol["phase"] == "formal":
        _FORMAL_LIMITS[protocol["run_root"]] = _FormalLimits(protocol, state)
        limit_reasons = _registered_reasons(protocol)
        if limit_reasons:
            state["status"], state["reasons"] = "admission_blocked", limit_reasons
            _checkpoint_formal_state(protocol, state, state_path)
            return state
    try:
        segment_receipt = _verify_segment_toolchain(protocol, root)
    except (OSError, ProtocolError) as error:
        state["status"], state["reasons"] = "admission_blocked", [str(error)]
        _checkpoint_formal_state(protocol, state, state_path)
        return state
    state.setdefault("toolchain_segments", []).append(segment_receipt)
    _checkpoint_formal_state(protocol, state, state_path)
    if protocol["phase"] == "capacity":
        state["performance_evidence"] = False
        state["inflight"] = {"kind": "capacity", "raw_root": str(root / "capacity")}
        _checkpoint_formal_state(protocol, state, state_path)
        observation = (_run_full_capacity_probe(protocol, root / "capacity", review, arm_finalizer)
                       if _full_capacity(protocol) else _run_capacity_probe(protocol, root / "capacity", review))
        state["capacity_observation"] = observation
        state["capacity_receipt"] = {"path": str(root / "capacity" / "capacity.json"), "sha256": sha256_file(root / "capacity" / "capacity.json")}
        state["inflight"] = None
        state["complete"] = observation["capture_complete"] and observation["status"] in ("ok", "capacity_limited")
        state["status"] = "complete" if state["complete"] else "failed"
        state["capacity_status"] = observation["status"]
        state["reasons"] = list(observation["reasons"])
        state["finished_at"] = _stamp()
        _checkpoint_formal_state(protocol, state, state_path)
        return state
    allocation_map = {(entry["pair_index"], entry["attempt"]): entry for entry in protocol["allocations"]}
    segment_count = 0
    while state["completed_valid_pairs"] < protocol["pairs"] and segment_count < pair_limit:
        pair_index = state["completed_valid_pairs"]
        prior = [entry for entry in state["pairs"] if entry["pair_index"] == pair_index]
        _require(all(entry["status"] == "contaminated" for entry in prior), "failed_pair_cannot_be_dropped_on_resume")
        attempt = len(prior)
        allocation = allocation_map.get((pair_index, attempt))
        if allocation is None:
            state["status"], state["reasons"] = "admission_blocked", ["fresh_preallocated_pair_sources_required"]
            break
        if attempt >= 3:
            state["status"], state["reasons"] = "admission_blocked", ["contamination_attempts_exhausted"]
            break
        try:
            _require(not build_process_snapshot(), "quiet_segment_required")
            _check_review(protocol, preview_protocol(protocol, _budget_sample(protocol)), review)
        except ProtocolError as error:
            state["status"], state["reasons"] = "admission_blocked", [str(error)]
            break
        pair_root = root / "pairs" / f"pair-{pair_index:04d}-attempt-{attempt:02d}"
        _require(not pair_root.exists(), "pair_output_identity_already_used")
        state["inflight"] = {"pair_index": pair_index, "attempt": attempt, "raw_root": str(pair_root)}
        state["status"] = "running"
        _checkpoint_formal_state(protocol, state, state_path)
        pair = {"pair_index": pair_index, "attempt": attempt, "order": protocol["order"][pair_index], "started_at": _stamp(), "status": "running", "arms": {}, "arm_receipts": {}}
        for arm in pair["order"]:
            registered_sample = _budget_sample(protocol) if _FORMAL_LIMITS.get(protocol["run_root"]) is not None else None
            limits = _registered_reasons(protocol, None if registered_sample is None else registered_sample["allocated_bytes"])
            if limits:
                pair["status"], pair["finalization_reasons"] = "budget_limited", limits
                break
            result = _run_arm(protocol, arm, Path(allocation[arm + "_source_root"]), pair_root / arm, review)
            pair["arms"][arm] = result
            arm_path = pair_root / arm / "arm.json"
            pair["arm_receipts"][arm] = {"path": str(arm_path), "sha256": sha256_file(arm_path)}
            if result["status"] != "ok":
                pair["status"] = result["status"]
                break
            if arm_finalizer is not None:
                # The coordinator explicitly supplies its approved lifecycle.
                # Returned flags have no authority: the local independent
                # helper must reread and verify the seal/disposition references.
                finalization_started = time.monotonic()
                try:
                    reference = arm_finalizer(protocol=protocol, arm=arm, pair_index=pair_index, attempt=attempt, arm_receipt=dict(pair["arm_receipts"][arm]), arm_record=dict(result))
                    _require(sha256_file(arm_path) == pair["arm_receipts"][arm]["sha256"] and _read_json(arm_path) == result, "arm_finalizer_changed_original_receipt")
                    retained = _retention_module().verify_retained_arm(reference, protocol, arm, pair_index, attempt)
                    _require(isinstance(retained, dict) and retained.get("arm_receipt") == result and isinstance(retained.get("seal"), dict), "arm_finalizer_seal_mismatch")
                    pair.setdefault("retained_evidence", {})[arm] = reference
                    # Keep the validated disposition even if finalization uses
                    # the registered budget; it remains failing trial evidence.
                    limits = _registered_reasons(protocol, _budget_sample(protocol)["allocated_bytes"])
                    if limits:
                        pair["status"], pair["finalization_reasons"] = "budget_limited", limits
                        break
                except (OSError, ValueError, ImportError) as error:
                    pair["status"] = "capture_failed"
                    pair["finalization_reasons"] = [str(error)]
                    break
                finally:
                    pair.setdefault("arm_finalization_duration_ms", {})[arm] = (time.monotonic() - finalization_started) * 1000
        if pair["status"] == "running":
            pair["status"] = "ok"
            state["completed_valid_pairs"] += 1
            growth = sum(max(entry.get("retained_growth_bytes", 0), entry.get("peak_allocated_bytes", 0) - entry.get("allocated_before_bytes", 0)) for entry in pair["arms"].values())
            state["peak_pair_growth_bytes"] = max(state["peak_pair_growth_bytes"], growth)
            if protocol["schema_version"] == 2:
                arm_growth = max(max(entry.get("owned_allocated_bytes", 0), entry.get("peak_allocated_bytes", 0) - entry.get("allocated_before_bytes", 0)) for entry in pair["arms"].values())
                state["peak_active_arm_growth_bytes"] = max(state.get("peak_active_arm_growth_bytes", 0), arm_growth)
            segment_count += 1
        pair["finished_at"] = _stamp()
        _write_new(pair_root / "pair.json", pair)
        state["pairs"].append(pair)
        state["inflight"] = None
        if pair["status"] not in ("ok", "contaminated"):
            state["status"], state["reasons"] = "failed", [pair["status"]]
            _checkpoint_formal_state(protocol, state, state_path)
            return state
        state["status"] = "ready"
        _checkpoint_formal_state(protocol, state, state_path)
    state["complete"] = state["completed_valid_pairs"] == protocol["pairs"]
    if state["complete"]:
        state["status"], state["reasons"] = "complete", []
        state["finished_at"] = _stamp()
    elif state["status"] == "ready":
        state["status"] = "segment_complete"
    if state["completed_valid_pairs"]:
        valid = [pair for pair in state["pairs"] if pair["status"] == "ok"]
        average_ms = sum(sum(arm["duration_ms"] for arm in pair["arms"].values()) + sum(pair.get("arm_finalization_duration_ms", {}).values()) for pair in valid) / len(valid)
        state["projected_total_compute_hours"] = average_ms * protocol["pairs"] / 3_600_000
        state["projected_remaining_compute_hours"] = average_ms * (protocol["pairs"] - len(valid)) / 3_600_000
    _checkpoint_formal_state(protocol, state, state_path)
    return state



class _CampaignLock:
    """A persistent local lock file; Windows releases its byte lock on crash."""
    def __init__(self, path: Path):
        self.path, self.handle = path, None

    def __enter__(self):
        import msvcrt
        self.handle = self.path.open("a+b")
        if self.path.stat().st_size == 0:
            self.handle.write(b"0")
            self.handle.flush()
        self.handle.seek(0)
        try:
            msvcrt.locking(self.handle.fileno(), msvcrt.LK_NBLCK, 1)
        except OSError as error:
            self.handle.close()
            self.handle = None
            raise ProtocolError("campaign_owned_by_other_runner") from error
        return self

    def __exit__(self, exc_type, exc, traceback):
        import msvcrt
        if self.handle is not None:
            self.handle.seek(0)
            msvcrt.locking(self.handle.fileno(), msvcrt.LK_UNLCK, 1)
            self.handle.close()
            self.handle = None


def run_protocol(protocol_path: str | Path, *, review_receipt_path: str | Path | None = None, pair_limit: int = 20, arm_finalizer=None) -> dict:
    """Admit and exclusively run/resume one bounded segment of frozen pairs."""
    protocol_path = _path(protocol_path, "protocol")
    protocol = _read_json(protocol_path)
    _validate_protocol(protocol)
    _require(os.name == "nt", "windows_capture_required")
    _count(pair_limit, "pair_limit", positive=True)
    _require(pair_limit <= 20, "health_segment_maximum_20_pairs")
    _execution_retention_policy(protocol, arm_finalizer)
    review = _path(review_receipt_path, "review_receipt") if review_receipt_path is not None else None
    root = Path(protocol["run_root"])
    initializing = not root.exists()
    if initializing:
        _require(not build_process_snapshot(), "quiet_segment_required")
        quick_free = _free_bytes()
        if quick_free - protocol["predicted_growth_bytes"] - _external_allowance(protocol) < _disk_floor(protocol):
            path = Path(protocol["baseline"]["receipt_path"]).parent / ("admission-disk-refused-" + uuid.uuid4().hex + ".json")
            _write_new(path, {"schema_version": 1, "kind": "build-feedback-admission", "protocol_sha256": canonical_sha256(protocol), "allowed": False,
                              "reason": "projected_disk_free_below_minimum", "disk_free_bytes": quick_free, "predicted_growth_bytes": protocol["predicted_growth_bytes"], "minimum_free_bytes": _disk_floor(protocol), "external_growth_allowance_bytes": _external_allowance(protocol)})
            raise ProtocolError("projected_disk_free_below_minimum:" + str(path))
        _check_review(protocol, preview_protocol(protocol, _budget_sample(protocol)), review)
        root.mkdir(parents=True, exist_ok=False)
    else:
        _require((root / "campaign.json").is_file(), "run_root_exists_without_matching_state")
    with _CampaignLock(root / "runner.lock"):
        try:
            return _run_protocol_locked(protocol_path, review_receipt_path=review, pair_limit=pair_limit, initializing=initializing, **({"arm_finalizer": arm_finalizer} if arm_finalizer is not None else {}))
        finally:
            _FORMAL_LIMITS.pop(protocol["run_root"], None)
