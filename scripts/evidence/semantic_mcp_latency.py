#!/usr/bin/env python3
"""Measure amortized semantic query latency through the long-lived MCP entry point.

One-shot CLI search pays the model load per invocation; MCP keeps the encoder
resident, so this script reports the honest per-query latency users of the
long-lived entry points actually experience. Stdlib only. Output is a small
JSON under scripts/evidence/out/ (gitignored).

The transport half is exposed as :func:`measure_tool_calls` so other harnesses
(``performance_gate_benchmark.py``) measure MCP latency through exactly this
code path instead of reimplementing the JSON-RPC handshake.

The reported vectorizer is read back from ``index embeddings`` rather than
hardcoded: a default build has no ``semantic-candle`` feature and produces
bigram-hash vectors, and labelling those as an embedding model would claim a
resident encoder that was never loaded.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any, Sequence

OUT = Path(__file__).resolve().parent / "out"
# `index embeddings` reports this backend id when no real encoder is loaded: the
# vectors are a bigram hash computed in-process, which is fuzzy lexical
# similarity, not semantic. A report must never label those vectors with an
# embedding-model id.
FALLBACK_BACKEND = "bigram-hash"


def frame(id: int, method: str, params: object) -> dict:
    return {"jsonrpc": "2.0", "id": id, "method": method, "params": params}


def percentiles(samples: Sequence[float]) -> tuple[float, float]:
    """(p50, p95) over ``samples`` using this script's original index rule."""
    if not samples:
        raise ValueError("cannot take percentiles of an empty sample set")
    ordered = sorted(samples)
    p50 = ordered[len(ordered) // 2]
    p95 = ordered[max(0, int(len(ordered) * 0.95) - 1)]
    return p50, p95


def measure_tool_calls(
    binary: str | Path,
    db: Path,
    calls: Sequence[dict[str, Any]],
    tool: str = "search_sessions",
    timeout: float = 120.0,
) -> list[float]:
    """Time one ``tools/call`` per entry in ``calls`` against a resident server.

    Spawns a single ``mcp`` process, completes the handshake once, then issues
    the calls sequentially and returns per-call wall clock in milliseconds. One
    process for the whole sequence is the point: anything the server loads once
    (the encoder, the store handle) is amortized exactly as it is for a real
    long-lived client, which a one-shot CLI invocation cannot show.
    """
    if not calls:
        raise ValueError("no tool calls requested")
    process = subprocess.Popen(
        [str(binary), "--db", str(db), "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        encoding="utf-8",
    )
    if not process.stdin or not process.stdout:
        raise RuntimeError("failed to open MCP stdio pipes")
    samples: list[float] = []
    try:
        process.stdin.write(
            json.dumps(
                frame(
                    1,
                    "initialize",
                    {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "latency-probe", "version": "0"},
                    },
                )
            )
            + "\n"
        )
        process.stdin.write(
            json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"
        )
        process.stdin.flush()
        # Drain initialize response.
        process.stdout.readline()
        for index, arguments in enumerate(calls):
            request_id = 2 + index
            started = time.perf_counter()
            process.stdin.write(
                json.dumps(
                    frame(request_id, "tools/call", {"name": tool, "arguments": arguments})
                )
                + "\n"
            )
            process.stdin.flush()
            while True:
                line = process.stdout.readline()
                if not line:
                    raise RuntimeError(
                        f"MCP server closed stdout before answering request {request_id}"
                    )
                parsed = json.loads(line)
                if parsed.get("id") != request_id:
                    continue
                if parsed.get("error") is not None:
                    raise RuntimeError(f"MCP tools/call failed: {parsed['error']}")
                break
            samples.append((time.perf_counter() - started) * 1000.0)
    finally:
        if process.stdin:
            process.stdin.close()
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
    return samples


def default_binary() -> str:
    name = "agent-session-grep.exe" if os.name == "nt" else "agent-session-grep"
    return str(Path("target") / "release" / name)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="semantic_mcp_latency.py",
        description=(
            "Measure amortized per-query latency through a long-lived MCP process, "
            "where anything the server loads once (the encoder, the store handle) "
            "is amortized the way a real client experiences it."
        ),
    )
    parser.add_argument(
        "binary",
        nargs="?",
        default=default_binary(),
        help="release CLI binary to probe (default: %(default)s)",
    )
    parser.add_argument(
        "db",
        nargs="?",
        default=None,
        help="existing store to query; omitted means seed a throwaway store",
    )
    return parser


def vector_backend(binary: str, db: Path) -> dict[str, Any]:
    """Build the vector index and report which backend actually produced it.

    The backend is read back from the command's own response rather than
    assumed. A default build has no `semantic-candle` feature and silently
    produces bigram-hash vectors; labelling those with an embedding-model id
    would claim a resident encoder that was never loaded.
    """
    completed = subprocess.run(
        [binary, "--db", str(db), "--robot", "index", "embeddings"],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    data = json.loads(completed.stdout.strip().splitlines()[-1]).get("data", {})
    backend = data.get("backend", "not_recorded")
    return {
        "backend": backend,
        "model_id": data.get("model_id", "not_recorded"),
        "dimension": data.get("dimension"),
        "indexed": data.get("indexed"),
        "skipped": data.get("skipped"),
        # The only claim that matters for interpreting the latency below: was a
        # real embedding model resident in the process, or a hash function?
        "is_real_embedding_model": backend != FALLBACK_BACKEND,
    }


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    binary = args.binary
    override_db = Path(args.db) if args.db else None
    queries = [
        "ferroflux invalidate_stale",
        "FerrofluxCache epoch invalidation",
        "nebulamail drain_queue",
        "nebula retry 指数退避",
        "quartzgrid 索引 重建 一致性",
        "atlas config 备份 恢复",
        "polymorph 泛型 派生",
        "bifrost 迁移 schema 幂等",
        "warpdrive 缓存 失效 策略",
        "vertex 图 遍历 环 检测",
    ]
    # One scratch directory holds the seed fixture and, unless the caller
    # supplied a store, the database too. The fixture is always written here, so
    # the scratch directory must exist even when the caller passes its own --db.
    with tempfile.TemporaryDirectory(prefix="asg-mcp-latency-") as scratch_name:
        scratch = Path(scratch_name)
        db = override_db if override_db is not None else scratch / "latency.db"
        # Seed a few synthetic messages so semantic queries have a non-empty index.
        seed_lines = [
            json.dumps(
                {
                    "type": "user",
                    "uuid": f"00000000-0000-4000-8000-0000000000{i:02d}",
                    "sessionId": "11111111-2222-4333-8444-555555555555",
                    "timestamp": "2026-08-01T00:00:00.000Z",
                    "message": {"role": "user", "content": query},
                }
            )
            for i, query in enumerate(queries)
        ]
        fixture = scratch / "seed.jsonl"
        fixture.write_text("\n".join(seed_lines) + "\n", encoding="utf-8")
        subprocess.run(
            [binary, "--db", str(db), "--robot", "sync", str(fixture)],
            check=True,
            capture_output=True,
        )
        vectorizer = vector_backend(binary, db)
        samples = measure_tool_calls(
            binary,
            db,
            [{"query": query, "mode": "semantic", "limit": 10} for query in queries],
        )

    p50, p95 = percentiles(samples)
    OUT.mkdir(parents=True, exist_ok=True)
    report = {
        "schema": "agent-session-grep.semantic-mcp-latency/v2",
        "entry_point": "mcp",
        # Read back from `index embeddings`, never hardcoded: the measured
        # binary decides which vectorizer was resident, not this script.
        "vectorizer": vectorizer,
        "query_count": len(samples),
        "p50_ms": round(p50, 1),
        "p95_ms": round(p95, 1),
        "note": (
            "amortized per-query latency with the vectorizer resident in the MCP process; "
            "see vectorizer.is_real_embedding_model before reading this as semantic evidence"
        ),
    }
    (OUT / "semantic-mcp-latency.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(report, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
