#!/usr/bin/env python3
"""Run the independent Rust search experiment with hard deadlines, or validate it."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
import time

import evidence as ev


def dependency_graph(args, workspace: Path, scratch: Path, helpers, roots: dict) -> dict:
    command = [args.cargo, "metadata", "--locked", "--offline", "--format-version", "1",
               "--manifest-path", str(ev.ROOT / "Cargo.toml"), "--no-default-features"]
    if args.feature == "cache":
        command += ["--features", "cache"]
    event, text = ev.run_process(command, workspace=workspace, scratch=scratch, label="cargo-metadata",
                                 timeout=args.setup_timeout_seconds,
                                 deadline=time.monotonic() + args.setup_timeout_seconds,
                                 helpers=helpers, roots=roots, preview=False)
    ev.require(event["status"] == "ok", "offline locked cargo metadata failed")
    metadata = json.loads(text)
    by_id = {p["id"]: p for p in metadata["packages"]}
    nodes = []
    for node in metadata["resolve"]["nodes"]:
        package = by_id[node["id"]]
        nodes.append({"name": package["name"], "version": package["version"],
                      "source": package["source"] or "local_path",
                      "features": sorted(node["features"]), "license": package.get("license")})
    nodes.sort(key=lambda node: (node["name"], node["version"]))
    return {"nodes": nodes, "sha256": ev.canonical_hash(nodes), "process": event}


def summarize_result(result: dict, helpers) -> None:
    measurements = [query[strategy]["measurement"] for query in result["queries"]
                    for strategy in ("cjk", "trigram_like")]
    measurements.extend(mode for case in result["cache_cases"] for mode in case["modes"])
    for measurement in measurements:
        measurement["summaries"] = {
            field: helpers.rounded_summary([float(row[field]) for row in measurement["raw_samples"]])
            for field in ("query_ms", "open_ms", "lifecycle_ms")}


def validate_ids(ids: list, messages: int) -> None:
    ev.require(isinstance(ids, list) and len(ids) <= 10 and len(set(ids)) == len(ids)
               and all(type(n) is int and 1 <= n <= messages for n in ids), "invalid retrieved IDs")


def validate_measurement(value: dict, messages: int, samples: int, helpers) -> None:
    ids = value.get("ids")
    validate_ids(ids, messages)
    ev.require(value.get("required_sample_count") == samples, "measurement sample requirement changed")
    rows = value.get("raw_samples", [])
    ev.require(len(rows) == samples and samples > 0, "missing/partial raw micro samples")
    ev.require(value.get("method") in {"prepare", "prepare_cached"} and
               value.get("lifetime") in {"persistent_connection", "reopen_per_request"}, "invalid measurement mode")
    for index, row in enumerate(rows):
        ev.require(row.get("index") == index and row.get("ids") == ids, "sample IDs/order changed")
        for field in ("query_ms", "open_ms", "lifecycle_ms"):
            ev.finite(row.get(field), field)
        ev.require(row["lifecycle_ms"] + 1e-6 >= row["query_ms"] + row["open_ms"], "inconsistent lifecycle duration")
        if value["lifetime"] == "persistent_connection":
            ev.require(row["open_ms"] == 0, "persistent measurement unexpectedly reopened")
    expected = {field: helpers.rounded_summary([float(row[field]) for row in rows])
                for field in ("query_ms", "open_ms", "lifecycle_ms")}
    ev.require(value.get("summaries") == expected, "micro statistics differ from raw samples")
    warm = value.get("warmup", {})
    ev.require(warm.get("count") == 1, "warmup count changed")
    ev.finite(warm.get("query_ms"), "warmup query")
    ev.finite(warm.get("initial_open_ms"), "initial open")
    retained = value["method"] == "prepare_cached" and value["lifetime"] == "persistent_connection"
    ev.require(warm.get("statement_retained") is retained, "warmup cache lifetime mislabeled")


def validate_plan(statement: dict, plan: list) -> None:
    ev.require(isinstance(statement.get("sql"), str) and statement["sql"].endswith("LIMIT 10"), "unbounded/missing SQL")
    ev.require(isinstance(statement.get("parameters"), list), "missing SQL parameters")
    ev.require(bool(plan) and all(isinstance(row.get("detail"), str) and row["detail"] for row in plan), "missing query plan")


def validate_result(result: dict, messages: int, samples: int, cache: bool, helpers) -> None:
    ev.require(result.get("schema_version") == "wake-reuse-search.micro/v1" and
               result.get("status") == "complete", "Rust result is not complete")
    ev.require(result.get("contains_real_transcripts") is False, "not synthetic-only")
    ev.require(result.get("messages") == messages and result.get("samples_per_query") == samples,
               "actual Rust input counts differ from requested counts")
    fixture = ev.load_fixture()
    ev.require(result.get("fixture_revision") == fixture["revision"] and
               result.get("fixture_hash") == helpers.sha256_file(ev.FIXTURE_PATH), "fixture hash/revision mismatch")
    ev.require(result.get("hash_method") == ev.HASH_METHOD and
               result.get("dataset_hash") == ev.corpus_hash(messages), "deterministic data hash mismatch")
    ev.require(result.get("row_counts") == {name: messages for name in ("corpus", "cjk_fts", "trigram_fts")},
               "partial/missing index rows")
    runtime = result.get("runtime", {})
    ev.require(runtime.get("rusqlite_version") == "0.40.2" and runtime.get("bundled") is True and
               runtime.get("cache_feature") is cache and runtime.get("default_features") is False and
               bool(runtime.get("sqlite_version")) and bool(runtime.get("sqlite_source_id")), "runtime mismatch")
    ev.require(result.get("cache_capacity") == (16 if cache else None), "cache capacity mismatch")
    build = result.get("build", {})
    for field in ("corpus_ms", "cjk_ms", "trigram_ms", "db_bytes"):
        ev.finite(build.get(field), field, positive=(field == "db_bytes"))
    ev.digest(build.get("db_hash"), "db_hash")
    queries = result.get("queries", [])
    ev.require(len(queries) == len(fixture["queries"]), "query coverage incomplete")
    for row, expected in zip(queries, fixture["queries"]):
        ev.require(row.get("query") == expected, "query specification changed")
        relevance = ev.qrels(fixture, expected["id"])
        ev.require(row.get("qrels") == relevance, "qrels do not match independently assigned beacons")
        ids = {}
        for name in ("cjk", "trigram_like"):
            branch = row[name]
            measurement = branch["measurement"]
            validate_measurement(measurement, messages, samples, helpers)
            ev.require(measurement["method"] == "prepare" and measurement["lifetime"] == "persistent_connection",
                       "strategy comparison must hold method/lifetime constant")
            validate_plan(branch["statement"], branch["query_plan"])
            ids[name] = measurement["ids"]
            matched = len(set(ids[name]) & set(relevance))
            ev.require(branch.get("relevant_retrieved") == matched and
                       branch.get("recall_at_10") == (matched / len(relevance) if relevance else None), "recall mismatch")
            ev.require(branch.get("missing_relevant") == [i for i in relevance if i not in ids[name]] and
                       branch.get("unexpected_ids") == [i for i in ids[name] if i not in relevance], "missing mismatch evidence")
            if expected["id"] == "negative":
                ev.require(not ids[name], "negative control returned an accidental filler hit")
        a, b = ids["cjk"], ids["trigram_like"]
        ev.require(row.get("mismatch") == {"same_ordered_ids": a == b,
                   "only_cjk": [i for i in a if i not in b], "only_trigram_like": [i for i in b if i not in a]},
                   "strategy mismatch summary changed")
    cases = result.get("cache_cases", [])
    ev.require(len(cases) == 3, "cache SQL cases incomplete")
    wanted = [(life, method) for life in ("persistent_connection", "reopen_per_request")
              for method in (("prepare", "prepare_cached") if cache else ("prepare",))]
    for case in cases:
        validate_plan(case["statement"], case["query_plan"])
        ev.require(case.get("reference_ids") and case.get("all_ids_equal") is True, "cache reference missing")
        ev.require([(v["lifetime"], v["method"]) for v in case["modes"]] == wanted, "cache/lifetime matrix incomplete")
        for mode in case["modes"]:
            validate_measurement(mode, messages, samples, helpers)
            ev.require(mode["ids"] == case["reference_ids"], "prepare and cached results are not identical")


def validate_report(report: dict, helpers, *, binary: Path | None = None, scratch: Path | None = None) -> None:
    ev.verify_seal(report)
    ev.require(report.get("schema_version") == "wake-reuse-search.run/v1", "unknown search report schema")
    ev.require_commit_id(report.get("commit"), "report commit")
    ev.require(report.get("profile") in ev.PROFILES, "unrecognized profile")
    ev.require(report.get("contains_real_transcripts") is False, "not synthetic-only")
    samples = report.get("samples_per_query")
    ev.require(type(samples) is int and 1 <= samples <= 10000, "invalid sample requirement")
    messages = ev.scales(",".join(str(n) for n in report["requested_scales"]), report["profile"])
    if report["profile"] == "smoke":
        ev.require(samples <= 5, "smoke exceeds bounded sample cap")
    ev.require(report.get("feature") in {"cache", "no-cache"}, "unknown cache feature")
    ev.digest(report["binary"]["sha256"], "binary")
    ev.require(report.get("binary_after") == report["binary"], "binary changed during the experiment")
    if binary is not None:
        ev.verify_artifact(report["binary"], binary, helpers)
    setup = report.get("setup_error")
    if not setup:
        graph = report["dependencies"]
        ev.require(graph["sha256"] == ev.canonical_hash(graph["nodes"]) and graph["nodes"], "dependency graph hash mismatch")
        selected = next(node for node in graph["nodes"] if node["name"] == "rusqlite")
        ev.require(("cache" in selected["features"]) == (report["feature"] == "cache"), "resolved features do not match binary label")
    rows = report.get("scales", [])
    ev.require([row["messages"] for row in rows] == messages, "missing requested scales")
    for row in rows:
        if row.get("process"):
            ev.validate_event(row["process"], helpers)
            if scratch is not None:
                ev.verify_logs(row["process"], scratch / row["scratch_subdir"], helpers)
        if row["status"] == "complete":
            ev.require(not setup and row["process"]["status"] == "ok", "successful scale contains process/setup failure")
            validate_result(row["result"], row["messages"], samples, report["feature"] == "cache", helpers)
            if scratch is not None:
                db = scratch / row["scratch_subdir"] / "search.db"
                ev.verify_artifact({"bytes": row["result"]["build"]["db_bytes"],
                                    "sha256": row["result"]["build"]["db_hash"]}, db, helpers)
        else:
            ev.require(row["status"] == "incomplete" and bool(row.get("reason")), "failure lacks explicit reason")
    complete = not setup and all(row["status"] == "complete" for row in rows)
    ev.require(report.get("status") == ("complete" if complete else "incomplete"), "partial experiment mislabeled success")


def run(args) -> int:
    workspace = ev.absolute_path(args.workspace, "workspace")
    binary = ev.absolute_path(args.binary, "binary")
    scratch = ev.absolute_path(args.scratch_dir, "scratch-dir")
    output = ev.absolute_path(args.output_dir, "output-dir")
    environment = ev.load_environment(ev.absolute_path(args.environment, "environment"))
    helpers = ev.load_helpers(workspace)
    commit = ev.check_workspace(workspace, args.expected_commit)
    sizes = ev.scales(args.scales or ("1000" if args.profile == "smoke" else "10000,100000,1000000"), args.profile)
    samples = args.samples if args.samples is not None else (5 if args.profile == "smoke" else 100)
    ev.require(1 <= samples <= (5 if args.profile == "smoke" else 10000), "invalid sample count")
    for value in (args.scale_timeout_seconds, args.setup_timeout_seconds):
        ev.finite(value, "timeout", positive=True)
    binary_before = ev.artifact(binary, helpers)
    report_path = output / f"search-micro-{args.profile}-{args.feature}.json"
    ev.require(not report_path.exists(), "report already exists; use a new explicit output directory")
    scratch.mkdir(parents=True, exist_ok=False)
    output.mkdir(parents=True, exist_ok=True)
    roots = {"workspace": workspace, "scratch": scratch, "output": output, "binary": binary}
    report = {"schema_version": "wake-reuse-search.run/v1", "status": "incomplete",
              "contains_real_transcripts": False, "commit": commit, "generated_at_utc": helpers.utc_now(),
              "profile": args.profile, "feature": args.feature, "requested_scales": sizes,
              "samples_per_query": samples, "scale_timeout_seconds": args.scale_timeout_seconds,
              "environment": environment, "provenance": ev.source_provenance(workspace, helpers),
              "binary": binary_before, "binary_provenance": "caller_supplied_experimental_release",
              "setup_error": None, "scales": [],
              "limitations": ["Synthetic local microbenchmark, not a Wake or product end-to-end comparison.",
                              "Caller-supplied binary hash pins the artifact; source-to-binary linkage is not independently proven.",
                              "No cold-disk claims; per-scale subprocess is killed on a finite deadline.",
                              "RSS uses 100 ms samples plus final Windows peak; short runs may lack samples; macOS RSS unavailable."]}
    try:
        report["sqlite_runtime_evidence"] = ev.runtime_evidence(binary, workspace, scratch, helpers, roots,
                args.setup_timeout_seconds, time.monotonic() + args.setup_timeout_seconds)
        ev.require(report["sqlite_runtime_evidence"]["runtime"]["cache_feature"] == (args.feature == "cache"), "binary feature mismatch")
        report["dependencies"] = dependency_graph(args, workspace, scratch, helpers, roots)
    except (ValueError, RuntimeError, OSError, KeyError) as error:
        report["setup_error"] = ev.redact(str(error), roots)
    for size in sizes:
        directory = scratch / f"micro-{size}"
        row = {"messages": size, "scratch_subdir": directory.name, "status": "incomplete", "result": None}
        report["scales"].append(row)
        if report["setup_error"]:
            row["reason"] = "not_started_due_to_setup_failure"
            continue
        directory.mkdir()
        deadline = time.monotonic() + args.scale_timeout_seconds
        try:
            event, text = ev.run_process([str(binary), "run", "--db", str(directory / "search.db"),
                                         "--messages", str(size), "--samples", str(samples)],
                                        workspace=workspace, scratch=directory, label="micro", timeout=args.scale_timeout_seconds,
                                        deadline=deadline, helpers=helpers, roots=roots)
            row["process"] = event
            row["progress"] = []
            final = None
            for line in text.split("\n"):
                if not line.strip():
                    continue
                try:
                    frame = json.loads(line)
                except json.JSONDecodeError:
                    if event["status"] == "ok":
                        raise ValueError("invalid Rust JSON output") from None
                    continue
                if frame.get("kind") == "progress":
                    row["progress"].append(frame)
                elif frame.get("kind") == "result":
                    ev.require(final is None, "duplicate Rust result")
                    final = frame.get("report")
            ev.require(event["status"] == "ok", f"Rust process {event['status']}")
            ev.require(final is not None, "process exited without a complete result")
            summarize_result(final, helpers)
            validate_result(final, size, samples, args.feature == "cache", helpers)
            row["result"], row["status"] = final, "complete"
        except (ValueError, RuntimeError, OSError, KeyError) as error:
            row["reason"] = ev.redact(str(error), roots)
    report["binary_after"] = ev.artifact(binary, helpers)
    if not report["setup_error"] and all(row["status"] == "complete" for row in report["scales"]):
        report["status"] = "complete"
    ev.seal(report)
    validate_report(report, helpers)
    ev.write_report(report_path, report)
    print(json.dumps({"report": report_path.name, "status": report["status"]}))
    return 0 if report["status"] == "complete" else 2


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("run")
    for name in ("workspace", "binary", "scratch-dir", "output-dir", "environment"):
        p.add_argument(f"--{name}", required=True)
    p.add_argument("--profile", choices=ev.PROFILES, default="smoke")
    p.add_argument("--feature", choices=("cache", "no-cache"), required=True)
    p.add_argument("--scales")
    p.add_argument("--samples", type=int)
    p.add_argument("--scale-timeout-seconds", type=float, default=600)
    p.add_argument("--setup-timeout-seconds", type=float, default=60)
    p.add_argument("--expected-commit", required=True,
                   help="explicit full commit id of the measured product tree")
    p.add_argument("--cargo", default="cargo")
    v = sub.add_parser("validate")
    v.add_argument("report")
    v.add_argument("--workspace", required=True)
    v.add_argument("--binary")
    v.add_argument("--scratch-dir")
    v.add_argument("--allow-incomplete", action="store_true", help="validate failure evidence, never relabel it complete")
    args = parser.parse_args()
    if args.command == "run":
        return run(args)
    helpers = ev.load_helpers(ev.absolute_path(args.workspace, "workspace"))
    report = ev.read_json(Path(args.report))
    validate_report(report, helpers, binary=ev.absolute_path(args.binary, "binary") if args.binary else None,
                    scratch=ev.absolute_path(args.scratch_dir, "scratch-dir") if args.scratch_dir else None)
    print(json.dumps({"valid_evidence": True, "status": report["status"]}))
    return 0 if report["status"] == "complete" or args.allow_incomplete else 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, KeyError, OSError) as error:
        print(f"invalid search evidence/input: {error}", file=sys.stderr)
        raise SystemExit(1)
