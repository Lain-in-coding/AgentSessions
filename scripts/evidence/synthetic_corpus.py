#!/usr/bin/env python3
"""Deterministic multi-provider synthetic corpus generator.

Every performance threshold in this project's release gates is stated at
100,000 messages, but the largest corpus committed to the tree holds 2,000
(``scripts/evidence/fixtures/semantic/``). This generator produces a
reproducible corpus at gate scale — 100,000 messages across ~5,000 sessions
and six providers — without ever reading, copying, or shipping a real
transcript.

Determinism is the contract:

* one fixed seed (:data:`SEED`), no wall-clock reads, no unseeded ``random``;
* every session draws from its own RNG seeded by ``sha256(SEED:index)``, so the
  bytes of one session never depend on how many sessions were generated or in
  what order the writer visited them;
* the corpus digest is a CRLF-normalized tree hash, mirroring
  ``scripts/evidence/semantic_benchmark.py::normalized_tree_hash`` — hashing raw
  bytes would make the manifest platform-dependent on a Windows checkout.

All prose, code, and identifiers are hand-authored in this file. No provider
transcript content, path, or identity is copied. Uses only the standard
library.

Commands::

    synthetic_corpus.py generate [--output-dir DIR] [--sessions N] [--messages M]
    synthetic_corpus.py verify   [--output-dir DIR]
    synthetic_corpus.py ingest   [--output-dir DIR] [--binary PATH]

The corpus is ~50 MB and is never committed: the default output directory sits
under the gitignored ``evidence-output/``. What is committed is this generator,
its test, and the frozen manifest at
``scripts/evidence/fixtures/synthetic-corpus-100k/manifest.json`` that pins the
expected tree hash and counts.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import random
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Callable, Iterable, Sequence

SCHEMA_VERSION = "agent-session-grep.synthetic-corpus-manifest/v1"

# The frozen manifest: the committed contract for the (uncommitted) corpus.
MANIFEST_PATH = (
    Path(__file__).resolve().parent / "fixtures" / "synthetic-corpus-100k" / "manifest.json"
)
# Default output directory, relative to the workspace root. `evidence-output/`
# is gitignored, so a generated corpus can never be committed by accident.
DEFAULT_OUTPUT_DIR = Path("evidence-output") / "synthetic-corpus-100k"

# ---------------------------------------------------------------------------
# Frozen generator parameters. Changing any of these changes the corpus and
# therefore its manifest hash; bump CORPUS_VERSION when that is deliberate.
# ---------------------------------------------------------------------------
SEED = 20260819
CORPUS_VERSION = "1"
DEFAULT_SESSIONS = 5_000
DEFAULT_MESSAGES = 100_000

# Epoch for every synthetic timestamp. Fixed so no wall clock is ever read.
EPOCH = datetime(2026, 1, 1, 0, 0, 0, tzinfo=timezone.utc)
SESSION_STRIDE = timedelta(minutes=7)
MESSAGE_STRIDE = timedelta(seconds=37)

# Provider mix. claude-code and codex carry the largest share because they are
# the only two providers with a recorded real-corpus regression in this repo
# (docs/evidence/integration-beta/real-data-regression.md ingests exactly those
# two roots); the four smaller adapters split the remainder evenly.
PROVIDER_SHARE: tuple[tuple[str, float], ...] = (
    ("aider", 0.09),
    ("claude-code", 0.40),
    ("cline", 0.09),
    ("codex", 0.25),
    ("hermes", 0.085),
    ("kimi", 0.085),
)
# Remainder of the rounded share split lands here, so the session total is exact.
BALANCING_PROVIDER = "claude-code"

PROVIDER_EXTENSION = {
    "aider": "md",
    "claude-code": "jsonl",
    "cline": "json",
    "codex": "jsonl",
    "hermes": "json",
    "kimi": "jsonl",
}

# Messages per session: inverse-CDF buckets (cumulative probability, low, high).
# Shaped so the mean lands near the 20 messages/session the 100k/5k target
# implies, with a long tail of ~100-message sessions.
SESSION_LENGTH_CDF: tuple[tuple[float, int, int], ...] = (
    (0.10, 4, 7),
    (0.35, 7, 12),
    (0.65, 12, 20),
    (0.85, 20, 30),
    (0.97, 30, 56),
    (1.00, 56, 96),
)

# Message text length, in characters. The body buckets reproduce the measured
# quantiles of the only corpus measured inside this repository, the frozen
# semantic corpus under scripts/evidence/fixtures/semantic/corpus (n=2000:
# p10 37, p25 39, p50 94, p75 107, p90 141, p99 216, max 355). Real transcripts
# also carry long tool output and stack traces, which that corpus has none of,
# so two heavier classes are layered on top (see MESSAGE_CLASS_MIX).
BODY_LENGTH_CDF: tuple[tuple[float, int, int], ...] = (
    (0.10, 24, 37),
    (0.25, 37, 39),
    (0.50, 39, 94),
    (0.75, 94, 107),
    (0.90, 107, 141),
    (0.99, 141, 216),
    (1.00, 216, 355),
)
# (class, probability, low, high). `body` uses BODY_LENGTH_CDF; `medium` and
# `long` are log-uniform over their range (code blocks / diffs, and tool output
# / stack traces respectively).
MESSAGE_CLASS_MIX: tuple[tuple[str, float, int, int], ...] = (
    ("body", 0.90, 0, 0),
    ("medium", 0.09, 300, 2_000),
    ("long", 0.01, 2_000, 8_000),
)

# Language mix per message, matching the measured mix of the frozen semantic
# corpus (zh 879 / en 691 / code 430 of 2000 = 43.95% / 34.55% / 21.50%).
LANGUAGE_MIX: tuple[tuple[str, float], ...] = (
    ("zh", 0.44),
    ("en", 0.35),
    ("code", 0.21),
)
# Every Nth fragment of a zh message is a latin technical snippet. The frozen
# semantic corpus measures 14.35% CJK characters overall; Chinese prose alone
# would land near 30%, so zh messages interleave latin commands, paths and
# identifiers the way a real bilingual transcript does. The achieved ratio is
# measured, not assumed, and recorded in the manifest.
ZH_LATIN_EVERY = 2

# ---------------------------------------------------------------------------
# Hand-authored content banks.
# ---------------------------------------------------------------------------
ZH_FRAGMENTS: tuple[str, ...] = (
    "跨度必须按字节计算，end 排他。",
    "中文查询的召回率在 bigram 预处理之前只有三成。",
    "解析器遇到截断行时应该静默跳过并计入 skipped。",
    "这个批次里有两个源文件的指纹没有变化。",
    "先确认写锁是否被上一个进程持有。",
    "全文索引是可重建投影，迁移走 rebuild。",
    "会话摘要不是对话记录，不应该进入检索。",
    "把时间窗过滤排除的记录数报出来，不要静默丢弃。",
    "同一个消息在两个文档里出现时应该按内容去重。",
    "补丁只改必要的那几行，不做无关重构。",
    "错误信息要指向根因，不要掩盖症状。",
    "回归跑完之后把两次的哈希贴出来对比。",
    "这个字段在旧版本里是可选的，需要兼容。",
    "工具调用的结果正文也要能被检索到。",
    "分页游标失效时应该返回明确的错误码。",
    "多字节字符的偏移量算错会导致高亮错位。",
    "先读现状再动手改，不要盲目重写。",
    "这一版的默认值改了，文档也要同步。",
    "本地评测语料是合成的，不含任何真实会话。",
    "提交信息按约定式提交写，一次只做一件事。",
    "如果计数不一致就停下来报告，不要绕过。",
    "旁支会话的消息同样要计入总数。",
    "空内容的记录跳过，但要记在诊断里。",
    "路径分隔符在两个平台上都要能正确解析。",
    "这个查询走的是词法通道，不是语义通道。",
)
ZH_LATIN_TOKENS: tuple[str, ...] = (
    "span.end 是排他的 exclusive upper bound",
    "session_id / provider_id / variant_id",
    "msg_v1_ 前缀的 de-duplicated entity id",
    "cargo test --workspace -- --nocapture",
    "sqlite3 WAL + synchronous=FULL",
    "FTS5 unicode61 remove_diacritics 2",
    "crates/provider/src/lib.rs:214",
    "--robot --db catalog.db sync <file>",
    "emitted=180718 skipped=0 unchanged=0",
    "generation=3 lease=held cursor=stale",
    "parentUuid / uuid / isSidechain",
    "utf-8 byte offset != char index",
    "SHA-256 digest over CRLF-normalized bytes",
    "p50=12ms p95=48ms p99=131ms",
    "GET /v1/sessions?limit=50&cursor=...",
)
EN_FRAGMENTS: tuple[str, ...] = (
    "The parser treats every span as a byte offset with an exclusive end.",
    "Chunk the source list so one command line never exceeds the OS limit.",
    "A truncated trailing line is skipped and counted, never guessed at.",
    "Fingerprint caching lets an unchanged source skip the whole batch.",
    "The writer lease is held for the duration of one atomic batch.",
    "Full-text search is a rebuildable projection over the catalog.",
    "Sequence-derived message ids collide across documents, so they are gone.",
    "Report the count a time filter excluded instead of dropping it silently.",
    "Deduplicate identical payloads by content, not by arrival order.",
    "Keep the change minimal and leave unrelated code untouched.",
    "Fix the root cause rather than suppressing the symptom.",
    "Print both digests so a reader can compare them without trusting us.",
    "This field was optional in the previous schema version.",
    "Tool result bodies must remain searchable after ingestion.",
    "An invalid cursor returns a typed error instead of an empty page.",
    "Off-by-one byte offsets shift every highlight in the rendered output.",
    "Read the surrounding code before changing a single line of it.",
    "The default changed in this release and the documentation follows.",
    "The evaluation corpus is synthetic and holds no real conversation.",
    "One logical change per commit, with a conventional commit subject.",
    "Stop and report a count mismatch instead of working around it.",
    "Sidechain messages count toward the emitted total like any other.",
    "Empty records are skipped but still show up in the diagnostics.",
    "Path separators must round-trip on both supported platforms.",
    "This query runs through the lexical channel, not the semantic one.",
)
CODE_FRAGMENTS: tuple[str, ...] = (
    "fn parse(bytes: &[u8], sink: &mut dyn Sink) -> Result<Report, ProviderError> {",
    "    let text = std::str::from_utf8(bytes).map_err(structural_fatal)?;",
    "    for (index, line) in text.lines().enumerate() {",
    "        if line.trim().is_empty() { continue; }",
    "        let value: serde_json::Value = match serde_json::from_str(line) {",
    "            Ok(value) => value,",
    "            Err(error) => { report.skipped += 1; continue; }",
    "        };",
    "    }",
    "}",
    "assert_eq!(report.committed, expected_message_count);",
    "SELECT id FROM entities WHERE id LIKE 'msg_v1_%' ORDER BY id;",
    "let digest = hashlib_sha256(payload.as_bytes());",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "python scripts/evidence/synthetic_corpus.py verify --output-dir out",
    "thread 'main' panicked at crates/provider/src/lib.rs:214:9",
    "  10: core::panicking::panic_fmt",
    "Traceback (most recent call last):",
    '  File "harness.py", line 88, in run_batch',
    "error[E0308]: mismatched types, expected `&str`, found `String`",
    "warning: unused variable: `report`",
    "@@ -214,7 +214,7 @@ impl ProviderAdapter for Adapter {",
    "-        native_id: &format!(\"provider-msg-{seq}\"),",
    "+        native_id: \"\",",
    "{\"ok\":true,\"command\":\"sync\",\"data\":{\"emitted\":42,\"skipped\":0}}",
    "index 1a2b3c4..5d6e7f8 100644",
)
PROJECT_SLUGS: tuple[str, ...] = (
    "svc-api",
    "cli-tools",
    "index-engine",
    "web-ui",
    "docs-site",
    "batch-worker",
    "parser-lab",
    "release-kit",
)


# ---------------------------------------------------------------------------
# Determinism helpers.
# ---------------------------------------------------------------------------
def seeded_rng(*parts: object) -> random.Random:
    """Return an RNG seeded by ``sha256(SEED:parts)``.

    A per-session RNG keeps one session's bytes independent of how many other
    sessions exist and of the order the writer visited them, which is what
    makes a partial regeneration comparable to a full one.
    """
    key = ":".join([str(SEED), *(str(part) for part in parts)])
    return random.Random(int(hashlib.sha256(key.encode("utf-8")).hexdigest(), 16))


def stable_hex(*parts: object, width: int = 32) -> str:
    """Deterministic lowercase hex token derived from the seed and ``parts``."""
    key = ":".join([str(SEED), *(str(part) for part in parts)])
    return hashlib.sha256(key.encode("utf-8")).hexdigest()[:width]


def stable_uuid(*parts: object) -> str:
    """A deterministic RFC-4122-shaped v4 identifier (no randomness, no clock)."""
    digest = stable_hex(*parts, width=32)
    return (
        f"{digest[0:8]}-{digest[8:12]}-4{digest[13:16]}-"
        f"8{digest[17:20]}-{digest[20:32]}"
    )


def normalized_tree_hash(paths: Iterable[Path], root: Path) -> str:
    """CRLF-normalized tree hash over ``paths`` relative to ``root``.

    Mirrors ``scripts/evidence/semantic_benchmark.py::normalized_tree_hash``:
    git's Windows checkout may rewrite line endings, so hashing raw bytes would
    make the manifest platform-dependent. The frozen contract is the logical
    content, so both generation and verification hash CRLF-normalized bytes.
    """
    digest = hashlib.sha256()
    for path in sorted(paths, key=lambda item: item.relative_to(root).as_posix()):
        digest.update(path.relative_to(root).as_posix().encode("utf-8"))
        digest.update(b"\0")
        digest.update(path.read_bytes().replace(b"\r\n", b"\n"))
        digest.update(b"\0")
    return digest.hexdigest()


def sample_cdf(rng: random.Random, buckets: Sequence[tuple[float, int, int]]) -> int:
    """Inverse-CDF sample: pick a bucket by cumulative probability, then lerp."""
    u = rng.random()
    for cumulative, low, high in buckets:
        if u <= cumulative:
            if high <= low:
                return low
            return low + int((high - low) * rng.random())
    _, low, high = buckets[-1]
    return high if high > low else low


def sample_log_uniform(rng: random.Random, low: int, high: int) -> int:
    """Log-uniform integer in ``[low, high]`` — a heavy tail without extremes."""
    return int(math.exp(rng.uniform(math.log(low), math.log(high))))


def pick_weighted(rng: random.Random, options: Sequence[tuple[str, float]]) -> str:
    u = rng.random()
    cumulative = 0.0
    for name, weight in options:
        cumulative += weight
        if u <= cumulative:
            return name
    return options[-1][0]


# ---------------------------------------------------------------------------
# Corpus plan: which provider owns which session, and how long each session is.
# ---------------------------------------------------------------------------
def provider_assignment(sessions: int) -> list[str]:
    """Assign a provider to every session index by frozen share.

    Rounding remainder goes to :data:`BALANCING_PROVIDER`, so the assignment
    covers exactly ``sessions`` entries. Providers are laid out in contiguous
    blocks ordered by provider id, which keeps the layout reproducible and the
    per-provider counts trivially checkable.
    """
    if sessions <= 0:
        raise ValueError("sessions must be positive")
    counts: dict[str, int] = {}
    for name, share in PROVIDER_SHARE:
        counts[name] = int(sessions * share)
    assigned = sum(counts.values())
    counts[BALANCING_PROVIDER] += sessions - assigned
    if any(count <= 0 for count in counts.values()):
        raise ValueError(
            f"{sessions} sessions is too few to give all "
            f"{len(PROVIDER_SHARE)} providers at least one session"
        )
    assignment: list[str] = []
    for name, _share in PROVIDER_SHARE:
        assignment.extend([name] * counts[name])
    return assignment


def session_message_counts(assignment: Sequence[str], messages: int) -> list[int]:
    """Per-session message counts that sum to exactly ``messages``.

    Counts are drawn from :data:`SESSION_LENGTH_CDF`. Aider transcripts are
    user/assistant markdown pairs, so those counts are rounded up to even. The
    residual against the requested total is then spread one message at a time
    over the balancing provider's sessions, in index order, so the total is
    exact without distorting the shape.
    """
    counts = [
        max(2, sample_cdf(seeded_rng("session-length", index), SESSION_LENGTH_CDF))
        for index in range(len(assignment))
    ]
    for index, provider in enumerate(assignment):
        if provider == "aider" and counts[index] % 2 == 1:
            counts[index] += 1
    balancing = [i for i, provider in enumerate(assignment) if provider == BALANCING_PROVIDER]
    if not balancing:
        raise ValueError("no balancing-provider session to absorb the residual")
    residual = messages - sum(counts)
    step = 1 if residual > 0 else -1
    cursor = 0
    while residual != 0:
        index = balancing[cursor % len(balancing)]
        if step > 0 or counts[index] > 2:
            counts[index] += step
            residual -= step
        cursor += 1
        if cursor > len(balancing) * 4_000:
            raise ValueError("cannot reach the requested message total")
    return counts


# ---------------------------------------------------------------------------
# Message content.
# ---------------------------------------------------------------------------
def build_text(rng: random.Random, language: str, target: int, single_line: bool) -> str:
    """Assemble ``language`` text of roughly ``target`` characters.

    Fragments are appended whole, so the result never cuts a word or an
    identifier in half; the achieved length distribution is measured after
    generation rather than assumed from ``target``.
    """
    if language == "zh":
        bank, joiner = ZH_FRAGMENTS, ""
    elif language == "en":
        bank, joiner = EN_FRAGMENTS, " "
    elif language == "code":
        bank, joiner = CODE_FRAGMENTS, "\n"
    else:
        raise ValueError(f"unknown language: {language!r}")
    parts: list[str] = []
    length = 0
    while length < target:
        if language == "zh" and len(parts) % ZH_LATIN_EVERY == ZH_LATIN_EVERY - 1:
            piece = rng.choice(ZH_LATIN_TOKENS)
        else:
            piece = rng.choice(bank)
        parts.append(piece)
        length += len(piece) + len(joiner)
    text = joiner.join(parts)
    if single_line:
        text = " ".join(text.split("\n"))
    return text


def message_target_length(rng: random.Random) -> int:
    """Sample one message's target character length from the class mix."""
    u = rng.random()
    cumulative = 0.0
    for name, probability, low, high in MESSAGE_CLASS_MIX:
        cumulative += probability
        if u <= cumulative:
            if name == "body":
                return sample_cdf(rng, BODY_LENGTH_CDF)
            return sample_log_uniform(rng, low, high)
    return sample_cdf(rng, BODY_LENGTH_CDF)


def build_session_messages(index: int, count: int, single_line: bool) -> list[dict[str, Any]]:
    """Build one session's messages: alternating roles, mixed languages."""
    rng = seeded_rng("content", index)
    messages: list[dict[str, Any]] = []
    for seq in range(count):
        language = pick_weighted(rng, LANGUAGE_MIX)
        target = message_target_length(rng)
        messages.append(
            {
                "seq": seq,
                "role": "user" if seq % 2 == 0 else "assistant",
                "language": language,
                "text": build_text(rng, language, target, single_line),
            }
        )
    return messages


def session_timestamp(index: int, seq: int) -> datetime:
    return EPOCH + SESSION_STRIDE * index + MESSAGE_STRIDE * seq


def iso_millis(moment: datetime) -> str:
    return moment.strftime("%Y-%m-%dT%H:%M:%S.") + f"{moment.microsecond // 1000:03d}Z"


def iso_seconds(moment: datetime) -> str:
    return moment.strftime("%Y-%m-%dT%H:%M:%SZ")


def iso_naive(moment: datetime) -> str:
    return moment.strftime("%Y-%m-%dT%H:%M:%S")


def project_cwd(index: int) -> str:
    """Placeholder working directory — never a real user home or checkout."""
    return f"/workspace/{PROJECT_SLUGS[index % len(PROJECT_SLUGS)]}"


# ---------------------------------------------------------------------------
# Per-provider renderers. Each returns the exact file text; the number of
# messages a renderer emits always equals ``len(messages)``.
# ---------------------------------------------------------------------------
def render_claude_code(index: int, messages: Sequence[dict[str, Any]]) -> str:
    session_id = stable_uuid("claude-session", index)
    cwd = project_cwd(index)
    lines: list[str] = []
    previous_uuid: str | None = None
    for message in messages:
        seq = message["seq"]
        uuid = stable_uuid("claude-msg", index, seq)
        role = message["role"]
        # Every fourth turn-pair runs as a sidechain branch, mirroring the
        # sub-agent traffic real Claude Code transcripts carry. Sidechain
        # messages are committed like any other, so the count is unaffected.
        sidechain = (seq // 2) % 8 == 7
        content: Any = message["text"]
        if role == "assistant":
            content = [{"type": "text", "text": message["text"]}]
        record = {
            "parentUuid": previous_uuid,
            "isSidechain": sidechain,
            "userType": "external",
            "cwd": cwd,
            "version": "2.0.0",
            "type": role,
            "message": {"role": role, "content": content},
            "uuid": uuid,
            "timestamp": iso_millis(session_timestamp(index, seq)),
            "sessionId": session_id,
        }
        lines.append(json.dumps(record, ensure_ascii=False, separators=(",", ":")))
        previous_uuid = uuid
    return "\n".join(lines) + "\n"


def render_codex(index: int, messages: Sequence[dict[str, Any]]) -> str:
    session_id = stable_uuid("codex-session", index)
    cwd = project_cwd(index)
    start = session_timestamp(index, 0)
    lines = [
        json.dumps(
            {
                "timestamp": iso_millis(start),
                "type": "session_meta",
                "payload": {
                    "session_id": session_id,
                    "cwd": cwd,
                    "originator": "codex_cli_rs",
                    "cli_version": "0.0.0-synthetic",
                },
            },
            ensure_ascii=False,
            separators=(",", ":"),
        ),
        json.dumps(
            {
                "timestamp": iso_millis(start),
                "type": "turn_context",
                "payload": {
                    "cwd": cwd,
                    "approval_policy": "on-request",
                    "model": "synthetic-model",
                },
            },
            ensure_ascii=False,
            separators=(",", ":"),
        ),
    ]
    for message in messages:
        seq = message["seq"]
        role = message["role"]
        block = "input_text" if role == "user" else "output_text"
        lines.append(
            json.dumps(
                {
                    "timestamp": iso_millis(session_timestamp(index, seq)),
                    "type": "response_item",
                    "payload": {
                        "type": "message",
                        "id": f"msg-{stable_hex('codex-msg', index, seq, width=16)}",
                        "role": role,
                        "content": [{"type": block, "text": message["text"]}],
                    },
                },
                ensure_ascii=False,
                separators=(",", ":"),
            )
        )
    return "\n".join(lines) + "\n"


def render_aider(index: int, messages: Sequence[dict[str, Any]]) -> str:
    started = session_timestamp(index, 0).strftime("%Y-%m-%d %H:%M:%S")
    lines = [f"# aider chat started at {started}", ""]
    for message in messages:
        if message["role"] == "user":
            lines.extend([f"#### {message['text']}", ""])
        else:
            lines.extend([message["text"], ""])
    return "\n".join(lines) + "\n"


def render_cline(index: int, messages: Sequence[dict[str, Any]]) -> str:
    rows = []
    for message in messages:
        rows.append(
            {
                "role": message["role"],
                "content": message["text"],
                "timestamp": iso_seconds(session_timestamp(index, message["seq"])),
            }
        )
    body = ",\n".join(
        "  " + json.dumps(row, ensure_ascii=False, separators=(", ", ": ")) for row in rows
    )
    return "[\n" + body + "\n]\n"


def render_hermes(index: int, messages: Sequence[dict[str, Any]]) -> str:
    rows = [
        {
            "role": message["role"],
            "content": message["text"],
            "timestamp": iso_naive(session_timestamp(index, message["seq"])),
        }
        for message in messages
    ]
    document = {
        "session_id": f"hermes-sess-{index:06d}",
        "model": "hermes-synthetic",
        "base_url": "http://localhost:11434",
        "session_start": iso_naive(session_timestamp(index, 0)),
        "last_updated": iso_naive(session_timestamp(index, len(messages))),
        "message_count": len(rows),
        "messages": rows,
    }
    return json.dumps(document, ensure_ascii=False, indent=2) + "\n"


def render_kimi(index: int, messages: Sequence[dict[str, Any]]) -> str:
    lines: list[str] = []
    for message in messages:
        lines.append(
            json.dumps(
                {
                    "type": "context.append_message",
                    "message": {"role": message["role"], "content": message["text"]},
                },
                ensure_ascii=False,
                separators=(",", ":"),
            )
        )
        if message["role"] == "assistant":
            lines.append(
                json.dumps(
                    {
                        "type": "context.append_loop_event",
                        "event": {
                            "type": "step.end",
                            "uuid": f"evt-{stable_hex('kimi-evt', index, message['seq'], width=12)}",
                        },
                    },
                    ensure_ascii=False,
                    separators=(",", ":"),
                )
            )
    return "\n".join(lines) + "\n"


RENDERERS: dict[str, Callable[[int, Sequence[dict[str, Any]]], str]] = {
    "aider": render_aider,
    "claude-code": render_claude_code,
    "cline": render_cline,
    "codex": render_codex,
    "hermes": render_hermes,
    "kimi": render_kimi,
}
# Providers whose format cannot carry a newline inside one message.
SINGLE_LINE_PROVIDERS = frozenset({"aider"})


# ---------------------------------------------------------------------------
# Measurement.
# ---------------------------------------------------------------------------
def cjk_count(text: str) -> int:
    return sum(1 for char in text if "一" <= char <= "鿿")


def quantile(ordered: Sequence[int], fraction: float) -> int:
    """Nearest-rank quantile over an already sorted sequence."""
    if not ordered:
        raise ValueError("cannot take a quantile of an empty sample")
    rank = max(1, math.ceil(fraction * len(ordered)))
    return ordered[rank - 1]


def measure(lengths: list[int], cjk_chars: int, total_chars: int, languages: dict[str, int]) -> dict[str, Any]:
    ordered = sorted(lengths)
    return {
        "message_text_chars": {
            "count": len(ordered),
            "min": ordered[0],
            "p10": quantile(ordered, 0.10),
            "p25": quantile(ordered, 0.25),
            "p50": quantile(ordered, 0.50),
            "p75": quantile(ordered, 0.75),
            "p90": quantile(ordered, 0.90),
            "p99": quantile(ordered, 0.99),
            "max": ordered[-1],
            "mean": round(sum(ordered) / len(ordered), 3),
        },
        "cjk_char_ratio": round(cjk_chars / total_chars, 6) if total_chars else 0.0,
        "total_text_chars": total_chars,
        "language_mix": dict(sorted(languages.items())),
    }


# ---------------------------------------------------------------------------
# Generation.
# ---------------------------------------------------------------------------
def generate_corpus(
    output_dir: Path, sessions: int = DEFAULT_SESSIONS, messages: int = DEFAULT_MESSAGES
) -> dict[str, Any]:
    """Write the corpus and its manifest under ``output_dir``; return manifest."""
    corpus_dir = output_dir / "corpus"
    assignment = provider_assignment(sessions)
    counts = session_message_counts(assignment, messages)

    lengths: list[int] = []
    languages: dict[str, int] = {}
    cjk_chars = 0
    total_chars = 0
    per_provider: dict[str, dict[str, int]] = {}
    written: list[Path] = []

    for index, provider in enumerate(assignment):
        session_messages = build_session_messages(
            index, counts[index], provider in SINGLE_LINE_PROVIDERS
        )
        text = RENDERERS[provider](index, session_messages)
        directory = corpus_dir / provider
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / f"{provider}-s{index:05d}.{PROVIDER_EXTENSION[provider]}"
        path.write_text(text, encoding="utf-8", newline="\n")
        written.append(path)

        stats = per_provider.setdefault(
            provider, {"sessions": 0, "messages": 0, "bytes": 0}
        )
        stats["sessions"] += 1
        stats["messages"] += len(session_messages)
        stats["bytes"] += path.stat().st_size
        for message in session_messages:
            body = message["text"]
            lengths.append(len(body))
            total_chars += len(body)
            cjk_chars += cjk_count(body)
            languages[message["language"]] = languages.get(message["language"], 0) + 1

    manifest = {
        "schema_version": SCHEMA_VERSION,
        "corpus_version": CORPUS_VERSION,
        "seed": SEED,
        "generator": "scripts/evidence/synthetic_corpus.py",
        "provenance": {
            "kind": "deterministic_synthetic",
            "contains_real_transcripts": False,
            "templates": (
                "all prose, code and identifiers are hand-authored inside "
                "scripts/evidence/synthetic_corpus.py; no provider transcript "
                "content, path or identity is copied"
            ),
        },
        "corpus": {
            "session_count": sessions,
            "message_count": sum(counts),
            "file_count": len(written),
            "total_bytes": sum(stats["bytes"] for stats in per_provider.values()),
            "providers": dict(sorted(per_provider.items())),
            "session_messages": {
                "min": min(counts),
                "max": max(counts),
                "mean": round(sum(counts) / len(counts), 3),
            },
            "fixture_hash": normalized_tree_hash(written, corpus_dir),
            "hash_algorithm": "sha256",
            "hash_normalization": "crlf_to_lf",
        },
        "distribution": {
            "session_length_cdf": [list(bucket) for bucket in SESSION_LENGTH_CDF],
            "body_length_cdf": [list(bucket) for bucket in BODY_LENGTH_CDF],
            "message_class_mix": [list(entry) for entry in MESSAGE_CLASS_MIX],
            "language_mix_target": dict(LANGUAGE_MIX),
            "provider_share": dict(PROVIDER_SHARE),
            "anchor": (
                "body length quantiles and language mix are taken from the only "
                "corpus measured inside this repository, the frozen semantic "
                "corpus at scripts/evidence/fixtures/semantic/corpus (n=2000); "
                "the medium/long classes are a deliberate addition because that "
                "corpus carries no tool output or stack traces"
            ),
        },
        "measured": measure(lengths, cjk_chars, total_chars, languages),
    }
    manifest_path = output_dir / "manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    return manifest


def corpus_files(corpus_dir: Path) -> list[Path]:
    return sorted(
        (path for path in corpus_dir.rglob("*") if path.is_file()),
        key=lambda item: item.relative_to(corpus_dir).as_posix(),
    )


def load_frozen_manifest() -> dict[str, Any]:
    """Load the committed manifest that pins the corpus contract."""
    return json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))


def verify_corpus(output_dir: Path) -> dict[str, Any]:
    """Recompute the hash and counts of an on-disk corpus.

    The corpus is checked against the manifest written beside it and, when the
    scale matches, against the frozen committed manifest. A drift in either is
    a problem, never a warning.
    """
    manifest = json.loads((output_dir / "manifest.json").read_text(encoding="utf-8"))
    frozen = load_frozen_manifest()
    corpus_dir = output_dir / "corpus"
    files = corpus_files(corpus_dir)
    expected = manifest["corpus"]
    problems: list[str] = []
    if len(files) != expected["file_count"]:
        problems.append(f"file count {len(files)} != manifest {expected['file_count']}")
    actual_hash = normalized_tree_hash(files, corpus_dir)
    if actual_hash != expected["fixture_hash"]:
        problems.append(f"fixture hash {actual_hash} != manifest {expected['fixture_hash']}")
    same_scale = (
        expected["session_count"] == frozen["corpus"]["session_count"]
        and expected["message_count"] == frozen["corpus"]["message_count"]
    )
    if same_scale and actual_hash != frozen["corpus"]["fixture_hash"]:
        problems.append(
            f"fixture hash {actual_hash} != frozen manifest "
            f"{frozen['corpus']['fixture_hash']}; regenerate or bump corpus_version"
        )
    return {
        "output_dir": str(output_dir),
        "file_count": len(files),
        "fixture_hash": actual_hash,
        "manifest_fixture_hash": expected["fixture_hash"],
        "frozen_manifest_fixture_hash": frozen["corpus"]["fixture_hash"],
        "checked_against_frozen_manifest": same_scale,
        "message_count": expected["message_count"],
        "session_count": expected["session_count"],
        "problems": problems,
    }


# ---------------------------------------------------------------------------
# Ingest verification through the real binary.
# ---------------------------------------------------------------------------
def resolve_binary(workspace: Path, override: str | None) -> Path:
    if override:
        return Path(override).expanduser().resolve()
    name = "agent-session-grep.exe" if os.name == "nt" else "agent-session-grep"
    binary = workspace / "target" / "release" / name
    if not binary.is_file():
        raise SystemExit(
            f"release binary not found at {binary}; build it with "
            "`cargo build --locked --release -p agent-session-grep-cli` "
            "or pass --binary"
        )
    return binary


def run_cli(binary: Path, db: Path, args: Sequence[str]) -> tuple[int, dict[str, Any]]:
    completed = subprocess.run(
        [str(binary), "--db", str(db), "--robot", *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    stdout = completed.stdout.strip()
    if not stdout:
        raise SystemExit(f"{args[0]} produced no stdout (exit {completed.returncode})")
    return completed.returncode, json.loads(stdout.splitlines()[-1])


def chunk_paths(paths: Sequence[Path], budget: int = 24_000) -> list[list[str]]:
    """Split paths into batches whose joined length fits one command line.

    Windows caps a command line near 32 KiB; 5,000 transcript paths blow past
    that in a single `sync`. Same batching precedent as
    ``scripts/evidence/real_data_regression.py::_chunk_sources``.
    """
    batches: list[list[str]] = []
    current: list[str] = []
    used = 0
    for path in paths:
        text = str(path)
        cost = len(text) + 3
        if current and used + cost > budget:
            batches.append(current)
            current, used = [], 0
        current.append(text)
        used += cost
    if current:
        batches.append(current)
    return batches


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def ingest_corpus(
    output_dir: Path, workspace: Path, binary_override: str | None
) -> dict[str, Any]:
    """Sync the whole corpus through the real binary and compare the counts."""
    binary = resolve_binary(workspace, binary_override)
    manifest = json.loads((output_dir / "manifest.json").read_text(encoding="utf-8"))
    files = corpus_files(output_dir / "corpus")
    emitted = skipped = unchanged = 0
    warnings: list[str] = []
    started = time.perf_counter()
    with tempfile.TemporaryDirectory(prefix="synthetic-corpus-ingest-") as scratch:
        db = Path(scratch) / "corpus.db"
        for batch in chunk_paths(files):
            code, envelope = run_cli(binary, db, ["sync", *batch])
            if code != 0 or envelope.get("ok") is not True:
                raise SystemExit(
                    f"sync failed (exit {code}): "
                    f"{envelope.get('error', {}).get('code', 'unknown')} "
                    f"{envelope.get('error', {}).get('message', '')}"
                )
            data = envelope.get("data", {})
            emitted += int(data.get("emitted", 0))
            skipped += int(data.get("skipped", 0))
            unchanged += int(data.get("unchanged", 0))
            warnings.extend(envelope.get("warnings", []))
        sync_seconds = time.perf_counter() - started
        code, status = run_cli(binary, db, ["status"])
        if code != 0 or status.get("ok") is not True:
            raise SystemExit(f"status failed (exit {code})")
        status_data = status.get("data", {})
        db_bytes = sum(
            path.stat().st_size for path in Path(scratch).glob("corpus.db*") if path.is_file()
        )
    expected = manifest["corpus"]["message_count"]
    claims = int(status_data.get("source_placement_claims", 0))
    return {
        # The binary is identified by name and digest, never by absolute path:
        # a local checkout path is machine information the reports must not carry.
        "binary_name": binary.name,
        "binary_sha256": sha256_file(binary),
        "sources": len(files),
        "expected_messages": expected,
        "emitted": emitted,
        "skipped": skipped,
        "unchanged": unchanged,
        "source_placement_claims": claims,
        "placements": int(status_data.get("placements", 0)),
        "catalog_count": int(status_data.get("catalog_count", 0)),
        "sync_seconds": round(sync_seconds, 3),
        "store_bytes": db_bytes,
        "warnings": len(warnings),
        "matches": emitted == expected and skipped == 0 and claims == expected,
    }


# ---------------------------------------------------------------------------
# CLI.
# ---------------------------------------------------------------------------
def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="synthetic_corpus.py",
        description=(
            "Deterministic multi-provider synthetic corpus at release-gate "
            "scale (100,000 messages / ~5,000 sessions / 6 providers)."
        ),
    )
    parser.add_argument(
        "--workspace",
        default=str(Path(__file__).resolve().parents[2]),
        help="repository root (default: inferred from this file)",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    generate = sub.add_parser("generate", help="write the corpus and its manifest")
    generate.add_argument(
        "--output-dir",
        default=None,
        help=(
            "destination directory; the corpus lands in <dir>/corpus "
            f"(default: <workspace>/{DEFAULT_OUTPUT_DIR.as_posix()}, gitignored)"
        ),
    )
    generate.add_argument("--sessions", type=int, default=DEFAULT_SESSIONS)
    generate.add_argument("--messages", type=int, default=DEFAULT_MESSAGES)

    verify = sub.add_parser(
        "verify", help="recheck an on-disk corpus against its manifest and the frozen one"
    )
    verify.add_argument("--output-dir", default=None)

    ingest = sub.add_parser(
        "ingest", help="sync the corpus with the real binary and compare counts"
    )
    ingest.add_argument("--output-dir", default=None)
    ingest.add_argument("--binary", default=None, help="prebuilt release binary to use")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    workspace = Path(args.workspace).expanduser().resolve()
    output_dir = (
        Path(args.output_dir).expanduser().resolve()
        if args.output_dir
        else workspace / DEFAULT_OUTPUT_DIR
    )
    if args.command == "generate":
        output_dir.mkdir(parents=True, exist_ok=True)
        manifest = generate_corpus(output_dir, args.sessions, args.messages)
        print(json.dumps(manifest, ensure_ascii=False, indent=2))
        return 0
    if args.command == "verify":
        result = verify_corpus(output_dir)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 1 if result["problems"] else 0
    result = ingest_corpus(output_dir, workspace, args.binary)
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0 if result["matches"] else 1


if __name__ == "__main__":
    sys.exit(main())
