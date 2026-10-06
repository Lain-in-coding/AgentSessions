"""B7 independent check: recompute hot-path medians and core-benchmark P50/P95
from the raw evidence, and compare against the numbers published in the
deliverable markdown tables. Written by the trellis-check reviewer.

Outputs: check-numbers.log (next to this script). Exit code 1 if any FAIL.
"""
import hashlib
import json
import math
from pathlib import Path

research = Path(r"C:\AgentSessions\.trellis\tasks\10-07-final-perf-regression\research")
lines = []
failures = []


def emit(text=""):
    lines.append(text)
    print(text)


def check(label, ok, detail=""):
    status = "PASS" if ok else "FAIL"
    if not ok:
        failures.append(label)
    emit(f"[{status}] {label}" + (f" :: {detail}" if detail else ""))


def nearest_rank(values, p):
    ordered = sorted(values)
    rank = max(1, math.ceil(p / 100 * len(ordered)))
    return ordered[rank - 1]


def median(values):
    o = sorted(values)
    n = len(o)
    if n % 2:
        return o[n // 2]
    return (o[n // 2 - 1] + o[n // 2]) / 2.0


def close(a, b, tol):
    return abs(a - b) <= tol


emit("=== B7 independent numeric verification (trellis-check) ===")
emit("")

# ---------------------------------------------------------------- hot path
emit("--- 1. hotpath-comparison.md / b7-summary.md vs after-measurements.json ---")
hp = json.loads((research / "after-measurements.json").read_text(encoding="utf-8"))
emit(f"phase={hp['phase']} commit={hp['commit']} runs={hp['runs_per_variant']} probe_runs={hp['probe_runs_per_variant']}")
exe = Path(hp["binary"])
actual_hash = hashlib.sha256(exe.read_bytes()).hexdigest()
check("binary sha256 in after-measurements.json matches the file on disk",
      actual_hash == hp["binary_sha256"], f"{actual_hash[:16]}...")
check("commit in after-measurements.json == HEAD 4bea67f",
      hp["commit"].startswith("4bea67f"), hp["commit"])

claims_md = {  # hotpath-comparison.md sections 1,2,4: (median, min, max)
    ("git_detect", "get"): (7.96, 7.37, 9.86),
    ("git_detect", "show"): (7.91, 7.54, 10.89),
    ("git_detect", "search"): (48.02, 45.65, 50.58),
    ("repo_disabled_by_test_seam", "get"): (7.91, 7.46, 12.16),
    ("repo_disabled_by_test_seam", "show"): (8.16, 7.45, 8.65),
    ("repo_disabled_by_test_seam", "search"): (9.08, 8.26, 9.87),
}
computed = {}
for (variant, cmd), (c_med, c_min, c_max) in claims_md.items():
    entry = hp["variants"][variant][cmd]
    raw = entry["raw_ms"]
    med, mn, mx = median(raw), min(raw), max(raw)
    computed[(variant, cmd)] = med
    check(f"raw sample count {variant}/{cmd} == 20", len(raw) == 20, str(len(raw)))
    check(f"exit codes all 0 {variant}/{cmd}", entry["exit_codes"] == [0], str(entry["exit_codes"]))
    check(f"stored median == recomputed {variant}/{cmd}",
          close(entry["median_ms"], med, 1e-9), f"{entry['median_ms']} vs {med}")
    check(f"median matches published md value {variant}/{cmd}",
          close(med, c_med, 0.0051), f"recomputed {med:.6f} vs md {c_med}")
    check(f"min matches published md value {variant}/{cmd}",
          close(mn, c_min, 0.0051), f"recomputed {mn:.4f} vs md {c_min}")
    check(f"max matches published md value {variant}/{cmd}",
          close(mx, c_max, 0.0051), f"recomputed {mx:.4f} vs md {c_max}")
    emit(f"      {variant}/{cmd}: median={med:.4f} min={mn:.4f} max={mx:.4f} probes={entry['probe_counts']}")

probe_expect = {
    ("git_detect", "get"): [0, 0, 0],
    ("git_detect", "show"): [0, 0, 0],
    ("git_detect", "search"): [2, 2, 2],
    ("repo_disabled_by_test_seam", "get"): [0, 0, 0],
    ("repo_disabled_by_test_seam", "show"): [0, 0, 0],
    ("repo_disabled_by_test_seam", "search"): [0, 0, 0],
}
for key, want in probe_expect.items():
    got = hp["variants"][key[0]][key[1]]["probe_counts"]
    check(f"probe counts {key[0]}/{key[1]} == {want}", got == want, str(got))

d1 = computed[("git_detect", "get")] - computed[("repo_disabled_by_test_seam", "get")]
d2 = computed[("git_detect", "show")] - computed[("repo_disabled_by_test_seam", "show")]
d3 = computed[("git_detect", "search")] - computed[("repo_disabled_by_test_seam", "search")]
check("md delta get +0.05", close(d1, 0.05, 0.0051), f"{d1:.4f}")
check("md delta show -0.25", close(d2, -0.25, 0.0051), f"{d2:.4f}")
check("md delta search +38.94", close(d3, 38.94, 0.0051), f"{d3:.4f}")

# b7-summary.md table claims (get 7.96/0, show 7.91/0, search 48.02/2, seam search 9.08)
check("b7-summary get 7.96", close(computed[("git_detect", "get")], 7.96, 0.0051))
check("b7-summary show 7.91", close(computed[("git_detect", "show")], 7.91, 0.0051))
check("b7-summary search 48.02", close(computed[("git_detect", "search")], 48.02, 0.0051))
check("b7-summary seam search 9.08", close(computed[("repo_disabled_by_test_seam", "search")], 9.08, 0.0051))

emit("")
# ---------------------------------------------------------------- core benchmark
emit("--- 2. core-benchmark-comparison.md vs raw benchmark JSON reports ---")
base_path = Path(r"C:\AgentSessions\.trellis\.runtime\competitive-audit-20261005\core\core-beta-benchmark-full.json")
b7_path = research / "core" / "core-beta-benchmark-full.json"
base = json.loads(base_path.read_text(encoding="utf-8"))
b7 = json.loads(b7_path.read_text(encoding="utf-8"))

check("baseline report commit startswith 5b232cd", base["commit"].startswith("5b232cd"), base["commit"])
check("baseline generated_at 2026-10-05T12:12:49.301029Z",
      base["generated_at_utc"] == "2026-10-05T12:12:49.301029Z", base["generated_at_utc"])
check("baseline binary hash startswith acd7a5055c25",
      base["binary"]["binary_hash"].startswith("acd7a5055c25"), base["binary"]["binary_hash"])
check("B7 report commit == 4bea67f...", b7["commit"].startswith("4bea67f"), b7["commit"])
check("B7 generated_at 2026-10-06T17:00:53.804910Z",
      b7["generated_at_utc"] == "2026-10-06T17:00:53.804910Z", b7["generated_at_utc"])
check("B7 binary hash startswith 5924c24d9ecb",
      b7["binary"]["binary_hash"].startswith("5924c24d9ecb"), b7["binary"]["binary_hash"])
check("dataset hash identical base/B7 && startswith 8ebb21d29ce5",
      base["dataset"]["dataset_hash"] == b7["dataset"]["dataset_hash"]
      and b7["dataset"]["dataset_hash"].startswith("8ebb21d29ce5"),
      base["dataset"]["dataset_hash"])
check("profile full both", base["profile"] == "full" and b7["profile"] == "full")

# metric -> (samples, base p50, b7 p50, base p95, b7 p95, delta)   [from the md table]
claims_core = {
    "cli_startup_cold_latency_ms": (20, 20.337, 19.278, 23.600, 23.284, -0.317),
    "cli_startup_warm_latency_ms": (20, 7.038, 5.561, 8.014, 6.007, -2.007),
    "initial_sync_latency_ms": (3, 917.095, 748.014, 1059.322, 777.271, -282.050),
    "noop_sync_latency_ms": (3, 15.486, 14.969, 16.221, 15.202, -1.020),
    "shrink_sync_latency_ms": (3, 728.598, 638.429, 742.797, 638.775, -104.022),
    "search_latency_ms": (100, 114.505, 63.600, 162.896, 72.133, -90.764),
    "show_latency_ms": (100, 53.965, 9.230, 59.781, 13.567, -46.214),
    "get_latency_ms": (100, 54.426, 9.405, 62.897, 11.018, -51.879),
    "initial_index_throughput_mb_s": (3, 1.167, 1.431, 1.221, 1.447, 0.226),
}
for metric, (samples, bp50, n50, bp95, n95, delta) in claims_core.items():
    braw = base["metrics"][metric]["raw_samples"]
    nraw = b7["metrics"][metric]["raw_samples"]
    rbp50, rbp95 = nearest_rank(braw, 50), nearest_rank(braw, 95)
    rnp50, rnp95 = nearest_rank(nraw, 50), nearest_rank(nraw, 95)
    check(f"{metric}: sample counts {samples}/{samples}",
          len(braw) == samples and len(nraw) == samples, f"{len(braw)}/{len(nraw)}")
    check(f"{metric}: base P50 {bp50} (recomputed {rbp50:.6f})", close(rbp50, bp50, 0.0051))
    check(f"{metric}: B7 P50 {n50} (recomputed {rnp50:.6f})", close(rnp50, n50, 0.0051))
    check(f"{metric}: base P95 {bp95} (recomputed {rbp95:.6f})", close(rbp95, bp95, 0.0051))
    check(f"{metric}: B7 P95 {n95} (recomputed {rnp95:.6f})", close(rnp95, n95, 0.0051))
    check(f"{metric}: md P95 delta {delta}", close(rnp95 - rbp95, delta, 0.0051),
          f"recomputed {rnp95 - rbp95:.6f}")
    # stored harness summaries must agree with the recomputation too
    check(f"{metric}: stored summary p50/p95 agree with recompute",
          close(base["metrics"][metric]["summary"]["p50"], rbp50, 1e-9)
          and close(base["metrics"][metric]["summary"]["p95"], rbp95, 1e-9)
          and close(b7["metrics"][metric]["summary"]["p50"], rnp50, 1e-9)
          and close(b7["metrics"][metric]["summary"]["p95"], rnp95, 1e-9))

emit("")
emit("--- 3. sizes table in core-benchmark-comparison.md ---")
size_claims = {
    "artifact_size_bytes": (6919168, 6951936),
    "dataset_source_bytes": (1122164, 1122164),
    "index_size_ratio": (17.830691, 17.845292),
    "initial_store_bytes_including_sidecars": (20008960, 20025344),
    "post_shrink_store_bytes_including_sidecars": (20041728, 20058112),
}
for key, (bval, nval) in size_claims.items():
    check(f"size {key} base={bval} B7={nval}",
          close(base["sizes"][key], bval, 1e-6) and close(b7["sizes"][key], nval, 1e-6),
          f"{base['sizes'][key]} / {b7['sizes'][key]}")
check("initial_store_files ['catalog.db'] both",
      base["sizes"]["initial_store_files"] == ["catalog.db"] == b7["sizes"]["initial_store_files"])
check("post_shrink_store_files 3 sidecar files both",
      len(base["sizes"]["post_shrink_store_files"]) == 3
      and len(b7["sizes"]["post_shrink_store_files"]) == 3,
      f"{base['sizes']['post_shrink_store_files']} / {b7['sizes']['post_shrink_store_files']}")
check("initial_store raw samples as published",
      base["sizes"]["initial_store_bytes_raw_samples"] == [20008960] * 3
      and b7["sizes"]["initial_store_bytes_raw_samples"] == [20025344] * 3)

emit("")
emit("=== RESULT: " + ("ALL PASS" if not failures else f"{len(failures)} FAILURES: {failures}") + " ===")
(research / "check-numbers.log").write_text("\n".join(lines) + "\n", encoding="utf-8")
raise SystemExit(1 if failures else 0)
