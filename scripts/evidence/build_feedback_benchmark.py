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

    payload = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_keys, parse_constant=finite_constant)
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
        if read_json(path) != arm:
            return None
        return {"path": str(path), "sha256": sha256_file(path)}
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


def normalize_runner_campaign(raw: dict[str, Any], protocol: dict[str, Any], qualifications: dict[str, Any]) -> dict[str, Any]:
    """Validate immutable runner receipts, then project without selecting samples.

    An incomplete allocation receipt stays incomplete. No current target scan
    can reconstruct a historic store peak or certify the other arm's storage.
    """
    runner = _module("runner")
    runner._validate_protocol(protocol)
    protocol_hash = runner.canonical_sha256(protocol)
    runner._validate_resume(raw, protocol, protocol_hash)
    source, chain = protocol["source"], protocol["toolchain"]
    identity = {
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
    jobs = [job["name"] for job in runner.JOBS]
    status_names = {"ok": "valid", "capture_failed": "failed", "admission_blocked": "blocked", "interrupted": "incomplete"}
    records = []
    for pair in raw["pairs"]:
        record = {key: pair.get(key) for key in ("pair_index", "attempt", "order")}
        record["status"] = status_names.get(pair.get("status"), pair.get("status"))
        record["reason"] = ";".join(str(reason) for arm in pair.get("arms", {}).values() for reason in arm.get("reasons", []))
        if record["status"] == "contaminated" and not record["reason"]:
            record["reason"] = "external_process_activity"
        for name, arm in pair.get("arms", {}).items():
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
            record[name] = {
                "status": "success" if arm.get("status") == "ok" else arm.get("status"),
                "identity": identity if arm.get("source_commit") == source["commit"] and arm.get("source_tree") == source["tree"] else {},
                "protocol_sha256": protocol_hash,
                "capture_complete": arm.get("process_tree_drained") is True and complete_capture,
                "source_verified": arm.get("status") == "ok" and arm.get("own_edit_restored") is True,
                "artifacts_verified": _runner_artifacts_verified(all_measurements, runner),
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
        records.append(record)
    result = {
        "schema_version": 1, "phase": protocol["phase"], "pairs_required": protocol["pairs"],
        "comparison_kind": "native" if all(config["tool"]["kind"] == "native" for config in protocol["configs"].values()) else "tool",
        "identity": identity, "protocol_sha256": protocol_hash, "seed": protocol["seed"],
        "job_names": jobs, "schedule": protocol["order"], "records": records,
        "qualification_receipts": qualifications,
    }
    if raw.get("complete") is not True or raw.get("status") != "complete":
        result["admission_reasons"] = ["runner_campaign_incomplete", *raw.get("reasons", [])]
    if raw.get("inflight") is not None:
        result.setdefault("admission_reasons", []).append("runner_attempt_inflight")
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
    run_parser.add_argument("--pair-limit", type=int, default=20, help="bounded segment size (1-20); formal N remains 100")
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
            read_json(args.protocol)
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
                campaign = normalize_runner_campaign(campaign, protocol, qualifications)
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
