# 事实账修复：竞品表技术栈与独占措辞（B0）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan（覆盖收口稿）。用户 2026-10-06 明确批准全部整改项（D1/D2/D3 全部允许）。

## Goal

按源码级证据修正对外竞品事实账与不可站住的独占措辞，使文档与可复核事实一致。只改文档，不改产品行为。

## Inputs（证据真源）

- .trellis/tasks/10-05-competitive-source-audit-plan/research/fact-table-corrections.md（逐行修正表+证据锚点）
- .trellis/tasks/10-05-competitive-source-audit-plan/review-report.md 第四节 P0-01（判决与措辞问题清单）
- 目标文件：docs/product/COMPETITOR-COMPARISON.md、docs/product/OPEN-SOURCE-ROADMAP.md

## Required fixes

1. COMPETITOR-COMPARISON.md 事实基线表修正 8 处 + 补 1 项：
   - cass：Python CLI → Rust CLI/TUI（Tantivy 生态）
   - agentsview：TS CLI → Go 后端 + Svelte 前端
   - AgentRecall：Python CLI → TypeScript/Electron/React + node:sqlite + Node MCP
   - agent-sessions：Go CLI → Swift/SwiftUI macOS 桌面
   - agf：Go CLI → Rust CLI+TUI
   - claude-historian-mcp：Python MCP → TypeScript/Node MCP
   - cc-switch：改为 FlexSearch 元数据检索（标题/摘要/项目路径/SourcePath/会话 ID，不含正文、不持久化）+ 7 家会话管理与 resume 命令生成；不可比限定为证据级检索不可比
   - hstry：TS adapter 生态 → Rust core（hstry-core/cli/tui）+ TS adapters 混合形态
   - 名单：14 项 → 15 项，补 Wake 一行（Rust+GPUI、FTS5 trigram、CLI/MCP）
2. 删除/改写无法站住的独占措辞（至少 OPEN-SOURCE-ROADMAP.md:88-94 的“无人做到 pack 级证据契约”）：改为可测承诺（证据验证成功率、出处失效行为、过滤分页一致性、输出 token 成本、恢复耗时），或删除。
3. 指标语义拆分：preview 可生成 / native resume 可执行 / pack schema 有效 / 接收方任务完成，命名不得混用；dry-run 不得当 native resume 成绩。
4. 保留历史：不删除旧快照记录；变更历史结论时保留“修正说明”行。

## Non-goals

- 不改 crates/ 任何代码；不跑 benchmark；不新增无法核验的竞品数字；不动 license 结论（cass rider / cc-sessions-viewer 无 LICENSE 维持原判并保留日期口径）。

## Acceptance Criteria

- [x] 上述 8 处形态错误 + 1 处遗漏（Wake）在目标文档中全部修正，保留原快照日期/口径说明。
- [x] 目标文档 grep 不再出现：“cass.*Python”“agentsview.*TS CLI”“AgentRecall.*Python”“agent-sessions.*Go”“agf.*Go”“claude-historian.*Python”“无检索(配置切换器)”“无人做到 pack”。
- [x] provider 数等快照数字未标注日期处补“快照日期 2026-08-14（deep-read）”口径，不做重数。
- [x] 变更仅限批准写域（两个产品文档；另经主会话 2026-10-06 批准扩域：`docs/operations/REUSE-LICENSE-AUDIT.md` 的 Wake 登记）；git diff --stat 可证。
