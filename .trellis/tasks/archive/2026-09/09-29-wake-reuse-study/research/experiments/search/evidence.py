"""Shared, offline-only evidence plumbing for the two search runners.

Import the existing benchmark's statistics, hashing, synthetic record, and
memory helpers from an EXPLICIT workspace. Do not call its discovery/run path.
"""
from __future__ import annotations

import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import time
try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10: Cargo.lock fallback below
    tomllib = None  # type: ignore[assignment]
from typing import Any

BASELINE_COMMIT = "2b8f89562e43bd1f68ad3c8e5cf8ccc9281f9af2"
WAKE_COMMIT = "71aeca67ec80f8645d1f9d5199290c2c732036ce"
ROOT = Path(__file__).resolve().parent
FIXTURE_PATH = ROOT / "fixtures" / "beacons.json"
HASH_METHOD = "sha256(concat(u64le(id),u64le(utf8_bytes),utf8_text)), id=1..N"
PROFILES = {"smoke": {"startup": 3, "search": 5, "noop": 1},
            "full": {"startup": 20, "search": 100, "noop": 3}}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def absolute_path(value: str | Path, label: str) -> Path:
    path = Path(value)
    require(path.is_absolute(), f"{label} must be an explicit absolute path (no ~ expansion)")
    return path.resolve()


def load_helpers(workspace: Path):
    path = workspace / "scripts/evidence/core_beta_benchmark.py"
    spec = importlib.util.spec_from_file_location("wake_study_core_benchmark", path)
    require(spec is not None and spec.loader is not None, "benchmark helpers not found")
    module = importlib.util.module_from_spec(spec)
    old = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = old
    return module


def canonical_hash(value: Any) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True,
                         separators=(",", ":"), allow_nan=False).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def seal(report: dict) -> dict:
    report["integrity_sha256"] = canonical_hash({k: v for k, v in report.items() if k != "integrity_sha256"})
    return report


def verify_seal(report: dict) -> None:
    expected = canonical_hash({k: v for k, v in report.items() if k != "integrity_sha256"})
    require(report.get("integrity_sha256") == expected, "report integrity hash mismatch")


def read_json(path: Path) -> Any:
    def reject_constant(value: str):
        raise ValueError(f"nonfinite JSON constant: {value}")
    return json.loads(path.read_text(encoding="utf-8"), parse_constant=reject_constant)


def write_report(path: Path, report: dict) -> None:
    # Refuse overwrite, including a previous incomplete run. No scratch cleanup.
    with path.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(seal(report), handle, ensure_ascii=False, indent=2, allow_nan=False)
        handle.write("\n")


def digest(value: Any, label: str) -> None:
    require(isinstance(value, str) and len(value) == 64 and
            all(c in "0123456789abcdef" for c in value), f"invalid SHA-256: {label}")


def finite(value: Any, label: str, positive: bool = False) -> None:
    require(type(value) in (float, int) and math.isfinite(value) and
            (value > 0 if positive else value >= 0), f"invalid finite duration/count: {label}")


def load_environment(path: Path) -> dict:
    value = read_json(path)
    for key in ("os", "target_triple", "cpu", "ram_gb", "disk", "filesystem", "antivirus_state"):
        require(key in value and value[key] not in (None, "", "not_recorded"), f"environment missing {key}")
    finite(value["ram_gb"], "ram_gb", positive=True)
    require(value.get("contains_real_transcripts") is False, "environment must declare synthetic-only")
    return value


def check_workspace(workspace: Path, expected: str) -> str:
    require(expected == BASELINE_COMMIT, "study baseline commit must stay pinned")
    result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=workspace,
                            capture_output=True, text=True, timeout=15, check=True)
    commit = result.stdout.strip()
    require(commit == expected, "workspace HEAD is not the pinned product baseline")
    subprocess.run(["git", "diff", "--quiet", expected, "--", "Cargo.toml", "Cargo.lock", "crates",
                    "scripts/evidence/core_beta_benchmark.py"], cwd=workspace,
                   capture_output=True, timeout=15, check=True)
    return commit


def artifact(path: Path, helpers) -> dict:
    require(path.is_file(), "binary/artifact file is missing")
    return {"name": path.name, "sha256": helpers.sha256_file(path), "bytes": path.stat().st_size}


def source_provenance(workspace: Path, helpers) -> dict:
    files = sorted([*ROOT.glob("*.py"), *ROOT.glob("src/*.rs"), ROOT / "Cargo.toml",
                    ROOT / "Cargo.lock", FIXTURE_PATH], key=lambda p: p.as_posix())
    return {
        "baseline_commit": BASELINE_COMMIT, "wake_commit": WAKE_COMMIT,
        "synthetic_only": True,
        "experiment_files": [{"path": p.relative_to(ROOT).as_posix(), "sha256": helpers.sha256_file(p)}
                             for p in files],
        "stats_helper": {"path": "scripts/evidence/core_beta_benchmark.py",
                         "sha256": helpers.sha256_file(workspace / "scripts/evidence/core_beta_benchmark.py")},
        "cjk_source": {"path": "crates/agent-session-grep-application/src/cjk.rs",
                       "sha256": helpers.sha256_file(workspace / "crates/agent-session-grep-application/src/cjk.rs")},
    }


def load_fixture() -> dict:
    return read_json(FIXTURE_PATH)


def text_for(fixture: dict, index: int) -> str:
    minimum = len(fixture["beacons"]) * fixture["repeat"]
    if index <= minimum:
        template, copy = divmod(index - 1, fixture["repeat"])
        return f"synthetic beacon {template:02d} copy {copy} {fixture['beacons'][template]['text']}"
    noise = ((index * 6364136223846793005 + 1442695040888963407) & ((1 << 64) - 1)) % 1000003
    return f"synthetic filler {index:08d} quiet amber stone meadow packet {noise:07d}"


def update_data_hash(hasher, index: int, text: str) -> None:
    encoded = text.encode("utf-8")
    hasher.update(index.to_bytes(8, "little"))
    hasher.update(len(encoded).to_bytes(8, "little"))
    hasher.update(encoded)


def corpus_hash(messages: int) -> str:
    fixture = load_fixture()
    hasher = hashlib.sha256()
    for index in range(1, messages + 1):
        update_data_hash(hasher, index, text_for(fixture, index))
    return hasher.hexdigest()


def qrels(fixture: dict, query_id: str) -> list[int]:
    return [index * fixture["repeat"] + copy + 1
            for index, beacon in enumerate(fixture["beacons"])
            if query_id in beacon["relevant_for"] for copy in range(fixture["repeat"])]


def scales(value: str, profile: str) -> list[int]:
    result = [int(part) for part in value.split(",")]
    require(bool(result) and len(result) == len(set(result)), "scales must be nonempty and unique")
    require(all(96 <= n <= 1000000 for n in result), "scales must be between 96 and 1000000")
    require(result == sorted(result), "run scales in increasing order")
    if profile == "smoke":
        require(all(n <= 1000 for n in result), "smoke is bounded to 1000 messages")
    return result


def redact(text: str, roots: dict[str, Path]) -> str:
    replacements = []
    for label, path in roots.items():
        for spelling in {str(path), path.as_posix(), str(path).replace("/", "\\")}:
            replacements.extend([(spelling, f"<{label}>"),
                                 (json.dumps(spelling)[1:-1], f"<{label}>")])
    for before, after in sorted(replacements, key=lambda pair: len(pair[0]), reverse=True):
        text = text.replace(before, after)
    return text


def remaining(deadline: float) -> float:
    return max(0.0, deadline - time.monotonic())


def run_process(command: list[str], *, workspace: Path, scratch: Path, label: str,
                timeout: float, deadline: float, helpers, roots: dict[str, Path],
                max_output_bytes: int = 16 * 1024 * 1024, preview: bool = True) -> tuple[dict, str]:
    """Capture bounded logs on disk; kill ONLY this directly-owned subprocess.

    The tested CLI and Rust probe do not launch descendants. No shell is used.
    Timeout includes process spawn/wait; the per-scale deadline clips it.
    """
    finite(timeout, "timeout", positive=True)
    event: dict[str, Any] = {"label": label, "status": "not_started", "exit_code": None,
                            "timeout_seconds": min(timeout, remaining(deadline)),
                            "duration_ms": 0.0, "peak_rss_mb_raw_samples": []}
    if event["timeout_seconds"] <= 0:
        event["reason"] = "per_scale_deadline_exhausted"
        return event, ""
    started = time.monotonic()
    started_perf = time.perf_counter()
    stdout_path, stderr_path = scratch / f"{label}.stdout", scratch / f"{label}.stderr"
    process = None
    status = "ok"
    memory = []
    next_memory = started
    try:
        with stdout_path.open("xb") as out, stderr_path.open("xb") as err:
            process = subprocess.Popen(command, cwd=workspace, stdout=out, stderr=err, shell=False)
            while True:
                now = time.monotonic()
                if now >= next_memory:
                    # Existing macOS helper runs ps without a timeout: omit RSS
                    # there rather than weakening this runner's finite boundary.
                    peak = helpers.peak_working_set_bytes(process) if sys.platform != "darwin" else None
                    if peak is not None:
                        memory.append(helpers.rounded(peak / (1024 * 1024)))
                    next_memory = now + 0.1
                if stdout_path.stat().st_size + stderr_path.stat().st_size > max_output_bytes:
                    status = "output_limit"
                    process.kill()
                    break
                if now - started >= event["timeout_seconds"]:
                    status = "timeout"
                    process.kill()
                    break
                try:
                    process.wait(timeout=min(0.01, event["timeout_seconds"] - (now - started)))
                    break
                except subprocess.TimeoutExpired:
                    pass
            process.wait(timeout=5)
            event["exit_code"] = process.returncode
            if status == "ok" and process.returncode != 0:
                status = "exit_error"
            peak = helpers.peak_working_set_bytes(process) if sys.platform != "darwin" else None
            if peak is not None:
                memory.append(helpers.rounded(peak / (1024 * 1024)))
    except OSError as error:
        status = "spawn_error"
        event["reason"] = redact(str(error), roots)
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=5)
    event["duration_ms"] = helpers.rounded((time.perf_counter() - started_perf) * 1000)
    event["status"] = status
    event["peak_rss_mb_raw_samples"] = memory
    event["peak_rss_mb"] = max(memory) if memory else None
    text = ""
    for kind, path in (("stdout", stdout_path), ("stderr", stderr_path)):
        if not path.is_file():
            continue
        record = {"path": path.relative_to(scratch).as_posix(), "bytes": path.stat().st_size,
                  "sha256": helpers.sha256_file(path)}
        raw = path.read_bytes() if record["bytes"] <= max_output_bytes else path.open("rb").read(max_output_bytes)
        try:
            decoded = raw.decode("utf-8", errors="strict")
        except UnicodeDecodeError:
            decoded = raw.decode("utf-8", errors="replace")
            if event["status"] == "ok":
                event["status"] = "decode_error"
        if preview:
            record["preview"] = redact(decoded[:2048], roots)
            record["preview_truncated"] = len(decoded) > 2048 or record["bytes"] > len(raw)
        event[kind] = record
        if kind == "stdout":
            text = decoded
    if sum(event.get(k, {}).get("bytes", 0) for k in ("stdout", "stderr")) > max_output_bytes:
        event["status"] = "output_limit"
    return event, text


def make_metric(events: list[dict], required: int, helpers) -> dict:
    values = [float(e["duration_ms"]) for e in events if e["status"] == "ok"]
    return {"unit": "ms", "required_sample_count": required, "raw_samples": values,
            "actual_sample_count": len(values),
            "status": "complete" if len(events) == required and len(values) == required else "incomplete",
            "summary": helpers.rounded_summary(values) if values else None}


def validate_metric(metric: dict, events: list[dict], required: int, helpers) -> None:
    expected = make_metric(events, required, helpers)
    require(metric == expected, "metric statistics/samples/status were not recomputed from raw events")
    for value in metric["raw_samples"]:
        finite(value, "raw sample")


def validate_event(event: dict, helpers) -> None:
    require(event.get("status") in {"ok", "not_started", "timeout", "output_limit", "exit_error",
                                   "spawn_error", "decode_error", "invalid_frame"}, "invalid process status")
    finite(event.get("duration_ms"), "process duration")
    finite(event.get("timeout_seconds"), "process timeout")
    if event["status"] == "ok":
        require(event.get("exit_code") == 0, "success requires exit code zero")
        require("stdout" in event and "stderr" in event, "successful command lacks raw logs")
    for kind in ("stdout", "stderr"):
        if kind in event:
            digest(event[kind].get("sha256"), kind)
            finite(event[kind].get("bytes"), f"{kind} bytes")
    memory = event.get("peak_rss_mb_raw_samples", [])
    for value in memory:
        finite(value, "peak RSS")
    if event["status"] != "not_started":
        require(event.get("peak_rss_mb") == (max(memory) if memory else None), "peak RSS aggregate mismatch")


def verify_artifact(record: dict, path: Path, helpers) -> None:
    require(path.is_file(), "evidence artifact is missing")
    require(path.stat().st_size == record["bytes"] and helpers.sha256_file(path) == record["sha256"],
            "evidence artifact hash/size mismatch")


def verify_logs(event: dict, directory: Path, helpers) -> None:
    for kind in ("stdout", "stderr"):
        if kind in event:
            path = (directory / event[kind]["path"]).resolve()
            require(path.is_relative_to(directory.resolve()), "log path escaped explicit scratch directory")
            verify_artifact(event[kind], path, helpers)


def _fallback_locked_packages(text: str) -> list[dict]:
    """Minimal [[package]] reader for Cargo.lock when tomllib is unavailable."""
    packages: list[dict] = []
    current: dict | None = None
    depth = 0
    for raw in text.splitlines():
        line = raw.strip()
        if depth:
            depth += line.count("[") - line.count("]")
            continue
        if line == "[[package]]":
            current = {}
            packages.append(current)
            continue
        if line.startswith("["):
            current = None
            continue
        if current is None or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        key, value = key.strip(), value.strip()
        if key not in ("name", "version", "source", "checksum"):
            continue
        depth += max(0, value.count("[") - value.count("]"))
        if len(value) >= 2 and value.startswith('"') and value.endswith('"'):
            current[key] = value[1:-1]
    return packages


def locked_packages(lock: Path) -> list[dict]:
    text = lock.read_text(encoding="utf-8")
    if tomllib is not None:
        return tomllib.loads(text)["package"]
    return _fallback_locked_packages(text)


def sqlite_locked_selection(lock: Path) -> list[dict]:
    packages = locked_packages(lock)
    selected = [{key: p.get(key) for key in ("name", "version", "source", "checksum")}
                for p in packages if p["name"] in {"rusqlite", "libsqlite3-sys"}]
    require(len(selected) == 2, "expected exactly one rusqlite and libsqlite3-sys selection")
    return sorted(selected, key=lambda p: p["name"])


def runtime_evidence(probe: Path, workspace: Path, scratch: Path, helpers, roots: dict,
                     timeout: float, deadline: float) -> dict:
    event, output = run_process([str(probe), "runtime"], workspace=workspace, scratch=scratch,
                                label="sqlite-runtime", timeout=timeout, deadline=deadline,
                                helpers=helpers, roots=roots)
    require(event["status"] == "ok", "linked Rust SQLite probe failed")
    runtime = json.loads(output)
    require(runtime.get("kind") == "linked_rust_probe" and runtime.get("rusqlite_version") == "0.40.2"
            and runtime.get("bundled") is True and runtime.get("default_features") is False,
            "runtime probe does not have the required dependency configuration")
    product = sqlite_locked_selection(workspace / "Cargo.lock")
    prototype = sqlite_locked_selection(ROOT / "Cargo.lock")
    require(product == prototype, "product/probe SQLite locked selections differ")
    return {"runtime": runtime, "probe_binary": artifact(probe, helpers), "process": event,
            "matched_product_lock_selection": product,
            "product_lock_sha256": helpers.sha256_file(workspace / "Cargo.lock"),
            "experiment_lock_sha256": helpers.sha256_file(ROOT / "Cargo.lock"),
            "scope": "Actual linked Rust probe runtime with identical locked rusqlite/libsqlite3-sys selection; not executable introspection of a caller-supplied product binary.",
            "python_sqlite_is_product_sqlite": False}
