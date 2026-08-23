#!/usr/bin/env python3
"""Authorized real-data regression harness.

Drives the real ``agent-session-grep`` binary over transcripts the operator is
authorized to read, and checks that ingestion, context assembly, and index
rebuild hold their invariants on real input rather than only on synthetic
fixtures.

Privacy is a structural property of this script, not an operator promise:
the corpus is ingested into a throwaway temporary data root, and the report
carries aggregate counts and invariant verdicts only. No message text, no
source paths, no provider-native ids, no fingerprints, no usernames, no
hostnames ever reach the report. See
``docs/operations/REAL-DATA-REGRESSION.md``.

Usage:
    python scripts/evidence/real_data_regression.py \
        --binary target/release/agent-session-grep \
        --sources <dir-or-file> [--sources ...] \
        [--out <path>] [--json] [--dry-run]

Exit codes: 0 all invariants passed, 1 at least one failed (the report is
still written), 2 usage error.
"""

from __future__ import annotations

import argparse
import datetime as _datetime
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any, Dict, List, Optional, Sequence, Tuple

REPORT_SCHEMA_VERSION = "1.0"
REPORT_KIND = "real-data-regression"

#: Invariant ids in report order. The set is fixed: a report that does not
#: carry every one of these is incomplete, and the validator says so.
INVARIANT_IDS = (
    "INV-SYNC-OK",
    "INV-NO-PARSE-LOSS",
    "INV-SESSION-PRESENT",
    "INV-CONTEXT-NONEMPTY",
    "INV-SPAN-COVERAGE",
    "INV-REBUILD-STABLE",
    "INV-SOURCES-UNCHANGED",
)

#: Wire-id prefixes of the three catalog entity kinds (RFC-0001).
MESSAGE_PREFIX = "msg_v1_"
SESSION_PREFIX = "ses_v1_"
DOCUMENT_PREFIX = "doc_v1_"

#: Closed set of role keys the report may carry; anything else buckets to
#: "other" so the aggregate key set stays structural (no provider-controlled
#: strings can surface verbatim in the report).
ROLE_ALLOWLIST = frozenset(
    {"user", "assistant", "system", "developer", "tool", "unknown", "other"}
)

#: Page size used when walking the catalog. Paging is driven by the
#: envelope's ``page.next_cursor``; the harness never constructs a cursor.
PAGE_SIZE = 200


class HarnessError(Exception):
    """Usage or environment failure (exit 2). Not an invariant failure."""


# ─── CLI invocation ──────────────────────────────────────────────────────────


def run_cli(binary: str, db: str, args: Sequence[str]) -> Tuple[int, Dict[str, Any]]:
    """Invoke the binary in robot mode and parse its single JSON envelope.

    Returns ``(exit_code, envelope)``. A non-JSON stdout is a protocol
    violation rather than a data problem, so it raises instead of being
    folded into an invariant verdict.
    """
    argv = [binary, "--db", db, "--robot", *args]
    completed = subprocess.run(
        argv,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    stdout = completed.stdout.strip()
    if not stdout:
        raise HarnessError(
            f"{args[0] if args else '<no command>'} produced no stdout "
            f"(exit {completed.returncode})"
        )
    try:
        envelope = json.loads(stdout.splitlines()[-1])
    except json.JSONDecodeError as error:
        raise HarnessError(
            f"{args[0] if args else '<no command>'} stdout is not a JSON "
            f"envelope: {error}"
        ) from error
    return completed.returncode, envelope


#: Max re-attempts for a sync chunk that failed with ``source_changed``
#: (exit 5). Real corpora include transcripts being appended to by a live
#: session; the snapshot check correctly rejects them, and a bounded wait
#: lets the file settle instead of failing the whole run. Any other failure
#: (or a change that persists across retries) is still reported verbatim.
SYNC_CHANGED_RETRIES = 3
SYNC_CHANGED_BACKOFF_S = 3.0


def _chunk_sources(sources: Sequence[str], budget: int = 24_000) -> List[List[str]]:
    """Split sources into batches whose joined length fits one command line.

    Windows caps a command line near 32 KiB, and a real corpus of several
    hundred transcripts blows past that in one `sync`. Batching keeps every
    invocation legal; each batch is still atomic, so a mid-corpus failure
    leaves the store at the last committed generation.
    """
    batches: List[List[str]] = []
    current: List[str] = []
    used = 0
    for path in sources:
        cost = len(path) + 3  # quoting and separator
        if current and used + cost > budget:
            batches.append(current)
            current = []
            used = 0
        current.append(path)
        used += cost
    if current:
        batches.append(current)
    return batches


def binary_version(binary: str) -> str:
    completed = subprocess.run(
        [binary, "--version"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    if completed.returncode != 0:
        raise HarnessError(
            f"{os.path.basename(binary)} --version exited {completed.returncode}"
        )
    return completed.stdout.strip()


def sha256_of(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


# ─── corpus collection ───────────────────────────────────────────────────────


#: Files that are never transcripts, so excluding them is not a coverage gap.
#: Deliberately short: anything not listed here is offered to the tool, which
#: decides by probe. A long exclusion list would recreate the extension filter
#: this function exists to avoid.
#:
#: ``.md`` is deliberately absent — aider stores its transcript as
#: ``.aider.chat.history.md``, so excluding Markdown would reintroduce exactly
#: the blind spot this list is guarding against.
NON_TRANSCRIPT_SUFFIXES = (
    ".png",
    ".jpg",
    ".jpeg",
    ".gif",
    ".webp",
    ".zip",
    ".gz",
    ".tar",
    ".exe",
    ".dll",
    ".pdb",
    ".lock",
)


def collect_sources(roots: Sequence[str]) -> List[str]:
    """Collect candidate transcript files from directories (recursively) and files.

    Returns a sorted list for run-to-run determinism. Paths stay local to
    this process: they are never written to the report.

    Candidacy is deliberately **not** an extension test. This function used to
    keep only ``.jsonl``, which silently dropped every transcript a provider
    stores as whole JSON (hermes, cline), as a SQLite database (opencode,
    cursor) or as Markdown (aider). Measured on the frozen 100k corpus: 3,675
    of 5,000 files were collected and the harness still reported all seven
    invariants green, so a quarter of the corpus was excluded by a report that
    read as full coverage. That is the same defect class the CLI's discovery
    path had (it now nominates candidates by probing a file head rather than by
    extension), and it is worse in a harness whose entire job is to be trusted.

    Non-transcript files are excluded by an explicit short list rather than by
    an allow-list of transcript extensions, so a provider format nobody
    anticipated is offered to the tool instead of being dropped in silence.
    """
    found: List[str] = []
    for root in roots:
        if os.path.isfile(root):
            found.append(os.path.abspath(root))
            continue
        if os.path.isdir(root):
            for dirpath, _dirnames, filenames in os.walk(root):
                for name in filenames:
                    if name.lower().endswith(NON_TRANSCRIPT_SUFFIXES):
                        continue
                    found.append(os.path.abspath(os.path.join(dirpath, name)))
            continue
        raise HarnessError(f"--sources path does not exist: {root}")
    return sorted(set(found))


# ─── pure report construction and validation ─────────────────────────────────


def invariant(id_: str, passed: bool, detail: str) -> Dict[str, Any]:
    """One invariant verdict. ``detail`` must carry aggregate numbers only."""
    return {"id": id_, "passed": bool(passed), "detail": detail}


def _source_integrity_invariant(
    sources: Sequence[str], before: Dict[str, str]
) -> Dict[str, Any]:
    """Compare source checksums without exposing paths or fingerprints."""
    unchanged = 0
    changed = 0
    for path in sources:
        try:
            matches = sha256_of(path) == before[path]
        except (KeyError, OSError):
            matches = False
        if matches:
            unchanged += 1
        else:
            changed += 1
    return invariant(
        "INV-SOURCES-UNCHANGED",
        changed == 0,
        f"{len(sources)} sources checked, {unchanged} unchanged, {changed} changed",
    )


def _is_zero_placement_context(data: Dict[str, Any]) -> bool:
    """Return whether a successful context belongs to a zero-placement Session."""
    session = data.get("session")
    return isinstance(session, dict) and session.get("messages") == []


def build_report(
    *,
    generated_at_utc: str,
    binary_basename: str,
    version: str,
    sha256: str,
    environment: Dict[str, str],
    source_files: int,
    total_bytes: int,
    totals: Dict[str, int],
    role_distribution: Dict[str, int],
    evidence_precision: Dict[str, int],
    invariants: Sequence[Dict[str, Any]],
) -> Dict[str, Any]:
    """Assemble the report. Field set is closed — see the privacy contract."""
    outcome = "passed" if all(entry["passed"] for entry in invariants) else "failed"
    return {
        "schema_version": REPORT_SCHEMA_VERSION,
        "kind": REPORT_KIND,
        "generated_at_utc": generated_at_utc,
        "binary": {
            "path_basename": binary_basename,
            "version": version,
            "sha256": sha256,
        },
        "environment": environment,
        "corpus": {"source_files": source_files, "total_bytes": total_bytes},
        "totals": totals,
        "role_distribution": role_distribution,
        "evidence_precision": evidence_precision,
        "invariants": list(invariants),
        "outcome": outcome,
    }


def validate_report(report: Dict[str, Any]) -> List[str]:
    """Return a list of structural problems; empty means the report is well formed."""
    problems: List[str] = []
    if report.get("schema_version") != REPORT_SCHEMA_VERSION:
        problems.append("schema_version must be " + REPORT_SCHEMA_VERSION)
    if report.get("kind") != REPORT_KIND:
        problems.append("kind must be " + REPORT_KIND)
    for field in (
        "generated_at_utc",
        "binary",
        "environment",
        "corpus",
        "totals",
        "role_distribution",
        "evidence_precision",
        "invariants",
        "outcome",
    ):
        if field not in report:
            problems.append(f"missing field: {field}")
    # 字段集是闭的（build_report 的隐私契约）：未知顶层字段可能携带未脱敏的
    # 敏感数据，必须拒绝而非静默透传。
    closed = {
        "schema_version",
        "kind",
        "generated_at_utc",
        "binary",
        "environment",
        "corpus",
        "totals",
        "role_distribution",
        "evidence_precision",
        "invariants",
        "outcome",
    }
    for key in report:
        if key not in closed:
            problems.append(f"unexpected field: {key}")
    seen = [entry.get("id") for entry in report.get("invariants", [])]
    for id_ in INVARIANT_IDS:
        if id_ not in seen:
            problems.append(f"missing invariant: {id_}")
    if report.get("outcome") not in ("passed", "failed"):
        problems.append("outcome must be passed or failed")
    return problems


def render_markdown(report: Dict[str, Any]) -> str:
    """Human projection of the JSON report. Adds no facts of its own."""
    binary = report["binary"]
    corpus = report["corpus"]
    totals = report["totals"]
    lines = [
        "# Real-data regression report",
        "",
        f"- outcome: **{report['outcome']}**",
        f"- generated (UTC): {report['generated_at_utc']}",
        f"- binary: `{binary['path_basename']}` {binary['version']}",
        f"- binary sha256: `{binary['sha256']}`",
        f"- environment: {report['environment'].get('os', '?')} "
        f"{report['environment'].get('release', '?')}, "
        f"Python {report['environment'].get('python', '?')}",
        "",
        "## Corpus",
        "",
        f"- source files: {corpus['source_files']}",
        f"- total bytes: {corpus['total_bytes']}",
        "",
        "This report contains aggregate counts only. No message text, source",
        "paths, native ids, or fingerprints are recorded.",
        "",
        "## Totals",
        "",
        "| metric | count |",
        "|---|---|",
    ]
    for key in ("messages", "sessions", "documents", "catalog_entities"):
        lines.append(f"| {key} | {totals.get(key, 0)} |")
    lines += ["", "## Role distribution", "", "| role | messages |", "|---|---|"]
    for role, count in sorted(report["role_distribution"].items()):
        lines.append(f"| {role} | {count} |")
    lines += ["", "## Evidence precision", "", "| precision | spans |", "|---|---|"]
    for tier in ("byte", "line", "record", "unknown"):
        lines.append(f"| {tier} | {report['evidence_precision'].get(tier, 0)} |")
    lines += ["", "## Invariants", "", "| id | result | detail |", "|---|---|---|"]
    for entry in report["invariants"]:
        result = "pass" if entry["passed"] else "**FAIL**"
        lines.append(f"| `{entry['id']}` | {result} | {entry['detail']} |")
    lines.append("")
    return "\n".join(lines)


# ─── regression run ──────────────────────────────────────────────────────────


def page_catalog(binary: str, db: str) -> List[Dict[str, Any]]:
    """Walk every catalog entity via ``list``, following ``page.next_cursor``."""
    entries: List[Dict[str, Any]] = []
    cursor: Optional[str] = None
    while True:
        args = ["list", str(PAGE_SIZE), "--max-items", str(PAGE_SIZE)]
        if cursor is not None:
            args += ["--cursor", cursor]
        code, envelope = run_cli(binary, db, args)
        if code not in (0, 10):
            raise HarnessError(
                f"list exited {code}: "
                f"{envelope.get('error', {}).get('code', 'unknown')}"
            )
        entries.extend(envelope["data"]["entries"])
        cursor = envelope.get("page", {}).get("next_cursor")
        if not cursor:
            return entries


def _is_unrecognized_source_refusal(code: int, envelope: Dict[str, Any]) -> bool:
    """Did this chunk fail only because some file is not a transcript?

    Explicit ``sync`` refuses a source no provider recognises, and that refusal
    fails the whole chunk. Matched on the stable canonical code plus the
    documented refusal wording, so an unrelated ``invalid_request`` (a bad flag,
    a malformed id) is still treated as a real failure.
    """
    if code == 0 or envelope.get("ok") is True:
        return False
    error = envelope.get("error")
    if not isinstance(error, dict) or error.get("code") != "invalid_request":
        return False
    return "no provider recognized this source" in str(error.get("message", ""))


def _sync_chunk_individually(
    binary: str, db: str, chunk: Sequence[str]
) -> Tuple[int, int, int, bool]:
    """Sync a chunk one source at a time.

    Returns ``(emitted, skipped, unrecognized, failed)``. ``failed`` is True only
    when a source fails for a reason other than not being a transcript — those
    are real defects and must not be absorbed into the unrecognized count.
    """
    emitted = 0
    skipped = 0
    unrecognized = 0
    for source in chunk:
        code, envelope = run_cli(binary, db, ["sync", source])
        if code == 0 and envelope.get("ok") is True:
            data = envelope.get("data", {})
            emitted += int(data.get("emitted", 0))
            skipped += int(data.get("skipped", 0))
            continue
        if _is_unrecognized_source_refusal(code, envelope):
            unrecognized += 1
            continue
        return emitted, skipped, unrecognized, True
    return emitted, skipped, unrecognized, False


def run_regression(binary: str, sources: Sequence[str]) -> Dict[str, Any]:
    """Execute the full regression against a throwaway data root."""
    version = binary_version(binary)
    sha256 = sha256_of(binary)
    total_bytes = sum(os.path.getsize(path) for path in sources)
    source_checksums_before = {path: sha256_of(path) for path in sources}

    workdir = tempfile.mkdtemp(prefix="agent-session-grep-regression-")
    db = os.path.join(workdir, "regression.db")
    invariants: List[Dict[str, Any]] = []
    totals = {"messages": 0, "sessions": 0, "documents": 0, "catalog_entities": 0}
    role_distribution: Dict[str, int] = {}
    evidence_precision = {"byte": 0, "line": 0, "record": 0, "unknown": 0}

    try:
        # 1) sync the corpus. A real corpus is hundreds of files, whose paths
        # together exceed the OS command-line limit (32 KiB on Windows), so the
        # source list is chunked. Each chunk is atomic on its own; the corpus as
        # a whole is not one transaction, which is fine here because this is a
        # source-read-only regression against a throwaway store, guarded by
        # INV-SOURCES-UNCHANGED after the full run. `sync` is idempotent, so a
        # retry of a chunk changes nothing.
        sync_ok = True
        reported_emitted = 0
        reported_skipped = 0
        unrecognized_sources = 0
        last_code = 0
        last_ok: Any = True
        for chunk in _chunk_sources(sources):
            last_code, sync = run_cli(binary, db, ["sync", *chunk])
            last_ok = sync.get("ok")
            if (last_code == 5 and not last_ok) or (
                last_code == 0 and last_ok is not True
            ):
                # A source in the chunk was written during the sync (a live
                # session appending). Wait for it to settle and retry; a
                # persistent change is a real failure, not a skip.
                for _attempt in range(SYNC_CHANGED_RETRIES):
                    time.sleep(SYNC_CHANGED_BACKOFF_S)
                    last_code, sync = run_cli(binary, db, ["sync", *chunk])
                    last_ok = sync.get("ok")
                    if last_code == 0 and last_ok is True:
                        break
            if _is_unrecognized_source_refusal(last_code, sync):
                # A provider data root holds far more than transcripts: pointing
                # `--sources` at one collects icons, state files and caches, and
                # explicit `sync` refuses a file no provider recognises, failing
                # the whole chunk. Measured on this machine: an antigravity root
                # yields 1,419 files of which 40 are transcripts, so the run died
                # with invalid_request and reported zero coverage — the same
                # "reads as full coverage while excluding most of the corpus"
                # defect this harness exists to avoid, just inverted.
                #
                # Fall back to one source at a time for this chunk so the
                # recognised transcripts are still exercised, and count the
                # refusals rather than hiding them.
                recovered_emitted, recovered_skipped, refused, failed = (
                    _sync_chunk_individually(binary, db, chunk)
                )
                reported_emitted += recovered_emitted
                reported_skipped += recovered_skipped
                unrecognized_sources += refused
                if failed:
                    sync_ok = False
                    break
                continue
            if last_code != 0 or last_ok is not True:
                sync_ok = False
                break
            reported_emitted += int(sync.get("data", {}).get("emitted", 0))
            reported_skipped += int(sync.get("data", {}).get("skipped", 0))
        code = last_code
        invariants.append(
            invariant(
                "INV-SYNC-OK",
                sync_ok and reported_emitted > 0,
                f"exit {code}, ok={last_ok}, "
                f"{len(sources)} sources, {reported_emitted} emitted records, "
                f"{reported_skipped} skipped, "
                f"{unrecognized_sources} not recognised by any provider",
            )
        )
        if not sync_ok:
            # Nothing downstream is meaningful without a committed corpus;
            # report the remaining invariants as failed rather than skipped.
            for id_ in INVARIANT_IDS[1:-1]:
                invariants.append(invariant(id_, False, "not evaluated: sync failed"))
            invariants.append(
                _source_integrity_invariant(sources, source_checksums_before)
            )
            return _finish(
                binary,
                version,
                sha256,
                len(sources),
                total_bytes,
                totals,
                role_distribution,
                evidence_precision,
                invariants,
            )

        # 2) catalog census by entity kind plus aggregate relation counts.
        status_code, status = run_cli(binary, db, ["status"])
        if status_code != 0 or status.get("ok") is not True:
            raise HarnessError(
                f"status exited {status_code}: "
                f"{status.get('error', {}).get('code', 'unknown')}"
            )
        source_placement_claims = int(
            status.get("data", {}).get("source_placement_claims", 0)
        )
        entries = page_catalog(binary, db)
        message_ids = [
            entry["id"] for entry in entries if entry["id"].startswith(MESSAGE_PREFIX)
        ]
        session_ids = [
            entry["id"] for entry in entries if entry["id"].startswith(SESSION_PREFIX)
        ]
        document_ids = [
            entry["id"] for entry in entries if entry["id"].startswith(DOCUMENT_PREFIX)
        ]
        totals = {
            "messages": len(message_ids),
            "sessions": len(session_ids),
            "documents": len(document_ids),
            "catalog_entities": len(entries),
        }
        invariants.append(
            invariant(
                "INV-NO-PARSE-LOSS",
                reported_emitted == source_placement_claims
                and reported_skipped == 0,
                f"provider emitted {reported_emitted}, persisted "
                f"{source_placement_claims} source-placement claims, "
                f"skipped {reported_skipped}; catalog holds "
                f"{len(message_ids)} de-duplicated {MESSAGE_PREFIX} entities",
            )
        )
        invariants.append(
            invariant(
                "INV-SESSION-PRESENT",
                1 <= len(session_ids) <= len(sources),
                f"{len(session_ids)} sessions for {len(sources)} source files",
            )
        )

        # 3) context assembly per session, accumulating aggregate facts only.
        context_failures = 0
        internal_errors = 0
        zero_placement_sessions = 0
        unexpected_empty_sessions = 0
        for session_id in session_ids:
            code, envelope = run_cli(
                binary, db, ["context", session_id, "--policy", "mainline"]
            )
            if code not in (0, 10):
                context_failures += 1
                if envelope.get("error", {}).get("code") == "internal":
                    internal_errors += 1
                continue
            data = envelope["data"]
            if not data.get("messages"):
                if _is_zero_placement_context(data):
                    zero_placement_sessions += 1
                else:
                    unexpected_empty_sessions += 1
            for _id, payload in (
                (item["id"], item["payload"]) for item in data.get("messages", [])
            ):
                # Role keys are bucketed to a closed allowlist so the report's
                # aggregate key set is structural, not data-dependent: an
                # unexpected role value must never surface verbatim in the
                # report.
                role = "unknown"
                if isinstance(payload, dict):
                    role = str(payload.get("role") or "unknown")
                if role not in ROLE_ALLOWLIST:
                    role = "other"
                role_distribution[role] = role_distribution.get(role, 0) + 1
            for span in data.get("evidence", []):
                tier = str(span.get("precision", "unknown"))
                if tier not in evidence_precision:
                    evidence_precision[tier] = 0
                evidence_precision[tier] += 1
        invariants.append(
            invariant(
                "INV-CONTEXT-NONEMPTY",
                context_failures == 0 and unexpected_empty_sessions == 0,
                f"{len(session_ids)} sessions, {context_failures} failed, "
                f"{zero_placement_sessions} zero-placement, "
                f"{unexpected_empty_sessions} unexpectedly empty, "
                f"{internal_errors} internal errors",
            )
        )

        # A provider whose declared `source_span` is Unsupported cannot produce
        # a byte-precise span, so demanding byte precision from every span makes
        # the invariant fail on a correct implementation.
        #
        # Measured on the frozen 100k corpus: 875 of 43,894 spans were not
        # byte-precise, and those 875 are exactly the cline (450) + hermes (425)
        # documents — both declared `source_span: Unsupported` in
        # `ProviderCapabilityMatrix::current()`, because a whole-JSON document
        # has no in-file byte range to report.
        #
        # `Precision::Unknown` is the *documented, correct* value for that case:
        # `application/src/evidence.rs:12-13` states v1 emits `Byte` when a
        # placement has an exact byte span and `Unknown` when the provider could
        # not attribute one contiguous range. So `unknown` here means honestly
        # absent, not un-established, and cannot be treated as a failure.
        #
        # What the invariant can still assert without lying: every span carries
        # one of the tiers the contract defines, and at least one span is
        # byte-precise (otherwise the byte-span machinery was never exercised
        # and a total regression to unknown would pass silently).
        #
        # This only became visible once `collect_sources` stopped filtering on
        # `.jsonl`; while it did, every span came from a byte-precise provider
        # and the stricter form looked satisfiable.
        known_tiers = {"byte", "line", "record", "unknown"}
        span_total = sum(evidence_precision.values())
        span_byte = evidence_precision.get("byte", 0)
        span_unknown = evidence_precision.get("unknown", 0)
        undefined_tiers = {
            tier: count
            for tier, count in evidence_precision.items()
            if tier not in known_tiers and count > 0
        }
        invariants.append(
            invariant(
                "INV-SPAN-COVERAGE",
                span_total > 0 and span_byte > 0 and not undefined_tiers,
                f"{span_byte}/{span_total} spans byte-precise; "
                f"{span_unknown} unknown (providers declaring source_span "
                f"unsupported cannot attribute a byte range); "
                f"undefined tiers {sorted(undefined_tiers) or 'none'}",
            )
        )

        # 4) rebuild must be a faithful reprojection: same catalog census,
        #    same hit counts for sampled terms drawn from real ids.
        sample_terms = _sample_terms(binary, db, message_ids)
        before = {term: _hit_count(binary, db, term) for term in sample_terms}
        code, rebuild = run_cli(binary, db, ["index", "rebuild"])
        rebuild_ok = code == 0 and rebuild.get("ok") is True
        after_entries = page_catalog(binary, db) if rebuild_ok else []
        after = (
            {term: _hit_count(binary, db, term) for term in sample_terms}
            if rebuild_ok
            else {}
        )
        # Blind-pass guards: (1) compare the entity id SET, not just the
        # count — rebuild dropping one entity and adding another must fail;
        # (2) a failed search (-1) is a mismatch, never a match (-1 == -1);
        # (3) an empty sample means the search half was never exercised —
        # treat that as a failure of the invariant, not a pass.
        before_ids = {entry["id"] for entry in entries}
        after_ids = {entry["id"] for entry in after_entries}
        ids_match = before_ids == after_ids
        searches_match = sample_terms != [] and after == before and all(
            v >= 0 for v in after.values()
        )
        invariants.append(
            invariant(
                "INV-REBUILD-STABLE",
                rebuild_ok and ids_match and searches_match,
                f"rebuild exit {code}, catalog {len(entries)} -> "
                f"{len(after_entries)} ({'ids match' if ids_match else 'IDS DIVERGED'}), "
                f"{len(sample_terms)} sampled terms "
                f"{'match' if searches_match else 'DIVERGED/EMPTY/FAILED'}",
            )
        )
        invariants.append(_source_integrity_invariant(sources, source_checksums_before))

        return _finish(
            binary,
            version,
            sha256,
            len(sources),
            total_bytes,
            totals,
            role_distribution,
            evidence_precision,
            invariants,
        )
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


def _sample_terms(binary: str, db: str, message_ids: Sequence[str]) -> List[str]:
    """Pick a few search terms from real stored payloads.

    Terms come from the corpus, so they stay local: they are used for hit
    counting and never written to the report.
    """
    terms: List[str] = []
    for message_id in list(message_ids)[:5]:
        code, envelope = run_cli(binary, db, ["show", message_id])
        if code != 0:
            continue
        entity = envelope.get("data", {}).get("entity")
        if not isinstance(entity, dict):
            continue
        text = str(entity.get("text") or "")
        for word in text.split():
            token = "".join(ch for ch in word if ch.isalnum())
            if len(token) >= 4:
                terms.append(token)
                break
    return terms


def _hit_count(binary: str, db: str, term: str) -> int:
    code, envelope = run_cli(binary, db, ["search", term])
    if code not in (0, 10):
        return -1
    return len(envelope.get("data", {}).get("hits", []))


def _finish(
    binary: str,
    version: str,
    sha256: str,
    source_files: int,
    total_bytes: int,
    totals: Dict[str, int],
    role_distribution: Dict[str, int],
    evidence_precision: Dict[str, int],
    invariants: Sequence[Dict[str, Any]],
) -> Dict[str, Any]:
    return build_report(
        generated_at_utc=_datetime.datetime.now(_datetime.timezone.utc).strftime(
            "%Y-%m-%dT%H:%M:%SZ"
        ),
        binary_basename=os.path.basename(binary),
        version=version,
        sha256=sha256,
        environment={
            # OS family and release only: never the hostname or the user.
            "os": platform.system(),
            "release": platform.release(),
            "python": platform.python_version(),
        },
        source_files=source_files,
        total_bytes=total_bytes,
        totals=totals,
        role_distribution=role_distribution,
        evidence_precision=evidence_precision,
        invariants=invariants,
    )


# ─── entry point ─────────────────────────────────────────────────────────────


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        description="Authorized real-data regression over local transcripts.",
    )
    parser.add_argument("--binary", required=True, help="agent-session-grep binary to drive")
    parser.add_argument(
        "--sources",
        action="append",
        default=[],
        metavar="DIR_OR_FILE",
        help="transcript directory (recursive) or transcript file; repeatable",
    )
    parser.add_argument("--out", default=None, help="report path (JSON)")
    parser.add_argument(
        "--json", action="store_true", help="also print the JSON report to stdout"
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="print the plan and write nothing"
    )
    args = parser.parse_args(argv)

    try:
        if not os.path.isfile(args.binary):
            raise HarnessError(f"--binary does not exist: {args.binary}")
        # Resolve to absolute: `isfile` accepts a cwd-relative path but Windows
        # CreateProcess does not resolve one the way a shell does, so a relative
        # --binary otherwise failed with a bare WinError 2 traceback.
        binary = os.path.abspath(args.binary)
        if not args.sources:
            raise HarnessError("--sources is required (directory or transcript file)")
        sources = collect_sources(args.sources)
        if not sources:
            raise HarnessError("no candidate transcript files found under the given --sources")

        out = args.out or os.path.join(
            "evidence-output", "real-data-regression.json"
        )
        if args.dry_run:
            print(f"regression: dry run: no file written")
            print(f"regression: would exercise {os.path.basename(binary)}")
            print(f"regression: would ingest {len(sources)} candidate source files")
            print(f"regression: would write {out}")
            return 0

        report = run_regression(binary, sources)
        problems = validate_report(report)
        if problems:
            raise HarnessError("generated report is malformed: " + "; ".join(problems))

        os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
        with open(out, "w", encoding="utf-8", newline="\n") as handle:
            json.dump(report, handle, indent=2, ensure_ascii=False)
            handle.write("\n")
        markdown_path = os.path.splitext(out)[0] + ".md"
        with open(markdown_path, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(render_markdown(report))

        if args.json:
            print(json.dumps(report, indent=2, ensure_ascii=False))
        failed = [
            entry["id"] for entry in report["invariants"] if not entry["passed"]
        ]
        print(f"regression: outcome {report['outcome']}")
        print(f"regression: report  {out}")
        print(f"regression: summary {markdown_path}")
        for entry in report["invariants"]:
            status = "pass" if entry["passed"] else "FAIL"
            print(f"regression: {status}  {entry['id']}: {entry['detail']}")
        return 0 if not failed else 1
    except HarnessError as error:
        print(f"regression: error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
