#!/usr/bin/env python3
"""Open-source gate benchmark for agent-session-grep.

Adds the metrics the public benchmark needs beyond the core latency harness:
lexical recall on a synthetic labeled corpus, parse-loss accounting,
discovery-coverage scaffolding, resume/handoff success counters, and a single
gate evidence manifest that records each metric's threshold, pass/fail, and
the environment block. Metrics whose feature has not landed yet are recorded
explicitly with state "not_applicable" — never silently skipped.

Uses only the Python standard library. Helpers are reused from
core_beta_benchmark.py so the two harnesses stay consistent.
"""

from __future__ import annotations

import argparse
import json
import platform
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# Reuse the frozen helpers from the core harness so both reports stay
# byte-compatible in their statistical projections.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from core_beta_benchmark import (  # noqa: E402
    cli,
    command_text,
    rounded_summary,
    sha256_file,
    tree_hash,
)

GATE_SCHEMA_VERSION = "agent-session-grep.open-source-gate/v1"
FIXTURE_DIR = Path(__file__).resolve().parent / "fixtures" / "gate"
LABELS_PATH = FIXTURE_DIR / "labels.json"

# Metric names that carry a gate threshold. The full manifest also records
# informational metrics (latency p50/p95, index size) without a threshold.
GATE_THRESHOLDS = {
    "lexical_recall_at_10": 0.95,
    "parse_loss_ratio": 0.05,
    "discovery_coverage": 0.95,
    "resume_handoff_success": 1.0,
}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def load_labels() -> dict[str, Any]:
    return json.loads(LABELS_PATH.read_text(encoding="utf-8"))


def corpus_files() -> list[Path]:
    files = sorted(FIXTURE_DIR.rglob("*.jsonl"))
    # labels.json is a manifest, not a transcript source.
    return [p for p in files if p.name != "labels.json"]


def resolve_binary(workspace: Path, args: argparse.Namespace) -> Path:
    if args.binary:
        binary = Path(args.binary).expanduser().resolve()
    else:
        build = subprocess.run(
            [args.cargo, "build", "--locked", "--release", "-p", "agent-session-grep-cli"],
            cwd=workspace,
            check=False,
            capture_output=True,
            text=True,
        )
        if build.returncode != 0:
            hint = (
                "\ncargo build failed. If a running agent-session-grep (e.g. an MCP"
                "\nserver) holds target/release/agent-session-grep.exe locked on"
                "\nWindows, build into an isolated target dir or pass --binary with"
                "\na prebuilt release binary."
            )
            raise RuntimeError(
                f"cargo build --locked --release failed (exit {build.returncode}):"
                f"\n{build.stdout.strip()}\n{build.stderr.strip()}{hint}"
            )
        name = "agent-session-grep.exe" if platform.system().lower() == "windows" else "agent-session-grep"
        binary = workspace / "target" / "release" / name
    if not binary.is_file():
        raise FileNotFoundError(f"release CLI binary not found: {binary}")
    return binary


def lexical_recall_at_10(
    binary: Path,
    workspace: Path,
    db: Path,
    labels: dict[str, Any],
) -> tuple[float, list[dict[str, Any]], list[dict[str, Any]]]:
    """Run every labeled query and score message-id recall in the top 10.

    Returns (mean_recall, per_query_detail, raw_search_samples). Search hits
    are message-level; recall is computed against expected_message_ids.
    """
    per_query: list[dict[str, Any]] = []
    search_samples: list[dict[str, Any]] = []
    recalls: list[float] = []
    for entry in labels["queries"]:
        result = cli(binary, workspace, db, "search", entry["query"], "--max-items", "10")
        search_samples.append(result)
        hits = result["frame"]["data"].get("hits", [])
        hit_ids = {hit.get("id") for hit in hits}
        expected = set(entry["expected_message_ids"])
        found = expected & hit_ids
        recall = len(found) / len(expected) if expected else 1.0
        recalls.append(recall)
        per_query.append(
            {
                "query_id": entry["id"],
                "query": entry["query"],
                "expected_count": len(expected),
                "found_count": len(found),
                "recall_at_10": round(recall, 6),
                "missing_ids": sorted(expected - hit_ids),
            }
        )
    mean_recall = round(sum(recalls) / len(recalls), 6) if recalls else 1.0
    return mean_recall, per_query, search_samples


def parse_loss_from_sync(sync_frames: list[dict[str, Any]]) -> dict[str, Any]:
    emitted = sum(int(f["data"].get("emitted", 0)) for f in sync_frames)
    skipped = sum(int(f["data"].get("skipped", 0)) for f in sync_frames)
    total = emitted + skipped
    ratio = round(skipped / total, 6) if total else 0.0
    return {"emitted": emitted, "skipped": skipped, "parse_loss_ratio": ratio}


def latency_p50_p95(samples: list[dict[str, Any]]) -> dict[str, float]:
    durations = [float(s["duration_ms"]) for s in samples]
    summary = rounded_summary(durations)
    return {"p50": summary["p50"], "p95": summary["p95"], "count": summary["count"]}


def metric_entry(
    name: str,
    unit: str,
    value: Any,
    threshold: float | None,
    pass_flag: bool | None,
    state: str = "measured",
    reason: str | None = None,
) -> dict[str, Any]:
    entry: dict[str, Any] = {
        "name": name,
        "unit": unit,
        "state": state,
        "value": value,
        "threshold": threshold,
        "pass": pass_flag,
    }
    if reason:
        entry["reason"] = reason
    return entry


def run_gate(args: argparse.Namespace) -> Path:
    workspace = Path(args.workspace).expanduser().resolve()
    output_dir = Path(args.output_dir).expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    commit = command_text(["git", "rev-parse", "HEAD"], workspace) or "not_recorded"
    binary = resolve_binary(workspace, args)
    labels = load_labels()
    fixture_files = corpus_files()
    if not fixture_files:
        raise RuntimeError(f"no labeled fixture sources found under {FIXTURE_DIR}")
    fixture_hash = tree_hash(fixture_files, FIXTURE_DIR)

    with tempfile.TemporaryDirectory(prefix="agent-session-grep-gate-") as temp_name:
        scratch = Path(temp_name)
        db = scratch / "gate.db"

        # Sync the labeled corpus explicitly (sync --discover is not yet
        # implemented; discovery_coverage is recorded as not_applicable below).
        sync_frames: list[dict[str, Any]] = []
        sync_samples: list[dict[str, Any]] = []
        for _ in range(max(1, args.sync_reps)):
            result = cli(binary, workspace, db, "sync", *(str(p) for p in fixture_files))
            sync_frames.append(result["frame"])
            sync_samples.append(result)

        loss = parse_loss_from_sync(sync_frames)

        mean_recall, per_query, search_samples = lexical_recall_at_10(binary, workspace, db, labels)

        # Latency for show/get against known wire ids from the labels.
        first_expected = labels["queries"][0]["expected_message_ids"][0]
        show_samples = [
            cli(binary, workspace, db, "show", first_expected) for _ in range(args.latency_reps)
        ]
        get_samples = [
            cli(binary, workspace, db, "get", first_expected) for _ in range(args.latency_reps)
        ]

        # Index size ratio from the synced store.
        storage_files = list(scratch.glob("gate.db*"))
        store_bytes = sum(p.stat().st_size for p in storage_files if p.is_file())
        source_bytes = sum(p.stat().st_size for p in fixture_files)
        index_size_ratio = round(store_bytes / source_bytes, 6) if source_bytes else None

    # Gate metrics. discovery_coverage and resume_handoff_success are recorded
    # explicitly as not_applicable until their features land — never silently
    # skipped.
    metrics: list[dict[str, Any]] = [
        metric_entry(
            "lexical_recall_at_10",
            "ratio",
            mean_recall,
            GATE_THRESHOLDS["lexical_recall_at_10"],
            mean_recall >= GATE_THRESHOLDS["lexical_recall_at_10"],
        ),
        metric_entry(
            "parse_loss_ratio",
            "ratio",
            loss["parse_loss_ratio"],
            GATE_THRESHOLDS["parse_loss_ratio"],
            loss["parse_loss_ratio"] <= GATE_THRESHOLDS["parse_loss_ratio"],
        ),
        metric_entry(
            "discovery_coverage",
            "ratio",
            None,
            GATE_THRESHOLDS["discovery_coverage"],
            None,
            state="not_applicable",
            reason="sync --discover is not yet implemented (task 08-14-provider-source-auto-discovery); corpus is synced via explicit paths.",
        ),
        metric_entry(
            "resume_handoff_success",
            "count",
            None,
            GATE_THRESHOLDS["resume_handoff_success"],
            None,
            state="not_applicable",
            reason="resume/handoff execution has not landed (task 08-15-resume-metadata-execution); counter scaffolding only.",
        ),
    ]

    applicable = [m for m in metrics if m["state"] == "measured"]
    failures = [m["name"] for m in applicable if m["pass"] is False]
    deferred = [m["name"] for m in metrics if m["state"] == "not_applicable"]
    gate_pass = not failures

    manifest: dict[str, Any] = {
        "schema_version": GATE_SCHEMA_VERSION,
        "generated_at_utc": utc_now(),
        "commit": commit,
        "environment": {
            "os": platform.system().lower(),
            "arch": platform.machine() or "not_recorded",
            "build": "release",
            "commit": commit,
            "provider_fixture_set_id": labels.get("fixture_set_id", "not_recorded"),
            "python_version": platform.python_version(),
        },
        "corpus": {
            "kind": "deterministic_synthetic_labeled",
            "contains_real_transcripts": False,
            "fixture_set_id": labels.get("fixture_set_id", "not_recorded"),
            "providers": labels.get("providers", []),
            "file_count": len(fixture_files),
            "fixture_hash": fixture_hash,
            "hash_algorithm": "sha256",
        },
        "binary": {
            "hash_algorithm": "sha256",
            "binary_hash": sha256_file(binary),
        },
        "metrics": metrics,
        "recall_detail": per_query,
        "parse_loss_detail": loss,
        "latency_p50_p95_ms": {
            "search": latency_p50_p95(search_samples),
            "show": latency_p50_p95(show_samples),
            "get": latency_p50_p95(get_samples),
            "initial_sync": latency_p50_p95(sync_samples),
        },
        "index_size": {
            "store_bytes_including_sidecars": store_bytes,
            "source_bytes": source_bytes,
            "index_size_ratio": index_size_ratio,
        },
        "gate": {
            "pass": gate_pass,
            "failures": failures,
            "deferred": deferred,
        },
    }

    manifest_path = output_dir / f"gate-manifest-{args.profile}.json"
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    validate_manifest(manifest_path)
    print(manifest_path)
    return manifest_path


def validate_manifest(path: Path) -> dict[str, Any]:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != GATE_SCHEMA_VERSION:
        raise ValueError(f"unsupported schema_version: {manifest.get('schema_version')!r}")
    commit = manifest.get("commit", "")
    if not isinstance(commit, str) or len(commit) != 40:
        raise ValueError("commit must be a full Git SHA")
    if manifest.get("corpus", {}).get("contains_real_transcripts") is not False:
        raise ValueError("corpus.contains_real_transcripts must be false")
    metrics = manifest.get("metrics")
    if not isinstance(metrics, list) or not metrics:
        raise ValueError("metrics must be a non-empty list")
    expected_names = set(GATE_THRESHOLDS)
    actual_names = {m.get("name") for m in metrics}
    if actual_names != expected_names:
        raise ValueError(f"metric names {sorted(actual_names)} do not match gate schema {sorted(expected_names)}")
    for m in metrics:
        name = m.get("name")
        threshold = m.get("threshold")
        if threshold != GATE_THRESHOLDS[name]:
            raise ValueError(f"{name}: threshold {threshold!r} != expected {GATE_THRESHOLDS[name]!r}")
        state = m.get("state")
        if state == "not_applicable":
            if m.get("value") is not None or m.get("pass") is not None:
                raise ValueError(f"{name}: not_applicable metrics must carry null value and pass")
            if not m.get("reason"):
                raise ValueError(f"{name}: not_applicable metrics must record a reason")
        elif state == "measured":
            if not isinstance(m.get("value"), (int, float)):
                raise ValueError(f"{name}: measured metrics must carry a numeric value")
            if not isinstance(m.get("pass"), bool):
                raise ValueError(f"{name}: measured metrics must carry a bool pass")
        else:
            raise ValueError(f"{name}: unknown state {state!r}")
    gate = manifest.get("gate", {})
    applicable_failures = [
        m["name"] for m in metrics if m["state"] == "measured" and m["pass"] is False
    ]
    if gate.get("failures") != applicable_failures:
        raise ValueError("gate.failures does not match measured metric pass flags")
    expected_pass = not applicable_failures
    if gate.get("pass") != expected_pass:
        raise ValueError(f"gate.pass {gate.get('pass')!r} inconsistent with failures")
    deferred = gate.get("deferred")
    expected_deferred = sorted(m["name"] for m in metrics if m["state"] == "not_applicable")
    if sorted(deferred or []) != expected_deferred:
        raise ValueError("gate.deferred does not match not_applicable metrics")
    print(f"valid {GATE_SCHEMA_VERSION} manifest: {path}")
    return manifest


def parser() -> argparse.ArgumentParser:
    root = Path(__file__).resolve().parents[2]
    result = argparse.ArgumentParser(description=__doc__)
    sub = result.add_subparsers(dest="command", required=True)
    run = sub.add_parser("run", help="sync the labeled corpus, measure recall/latency, emit the gate manifest")
    run.add_argument("--profile", default="gate", help="profile name used in the manifest filename")
    run.add_argument("--workspace", default=str(root))
    run.add_argument("--output-dir", default=str(root / "scripts" / "evidence" / "out"))
    run.add_argument("--binary", help="explicit prebuilt release CLI; otherwise cargo build --release is run")
    run.add_argument("--cargo", default="cargo")
    run.add_argument("--sync-reps", type=int, default=1, help="sync repetitions for parse-loss aggregation")
    run.add_argument("--latency-reps", type=int, default=10, help="show/get latency repetitions")
    validate = sub.add_parser("validate-report", help="validate a gate manifest against the schema and thresholds")
    validate.add_argument("report")
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "run":
            run_gate(args)
        else:
            validate_manifest(Path(args.report).expanduser().resolve())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
