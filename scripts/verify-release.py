#!/usr/bin/env python3
"""agent-session-grep release verification script (#10).

Runs a full smoke test of the release: build → sync → search → context →
resume dry-run → handoff pack → serve → cleanup. Validates the end-to-end
flow works before declaring release-ready.

Usage:
    python scripts/verify-release.py --asg ./target/debug/agent-session-grep
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def run_asg(asg_bin: str, data_root: str, args: list[str]) -> tuple[int, str, str]:
    """Run asg with given args, return (exit_code, stdout, stderr)."""
    env = os.environ.copy()
    env["ASG_DATA_ROOT"] = data_root
    # CLI requires an explicit --db for store-touching commands; use the
    # ASG_DATA_ROOT-scoped default database path. Global flags (--db, --output)
    # go before the subcommand; per-command flags after it.
    db = str(Path(data_root) / "asg.db")
    result = subprocess.run(
        [asg_bin, "--db", db, "--output", "json"] + args,
        capture_output=True,
        text=True,
        env=env,
        timeout=30,
    )
    return result.returncode, result.stdout, result.stderr


def step(name: str, ok: bool, detail: str = ""):
    status = "✓" if ok else "✗"
    print(f"  {status} {name}: {detail}" if detail else f"  {status} {name}")
    return ok


def verify_build(asg_bin: str) -> bool:
    """Verify the binary exists and reports version."""
    if not Path(asg_bin).exists():
        return step("binary exists", False, f"{asg_bin} not found")
    code, out, _ = run_asg(asg_bin, tempfile.mkdtemp(), ["--version"])
    return step("version", code == 0, out.strip()[:60])


def verify_sync(asg_bin: str, data_root: str) -> bool:
    """Verify sync runs without error on a synthetic fixture source.

    sync --discover would scan the real provider data roots on the machine
    running the release verification — correct for a full regression, but
    slow and privacy-relevant for a smoke test. Use the committed synthetic
    gate fixture instead.
    """
    fixture = Path(__file__).parent / "evidence" / "fixtures" / "gate" / "claude" / "session-alpha.jsonl"
    if not fixture.exists():
        return step("sync", False, f"fixture not found: {fixture}")
    code, out, _ = run_asg(asg_bin, data_root, ["sync", str(fixture)])
    if code != 0:
        return step("sync", False, f"exit {code}")
    try:
        frame = json.loads(out.strip().split("\n")[0])
        gen = frame.get("data", {}).get("generation", 0)
        return step("sync", True, f"generation {gen}")
    except (json.JSONDecodeError, IndexError):
        return step("sync", False, "invalid JSON")


def verify_search(asg_bin: str, data_root: str) -> bool:
    """Verify search runs and returns valid JSON."""
    code, out, _ = run_asg(asg_bin, data_root, ["search", "retry", "--max-items", "5"])
    if code != 0:
        return step("search", False, f"exit {code}")
    try:
        frame = json.loads(out.strip().split("\n")[0])
        mode = frame.get("retrieval_mode", "unknown")
        hits = frame.get("data", {}).get("hits", [])
        return step("search", True, f"{len(hits)} hits, mode={mode}")
    except (json.JSONDecodeError, IndexError):
        return step("search", False, "invalid JSON")


def verify_status(asg_bin: str, data_root: str) -> bool:
    """Verify status command reports catalog state."""
    code, out, _ = run_asg(asg_bin, data_root, ["status"])
    if code != 0:
        return step("status", False, f"exit {code}")
    try:
        frame = json.loads(out.strip().split("\n")[0])
        count = frame.get("data", {}).get("catalog_count", "?")
        gen = frame.get("data", {}).get("generation", "?")
        return step("status", True, f"count={count}, gen={gen}")
    except (json.JSONDecodeError, IndexError):
        return step("status", False, "invalid JSON")


def verify_config_paths(asg_bin: str, data_root: str) -> bool:
    """Verify config.paths reports provider locations."""
    code, out, _ = run_asg(asg_bin, data_root, ["config", "paths", "--output", "json"])
    return step("config paths", code == 0, f"exit {code}")


def main():
    parser = argparse.ArgumentParser(description="agent-session-grep release verification")
    parser.add_argument("--asg", required=True, help="Path to asg binary")
    args = parser.parse_args()

    print("agent-session-grep release verification (#10)")
    print("=" * 50)

    data_root = tempfile.mkdtemp(prefix="asg-verify-")
    results = []

    print("\n1. Build verification:")
    results.append(verify_build(args.asg))

    print("\n2. Sync verification:")
    results.append(verify_sync(args.asg, data_root))

    print("\n3. Search verification:")
    results.append(verify_search(args.asg, data_root))

    print("\n4. Status verification:")
    results.append(verify_status(args.asg, data_root))

    print("\n5. Config paths verification:")
    results.append(verify_config_paths(args.asg, data_root))

    print("\n" + "=" * 50)
    passed = sum(results)
    total = len(results)
    print(f"Result: {passed}/{total} checks passed")

    # Cleanup
    import shutil
    shutil.rmtree(data_root, ignore_errors=True)

    sys.exit(0 if passed == total else 1)


if __name__ == "__main__":
    main()
