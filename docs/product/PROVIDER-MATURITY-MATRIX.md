# Provider Maturity 与 Capability Matrix

> 对外可见的 Provider 状态清单，是 `0.3 Integration Beta` 的公开状态记录。
> - 术语与晋级证据要求见 `../architecture/RFC-0002-provider-adapter-contract.md` §6。
> - 本文件是**当前实现状态**的事实记录，不是承诺；晋级必须有证据，不由代码存在自动推断。
> - 最后更新：2026-07-26

## 术语

**整体成熟度**（Provider 级，独立于字段能力）：

| maturity | 含义 |
|---|---|
| `Experimental` | 可发现、可 probe、可解析小 fixture，限制明确 |
| `Beta` | 主路径 fixture、golden、contract、只读、增量、source span 均通过 |
| `Certified/GA` | 历史 variant、混合版本、未知字段、崩溃恢复、正式 target、性能与回滚证据齐备 |
| `Unsupported` | 明确不支持，附原因 |

**字段能力**（capability，逐字段）：`native`（provider 原生提供）、`derived`（由内在内容确定性派生）、`partial`（部分场景可得）、`unsupported`（该 provider 无此概念）、`unknown`（尚未评估）。

## 成熟度总览

| Provider | provider_id | variant | maturity | 证据 |
|---|---|---|---|---|
| Claude Code | `claude-code` | `claude-code/jsonl-v1` | **Experimental** | 单元 + e2e + golden（`crates/agentsessions-provider-claude/tests/golden.rs`）+ 确定性 property 套件（`tests/properties.rs`，固定种子）+ span round-trip |
| Codex | `codex` | `codex/rollout-jsonl-v1` | **Experimental** | 单元 + e2e + golden（`crates/agentsessions-provider-codex/tests/golden.rs`）+ 确定性 property 套件（含镜像去重性质）+ span round-trip |
| CodeBuddy | — | — | **Unsupported（未实现）** | 无 adapter |
| Pi | — | — | **Unsupported（未实现）** | 无 adapter |
| Cursor | — | — | **Unsupported（未实现）** | 无 adapter |

两个已实现 provider 均为 **Experimental**：golden、property、source span 三项 Beta
证据已入库（见下），剩余 blocker 收窄为两项——授权真实数据回归与跨 target CI 认证。

## Capability Matrix

字段对应 Canonical `Message`（`crates/agentsessions-domain/src/lib.rs`）与解析产出。

| 字段 | Claude Code | Codex | 说明 |
|---|---|---|---|
| native message id | `native` | `native` | Claude Code 用 `uuid`；Codex 用 `response_item.payload.id`。均原样采用为 `Stability::Native` |
| role | `native` | `native` | Claude Code `message.role`；Codex `payload.role`（含 `developer` 系统层）|
| text | `native` | `native` | Claude Code content（string / block 数组）；Codex `content[].text` 拼接 |
| parent / threading | `native` | `unsupported` | Claude Code `parentUuid` 形成 DAG；Codex rollout 为线性序列，无显式父指针（threading 靠顺序）|
| timestamp | `native` | `native` | Claude Code 对话行内 `timestamp`；Codex 记录封套 `timestamp`（ISO-8601 UTC）|
| is_sidechain | `native` | `derived` | Claude Code `isSidechain`；Codex 无该标记，一律 false |
| session id | `native` | `native` | 二者均有 native 会话 id；自 schema v6 起落库为独立 `ses_v1_` 实体 |
| source span | `native` | `native` | 每条 message 携带源记录在已验证快照内的字节区间（end 排他）；golden/property 测试锁定 round-trip |
| git branch / cwd / version | `native` | `partial` | Claude Code 对话行携带；Codex 在 `session_meta` / `turn_context`，当前未抽取 |

## 已知限制

- **Claude Code**：只抽取 `user`/`assistant`/`system` 对话记录；工具调用块（无 text）被忽略；`cwd`/`gitBranch`/`version` provenance 尚未落库。
- **Codex**：只取权威 `response_item` + 内层 `message`，忽略 `event_msg` UI 镜像以避免重复计数；`world_state`/`turn_context`/工具调用记录未抽取；无 threading（线性）。
- **共同**：真实历史数据回归仍为手工验证（2026-07-21 对真实 transcript/rollout 各验证一次），
  尚无隔离沙箱内可重复的授权数据集回归。

## 晋级到 Beta 的缺口

1. ~~golden 测试~~ —— 已入库：`tests/golden/` fixture（BLAKE3 锁定字节）+ 结构化
   期望输出比对，任何 canonical 输出漂移即失败（2026-07-26）。
2. ~~property/fuzz 覆盖~~ —— 已入库：固定种子确定性 property 套件（畸形行、
   Unicode 多字节 span、大字段、threading、codex 镜像去重），失败可由种子复现（2026-07-26）。
3. ~~source span~~ —— 已入库：schema v6 + `MessageEvent.span` 契约，golden/e2e
   round-trip 锁定（2026-07-26，见 `docs/operations/migration-v5-to-v6.md`）。
4. 真实历史数据回归（隔离沙箱、授权数据集、不外传）——仍缺可重复流程。
5. 跨正式 target（Windows/Linux/macOS）的 CI 认证——新测试需随 PR 在三平台
   CI 跑绿后方可主张。
