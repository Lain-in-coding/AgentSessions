# 命中窗口摘要（SearchHit.text 语义变更）

## Goal

让 `SearchHit.text` 在存在可证明的字面命中时显示命中窗口，而不是固定前缀；无可证明命中时保持前缀，绝不伪造证据。排名、游标、证据字段与所有其他 wire 字段不变。

## Requirements

- 语义：字面命中 → 以最早命中为中心的原文连续窗口；无字面命中/语义-only/无文本 → 现有前缀或 `None`。
- 同一字段同类型（`String`），≤ `max_snippet_chars`（字符），并计入 `max_response_bytes`；不插入省略号、高亮或任何合成字符。
- 词元与 `guidance::literal_terms` 同源（该函数已使用与索引一致的 CJK/plain-text 变换）；CJK bigram 是真实子串证据、bigram 之外的 FTS 语法产物不得作为证据。末位 `*` 按 guidance 现状处理（星号是字面词元的一部分，不匹配则回退前缀）——与 `why_matched` 保持一致，不新增前缀解析语义。
- Unicode：大小写不敏感匹配需把 lower-case 展开映射回原字符边界（如 `İ`）；输出必须是原文精确切片；不保证 grapheme 完整。
- 不变：排名、游标（cursor digest/total order）、`why_matched`、`suggested_next_commands`、occurrences、证据字段、facets。
- 更新 `SearchHit.text` 契约注释与相关文档/规范；跨入口（CLI/MCP/Robot/Web/TUI）由共享 Application 投影自动一致。
- 不新增字段、CLI flag 或依赖；不改 schema。

## Acceptance Criteria

- [x] 长尾命中可见：命中位于正文远超 `max_snippet_chars` 之后时，输出窗口包含该命中。
- [x] 多命中取最早命中；窗口为原文连续切片且不含合成字符。
- [x] 语义-only、无匹配、空文本不伪造命中：分别回退前缀或 `None`。
- [x] 字符上限与最终序列化字节预算均不超；预算收紧时如实截断并可复现。
- [x] 排名、游标、`why_matched`、建议命令输出与改动前一致（回归断言）。
- [x] 移植实验 A 的合成用例矩阵（裁剪版）并通过；fmt/clippy/test 全绿。
- [x] `.trellis/spec/agentsessions-application/backend/index.md` 与本任务契约文档同步更新。

## Notes

- 实验 A 原型与 49 例证据：`.trellis/tasks/archive/2026-09/09-29-wake-reuse-study/research/experiments/snippet/`（结论：候选 25/49 可见命中 vs 前缀 16/49；6/49 前缀超局部字节上限）。
- 原型允许的超预算锚点返回空串行为不进入产品：产品在锚点自身超过上限时输出锚点起始的 `max_snippet_chars` 连续切片（`text` 契约是显示摘要）。

## Verification

- 提交：`feat(application): show match-centered search snippets`（d556216）。
- 门禁：`cargo fmt --all --check` 0；application/cli clippy `--all-targets -D warnings` 0（含 `cargo clean -p` 强制重编）；`cargo test -p agent-session-grep-application --offline` 283 passed / 0 failed；`cargo test -p agent-session-grep-cli --offline` 590 passed / 0 failed；`git diff --check` 通过。
- 关键用例：`search_text_window_keeps_late_literal_match_visible`（长尾命中可见）、`search_text_falls_back_to_prefix_without_literal_evidence`（无证据回退前缀）、`window_is_an_exact_contiguous_slice`（原文连续切片）、`zero_cap_is_empty_not_a_panic`、`search_window_bytes_are_charged_not_full_payload`（窗口字节计入 `max_response_bytes` 并在收紧时如实截断）、窗口后 `why_matched` 回归断言。
- 独立核查：check 代理逐条核对契约、Unicode、CLI 预览与改动范围，verdict PASS；残留项（human 会话表格视图标题列仍为前缀截断、CLI 复制一份 anchor 算法、why_matched 非 text 叶证据组合未覆盖）均为非阻断，留待后续任务。
