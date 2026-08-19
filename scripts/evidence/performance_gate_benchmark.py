#!/usr/bin/env python3
"""Performance release gate for agent-session-grep.

The correctness gate lives in ``open_source_gate_benchmark.py`` and thresholds
four *correctness* quantities (lexical recall, parse loss, discovery coverage,
resume/handoff success) against a committed 8-file labeled fixture. It carries
no performance threshold at all. This harness adds the four *performance*
thresholds the owner stated, measured on the 100,000-message synthetic corpus
that ``synthetic_corpus.py`` generates:

===============================  =========  ====================================
action                           threshold  where the number comes from
===============================  =========  ====================================
initial full index               >= 3.5     owner's stated first-run ceiling
                                 MiB/s      (1 GiB in <= 5 min)
incremental sync, no changes      <= 1 s    owner's stated interactive ceiling
search (p95)                     <= 50 ms   interactive "instant" feel
MCP single call (p95)            <= 50 ms   guards an already-measured win
===============================  =========  ====================================

Why a sibling script rather than an extension of the correctness gate:

* ``open_source_gate_benchmark.validate_manifest`` asserts the manifest's metric
  names equal ``GATE_THRESHOLDS | INFORMATIONAL_METRICS`` exactly. Folding four
  performance metrics into that set would invalidate every correctness manifest
  already emitted and force a schema bump of the correctness gate for reasons
  that have nothing to do with correctness.
* The two gates read different corpora. The correctness gate's corpus is
  committed, 8 files, and hashes in milliseconds. This one needs a generated
  ~49 MiB / 5,000-file corpus that is deliberately never committed.
* Their runtimes differ by two orders of magnitude, so they belong on different
  CI triggers (see the ``ci_viability`` block in the emitted manifest).

Nothing is duplicated to achieve the split: the gate machinery
(``metric_entry``, ``resolve_binary``, ``latency_p50_p95``), the measurement
primitives (``cli``, ``rounded_summary``, ``nearest_rank``, ``sha256_file``),
the corpus contract (``load_frozen_manifest``, ``chunk_paths``) and the MCP
transport (``measure_tool_calls``) are all imported from their existing homes.

Uses only the Python standard library.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Sequence

sys.path.insert(0, str(Path(__file__).resolve().parent))
from core_beta_benchmark import (  # noqa: E402
    cli,
    command_text,
    nearest_rank,
    rounded,
    rounded_summary,
    sha256_file,
    total_memory_bytes,
)
from open_source_gate_benchmark import (  # noqa: E402
    latency_p50_p95,
    metric_entry,
    resolve_binary,
    utc_now,
)
from semantic_mcp_latency import FALLBACK_BACKEND, measure_tool_calls  # noqa: E402
from synthetic_corpus import (  # noqa: E402
    DEFAULT_BODY_SCALE,
    DEFAULT_MESSAGES,
    DEFAULT_OUTPUT_DIR,
    DEFAULT_SESSIONS,
    chunk_paths,
    corpus_files,
    load_frozen_manifest,
)

PERFORMANCE_SCHEMA_VERSION = "agent-session-grep.performance-gate/v1"

# The four thresholds. Values are the owner's stated ceilings, not measured
# results: a threshold tuned to fit a measurement gates nothing.
PERFORMANCE_THRESHOLDS: dict[str, float] = {
    "initial_index_throughput_mib_s": 3.5,
    "noop_sync_latency_ms": 1000.0,
    "search_latency_p95_ms": 50.0,
    "mcp_single_call_latency_p95_ms": 50.0,
}

# Comparison direction per threshold. "min" means value >= threshold passes.
THRESHOLD_DIRECTION: dict[str, str] = {
    "initial_index_throughput_mib_s": "min",
    "noop_sync_latency_ms": "max",
    "search_latency_p95_ms": "max",
    "mcp_single_call_latency_p95_ms": "max",
}

# Where each number came from, recorded in the manifest so a reader never has to
# trust that a threshold was not reverse-engineered from a passing run.
THRESHOLD_ORIGIN: dict[str, str] = {
    "initial_index_throughput_mib_s": (
        "owner's stated first-run ceiling: 1 GiB must index in <= 5 minutes, "
        "i.e. >= 3.5 MiB/s. Stated as 3.5 MB/s; applied as 3.5 MiB/s, the "
        "stricter of the two readings."
    ),
    "noop_sync_latency_ms": (
        "owner's stated interactive ceiling for a re-sync that finds nothing to "
        "do. Expected to fail at first measurement: the previously recorded "
        "figure was 1.5 s at only 4,000 messages."
    ),
    "search_latency_p95_ms": (
        "interactive 'instant' feel for the primary user action; p95 rather than "
        "p50 so the tail is what gates."
    ),
    "mcp_single_call_latency_p95_ms": (
        "guards an already-measured win rather than demanding one: a resident "
        "MCP process previously measured 21 ms p95 (n=10, 10-message corpus, "
        "semantic-candle build)."
    ),
}

# Measured and published, but deliberately un-thresholded. Each entry must carry
# a reason; the validator rejects an informational metric that carries a
# threshold or a pass verdict, so none of these can gate a release by accident.
PERFORMANCE_INFORMATIONAL_METRICS: dict[str, str] = {
    "mcp_semantic_call_latency_p95_ms": (
        "the thresholded MCP metric is the default lexical call. Semantic mode is "
        "reported for comparison only: unless the binary is built with "
        "--features semantic-candle the vector backend is a bigram hash computed "
        "in-process, so there is no encoder to keep resident and the number does "
        "not describe the condition the 50 ms figure was stated under. See "
        "vectorizer.is_real_embedding_model."
    ),
    "embeddings_index_build_ms": (
        "prerequisite for semantic/hybrid retrieval, not a user-facing "
        "interactive action; the owner has stated no ceiling for it."
    ),
    "cli_process_overhead_p50_ms": (
        "diagnostic decomposition, not a user action: one-shot CLI latency is "
        "process launch plus query work, and this separates the two so a reader "
        "can see how much of search_latency_p95_ms is not query time."
    ),
    "initial_index_messages_per_s": (
        "the same initial-index pass as initial_index_throughput_mib_s, divided by "
        "messages instead of bytes. Un-thresholded because the owner stated the "
        "requirement in bytes, but recorded because it is the density-invariant "
        "figure: commit cost was measured to be almost entirely per message, so "
        "MiB/s moves with corpus density while this does not. Comparing two "
        "corpora of different density is only meaningful through this number."
    ),
    "store_size_ratio": (
        "store bytes divided by corpus source bytes; recorded for comparability "
        "because the owner has stated no storage-amplification ceiling."
    ),
}

# Lexical queries for the search measurement. Every one is verified to return at
# least one hit against this corpus during the run; a query returning nothing
# would measure an empty-result fast path rather than retrieval.
SEARCH_QUERIES: tuple[str, ...] = (
    "sidechain",
    "cursor",
    "FTS5",
    "unicode61",
    "panicked",
    "Traceback",
    "clippy",
    "跨度",
    "回归",
    "指纹",
)
# Recorded so nobody later assumes the set above was cherry-picked to look fast.
# `safetensors` was probed and dropped for returning zero hits on this corpus:
# the word appears nowhere in synthetic_corpus.py's content banks.
DROPPED_QUERIES: tuple[dict[str, str], ...] = (
    {
        "query": "safetensors",
        "reason": "returned 0 hits on this corpus; a zero-hit query measures the "
        "empty-result path, not retrieval",
    },
)

# States a threshold metric may carry.
STATE_MEASURED = "measured"
STATE_BELOW_SCALE = "below_threshold_scale"
# The corpus holds the right message count but is not the frozen corpus the
# thresholds are stated on — currently only reachable by generating a deliberate
# density variant (`synthetic_corpus.py generate --body-scale`). The values are
# measured and recorded; a verdict is not, because the threshold was never stated
# against this corpus.
STATE_OFF_CONTRACT = "off_frozen_corpus_contract"
# Every state in which a thresholded metric records a value but no verdict.
NO_VERDICT_STATES = frozenset({STATE_BELOW_SCALE, STATE_OFF_CONTRACT})
# Informational metrics only: the run deliberately did not measure this. The
# value stays null rather than being estimated.
STATE_NOT_MEASURED = "not_measured"

PROFILES: dict[str, dict[str, int]] = {
    # The corpus the thresholds are stated on. Produces a gate verdict.
    "full": {"sessions": DEFAULT_SESSIONS, "messages": DEFAULT_MESSAGES},
    # Wiring check only. Every threshold metric is emitted below scale with a
    # null verdict, so a smoke run can never manufacture a pass.
    "smoke": {"sessions": 300, "messages": 6_000},
}


def evaluate(name: str, value: float) -> bool:
    """Pass/fail for one thresholded metric. Pure; the unit tests pin both directions."""
    threshold = PERFORMANCE_THRESHOLDS[name]
    direction = THRESHOLD_DIRECTION[name]
    if direction == "min":
        return value >= threshold
    if direction == "max":
        return value <= threshold
    raise ValueError(f"{name}: unknown threshold direction {direction!r}")


def throughput_mib_s(source_bytes: int, elapsed_ms: float) -> float:
    """MiB/s over ``source_bytes`` ingested in ``elapsed_ms``."""
    if elapsed_ms <= 0:
        raise ValueError("elapsed_ms must be positive to derive a throughput")
    return rounded(source_bytes / (1024 * 1024) / (elapsed_ms / 1000.0))


def throughput_messages_s(messages: int, elapsed_ms: float) -> float:
    """Messages/s over ``messages`` ingested in ``elapsed_ms``.

    The density-invariant companion to :func:`throughput_mib_s`: the commit
    transaction was measured to price nearly every step per message, so this is
    the figure that is comparable between corpora of different densities.
    """
    if elapsed_ms <= 0:
        raise ValueError("elapsed_ms must be positive to derive a throughput")
    return rounded(messages / (elapsed_ms / 1000.0))


def requirement_projection(
    bytes_per_message: float, messages_per_s: float, minutes_allowed: float = 5.0
) -> dict[str, Any]:
    """Project the owner's "1 GiB in <= 5 minutes" onto one measured rate.

    The threshold is stated in MiB/s only as a proxy for this: how long a 1 GiB
    corpus takes. A 1 GiB corpus is a *count* of messages that depends entirely
    on the corpus density, so the projection has to carry the density it assumed.
    This is a projection, never a gate verdict -- ``gate.pass`` ignores it.
    """
    if bytes_per_message <= 0:
        raise ValueError("bytes_per_message must be positive")
    if messages_per_s <= 0:
        raise ValueError("messages_per_s must be positive")
    messages_in_a_gib = 1024**3 / bytes_per_message
    minutes = messages_in_a_gib / messages_per_s / 60.0
    return {
        "requirement": (
            f"1 GiB of source transcripts indexed in <= {minutes_allowed} minutes, the "
            f"origin of the {PERFORMANCE_THRESHOLDS['initial_index_throughput_mib_s']} "
            f"MiB/s threshold"
        ),
        "assumed_bytes_per_message": rounded(bytes_per_message),
        "messages_in_one_gib_at_this_density": rounded(messages_in_a_gib),
        "measured_messages_per_s": rounded(messages_per_s),
        "projected_minutes_for_one_gib": rounded(minutes),
        "minutes_allowed": minutes_allowed,
        "projection_meets_the_requirement": minutes <= minutes_allowed,
        "note": (
            "a projection from this run's measured messages/s onto this corpus's "
            "density, not a measured 1 GiB ingest and not a gate verdict"
        ),
    }


def corpus_descriptor(manifest: dict[str, Any], at_threshold_scale: bool) -> dict[str, Any]:
    """Full corpus identity block for the manifest header."""
    corpus = manifest["corpus"]
    message_count = corpus["message_count"]
    total_bytes = corpus["total_bytes"]
    return {
        "id": f"synthetic-corpus-{message_count}",
        "generator": "scripts/evidence/synthetic_corpus.py",
        "kind": "deterministic_synthetic_multi_provider",
        "contains_real_transcripts": False,
        "message_count": message_count,
        "session_count": corpus["session_count"],
        "file_count": corpus["file_count"],
        "source_bytes": total_bytes,
        # Density, recorded because MiB/s is not comparable across densities:
        # the same per-message cost yields a different MiB/s on a denser corpus.
        "body_scale": corpus.get("body_scale", 1.0),
        "bytes_per_message": corpus.get(
            "bytes_per_message",
            round(total_bytes / message_count, 3) if message_count else 0.0,
        ),
        "providers": sorted(corpus["providers"]),
        "fixture_hash": corpus["fixture_hash"],
        "hash_algorithm": corpus.get("hash_algorithm", "sha256"),
        "hash_normalization": corpus.get("hash_normalization", "crlf_to_lf"),
        "at_threshold_scale": at_threshold_scale,
    }


def corpus_ref(descriptor: dict[str, Any]) -> dict[str, Any]:
    """Compact corpus reference embedded in every metric entry.

    A latency figure without its corpus and n is misleading, and this repository
    already had to retrofit exactly that onto its semantic latency numbers. Each
    metric therefore names its corpus inline rather than relying on the reader to
    scroll to the header. ``bytes_per_message`` is part of that identity: a
    throughput in MiB/s read without the corpus density is not comparable to any
    other throughput figure.
    """
    return {
        "id": descriptor["id"],
        "message_count": descriptor["message_count"],
        "session_count": descriptor["session_count"],
        "source_bytes": descriptor["source_bytes"],
        "bytes_per_message": descriptor["bytes_per_message"],
        "body_scale": descriptor["body_scale"],
        "at_threshold_scale": descriptor["at_threshold_scale"],
    }


def performance_metric(
    name: str,
    unit: str,
    value: Any,
    *,
    action: str,
    corpus: dict[str, Any],
    sample_count: int,
    sample_unit: str,
    state: str = STATE_MEASURED,
    reason: str | None = None,
    detail: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Build one metric entry, thresholded or informational.

    Reuses ``open_source_gate_benchmark.metric_entry`` so both manifests share
    the same key shape, then adds the corpus reference and sample count this
    gate requires inline on every entry.
    """
    informational = name in PERFORMANCE_INFORMATIONAL_METRICS
    if informational:
        threshold: float | None = None
        pass_flag: bool | None = None
        reason = reason or PERFORMANCE_INFORMATIONAL_METRICS[name]
    elif name in PERFORMANCE_THRESHOLDS:
        if state == STATE_NOT_MEASURED:
            raise ValueError(
                f"{name}: a thresholded metric may not be left unmeasured; that would "
                f"silently drop it from the gate"
            )
        threshold = PERFORMANCE_THRESHOLDS[name]
        if state == STATE_MEASURED:
            pass_flag = evaluate(name, float(value))
        else:
            # Off the corpus the threshold is stated on — smaller, or a different
            # density — a verdict would be a fabrication. The value is still recorded.
            pass_flag = None
    else:
        raise ValueError(f"{name}: not a known performance metric")
    entry = metric_entry(name, unit, value, threshold, pass_flag, state=state, reason=reason)
    entry["action"] = action
    entry["corpus"] = corpus
    entry["sample_count"] = sample_count
    entry["sample_unit"] = sample_unit
    if not informational:
        entry["threshold_direction"] = THRESHOLD_DIRECTION[name]
        entry["threshold_origin"] = THRESHOLD_ORIGIN[name]
    if detail:
        entry["detail"] = detail
    return entry


def environment_block(commit: str) -> dict[str, Any]:
    memory = total_memory_bytes()
    return {
        "os": platform.system().lower(),
        "os_release": platform.release(),
        "arch": platform.machine() or "not_recorded",
        "logical_cpu_count": os.cpu_count(),
        "ram_gb": rounded(memory / (1024**3)) if memory else "not_recorded",
        "build": "release",
        "commit": commit,
        "python_version": platform.python_version(),
    }


def sum_durations(samples: Sequence[dict[str, Any]]) -> float:
    return rounded(sum(float(sample["duration_ms"]) for sample in samples))


def sync_pass(
    binary: Path, workspace: Path, db: Path, batches: Sequence[Sequence[str]]
) -> dict[str, Any]:
    """One full-corpus sync pass across every command-line batch.

    5,000 source paths do not fit in one Windows command line, so a full pass is
    N processes. The user-visible cost is the whole pass, so that is what the
    metric reports; per-batch statistics and the batch count are recorded
    alongside it so process-launch overhead stays visible rather than hidden.
    """
    samples: list[dict[str, Any]] = []
    emitted = skipped = unchanged = unrecognized = 0
    for batch in batches:
        result = cli(binary, workspace, db, "sync", *batch)
        samples.append(result)
        data = result["frame"]["data"]
        emitted += int(data.get("emitted", 0))
        skipped += int(data.get("skipped", 0))
        unchanged += int(data.get("unchanged", 0))
        # `unrecognized` is newer than the other counters; absent on older builds.
        unrecognized += int(data.get("unrecognized", 0) or 0)
    return {
        "total_ms": sum_durations(samples),
        "batches": len(batches),
        "per_batch_ms": latency_p50_p95(samples),
        "emitted": emitted,
        "skipped": skipped,
        "unchanged": unchanged,
        "unrecognized": unrecognized,
    }


def model_status(binary: Path, workspace: Path, db: Path) -> dict[str, Any]:
    """Read the binary's embedding-model capability without building an index.

    Cheap enough to run unconditionally, and it answers the only question the
    MCP thresholds' "encoder resident" wording depends on: was this binary even
    built with a real embedding model available?
    """
    data = cli(binary, workspace, db, "model", "status")["frame"]["data"]
    return {
        "feature": data.get("feature"),
        "present": bool(data.get("present")),
        "verified": bool(data.get("verified")),
    }


def vectorizer_from_status(status: dict[str, Any]) -> dict[str, Any]:
    """Capability-only vectorizer block, used when the index was not built.

    Without `index embeddings` the backend that *would* serve a semantic query
    is inferred from the model capability rather than observed. That is recorded
    as such: state "inferred_from_model_status", never presented as measured.
    """
    real = bool(status["feature"]) and status["present"] and status["verified"]
    return {
        "state": "inferred_from_model_status",
        "backend": "not_measured" if real else FALLBACK_BACKEND,
        "model_id": "not_measured",
        "dimension": None,
        "indexed": None,
        "skipped": None,
        "is_real_embedding_model": real,
        "model_status": status,
    }


def run_performance_gate(args: argparse.Namespace) -> Path:
    started_wall = time.perf_counter()
    phases: dict[str, float] = {}
    workspace = Path(args.workspace).expanduser().resolve()
    output_dir = Path(args.output_dir).expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    corpus_dir_root = (
        Path(args.corpus_dir).expanduser().resolve()
        if args.corpus_dir
        else workspace / DEFAULT_OUTPUT_DIR
    )
    profile = PROFILES[args.profile]
    commit = command_text(["git", "rev-parse", "HEAD"], workspace) or "not_recorded"
    binary = resolve_binary(workspace, args)
    binary_provenance = (
        "caller_supplied_prebuilt" if args.binary else "built_by_harness_from_workspace"
    )

    corpus_manifest_path = corpus_dir_root / "manifest.json"
    if not corpus_manifest_path.is_file():
        hint = "  python scripts/evidence/synthetic_corpus.py generate"
        if args.profile != "full":
            hint += f" --sessions {profile['sessions']} --messages {profile['messages']}"
        raise RuntimeError(
            f"no generated corpus manifest at {corpus_manifest_path}. Generate it first:\n{hint}"
        )
    corpus_manifest = json.loads(corpus_manifest_path.read_text(encoding="utf-8"))
    actual_messages = corpus_manifest["corpus"]["message_count"]
    if actual_messages != profile["messages"]:
        raise RuntimeError(
            f"corpus at {corpus_manifest_path} holds {actual_messages} messages but "
            f"profile {args.profile!r} expects {profile['messages']}; regenerate the "
            f"corpus or pick the matching profile"
        )
    # A density variant carries the right message count but different bytes, so it
    # is not the corpus the thresholds were stated against. Detecting it from the
    # corpus manifest rather than from a harness flag is deliberate: a variant can
    # only be produced by explicitly passing --body-scale, and the harness then
    # cannot be talked into scoring a verdict on it.
    body_scale = corpus_manifest["corpus"].get("body_scale", DEFAULT_BODY_SCALE)
    on_frozen_contract = args.profile == "full" and body_scale == DEFAULT_BODY_SCALE
    at_threshold_scale = on_frozen_contract
    if on_frozen_contract:
        # A gate verdict is only meaningful against the frozen contract.
        frozen = load_frozen_manifest()
        if corpus_manifest["corpus"]["fixture_hash"] != frozen["corpus"]["fixture_hash"]:
            raise RuntimeError(
                "generated corpus hash does not match the frozen manifest; "
                "run `synthetic_corpus.py verify` before gating"
            )
    descriptor = corpus_descriptor(corpus_manifest, at_threshold_scale)
    reference = corpus_ref(descriptor)
    sources = corpus_files(corpus_dir_root / "corpus")
    if not sources:
        raise RuntimeError(f"no corpus files under {corpus_dir_root / 'corpus'}")
    batches = chunk_paths(sources)

    with tempfile.TemporaryDirectory(prefix="asg-performance-gate-") as scratch_name:
        scratch = Path(scratch_name)
        db = scratch / "performance.db"

        # ---- 1. initial full index -------------------------------------------
        mark = time.perf_counter()
        initial = sync_pass(binary, workspace, db, batches)
        phases["initial_sync_s"] = rounded(time.perf_counter() - mark)
        if initial["emitted"] != actual_messages:
            raise RuntimeError(
                f"initial sync emitted {initial['emitted']} of {actual_messages} messages; "
                f"a throughput figure over a partial ingest would be meaningless"
            )
        throughput = throughput_mib_s(descriptor["source_bytes"], initial["total_ms"])
        messages_per_s = throughput_messages_s(initial["emitted"], initial["total_ms"])

        # ---- 2. incremental sync, nothing changed ----------------------------
        mark = time.perf_counter()
        noop_passes = [
            sync_pass(binary, workspace, db, batches) for _ in range(max(1, args.noop_reps))
        ]
        phases["noop_sync_s"] = rounded(time.perf_counter() - mark)
        noop_totals = [pass_["total_ms"] for pass_ in noop_passes]
        noop_summary = rounded_summary(noop_totals)
        noop_value = noop_summary["p50"]
        for pass_ in noop_passes:
            if pass_["emitted"]:
                raise RuntimeError(
                    f"a no-op re-sync emitted {pass_['emitted']} messages; the source "
                    f"did not change, so this is not measuring the no-op path"
                )

        # ---- 3. search -------------------------------------------------------
        mark = time.perf_counter()
        search_samples: list[dict[str, Any]] = []
        hits_by_query: dict[str, int] = {}
        rounds = max(1, args.search_reps // len(SEARCH_QUERIES))
        for _ in range(rounds):
            for query in SEARCH_QUERIES:
                result = cli(binary, workspace, db, "search", query, "--max-items", "10")
                search_samples.append(result)
                hits = result["frame"]["data"].get("hits", [])
                hits_by_query[query] = len(hits)
        phases["search_s"] = rounded(time.perf_counter() - mark)
        empty = sorted(query for query, count in hits_by_query.items() if count == 0)
        if empty:
            raise RuntimeError(
                f"queries returned no hits on this corpus: {empty}. A zero-hit query "
                f"measures the empty-result path; drop it from SEARCH_QUERIES and "
                f"record it in DROPPED_QUERIES instead of reporting it as retrieval."
            )
        search_stats = latency_p50_p95(search_samples)

        # Process-launch overhead, so a reader can see how much of the one-shot
        # search figure above is not query work.
        overhead_samples = [
            cli(binary, workspace, db, "status") for _ in range(max(1, args.overhead_reps))
        ]
        overhead_stats = latency_p50_p95(overhead_samples)

        # ---- 4. MCP single call ----------------------------------------------
        mark = time.perf_counter()
        lexical_calls = [
            {"query": SEARCH_QUERIES[index % len(SEARCH_QUERIES)], "mode": "lexical", "limit": 10}
            for index in range(max(1, args.mcp_reps))
        ]
        mcp_lexical = measure_tool_calls(binary, db, lexical_calls)
        phases["mcp_lexical_s"] = rounded(time.perf_counter() - mark)
        mcp_lexical_p95 = rounded(nearest_rank(mcp_lexical, 95))
        mcp_lexical_p50 = rounded(nearest_rank(mcp_lexical, 50))

        # ---- informational: embeddings + semantic MCP ------------------------
        # The embeddings build dominates this harness's wall clock (measured:
        # 285 s of a 372 s full run at 100k) and serves only the two
        # informational metrics below. --skip-embeddings drops it so the four
        # thresholded metrics can be gated on their own; the informational
        # entries then record that they were not measured, never a guessed value.
        if args.skip_embeddings:
            model = model_status(binary, workspace, db)
            vectorizer = vectorizer_from_status(model)
            embeddings = None
            mcp_semantic: list[float] = []
            mcp_semantic_p95 = None
            mcp_semantic_p50 = None
        else:
            mark = time.perf_counter()
            embeddings = cli(binary, workspace, db, "index", "embeddings")
            phases["embeddings_index_s"] = rounded(time.perf_counter() - mark)
            embeddings_data = embeddings["frame"]["data"]
            backend = embeddings_data.get("backend", "not_recorded")
            vectorizer = {
                "state": "measured",
                "backend": backend,
                "model_id": embeddings_data.get("model_id", "not_recorded"),
                "dimension": embeddings_data.get("dimension"),
                "indexed": embeddings_data.get("indexed"),
                "skipped": embeddings_data.get("skipped"),
                "is_real_embedding_model": backend != FALLBACK_BACKEND,
            }
            mark = time.perf_counter()
            semantic_calls = [
                {
                    "query": SEARCH_QUERIES[index % len(SEARCH_QUERIES)],
                    "mode": "semantic",
                    "limit": 10,
                }
                for index in range(max(1, args.mcp_reps))
            ]
            mcp_semantic = measure_tool_calls(binary, db, semantic_calls)
            phases["mcp_semantic_s"] = rounded(time.perf_counter() - mark)
            mcp_semantic_p95 = rounded(nearest_rank(mcp_semantic, 95))
            mcp_semantic_p50 = rounded(nearest_rank(mcp_semantic, 50))

        store_bytes = sum(
            path.stat().st_size for path in scratch.glob("performance.db*") if path.is_file()
        )

    store_ratio = (
        rounded(store_bytes / descriptor["source_bytes"]) if descriptor["source_bytes"] else None
    )
    if on_frozen_contract:
        state = STATE_MEASURED
        no_verdict_reason = None
    elif args.profile != "full":
        state = STATE_BELOW_SCALE
        no_verdict_reason = (
            f"profile {args.profile!r} runs {actual_messages} messages; every threshold "
            f"in this gate is stated at {DEFAULT_MESSAGES}. A verdict at a smaller scale "
            f"would manufacture a pass, so the value is recorded without one."
        )
    else:
        state = STATE_OFF_CONTRACT
        frozen_bpm = load_frozen_manifest()["corpus"]["bytes_per_message"]
        no_verdict_reason = (
            f"the corpus holds the stated {actual_messages} messages but was generated "
            f"with --body-scale {body_scale} "
            f"({descriptor['bytes_per_message']} bytes/message against the frozen "
            f"corpus's {frozen_bpm}). The thresholds were stated against the frozen "
            f"corpus, so this run measures a density variant and records values "
            f"without a verdict. initial_index_messages_per_s is the density-invariant "
            f"figure to compare across the two."
        )

    projection = requirement_projection(descriptor["bytes_per_message"], messages_per_s)

    metrics: list[dict[str, Any]] = [
        performance_metric(
            "initial_index_throughput_mib_s",
            "MiB/s",
            throughput,
            action="initial full index of the whole corpus into an empty store",
            corpus=reference,
            sample_count=1,
            sample_unit="full-corpus pass",
            state=state,
            reason=no_verdict_reason,
            detail={
                "total_ms": initial["total_ms"],
                "source_bytes": descriptor["source_bytes"],
                "emitted": initial["emitted"],
                "skipped": initial["skipped"],
                "unrecognized": initial["unrecognized"],
                "command_line_batches": initial["batches"],
                "per_batch_ms": initial["per_batch_ms"],
                "messages_per_s": messages_per_s,
                "bytes_per_message": descriptor["bytes_per_message"],
                "requirement_projection": projection,
                "note": (
                    "one pass is N processes because 5,000 source paths exceed the "
                    "OS command-line limit; the metric is the whole pass, which is "
                    "what a user waits for. MiB/s is a function of corpus density: "
                    "the same per-message cost reads as a different MiB/s on a corpus "
                    "with larger messages, so messages_per_s is carried alongside it"
                ),
            },
        ),
        performance_metric(
            "noop_sync_latency_ms",
            "ms",
            noop_value,
            action="re-sync the whole corpus with nothing changed on disk",
            corpus=reference,
            sample_count=len(noop_totals),
            sample_unit="full-corpus no-op pass",
            state=state,
            reason=no_verdict_reason,
            detail={
                "value_is": "p50 of the per-pass totals",
                "raw_pass_totals_ms": noop_totals,
                "summary_ms": noop_summary,
                "command_line_batches": noop_passes[0]["batches"],
                "per_batch_ms": noop_passes[0]["per_batch_ms"],
                "unchanged": noop_passes[0]["unchanged"],
                "emitted": noop_passes[0]["emitted"],
            },
        ),
        performance_metric(
            "search_latency_p95_ms",
            "ms",
            search_stats["p95"],
            action="one-shot CLI lexical search, default mode, --max-items 10",
            corpus=reference,
            sample_count=search_stats["count"],
            sample_unit="search invocation",
            state=state,
            reason=no_verdict_reason,
            detail={
                "p50_ms": search_stats["p50"],
                "queries": list(SEARCH_QUERIES),
                "rounds_per_query": rounds,
                "hits_per_query": dict(sorted(hits_by_query.items())),
                "dropped_queries": [dict(entry) for entry in DROPPED_QUERIES],
                "note": (
                    "end-to-end process wall clock, matching the existing "
                    "core_beta_benchmark search_latency_ms convention; includes "
                    "process launch, which cli_process_overhead_p50_ms isolates"
                ),
            },
        ),
        performance_metric(
            "mcp_single_call_latency_p95_ms",
            "ms",
            mcp_lexical_p95,
            action="one search_sessions tools/call on a resident MCP process, default lexical mode",
            corpus=reference,
            sample_count=len(mcp_lexical),
            sample_unit="tools/call",
            state=state,
            reason=no_verdict_reason,
            detail={
                "p50_ms": mcp_lexical_p50,
                "mode": "lexical",
                "process_model": "one mcp process for the whole sequence, handshake paid once",
                "harness": "scripts/evidence/semantic_mcp_latency.py::measure_tool_calls",
                # Stated on the metric itself, not only in limitations: the
                # threshold's wording says "encoder resident", and a reader
                # checking this one entry must be able to see what was actually
                # loaded in the process that served these calls.
                "encoder_resident": False,
                "binary_vector_backend": vectorizer["backend"],
                "binary_vector_model_id": vectorizer["model_id"],
                "binary_has_real_embedding_model": vectorizer["is_real_embedding_model"],
                "threshold_scope_note": (
                    "this metric times a default lexical tools/call, which loads no "
                    "encoder at all, against a store with no vector index yet. The 50 ms "
                    "threshold was stated for an encoder-resident process, so a pass here "
                    "means the resident-process call path is fast at this corpus size, not "
                    "that a semantic call with a real embedding model meets 50 ms. "
                    + (
                        f"The measured binary's vector backend is "
                        f"{vectorizer['backend']!r}, not a real embedding model, so the "
                        f"encoder-resident condition is not reproducible with it at all."
                        if not vectorizer["is_real_embedding_model"]
                        else "The measured binary does carry a real embedding model; see "
                        "mcp_semantic_call_latency_p95_ms for the encoder-resident figure."
                    )
                ),
            },
        ),
        performance_metric(
            "mcp_semantic_call_latency_p95_ms",
            "ms",
            mcp_semantic_p95,
            action="one search_sessions tools/call in semantic mode on a resident MCP process",
            corpus=reference,
            sample_count=len(mcp_semantic) or 1,
            sample_unit="tools/call",
            state=STATE_NOT_MEASURED if args.skip_embeddings else STATE_MEASURED,
            detail={
                "p50_ms": mcp_semantic_p50,
                "mode": "semantic",
                "encoder_resident": vectorizer["is_real_embedding_model"],
                "binary_vector_backend": vectorizer["backend"],
                "binary_vector_model_id": vectorizer["model_id"],
                "vectorizer": vectorizer,
                "harness": "scripts/evidence/semantic_mcp_latency.py::measure_tool_calls",
                "skipped_reason": (
                    "--skip-embeddings: the vector index was not built, so no semantic "
                    "call was issued"
                    if args.skip_embeddings
                    else None
                ),
            },
        ),
        performance_metric(
            "embeddings_index_build_ms",
            "ms",
            None if embeddings is None else embeddings["duration_ms"],
            action="index embeddings over the whole catalog",
            corpus=reference,
            sample_count=1,
            sample_unit="full-catalog build",
            state=STATE_NOT_MEASURED if embeddings is None else STATE_MEASURED,
            detail={
                "vectorizer": vectorizer,
                "skipped_reason": (
                    "--skip-embeddings: not built on this run" if embeddings is None else None
                ),
            },
        ),
        performance_metric(
            "cli_process_overhead_p50_ms",
            "ms",
            overhead_stats["p50"],
            action="one-shot CLI status invocation, a near-zero-work command",
            corpus=reference,
            sample_count=overhead_stats["count"],
            sample_unit="status invocation",
            detail={"p95_ms": overhead_stats["p95"]},
        ),
        performance_metric(
            "initial_index_messages_per_s",
            "messages/s",
            messages_per_s,
            action="the same initial full index pass, divided by messages instead of bytes",
            corpus=reference,
            sample_count=1,
            sample_unit="full-corpus pass",
            detail={
                "total_ms": initial["total_ms"],
                "emitted": initial["emitted"],
                "bytes_per_message": descriptor["bytes_per_message"],
                "body_scale": descriptor["body_scale"],
                "paired_mib_s": throughput,
                "requirement_projection": projection,
            },
        ),
        performance_metric(
            "store_size_ratio",
            "ratio",
            store_ratio,
            action="store bytes including sidecars divided by corpus source bytes",
            corpus=reference,
            sample_count=1,
            sample_unit="store measurement",
            detail={
                "store_bytes_including_sidecars": store_bytes,
                "source_bytes": descriptor["source_bytes"],
            },
        ),
    ]

    thresholded = [m for m in metrics if m["name"] in PERFORMANCE_THRESHOLDS]
    failures = sorted(m["name"] for m in thresholded if m["pass"] is False)
    deferred = sorted(m["name"] for m in thresholded if m["state"] in NO_VERDICT_STATES)
    gate_pass = None if deferred else not failures

    total_wall_s = rounded(time.perf_counter() - started_wall)
    manifest: dict[str, Any] = {
        "schema_version": PERFORMANCE_SCHEMA_VERSION,
        "profile": args.profile,
        "generated_at_utc": utc_now(),
        "commit": commit,
        "environment": environment_block(commit),
        "corpus": descriptor,
        "binary": {
            "hash_algorithm": "sha256",
            "binary_hash": sha256_file(binary),
            "name": binary.name,
            "provenance": binary_provenance,
        },
        "vectorizer": vectorizer,
        "methodology": {
            "clock": "time.perf_counter / time.perf_counter_ns",
            "percentiles": "nearest-rank",
            "search_entry_point": "one-shot CLI process, wall clock per invocation",
            "mcp_entry_point": "single resident mcp process, wall clock per tools/call",
            "sync_pass_definition": (
                "sum of every command-line batch in one full-corpus pass; the batch "
                "split exists because the OS caps command-line length"
            ),
            "raw_samples_authoritative": True,
        },
        "metrics": metrics,
        "runtime": {
            "total_wall_clock_s": total_wall_s,
            "phases_s": phases,
            "note": (
                "excludes corpus generation (~5 s) and the cargo release build; "
                "includes every measured phase and the informational ones"
            ),
        },
        "gate": {
            "pass": gate_pass,
            "failures": failures,
            "deferred": deferred,
        },
    }
    manifest["ci_viability"] = ci_viability(
        total_wall_s, phases, descriptor, args.skip_embeddings
    )
    manifest["limitations"] = limitations(
        vectorizer, binary_provenance, at_threshold_scale, no_verdict_reason
    )

    manifest_path = output_dir / f"performance-gate-manifest-{args.profile}.json"
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    validate_performance_manifest(manifest_path)
    print(manifest_path)
    return manifest_path


def ci_viability(
    total_wall_s: float,
    phases: dict[str, float],
    descriptor: dict[str, Any],
    skipped_embeddings: bool,
) -> dict[str, Any]:
    """State plainly whether the full gate fits CI, with the number.

    The proposal is derived from the measured phase breakdown, not guessed: the
    embeddings build dominates and feeds only informational metrics, so dropping
    it leaves every thresholded metric intact at full corpus scale.
    """
    fits = total_wall_s <= 300.0
    embeddings_s = phases.get("embeddings_index_s", 0.0) + phases.get("mcp_semantic_s", 0.0)
    gated_only_s = rounded(total_wall_s - embeddings_s)
    return {
        "full_gate_wall_clock_s": total_wall_s,
        "corpus_generation_s_excluded": "~5 s, run separately via synthetic_corpus.py generate",
        "embeddings_phase_s": rounded(embeddings_s),
        "thresholded_metrics_only_wall_clock_s": gated_only_s,
        "fits_a_5_minute_ci_budget": fits,
        "verdict": (
            f"The full gate takes {total_wall_s} s of wall clock"
            + (
                " and fits a per-PR CI budget."
                if fits
                else (
                    f" plus ~5 s of corpus generation and a "
                    f"~{descriptor['source_bytes'] // (1024 * 1024)} MiB corpus on disk, which "
                    f"is too slow for a per-PR job. {rounded(embeddings_s)} s of that is the "
                    f"vector-index build and semantic probe, which feed only informational "
                    f"metrics."
                )
            )
        ),
        "proposed_subset_for_per_pr_ci": {
            "run": (
                "performance_gate_benchmark.py run --profile full --skip-embeddings"
                if not skipped_embeddings
                else "performance_gate_benchmark.py run --profile full --skip-embeddings (this run)"
            ),
            "purpose": (
                "gates all four thresholds at full 100k corpus scale, dropping only the "
                "vector-index build and the semantic probe, which are informational. This "
                "is a real gate, not a smoke test: no threshold is weakened and no metric "
                "is dropped from the verdict."
            ),
            "measured_or_projected_wall_clock_s": gated_only_s,
            "basis": (
                "measured on this run"
                if skipped_embeddings
                else "this run's total minus its measured embeddings and semantic phases"
            ),
        },
        "alternative_smoke_subset": {
            "run": "performance_gate_benchmark.py run --profile smoke",
            "purpose": (
                "wiring and regression check only, at 6,000 messages. Emits every "
                "threshold metric with state below_threshold_scale and a null verdict, so "
                "it cannot be mistaken for a release gate."
            ),
            "measured_wall_clock_s": 14.0,
        },
        "proposed_full_gate_trigger": (
            "nightly and pre-release, on the release runner, with the 100k corpus "
            "generated in the job and discarded after"
        ),
        "slowest_phases_s": dict(sorted(phases.items(), key=lambda item: -item[1])[:3]),
    }


def limitations(
    vectorizer: dict[str, Any],
    provenance: str,
    at_threshold_scale: bool,
    no_verdict_reason: str | None = None,
) -> list[str]:
    items = [
        "These are local measurements on one machine, not multi-host certification.",
        "A full sync pass spans several processes because the OS caps command-line "
        "length; per-batch statistics are recorded so process-launch cost stays visible.",
        "One-shot CLI latency includes process launch. cli_process_overhead_p50_ms "
        "isolates that component; it is not subtracted from the gated figures.",
        "The store is measured after the commands exit, including any SQLite "
        "sidecars present; no explicit checkpoint command exists.",
    ]
    if not vectorizer["is_real_embedding_model"]:
        items.append(
            f"The measured binary's vector backend is {vectorizer['backend']!r}, not a "
            f"real embedding model, so no encoder was resident. "
            f"mcp_semantic_call_latency_p95_ms therefore does not describe the "
            f"encoder-resident condition the 50 ms MCP figure was originally stated "
            f"under; build with --features semantic-candle to measure that."
        )
    if provenance == "caller_supplied_prebuilt":
        items.append(
            "The measured binary was caller-supplied; its hash is authoritative, but "
            "source-to-binary linkage is not independently attested."
        )
    if not at_threshold_scale:
        items.append(
            no_verdict_reason
            or (
                "This run is not on the corpus every threshold is stated against, so it "
                "carries no gate verdict."
            )
        )
    return items


def validate_performance_manifest(path: Path) -> dict[str, Any]:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != PERFORMANCE_SCHEMA_VERSION:
        raise ValueError(f"unsupported schema_version: {manifest.get('schema_version')!r}")
    profile = manifest.get("profile")
    if profile not in PROFILES:
        raise ValueError(f"unknown profile: {profile!r}")
    commit = manifest.get("commit", "")
    if not isinstance(commit, str) or len(commit) != 40:
        raise ValueError("commit must be a full Git SHA")
    corpus = manifest.get("corpus", {})
    if corpus.get("contains_real_transcripts") is not False:
        raise ValueError("corpus.contains_real_transcripts must be false")
    at_threshold_scale = corpus.get("at_threshold_scale")
    if not isinstance(at_threshold_scale, bool):
        raise ValueError("corpus.at_threshold_scale must be a bool")
    if at_threshold_scale and corpus.get("message_count") != DEFAULT_MESSAGES:
        raise ValueError(
            f"corpus claims threshold scale but holds {corpus.get('message_count')!r} "
            f"messages, not {DEFAULT_MESSAGES}"
        )
    # Message count alone does not identify the thresholded corpus: a density
    # variant carries the same count and different bytes, and MiB/s is a function
    # of both. A verdict may only be claimed on the frozen density.
    body_scale = corpus.get("body_scale", DEFAULT_BODY_SCALE)
    if at_threshold_scale and body_scale != DEFAULT_BODY_SCALE:
        raise ValueError(
            f"corpus claims threshold scale but was generated with body_scale "
            f"{body_scale!r}; the thresholds are stated against the frozen corpus at "
            f"body_scale {DEFAULT_BODY_SCALE}"
        )
    metrics = manifest.get("metrics")
    if not isinstance(metrics, list) or not metrics:
        raise ValueError("metrics must be a non-empty list")
    expected = set(PERFORMANCE_THRESHOLDS) | set(PERFORMANCE_INFORMATIONAL_METRICS)
    actual = {m.get("name") for m in metrics}
    if actual != expected:
        raise ValueError(
            f"metric names {sorted(actual)} do not match the performance gate schema "
            f"{sorted(expected)}"
        )
    for entry in metrics:
        name = entry["name"]
        # Requirement enforced for every metric, thresholded or not: a latency
        # number without its corpus and n is misleading.
        reference = entry.get("corpus")
        if not isinstance(reference, dict) or "id" not in reference:
            raise ValueError(f"{name}: must name its corpus inline")
        if reference.get("message_count") != corpus.get("message_count"):
            raise ValueError(f"{name}: corpus reference disagrees with the manifest corpus")
        if not isinstance(entry.get("sample_count"), int) or entry["sample_count"] < 1:
            raise ValueError(f"{name}: must record a positive sample_count")
        if not entry.get("sample_unit"):
            raise ValueError(f"{name}: must record what one sample is")
        if not entry.get("action"):
            raise ValueError(f"{name}: must name the action measured")
        # Both MCP metrics carry a threshold whose wording names a condition
        # ("encoder resident") that a default build cannot satisfy. Each must
        # state on its own entry what was loaded, so reading one metric is
        # enough to know how narrow its number is.
        if name.startswith("mcp_"):
            detail = entry.get("detail")
            if not isinstance(detail, dict):
                raise ValueError(f"{name}: must carry a detail block")
            if not isinstance(detail.get("encoder_resident"), bool):
                raise ValueError(f"{name}: must state whether an encoder was resident")
            if not detail.get("binary_vector_backend"):
                raise ValueError(f"{name}: must name the binary's vector backend")
        if name in PERFORMANCE_INFORMATIONAL_METRICS:
            if entry.get("threshold") is not None or entry.get("pass") is not None:
                raise ValueError(
                    f"{name}: informational metrics must carry null threshold and pass"
                )
            if not entry.get("reason"):
                raise ValueError(f"{name}: informational metrics must record a reason")
            if entry.get("state") == STATE_NOT_MEASURED:
                # Deliberately not measured: the value must be null rather than
                # an estimate, and the entry must say why.
                if entry.get("value") is not None:
                    raise ValueError(f"{name}: unmeasured metrics must carry a null value")
                if not entry.get("detail", {}).get("skipped_reason"):
                    raise ValueError(f"{name}: unmeasured metrics must record why")
            elif not isinstance(entry.get("value"), (int, float)):
                raise ValueError(f"{name}: measured informational metrics need a numeric value")
            continue
        if entry.get("threshold") != PERFORMANCE_THRESHOLDS[name]:
            raise ValueError(
                f"{name}: threshold {entry.get('threshold')!r} != stated "
                f"{PERFORMANCE_THRESHOLDS[name]!r}"
            )
        if entry.get("threshold_direction") != THRESHOLD_DIRECTION[name]:
            raise ValueError(f"{name}: threshold_direction does not match the schema")
        if not entry.get("threshold_origin"):
            raise ValueError(f"{name}: must record where its threshold came from")
        if not isinstance(entry.get("value"), (int, float)):
            raise ValueError(f"{name}: must carry a numeric value")
        state = entry.get("state")
        if state == STATE_MEASURED:
            if not at_threshold_scale:
                raise ValueError(f"{name}: measured verdict off the thresholded corpus")
            if entry.get("pass") is not evaluate(name, float(entry["value"])):
                raise ValueError(f"{name}: pass flag disagrees with the recorded value")
        elif state in NO_VERDICT_STATES:
            if at_threshold_scale:
                raise ValueError(f"{name}: {state} claimed on the thresholded corpus")
            if entry.get("pass") is not None:
                raise ValueError(f"{name}: no-verdict metrics must carry a null pass")
            if not entry.get("reason"):
                raise ValueError(f"{name}: no-verdict metrics must record a reason")
        else:
            raise ValueError(f"{name}: unknown state {state!r}")
    gate = manifest.get("gate", {})
    thresholded = [m for m in metrics if m["name"] in PERFORMANCE_THRESHOLDS]
    expected_failures = sorted(m["name"] for m in thresholded if m["pass"] is False)
    expected_deferred = sorted(
        m["name"] for m in thresholded if m["state"] in NO_VERDICT_STATES
    )
    if gate.get("failures") != expected_failures:
        raise ValueError("gate.failures does not match the measured pass flags")
    if gate.get("deferred") != expected_deferred:
        raise ValueError("gate.deferred does not match the no-verdict metrics")
    expected_pass = None if expected_deferred else not expected_failures
    if gate.get("pass") != expected_pass:
        raise ValueError(f"gate.pass {gate.get('pass')!r} inconsistent with the metrics")
    runtime = manifest.get("runtime", {})
    if not isinstance(runtime.get("total_wall_clock_s"), (int, float)):
        raise ValueError("runtime.total_wall_clock_s must be recorded")
    if not manifest.get("ci_viability", {}).get("verdict"):
        raise ValueError("ci_viability.verdict must be recorded")
    print(f"valid {PERFORMANCE_SCHEMA_VERSION} manifest: {path}")
    return manifest


def parser() -> argparse.ArgumentParser:
    root = Path(__file__).resolve().parents[2]
    result = argparse.ArgumentParser(
        prog="performance_gate_benchmark.py", description=__doc__.split("\n\n")[0]
    )
    sub = result.add_subparsers(dest="command", required=True)
    run = sub.add_parser(
        "run", help="measure the four performance thresholds and emit the gate manifest"
    )
    run.add_argument(
        "--profile",
        choices=sorted(PROFILES),
        default="full",
        help="full gates at 100k; smoke is a wiring check with no verdict (default: %(default)s)",
    )
    run.add_argument("--workspace", default=str(root))
    run.add_argument("--output-dir", default=str(root / "scripts" / "evidence" / "out"))
    run.add_argument(
        "--corpus-dir",
        default=None,
        help=f"generated corpus root (default: <workspace>/{DEFAULT_OUTPUT_DIR.as_posix()})",
    )
    run.add_argument(
        "--binary", help="explicit prebuilt release CLI; otherwise cargo build --release is run"
    )
    run.add_argument("--cargo", default="cargo")
    run.add_argument("--noop-reps", type=int, default=3, help="full-corpus no-op sync passes")
    run.add_argument("--search-reps", type=int, default=100, help="total search invocations")
    run.add_argument("--mcp-reps", type=int, default=100, help="tools/call samples per mode")
    run.add_argument(
        "--overhead-reps", type=int, default=20, help="status invocations for process overhead"
    )
    run.add_argument(
        "--skip-embeddings",
        action="store_true",
        help=(
            "skip the vector-index build and the semantic MCP probe. Measured at 100k: "
            "the build alone is 285 s of a 372 s full run, and it feeds only "
            "informational metrics, so skipping it gates the four thresholds in ~87 s"
        ),
    )
    validate = sub.add_parser(
        "validate-report", help="validate a performance gate manifest against the schema"
    )
    validate.add_argument("report")
    return result


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "run":
            run_performance_gate(args)
        else:
            validate_performance_manifest(Path(args.report).expanduser().resolve())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
