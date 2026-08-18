#!/usr/bin/env python3
"""Semantic recall gate for the agent-session-grep CLI (semantic-candle feature).

Frozen benchmark gate for the optional multilingual-e5-small Candle backend.
Uses a deterministic synthetic corpus with ground-truth relevance labels
(paraphrase topics + distractors, no real transcripts) and measures, per
retrieval mode (lexical / semantic / hybrid):

- recall@k against ground truth,
- p50/p95 search latency.

The harness refuses to fabricate semantic numbers. When no verified local
bundle is present (`asg model status` -> present/verified false), it emits a
`model_not_imported` manifest under the output directory and exits with the
dedicated exit code 2 (EXIT_MODEL_NOT_IMPORTED) instead of pretending the
semantic mode measured anything.

Exit codes: 0 = locally verified evidence emitted, 1 = harness/CLI error,
2 = model_not_imported (semantic measurements skipped on purpose).

Uses only the Python standard library. Never discovers or reads provider data
roots; all corpus data is generated in-process.
"""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import math
import os
import platform
import statistics
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

SCHEMA_VERSION = "agent-session-grep.semantic-benchmark/v1"
EXIT_OK = 0
EXIT_ERROR = 1
EXIT_MODEL_NOT_IMPORTED = 2

MODES = ("lexical", "semantic", "hybrid")

PROFILES = {
    "smoke": {"topics": 4, "distractors": 4, "repeat": 2},
    "full": {"topics": 8, "distractors": 8, "repeat": 5},
}

# Each topic contributes 4 relevant messages: 2 anchors sharing the query's
# distinctive term (lexical-friendly) and 2 paraphrases sharing no token with
# the query (only a semantic model can find them). Ground truth per query is
# "every message whose text ends with the topic marker".
TOPICS: list[dict[str, Any]] = [
    {
        "query": "database backup",
        "anchors": [
            "Please schedule the database backup for friday night",
            "The database backup job failed with exit code 3",
        ],
        "paraphrases": [
            "We need a nightly snapshot of the datastore so we can survive a disk loss",
            "The restore drill proved our archived copies are stale",
        ],
    },
    {
        "query": "network timeout",
        "anchors": [
            "The network timeout setting is too low for this API",
            "Fix the network timeout configuration in the client",
        ],
        "paraphrases": [
            "Remote calls keep getting cut off after a few seconds of waiting",
            "The handshake gives up before the server answers",
        ],
    },
    {
        "query": "token budget",
        "anchors": [
            "The token budget for this request was exceeded",
            "Reduce the token budget to fit the context window",
        ],
        "paraphrases": [
            "We ran out of room in the prompt allowance again",
            "The context allowance must shrink before the next run",
        ],
    },
    {
        "query": "ui button color",
        "anchors": [
            "Change the ui button color to blue",
            "The ui button color is inconsistent across pages",
        ],
        "paraphrases": [
            "The clickable controls should use a cooler tone for consistency",
            "Make every interactive element share one palette",
        ],
    },
    {
        "query": "python dependency conflict",
        "anchors": [
            "Python dependency conflict between requests and urllib3",
            "Resolve the python dependency conflict in the lockfile",
        ],
        "paraphrases": [
            "Two libraries fight over the same package version and break the build",
            "The pinned package set cannot be installed together",
        ],
    },
    {
        "query": "sqlite wal checkpoint",
        "anchors": [
            "Run a sqlite wal checkpoint before backup",
            "The sqlite wal checkpoint takes too long",
        ],
        "paraphrases": [
            "The write-ahead log must be folded back into the main file periodically",
            "Flush the sidecar journal into the data file first",
        ],
    },
    {
        "query": "数据库备份",
        "anchors": [
            "请安排周五晚上的数据库备份任务",
            "数据库备份作业以退出码 3 失败",
        ],
        "paraphrases": [
            "每晚将资料库转储到离线磁带以抵御磁盘损坏",
            "恢复演练证明归档副本已经过期",
        ],
    },
    {
        "query": "上下文长度",
        "anchors": [
            "上下文长度超过了模型限制",
            "请把上下文长度压缩到窗口以内",
        ],
        "paraphrases": [
            "提示词塞不下更多的历史消息了",
            "输入规模必须缩小才能继续处理",
        ],
    },
]

DISTRACTORS = [
    "Reminder to update the changelog before the release",
    "The team meeting moved to thursday afternoon",
    "Pinned the rust toolchain to a stable version",
    "Uploaded the screenshots to the shared drive",
    "The printer needs a new toner cartridge",
    "Booked the conference room for the design review",
    "The build badge turned green after the fix",
    "Submitted the expense report for the trip",
]


def marker_for(topic_index: int) -> str:
    return f"[topic-{topic_index:02d}]"


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_hash(paths: Iterable[Path], root: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(paths, key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix().encode("utf-8")
        digest.update(relative)
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def nearest_rank(values: list[float], percentile: int) -> float:
    ordered = sorted(values)
    rank = max(1, math.ceil(percentile / 100 * len(ordered)))
    return ordered[rank - 1]


def summarize(values: list[float]) -> dict[str, float | int]:
    if not values:
        raise ValueError("cannot summarize an empty sample set")
    return {
        "count": len(values),
        "min": min(values),
        "max": max(values),
        "p50": nearest_rank(values, 50),
        "p95": nearest_rank(values, 95),
        "p99": nearest_rank(values, 99),
        "mean": statistics.fmean(values),
        "sample_stddev": statistics.stdev(values) if len(values) > 1 else 0.0,
    }


def rounded(value: float) -> float:
    return round(value, 6)


def rounded_summary(values: list[float]) -> dict[str, float | int]:
    return {
        key: rounded(value) if isinstance(value, float) else value
        for key, value in summarize(values).items()
    }


def assert_summary(samples: list[float], actual: dict[str, Any], label: str) -> None:
    expected = rounded_summary(samples)
    for key, value in expected.items():
        if actual.get(key) != value:
            raise ValueError(
                f"{label}.summary.{key}: expected {value!r}, got {actual.get(key)!r}"
            )


def total_memory_bytes() -> int | None:
    if os.name == "nt":
        class MemoryStatus(ctypes.Structure):
            _fields_ = [
                ("dwLength", ctypes.c_ulong),
                ("dwMemoryLoad", ctypes.c_ulong),
                ("ullTotalPhys", ctypes.c_ulonglong),
                ("ullAvailPhys", ctypes.c_ulonglong),
                ("ullTotalPageFile", ctypes.c_ulonglong),
                ("ullAvailPageFile", ctypes.c_ulonglong),
                ("ullTotalVirtual", ctypes.c_ulonglong),
                ("ullAvailVirtual", ctypes.c_ulonglong),
                ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
            ]

        status = MemoryStatus()
        status.dwLength = ctypes.sizeof(status)
        return (
            int(status.ullTotalPhys)
            if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status))
            else None
        )
    try:
        if sys.platform == "darwin":
            result = subprocess.run(
                ["sysctl", "-n", "hw.memsize"],
                capture_output=True,
                text=True,
                check=True,
            )
            return int(result.stdout.strip())
        pages = os.sysconf("SC_PHYS_PAGES")
        page_size = os.sysconf("SC_PAGE_SIZE")
        return int(pages * page_size)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        return None


def command_text(command: list[str], cwd: Path) -> str | None:
    try:
        result = subprocess.run(
            command, cwd=cwd, capture_output=True, text=True, check=True
        )
        return result.stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return None


def environment(workspace: Path, args: argparse.Namespace) -> dict[str, Any]:
    rustc = command_text([args.rustc, "-vV"], workspace)
    target = None
    if rustc:
        target = next(
            (
                line.split(":", 1)[1].strip()
                for line in rustc.splitlines()
                if line.startswith("host:")
            ),
            None,
        )
    memory = total_memory_bytes()
    return {
        "captured_at_utc": utc_now(),
        "os": platform.system().lower(),
        "os_release": platform.release(),
        "os_version": platform.version(),
        "target_triple": target or "not_recorded",
        "architecture": platform.machine() or "not_recorded",
        "cpu": platform.processor()
        or os.environ.get("PROCESSOR_IDENTIFIER")
        or "not_recorded",
        "logical_cpu_count": os.cpu_count(),
        "ram_gb": rounded(memory / (1024**3)) if memory else "not_recorded",
        "rustc": rustc or "not_recorded",
        "python_version": platform.python_version(),
    }


def resolve_binary(workspace: Path, args: argparse.Namespace) -> Path:
    if args.binary:
        binary = Path(args.binary).expanduser().resolve()
    else:
        subprocess.run(
            [
                args.cargo,
                "build",
                "--release",
                "--features",
                "semantic-candle",
                "-p",
                "agent-session-grep-cli",
                "--locked",
            ],
            cwd=workspace,
            check=True,
        )
        name = "agent-session-grep.exe" if os.name == "nt" else "agent-session-grep"
        binary = workspace / "target" / "release" / name
    if not binary.is_file():
        raise FileNotFoundError(f"release CLI binary not found: {binary}")
    return binary


def cli_frame(binary: Path, workspace: Path, command: list[str]) -> dict[str, Any]:
    result = subprocess.run(
        command, cwd=workspace, capture_output=True, text=True, encoding="utf-8"
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {command!r}\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )
    lines = result.stdout.splitlines()
    if not lines:
        raise RuntimeError(f"command produced no stdout: {command!r}")
    frame = json.loads(lines[0])
    if not isinstance(frame, dict) or frame.get("ok") is not True:
        raise RuntimeError(f"CLI did not return a successful frame: {lines[0]}")
    return frame


def model_status(binary: Path, workspace: Path) -> dict[str, Any]:
    frame = cli_frame(binary, workspace, [str(binary), "--output", "json", "model", "status"])
    return frame["data"]


def write_corpus(root: Path, topic_count: int, distractor_count: int) -> tuple[list[Path], dict[str, str]]:
    """Write the deterministic corpus and return (files, query -> marker)."""
    root.mkdir(parents=True, exist_ok=True)
    path = root / "semantic-corpus.jsonl"
    query_markers: dict[str, str] = {}
    record_index = 0
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        for topic_index, topic in enumerate(TOPICS[:topic_count]):
            marker = marker_for(topic_index)
            query_markers[topic["query"]] = marker
            for text in [*topic["anchors"], *topic["paraphrases"]]:
                handle.write(
                    json.dumps(
                        synthetic_record(record_index, f"{text} {marker}"),
                        ensure_ascii=False,
                        separators=(",", ":"),
                    )
                    + "\n"
                )
                record_index += 1
        for text in DISTRACTORS[:distractor_count]:
            handle.write(
                json.dumps(
                    synthetic_record(record_index, text),
                    ensure_ascii=False,
                    separators=(",", ":"),
                )
                + "\n"
            )
            record_index += 1
    return [path], query_markers


def synthetic_record(index: int, text: str) -> dict[str, Any]:
    role = "user" if index % 2 == 0 else "assistant"
    return {
        "type": role,
        "uuid": f"00000000-0000-4000-8000-{index:012d}",
        "parentUuid": None if index % 10 == 0 else f"00000000-0000-4000-8000-{index - 1:012d}",
        "timestamp": f"2026-02-{(index % 28) + 1:02d}T12:00:00.000Z",
        "isSidechain": False,
        "message": {"role": role, "content": text},
        "synthetic": True,
    }


def recall_at_k(hits: list[dict[str, Any]], marker: str) -> tuple[float, int, int]:
    """Recall@k of a single query against the marker-based ground truth.

    Relevant messages are every topic message (4 per topic); the retrieved set
    is the top-k hit texts ending with the marker.
    """
    relevant = 4
    retrieved_relevant = sum(
        1
        for hit in hits
        if isinstance(hit.get("text"), str) and hit["text"].rstrip().endswith(marker)
    )
    return retrieved_relevant / relevant, retrieved_relevant, relevant


def run_search_once(
    binary: Path, workspace: Path, db: Path, query: str, mode: str, k: int
) -> tuple[dict[str, Any], float]:
    started = time.perf_counter_ns()
    frame = cli_frame(
        binary,
        workspace,
        [
            str(binary),
            "--db",
            str(db),
            "--output",
            "json",
            "search",
            query,
            "--mode",
            mode,
            "--max-items",
            str(k),
        ],
    )
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    return frame, elapsed_ms


def measure_modes(
    binary: Path,
    workspace: Path,
    db: Path,
    query_markers: dict[str, str],
    k: int,
    repeat: int,
) -> dict[str, Any]:
    results: dict[str, Any] = {}
    for mode in MODES:
        latencies: list[float] = []
        query_rows: list[dict[str, Any]] = []
        effective_modes: set[str] = set()
        for query, marker in query_markers.items():
            recalls: list[float] = []
            hit_id_sets: list[tuple[str, ...]] = []
            effective_by_repeat: list[str] = []
            for _ in range(repeat):
                frame, elapsed_ms = run_search_once(
                    binary, workspace, db, query, mode, k
                )
                latencies.append(rounded(elapsed_ms))
                data = frame["data"]
                effective = data.get("retrieval_mode", "unknown")
                effective_by_repeat.append(effective)
                effective_modes.add(effective)
                hits = data.get("hits", [])
                recall, retrieved, relevant = recall_at_k(hits, marker)
                recalls.append(rounded(recall))
                hit_id_sets.append(tuple(hit.get("id") for hit in hits))
            if len(set(hit_id_sets)) > 1:
                raise RuntimeError(
                    f"nondeterministic ranking for {mode} query {query!r}: "
                    f"repeat hit id lists differ"
                )
            if len(set(effective_by_repeat)) > 1:
                raise RuntimeError(
                    f"nondeterministic effective mode for {mode} query {query!r}: "
                    f"{effective_by_repeat}"
                )
            query_rows.append(
                {
                    "query": query,
                    "relevant": relevant,
                    "retrieved_relevant": retrieved,
                    "recall_at_k": recalls[0],
                    "effective_mode": effective_by_repeat[0],
                }
            )
        mode_results = {
            "requested_mode": mode,
            "effective_modes_observed": sorted(effective_modes),
            "degraded_query_count": sum(
                1
                for row in query_rows
                if mode != "lexical" and row["effective_mode"] == "lexical_fallback"
            ),
            "mean_recall_at_k": rounded(
                sum(row["recall_at_k"] for row in query_rows) / len(query_rows)
            ),
            "queries": query_rows,
            "latency_ms": {
                "raw_samples": latencies,
                "summary": rounded_summary(latencies),
            },
        }
        results[mode] = mode_results
    return results


def run_benchmark(args: argparse.Namespace) -> tuple[int, Path]:
    workspace = Path(args.workspace).expanduser().resolve()
    output_dir = Path(args.output_dir).expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    if args.k < 1:
        raise ValueError("--k must be a positive integer")
    profile = PROFILES[args.profile]
    commit = command_text(["git", "rev-parse", "HEAD"], workspace) or "not_recorded"
    if args.expected_commit and commit != args.expected_commit:
        raise ValueError(
            f"workspace commit {commit!r} does not match --expected-commit {args.expected_commit!r}"
        )
    binary = resolve_binary(workspace, args)
    binary_provenance = "caller_supplied_prebuilt" if args.binary else "built_by_harness_from_workspace"
    status = model_status(binary, workspace)

    common: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "generated_at_utc": utc_now(),
        "commit": commit,
        "profile": args.profile,
        "environment": environment(workspace, args),
        "binary": {
            "hash_algorithm": "sha256",
            "binary_hash": sha256_file(binary),
            "artifact_size_bytes": binary.stat().st_size,
            "provenance": binary_provenance,
        },
        "model": {
            "feature": status.get("feature"),
            "present": status.get("present"),
            "verified": status.get("verified"),
            "detail": status.get("detail"),
        },
    }

    if status.get("present") is not True or status.get("verified") is not True:
        report: dict[str, Any] = {
            **common,
            "status": "model_not_imported",
            "dataset": None,
            "index": None,
            "results": None,
            "limitations": [
                "No verified local multilingual-e5-small bundle is present under the platform model cache "
                "(`asg model status` reports present/verified false), so semantic measurements were skipped "
                "by design. The offline import path is the product decision; this harness never downloads weights.",
                "Import a verified bundle (`asg model import --dir <bundle>`) and re-run to produce recall@k and "
                "latency measurements for lexical/semantic/hybrid modes.",
            ],
        }
    else:
        with tempfile.TemporaryDirectory(prefix="agent-session-grep-semantic-") as temp_name:
            scratch = Path(temp_name)
            corpus_root = scratch / "corpus"
            corpus_files, query_markers = write_corpus(
                corpus_root, profile["topics"], profile["distractors"]
            )
            db = scratch / "catalog.db"
            sync_frame = cli_frame(
                binary,
                workspace,
                [
                    str(binary),
                    "--db",
                    str(db),
                    "--output",
                    "json",
                    "sync",
                    *(str(path) for path in corpus_files),
                ],
            )
            sync_data = sync_frame["data"]
            index_frame = cli_frame(
                binary,
                workspace,
                [str(binary), "--db", str(db), "--output", "json", "index", "embeddings"],
            )
            index_data = index_frame["data"]
            index_warnings = index_frame.get("warnings", [])
            mode_results = measure_modes(
                binary,
                workspace,
                db,
                query_markers,
                args.k,
                profile["repeat"],
            )
            report = {
                **common,
                "status": "locally_verified",
                "dataset": {
                    "kind": "deterministic_synthetic_semantic_paraphrase_corpus",
                    "contains_real_transcripts": False,
                    "hash_algorithm": "sha256",
                    "dataset_hash": tree_hash(corpus_files, corpus_root),
                    "file_count": len(corpus_files),
                    "message_count": profile["topics"] * 4 + profile["distractors"],
                    "topic_count": profile["topics"],
                    "relevant_messages_per_topic": 4,
                    "distractor_count": profile["distractors"],
                    "marker_convention": "each topic message text ends with ' [topic-NN]'; ground truth per query is the exact marker suffix",
                    "k": args.k,
                    "sync": sync_data,
                },
                "methodology": {
                    "clock": "time.perf_counter_ns",
                    "percentiles": "nearest-rank",
                    "dispersion": "sample standard deviation (n-1); 0 for one sample",
                    "repeats_per_query": profile["repeat"],
                    "ranking_determinism": "repeat hit id lists must agree or the run fails",
                    "recall_definition": "retrieved_relevant_in_top_k / relevant_messages_for_query; relevant is 4 per topic",
                    "latency_scope": "wall time of one CLI process performing one search against a warm store; no OS cache flushing",
                },
                "index": {
                    "backend": index_data.get("backend"),
                    "model_id": index_data.get("model_id"),
                    "dimension": index_data.get("dimension"),
                    "indexed": index_data.get("indexed"),
                    "skipped": index_data.get("skipped"),
                    "cleared": index_data.get("cleared"),
                    "warnings": index_warnings,
                },
                "results": mode_results,
                "limitations": [
                    "These local measurements are evidence anchors, not formal SLOs or release certification.",
                    "The corpus is synthetic paraphrase pairs plus distractors; paraphrase recall measures the model's ability to bridge no-shared-token reformulations, not production retrieval quality.",
                    "Semantic measurements are only recorded when the vector index was built by the semantic-candle backend; any lexical_fallback degradation is recorded per query.",
                    "Latency measures whole-process wall time (startup + search), matching how the CLI is invoked in practice.",
                ],
            }

    report_path = output_dir / f"semantic-benchmark-{args.profile}.json"
    report_path.write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    validate_report(report_path, expected_profile=args.profile)
    return (EXIT_MODEL_NOT_IMPORTED if report["status"] == "model_not_imported" else EXIT_OK), report_path


def validate_report(path: Path, expected_profile: str | None = None) -> dict[str, Any]:
    report = json.loads(path.read_text(encoding="utf-8"))
    if report.get("schema_version") != SCHEMA_VERSION:
        raise ValueError(f"unsupported schema_version: {report.get('schema_version')!r}")
    status = report.get("status")
    if status not in {"locally_verified", "model_not_imported"}:
        raise ValueError(f"unrecognized status: {status!r}")
    profile_name = report.get("profile")
    if profile_name not in PROFILES or (expected_profile and profile_name != expected_profile):
        raise ValueError(f"unexpected profile: {profile_name!r}")
    commit = report.get("commit", "")
    if not isinstance(commit, str) or len(commit) != 40 or any(
        char not in "0123456789abcdef" for char in commit
    ):
        raise ValueError("commit must be a full lowercase Git SHA")
    binary = report.get("binary", {})
    binary_hash = binary.get("binary_hash", "")
    if not isinstance(binary_hash, str) or len(binary_hash) != 64 or any(
        char not in "0123456789abcdef" for char in binary_hash
    ):
        raise ValueError("binary.binary_hash must be a lowercase SHA-256 digest")
    if binary.get("provenance") not in {
        "built_by_harness_from_workspace",
        "caller_supplied_prebuilt",
    }:
        raise ValueError("binary.provenance is not recognized")
    model = report.get("model", {})
    if "feature" not in model or "present" not in model or "verified" not in model:
        raise ValueError("model status fields are missing")
    limitations = report.get("limitations")
    if not isinstance(limitations, list) or not all(
        isinstance(item, str) for item in limitations
    ):
        raise ValueError("limitations must be a list of strings")
    if status == "model_not_imported":
        if model.get("present") is True or model.get("verified") is True:
            raise ValueError(
                "model_not_imported report claims present/verified model status"
            )
        if report.get("results") is not None:
            raise ValueError("model_not_imported report must not carry results")
    else:
        if model.get("present") is not True or model.get("verified") is not True:
            raise ValueError("locally_verified report requires a verified model")
        dataset = report.get("dataset", {})
        if dataset.get("contains_real_transcripts") is not False:
            raise ValueError("dataset.contains_real_transcripts must be false")
        dataset_hash = dataset.get("dataset_hash", "")
        if not isinstance(dataset_hash, str) or len(dataset_hash) != 64 or any(
            char not in "0123456789abcdef" for char in dataset_hash
        ):
            raise ValueError("dataset.dataset_hash must be a lowercase SHA-256 digest")
        k = dataset.get("k")
        if not isinstance(k, int) or k < 1:
            raise ValueError("dataset.k must be a positive integer")
        index = report.get("index", {})
        if not index.get("backend") or not index.get("model_id"):
            raise ValueError("index.backend/model_id must be recorded")
        if index.get("backend") != "semantic-candle":
            raise ValueError(
                "locally_verified semantic evidence requires index backend semantic-candle"
            )
        results = report.get("results")
        if not isinstance(results, dict) or set(results) != set(MODES):
            raise ValueError("results must contain exactly lexical/semantic/hybrid")
        profile = PROFILES[profile_name]
        expected_queries = profile["topics"]
        for mode in MODES:
            entry = results[mode]
            if entry.get("requested_mode") != mode:
                raise ValueError(f"results.{mode}.requested_mode mismatch")
            rows = entry.get("queries")
            if not isinstance(rows, list) or len(rows) != expected_queries:
                raise ValueError(
                    f"results.{mode}.queries must have {expected_queries} entries"
                )
            recalls: list[float] = []
            for row in rows:
                recall = row.get("recall_at_k")
                if not isinstance(recall, (int, float)) or not 0.0 <= recall <= 1.0:
                    raise ValueError(f"results.{mode}: recall_at_k out of range: {recall!r}")
                if row.get("relevant") != 4:
                    raise ValueError(f"results.{mode}: relevant must be 4 per topic")
                retrieved = row.get("retrieved_relevant")
                if not isinstance(retrieved, int) or retrieved != round(recall * 4):
                    raise ValueError(
                        f"results.{mode}: retrieved_relevant inconsistent with recall_at_k"
                    )
                recalls.append(float(recall))
            mean = entry.get("mean_recall_at_k")
            if mean != rounded(sum(recalls) / len(recalls)):
                raise ValueError(f"results.{mode}.mean_recall_at_k does not match query recalls")
            latency = entry.get("latency_ms", {})
            samples = latency.get("raw_samples")
            required = expected_queries * profile["repeat"]
            if (
                not isinstance(samples, list)
                or len(samples) != required
                or not all(isinstance(value, (int, float)) and value >= 0 for value in samples)
            ):
                raise ValueError(
                    f"results.{mode}.latency_ms must have {required} non-negative samples"
                )
            assert_summary(
                [float(value) for value in samples],
                latency.get("summary", {}),
                f"results.{mode}.latency_ms",
            )
            observed = entry.get("effective_modes_observed", [])
            if not isinstance(observed, list):
                raise ValueError(f"results.{mode}.effective_modes_observed must be a list")
            degraded = entry.get("degraded_query_count", 0)
            if degraded != sum(
                1
                for row in rows
                if mode != "lexical" and row.get("effective_mode") == "lexical_fallback"
            ):
                raise ValueError(
                    f"results.{mode}.degraded_query_count does not match query rows"
                )
            if mode == "semantic" and degraded == expected_queries:
                raise ValueError(
                    "results.semantic: every query degraded to lexical_fallback; "
                    "this is not semantic evidence"
                )
    print(f"valid {SCHEMA_VERSION} report: {path}")
    return report


def parser() -> argparse.ArgumentParser:
    root = Path(__file__).resolve().parents[2]
    result = argparse.ArgumentParser(description=__doc__)
    sub = result.add_subparsers(dest="command", required=True)
    run = sub.add_parser(
        "run",
        help="generate the corpus, gate on model status, measure, and emit JSON",
    )
    run.add_argument("--profile", choices=sorted(PROFILES), default="smoke")
    run.add_argument("--workspace", default=str(root))
    run.add_argument(
        "--output-dir",
        default=str(root / "scripts" / "evidence" / "out"),
        help="manifest output directory (JSON only; gitignored)",
    )
    run.add_argument(
        "--binary",
        help="explicit prebuilt semantic-candle CLI; otherwise cargo build --release "
        "--features semantic-candle is run",
    )
    run.add_argument("--cargo", default="cargo")
    run.add_argument("--rustc", default="rustc")
    run.add_argument("--expected-commit", help="full Git SHA required for full evidence")
    run.add_argument("--k", type=int, default=10, help="recall cut-off (page size)")
    validate = sub.add_parser("validate-report", help="validate a manifest JSON")
    validate.add_argument("report")
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "run":
            exit_code, report_path = run_benchmark(args)
            print(report_path)
            if exit_code == EXIT_MODEL_NOT_IMPORTED:
                print("model_not_imported", file=sys.stderr)
            return exit_code
        validate_report(Path(args.report).expanduser().resolve())
        return EXIT_OK
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return EXIT_ERROR


if __name__ == "__main__":
    raise SystemExit(main())
