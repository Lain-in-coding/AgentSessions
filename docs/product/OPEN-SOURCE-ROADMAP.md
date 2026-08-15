# agent-session-grep 开源登顶路线图

> 治理记录(Governance Record)
>
> - decision_id: DOC-OPEN-SOURCE-ROADMAP
> - status: **Accepted**(owner 2026-08-15 grill-with-docs 拷问逐轮确认)
> - owner: QIN
> - approver: 项目最终验收人(owner)
> - evidence_path: `.trellis/tasks/08-15-open-source-product-roadmap/prd.md`(决策全文)
> - 本文是执行总规划的持久化入口;新会话/新 agent 以本文 + 父任务 PRD 为权威。

## 1. 使命与定位

**产品名 `agent-session-grep`,CLI 别名 `asg`。**

交付采用 **CLI-first**: CLI 是个人重度用户的主入口,MCP/Robot/TUI/Web UI
通过统一 Application ADT 提供一致能力;原生 GUI 不属于首发,但保留为后续扩展边界。

把散落在 16 个 AI coding agent 本地目录里的会话历史,变成一套
**可验证、可恢复、可交接**的本地基础设施:

> 给一个模糊问题,不仅找到相关历史,还给可验证证据、关键上下文,
> 以及可直接 resume 的会话或可交给当前 agent 的 handoff pack。

- 第一用户:个人 AI coding-agent 重度用户;第二入口:agent/MCP 消费者。
- 团队同步、云端、知识图谱**不在首发范围**。
- 发布形态:一次性完整开源(仓库保持 PRIVATE 直到发布门全绿,公开是
  owner 最终裁量)。
- License:Apache-2.0 + Provider Adapter Protocol contribution policy。

## 2. 差异化主张(对已核验外部参考项目)

| 差异化 | 说明 | 对手现状 |
|---|---|---|
| Evidence-first | 每条结果带 source span 可回溯原文;handoff pack 原文/推断分栏 | 无人做到 pack 级证据契约 |
| 身份与一致性 | StableId 三级 + durable outbox + CAS generation + 失败不删除 | hstry 的 UUID v5 最接近但无 outbox |
| CJK 一等公民 | bigram 索引 + 中文语义模型 benchmark 达标后锁定 | 竞品基本无中文分词处理 |
| 诚实能力矩阵 | certified/GA/beta/experimental/unsupported 分级,证据晋级,禁止跨级宣传 | 多数项目虚标 provider 数 |
| 零遥测可验证 | 代码级禁止 + CI 静态检查 + `--offline` | 无人做到可验证 |
| 跨边界默认脱敏 | Web/Handoff/MCP/Robot 默认脱敏,CLI/TUI 本地不脱敏 | agentsview 有 secret 扫描但非分层边界 |
| Robot 契约 | 13+ 码 error catalog + cursor 防篡改 + retrieval_mode | cass robot 模式粒度更粗 |

## 3. 阶段与任务树

**Phase 0(进行中,照常收尾)**:08-13 四功能整合 + 08-14 Resume Metadata
流——是本规划的既有地基,产出被后续任务吸收。

**Phase 1–10(08-15 任务树,父任务
`08-15-open-source-product-roadmap` 只做集成验收)**:

| 阶段 | 子任务 | 一句话目标 | 优先级 |
|---|---|---|---|
| 1 | 08-15-unified-release-contract | provider-scoped identity、Robot 1.1(retrieval_mode)、能力矩阵单源、handoff-pack/v1 schema 草案、`asg` 别名 | P0 |
| 2 | 08-15-sixteen-provider-evidence-wave | 16 provider 证据→fixture→adapter→分级;Claude/Codex 晋 certified;DeepSeek Harness/ZCode 证据补齐 | P0 |
| 3 | 08-15-semantic-hybrid-local-retrieval | 本地 embedding 模型(benchmark 锁定)、message/placement 召回+session 聚合、lexical 永远可用+显式降级 | P0 |
| 4 | 08-15-evidence-handoff-pack | handoff-pack/v1 落地:deterministic 默认、evidence/inference 分栏、预算/截断/脱敏、可复现 | P0 |
| 5 | 08-15-resume-metadata-execution | resume 命令矩阵、dry-run 默认+opt-in 执行、首次强制预览 | P0 |
| 6 | 08-15-structured-activity-context-facets | sidechain/subagent facet、tool activity 检索、session metadata 搜索 | P1 |
| 7 | 08-15-loopback-web-ui-parity | `asg serve` loopback HTTP + Web UI 核心 parity + 安全模式 | P1 |
| 8 | 08-15-offline-privacy-hooks | ADR-0009 脱敏边界、零遥测可验证、Hook 默认关闭 | P0 |
| 9 | 08-15-benchmark-install-open-source-gate | 公开 benchmark、三平台安装器、开源交付物、竞品对比表 | P0 |
| 10 | 08-15-final-integration-release-rehearsal | 三平台全新环境演练、五入口一致性、Go/No-Go 报告 | P0 |

依赖关系:1 先行;2/3/4 依赖 1 的契约;5 依赖 08-14 树收尾;7 依赖
1+3+4 的能力面;9 依赖 2–8 主体;10 终局。并行原则:同一时间最多
2–3 个子任务 in_progress,每个子任务独立 worktree、独立验收。

## 4. 发布门(全部满足才提请 owner 公开)

见父任务 PRD 验收清单;要点:16 provider 证据齐、Claude/Codex certified、
≥5 主流 beta/GA 主路径、semantic+benchmark、handoff-pack/v1、Web UI
parity、零遥测可验证、跨边界脱敏、Hook 默认关闭、三平台安装、
benchmark/文档/对比表一致、三平台演练通过。

## 5. 竞品事实基线(2026-08-15 精读结论,13 个外部项目)

当前仓库已保存并核验的外部参考项目为:
`ctx`、`cass`、`agentsview`、`AgentRecall`、`agent-sessions`、`agf`、
`cc-switch`、`claude-historian-mcp`、`fast-resume`、`hstry`、`memex`、
`Recall`、`sessiongrep`、`coding_agent_session_search`。
其中 13 个可自由引用,是对比表与 benchmark 基线的合法来源;
`cass` 因其 LICENSE 含 restricted-party rider,仅限 clean-room 思路,
计入名单但不计入可引用对比基线——因此”13 个外部项目”指可自由引用集,
名单共 14 项是”13 可引用 + 1 clean-room-only”,两者不矛盾。
发布前 benchmark 必须逐项给出来源、commit/version 和可复现命令;
若要继续使用”15 个外部项目”宣传口径,必须先补齐两项独立外部基线,
否则统一改称”13 个外部项目”。

- provider 覆盖:ctx 40+、agentsview 40+、AgentRecall 16、hstry 16、
  fast-resume 12、Recall 11、agent-sessions 10、agf 8、cc-switch 7、
  sessiongrep 5、memex 4、claude-historian-mcp 1、
  coding_agent_session_search(待以同一 benchmark harness 核验)。
- 检索:纯 FTS5 系(sessiongrep/agent-sessions)、FTS+模糊(fast-resume)、
  hybrid RRF(ctx/cass/Recall/agentsview/memex)、fuzzy(agf)、零存储全扫
  (claude-historian-mcp)。
- 深度精读报告(万字)存于 `C:/AgentHub/project/Github_src/deep-read-*.md`
  (15 份:13 份外部项目 + 2 份本项目文档,2026-08-14 生成;cass 因
  clean-room 边界不产 deep-read 报告),provider 格式证据主要来源。
- 法律红线:cass LICENSE 含 rider,仅 clean-room 思路;REUSE-LICENSE-AUDIT
  维护 direct-copy/adapt/idea-only/reject 边界。

## 6. 术语与决策日志

- 术语以 `CONTEXT.md` 为准(本轮新增决策见其 2026-08-15 日志)。
- 不可逆决策入 ADR:本轮新增 ADR-0009(跨边界输出脱敏)。
