"""Windows-only, fail-closed build experiment runner (Python 3.10+, stdlib).

Public entry points: JOBS, make_protocol(configuration=None, **fields),
preview_protocol(protocol, inventory_report), and run_protocol(protocol_path,
review_receipt_path=None, pair_limit=20). No command creates a Git worktree,
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
import json
import math
import os
import random
import socket
import subprocess
import time
import uuid
from collections.abc import Mapping
from ctypes import wintypes
from datetime import datetime, timezone
from functools import lru_cache
from pathlib import Path
from typing import Any

GIB = 1024 ** 3
MEMORY_LIMIT_BYTES = 24 * GIB
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


def balanced_order(pairs: int, seed: int) -> list[list[str]]:
    _require(pairs in (5, 100), "fixed_pairs_required")
    _count(seed, "seed")
    orders = [["baseline", "candidate"] if index % 2 == 0 else ["candidate", "baseline"] for index in range(pairs)]
    random.Random(seed).shuffle(orders)
    return orders


def cache_reservation_bytes(peak_bytes: int) -> int:
    """ceil(1.25 * measured GiB peak + 1) GiB, with integer arithmetic."""
    _count(peak_bytes, "pilot_peak")
    return ((5 * peak_bytes + 4 * GIB - 1) // (4 * GIB) + 1) * GIB


def make_protocol(configuration: Mapping[str, Any] | None = None, **fields: Any) -> dict:
    data = dict(configuration or {})
    _require(not (set(data) & set(fields)), "duplicate_protocol_field")
    data.update(fields)
    known = {"source_root", "source_commit", "source_tree", "source_receipt_path", "baseline_receipt_path", "run_root", "toolchain", "baseline_config", "candidate_config", "phase", "seed", "pairs", "jobs", "test_threads", "inventory_roots", "store_roots", "predicted_growth_bytes", "allocations", "edit_path", "timeout_seconds", "poll_seconds", "pilot_receipt_path", "pilot_receipt_sha256", "msvc_environment_receipt_path"}
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
    _require(phase in ("pilot", "formal"), "unsupported_phase")
    pairs = 5 if phase == "pilot" else 100
    _require(data.get("pairs", pairs) == pairs and type(data.get("pairs", pairs)) is int, "fixed_pairs_required")
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
        _require(growth >= cache_reservation_bytes(peak), "forecast_below_measured_pilot_margin")
        pilot = {"receipt_path": str(pilot_path), "receipt_sha256": pilot_hash, "peak_pair_growth_bytes": peak}
    protocol = {"schema_version": 1, "kind": "build-feedback-protocol", "created_at": _stamp(), "source": {"root": str(source_root), "commit": commit, "tree": tree, "receipt_path": str(source_receipt), "receipt_sha256": sha256_file(source_receipt), "manifest_sha256": canonical_sha256(manifest), "lock_sha256": by_path["Cargo.lock"]["sha256"], "resources_sha256": canonical_sha256(resources)},
                "baseline": {"receipt_path": str(baseline_receipt), "receipt_sha256": sha256_file(baseline_receipt), "allocated_bytes": baseline_bytes}, "run_root": str(run_root), "toolchain": normalized_chain, "configs": configs,
                "phase": phase, "pairs": pairs, "jobs": 2, "test_threads": 2, "seed": seed, "order": balanced_order(pairs, seed), "inventory_roots": roots, "store_roots": stores,
                "predicted_growth_bytes": growth, "allocations": normalized_allocations, "edit": {"kind": "append-comment-v1", "path": edit_path, "before_sha256": by_path[edit_path]["sha256"], "after_sha256": hashlib.sha256(edit_original + EDIT_SUFFIX).hexdigest()},
                "timeout_seconds": timeout, "poll_seconds": poll, "memory_limit_bytes": MEMORY_LIMIT_BYTES, "pilot": pilot}
    if msvc_environment is not None:
        protocol["msvc_environment"] = msvc_environment
    _verify_source(source_root, protocol)
    for root in allocated_sources:
        _verify_source(root, protocol)
    return protocol


def _validate_protocol(protocol: dict) -> None:
    """Rebuild the protocol from its explicit inputs to reject hand-edited modes."""
    _require(type(protocol.get("schema_version")) is int and protocol.get("schema_version") == 1 and protocol.get("kind") == "build-feedback-protocol", "unsupported_protocol")
    source, baseline = protocol.get("source", {}), protocol.get("baseline", {})
    _require(sha256_file(_path(source.get("receipt_path"), "source_receipt")) == source.get("receipt_sha256"), "source_receipt_changed")
    _require(sha256_file(_path(baseline.get("receipt_path"), "baseline_receipt")) == baseline.get("receipt_sha256"), "baseline_receipt_changed")
    fields = {"source_root": source.get("root"), "source_commit": source.get("commit"), "source_tree": source.get("tree"), "source_receipt_path": source.get("receipt_path"), "baseline_receipt_path": baseline.get("receipt_path"), "run_root": protocol.get("run_root"), "toolchain": protocol.get("toolchain"), "baseline_config": protocol.get("configs", {}).get("baseline"), "candidate_config": protocol.get("configs", {}).get("candidate"),
              **{key: protocol.get(key) for key in ("phase", "seed", "pairs", "jobs", "test_threads", "inventory_roots", "store_roots", "predicted_growth_bytes", "allocations", "timeout_seconds", "poll_seconds")}, "edit_path": protocol.get("edit", {}).get("path")}
    if protocol.get("msvc_environment"):
        bound_env = protocol["msvc_environment"]
        _require(sha256_file(_path(bound_env.get("receipt_path"), "msvc_environment_receipt")) == bound_env.get("receipt_sha256"), "msvc_environment_receipt_changed")
        fields["msvc_environment_receipt_path"] = bound_env["receipt_path"]
    if protocol.get("pilot"):
        fields.update(pilot_receipt_path=protocol["pilot"].get("receipt_path"), pilot_receipt_sha256=protocol["pilot"].get("receipt_sha256"))
    rebuilt = make_protocol(**fields)
    rebuilt["created_at"] = protocol.get("created_at")
    _require(rebuilt == protocol, "protocol_contract_changed")


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
        budget = _inventory_module().budget_status(protocol["baseline"]["allocated_bytes"], current, free, protocol["predicted_growth_bytes"])
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
    if not any(_inside(Path(protocol["run_root"]), Path(root)) for root in protocol["inventory_roots"]):
        reasons.append("trial_run_root_not_in_inventory_scope")
    if not protocol["allocations"]:
        reasons.append("preallocated_sources_required")
    for config in protocol["configs"].values():
        if config["tool"]["kind"] == "sccache" and config["tool"].get("local_rw_mode") != "READ_ONLY":
            reasons.append("sccache_warm_store_read_only_required")
    return {"schema_version": 1, "kind": "build-feedback-preview", "created_at": _stamp(), "protocol_sha256": canonical_sha256(protocol), "inventory_sha256": canonical_sha256(inventory_report), "allowed": not reasons, "review_required": bool(budget and budget["review_required"]), "reasons": sorted(set(reasons)), "budget": budget,
            "allocated_bytes": current, "disk_free_bytes": free, "predicted_growth_bytes": protocol["predicted_growth_bytes"], "run_root": protocol["run_root"], "planned_pairs": protocol["pairs"], "phase": protocol["phase"], "disposable_target_paths": [str(Path(protocol["run_root"]) / "pairs" / f"pair-{allocation['pair_index']:04d}-attempt-{allocation['attempt']:02d}" / arm / "targets") for allocation in protocol["allocations"] for arm in ("baseline", "candidate")]}


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


def parse_cargo_json(stdout: bytes, target_root: Path, source_root: Path) -> dict:
    """Accept artifacts only named by Cargo in this invocation and under target.

    Test text after build-finished is preserved and need not be JSON. Artifacts
    from a fresh warm lane are allowed because Cargo explicitly selected them;
    this never falls back to scanning a target directory for an executable.
    """
    target_root, source_root = target_root.resolve(), source_root.resolve()
    finished, artifacts, diagnostics, errors = None, [], [], []
    for raw in stdout.decode("utf-8", "strict").splitlines():
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
                _require(path.is_absolute() and _inside(path.resolve(strict=True), target_root), "artifact_outside_invocation_target")
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


def verify_artifact_receipt(receipt: dict) -> None:
    path = _path(receipt.get("path"), "artifact")
    current = _artifact_identity(path)
    _require(current == receipt, "artifact_changed_since_invocation")


def _budget_sample(protocol: dict) -> dict:
    report = _inventory_module().inventory(protocol["inventory_roots"], store_roots=protocol["store_roots"], discover=True)
    report["disk_free_bytes"] = _free_bytes()
    if report.get("complete") is not True or report.get("errors") != []:
        receipt_root = Path(protocol["baseline"]["receipt_path"]).parent
        failure_path = receipt_root / ("inventory-admission-failed-" + uuid.uuid4().hex + ".json")
        _write_new(failure_path, report)
        raise ProtocolError("inventory_capture_incomplete:" + str(failure_path))
    _count(report.get("allocated_bytes"), "current_allocated_bytes")
    report["budget"] = _inventory_module().budget_status(protocol["baseline"]["allocated_bytes"], report["allocated_bytes"], report["disk_free_bytes"], 0)
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


class _LiveBudget:
    """Low-cost conservative admission bound, never a full allocation receipt.

    Disk consumption includes unrelated writes. Job I/O is an upper bound on
    writes, rather than NTFS allocation evidence; final quiet handle scans are
    authoritative. The monitor performs no filesystem walk in timed loops.
    """
    def __init__(self, protocol: dict, before: dict, job: WindowsJob, store_bytes: int = 0, config: dict | None = None, *, review_floor_bytes: int = 80 * GIB):
        self.protocol, self.before, self.job = protocol, before, job
        self.config = config
        # Pin the admission observation: intervening external writes consume B's
        # remaining allowance before our first owned process starts.
        self.start_free = _count(before.get("disk_free_bytes"), "admission_disk_free_bytes")
        self.floor_free = self.start_free
        self.review_floor_bytes = max(80 * GIB, _count(review_floor_bytes, "review_disk_free_floor"))
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
        writes = max(0, self._write_bound() - self.io_origin)
        growth = max(physical_growth, writes)
        allocated_bound = self.before["allocated_bytes"] + growth
        budget = _inventory_module().budget_status(self.protocol["baseline"]["allocated_bytes"], allocated_bound, free, 0)
        if free < self.review_floor_bytes:
            budget["allowed"] = False
            budget["reasons"].append("budget_review_free_space_floor_exceeded")
        if self.config is not None:
            tool = self.config["tool"]
            if tool["kind"] == "mbx" and self.store_bytes + growth >= tool["store_limit_bytes"]:
                budget["allowed"] = False
                budget["reasons"].append("store_reservation_write_bound_reached")
        sample = {"allocated_bytes": allocated_bound, "disk_free_bytes": free, "budget": budget,
                     "allocation_kind": "conservative-disk-and-job-write-bound", "physical_growth_bytes": physical_growth, "job_write_bound_bytes": writes}
        self.last = sample
        return sample

def _check_review(protocol: dict, preview: dict, review_receipt_path: Path | None) -> None:
    _require(preview["allowed"], "admission_denied:" + ",".join(preview["reasons"]))
    if preview["review_required"] or review_receipt_path is not None:
        if review_receipt_path is None:
            raise ProtocolError("budget_review_receipt_required")
        review = _read_json(review_receipt_path)
        _require(review.get("schema_version") == 1 and review.get("approved") is True and review.get("protocol_sha256") == preview["protocol_sha256"], "invalid_budget_review")
        _require(review.get("baseline_receipt_sha256") == protocol["baseline"]["receipt_sha256"], "budget_review_baseline_mismatch")
        _require(type(review.get("max_allocated_bytes")) is int and review["max_allocated_bytes"] >= preview["allocated_bytes"] + protocol["predicted_growth_bytes"], "budget_review_does_not_cover_batch")
        _require(type(review.get("min_disk_free_bytes")) is int and 80 * GIB <= review["min_disk_free_bytes"] <= preview["disk_free_bytes"] - protocol["predicted_growth_bytes"], "budget_review_free_space_mismatch")



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
            samples.append({"at": _stamp(), "allocated_bytes": sample["allocated_bytes"], "disk_free_bytes": sample["disk_free_bytes"], "peak_commit_bytes": accounting["peak_commit_bytes"], "active_processes": accounting["active_processes"]})
            if not sample["budget"]["allowed"]:
                status, reasons = "budget_limited", list(sample["budget"]["reasons"])
            if review_cap is not None and sample["allocated_bytes"] > review_cap:
                status, reasons = "budget_limited", ["budget_review_ceiling_exceeded"]
            if daemon is not None and (job.poll(daemon) is not None or _tcp_listener_pid(daemon["listener_port"]) != daemon["pid"] or not set(daemon.get("owned_process_ids", [daemon["pid"]])) <= members):
                status, reasons = "capture_failed", ["dedicated_cache_server_identity_changed"]
            final_code = job.poll(process)
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
    # A failed allocation is sometimes only observable in stderr/exit code;
    # do not turn an unknown memory failure into successful evidence.
    stderr = Path(process["stderr_path"]).read_bytes()
    if status == "ok" and final_code != 0:
        status, reasons = "failed", ["command_nonzero"]
        if accounting["memory_limited"] or any(marker in stderr.lower() for marker in (b"memory allocation of", b"out of memory", b"not enough memory")):
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
    report = _inventory_module().inventory([], store_roots=[tool["store"]], discover=False)
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
    stderr = Path(process["stderr_path"]).read_bytes()
    if status == "ok" and final_code != 0:
        status, reasons = "failed", ["command_nonzero"]
        if account["memory_limited"] or any(marker in stderr.lower() for marker in (b"memory allocation of", b"out of memory", b"not enough memory")):
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
                            "peak_commit_bytes": account["peak_commit_bytes"], "active_processes": account["active_processes"], "active_root_commands": len(active), "allocation_kind": sample.get("allocation_kind", "synthetic")})
            if not sample["budget"]["allowed"]:
                phase_failure, failure_reasons = "budget_limited", list(sample["budget"]["reasons"])
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
                    stderr = Path(process["stderr_path"]).read_bytes()
                    phase_failure, failure_reasons = "failed", ["command_nonzero"]
                    if account["memory_limited"] or any(marker in stderr.lower() for marker in (b"memory allocation of", b"out of memory", b"not enough memory")):
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
        if capture["status"] == "ok":
            try:
                cargo = parse_cargo_json(Path(capture["stdout_path"]).read_bytes(), target, source)
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

def _run_arm(protocol: dict, arm: str, source: Path, arm_root: Path, review_receipt_path: Path | None) -> dict:
    config = protocol["configs"][arm]
    _require(not arm_root.exists(), "arm_output_identity_already_used")
    arm_root.mkdir(parents=True, exist_ok=False)
    targets_root = arm_root / "targets"
    targets_root.mkdir()
    log_root = arm_root / "logs"
    log_root.mkdir()
    result = {"schema_version": 1, "kind": "build-feedback-arm", "arm": arm, "status": "running", "reasons": [], "started_at": _stamp(), "source_root": str(source), "source_commit": protocol["source"]["commit"], "source_tree": protocol["source"]["tree"], "config": config, "jobs": [], "feedback_warm_jobs": [], "edit_jobs": [], "target_root": str(targets_root), "peak_commit_bytes": 0}
    start = time.monotonic()
    edit_original = None
    job = None
    review_cap = protocol["baseline"]["allocated_bytes"] + 40 * GIB - 1
    review_floor = 80 * GIB
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
        job = WindowsJob()
        memory = _memory_status()
        _require(memory["physical_available"] >= 2 * GIB and memory["commit_available"] >= 2 * GIB, "system_resource_headroom_insufficient")
        environment = tool_environment(protocol, config)
        store_bytes = 0
        if config["tool"]["kind"] != "native":
            store_scan = _inventory_module().inventory([], store_roots=[config["tool"]["store"]], discover=False)
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
                if capture["status"] != "ok":
                    result["status"], result["reasons"] = capture["status"], capture["reasons"]
                    break
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
                result["peak_commit_bytes"] = max(result["peak_commit_bytes"], account["peak_commit_bytes"])
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
            after_reasons = list(after["budget"]["reasons"] if not after["budget"]["allowed"] else [])
            if after["allocated_bytes"] > review_cap:
                after_reasons.append("budget_review_ceiling_exceeded")
            if after["disk_free_bytes"] < review_floor:
                after_reasons.append("budget_review_free_space_floor_exceeded")
            if after_reasons and result["status"] == "ok":
                result["status"], result["reasons"] = "budget_limited", sorted(set(after_reasons))
            result["peak_allocated_bytes"] = max([after["allocated_bytes"], result.get("allocated_before_bytes", 0), *[sample["allocated_bytes"] for entry in result["jobs"] + result["feedback_warm_jobs"] + result["edit_jobs"] for sample in entry["resource_samples"]]])
            result["retained_growth_bytes"] = max(0, after["allocated_bytes"] - result.get("allocated_before_bytes", after["allocated_bytes"]))
            target_report = _inventory_module().inventory([], store_roots=[targets_root], discover=False)
            _require(target_report.get("complete") is True and target_report.get("errors") == [], "target_allocation_capture_incomplete")
            result["target_allocated_bytes"] = _count(target_report.get("allocated_bytes"), "target_allocated_bytes")
            owned_roots = [targets_root]
            if config["tool"]["kind"] != "native":
                owned_roots.append(Path(config["tool"]["store"]))
            owned_report = _inventory_module().inventory([], store_roots=owned_roots, discover=False)
            _require(owned_report.get("complete") is True and owned_report.get("errors") == [], "owned_allocation_capture_incomplete")
            result["owned_allocated_bytes"] = _count(owned_report.get("allocated_bytes"), "owned_allocated_bytes")
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
        result["capture_complete"] = result["status"] == "ok" and result.get("process_tree_drained") is True and all(entry["process_tree_capture_complete"] for entry in result["jobs"] + result["feedback_warm_jobs"] + result["edit_jobs"])
        result["artifacts_verified"] = result["status"] == "ok" and all(entry.get("cargo", {}).get("build_success") and not entry.get("cargo", {}).get("errors") for entry in result["jobs"] + result["edit_jobs"])
        result["quiet"] = result["status"] == "ok" and all(not entry["external_processes"] for entry in result["jobs"] + result["edit_jobs"])
        _write_new(arm_root / "arm.json", result)
    return result


def _validate_resume(state: dict, protocol: dict, protocol_hash: str) -> None:
    seal = state.get("integrity_sha256")
    _require(isinstance(seal, str) and seal == canonical_sha256({key: value for key, value in state.items() if key != "integrity_sha256"}), "resume_state_corrupt")
    _require(state.get("schema_version") == 1 and state.get("protocol_sha256") == protocol_hash and state.get("phase") == protocol["phase"] and state.get("planned_pairs") == protocol["pairs"], "resume_protocol_mismatch")
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
        if pair.get("status") == "ok":
            _require(index not in indices and set(pair.get("arms", {})) == {"baseline", "candidate"} and all(arm.get("status") == "ok" for arm in pair["arms"].values()), "duplicate_or_incomplete_valid_pair")
            indices.add(index)
        for arm, receipt in pair.get("arm_receipts", {}).items():
            path = _path(receipt.get("path"), "arm_receipt")
            _require(_inside(path, Path(protocol["run_root"])) and sha256_file(path) == receipt.get("sha256"), "resume_arm_receipt_changed")
            _require(_read_json(path) == pair["arms"][arm], "resume_arm_payload_changed")
    _require(indices == set(range(len(indices))), "resume_valid_pair_gap")
    _require(state.get("completed_valid_pairs") == len(indices), "resume_completed_count_changed")
    if state.get("complete") is True or state.get("status") == "complete":
        _require(len(indices) == protocol["pairs"] and state.get("complete") is True and state.get("status") == "complete", "resume_incomplete_campaign_claims_complete")
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
        output = Path(capture["stdout_path"]).read_bytes() + Path(capture["stderr_path"]).read_bytes()
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


def _run_protocol_locked(protocol_path: str | Path, *, review_receipt_path: str | Path | None = None, pair_limit: int = 20, initializing: bool = False) -> dict:
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
            state["pairs"].append({"pair_index": inflight["pair_index"], "attempt": inflight["attempt"], "order": protocol["order"][inflight["pair_index"]], "status": "interrupted", "arms": {}, "arm_receipts": {}, "raw_root": inflight["raw_root"]})
            state["inflight"] = None
            _write_state(state_path, state)
            return state
        if state["status"] in ("complete", "failed", "interrupted"):
            return state
    else:
        _require(not build_process_snapshot(), "quiet_segment_required")
        preview = preview_protocol(protocol, _budget_sample(protocol))
        _check_review(protocol, preview, review)
        if not initializing:
            root.mkdir(parents=True, exist_ok=False)
        state = {"schema_version": 1, "kind": "build-feedback-campaign", "protocol_sha256": protocol_hash, "protocol_path": str(protocol_path), "source_commit": protocol["source"]["commit"], "source_tree": protocol["source"]["tree"], "toolchain_sha256": canonical_sha256(protocol["toolchain"]), "phase": protocol["phase"], "planned_pairs": protocol["pairs"], "completed_valid_pairs": 0, "started_at": _stamp(), "status": "ready", "complete": False, "reasons": [], "pairs": [], "inflight": None, "peak_pair_growth_bytes": 0}
        _write_state(state_path, state)
    try:
        segment_receipt = _verify_segment_toolchain(protocol, root)
    except (OSError, ProtocolError) as error:
        state["status"], state["reasons"] = "admission_blocked", [str(error)]
        _write_state(state_path, state)
        return state
    state.setdefault("toolchain_segments", []).append(segment_receipt)
    _write_state(state_path, state)
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
        _write_state(state_path, state)
        pair = {"pair_index": pair_index, "attempt": attempt, "order": protocol["order"][pair_index], "started_at": _stamp(), "status": "running", "arms": {}, "arm_receipts": {}}
        for arm in pair["order"]:
            result = _run_arm(protocol, arm, Path(allocation[arm + "_source_root"]), pair_root / arm, review)
            pair["arms"][arm] = result
            arm_path = pair_root / arm / "arm.json"
            pair["arm_receipts"][arm] = {"path": str(arm_path), "sha256": sha256_file(arm_path)}
            if result["status"] != "ok":
                pair["status"] = result["status"]
                break
        if pair["status"] == "running":
            pair["status"] = "ok"
            state["completed_valid_pairs"] += 1
            growth = sum(max(entry.get("retained_growth_bytes", 0), entry.get("peak_allocated_bytes", 0) - entry.get("allocated_before_bytes", 0)) for entry in pair["arms"].values())
            state["peak_pair_growth_bytes"] = max(state["peak_pair_growth_bytes"], growth)
            segment_count += 1
        pair["finished_at"] = _stamp()
        _write_new(pair_root / "pair.json", pair)
        state["pairs"].append(pair)
        state["inflight"] = None
        if pair["status"] not in ("ok", "contaminated"):
            state["status"], state["reasons"] = "failed", [pair["status"]]
            _write_state(state_path, state)
            return state
        state["status"] = "ready"
        _write_state(state_path, state)
    state["complete"] = state["completed_valid_pairs"] == protocol["pairs"]
    if state["complete"]:
        state["status"], state["reasons"] = "complete", []
        state["finished_at"] = _stamp()
    elif state["status"] == "ready":
        state["status"] = "segment_complete"
    if state["completed_valid_pairs"]:
        valid = [pair for pair in state["pairs"] if pair["status"] == "ok"]
        average_ms = sum(sum(arm["duration_ms"] for arm in pair["arms"].values()) for pair in valid) / len(valid)
        state["projected_total_compute_hours"] = average_ms * protocol["pairs"] / 3_600_000
        state["projected_remaining_compute_hours"] = average_ms * (protocol["pairs"] - len(valid)) / 3_600_000
    _write_state(state_path, state)
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


def run_protocol(protocol_path: str | Path, *, review_receipt_path: str | Path | None = None, pair_limit: int = 20) -> dict:
    """Admit and exclusively run/resume one bounded segment of frozen pairs."""
    protocol_path = _path(protocol_path, "protocol")
    protocol = _read_json(protocol_path)
    _validate_protocol(protocol)
    _require(os.name == "nt", "windows_capture_required")
    _count(pair_limit, "pair_limit", positive=True)
    _require(pair_limit <= 20, "health_segment_maximum_20_pairs")
    review = _path(review_receipt_path, "review_receipt") if review_receipt_path is not None else None
    root = Path(protocol["run_root"])
    initializing = not root.exists()
    if initializing:
        _require(not build_process_snapshot(), "quiet_segment_required")
        quick_free = _free_bytes()
        if quick_free - protocol["predicted_growth_bytes"] < 80 * GIB:
            path = Path(protocol["baseline"]["receipt_path"]).parent / ("admission-disk-refused-" + uuid.uuid4().hex + ".json")
            _write_new(path, {"schema_version": 1, "kind": "build-feedback-admission", "protocol_sha256": canonical_sha256(protocol), "allowed": False,
                              "reason": "projected_disk_free_below_minimum", "disk_free_bytes": quick_free, "predicted_growth_bytes": protocol["predicted_growth_bytes"], "minimum_free_bytes": 80 * GIB})
            raise ProtocolError("projected_disk_free_below_minimum:" + str(path))
        _check_review(protocol, preview_protocol(protocol, _budget_sample(protocol)), review)
        root.mkdir(parents=True, exist_ok=False)
    else:
        _require((root / "campaign.json").is_file(), "run_root_exists_without_matching_state")
    with _CampaignLock(root / "runner.lock"):
        return _run_protocol_locked(protocol_path, review_receipt_path=review, pair_limit=pair_limit, initializing=initializing)
