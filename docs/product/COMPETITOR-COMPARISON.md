# Competitor Comparison Table

> Open-source gate artifact (08-15-benchmark-install-open-source-gate R1):
> 与当前已核验外部参考项目的公开对比表。只列可复现事实(provider 数、
> 能力、license、形态);不能合法/稳定运行的竞品标「不可比」。
> 证据来源: `C:/AgentHub/project/Github_src/deep-read-*.md`(13 份外部项目
> 深读报告,2026-08-14 生成)与固定 clone(commit 见
> `docs/operations/REUSE-LICENSE-AUDIT.md`)。
> cass(coding_agent_session_search)因 LICENSE 含 restricted-party rider
> 仅 clean-room 思路可引用,不进入可复现对比基线。
> 更新:2026-08-16

## 事实基线

| 项目 | 形态 | License | Provider 覆盖 | 检索方式 | 其他可复现事实 |
|---|---|---|---|---|---|
| **agent-session-grep (本产品)** | Rust CLI + MCP + Robot + TUI + Web UI | MIT OR Apache-2.0 | 14 实现 + 2 deferred(16 行矩阵,诚实分级) | lexical(FTS5) + semantic/hybrid RRF | evidence-first(source span)、handoff-pack/v1、resume dry-run、零遥测可验证、跨边界默认脱敏 |
| ctx | Rust CLI | MIT | 40+ | hybrid RRF | 语义+词法融合,分阶段发布 |
| coding_agent_session_search (cass) | Python CLI | **restricted-party rider** | 40+(claimed) | hybrid | 仅 clean-room 思路可引用;不可复现对比 |
| agentsview | TS CLI | MIT | 40+ | hybrid | secret 扫描但非分层边界 |
| AgentRecall | Python CLI | MIT | 16 | hybrid | — |
| hstry | TS adapter 生态 | MIT | 16 | FTS + adapter 架构 | UUID v5 身份最接近,但无 durable outbox |
| fast-resume | Rust CLI | MIT | 12 | FTS + 模糊 | — |
| Recall | Rust CLI | MIT | 11 | hybrid | — |
| agent-sessions | Go CLI | MIT | 10 | FTS5 | — |
| agf | Go CLI | MIT | 8 | fuzzy | — |
| cc-switch | 桌面 App | MIT | 7 | 无检索(配置切换器) | 非检索工具,「不可比」 |
| sessiongrep | Rust CLI | MIT | 5 | FTS5 | — |
| memex | Rust CLI | MIT | 4 | hybrid | — |
| claude-historian-mcp | Python MCP | MIT | 1 | 零存储全扫 | 仅 Claude Code |

## 差异化对比(agent-session-grep vs 最佳对手)

| 维度 | agent-session-grep | 最佳对手 | 差距 |
|---|---|---|---|
| 检索 | lexical + semantic hybrid,bigram CJK 一等公民 | ctx hybrid RRF | 中文场景优于纯 FTS/纯 RRF |
| 证据 | 每条命中带 source span 可回溯 + handoff pack 原文/推断分栏 | hstry 有 evidence,无 pack 契约 | pack 级证据契约无人做到 |
| 身份 | StableId 三级 + durable outbox + CAS generation + 失败不删除 | hstry UUID v5(无 outbox) | 增量崩溃安全更完整 |
| 成熟度 | certified/GA/beta/experimental/unsupported 证据晋级 | 多数虚标 provider 数 | 诚实分级,不跨级宣传 |
| 隐私 | 零遥测代码级禁止 + CI 静态检查 + `--offline` + 跨边界默认脱敏 | agentsview secret 扫描(单层) | 分层脱敏可验证 |
| 入口 | CLI/MCP/Robot/TUI/Web 五入口一致(统一 Application ADT) | cass robot 粒度更粗 | 五入口契约一致 |

## 不可比清单

- **cc-switch**: 配置切换器,无检索能力。
- **cass**: 受限 license,不可复现对比。
- 其余 13 项:license 可自由引用,但 provider 数、能力为 deep-read 快照
  (2026-08-14),与最新上游可能有差异;发布 benchmark 时以固定 clone commit
  为准复现。

## 口径说明

- 「13 个外部项目」= 可自由引用集合(13 项);总名单 14 项(含 clean-room-only
  cass)。未补齐两项独立外部基线前,不使用「15 个外部项目」宣传口径。
- 本表与 PROVIDER-MATURITY-MATRIX.md、README 数字一致(16 行矩阵、
  14 实现 + 2 deferred)。
