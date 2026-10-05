#!/usr/bin/env python3
"""Validate real Robot 1.1 frames using only the checked-in schema.

Test dependency: python -m pip install jsonschema==4.26.0
Usage: python scripts/verify-robot-schema.py --asg <built-binary>

This is a synthetic, offline contract gate, not a release or maturity promotion.
Diagnostic frames are schema-tested fixtures only: the CLI does not emit them.
"""
from __future__ import annotations

import argparse
from collections import Counter
from importlib.metadata import version
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from jsonschema import Draft202012Validator
from referencing import Registry, Resource
from referencing.exceptions import NoSuchResource

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "schemas/robot/v1.1/envelope.schema.json"
VALIDATOR_VERSION = "4.26.0"


class VerificationError(Exception):
    """A missing prerequisite, invalid frame or failed synthetic scenario."""


def deny_retrieval(uri: str) -> Resource:
    raise NoSuchResource(ref=uri)


def load_validator(path: Path = SCHEMA) -> Draft202012Validator:
    if version("jsonschema") != VALIDATOR_VERSION:
        raise VerificationError(f"test gate requires jsonschema=={VALIDATOR_VERSION}")
    schema = json.loads(path.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    registry = Registry(retrieve=deny_retrieval).with_resource(
        schema["$id"], Resource.from_contents(schema)
    )
    return Draft202012Validator(schema, registry=registry)


def reject_constant(value: str) -> None:
    raise ValueError(f"non-JSON numeric constant: {value}")


def validate_output(
    validator: Draft202012Validator, output: str, request_id: str, outcome: str, command: str
) -> list[dict]:
    frames = []
    for ordinal, line in enumerate(output.split("\n"), start=1):
        if not line.strip():
            continue
        try:
            frame = json.loads(line, parse_constant=reject_constant)
        except ValueError as error:
            raise VerificationError(f"stdout line {ordinal} is not JSON") from error
        errors = list(validator.iter_errors(frame))
        if errors:
            # Do not print frame values or the local fixture/catalog path.
            raise VerificationError(f"stdout frame {ordinal} violates Robot 1.1 schema")
        if frame["command"] != command:
            raise VerificationError(f"stdout frame {ordinal} changed expected command {command}")
        if frame["request_id"] != request_id:
            raise VerificationError(f"stdout frame {ordinal} changed request correlation")
        frames.append(frame)
    if not frames or frames[-1]["frame_type"] not in {"response", "error"}:
        raise VerificationError("stdout must end with a terminal response/error frame")
    if any(frame["frame_type"] != "progress" for frame in frames[:-1]):
        raise VerificationError("only progress may precede the single terminal frame")
    if frames[-1]["outcome"] != outcome:
        raise VerificationError(f"terminal outcome must be {outcome}")
    return frames


def isolated_environment(root: Path) -> dict[str, str]:
    env = {key: value for key, value in os.environ.items() if not key.startswith("ASG_")}
    env.update(ASG_DATA_ROOT=str(root), ASG_CLOCK_MS="1787616000000", ASG_CURRENT_REPO="")
    for key in (
        "HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA",
        "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME",
    ):
        home = root / key.lower()
        home.mkdir()
        env[key] = str(home)
    return env


def verify(binary: Path) -> dict:
    binary = binary.expanduser().resolve()
    if not binary.is_file():
        raise VerificationError("explicit --asg binary does not exist; build it first")
    validator = load_validator()
    totals: Counter = Counter()
    outcomes: set[str] = set()
    cases = 0
    with tempfile.TemporaryDirectory(prefix="asg-schema-") as directory:
        root = Path(directory)
        env = isolated_environment(root)
        fixture = root / "session.jsonl"
        shutil.copyfile(ROOT / "scripts/evidence/fixtures/gate/claude/session-alpha.jsonl", fixture)
        # These are valid JSON string characters, not JSONL record delimiters.
        unicode_text = "gateprobe before\u0085between\u2028middle\u2029after"
        with fixture.open("a", encoding="utf-8", newline="\n") as stream:
            stream.write(json.dumps({
                "type": "user", "uuid": "00000001-0000-4000-8000-000000000005",
                "parentUuid": "00000001-0000-4000-8000-000000000004",
                "sessionId": "gate-alpha-0001", "timestamp": "2026-01-01T00:00:04.000Z",
                "message": {"role": "user", "content": unicode_text},
            }, ensure_ascii=False) + "\n")

        def run(name: str, args: list[str], *, command: str, outcome: str = "success", mode: str = "jsonl") -> dict:
            nonlocal cases
            request_id = "schema-" + name
            result = subprocess.run(
                [str(binary), "--offline", "--db", str(root / "catalog.db"),
                 "--output", mode, "--request-id", request_id, *args],
                capture_output=True, text=True, encoding="utf-8", errors="strict",
                env=env, cwd=root, timeout=30,
            )
            expected_exit = {"success": 0, "partial": 10, "failure": 2}[outcome]
            if result.returncode != expected_exit:
                raise VerificationError(f"{name}: exit {result.returncode}, expected {expected_exit}")
            try:
                frames = validate_output(validator, result.stdout, request_id, outcome, command)
            except VerificationError as error:
                raise VerificationError(f"{name}: {error}") from error
            if mode == "json" and len(frames) != 1:
                raise VerificationError("JSON output must contain exactly one frame")
            totals.update(frame["frame_type"] for frame in frames)
            outcomes.add(outcome)
            cases += 1
            return frames[-1]

        run("version", ["--version"], command="version", mode="json")
        run("sync", ["sync", str(fixture)], command="sync")
        search = run("search", ["search", "gateprobe", "--provider", "claude"], command="search")
        hits = search["data"]["hits"]
        if not hits or not hits[0].get("session_id"):
            raise VerificationError("synthetic search must produce a session-bound hit")
        if not any(hit.get("text") == unicode_text for hit in hits):
            raise VerificationError("real search must preserve the Unicode separator fixture")
        message_id, session_id = hits[0]["id"], hits[0]["session_id"]
        run("message", ["get", message_id], command="get", mode="json")
        for level in ("raw", "talks", "sessions"):
            run("context-" + level, ["context", session_id, "--policy", "full", "--level", level], command="context")
        run("partial", ["context", session_id, "--policy", "full", "--max-messages", "1"], command="context", outcome="partial")
        run("handoff", ["handoff", "gateprobe"], command="handoff", mode="json")
        run("invalid-budget", ["search", "gateprobe", "--max-bytes", "1"], command="search", outcome="failure")
    if not all(totals[kind] for kind in ("response", "error", "progress")):
        raise VerificationError("real response, error and progress coverage is required")
    if outcomes != {"success", "partial", "failure"}:
        raise VerificationError("all terminal outcomes must be exercised")
    return {"schema_version": "1.1", "cases": cases, "frames": dict(totals),
            "outcomes": sorted(outcomes), "diagnostic": "schema-fixture-only"}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--asg", type=Path, required=True, help="explicit current-build binary")
    args = parser.parse_args(argv)
    try:
        report = verify(args.asg)
    except (VerificationError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Robot schema gate failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, ensure_ascii=True, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
