#!/usr/bin/env python3
"""Validate the Wake reuse study as one coherent, synthetic-only evidence set.

`--mode source` checks provenance, license and the reuse matrix. `--mode full`
additionally requires every experiment report to exist and be complete; it
never relabels incomplete evidence. No network, no Wake execution, no product
mutation, and no home-directory scanning.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

HERE = Path(__file__).resolve().parent
RESEARCH = HERE
WAKE_COMMIT = "71aeca67ec80f8645d1f9d5199290c2c732036ce"
BASELINE_COMMIT = "2b8f89562e43bd1f68ad3c8e5cf8ccc9281f9af2"
FULL_SCALES = [10000, 100000, 1000000]
TIERS = {"direct-copy", "adapt", "idea-only", "reject"}
RESULT_FILES = {
    "snippet": "results/snippet.json",
    "providers": "results/providers.json",
    "search-micro-no-cache": "results/search-micro-full-no-cache.json",
    "search-micro-cache": "results/search-micro-full-cache.json",
    "baseline": "results/search-baseline-full.json",
}
HOME_PATH_PATTERNS = (
    re.compile(r"[A-Za-z]:\\+Users\\+[^\\\s\"']+", re.IGNORECASE),
    re.compile(r"/(?:Users|home)/[A-Za-z0-9._-]+"),
)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def canonical_hash(value) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True,
                         separators=(",", ":"), allow_nan=False).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def load(path: Path) -> dict:
    def reject(value):
        raise ValueError(f"non-finite JSON constant in {path.name}: {value}")
    return json.loads(path.read_text(encoding="utf-8"), parse_constant=reject)


def verify_sealed(report: dict, label: str) -> None:
    stored = {k: v for k, v in report.items() if k != "integrity_sha256"}
    require(report.get("integrity_sha256") == canonical_hash(stored), f"{label}: integrity hash mismatch")


def verify_sources(sources: dict, wake_checkout: Path | None = None) -> None:
    require(sources.get("commit") == WAKE_COMMIT, "sources.json is not pinned to the studied Wake commit")
    require(sources.get("baseline_commit") == BASELINE_COMMIT, "baseline commit changed")
    require(sources.get("license") == "MIT", "Wake license is not recorded as MIT")
    require("Corey Chiu" in sources.get("copyright", ""), "Wake copyright holder missing")
    require(sources.get("contains_real_transcripts") is False, "sources.json must declare synthetic-only")
    files = sources.get("files")
    require(isinstance(files, list) and len(files) >= 20, "pinned source file list is too small")
    paths = set()
    for entry in files:
        require(entry["path"] not in paths, f"duplicate source path {entry['path']}")
        paths.add(entry["path"])
        require(WAKE_COMMIT in entry["url"], f"source URL not pinned: {entry['path']}")
        require(re.fullmatch(r"[0-9a-f]{64}", entry["sha256"] or ""), f"bad sha256: {entry['path']}")
        require(type(entry["line_count"]) is int and entry["line_count"] >= 1, f"bad line count: {entry['path']}")
        if wake_checkout is not None:
            local = wake_checkout / entry["path"]
            require(local.is_file(), f"pinned file missing in checkout: {entry['path']}")
            require(hashlib.sha256(local.read_bytes()).hexdigest() == entry["sha256"],
                    f"checkout hash mismatch: {entry['path']}")
            blob = subprocess.run(["git", "-C", str(wake_checkout), "rev-parse", f"HEAD:{entry['path']}"],
                                  capture_output=True, text=True, check=True).stdout.strip()
            require(blob == entry["git_blob"], f"checkout blob mismatch: {entry['path']}")


def verify_matrix(matrix: dict, sources: dict) -> None:
    require(matrix.get("wake_commit") == WAKE_COMMIT, "matrix Wake commit changed")
    require(matrix.get("baseline_commit") == BASELINE_COMMIT, "matrix baseline changed")
    require(matrix.get("contains_real_transcripts") is False, "matrix must declare synthetic-only")
    require(matrix.get("direct_copy_approved") == [], "direct copy must not be approved by this study")
    indexed = {entry["path"]: entry for entry in sources["files"]}
    entries = matrix.get("entries", [])
    require(len(entries) >= 15, "reuse matrix thinned below the studied surface")
    seen = set()
    for entry in entries:
        require(entry["id"] not in seen, f"duplicate matrix id {entry['id']}")
        seen.add(entry["id"])
        require(entry["classification"] in TIERS, f"invalid tier: {entry['id']}")
        require(entry.get("product_adoption_approved") is False, f"unapproved adoption claim: {entry['id']}")
        require(isinstance(entry.get("required_before_product_adoption"), list) and
                entry["required_before_product_adoption"], f"missing adoption gates: {entry['id']}")
        for ref in entry["source"]:
            require(ref["path"] in indexed, f"unknown source file in {entry['id']}: {ref['path']}")
            limit = indexed[ref["path"]]["line_count"]
            start, end = ref["lines"]
            require(1 <= start <= end <= limit, f"line range out of bounds: {entry['id']} {ref['path']}")
    tiers = {entry["classification"] for entry in entries}
    require({"adapt", "idea-only", "reject"} <= tiers, "matrix lost required classification examples")


def verify_environment(environment: dict) -> None:
    for key in ("os", "target_triple", "cpu", "ram_gb", "disk", "filesystem", "antivirus_state"):
        require(key in environment and environment[key] not in (None, "", "not_recorded"),
                f"environment missing {key}")
    require(environment.get("contains_real_transcripts") is False, "environment must declare synthetic-only")


def verify_results(mode: str) -> list[str]:
    checked = []
    missing = [name for name, relative in RESULT_FILES.items() if not (RESEARCH / relative).is_file()]
    if mode == "source":
        return checked
    require(not missing, f"full mode requires experiment reports: {', '.join(sorted(missing))}")
    snippet = load(RESEARCH / RESULT_FILES["snippet"])
    require(snippet.get("status") == "passed" and snippet.get("contains_real_transcripts") is False,
            "snippet report is not a passed synthetic run")
    require(snippet.get("performance_claims") is False and snippet.get("product_integration") is False,
            "snippet report claims performance/product integration")
    require(snippet.get("summary", {}).get("failed") == 0, "snippet report has failures")
    checked.append("snippet")
    providers = load(RESEARCH / RESULT_FILES["providers"])
    verify_sealed(providers, "providers")
    require(providers.get("status") == "passed" and providers.get("contains_real_transcripts") is False,
            "providers report is not a passed synthetic run")
    require(providers["summary"]["failed"] == 0 and providers["summary"]["case_count"] >= 30,
            "providers suite incomplete")
    checked.append("providers")
    for name, feature in (("search-micro-no-cache", "no-cache"), ("search-micro-cache", "cache")):
        report = load(RESEARCH / RESULT_FILES[name])
        verify_sealed(report, name)
        require(report.get("status") == "complete", f"{name}: run is not complete")
        require(report.get("feature") == feature, f"{name}: feature mismatch")
        require(report.get("contains_real_transcripts") is False, f"{name}: not synthetic-only")
        require(report.get("requested_scales") == FULL_SCALES, f"{name}: full scales not requested")
        require([row["messages"] for row in report["scales"]] == FULL_SCALES
                and all(row["status"] == "complete" for row in report["scales"]), f"{name}: scale rows incomplete")
        checked.append(name)
    baseline = load(RESEARCH / RESULT_FILES["baseline"])
    verify_sealed(baseline, "baseline")
    require(baseline.get("status") == "complete" and baseline.get("contains_real_transcripts") is False,
            "baseline is not a complete synthetic run")
    require(baseline.get("requested_scales") == FULL_SCALES, "baseline full scales not requested")
    require(all(row["status"] == "complete" for row in baseline["scales"]), "baseline scale rows incomplete")
    checked.append("baseline")
    return checked


def privacy_scan() -> None:
    hits = []
    for path in sorted(RESEARCH.rglob("*")):
        if not path.is_file() or path.suffix not in {".md", ".json", ".py", ".rs", ".toml"}:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for pattern in HOME_PATH_PATTERNS:
            for match in pattern.findall(text):
                hits.append(f"{path.relative_to(RESEARCH)}: {match}")
    require(not hits, "possible machine-home paths in tracked research: " + "; ".join(hits[:5]))


def verify_product_scope(workspace: Path) -> None:
    result = subprocess.run(
        ["git", "diff", "--quiet", BASELINE_COMMIT, "--", "Cargo.toml", "Cargo.lock", "crates",
         "schemas", "scripts", "docs"],
        cwd=workspace, capture_output=True, timeout=60)
    require(result.returncode == 0, "product-scope files differ from the pinned baseline")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("source", "full"), default="full")
    parser.add_argument("--wake-checkout", help="explicit pinned Wake checkout for hash verification")
    parser.add_argument("--workspace", help="explicit product workspace for scope verification")
    args = parser.parse_args()
    verify_sources(load(RESEARCH / "sources.json"),
                   Path(args.wake_checkout) if args.wake_checkout else None)
    verify_environment(load(RESEARCH / "environment.json"))
    verify_matrix(load(RESEARCH / "reuse-matrix.json"), load(RESEARCH / "sources.json"))
    checked = verify_results(args.mode)
    privacy_scan()
    if args.workspace:
        verify_product_scope(Path(args.workspace))
    print(json.dumps({"status": "validated", "mode": args.mode, "result_reports": checked}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
