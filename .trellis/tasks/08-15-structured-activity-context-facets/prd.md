# Structured Activity and Context Facets

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。吸收 08-14-sidechain-subagent-facets 与
> 08-14-structured-tool-activity 两个任务的范围。

## Goal

让检索维度从"只搜 message 正文"扩展到结构化面:main/sidechain/subagent
facet 过滤、tool call/result/文件/命令活动检索、session 元数据
(provider/title/cwd)搜索——全部建立在与 Canonical Model 一致的关系模型上。

## Requirements

1. **Sidechain/subagent facets**(吸收 08-14-sidechain-subagent-facets):
   - 搜索与 context 支持按 main-only / include-sidechain / subagent-only
     过滤;
   - subagent 关系(哪个 agent、父 session)作为一等可查面;
   - 与 domain 现有 `is_sidechain` / MessageRelation(Subagent)对齐。
2. **Structured tool activity**(吸收 08-14-structured-tool-activity):
   - tool call / result 统一抽象(kind/actor/name/target/status/message 关联),
     参考 Recall 的 session_events 形态但落在现有 schema 演进上;
   - 可按"读过的文件、执行过的命令、失败的工具调用"检索;
   - target 提取(path/file_path/command/query 等优先级链)与 kind 推断
     规则入库并有 fixture;
   - 文本保留策略分级(成功输出可只索引 preview,失败输出保留更多;
     patch/diff 大块可选省略)——控制索引体积。
3. **Session metadata search**(吸收 08-14-session-metadata-search):
   - 按 provider session id、title、working directory 搜索;
   - 不索引 source path(隐私),cwd 可配脱敏。
4. **Robot/MCP 面**:新 filter 与 facet 在 Robot 1.1 envelope 与 MCP
   工具参数中统一暴露;能力缺失显式报错。#2 只声明 provider-specific
   parse/tool-event 能力与 canonical input contract;本任务负责统一 domain
   projection、索引列、过滤与查询，不重复实现 provider adapter parsing。
   所有 activity、cwd、env value、command/query、tool result 在机器/跨边界
   输出中经过 #1 的 OutputBoundary redactor。
5. **性能**:facet 过滤走索引列而非全表扫描;Gate D 全量语料回归不回退。

## Acceptance Criteria

- [ ] main/sidechain/subagent facet 在 CLI/MCP/Robot/TUI 一致可用;
- [ ] tool activity 检索(文件/命令/失败)端到端可用,含 fixture golden;
- [ ] session metadata 搜索可用且不泄漏 source path;
- [ ] 结构化数据是 catalog 可重建投影(或 additive schema,迁移向后兼容);
- [ ] 全量真实语料 Gate D 六不变量全绿;
- [ ] cargo fmt/clippy/test 全绿。

## Notes

- 参考(思路级):ctx 的 provider_policy_event_text 分级、Recall 的
  events.rs target 优先级链、claude-historian-mcp 的 context 结构化提取。
