"""B7 check-side output integrity: (a) deliverable variant diff claim, (b) cross-run stability
(deliverable git_detect outputs vs reviewer's re-run outputs). Writes check-output-diff.txt."""
import json
from pathlib import Path

research = Path(r"C:\AgentSessions\.trellis\tasks\10-07-final-perf-regression\research")
commands = ["get", "show", "search"]
VOLATILE = "$.meta.duration_ms"


def load(path):
    return json.loads(path.read_text(encoding="utf-8").strip().splitlines()[0])


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


lines = ["B7 check-side output diff (reviewer)", "=" * 60]
ok = True

lines.append("[A] deliverable: git_detect vs repo_disabled_by_test_seam (claim: only meta.duration_ms differs)")
for cmd in commands:
    a = load(research / f"after-output-git_detect-{cmd}.json")
    b = load(research / f"after-output-repo_disabled_by_test_seam-{cmd}.json")
    diffs = []
    walk(a, b, "$", diffs)
    hard = [d for d in diffs if not d.startswith(VOLATILE + ":")]
    lines.append(f"  {cmd}: diffs={len(diffs)} volatile={len(diffs) - len(hard)} non-volatile={len(hard)}")
    for d in hard:
        lines.append("    NON-VOLATILE DIFF: " + d)
    if hard:
        ok = False
    for d in diffs:
        if d.startswith(VOLATILE + ":"):
            lines.append("    volatile: " + d)

lines.append("[B] cross-run: deliverable git_detect outputs vs reviewer re-run outputs (expect only meta.duration_ms)")
for cmd in commands:
    a = load(research / f"after-output-git_detect-{cmd}.json")
    b = load(research / f"check-after-output-git_detect-{cmd}.json")
    diffs = []
    walk(a, b, "$", diffs)
    hard = [d for d in diffs if not d.startswith(VOLATILE + ":")]
    lines.append(f"  {cmd}: diffs={len(diffs)} volatile={len(diffs) - len(hard)} non-volatile={len(hard)}")
    for d in hard:
        lines.append("    NON-VOLATILE DIFF: " + d)
    if hard:
        ok = False
    for d in diffs:
        if d.startswith(VOLATILE + ":"):
            lines.append("    volatile: " + d)

lines.append("")
lines.append("RESULT: " + ("PASS" if ok else "FAIL"))
text = "\n".join(lines)
print(text)
(research / "check-output-diff.txt").write_text(text + "\n", encoding="utf-8")
raise SystemExit(0 if ok else 1)
