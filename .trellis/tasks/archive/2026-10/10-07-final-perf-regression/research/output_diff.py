"""B7: cross-variant output integrity check on the re-measured hot-path fixture.

Compares git_detect vs repo_disabled_by_test_seam outputs field-by-field.
Only meta.duration_ms is treated as volatile. Adapted from B1 output_diff.py.
"""
import json
from pathlib import Path

root = Path(r"C:\AgentSessions\.trellis\tasks\10-07-final-perf-regression\research")
variants = ["git_detect", "repo_disabled_by_test_seam"]
commands = ["get", "show", "search"]
VOLATILE_PREFIXES = ("$.meta.duration_ms",)


def load(path):
    text = path.read_text(encoding="utf-8").strip()
    return json.loads(text.splitlines()[0])


def count_leaves(node):
    if isinstance(node, dict):
        return sum(count_leaves(v) for v in node.values())
    if isinstance(node, list):
        return sum(count_leaves(v) for v in node)
    return 1


def walk(a, b, path, out):
    if type(a) is not type(b):
        out.append(f"{path}: type {type(a).__name__} -> {type(b).__name__}")
        return
    if isinstance(a, dict):
        for key in sorted(set(a) | set(b)):
            if key not in a:
                out.append(f"{path}.{key}: <absent> -> {json.dumps(b[key], ensure_ascii=False)}")
            elif key not in b:
                out.append(f"{path}.{key}: {json.dumps(a[key], ensure_ascii=False)} -> <absent>")
            else:
                walk(a[key], b[key], f"{path}.{key}", out)
    elif isinstance(a, list):
        if len(a) != len(b):
            out.append(f"{path}: list length {len(a)} -> {len(b)}")
        for i, (x, y) in enumerate(zip(a, b)):
            walk(x, y, f"{path}[{i}]", out)
    elif a != b:
        out.append(f"{path}: {json.dumps(a, ensure_ascii=False)} -> {json.dumps(b, ensure_ascii=False)}")


lines = [
    "B7 hotpath cross-variant output diff (fixed data, fixed clock, pinned request-id)",
    "=" * 78,
    "Method: same fixture DB (.trellis/.runtime/hotpath-experiment-b7/fixture.db, schema v19,",
    "same deterministic seed as B1: wire id msg_v1_cb6837d0712fbb4e022f869652978b74), same cwd",
    "(C:\\AgentSessions, a repo with origin), ASG_CLOCK_MS=1787616000000, --robot,",
    "--request-id hotpath-diff. Left = git_detect (2 probes on search), right = seam",
    "(ASG_CURRENT_REPO='', 0 probes). Binary sha256 5924c24d... at commit 4bea67f.",
    "Field-by-field JSON walk; only meta.duration_ms is volatile.",
    "",
]
all_ok = True
for command in commands:
    left = root / f"after-output-git_detect-{command}.json"
    right = root / f"after-output-repo_disabled_by_test_seam-{command}.json"
    a, b = load(left), load(right)
    diffs = []
    walk(a, b, "$", diffs)
    volatile = [d for d in diffs if any(d.startswith(p + ":") for p in VOLATILE_PREFIXES)]
    hard = [d for d in diffs if d not in volatile]
    leaves = count_leaves(a)
    lines.append(f"[git_detect vs seam / {command}] leaf fields compared: {leaves}; diffs: {len(diffs)} "
                 f"(volatile duration_ms: {len(volatile)}, non-volatile: {len(hard)})")
    if hard:
        all_ok = False
        lines.append("  NON-VOLATILE DIFFS (contract violation):")
        for d in hard:
            lines.append("    - " + d)
    else:
        lines.append("  non-volatile fields identical (hits/score/page/cursor/schema/enums/outcome unchanged)")
    for d in volatile:
        lines.append("  volatile: " + d)
    lines.append("")

lines.append("RESULT: " + ("PASS - variants identical except meta.duration_ms" if all_ok
                            else "FAIL - non-volatile differences found"))
report = "\n".join(lines) + "\n"
(root / "output-diff.txt").write_text(report, encoding="utf-8")
print(report)