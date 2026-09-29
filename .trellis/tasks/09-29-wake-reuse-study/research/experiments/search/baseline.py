#!/usr/bin/env python3
"""Portable bounded benchmark of an UNMODIFIED release CLI on synthetic data.

Explicit paths only: no discovery, home scans, network, GUI or Wake execution.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import time

import evidence as ev

SEARCH_QUERY_IDS = ["substring", "cjk_two", "snake", "namespace", "error"]
COUNT_FIELDS = ("sources", "emitted", "committed", "unchanged", "retained", "deferred", "skipped", "diagnostics")


def check_deadline(deadline: float) -> None:
    if ev.remaining(deadline) <= 0:
        raise TimeoutError("per_scale_deadline_exhausted")


def generated_record(index: int, local: int, fixture: dict, helpers) -> dict:
    record = helpers.synthetic_record(index - 1, local)
    record["message"]["content"] = ev.text_for(fixture, index)
    return record


def generate_dataset(directory: Path, messages: int, per_file: int, deadline: float,
                     metadata: dict, helpers) -> list[Path]:
    directory.mkdir(parents=True)
    fixture = ev.load_fixture()
    data_hash, source_hash = hashlib.sha256(), hashlib.sha256()
    metadata.update({"status": "incomplete", "requested_messages": messages, "actual_messages": 0,
                     "messages_per_file": per_file, "files": [], "source_bytes": 0,
                     "fixture_revision": fixture["revision"], "fixture_hash": helpers.sha256_file(ev.FIXTURE_PATH),
                     "hash_method": ev.HASH_METHOD, "contains_real_transcripts": False,
                     "source_tree_hash_method": "sha256(sorted(relative_posix_path + NUL + file_bytes + NUL))"})
    paths = []
    exhausted = False
    for file_index, first in enumerate(range(1, messages + 1, per_file)):
        if ev.remaining(deadline) <= 0:
            exhausted = True
            break
        path = directory / f"session-{file_index:06d}.jsonl"
        file_hash = hashlib.sha256()
        source_hash.update(path.name.encode("utf-8") + b"\0")
        actual, size = 0, 0
        with path.open("xb") as handle:
            for local, index in enumerate(range(first, min(first + per_file, messages + 1))):
                if local % 128 == 0 and ev.remaining(deadline) <= 0:
                    exhausted = True
                    break
                record = generated_record(index, local, fixture, helpers)
                raw = (json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8")
                handle.write(raw)
                file_hash.update(raw)
                source_hash.update(raw)
                ev.update_data_hash(data_hash, index, record["message"]["content"])
                actual += 1
                size += len(raw)
        source_hash.update(b"\0")
        paths.append(path)
        metadata["files"].append({"path": path.name, "sha256": file_hash.hexdigest(), "bytes": size, "messages": actual})
        metadata["actual_messages"] += actual
        metadata["source_bytes"] += size
        if exhausted:
            break
    metadata["dataset_hash"] = data_hash.hexdigest()
    metadata["source_tree_hash"] = source_hash.hexdigest()
    metadata["file_count"] = len(paths)
    if exhausted or metadata["actual_messages"] != messages:
        raise TimeoutError("per_scale_deadline_exhausted_during_generation")
    metadata["status"] = "complete"
    return paths


def command_units(command: list[str]) -> int:
    return len(subprocess.list2cmdline(command).encode("utf-16-le")) // 2


def source_batches(base: list[str], paths: list[Path], max_sources: int, argv_units: int) -> list[list[Path]]:
    ev.require(paths and max_sources > 0 and argv_units > 0, "invalid source batch inputs")
    batches, current = [], []
    for path in paths:
        candidate = [*base, *(str(p) for p in [*current, path])]
        if current and (len(current) >= max_sources or command_units(candidate) > argv_units):
            batches.append(current)
            current = []
        ev.require(command_units([*base, str(path)]) <= argv_units, "one source path exceeds argv budget")
        current.append(path)
    if current:
        batches.append(current)
    return batches


def parse_success_frame(output: str, helpers) -> dict:
    ev.require(len([line for line in output.split("\n") if line.strip()]) == 1, "expected exactly one complete Robot frame")
    frame = helpers.parse_frame(output)
    ev.require(isinstance(frame.get("data"), dict), "Robot frame lacks data object")
    warnings = frame.get("warnings", []) or frame.get("meta", {}).get("warnings", [])
    ev.require(not warnings, "synthetic operation produced warnings")
    return frame


def command_event(command: list[str], label: str, timeout: float, context: dict, robot: bool = True) -> dict:
    event, output = ev.run_process(command, label=label, timeout=timeout, **context)
    if event["status"] == "ok":
        try:
            if robot:
                event["frame_data"] = parse_success_frame(output, context["helpers"])["data"]
            else:
                ev.require(bool(output.strip()), "version command returned empty output")
                event["version"] = output.strip()
        except (ValueError, RuntimeError, KeyError) as error:
            event["status"] = "invalid_frame"
            event["reason"] = ev.redact(str(error), context["roots"])
    return event


def sync_pass(label: str, base: list[str], batches: list[list[Path]], expected_by_name: dict,
              timeout: float, context: dict, noop: bool) -> dict:
    started = time.perf_counter()
    result = {"status": "incomplete", "label": label, "batches": [], "expected_batch_count": len(batches),
              "actual_counts": {key: 0 for key in COUNT_FIELDS}}
    for index, paths in enumerate(batches):
        event = command_event([*base, *(str(path) for path in paths)], f"{label}-batch-{index:04d}", timeout, context)
        event["source_files"] = [path.name for path in paths]
        event["expected_messages"] = sum(expected_by_name[path.name] for path in paths)
        result["batches"].append(event)
        if event["status"] == "ok":
            data = event["frame_data"]
            for field in COUNT_FIELDS:
                if type(data.get(field)) is int and data[field] >= 0:
                    result["actual_counts"][field] += data[field]
            wanted = {"sources": len(paths), "emitted": 0 if noop else event["expected_messages"],
                      "committed": 0 if noop else event["expected_messages"], "unchanged": event["expected_messages"] if noop else 0,
                      "retained": 0, "deferred": 0, "skipped": 0, "diagnostics": 0}
            if any(type(data.get(key)) is not int or data[key] != value for key, value in wanted.items()):
                event["status"], event["reason"] = "invalid_frame", "partial_or_unexpected_sync_counts"
        if event["status"] != "ok":
            result["reason"] = f"{label}: {event['status']}"
            break
    if len(result["batches"]) == len(batches) and all(e["status"] == "ok" for e in result["batches"]):
        result["status"] = "ok"
    result["duration_ms"] = context["helpers"].rounded((time.perf_counter() - started) * 1000)
    return result


def inspect_catalog(db: Path, deadline: float) -> dict:
    check_deadline(deadline)
    # Python SQLite is ONLY a read-only count observer, NOT product runtime evidence.
    with sqlite3.connect(db.as_uri() + "?mode=ro", uri=True, timeout=1) as connection:
        connection.set_progress_handler(lambda: int(ev.remaining(deadline) <= 0), 10000)
        queries = {"messages": "SELECT count(*) FROM catalog WHERE id GLOB 'msg_v1_*'",
                   "fts_messages": "SELECT count(*) FROM fts_ids WHERE wire_id GLOB 'msg_v1_*'",
                   "fts_rows": "SELECT count(*) FROM fts",
                   "sources": "SELECT count(*) FROM source_scans"}
        counts = {name: connection.execute(sql).fetchone()[0] for name, sql in queries.items()}
    return {"status": "complete", "counts": counts,
            "observer": "Python sqlite3 read-only counts; not the product runtime version"}


def check_sources(directory: Path, metadata: dict, deadline: float, helpers) -> str:
    digest = hashlib.sha256()
    for item in metadata["files"]:
        check_deadline(deadline)
        path = directory / item["path"]
        ev.verify_artifact(item, path, helpers)
        digest.update(item["path"].encode("utf-8") + b"\0")
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                check_deadline(deadline)
                digest.update(chunk)
        digest.update(b"\0")
    return digest.hexdigest()


def storage_artifacts(db: Path, deadline: float) -> list[dict]:
    result = []
    for suffix in ("", "-wal", "-shm", "-journal"):
        path = Path(str(db) + suffix)
        if not path.is_file():
            continue
        digest = hashlib.sha256()
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                check_deadline(deadline)
                digest.update(chunk)
        result.append({"path": path.name, "bytes": path.stat().st_size, "sha256": digest.hexdigest()})
    return result


def wire_id(index: int) -> str:
    return f"msg_v1_00000000-0000-4000-8000-{index - 1:012d}"


def finish_metrics(row: dict, profile: dict, helpers) -> None:
    row["metrics"] = {"startup_ms": ev.make_metric(row["startup"], profile["startup"], helpers),
                      "initial_sync_ms": ev.make_metric(row["initial_sync"], 1, helpers),
                      "search_ms": ev.make_metric(row["search"], profile["search"], helpers),
                      "noop_sync_ms": ev.make_metric(row["noop_sync"], profile["noop"], helpers)}
    memory = [value for event in all_events(row) for value in event.get("peak_rss_mb_raw_samples", [])]
    row["memory"] = {"unit": "MiB", "raw_samples": memory, "summary": helpers.rounded_summary(memory) if memory else None}


def all_events(row: dict) -> list[dict]:
    result = [*row["warmup"], *row["startup"], *row["search"]]
    result.extend(event for item in [*row["initial_sync"], *row["noop_sync"]] for event in item["batches"])
    return result


def query_pairs(fixture: dict) -> list[tuple[str, str]]:
    by_id = {query["id"]: query["text"] for query in fixture["queries"]}
    pairs = [(query_id, by_id[query_id]) for query_id in SEARCH_QUERY_IDS]
    ev.require(all(text for _, text in pairs), "search query ids are missing from the fixture")
    return pairs


def wire_index(wire_id) -> int | None:
    prefix = "msg_v1_00000000-0000-4000-8000-"
    if isinstance(wire_id, str) and wire_id.startswith(prefix):
        try:
            return int(wire_id[len(prefix):]) + 1
        except ValueError:
            return None
    return None


def search_event(binary: Path, db: Path, index: int, query_id: str, text: str,
                 timeout: float, context: dict) -> dict:
    command = [str(binary), "--db", str(db), "--output", "json", "search", text, "--max-items", "20"]
    event = command_event(command, f"search-{index:03d}", timeout, context)
    event["query_id"] = query_id
    event["query_text"] = text
    if event["status"] == "ok":
        hits = event["frame_data"].get("hits")
        if type(hits) is not list:
            event["status"], event["reason"] = "invalid_frame", "search frame lacks hits array"
        else:
            ids = [hit.get("id") for hit in hits if isinstance(hit, dict)]
            event["hits"] = len(hits)
            event["hit_ids"] = ids
            event["hit_message_indexes"] = [value for value in (wire_index(i) for i in ids) if value is not None]
    return event


def run_scale(binary: Path, size: int, args, profile: dict, fixture: dict,
              scratch: Path, base_context: dict, helpers,
              startup: list, warmup: list) -> dict:
    row: dict = {"messages": size, "status": "incomplete", "startup": startup, "warmup": warmup}
    directory = scratch / f"baseline-{size}"
    directory.mkdir()
    deadline = time.monotonic() + args.scale_timeout_seconds
    context = {**base_context, "scratch": directory, "deadline": deadline}
    metadata: dict = {}
    paths = generate_dataset(directory / "dataset", size, args.messages_per_file, deadline, metadata, helpers)
    expected_by_name = {item["path"]: item["messages"] for item in metadata["files"]}
    row["dataset"] = metadata
    db = directory / "catalog.db"
    base = [str(binary), "--db", str(db), "--output", "json", "sync"]
    batches = source_batches(base, paths, args.max_sources_per_batch, args.argv_units)
    row["batch_count"] = len(batches)
    initial = sync_pass("initial", base, batches, expected_by_name, args.command_timeout_seconds, context, noop=False)
    row["initial_sync"] = [initial]
    noops = [sync_pass(f"noop-{index}", base, batches, expected_by_name,
                       args.command_timeout_seconds, context, noop=True)
             for index in range(profile["noop"])]
    row["noop_sync"] = noops
    pairs = query_pairs(fixture)
    searches = [search_event(binary, db, index, *pairs[index % len(pairs)],
                             args.command_timeout_seconds, context)
                for index in range(profile["search"])]
    for event in searches:
        qid = event["query_id"]
        qrel = set(ev.qrels(fixture, qid))
        event["qrels_count"] = len(qrel)
        if event["status"] == "ok":
            matched = len(set(event["hit_message_indexes"]) & qrel)
            event["matched_qrels"] = matched
            event["recall_at_10"] = matched / len(qrel) if qrel else None
    row["search"] = searches
    catalog = inspect_catalog(db, deadline) if initial["status"] == "ok" else {"status": "not_started"}
    row["catalog"] = catalog
    source_hash = check_sources(directory / "dataset", metadata, deadline, helpers)
    row["source_tree_hash_verified"] = source_hash == metadata["source_tree_hash"]
    row["storage"] = storage_artifacts(db, deadline)
    finish_metrics(row, profile, helpers)
    positives = all(event["status"] == "ok" and (event.get("matched_qrels") or 0) >= 1 for event in searches)
    counts_ok = (catalog.get("status") == "complete"
                 and catalog["counts"]["messages"] == size
                 and catalog["counts"]["fts_messages"] == size)
    ok = (initial["status"] == "ok" and all(item["status"] == "ok" for item in noops)
          and positives and counts_ok and row["source_tree_hash_verified"]
          and all(metric["status"] == "complete" for metric in row["metrics"].values()))
    if ok:
        row["status"] = "complete"
    else:
        row["reason"] = "one or more product stages did not complete or validate"
    return row


def validate_row(row: dict, profile: dict, samples: int, helpers) -> None:
    ev.require(row.get("status") in {"complete", "incomplete"}, "invalid scale status")
    if row["status"] != "complete":
        return
    dataset = row["dataset"]
    ev.require(dataset["status"] == "complete" and dataset["contains_real_transcripts"] is False,
               "dataset metadata is not complete/synthetic")
    ev.require(len(row["initial_sync"]) == 1 and len(row["noop_sync"]) == profile["noop"],
               "sync sample counts changed")
    ev.require(len(row["search"]) == samples, "search sample count changed")
    ev.validate_metric(row["metrics"]["startup_ms"], row["startup"], profile["startup"], helpers)
    ev.validate_metric(row["metrics"]["initial_sync_ms"], row["initial_sync"], 1, helpers)
    ev.validate_metric(row["metrics"]["search_ms"], row["search"], samples, helpers)
    ev.validate_metric(row["metrics"]["noop_sync_ms"], row["noop_sync"], profile["noop"], helpers)
    for event in [*row["startup"], *row["search"]]:
        ev.validate_event(event, helpers)
    for group in [*row["initial_sync"], *row["noop_sync"]]:
        for event in group["batches"]:
            ev.validate_event(event, helpers)
    ev.require(row["catalog"]["counts"]["messages"] == row["messages"], "catalog message count mismatch")
    ev.require(row["catalog"]["counts"]["fts_messages"] == row["messages"], "fts message count mismatch")


def run(args) -> int:
    workspace = ev.absolute_path(args.workspace, "workspace")
    binary = ev.absolute_path(args.binary, "binary")
    scratch = ev.absolute_path(args.scratch_dir, "scratch-dir")
    output = ev.absolute_path(args.output_dir, "output-dir")
    environment = ev.load_environment(ev.absolute_path(args.environment, "environment"))
    helpers = ev.load_helpers(workspace)
    commit = ev.check_workspace(workspace, args.expected_commit)
    sizes = ev.scales(args.scales or ("1000" if args.profile == "smoke" else "10000,100000,1000000"),
                      args.profile)
    profile = ev.PROFILES[args.profile]
    ev.require(type(args.messages_per_file) is int and args.messages_per_file >= 1, "invalid messages-per-file")
    for value in (args.scale_timeout_seconds, args.command_timeout_seconds):
        ev.finite(value, "timeout", positive=True)
    binary_before = ev.artifact(binary, helpers)
    report_path = output / f"search-baseline-{args.profile}.json"
    ev.require(not report_path.exists(), "report already exists; use a new explicit output directory")
    scratch.mkdir(parents=True, exist_ok=False)
    output.mkdir(parents=True, exist_ok=True)
    roots = {"workspace": workspace, "scratch": scratch, "output": output, "binary": binary}
    context = {"workspace": workspace, "scratch": scratch, "deadline": time.monotonic() + args.command_timeout_seconds,
               "helpers": helpers, "roots": roots}
    report = {"schema_version": "wake-reuse-search.baseline/v1", "status": "incomplete",
              "contains_real_transcripts": False, "commit": commit,
              "generated_at_utc": helpers.utc_now(), "profile": args.profile,
              "requested_scales": sizes, "environment": environment,
              "provenance": ev.source_provenance(workspace, helpers), "binary": binary_before,
              "binary_provenance": "unmodified_product_release_binary",
              "query_ids": SEARCH_QUERY_IDS, "scales": [],
              "limitations": ["Product CLI levels are end-to-end process measurements on synthetic data.",
                              "Initial sync may run in bounded source batches; duration is batch-total, not one command.",
                              "Python sqlite3 is a read-only count observer, not product runtime evidence.",
                              "No OS cache flush: samples are warm, never labeled cold-disk.",
                              "Search timing includes CLI startup, DB open and rendering; not an FTS-only latency."]}
    fixture = ev.load_fixture()
    try:
        warmup = [command_event([str(binary), "--version"], f"startup-warmup-{index}",
                                args.command_timeout_seconds, context, robot=False) for index in range(3)]
        startup = [command_event([str(binary), "--version"], f"startup-{index:03d}",
                                 args.command_timeout_seconds, context, robot=False)
                   for index in range(profile["startup"])]
    except (ValueError, RuntimeError, OSError, KeyError) as error:
        startup, warmup = [], []
        report["setup_error"] = ev.redact(str(error), roots)
    report["startup_warmup"] = warmup
    report["startup"] = startup
    report["metrics"] = {"startup_ms": ev.make_metric(startup, profile["startup"], helpers)}
    for size in sizes:
        try:
            report["scales"].append(run_scale(binary, size, args, profile, fixture, scratch, context, helpers,
                                              startup, warmup))
        except (ValueError, RuntimeError, OSError, KeyError, TimeoutError) as error:
            report["scales"].append({"messages": size, "status": "incomplete",
                                     "reason": ev.redact(str(error), roots)})
    report["binary_after"] = ev.artifact(binary, helpers)
    complete = (not report.get("setup_error")
                and report["metrics"]["startup_ms"]["status"] == "complete"
                and all(row["status"] == "complete" for row in report["scales"]))
    report["status"] = "complete" if complete else "incomplete"
    ev.seal(report)
    validate(report, profile, helpers)
    ev.write_report(report_path, report)
    print(json.dumps({"report": report_path.name, "status": report["status"]}))
    return 0 if report["status"] == "complete" else 2


def validate(report: dict, profile: dict | None = None, helpers=None) -> None:
    """Replay-validate a sealed report; deep metric replay requires helpers."""
    ev.verify_seal(report)
    ev.require(report.get("schema_version") == "wake-reuse-search.baseline/v1", "unknown schema version")
    ev.require(report.get("contains_real_transcripts") is False, "not synthetic-only")
    ev.require(report.get("status") in {"complete", "incomplete"}, "invalid report status")
    profile = profile or ev.PROFILES[report["profile"]]
    ev.require(report.get("profile") in ev.PROFILES, "unknown profile")
    ev.require(len(report.get("query_ids", [])) == len(SEARCH_QUERY_IDS), "query set changed")
    for row in report["scales"]:
        ev.require(row.get("status") in {"complete", "incomplete"}, "invalid scale status")
        if row["status"] == "complete":
            ev.require(row.get("reason") is None, "complete row carries a failure reason")
            if helpers is not None:
                validate_row(row, profile, len(row["search"]), helpers)
        else:
            ev.require(bool(row.get("reason")), "incomplete row lacks a reason")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("run")
    for name in ("workspace", "binary", "scratch-dir", "output-dir", "environment"):
        p.add_argument(f"--{name}", required=True)
    p.add_argument("--profile", choices=ev.PROFILES, default="smoke")
    p.add_argument("--scales")
    p.add_argument("--scale-timeout-seconds", type=float, default=1800)
    p.add_argument("--command-timeout-seconds", type=float, default=600)
    p.add_argument("--messages-per-file", type=int, default=5000)
    p.add_argument("--max-sources-per-batch", type=int, default=40)
    p.add_argument("--argv-units", type=int, default=30000)
    p.add_argument("--expected-commit", default=ev.BASELINE_COMMIT)
    v = sub.add_parser("validate")
    v.add_argument("report")
    args = parser.parse_args()
    if args.command == "run":
        return run(args)
    report = ev.read_json(Path(args.report))
    validate(report)
    print(json.dumps({"report": args.report, "status": "validated"}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
