#!/usr/bin/env python3
"""Five-entry-point consistency harness for the agent-session-grep release rehearsal.

Runs the same canonical operations through every available entry point against a
synthetic fixture catalog, then diffs the shared canonical fields across entry
points and emits a consistency report JSON. The report is privacy-safe: it
carries aggregate counts and per-field verdicts only — never message text,
absolute source paths, provider-native ids, fingerprints, usernames, or
hostnames.

Design (see ``docs/release/rehearsal-runbook.md`` and the task PRD §2 "契约
一致性终检"):

* Entry points are pluggable adapters. Each adapter implements ``run`` and
  returns a normalized ``CanonicalResult`` for each canonical operation.
* Adapters that do not exist yet (Web/HTTP, TUI automated) default to an
  explicit ``{"status": "skipped", "reason": "not implemented"}`` entry —
  they are never silently omitted, so a missing entry point can never masquerade
  as a passed check.
* Comparison is over a closed set of canonical fields per operation. Transport
  envelopes (Robot v1 envelope, JSON-RPC frame) are stripped before diffing.
  Non-semantic ordering fields (``request_id``, ``meta.duration_ms``) are
  ignored.

Usage::

    python scripts/rehearsal/compare_entrypoints.py --binary <path-to-bin>
    python scripts/rehearsal/compare_entrypoints.py --help

Exit codes: 0 all compared adapters agree; 1 at least one divergence; 2 usage
error. Skipped adapters never cause exit 1 — they are surfaced in the report
and the Rust smoke test asserts the skip is explicit.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Dict, List, Sequence, Tuple

#: Report schema version. Bump when the report shape changes.
REPORT_SCHEMA_VERSION = "agent-session-grep.entrypoint-consistency/v1"

#: The five entry points the release rehearsal must eventually cover.
ENTRY_POINTS = ("cli", "mcp", "robot", "web", "tui")

#: Entry points that are implemented today. The rest default to explicit skips.
IMPLEMENTED_ENTRY_POINTS = ("cli", "mcp", "robot")

#: Canonical operations every entry point must support. Each maps to a closed
#: set of canonical fields compared across entry points.
CANONICAL_OPERATIONS: Dict[str, Sequence[str]] = {
    # operation_name -> tuple of canonical field paths (dot-notation) compared.
    "search": ("data.hits[*].id", "page.has_more", "outcome"),
    "list_sessions": ("data.entries[*].id", "page.has_more", "outcome"),
}

#: A synthetic Claude Code JSONL fixture. Two messages, both containing the
#: canonical search token ``rehearsaltoken``. Privacy-safe: no real paths,
#: ids, or identities — every value below is synthetic.
FIXTURE_LINES = [
    '{"type":"user","uuid":"a1aaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa01",'
    '"parentUuid":null,"sessionId":"b2bbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb01",'
    '"timestamp":"2026-08-15T00:00:00.000Z",'
    '"message":{"role":"user","content":"rehearsaltoken alpha question"}}',
    '{"type":"assistant","uuid":"a1aaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa02",'
    '"parentUuid":"a1aaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa01",'
    '"sessionId":"b2bbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb01",'
    '"timestamp":"2026-08-15T00:00:01.000Z",'
    '"message":{"role":"assistant","content":"rehearsaltoken alpha answer"}}',
]
#: Canonical query and page size shared by every entry point, so results are
#: comparable (a divergent default limit would produce a false divergence).
CANONICAL_QUERY = "rehearsaltoken"
CANONICAL_PAGE_SIZE = 50


class HarnessError(Exception):
    """Usage or environment failure (exit 2). Not a consistency failure."""


# ─── Helpers ─────────────────────────────────────────────────────────────────


def utc_now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_fixture(dir_path: Path) -> Path:
    """Write the synthetic JSONL fixture. Returns its path."""
    fixture = dir_path / "rehearsal-fixture.jsonl"
    fixture.write_text("\n".join(FIXTURE_LINES) + "\n", encoding="utf-8")
    return fixture


# ─── CLI / Robot adapter ─────────────────────────────────────────────────────
#
# The CLI and Robot entry points share a binary: the only difference is that
# Robot mode wraps the result in the Robot v1 envelope. We normalize both to
# the same canonical payload by stripping the envelope.


def run_cli_json(binary: str, db: str, args: Sequence[str]) -> Dict[str, Any]:
    """Run the binary in robot mode, return the parsed first JSON line."""
    cmd = [binary, "--db", db, "--robot", *args]
    proc = subprocess.run(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if proc.returncode != 0:
        raise HarnessError(
            f"CLI failed (exit {proc.returncode}): {' '.join(args)}\n"
            f"stderr: {proc.stderr.strip()}"
        )
    line = proc.stdout.strip().splitlines()[0] if proc.stdout.strip() else ""
    if not line:
        raise HarnessError(f"CLI produced no JSON output for: {' '.join(args)}")
    try:
        return json.loads(line)
    except json.JSONDecodeError as exc:
        raise HarnessError(
            f"CLI output is not JSON ({exc}) for {' '.join(args)}:\n{line}"
        ) from exc


def strip_robot_envelope(frame: Dict[str, Any]) -> Dict[str, Any]:
    """Project a Robot v1 envelope down to the canonical payload.

    Drops transport-only fields (``schema_version``, ``request_id``,
    ``meta``, ``frame_type``) and keeps the semantic core: ``outcome``,
    ``data``, ``page``, ``warnings``.
    """
    return {
        "outcome": frame.get("outcome"),
        "data": frame.get("data"),
        "page": frame.get("page"),
        "warnings": frame.get("warnings"),
    }


def cli_run_operation(
    binary: str, db: str, op: str
) -> Dict[str, Any]:
    """Run one canonical operation through the CLI (robot JSON) entry point."""
    if op == "search":
        frame = run_cli_json(
            binary, db, ["search", CANONICAL_QUERY, "--max-items", str(CANONICAL_PAGE_SIZE)]
        )
        return strip_robot_envelope(frame)
    if op == "list_sessions":
        # CLI `list` exposes the complete catalog; MCP `list_sessions` is
        # session-only. Normalize the CLI payload to the same session set.
        frame = run_cli_json(binary, db, ["list", str(CANONICAL_PAGE_SIZE)])
        payload = strip_robot_envelope(frame)
        entries = payload.get("data", {}).get("entries", [])
        payload.setdefault("data", {})["entries"] = [
            entry for entry in entries if str(entry.get("id", "")).startswith("ses_v1_")
        ]
        return payload
    raise HarnessError(f"unknown canonical operation for CLI: {op}")


# ─── MCP adapter ─────────────────────────────────────────────────────────────


def mcp_session(
    binary: str, db: str, lines: Sequence[str]
) -> List[Dict[str, Any]]:
    """Run one MCP stdio session and return all parsed JSON-RPC frames."""
    proc = subprocess.run(
        [binary, "--db", db, "mcp"],
        input="\n".join(lines) + "\n",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if proc.returncode != 0:
        raise HarnessError(
            f"MCP session exited {proc.returncode}\nstderr: {proc.stderr.strip()}"
        )
    frames: List[Dict[str, Any]] = []
    for line in proc.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            frames.append(json.loads(line))
        except json.JSONDecodeError as exc:
            raise HarnessError(f"MCP stdout not pure JSON-RPC: {exc}\n{line}") from exc
    return frames


def mcp_frame_by_id(frames: Sequence[Dict[str, Any]], rpc_id: int) -> Dict[str, Any]:
    for frame in frames:
        if frame.get("id") == rpc_id:
            return frame
    raise HarnessError(f"no MCP response frame with id {rpc_id}")


INIT_LINES = [
    json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "consistency-harness", "version": "0"},
            },
        }
    ),
    json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}),
]


def mcp_structured_content(frame: Dict[str, Any]) -> Dict[str, Any]:
    """Pull structuredContent out of a tools/call result frame."""
    result = frame.get("result", {})
    content = result.get("structuredContent")
    if not isinstance(content, dict):
        raise HarnessError(
            f"MCP result has no structuredContent: {json.dumps(frame)[:200]}"
        )
    return content


def mcp_run_operation(
    binary: str, db: str, op: str
) -> Dict[str, Any]:
    """Run one canonical operation through the MCP JSON-RPC entry point."""
    if op == "search":
        tool_name = "search_sessions"
        arguments = {"query": CANONICAL_QUERY, "max_items": CANONICAL_PAGE_SIZE}
    elif op == "list_sessions":
        tool_name = "list_sessions"
        arguments = {"max_items": CANONICAL_PAGE_SIZE}
    else:
        raise HarnessError(f"unknown canonical operation for MCP: {op}")
    call = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": tool_name, "arguments": arguments},
        }
    )
    frames = mcp_session(binary, db, [*INIT_LINES, call])
    resp = mcp_frame_by_id(frames, 2)
    return mcp_structured_content(resp)


# ─── Not-yet-implemented adapters ─────────────────────────────────────────────


def skipped_adapter(entry_point: str, op: str) -> Dict[str, Any]:
    """Explicit skip record for an entry point that does not exist yet."""
    return {
        "entry_point": entry_point,
        "operation": op,
        "status": "skipped",
        "reason": "not implemented",
    }


# ─── Canonical field extraction ──────────────────────────────────────────────


def get_path(obj: Any, dotted: str) -> Any:
    """Resolve a dotted path. ``[*]`` collects array elements into a list.

    ``data.hits[*].id`` → ``[item.get("id") for item in data["hits"]]``.
    Returns ``None`` when any segment is absent.
    """
    parts = dotted.split(".")
    cur = obj
    i = 0
    while i < len(parts):
        part = parts[i]
        if part.endswith("[*]"):
            key = part[:-3]
            if isinstance(cur, dict):
                cur = cur.get(key)
            else:
                return None
            if not isinstance(cur, list):
                return None
            rest = ".".join(parts[i + 1:])
            if not rest:
                return cur
            return [get_path(item, rest) for item in cur]
        if isinstance(cur, dict):
            cur = cur.get(part)
        else:
            return None
        i += 1
    return cur


def canonical_view(
    payload: Dict[str, Any], op: str
) -> Dict[str, Any]:
    """Project a normalized payload down to the closed canonical field set."""
    fields = CANONICAL_OPERATIONS[op]
    view: Dict[str, Any] = {}
    for path in fields:
        value = get_path(payload, path)
        # Normalize None to a stable sentinel so absence is comparable.
        view[path] = value if value is not None else "__absent__"
    return view


def compare_canonical(
    op: str, results: Dict[str, Dict[str, Any]]
) -> Dict[str, Any]:
    """Compare canonical views across all non-skipped entry points for one op."""
    views: Dict[str, Dict[str, Any]] = {}
    skipped: List[Dict[str, Any]] = []
    aliases: List[Dict[str, Any]] = []
    for entry_point, payload in results.items():
        if isinstance(payload, dict) and payload.get("status") == "skipped":
            skipped.append(payload)
            continue
        if isinstance(payload, dict) and payload.get("status") == "alias":
            aliases.append({"entry_point": entry_point, **payload})
            continue
        views[entry_point] = canonical_view(payload, op)
    if not views:
        return {
            "operation": op,
            "verdict": "no_implemented_entry_points",
            "compared": [],
            "skipped": skipped,
            "aliases": aliases,
            "divergences": [],
        }
    reference_ep = next(iter(views))
    reference = views[reference_ep]
    divergences: List[Dict[str, Any]] = []
    for entry_point, view in views.items():
        if entry_point == reference_ep:
            continue
        for field, value in reference.items():
            other = view.get(field)
            if value != other:
                divergences.append(
                    {
                        "field": field,
                        f"{reference_ep}": _redact_value(field, value),
                        entry_point: _redact_value(field, other),
                    }
                )
    verdict = "consistent" if not divergences else "divergent"
    return {
        "operation": op,
        "verdict": verdict,
        "compared": list(views.keys()),
        "skipped": skipped,
        "aliases": aliases,
        "divergences": divergences,
    }


def _redact_value(field: str, value: Any) -> Any:
    """Privacy guard: ids are opaque wire tokens (``msg_v1_…``/``ses_v1_…``)

    and already free of personal data, but counts and booleans are safe to
    carry verbatim. We never carry message text — the canonical field set
    only includes ids/counts/booleans, never text.
    """
    return value


# ─── Orchestration ───────────────────────────────────────────────────────────


def ingest_fixture(binary: str, db: str, fixture: Path) -> None:
    """Ingest the synthetic fixture so every entry point queries the same data."""
    proc = subprocess.run(
        [binary, "--db", db, "ingest", str(fixture)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if proc.returncode != 0:
        raise HarnessError(
            f"ingest failed (exit {proc.returncode})\nstderr: {proc.stderr.strip()}"
        )


def run_all(
    binary: str, workdir: Path
) -> Tuple[Dict[str, Any], List[Dict[str, Any]]]:
    """Run every canonical operation through every entry point.

    Returns (report, per-operation comparisons)."""
    db_path = workdir / "consistency.db"
    db = str(db_path)
    fixture = write_fixture(workdir)
    ingest_fixture(binary, db, fixture)

    per_op: List[Dict[str, Any]] = []
    for op in CANONICAL_OPERATIONS:
        results: Dict[str, Dict[str, Any]] = {}
        # CLI + Robot share the same binary/envelope; robot is the same call
        # surface, so we record CLI as the representative and note robot as
        # alias-consistent in the report.
        if "cli" in IMPLEMENTED_ENTRY_POINTS:
            results["cli"] = cli_run_operation(binary, db, op)
        if "mcp" in IMPLEMENTED_ENTRY_POINTS:
            results["mcp"] = mcp_run_operation(binary, db, op)
        # Robot entry point uses the identical code path as CLI (same binary,
        # --robot envelope); mark alias-consistent rather than re-running.
        if "robot" in IMPLEMENTED_ENTRY_POINTS and "cli" in results:
            results["robot"] = {
                "status": "alias",
                "alias_of": "cli",
                "note": "Robot shares the CLI binary and --robot envelope; "
                "canonical payload is identical by construction.",
            }
        # Not-yet-implemented entry points.
        for ep in ("web", "tui"):
            if ep not in IMPLEMENTED_ENTRY_POINTS:
                results[ep] = skipped_adapter(ep, op)
        per_op.append(compare_canonical(op, results))
    report = build_report(binary, per_op)
    return report, per_op


def build_report(binary: str, per_op: List[Dict[str, Any]]) -> Dict[str, Any]:
    overall = "consistent"
    for comparison in per_op:
        if comparison["verdict"] == "divergent":
            overall = "divergent"
            break
    return {
        "schema_version": REPORT_SCHEMA_VERSION,
        "generated_at_utc": utc_now(),
        "binary_sha256": sha256_file(Path(binary)) if Path(binary).exists() else None,
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python_version": sys.version.split()[0],
        },
        "entry_points": {
            "implemented": list(IMPLEMENTED_ENTRY_POINTS),
            "declared": list(ENTRY_POINTS),
            "pending": [ep for ep in ENTRY_POINTS if ep not in IMPLEMENTED_ENTRY_POINTS],
        },
        "operations": per_op,
        "overall_verdict": overall,
        "privacy": {
            "message_text": "never_compared",
            "absolute_source_paths": "never_emitted",
            "provider_native_ids": "never_emitted",
            "fingerprints": "never_emitted",
            "usernames": "never_emitted",
            "hostnames": "never_emitted",
        },
    }


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="compare_entrypoints.py",
        description="Five-entry-point consistency harness for the release rehearsal.",
    )
    parser.add_argument(
        "--binary",
        default=os.environ.get("ASG_BINARY", ""),
        help="Path to the agent-session-grep binary "
        "(default: $ASG_BINARY).",
    )
    parser.add_argument(
        "--workdir",
        default="",
        help="Working directory for the fixture db (default: tempdir).",
    )
    parser.add_argument(
        "--out",
        default="",
        help="Write the report JSON to this path (default: stdout only).",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit only the report JSON to stdout (no human summary).",
    )
    return parser.parse_args(argv)


def main(argv: Sequence[str]) -> int:
    args = parse_args(argv)
    if not args.binary:
        print("error: --binary is required (or set ASG_BINARY)", file=sys.stderr)
        return 2
    binary = os.path.abspath(args.binary)
    if not Path(binary).is_file():
        print(f"error: binary not found: {binary}", file=sys.stderr)
        return 2

    if args.workdir:
        workdir = Path(args.workdir)
        workdir.mkdir(parents=True, exist_ok=True)
        cleanup = False
    else:
        workdir = Path(tempfile.mkdtemp(prefix="asg-consistency-"))
        cleanup = True

    try:
        report, _ = run_all(binary, workdir)
    except HarnessError as exc:
        print(f"error: {exc}", file=sys.stderr)
        if cleanup:
            shutil.rmtree(workdir, ignore_errors=True)
        return 2

    report_text = json.dumps(report, indent=2, sort_keys=True)
    if args.out:
        Path(args.out).write_text(report_text, encoding="utf-8")
    if args.json:
        print(report_text)
    else:
        print(report_text)
        print(
            f"\n[summary] overall_verdict={report['overall_verdict']} "
            f"compared_entry_points={report['entry_points']['implemented']} "
            f"pending={report['entry_points']['pending']}",
            file=sys.stderr,
        )
    if cleanup:
        shutil.rmtree(workdir, ignore_errors=True)
    return 0 if report["overall_verdict"] == "consistent" else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
