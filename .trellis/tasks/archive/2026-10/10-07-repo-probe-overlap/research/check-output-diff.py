"""B8: cross-variant output integrity check on the re-measured hot-path fixture.

Compares git_detect vs repo_disabled_by_test_seam outputs field-by-field.
Only meta.duration_ms is treated as volatile. Adapted from B7 output_diff.py
(only the root path and the header text differ).
"""
import json
from pathlib import Path

root = Path(r"C:\AgentSessions\.trellis\tasks\10-07-repo-probe-overlap\research")
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


meta = json.loads((root / "check-after-measurements.json").read_text(encoding="utf-8"))
lines = [
    "B8 hotpath cross-variant output diff (fixed data, fixed clock, pinned request-id)",
    "=" * 78,
    f"Method: same fixture DB ({meta['db']}), deterministic seed (wire id {meta['wire_id']}),",
    f"cwd {meta['cwd']}, ASG_CLOCK_MS={meta['clock_ms']}, --robot, --request-id hotpath-diff.",
    "Left = git_detect (search: 2 overlapped git subprocesses), right = seam",
    f"(ASG_CURRENT_REPO='', 0 probes). Binary sha256 {meta['binary_sha256'][:12]}... at commit {str(meta['commit']).strip()[:7]}.",
    "Field-by-field JSON walk; only meta.duration_ms is volatile.",
    "",
]
all_ok = True
for command in commands:
    left = root / f"check-after-output-git_detect-{command}.json"
    right = root / f"check-after-output-repo_disabled_by_test_seam-{command}.json"
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
(root / "check-output-diff.txt").write_text(report, encoding="utf-8")
print(report)
# ---- trellis-check addition: compare this re-run against the delivered after-* outputs ----
print("---- cross-run comparison (this check run vs delivered after-* outputs) ----")
for variant in variants:
    for command in commands:
        mine = load(root / f"check-after-output-{variant}-{command}.json")
        theirs = load(root / f"after-output-{variant}-{command}.json")
        diffs = []
        walk(theirs, mine, "$", diffs)
        hard = [d for d in diffs if not any(d.startswith(p + ":") for p in VOLATILE_PREFIXES)]
        print(f"{variant}/{command}: diffs={len(diffs)} non-volatile={len(hard)}")
        for d in hard:
            print("    - " + d)
