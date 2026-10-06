"""B7: build the audit-baseline vs B7 comparison table from the two full-profile reports."""
import json

BASE = r".trellis/.runtime/competitive-audit-20261005/core/core-beta-benchmark-full.json"
NOW = r".trellis/tasks/10-07-final-perf-regression/research/core/core-beta-benchmark-full.json"
OUT = r".trellis/tasks/10-07-final-perf-regression/research/core-benchmark-comparison.md"

base = json.load(open(BASE, encoding="utf-8"))
now = json.load(open(NOW, encoding="utf-8"))

lines = []
lines.append("# Core/Beta benchmark 复测对比（audit 基线 vs B7）")
lines.append("")
lines.append(f"- baseline: commit `{base['commit']}` binary `{base['binary']['binary_hash'][:12]}…` generated `{base['generated_at_utc']}`")
lines.append(f"- B7: commit `{now['commit']}` binary `{now['binary']['binary_hash'][:12]}…` generated `{now['generated_at_utc']}`")
lines.append(f"- dataset: {now['dataset']['message_count']} messages / {now['dataset']['file_count']} files (hash `{now['dataset']['dataset_hash'][:12]}…`; baseline `{base['dataset']['dataset_hash'][:12]}…`)")
lines.append("")
lines.append("| Metric | Samples | Base P50 | B7 P50 | Base P95 | B7 P95 | P95 Δ |")
lines.append("|---|---:|---:|---:|---:|---:|---:|")
for key, metric in now["metrics"].items():
    b = base["metrics"][key]["summary"]
    n = metric["summary"]
    lines.append(
        f"| `{key}` | {n['count']} | {b['p50']:.3f} | {n['p50']:.3f} | {b['p95']:.3f} | {n['p95']:.3f} | {n['p95'] - b['p95']:+.3f} |"
    )
lines.append("")
lines.append("## Sizes")
lines.append("")
lines.append("| Item | Base | B7 |")
lines.append("|---|---:|---:|")
for k in sorted(now["sizes"]):
    lines.append(f"| `{k}` | {base['sizes'][k]} | {now['sizes'][k]} |")
lines.append("")
lines.append("## Notes")
lines.append("")
lines.append("- Same harness (`scripts/evidence/core_beta_benchmark.py`), same profile (`full`), same machine class (NVMe SAMSUNG MZVL81T0HELB-00BTW SSD / NTFS), same binary provenance kind (`caller_supplied_prebuilt`).")
lines.append("- Baseline binary was built from commit `5b232cd` before the B1 hot-path fix; B7 binary is built from the current HEAD.")
lines.append("- Rows are not formal SLOs; they are local evidence anchors.")
open(OUT, "w", encoding="utf-8", newline="\n").write("\n".join(lines) + "\n")
print(OUT)