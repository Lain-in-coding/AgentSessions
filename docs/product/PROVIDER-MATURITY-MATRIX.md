# Provider Maturity 与 Capability Matrix

> 对外可见的 Provider 状态清单，是 `0.3 Integration Beta` 的公开状态记录。
> - 术语与晋级证据要求见 `../architecture/RFC-0002-provider-adapter-contract.md` §6。
> - 本文件是**当前实现状态**的事实记录，不是承诺；晋级必须有证据，不由代码存在自动推断。
> - 最后更新：2026-07-22

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
| Claude Code | `claude-code` | `claude-code/jsonl-v1` | **Experimental** | 单元 + e2e（合成 fixture + 真实 transcript 手工验证）|
| Codex | `codex` | `codex/rollout-jsonl-v1` | **Experimental** | 单元 + e2e（合成 fixture + 真实 rollout 手工验证）|
| CodeBuddy | — | — | **Unsupported（未实现）** | 无 adapter |
| Pi | — | — | **Unsupported（未实现）** | 无 adapter |
| Cursor | — | — | **Unsupported（未实现）** | 无 adapter |

两个已实现 provider 均为 **Experimental**：满足"可发现/probe/解析小 fixture + 限制明确"，
但尚未达到 Beta——缺 golden 测试、property/fuzz 覆盖与 source span，且未跨正式 target 认证。

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
| session id | `native` | `native` | 二者均有 native 会话 id；当前切片未落库为独立实体 |
| git branch / cwd / version | `native` | `partial` | Claude Code 对话行携带；Codex 在 `session_meta` / `turn_context`，当前未抽取 |

## 已知限制

- **Claude Code**：只抽取 `user`/`assistant`/`system` 对话记录；工具调用块（无 text）被忽略；`cwd`/`gitBranch`/`version` provenance 尚未落库。
- **Codex**：只取权威 `response_item` + 内层 `message`，忽略 `event_msg` UI 镜像以避免重复计数；`world_state`/`turn_context`/工具调用记录未抽取；无 threading（线性）。
- **共同**：session/document 尚未作为独立 Catalog 实体建模（当前只落 message）；source span（原始字节定位）未实现，是 Beta 的前置。

## 晋级到 Beta 的缺口

1. golden 测试（固定输入→固定 Canonical 输出快照）；
2. property/fuzz 覆盖（随机/畸形输入不 panic、不产出部分结果）；
3. source span（每条 message 可回溯到源文件字节范围）；
4. 真实历史数据回归（隔离沙箱、授权数据集、不外传）；
5. 跨正式 target（Windows/Linux/macOS）的 CI 认证。
