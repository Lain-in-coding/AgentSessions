#!/usr/bin/env python3
"""B4 holdout retrieval-quality evaluation for agent-session-grep.

Six-bucket deterministic synthetic holdout (zh / en / code / long / near-dup /
no-result) with generator-side gold bookkeeping, a cosine similarity-floor
sweep for the semantic evidence gate, and per-bucket Recall@10 / MRR@10 /
false-hit / no-result / latency reporting.

Contracts this harness enforces:
- Synthetic and hand-authored only; no real transcripts, paths or identities.
- Claude-format JSONL records; message ids are `msg_v1_<record uuid>`; every
  gold id is verified reachable with `get` before measuring anything.
- Each query is built around a unique marker token that appears exactly in its
  gold messages; the generator asserts that (and that no-result markers appear
  nowhere in the corpus).
- Query construction is "remembered phrase": every gold query term is a
  contiguous substring of its gold text, because the default engine is
  lexical-first (FTS MATCH is AND over tokens). Pure paraphrase recall is not
  measured here; the frozen 100-query regression covers it. The lexical
  reachability contract is asserted per query at runtime (long-tail queries
  must be unreachable because the marker sits beyond the 16k index cap).
- The floor is applied only through the CLI test-only override
  `ASG_SEMANTIC_SIMILARITY_FLOOR`. The shipped-default pass sets no override
  and must reproduce the sweep row for the published default exactly.
- Honesty: when the backend is the default bigram fuzzy-lexical vectorizer,
  semantic/hybrid numbers are informational (model_kind=fuzzy_lexical_hash,
  is_real_embedding_model=false) and can never license promotion. A real E5
  run is skipped unless a verified bundle exists.

Usage:
  python retrieval_holdout.py run --binary <release binary> --shipped-default 0.2
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

N_SESSIONS = 12
MESSAGES_PER_SESSION = 10
K = 10
FLOORS = [
    "0.00", "0.05", "0.10", "0.15", "0.20", "0.25",
    "0.30", "0.35", "0.40", "0.45", "0.50", "0.60",
]
MODES = ["semantic", "hybrid"]
BUCKETS = ["zh", "en", "code", "long_body", "long_tail", "near_dup", "no_result"]
GOLD_BUCKETS = ["zh", "en", "code", "long_body", "long_tail", "near_dup"]


def message_uuid(index: int) -> str:
    return f"{index:08d}-0000-4000-8000-{index:012d}"


def gold_id(index: int) -> str:
    return f"msg_v1_{message_uuid(index)}"


def timestamp_for(session: int, position: int) -> str:
    total = (session * MESSAGES_PER_SESSION + position) * 3
    return (
        f"2026-04-{(total // 86400) + 1:02d}T"
        f"{(total // 3600) % 24 + 9:02d}:{(total // 60) % 60:02d}:{total % 60:02d}.000Z"
    )


ZH_FILLER = [
    "今天先整理一下上下文压缩的边界，旧数据不做删除。",
    "缓存层的命中率比上周高了，先记一笔观察结果。",
    "把重试策略的退避参数抄送给值班同学确认。",
    "构建产物需要核对版本号和依赖锁定文件。",
    "日志分级调整为 info 起步，排查时再临时开 debug。",
    "把常见的参数错误归到使用错误一类处理。",
    "会话摘要里只保留可复核的事实，不写推测。",
    "备份目录的清理计划下个迭代再定。",
]
EN_FILLER = [
    "note the retry budget numbers for the weekly review",
    "the cache warms slowly after a cold start",
    "keep the changelog entries short and factual",
    "we should re-check the lockfile before tagging",
    "logging stays at info unless debugging",
    "the summary should only contain checkable facts",
    "cleanup of the backup directory is deferred",
    "the audit trail keeps the original ordering",
]
CODE_FILLER = [
    "fn drain_pending(queue: &mut Vec<Job>) -> usize { queue.len() }",
    "SELECT id, payload FROM catalog ORDER BY id LIMIT 32",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "Path::new(\"src/main.rs\").extension() == Some(OsStr::new(\"rs\"))",
    "if let Some(row) = rows.next() { process(row)?; }",
    "pub const DEFAULT_TTL_MS: i64 = 900_000;",
]

LONG_BACKGROUND = (
    "这段是长文背景：会话历史里既有结构化的字段，也有自由文本的讨论；"
    "处理时先做规范化，再做投影，最后按预算裁剪响应。"
)

DUP_BODIES = [
    {
        "body": "当数据库迁移中断时先保留 last-good 索引再记录 fingerprint 重放批次",
        "queries": ["迁移中断 先保留 索引", "fingerprint 重放批次"],
    },
    {
        "body": "写日志前先脱敏逐条保留审计顺序跨边界输出统一走掩码规则",
        "queries": ["写日志前先脱敏 保留审计顺序", "跨边界输出 统一走掩码规则"],
    },
    {
        "body": "向量重建必须单事务完成失败时回滚全部替换并保留旧模型向量",
        "queries": ["向量重建 单事务完成", "失败时回滚全部替换"],
    },
    {
        "body": "分页令牌绑定查询摘要与代际过期后显式报错禁止静默从头再来",
        "queries": ["分页令牌绑定查询摘要", "过期后显式报错"],
    },
]
def build_dataset() -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Return (messages, queries). Gold ids come from generator bookkeeping."""
    messages: list[dict[str, Any]] = []
    planted: dict[tuple[int, int], tuple[str, str, str]] = {}

    def place(session: int, position: int, text: str, kind: str, marker: str) -> None:
        planted[(session, position)] = (text, kind, marker)

    # zh bucket: 10 golds, sessions 0-4 positions 1-2.
    zh_topics = [
        ("备份策略", "每日快照", "对比"),
        ("索引重建", "全量重投影", "耗时"),
        ("游标过期", "十五分钟", "续页"),
        ("脱敏规则", "默认掩码", "审计"),
        ("向量维度", "三百八十四", "存储"),
    ]
    for s in range(5):
        topic, detail, tail = zh_topics[s]
        for p, variant in ((1, "排查"), (2, "复盘")):
            marker = f"zhmarkerk{s}{p}"
            phrase = f"{marker} 把{topic}改成{detail}，并记录{variant}{tail}"
            extra = "补充说明：只保留可复核的事实，细节等下个迭代再讨论。"
            place(s, p, f"备忘：{phrase}。{extra}", "zh", marker)

    # en bucket: 10 golds, sessions 5-9 positions 1-2.
    en_topics = [
        ("retry budget", "capped at three attempts"),
        ("cold cache", "warmed by a small script"),
        ("lockfile drift", "checked before tagging"),
        ("audit ordering", "stable and explicit"),
        ("summary facts", "checkable only"),
    ]
    for i, (topic, detail) in enumerate(en_topics):
        s = 5 + i
        for p, verb in ((1, "triaged"), (2, "reviewed")):
            marker = f"enmarkerk{s}{p}"
            phrase = f"the {marker} {topic} policy is now {detail}"
            extra = f"we {verb} the incident today and keep only facts."
            place(s, p, f"note: {phrase}; {extra}", "en", marker)

    # code bucket: 10 golds, sessions 0-4 positions 3-4.
    code_topics = [
        ("resolve_symbol_table", "backend/query/router.go", "nil map write"),
        ("drain_pending", "src/queue/drain.rs", "index out of bounds"),
        ("parse_search_instant", "app/src/time.rs", "invalid RFC3339 input"),
        ("advance_generation_in_tx", "store/src/generation.rs", "stale baseline"),
        ("bounded_index_text", "app/src/retention.rs", "char boundary panic"),
    ]
    for s in range(5):
        func, path, err = code_topics[s]
        for p, ctx in ((3, "canary"), (4, "hotfix")):
            marker = f"codemarkerk{s}{p}"
            text = (
                f"{marker}: {path}:45 panicked at `{func}` with `{err}` "
                f"during the {ctx} run."
            )
            place(s, p, text, "code", marker)

    # long bucket: 8 golds, sessions 5-8 positions 3-4 (body + tail per session).
    for i in range(4):
        s = 5 + i
        body_marker = f"longmarkerbody{s}"
        tail_marker = f"longmarkertail{s}"
        body_prefix = LONG_BACKGROUND * ((6000 // len(LONG_BACKGROUND)) + 1)
        body_text = (
            body_prefix[:4600]
            + f" 补充：{body_marker} 的处理结论是保留旧索引并记录指纹。"
            + body_prefix[:300]
        )
        tail_prefix = LONG_BACKGROUND * ((22000 // len(LONG_BACKGROUND)) + 1)
        tail_text = (
            tail_prefix[:20000]
            + f" 文末追加：{tail_marker} 的最终结论是保留旧索引并记录指纹。"
            + tail_prefix[:800]
        )
        place(s, 3, body_text, "long_body", body_marker)
        place(s, 4, tail_text, "long_tail", tail_marker)

    # near-dup bucket: 4 pairs (8 golds) + 4 decoys sharing the body but not the marker.
    pair_slots = [
        ((9, 5), (10, 5)), ((9, 6), (10, 6)), ((9, 7), (11, 5)), ((10, 7), (11, 6)),
    ]
    decoy_slots = [(11, 7), (8, 5), (7, 5), (6, 5)]
    for k in range(4):
        marker = f"dupmarkerk{k:02d}"
        body = DUP_BODIES[k]["body"]
        for (s, p), suffix in zip(pair_slots[k], ("（副本 A）", "（副本 B）")):
            place(s, p, f"{marker} {body}{suffix}。复核对齐结果。", "near_dup", marker)
        s, p = decoy_slots[k]
        decoy_marker = f"decoymarkerk{k:02d}"
        place(s, p, f"{body}（旧版流程 {decoy_marker}）仅供参考。", "decoy", decoy_marker)

    # Fill everything else; position 0 stays neutral so session metadata hits
    # never fire on query terms.
    filler_index = 0
    for s in range(N_SESSIONS):
        for p in range(MESSAGES_PER_SESSION):
            if (s, p) in planted:
                continue
            if p == 0:
                text = f"session bootstrap note {s:02d}: neutral header, no query terms."
            elif p % 3 == 0:
                text = ZH_FILLER[filler_index % len(ZH_FILLER)]
            elif p % 3 == 1:
                text = EN_FILLER[filler_index % len(EN_FILLER)]
            else:
                text = CODE_FILLER[filler_index % len(CODE_FILLER)]
            filler_index += 1
            planted[(s, p)] = (text, "filler", "")

    for s in range(N_SESSIONS):
        for p in range(MESSAGES_PER_SESSION):
            text, kind, marker = planted[(s, p)]
            index = s * MESSAGES_PER_SESSION + p + 1
            messages.append(
                {
                    "message_index": index,
                    "session": s,
                    "position": p,
                    "role": "user" if p % 2 == 0 else "assistant",
                    "uuid": message_uuid(index),
                    "id": gold_id(index),
                    "text": text,
                    "kind": kind,
                    "marker": marker,
                }
            )

    by_marker: dict[str, list[str]] = {}
    for m in messages:
        if m["marker"]:
            by_marker.setdefault(m["marker"], []).append(m["id"])

    queries: list[dict[str, Any]] = []

    def add_query(
        qid: str, bucket: str, text: str, gold: list[str], marker: str, reachable: bool
    ) -> None:
        queries.append(
            {
                "id": qid,
                "bucket": bucket,
                "query": text,
                "gold_message_ids": gold,
                "marker": marker,
                "expect_lexical_reachable": reachable,
            }
        )

    for k, (topic, detail, tail) in enumerate(zh_topics):
        for p, variant in ((1, "排查"), (2, "复盘")):
            marker = f"zhmarkerk{k}{p}"
            add_query(
                f"zh-{k}-{p}", "zh",
                f"{marker} {topic} {detail} {variant} {tail}",
                by_marker[marker], marker, True,
            )
    for i, (topic, detail) in enumerate(en_topics):
        s = 5 + i
        for p, verb in ((1, "triaged"), (2, "reviewed")):
            marker = f"enmarkerk{s}{p}"
            add_query(
                f"en-{i}-{p}", "en",
                f"{marker} {topic} {detail} {verb}",
                by_marker[marker], marker, True,
            )
    for k, (func, path, err) in enumerate(code_topics):
        for p, ctx in ((3, "canary"), (4, "hotfix")):
            marker = f"codemarkerk{k}{p}"
            add_query(
                f"code-{k}-{p}", "code",
                f"{marker} {func} {path} {err} {ctx}",
                by_marker[marker], marker, True,
            )
    for i in range(4):
        s = 5 + i
        add_query(
            f"long-body-{i}", "long_body",
            f"longmarkerbody{s} 处理结论 保留旧索引",
            by_marker[f"longmarkerbody{s}"], f"longmarkerbody{s}", True,
        )
        add_query(
            f"long-tail-{i}", "long_tail",
            f"longmarkertail{s} 最终结论 保留旧索引",
            by_marker[f"longmarkertail{s}"], f"longmarkertail{s}", False,
        )
    for k in range(4):
        marker = f"dupmarkerk{k:02d}"
        pair = by_marker[marker]
        q1, q2 = DUP_BODIES[k]["queries"]
        add_query(f"dup-{k}-a", "near_dup", f"{marker} {q1}", pair, marker, True)
        add_query(f"dup-{k}-b", "near_dup", f"{marker} {q2}", pair, marker, True)
    for k in range(8):
        marker = f"voidmarkerk{k:02d}"
        add_query(
            f"none-{k}", "no_result",
            f"{marker} zzqxjw kqwwzpt nzqxwv", [], marker, False,
        )

    # Generator self-checks: marker scoping and gold-id sanity.
    for query in queries:
        marker = query["marker"]
        present = [m for m in messages if marker in m["text"]]
        got = sorted(m["id"] for m in present)
        want = sorted(query["gold_message_ids"])
        assert got == want, f"{query['id']}: marker {marker} scoped to {got}, expected {want}"
    return messages, queries

def write_corpus(corpus_dir: Path, messages: list[dict[str, Any]]) -> list[Path]:
    shutil.rmtree(corpus_dir, ignore_errors=True)
    corpus_dir.mkdir(parents=True)
    files: list[Path] = []
    for session in range(N_SESSIONS):
        rows = [m for m in messages if m["session"] == session]
        rows.sort(key=lambda m: m["position"])
        lines = []
        previous: str | None = None
        for m in rows:
            record: dict[str, Any] = {
                "type": m["role"],
                "uuid": m["uuid"],
                "parentUuid": previous,
                "sessionId": f"holdout-s{session:02d}",
                "timestamp": timestamp_for(m["session"], m["position"]),
                "message": {"role": m["role"], "content": m["text"]},
            }
            lines.append(json.dumps(record, ensure_ascii=False, separators=(",", ":")))
            previous = m["uuid"]
        path = corpus_dir / f"holdout-s{session:02d}.jsonl"
        path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        files.append(path)
    return files


def run_cli(
    binary: Path,
    workspace: Path,
    db: Path,
    args: list[str],
    floor: str | None,
) -> dict[str, Any]:
    env = None
    if floor is not None:
        env = dict(os.environ)
        env["ASG_SEMANTIC_SIMILARITY_FLOOR"] = floor
    command = [str(binary), "--db", str(db), "--output", "json", *args]
    started = time.perf_counter_ns()
    process = subprocess.run(
        command, cwd=workspace, env=env, capture_output=True, text=True, encoding="utf-8"
    )
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    frame: Any = None
    first = process.stdout.splitlines()[0] if process.stdout.strip() else ""
    try:
        frame = json.loads(first)
    except json.JSONDecodeError:
        frame = None
    return {
        "duration_ms": elapsed_ms,
        "exit_code": process.returncode,
        "stderr": process.stderr.strip(),
        "stdout_lines": len(process.stdout.splitlines()),
        "frame": frame,
    }


def percentile(values: list[float], pct: int) -> float:
    ordered = sorted(values)
    rank = max(1, math.ceil(pct / 100 * len(ordered)))
    return round(ordered[rank - 1], 3)


def evaluate_query(result: dict[str, Any], gold: list[str]) -> dict[str, Any]:
    frame = result["frame"]
    assert result["exit_code"] == 0, result
    assert frame is not None and frame.get("ok") is True, result
    hits = [hit["id"] for hit in frame["data"].get("hits", [])]
    gold_set = set(gold)
    recalled = [hit for hit in hits if hit in gold_set]
    rank = next((i + 1 for i, hit in enumerate(hits) if hit in gold_set), None)
    if gold:
        recall = len(recalled) / len(gold)
        mrr = 0.0 if rank is None else 1.0 / rank
    else:
        recall = None
        mrr = None
    return {
        "hits": hits,
        "hit_count": len(hits),
        "recalled": len(recalled),
        "recall": recall,
        "mrr": mrr,
        "false_hits": sum(1 for hit in hits if hit not in gold_set),
        "session_level_hits": sum(1 for hit in hits if not hit.startswith("msg_v1_")),
        "empty": not hits,
        "retrieval_mode": frame["data"].get("retrieval_mode"),
        "warnings": frame.get("warnings", []),
        "duration_ms": round(result["duration_ms"], 3),
    }


def summarize(rows: list[dict[str, Any]]) -> dict[str, Any]:
    gold_rows = [r for r in rows if r["bucket"] in GOLD_BUCKETS]
    none_rows = [r for r in rows if r["bucket"] == "no_result"]

    def bucket_mean(bucket: str, field: str) -> float:
        values = [
            r["metrics"][field]
            for r in rows
            if r["bucket"] == bucket and r["metrics"][field] is not None
        ]
        return round(sum(values) / max(1, len(values)), 6)

    by_bucket = {
        bucket: {
            "query_count": len([r for r in rows if r["bucket"] == bucket]),
            "recall_at_10": bucket_mean(bucket, "recall"),
            "mrr_at_10": bucket_mean(bucket, "mrr"),
            "false_hits": sum(r["metrics"]["false_hits"] for r in rows if r["bucket"] == bucket),
            "session_level_hits": sum(
                r["metrics"]["session_level_hits"] for r in rows if r["bucket"] == bucket
            ),
        }
        for bucket in BUCKETS
    }
    return {
        "query_count": len(rows),
        "gold_query_count": len(gold_rows),
        "recall_at_10_macro": round(
            sum(r["metrics"]["recall"] for r in gold_rows) / max(1, len(gold_rows)), 6
        ),
        "mrr_at_10_macro": round(
            sum(r["metrics"]["mrr"] for r in gold_rows) / max(1, len(gold_rows)), 6
        ),
        "false_hits_total": sum(r["metrics"]["false_hits"] for r in gold_rows),
        "no_result_empty_rate": round(
            sum(1 for r in none_rows if r["metrics"]["empty"]) / max(1, len(none_rows)), 6
        ),
        "no_result_false_hits": sum(r["metrics"]["false_hits"] for r in none_rows),
        "latency_p50_ms": percentile([r["metrics"]["duration_ms"] for r in rows], 50),
        "by_bucket": by_bucket,
    }

def run(args: argparse.Namespace) -> Path:
    workspace = Path(args.workspace).expanduser().resolve()
    binary = Path(args.binary).expanduser().resolve()
    output_dir = Path(args.output_dir).expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    corpus_dir = output_dir / "holdout-corpus"

    messages, queries = build_dataset()
    files = write_corpus(corpus_dir, messages)
    dataset_path = output_dir / "holdout-dataset.json"
    dataset_path.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "generator": "retrieval_holdout.py",
                "kind": "deterministic_synthetic",
                "contains_real_transcripts": False,
                "corpus": {
                    "provider": "claude-code",
                    "sessions": N_SESSIONS,
                    "messages": len(messages),
                    "files": [path.name for path in files],
                    "query_reachability_contract": (
                        "gold queries are remembered-phrase queries over the gold "
                        "text; long_tail queries must be lexically unreachable "
                        "(marker beyond the 16,000-char index cap)"
                    ),
                },
                "buckets": BUCKETS,
                "queries": queries,
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
        newline="\n",
    )

    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=workspace, capture_output=True, text=True
    ).stdout.strip()
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()

    raw_path = output_dir / "raw-runs.jsonl"
    raw = raw_path.open("w", encoding="utf-8", newline="\n")

    with tempfile.TemporaryDirectory(prefix="asg-holdout-") as temp:
        db = Path(temp) / "holdout.db"
        sync = run_cli(binary, workspace, db, ["sync", *(str(p) for p in files)], None)
        assert sync["exit_code"] == 0 and sync["frame"]["ok"], sync
        emitted = sync["frame"]["data"]["emitted"]
        skipped = sync["frame"]["data"]["skipped"]
        assert emitted == len(messages), (emitted, len(messages))
        assert skipped == 0, skipped
        embeddings = run_cli(binary, workspace, db, ["index", "embeddings"], None)
        assert embeddings["exit_code"] == 0 and embeddings["frame"]["ok"], embeddings
        embed_data = embeddings["frame"]["data"]

        # Verify every gold id is reachable before measuring.
        for query in queries:
            for gold in query["gold_message_ids"]:
                got = run_cli(binary, workspace, db, ["get", gold], None)
                assert got["exit_code"] == 0 and got["frame"]["ok"], (query["id"], gold, got)

        evaluations: dict[str, Any] = {}
        failures: list[str] = []

        # Lexical baseline (the floor does not apply) + reachability contract.
        lexical_rows = []
        for query in queries:
            result = run_cli(
                binary, workspace, db,
                ["search", query["query"], "--mode", "lexical", "--max-items", str(K)],
                None,
            )
            metrics = evaluate_query(result, query["gold_message_ids"])
            expected = query["expect_lexical_reachable"]
            if expected is True and metrics["recall"] != 1.0:
                failures.append(
                    f"{query['id']}: lexical reachability violated (recall {metrics['recall']})"
                )
            if expected is False and metrics["recall"] not in (None, 0.0):
                failures.append(f"{query['id']}: lexical query unexpectedly reachable")
            if expected is False and metrics["hits"]:
                failures.append(f"{query['id']}: expected no lexical hits, got {metrics['hits']}")
            row = {"query_id": query["id"], "bucket": query["bucket"], "metrics": metrics}
            lexical_rows.append(row)
            raw.write(json.dumps({"mode": "lexical", "floor": None, **row}, ensure_ascii=False) + "\n")
        evaluations["lexical"] = {
            "floor": None, "summary": summarize(lexical_rows), "rows": lexical_rows
        }
        assert not failures, "lexical reachability contract violated:\n" + "\n".join(failures)

        for floor in FLOORS:
            for mode in MODES:
                rows = []
                for query in queries:
                    result = run_cli(
                        binary, workspace, db,
                        ["search", query["query"], "--mode", mode, "--max-items", str(K)],
                        floor,
                    )
                    metrics = evaluate_query(result, query["gold_message_ids"])
                    row = {"query_id": query["id"], "bucket": query["bucket"], "metrics": metrics}
                    rows.append(row)
                    raw.write(
                        json.dumps({"mode": mode, "floor": floor, **row}, ensure_ascii=False) + "\n"
                    )
                evaluations[f"{mode}@{floor}"] = {
                    "floor": floor,
                    "summary": summarize(rows),
                    "rows": rows,
                }

        # Shipped-default pass: no override; must match the sweep row of the
        # published default exactly (guards constant/report drift).
        shipped = args.shipped_default
        shipped_rows = []
        for query in queries:
            result = run_cli(
                binary, workspace, db,
                ["search", query["query"], "--mode", "hybrid", "--max-items", str(K)],
                None,
            )
            metrics = evaluate_query(result, query["gold_message_ids"])
            row = {"query_id": query["id"], "bucket": query["bucket"], "metrics": metrics}
            shipped_rows.append(row)
            raw.write(
                json.dumps(
                    {"mode": "hybrid", "floor": "shipped_default", **row}, ensure_ascii=False
                )
                + "\n"
            )
        reference = evaluations[f"hybrid@{float(shipped):.2f}"]
        mismatches = [
            (a["query_id"], a["metrics"]["hits"], b["metrics"]["hits"])
            for a, b in zip(shipped_rows, reference["rows"])
            if a["metrics"]["hits"] != b["metrics"]["hits"]
        ]
        assert not mismatches, f"shipped default {shipped} != sweep row: {mismatches[:3]}"

        # Candidate ranking: per-bucket holdout recall loss (semantic AND hybrid)
        # vs floor 0.00, plus the frozen 100-query regression cross-check when a
        # report directory is supplied. Selection = largest 0.05-grid point that
        # is lossless on BOTH benchmarks, minus one grid step of margin.
        candidates = []
        for floor in FLOORS:
            hybrid = evaluations[f"hybrid@{floor}"]["summary"]
            semantic = evaluations[f"semantic@{floor}"]["summary"]
            loss = {
                bucket: max(
                    round(
                        evaluations["hybrid@0.00"]["summary"]["by_bucket"][bucket]["recall_at_10"]
                        - hybrid["by_bucket"][bucket]["recall_at_10"],
                        6,
                    ),
                    round(
                        evaluations["semantic@0.00"]["summary"]["by_bucket"][bucket]["recall_at_10"]
                        - semantic["by_bucket"][bucket]["recall_at_10"],
                        6,
                    ),
                )
                for bucket in GOLD_BUCKETS
            }
            candidates.append(
                {
                    "floor": float(floor),
                    "max_recall_loss": max(loss.values()) if loss else 0.0,
                    "hybrid_no_result_empty_rate": hybrid["no_result_empty_rate"],
                    "semantic_no_result_empty_rate": semantic["no_result_empty_rate"],
                    "hybrid_false_hits_total": hybrid["false_hits_total"],
                    "semantic_false_hits_total": semantic["false_hits_total"],
                    "recall_loss_by_bucket": loss,
                }
            )
        holdout_lossless = [c["floor"] for c in candidates if c["max_recall_loss"] <= 0.0]

        frozen = None
        frozen_lossless: list[float] = []
        if args.frozen_regression_dir:
            frozen_dir = Path(args.frozen_regression_dir).expanduser().resolve()
            frozen = {}
            for report_path in sorted(frozen_dir.glob("semantic-benchmark-b4-floor-*.json")):
                label = report_path.name.split("semantic-benchmark-b4-floor-")[1][:-5]
                try:
                    floor_value = float(label)
                except ValueError:
                    continue
                report = json.loads(report_path.read_text(encoding="utf-8"))
                per_mode = report["recall"]["per_mode"]
                frozen[floor_value] = {
                    "semantic_at_5": per_mode["semantic"]["recall_at_5"],
                    "semantic_at_10": per_mode["semantic"]["recall_at_10"],
                    "semantic_at_20": per_mode["semantic"]["recall_at_20"],
                    "hybrid_at_5": per_mode["hybrid"]["recall_at_5"],
                    "hybrid_at_10": per_mode["hybrid"]["recall_at_10"],
                    "hybrid_at_20": per_mode["hybrid"]["recall_at_20"],
                }
            baseline = frozen.get(0.0)
            assert baseline, "frozen regression baseline (floor 0.00) report is missing"
            frozen_lossless = sorted(
                floor_value
                for floor_value, metrics in frozen.items()
                if metrics["semantic_at_10"] == baseline["semantic_at_10"]
                and metrics["hybrid_at_10"] == baseline["hybrid_at_10"]
            )

        if frozen_lossless:
            combined = sorted(set(holdout_lossless) & set(frozen_lossless))
        else:
            combined = list(holdout_lossless)
        boundary = max(combined) if combined else 0.0
        selected = round(max(0.0, boundary - 0.05), 2)

        scan = {
            "schema_version": 1,
            "commit": commit,
            "binary": {"path": str(binary), "sha256": binary_hash},
            "dataset": {"path": dataset_path.name, "buckets": BUCKETS, "query_count": len(queries)},
            "model": {
                "model_id": embed_data.get("model_id"),
                "model_kind": embed_data.get("model_kind"),
                "backend": embed_data.get("backend"),
                "is_real_embedding_model": embed_data.get("backend") == "semantic-candle",
                "skip_real_e5": embed_data.get("backend") != "semantic-candle",
            },
            "indexed": embed_data.get("indexed"),
            "skipped": embed_data.get("skipped"),
            "k": K,
            "floors": FLOORS,
            "evaluations": evaluations,
            "selection": {
                "rule": (
                    "default = largest 0.05-grid floor that is lossless on BOTH the "
                    "holdout (per-bucket Recall@10, semantic and hybrid, vs floor "
                    "0.00) and the frozen 100-query regression (semantic@10 and "
                    "hybrid@10 vs floor 0.00), minus one grid step of margin; if no "
                    "positive floor is lossless, default 0.0 (defensive path only)"
                ),
                "candidates": candidates,
                "holdout_lossless_floors": holdout_lossless,
                "frozen_regression": frozen,
                "frozen_lossless_floors": frozen_lossless,
                "combined_lossless_floors": combined,
                "boundary_floor": boundary,
                "selected_floor": selected,
                "shipped_default": float(shipped),
                "shipped_default_matches_selected": float(shipped) == selected,
                "shipped_default_run_matches_sweep_row": True,
            },
            "honesty": {
                "promotion_claim": "none",
                "lexical_stays_default": True,
                "note": (
                    "Default backend is the bigram fuzzy lexical hash; semantic/hybrid "
                    "numbers are informational and cannot license promotion. Real-E5 "
                    "calibration is skipped unless a verified bundle is imported."
                ),
            },
        }
        scan_path = output_dir / "threshold-scan.json"
        scan_path.write_text(
            json.dumps(scan, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n"
        )

    raw.close()

    # Human-readable console summary.
    print(f"dataset: {dataset_path}")
    print(f"scan:    {output_dir / 'threshold-scan.json'}")
    print(f"backend: {embed_data.get('model_kind')} ({embed_data.get('model_id')})")
    print(f"indexed: {embed_data.get('indexed')} skipped: {embed_data.get('skipped')}")
    print(f"selected floor: {selected} (shipped default {shipped})")
    print(
        f"{'mode':>10} {'floor':>6} {'recall@10':>10} {'mrr@10':>8} "
        f"{'false':>6} {'no-res-empty':>13}"
    )
    for key, value in evaluations.items():
        s = value["summary"]
        print(
            f"{key.split('@')[0]:>10} {str(value['floor']):>6} "
            f"{s['recall_at_10_macro']:>10.4f} {s['mrr_at_10_macro']:>8.4f} "
            f"{s['false_hits_total']:>6} {s['no_result_empty_rate']:>13.4f}"
        )
    for bucket in BUCKETS:
        row = evaluations[f"hybrid@{float(shipped):.2f}"]["summary"]["by_bucket"][bucket]
        print(
            f"  bucket {bucket:<10} n={row['query_count']:>3} "
            f"recall={row['recall_at_10']:.4f} mrr={row['mrr_at_10']:.4f} "
            f"false={row['false_hits']} sess_hits={row['session_level_hits']}"
        )
    return scan_path


def positive_float(value: str) -> float:
    try:
        return float(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError(str(error)) from error


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run_parser = sub.add_parser("run", help="generate the holdout and run the floor sweep")
    run_parser.add_argument("--binary", required=True)
    run_parser.add_argument("--workspace", default=str(Path(__file__).resolve().parents[4]))
    run_parser.add_argument("--output-dir", default=str(Path(__file__).resolve().parent))
    run_parser.add_argument("--shipped-default", type=positive_float, required=True)
    run_parser.add_argument(
        "--frozen-regression-dir",
        help="directory holding semantic-benchmark-b4-floor-*.json frozen regression reports",
    )
    args = parser.parse_args()
    if args.command == "run":
        run(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())