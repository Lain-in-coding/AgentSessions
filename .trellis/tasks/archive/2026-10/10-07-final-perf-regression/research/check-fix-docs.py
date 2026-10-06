# Reviewer trivial fix: correct two factual wording slips in B7 deliverable docs.
# Preserves LF line endings and file encoding exactly.
p1 = r".trellis/tasks/10-07-final-perf-regression/research/b7-summary.md"
t = open(p1, encoding="utf-8", newline="").read()
old = (
    "- 唯一命中在历史证据 manifest/checklist（`docs/evidence/core-beta/88d86f4/`、\n"
    "  `docs/release/OWNER-RELEASE-CHECKLIST.md:48` 的旧 artifact size 6,929,408 bytes）——按\n"
    "  \u201c不改历史记录\u201d原则不动；本轮无产品代码改动，不产生新的过时数字。"
)
new = (
    "- 复核时全文仅 2 处子串命中，均为 rustc 版本号 `1.97.1` 内的 `97.1`\n"
    "  （`docs/operations/INSTALL-AND-UPGRADE.md:15`、`docs/release/OWNER-RELEASE-CHECKLIST.md:48`），\n"
    "  不是性能数字；`docs/evidence/core-beta/88d86f4/` 等历史记录按\u201c不改历史记录\u201d原则不动；\n"
    "  本轮无产品代码改动，不产生新的过时数字。"
)
assert t.count(old) == 1, f"b7-summary pattern occurrences: {t.count(old)}"
open(p1, "w", encoding="utf-8", newline="").write(t.replace(old, new))

p2 = r".trellis/tasks/10-07-final-perf-regression/research/maintainability-map.md"
t2 = open(p2, encoding="utf-8", newline="").read()
old2 = (
    "\u201c生产/测试\u201d以首个 `#[cfg(test)]` 行切分。行数快照 = HEAD `4bea67f`（与 B7 测量同一\n"
    "工作树；crates 下无未提交改动）。"
)
new2 = (
    "\u201c生产/测试\u201d以表内标注的测试模块 `#[cfg(test)]` 行切分（adapters-sqlite 在 4,794 行另有\n"
    "一个 `#[cfg(test)]` 门控的辅助方法，按本口径计入生产段）。行数快照 = HEAD `4bea67f`\n"
    "（与 B7 测量同一工作树；crates 下无未提交改动）。"
)
assert t2.count(old2) == 1, f"maintainability pattern occurrences: {t2.count(old2)}"
open(p2, "w", encoding="utf-8", newline="").write(t2.replace(old2, new2))

for p in (p1, p2):
    b = open(p, "rb").read()
    print(p, "CRLF" if b"\r\n" in b else "LF", "BOM" if b.startswith(b"\xef\xbb\xbf") else "noBOM")
print("edits applied")
