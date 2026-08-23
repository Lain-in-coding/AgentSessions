#!/usr/bin/env python3
"""Reproduce the growing-source race that M2P-10 tracks.

An agent writing a transcript keeps appending to it while ``sync`` is reading,
so the snapshot verification necessarily sees a length it did not capture. This
harness makes that race deterministic instead of "sometimes on a real machine":
it writes a synthetic transcript large enough that the read overlaps a
background appender, runs the real binary against a throwaway data root, and
reports the resulting exit code and error classification.

Everything here is synthetic. No provider data root is touched, no real
transcript is read, and the temporary HOME is created by the caller. The
transcript body is filler text with fabricated UUIDs, so the report can be
pasted into an issue without leaking anything.

The classification this asserts is the contract in
``schemas/robot/v1/error-catalog.json``: a source that changed under the reader
is ``source_changed`` (exit 5, retryable), because waiting and retrying wins the
race. Anything else — in particular ``provider_error`` (exit 7, not retryable) —
points the operator at "unrecognised format" for what is really a timing
conflict.

**Why this runs several trials.** The drift can surface from two different
places: the pre-commit verification (which always classified correctly) or the
adapter's read (which did not, before the fix). Which one loses the race varies
per run, so a single trial passed against a known-broken binary in roughly half
of measured attempts. One trial therefore proves nothing; every reproduced trial
must match, and a run where the race never happened is reported as such rather
than counted as a pass.

Usage:
    python scripts/evidence/growing_source_repro.py \
        --binary target/release/agent-session-grep \
        [--home <dir>] [--records N] [--trials N] [--json]

Exit codes: 0 every reproduced trial matched the catalog, 1 at least one did not
(or the race never reproduced), 2 usage error.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any, Dict, Optional

#: Contract under test: a source changing under the reader is a retryable race.
EXPECTED_CODE = "source_changed"
EXPECTED_EXIT = 5
EXPECTED_RETRYABLE = True

#: Enough records that reading takes long enough to overlap the appender. Tuned
#: so the base file is a few MB — below that the read finishes before the first
#: append lands and the race simply does not reproduce.
DEFAULT_RECORDS = 4000

#: Trials per run. The race surfaces from either the adapter read or the
#: pre-commit verification, and which one wins varies per run, so one trial can
#: pass against a binary that misclassifies the other path.
DEFAULT_TRIALS = 6

#: Filler width per record. Fabricated content, never real transcript text.
_FILLER = "x" * 400


def _record(index: int) -> str:
    """One synthetic Claude Code style record with a fabricated uuid."""
    return json.dumps(
        {
            "type": "user",
            "uuid": f"aaaa0000-0000-4000-8000-{index:012d}",
            "parentUuid": None,
            "sessionId": "aaaa0000-0000-4000-8000-0000000000ff",
            "timestamp": "2026-01-01T00:00:00.000Z",
            "message": {
                "role": "user",
                "content": f"synthetic record {index} {_FILLER}",
            },
        }
    )


def _write_base(target: pathlib.Path, records: int) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("w", encoding="utf-8", newline="\n") as handle:
        for index in range(records):
            handle.write(_record(index) + "\n")


def _child_env(home: pathlib.Path) -> Dict[str, str]:
    """Point the binary at a throwaway data root, never the operator's own."""
    env = dict(os.environ)
    env["HOME"] = str(home)
    env["USERPROFILE"] = str(home)
    env["LOCALAPPDATA"] = str(home / "AppData" / "Local")
    # An inherited store path would send the run at a real database.
    env.pop("AGENT_SESSION_GREP_DB", None)
    env.pop("ASG_DB", None)
    return env


def _error_frame(output: str) -> Optional[Dict[str, Any]]:
    """First frame carrying an ``error`` object, or None if the run succeeded."""
    for line in output.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            frame = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(frame, dict) and isinstance(frame.get("error"), dict):
            return frame
    return None


def reproduce(binary: str, home: pathlib.Path, records: int) -> Dict[str, Any]:
    """One trial. The caller decides how many trials make a verdict."""
    # Resolve before spawning: the child runs with a rewritten HOME, and on
    # Windows CreateProcess does not resolve a relative path the way a shell does.
    binary_path = str(pathlib.Path(binary).resolve())
    target = home / ".claude" / "projects" / "growing" / "live.jsonl"
    _write_base(target, records)

    stop = threading.Event()

    def keep_appending() -> None:
        batch = 60
        while not stop.is_set():
            with target.open("a", encoding="utf-8", newline="\n") as handle:
                for index in range(batch):
                    handle.write(_record(100_000 + index) + "\n")
                handle.flush()
            time.sleep(0.01)

    writer = threading.Thread(target=keep_appending, daemon=True)
    writer.start()
    try:
        completed = subprocess.run(
            [binary_path, "--robot", "sync", "--discover"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            env=_child_env(home),
            check=False,
        )
    finally:
        stop.set()
        writer.join(timeout=2)

    output = (completed.stdout or "") + (completed.stderr or "")
    frame = _error_frame(output)
    error = (frame or {}).get("error") or {}
    observed_code = error.get("code")
    observed_retryable = error.get("retryable")

    if frame is None:
        # The appender lost the race: the read finished before any append landed.
        # That is not a pass — the race simply did not happen, so the run proves
        # nothing about the classification. Say so rather than reporting green.
        verdict = "not_reproduced"
    elif (
        observed_code == EXPECTED_CODE
        and completed.returncode == EXPECTED_EXIT
        and observed_retryable is EXPECTED_RETRYABLE
    ):
        verdict = "matches_catalog"
    else:
        verdict = "violates_catalog"

    return {
        "kind": "growing-source-repro-trial",
        "verdict": verdict,
        "records": records,
        "expected": {
            "code": EXPECTED_CODE,
            "exit_code": EXPECTED_EXIT,
            "retryable": EXPECTED_RETRYABLE,
        },
        "observed": {
            "code": observed_code,
            "exit_code": completed.returncode,
            "retryable": observed_retryable,
            # The message carries only lengths, never a path or transcript text.
            "message": error.get("message"),
        },
    }


def run_trials(binary: str, home: Optional[str], records: int, trials: int) -> Dict[str, Any]:
    """Run every trial and fail if any reproduced trial misclassified.

    Each trial gets its own throwaway data root, so a store left behind by an
    aborted trial cannot make the next one report ``unchanged``.
    """
    results = []
    for _ in range(trials):
        if home:
            results.append(reproduce(binary, pathlib.Path(home), records))
        else:
            with tempfile.TemporaryDirectory(prefix="asg-growing-repro-") as scratch:
                results.append(reproduce(binary, pathlib.Path(scratch), records))

    reproduced = [r for r in results if r["verdict"] != "not_reproduced"]
    violations = [r for r in results if r["verdict"] == "violates_catalog"]

    if not reproduced:
        verdict = "not_reproduced"
    elif violations:
        verdict = "violates_catalog"
    else:
        verdict = "matches_catalog"

    return {
        "kind": "growing-source-repro",
        "verdict": verdict,
        "records": records,
        "trials": trials,
        "reproduced": len(reproduced),
        "violations": len(violations),
        "expected": {
            "code": EXPECTED_CODE,
            "exit_code": EXPECTED_EXIT,
            "retryable": EXPECTED_RETRYABLE,
        },
        "observations": [r["observed"] for r in results],
    }


def main(argv: Optional[list] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="path to the built binary")
    parser.add_argument(
        "--home",
        default=None,
        help="throwaway HOME to use; a temporary directory is created when omitted",
    )
    parser.add_argument("--records", type=int, default=DEFAULT_RECORDS)
    parser.add_argument(
        "--trials",
        type=int,
        default=DEFAULT_TRIALS,
        help="trials per run; one trial is not enough to judge a race",
    )
    parser.add_argument("--json", action="store_true", help="emit the report as JSON")
    args = parser.parse_args(argv)

    if not pathlib.Path(args.binary).exists():
        print(f"binary not found: {args.binary}", file=sys.stderr)
        return 2
    if args.records < 1:
        print("--records must be positive", file=sys.stderr)
        return 2
    if args.trials < 1:
        print("--trials must be positive", file=sys.stderr)
        return 2

    report = run_trials(args.binary, args.home, args.records, args.trials)

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(
            f"verdict: {report['verdict']} "
            f"({report['reproduced']}/{report['trials']} trials reproduced the race, "
            f"{report['violations']} misclassified)"
        )
        for index, observed in enumerate(report["observations"], start=1):
            print(
                f"  trial {index}: exit {observed['exit_code']} "
                f"code={observed['code']} retryable={observed['retryable']}"
            )
        if report["verdict"] == "not_reproduced":
            print("the appender never won a trial; rerun with a larger --records")

    return 0 if report["verdict"] == "matches_catalog" else 1


if __name__ == "__main__":
    sys.exit(main())
