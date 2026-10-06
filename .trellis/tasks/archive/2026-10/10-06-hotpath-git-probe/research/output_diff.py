import json
from pathlib import Path

root = Path(r"C:\AgentSessions\.trellis\tasks\10-06-hotpath-git-probe\research")
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
    "B1 hotpath before/after output diff (fixed data, fixed clock, pinned request-id)",
    "=" * 78,
    "Method: same fixture DB (/--db .trellis/.runtime/hotpath-experiment/fixture.db), same",
    "cwd (C:\\AgentSessions, a repo with origin), ASG_CLOCK_MS=1787616000000, --robot,",
    "--request-id hotpath-diff. before = HEAD 5b232cd release build sha256 6d7ebe73...;",
    "after = B1 fix release build sha256 912f45c8... Field-by-field JSON walk; only",
    "meta.duration_ms is treated as volatile (wall-clock measurement inside the envelope).",
    "",
]
all_ok = True
for variant in variants:
    for command in commands:
        before = root / f"before-output-{variant}-{command}.json"
        after = root / f"after-output-{variant}-{command}.json"
        b, a = load(before), load(after)
        diffs = []
        walk(b, a, "$", diffs)
        volatile = [d for d in diffs if any(d.startswith(p + ":") for p in VOLATILE_PREFIXES)]
        hard = [d for d in diffs if d not in volatile]
        leaves = count_leaves(b)
        lines.append(f"[{variant} / {command}] leaf fields compared: {leaves}; diffs: {len(diffs)} "
                     f"(volatile duration_ms: {len(volatile)}, non-volatile: {len(hard)})")
        if hard:
            all_ok = False
            lines.append("  NON-VOLATILE DIFFS (contract violation):")
            for d in hard:
                lines.append("    - " + d)
        else:
            lines.append("  non-volatile fields identical (hits/score/page/cursor/schema/enums/exit path unchanged)")
        for d in volatile:
            lines.append("  volatile: " + d)
        lines.append("  raw diff (un-normalized bytes): " +
                     ("identical" if before.read_text(encoding="utf-8") == after.read_text(encoding="utf-8")
                      else "differs only in the volatile fields above" if not hard else "see above"))
        lines.append("")

lines.append("RESULT: " + ("PASS - before/after JSON identical except meta.duration_ms" if all_ok
                            else "FAIL - non-volatile differences found"))
report = "\n".join(lines) + "\n"
(root / "output-diff.txt").write_text(report, encoding="utf-8")
print(report)