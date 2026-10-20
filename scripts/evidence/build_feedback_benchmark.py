"""Machine-local CLI for inventory, preview, run and paired build comparisons.

This is development evidence tooling. It never changes Cargo profiles, installs
cache tools, modifies the live source, or deletes a target/store. Inputs and
outputs are explicit local paths; shared documentation contains no receipts.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import sys
from collections.abc import Sequence
from pathlib import Path
from types import ModuleType
from typing import Any

MODULE_DIR = Path(__file__).resolve().parent
_MODULES: dict[str, ModuleType] = {}


def _module(name: str) -> ModuleType:
    if name not in _MODULES:
        path = MODULE_DIR / f"build_feedback_{name}.py"
        spec = importlib.util.spec_from_file_location(f"build_feedback_{name}", path)
        if spec is None or spec.loader is None:
            raise ValueError(f"build feedback {name} module is unavailable")
        module = importlib.util.module_from_spec(spec)
        # Dataclass decorators resolve their module through sys.modules.
        sys.modules[spec.name] = module
        try:
            spec.loader.exec_module(module)
        except BaseException:
            sys.modules.pop(spec.name, None)
            raise
        _MODULES[name] = module
    return _MODULES[name]


def read_json(path: Path) -> dict[str, Any]:
    def unique_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    def finite_constant(value: str) -> None:
        raise ValueError(f"non-finite JSON number: {value}")

    before = sha256_file(path)
    with path.open("r", encoding="utf-8") as stream:
        payload = json.load(stream, object_pairs_hook=unique_keys, parse_constant=finite_constant)
    if sha256_file(path) != before:
        raise ValueError("receipt changed during JSON read")
    if not isinstance(payload, dict):
        raise TypeError("receipt must be a JSON object")
    return payload


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _runner_allocation_verified(arm: dict[str, Any]) -> bool:
    allocation = arm.get("owned_allocation_receipt")
    if not (
        isinstance(allocation, dict) and type(allocation.get("schema_version")) is int
        and allocation["schema_version"] == 1 and allocation.get("complete") is True
        and allocation.get("errors") == []
        and allocation.get("storage_scope") == "owned_targets_and_stores"
        and type(allocation.get("allocated_bytes")) is int and allocation["allocated_bytes"] >= 0
        and arm.get("allocation_complete") is True
        and type(arm.get("owned_allocated_bytes")) is int
        and arm["owned_allocated_bytes"] == allocation["allocated_bytes"]
    ):
        return False
    try:
        target = Path(arm["target_root"])
        if not target.is_absolute():
            return False
        receipt_path = target.parent / "owned-allocation.json"
        return (
            sha256_file(receipt_path) == arm.get("owned_allocation_receipt_sha256")
            and read_json(receipt_path) == allocation
        )
    except (OSError, ValueError, TypeError, KeyError):
        return False

def _runner_arm_reference(arm: dict[str, Any]) -> dict[str, str] | None:
    try:
        target = Path(arm["target_root"])
        if not target.is_absolute():
            return None
        path = target.parent / "arm.json"
        before = sha256_file(path)
        original = read_json(path)
        if (json.dumps(original, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
                != json.dumps(arm, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
                or sha256_file(path) != before):
            return None
        return {"path": str(path), "sha256": before}
    except (OSError, ValueError, TypeError, KeyError):
        return None



def _runner_artifacts_verified(measurements: list[dict[str, Any]], runner: ModuleType) -> bool:
    if not measurements:
        return False
    latest_receipts: dict[str, dict[str, Any]] = {}
    for measurement in measurements:
        cargo = measurement.get("cargo")
        if not isinstance(cargo, dict) or cargo.get("build_success") is not True or cargo.get("errors") != []:
            return False
        try:
            for name in ("stdout", "stderr"):
                if sha256_file(Path(measurement[name + "_path"])) != measurement.get(name + "_sha256"):
                    return False
            artifacts = cargo.get("artifacts")
            if not isinstance(artifacts, list):
                return False
            for artifact in artifacts:
                if not isinstance(artifact, dict) or not isinstance(artifact.get("files"), list):
                    return False
                for receipt in artifact["files"]:
                    if not isinstance(receipt, dict) or not isinstance(receipt.get("path"), str):
                        return False
                    latest_receipts[receipt["path"]] = receipt
        except (OSError, ValueError, TypeError, KeyError):
            return False
    # Edit/lane reuse intentionally replaces earlier artifacts at the same path.
    # Logs pin every invocation; recheck the final recorded version of each file.
    try:
        for receipt in latest_receipts.values():
            runner.verify_artifact_receipt(receipt)
    except (OSError, ValueError, TypeError, KeyError):
        return False
    return True


def _runner_identity(protocol: dict[str, Any]) -> dict[str, Any]:
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


def _normalize_runner_arm(arm: dict[str, Any], protocol: dict[str, Any], runner: ModuleType, *, retained: Any = None) -> dict[str, Any]:
    source = protocol["source"]
    protocol_hash = runner.canonical_sha256(protocol)
    identity = _runner_identity(protocol)
    jobs = [job["name"] for job in runner.JOBS]
    measurements = arm.get("jobs", [])
    warm_measurements = arm.get("feedback_warm_jobs", [])
    feedback_measurements = arm.get("edit_jobs", [])
    phases = (measurements, warm_measurements, feedback_measurements)
    if not all(isinstance(phase, list) and all(isinstance(entry, dict) for entry in phase) for phase in phases):
        raise ValueError("invalid runner phase measurements")
    all_measurements = measurements + warm_measurements + feedback_measurements
    allocation = arm.get("owned_allocation_receipt")
    allocated = allocation.get("allocated_bytes") if isinstance(allocation, dict) else arm.get("owned_allocated_bytes", arm.get("target_allocated_bytes"))
    complete_allocation = _runner_allocation_verified(arm)
    complete_capture = all(
        [entry.get("job") for entry in phase] == jobs
        and all(entry.get("status") == "ok" and type(entry.get("returncode")) is int
                and entry["returncode"] == 0 and entry.get("process_tree_capture_complete") is True for entry in phase)
        for phase in phases
    )
    normalized_jobs = {}
    for measurement in measurements:
        if measurement.get("job") in normalized_jobs:
            raise ValueError("duplicate runner job measurement")
        normalized_jobs[measurement.get("job")] = {
            "status": "success" if measurement.get("status") == "ok" else measurement.get("status"),
            "exit_code": measurement.get("returncode"),
            "duration_seconds": measurement.get("duration_ms", 0) / 1000,
        }
    return {
        "status": "success" if arm.get("status") == "ok" else arm.get("status"),
        "identity": identity if arm.get("source_commit") == source["commit"] and arm.get("source_tree") == source["tree"] else {},
        "protocol_sha256": protocol_hash,
        "capture_complete": arm.get("process_tree_drained") is True and complete_capture,
        "source_verified": arm.get("status") == "ok" and arm.get("own_edit_restored") is True,
        "artifacts_verified": _runner_artifacts_verified(all_measurements, runner) if retained is None else True,
        "allocation_complete": complete_allocation,
        "storage_scope": "owned_targets_and_stores" if complete_allocation else "unverified_store_allocation",
        "quiet": bool(all_measurements) and all(measurement.get("external_processes") == [] for measurement in all_measurements),
        "duration_seconds": arm.get("primary_duration_ms", 0) / 1000,
        "elapsed_seconds": arm.get("duration_ms", 0) / 1000,
        "allocated_bytes": allocated, "jobs": normalized_jobs,
        "source_root": arm.get("source_root"),
        "config_sha256": hashlib.sha256(json.dumps(arm.get("config"), sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest(),
        "evidence": _runner_arm_reference(arm),
        "feedback_phases": {
            phase: {
                "duration_seconds": arm.get("phase_duration_ms", {}).get(phase, 0) / 1000,
                "jobs": {entry.get("job"): {
                    "status": "success" if entry.get("status") == "ok" else entry.get("status"),
                    "exit_code": entry.get("returncode"),
                    "duration_seconds": entry.get("duration_ms", 0) / 1000,
                } for entry in entries},
            } for phase, entries in (("feedback-warm", warm_measurements), ("edit", feedback_measurements))
        },
    }


def normalize_runner_arm(raw_arm: dict[str, Any], protocol: dict[str, Any]) -> dict[str, Any]:
    """Project one immutable live arm, including unpaired full capacity work.

    Does not fabricate a campaign, valid pair or retained-artifact authority.
    The exact raw receipt remains required and the comparison/seal consumer
    independently verifies commands, artifacts, source and qualification.
    """
    runner = _module("runner")
    runner._validate_protocol(protocol)
    if not isinstance(raw_arm, dict):
        raise TypeError("runner arm must be an object")
    if "retained_evidence" in raw_arm:
        raise ValueError("single-arm projection requires original live evidence")
    full_capacity = protocol.get("phase") == "capacity"
    if full_capacity:
        probe = protocol.get("capacity_probe", {})
        if (type(protocol.get("schema_version")) is not int or protocol["schema_version"] != 2
                or probe.get("kind") != "full-arm-and-qualification" or probe.get("stage") != "expanded"
                or raw_arm.get("arm") != probe.get("arm") or probe.get("arm") not in ("baseline", "candidate")
                or type(protocol.get("pairs")) is not int or protocol["pairs"] != 0 or protocol.get("allocations") != []
                or raw_arm.get("phase") != "capacity" or raw_arm.get("performance_evidence") is not False
                or not all(field in raw_arm and raw_arm[field] is None for field in ("pair_index", "attempt"))
                or type(raw_arm.get("completed_valid_pairs")) is not int or raw_arm["completed_valid_pairs"] != 0):
            raise ValueError("single full-capacity arm must keep zero paired samples")
        expected = Path(protocol["run_root"]) / "capacity"
        if (raw_arm.get("source_root") != probe.get("source_root")
                or runner.canonical_sha256(raw_arm.get("config")) != runner.canonical_sha256(protocol["configs"][probe["arm"]])
                or Path(raw_arm["target_root"]).resolve(strict=True) != expected / "targets"):
            raise ValueError("single full-capacity arm differs from its approved source/config/output allocation")
    reference = _runner_arm_reference(raw_arm)
    if reference is None:
        raise ValueError("single-arm projection requires its exact immutable arm receipt")
    if full_capacity and Path(reference["path"]).resolve(strict=True) != expected / "arm.json":
        raise ValueError("single full-capacity arm differs from its approved receipt path")
    normalized = _normalize_runner_arm(raw_arm, protocol, runner)
    if normalized.get("evidence") != reference or sha256_file(Path(reference["path"])) != reference["sha256"]:
        raise ValueError("single-arm receipt changed during normalization")
    return normalized


def normalize_runner_campaign(raw: dict[str, Any], protocol: dict[str, Any], qualifications: dict[str, Any]) -> dict[str, Any]:
    """Validate immutable runner receipts, then project without selecting samples.

    An incomplete allocation receipt stays incomplete. No current target scan
    can reconstruct a historic store peak or certify the other arm's storage.
    """
    runner = _module("runner")
    runner._validate_protocol(protocol)
    protocol_hash = runner.canonical_sha256(protocol)
    runner._validate_resume(raw, protocol, protocol_hash)
    runner_reference = None
    if protocol.get("schema_version") == 2:
        raw_path = Path(protocol["run_root"]) / "campaign.json"
        if not raw_path.is_absolute() or raw_path.resolve(strict=True) != raw_path:
            raise ValueError("runner campaign must use the exact approved run_root/campaign.json")
        runner_reference = {"path": str(raw_path), "sha256": sha256_file(raw_path)}
        if read_json(raw_path) != raw or sha256_file(raw_path) != runner_reference["sha256"]:
            raise ValueError("runner campaign differs from pinned raw source")
    identity = _runner_identity(protocol)
    jobs = [job["name"] for job in runner.JOBS]
    status_names = {"ok": "valid", "capture_failed": "failed", "admission_blocked": "blocked", "interrupted": "incomplete"}
    records = []
    for pair in raw["pairs"]:
        record = {key: pair.get(key) for key in ("pair_index", "attempt", "order")}
        record["status"] = status_names.get(pair.get("status"), pair.get("status"))
        record["reason"] = ";".join(str(reason) for arm in pair.get("arms", {}).values() for reason in arm.get("reasons", []))
        if protocol.get("schema_version") == 2:
            record["finalization_reasons"] = pair.get("finalization_reasons", [])
            record["arm_finalization_duration_ms"] = pair.get("arm_finalization_duration_ms", {})
        if record["status"] == "contaminated" and not record["reason"]:
            record["reason"] = "external_process_activity"
        for name, arm in pair.get("arms", {}).items():
            retained_links = pair.get("retained_evidence", {})
            if not isinstance(retained_links, dict):
                raise TypeError("invalid retained arm reference map")
            retained_reference = retained_links.get(name)
            if "retained_evidence" in arm:
                raise ValueError("runner retained references must not alter the immutable arm receipt")
            retained = None
            if retained_reference is not None:
                if type(protocol.get("schema_version")) is not int or protocol["schema_version"] != 2:
                    raise ValueError("retained evidence requires protocol v2")
                retained = _module("retention").verify_retained_arm(retained_reference, protocol, name, pair["pair_index"], pair["attempt"])
                if not isinstance(retained, dict) or retained.get("arm_receipt") != arm:
                    raise ValueError("runner arm differs from original sealed receipt")
                arm = retained["arm_receipt"]
            record[name] = _normalize_runner_arm(arm, protocol, runner, retained=retained)
            if retained is not None:
                if not isinstance(retained.get("seal"), dict) or retained["seal"].get("arm_projection") != record[name]:
                    raise ValueError("normalized arm differs from prerecycle sealed projection")
                record[name]["retained_evidence"] = retained_reference
        records.append(record)
    result = {
        "schema_version": protocol.get("schema_version", 1), "phase": protocol["phase"], "pairs_required": protocol["pairs"],
        "comparison_kind": "native" if all(config["tool"]["kind"] == "native" for config in protocol["configs"].values()) else "tool",
        "identity": identity, "protocol_sha256": protocol_hash, "seed": protocol["seed"],
        "job_names": jobs, "schedule": protocol["order"], "records": records,
        "qualification_receipts": qualifications,
    }
    if runner_reference is not None:
        result["runner_campaign_evidence"] = runner_reference
        result["runner_state"] = {key: raw.get(key) for key in (
            "status", "complete", "reasons", "planned_pairs", "completed_valid_pairs", "inflight",
            "started_at", "finished_at", "toolchain_segments", "registered_compute_seconds",
            "formal_registration_sha256", "peak_pair_growth_bytes", "peak_active_arm_growth_bytes",
        )}
    if raw.get("complete") is not True or raw.get("status") != "complete":
        result["admission_reasons"] = ["runner_campaign_incomplete", *raw.get("reasons", [])]
    if raw.get("inflight") is not None:
        result.setdefault("admission_reasons", []).append("runner_attempt_inflight")
    if protocol["phase"] == "capacity":
        if protocol.get("schema_version") != 2 or protocol["pairs"] != 0 or records:
            raise ValueError("capacity is not a paired campaign")
        result["capacity_observation"] = raw.get("capacity_observation", raw.get("capacity"))
        result["performance_evidence"] = False
    if runner_reference is not None and (sha256_file(Path(runner_reference["path"])) != runner_reference["sha256"]
                                         or read_json(Path(runner_reference["path"])) != raw):
        raise ValueError("runner campaign changed during normalization")
    return result

def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    commands = result.add_subparsers(dest="command", required=True)
    inventory_parser = commands.add_parser("inventory", help="scan actual owned build/cache allocation")
    inventory_parser.add_argument("--root", type=Path, action="append", default=[], help="known source/build root (repeatable)")
    inventory_parser.add_argument("--store-root", type=Path, action="append", default=[], help="explicit local cache store (repeatable)")
    inventory_parser.add_argument("--no-discover", action="store_true", help="scan only explicitly supplied build roots")
    inventory_parser.add_argument("--output", type=Path, help="new local JSON receipt; omitted prints JSON")
    preview_parser = commands.add_parser("preview", help="read-only protocol and next-batch admission preview")
    preview_parser.add_argument("--protocol", type=Path, required=True)
    preview_parser.add_argument("--inventory", type=Path, required=True)
    run_parser = commands.add_parser("run", help="execute/resume only the previously frozen protocol")
    run_parser.add_argument("--protocol", type=Path, required=True)
    run_parser.add_argument("--review-receipt", type=Path, help="explicit owner review for the exact budget preview")
    run_parser.add_argument("--pair-limit", type=int, default=20, help="bounded segment size (1-20); frozen formal N is unchanged")
    compare_parser = commands.add_parser("compare", help="compare complete campaign and qualification receipts")
    compare_parser.add_argument("--campaign", type=Path, required=True, help="runner or normalized local campaign JSON")
    compare_parser.add_argument("--protocol", type=Path, help="frozen protocol required for a runner campaign")
    compare_parser.add_argument("--qualifications", type=Path, help="local identity-bound qualification receipt bundle")
    compare_parser.add_argument("--output", type=Path, help="new local result JSON; omitted prints JSON")
    return result


def _emit(payload: dict[str, Any], output: Path | None = None) -> None:
    if output is not None:
        # Preserve earlier evidence even if an operator repeats a command.
        if output.exists():
            raise ValueError("output receipt already exists; choose a new local path")
        _module("inventory").atomic_json(output, payload)
    else:
        print(json.dumps(payload, ensure_ascii=False, indent=2, allow_nan=False))


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "inventory":
            if not args.root and not args.store_root:
                raise ValueError("inventory requires at least one explicit root/store root")
            if args.output and args.output.exists():
                raise ValueError("output receipt already exists; choose a new local path")
            report = _module("inventory").inventory(args.root, store_roots=args.store_root, discover=not args.no_discover)
            _emit(report, args.output)
            return 0 if report.get("complete") is True and not report.get("errors") else 2
        if args.command == "preview":
            protocol = read_json(args.protocol)
            report = read_json(args.inventory)
            preview = _module("runner").preview_protocol(protocol, report)
            _emit(preview)
            return 0 if preview.get("allowed") is True else 2
        if args.command == "run":
            # Validate explicit receipts before a runner may create any evidence.
            protocol = read_json(args.protocol)
            if protocol.get("schema_version") == 2 and protocol.get("phase") in ("pilot", "formal"):
                raise ValueError("v2 paired run requires the coordinator's approved arm_finalizer for verify-seal-recycle; use runner.run_protocol with that callback")
            if not 1 <= args.pair_limit <= 20:
                raise ValueError("pair-limit must be in 1..20; it does not alter fixed N")
            if args.review_receipt:
                read_json(args.review_receipt)
            result = _module("runner").run_protocol(
                args.protocol, review_receipt_path=args.review_receipt,
                pair_limit=args.pair_limit,
            )
            if isinstance(result, Path):
                result = read_json(result)
            if not isinstance(result, dict):
                raise ValueError("runner did not return an auditable result receipt")
            _emit(result)
            return 0 if result.get("complete") is True and result.get("status") in {"complete", "completed", "success"} else 2
        if args.command == "compare":
            if args.output and args.output.exists():
                raise ValueError("output receipt already exists; choose a new local path")
            campaign = read_json(args.campaign)
            if campaign.get("kind") == "build-feedback-campaign":
                if args.protocol is None:
                    raise ValueError("runner campaign comparison requires --protocol")
                protocol = read_json(args.protocol)
                qualifications = read_json(args.qualifications) if args.qualifications else {}
                if protocol.get("schema_version") == 2 and args.campaign.resolve(strict=True) != Path(protocol["run_root"]) / "campaign.json":
                    raise ValueError("v2 comparison requires the exact approved run_root/campaign.json")
                campaign = normalize_runner_campaign(campaign, protocol, qualifications)
                campaign["protocol_evidence"] = {
                    "path": str(args.protocol.resolve(strict=True)),
                    "sha256": sha256_file(args.protocol),
                }
            elif args.protocol is not None or args.qualifications is not None:
                raise ValueError("normalized campaign already contains its protocol identity and qualifications")
            result = _module("statistics").compare_campaign(campaign)
            result["campaign_sha256"] = sha256_file(args.campaign)
            _emit(result, args.output)
            return 0 if result.get("qualified") is True else 2
        raise ValueError("unknown command")
    except (OSError, ValueError, TypeError, KeyError) as error:
        print(json.dumps({"schema_version": 1, "status": "error", "reason": str(error)}, ensure_ascii=False), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
