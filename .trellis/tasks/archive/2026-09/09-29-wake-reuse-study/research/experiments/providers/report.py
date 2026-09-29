#!/usr/bin/env python3
"""Run the synthetic provider probes, seal the evidence, or replay-validate it.

Synthetic-only isolated structure probes. No product adapter, provider source,
home scan, resume claim, canonical identity proof or Wake execution.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys

import cases
from fixtures import FIXTURE_REVISION

WAKE_COMMIT = "71aeca67ec80f8645d1f9d5199290c2c732036ce"
SCHEMA = "wake-reuse-study.providers/v1"


def canonical_hash(value) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True,
                         separators=(",", ":"), allow_nan=False).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def seal(report: dict) -> dict:
    report["integrity_sha256"] = canonical_hash(
        {key: value for key, value in report.items() if key != "integrity_sha256"})
    return report


def expected_digest(specs) -> str:
    return canonical_hash([{"id": s.id, "recipe": s.recipe, "expected": s.expected} for s in specs])


def build_report() -> dict:
    rows = []
    for spec in cases.CASES:
        actual = cases.execute(spec)
        rows.append({"id": spec.id, "recipe": spec.recipe, "expected": spec.expected,
                     "actual": actual, "passed": actual == spec.expected})
    failed = [row["id"] for row in rows if not row["passed"]]
    report = {
        "schema_version": SCHEMA,
        "status": "passed" if not failed else "failed",
        "contains_real_transcripts": False,
        "evidence_status": "isolated_synthetic_probe",
        "certification": "none: this does not certify provider compatibility, resume support or canonical identity",
        "provider_version_evidence": {
            "cursor_ide": "no official versioned storage contract located; pinned Wake implementation plus synthetic probes only",
            "hermes": ("independent snapshot NousResearch/hermes-agent@bac0c45d8593ed9d53a8e3fcacecdc920b71a2c4 "
                       "hermes_state_common.py schema_version 30, sessions/messages columns; see "
                       "../../upstream-evidence.md"),
        },
        "wake_commit": WAKE_COMMIT,
        "fixture_revision": FIXTURE_REVISION,
        "expected_suite_sha256": expected_digest(cases.CASES),
        "python_sqlite_version": __import__("sqlite3").sqlite_version,
        "limitations": [
            "Python sqlite3 version is probe metadata only, not product runtime evidence.",
            "Synthetic schema fixtures are hand-built; they are not provider certification or version coverage.",
            "Association results are observation-only and never authoritative.",
            "Native IDs are preserved exactly in observations but this is not StableId proof.",
            "No resume, source-write, discovery or platform-path behavior is exercised.",
        ],
        "summary": {"case_count": len(rows), "passed": len(rows) - len(failed), "failed": len(failed),
                    "failed_ids": failed},
        "cases": rows,
    }
    return seal(report)


def validate(report: dict) -> None:
    stored = dict(report)
    seal_expected = stored.pop("integrity_sha256", None)
    if seal_expected != canonical_hash(stored):
        raise SystemExit("report integrity hash mismatch")
    fresh = build_report()
    if fresh != report:
        raise SystemExit("report does not replay against the current synthetic suite")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    report = sub.add_parser("report")
    report.add_argument("output")
    report.add_argument("--refuse-overwrite", action="store_true")
    replay = sub.add_parser("validate-report")
    replay.add_argument("report")
    args = parser.parse_args()
    if args.command == "report":
        target = Path(args.output)
        if target.exists():
            raise SystemExit(f"refusing to overwrite: {target}")
        value = build_report()
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"report": str(target), "status": value["status"],
                          "passed": value["summary"]["passed"], "failed": value["summary"]["failed"]}))
        return 0 if value["status"] == "passed" else 1
    value = json.loads(Path(args.report).read_text(encoding="utf-8"))
    validate(value)
    print(json.dumps({"report": args.report, "status": "validated"}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
