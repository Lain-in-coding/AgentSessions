# agent-session-grep 开源交付路线图

> 治理记录(Governance Record)
>
> - decision_id: DOC-OPEN-SOURCE-ROADMAP
> - status: **Accepted**(owner 2026-08-15 逐项评审确认)
> - owner: QIN
> - approver: 项目最终验收人(owner)
> - 本文是执行总规划的持久化入口。

## 1. 产品定位与交付形态

**产品名 `agent-session-grep`,CLI 别名 `asg`。**

交付采用 **CLI-first**: CLI 是个人重度用户的主入口,MCP/Robot/TUI/Web UI
通过统一 Application ADT 提供一致能力;原生 GUI 不属于首发,但保留为后续扩展边界。

把散落在 16 个 AI coding agent 本地目录里的会话历史,变成一套
**可验证、可恢复、可交接**的本地基础设施:

> 给一个模糊问题,不仅找到相关历史,还给可验证证据、关键上下文,
> 以及可直接 resume 的会话或可交给当前 agent 的 handoff pack。

- 第一用户:个人 AI coding-agent 重度用户;第二入口:agent/MCP 消费者。
- 团队同步、云端、知识图谱**不在首发范围**。
- 发布形态:一次性完整开源(发布门全绿后公开,时点是 owner 最终裁量)。
- License:MIT OR Apache-2.0 + Provider Adapter Protocol contribution policy。

## 2. 差异化主张(对已核验外部参考项目)

下表「对手现状」一列描述的是 §5 列出的固定 clone 集合在 2026-08-14
精读快照中的观察结果,不是对全部同类项目的断言。

| 差异化 | 说明 | 对手现状 |
|---|---|---|
| Evidence-first | 每条结果带 source span 可回溯原文;handoff pack 原文/推断分栏 | 在 2026-08-14 调研集合内未观察到 pack 级证据契约 |
| 身份与一致性 | StableId 三级 + durable outbox + CAS generation + 失败不删除 | hstry 的 UUID v5 最接近但无 outbox |
| CJK 一等公民 | bigram 索引 + 可选 `semantic-candle` 本地 E5 后端(feature-gated、默认 off、离线导入) + 中文语义模型 benchmark 待落地后锁定(默认向量模式为 bigram-hash fuzzy-lexical,非真实语义模型) | 该集合内竞品基本无中文分词处理 |
| 诚实能力矩阵 | certified/GA/beta/experimental/unsupported 分级,证据晋级,禁止跨级宣传 | 该集合内多数项目虚标 provider 数 |
| 零遥测可验证 | 代码级禁止 + CI 静态检查 + 全局 `--offline` flag（fail-closed）+ `tests/network_egress.rs` 零 HTTP client / 唯一 loopback socket 断言 | 在 2026-08-14 调研集合内未观察到可验证的零遥测约束 |
| 跨边界默认脱敏 | Web/Handoff/MCP/Robot 默认脱敏,CLI/TUI 本地不脱敏 | agentsview 有 secret 扫描但非分层边界 |
| Robot 契约 | error catalog(权威码表见 `schemas/robot/v1/error-catalog.json`) + cursor 防篡改 + retrieval_mode | cass robot 模式粒度更粗 |

## 3. 已交付的能力阶段

开源前的工程规划分十个阶段推进,实现均已落地;是否满足发布门另见 §4,
本节只记录交付范围:

1. **统一发布契约**:provider-scoped identity、Robot 1.1(`retrieval_mode`)、
   能力矩阵单源、handoff-pack/v1 schema、`asg` 别名。
2. **16 provider 证据链**:证据→fixture→adapter→分级(14 实现 + 2 deferred);
   Claude/Codex 的 certified 目标尚未达成,矩阵保持 Experimental。
3. **语义/混合本地检索**:message/placement 召回 + session 聚合;lexical 永远
   可用并显式降级,默认仍是 lexical,semantic 仅在可选 feature 下启用。
4. **证据 handoff pack**:deterministic 默认、evidence/inference 分栏、
   预算/截断/脱敏、可复现。
5. **Resume metadata 与执行**:resume 命令矩阵、dry-run 默认 + opt-in 执行、
   首次强制预览。
6. **结构化活动与上下文分面**:sidechain/subagent facet、tool activity 检索、
   session metadata 搜索。
7. **Loopback Web UI parity**:`asg serve` loopback HTTP + Web UI 核心 parity +
   安全模式。
8. **离线与隐私 hook**:ADR-0009 脱敏边界、零遥测可验证、Hook 默认关闭。
9. **Benchmark、安装与开源交付物**:公开 benchmark harness、三平台安装器、
   竞品对比表。
10. **最终集成与发布演练**:演练 runbook、五入口一致性 harness 与 Go/No-Go
    报告已落地;三平台干净环境演练尚未全部执行(macOS 未跑)。

## 4. 发布门(全部满足才提请 owner 公开)

要点:16 provider 证据齐、Claude/Codex certified、
≥5 主流 beta/GA 主路径、semantic+benchmark、handoff-pack/v1、Web UI
parity、零遥测可验证、跨边界脱敏、Hook 默认关闭、三平台安装、
benchmark/文档/对比表一致、三平台演练通过。

## 5. 竞品事实基线(2026-08-15 精读结论,13 个外部项目)

当前仓库已保存并核验的外部参考项目为:
`ctx`、`cass`(即 `coding_agent_session_search`)、`agentsview`、`AgentRecall`、
`agent-sessions`、`agf`、`cc-switch`、`claude-historian-mcp`、`fast-resume`、
`hstry`、`memex`、`Recall`、`sessiongrep`,共 13 项。
其中 12 项可自由引用(13 项减去 LICENSE 含 restricted-party rider、
仅限 clean-room 思路的 `cass`);进入可复现检索对比基线的是 11 项
(12 项再减去无检索能力的配置切换器 `cc-switch`)。逐项口径见
`COMPETITOR-COMPARISON.md`。
发布前 benchmark 必须逐项给出来源、commit/version 和可复现命令;
若要继续使用”15 个外部项目”宣传口径,必须先补齐两项独立外部基线,
否则统一改称”13 个外部项目”。

- provider 覆盖:ctx 40+、agentsview 40+、AgentRecall 16、hstry 16、
  fast-resume 12、Recall 11、agent-sessions 10、agf 8、cc-switch 7、
  sessiongrep 5、memex 4、claude-historian-mcp 1、
  cass(claimed 40+,待以同一 benchmark harness 核验)。
- 检索:纯 FTS5 系(sessiongrep/agent-sessions)、FTS+模糊(fast-resume)、
  hybrid RRF(ctx/cass/Recall/agentsview/memex)、fuzzy(agf)、零存储全扫
  (claude-historian-mcp)。
- 深度精读报告(2026-08-14 生成)仅作为本地研究输入,不纳入公开树;cass 因
  clean-room 边界不产 deep-read 报告。对外可引用的结论收敛到
  `COMPETITOR-COMPARISON.md` 与本节。
- 法律红线:cass LICENSE 含 rider,仅 clean-room 思路;REUSE-LICENSE-AUDIT
  维护 direct-copy/adapt/idea-only/reject 边界。

## 6. 术语与决策记录

- 术语以 `CONTEXT.md` 为准。
- 不可逆决策入 ADR:2026-08-15 新增 ADR-0009(跨边界输出脱敏)。
