# Worker 队列与分片计划（滚动维护）

> 更新：2026-10-06。并发硬上限 = 6 个 agent thread。完成一个补一个，不砍项。

## 运行中（6）
| agent | 项目/分片 | 交付（预期） |
|---|---|---|
| Beauvoir | hstry 核心（db/parts/peek/service/config + CLI 区域 + 2 adapters） | coverage-hstry.json / audit-hstry.md |
| Godel | Recall 核心（store/sync/session/cli/import/extension + 2 adapters + handoff 区域） | coverage-Recall.json / audit-Recall.md |
| Arendt | cass-B 检索管线（query 区域/two_tier/policy/lexical_generation） | coverage-cass-query.json / audit-cass-query.md |
| Dewey | Wake 核心（scanner/adapters/models/mcp/cli + db 区域） | coverage-Wake.json / audit-Wake.md |
| Pascal | cc-switch-A 会话管理/检索/用量 | coverage-cc-switch-sessions.json / audit-cc-switch-sessions.md |
| Mencius | agentsview-A Go 数据层与检索 | coverage-agentsview-core.json / audit-agentsview-core.md |

## 排队（按优先级）
1. **ctx-A**（上一轮被上限挡下）：projections.rs / catalog.rs / schema/migrations.rs / importer.rs / provider_sources/discovery.rs / provider/native.rs / ctx-history-search/src/packet.rs
2. **agent-sessions-A**（Swift）：SessionIndexer.swift / UnifiedSessionIndexer.swift / Indexing/DB.swift / Model/Session.swift / ClaudeSessionParser.swift / Search/SearchCoordinator.swift（+ ClaudeSessionIndexer.swift）
3. **cc-sessions-viewer-A**：src-tauri/src/agent_chat.rs / turn.rs / agents/mod.rs / util.rs（+ resume/fork/trash 区域）
4. **AgentRecall-A**：src/core/store/sessions.ts / session-loader.ts / main/index.ts / session-activity.ts / session-summarizer.ts（MCP 与 desktop 差异单列）
5. **cass-C**：src/storage/sqlite.rs 区域 + src/indexer/semantic.rs + src/indexer/mod.rs 区域（ingestion/storage）
6. **cass-D**：src/lib.rs 脊柱区域 + src/sources/sync.rs + src/ui/app.rs 区域（接口面）
7. **agentsview-B**：internal/sync/engine.go 区域 + internal/parser/{codex,claude}.go（同步与解析）
8. **cc-switch-B**：src-tauri/src/proxy/*（forwarder.rs、handlers.rs、providers/transform_codex_chat.rs 区域）——相邻产品/事实表补证
9. **agent-sessions-B**：Views 区（SessionTerminalView / UnifiedSessionsView 区域）+ Codex 状态/恢复路径（视必要）
10. **ctx-B**：semantic/daemon + integrations/mcp + CLI 区域（视必要）

## 已完成并入账本（7 项目）
agf(33/34)、sessiongrep(25/26)、fast-resume(12 full/2 partial)、cass-A(5 full)、claude-historian-mcp(41 full/1 partial/4 excluded)、memex(9 full)、+ T2 全 15 项目、T3 排除记录。

## 合并窗口后的固定动作
1. `node research/merge-receipt.mjs --project <name> --receipt research/coverage-<x>.json`
2. 校验 states 与 hash_failures=0
3. 把该分片 Top 发现追加到 review-report.md §三·B
4. `close_agent` 释放槽位 → spawn 队列下一项（prompt 带 T1 清单与格式基准）
