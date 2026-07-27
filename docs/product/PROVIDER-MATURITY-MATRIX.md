# Provider Maturity 与 Capability Matrix

> 对外可见的 Provider 状态清单，是 `0.3 Integration Beta` 的公开状态记录。
> - 术语与晋级证据要求见 `../architecture/RFC-0002-provider-adapter-contract.md` §6。
> - 本文件是**当前实现状态**的事实记录，不是承诺；晋级必须有证据，不由代码存在自动推断。
> - 最后更新：2026-07-27

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
- **共同**：真实历史数据回归自 2026-07-27 起有可重复 harness（抛弃式临时 store +
  聚合-only 报告），但语料留本机不可共享，故任何单次运行结果第三方无法复核；
  跨平台真实数据回归（CI 上无真实语料）仍不存在。
- **共同（2026-07-27 新发现，阻塞级）**：真实语料上 `sync` 无法完成——一个
  Claude Code 会话常跨多个 `.jsonl` 文件（实测某会话被 55 个文件各自声明），
  而当前存储层按"一源一会话"投影，同一 `ses_v1_` 在不同源上成员列表不同即被判
  冲突投影并拒绝整批（`catalog_error`，exit 6）。合成夹具（一文件一会话）不触发
  此路径，故此前无从暴露。详见 `docs/evidence/integration-beta/real-data-regression.md`。

## 晋级到 Beta 的缺口

1. ~~golden 测试~~ —— 已入库：`tests/golden/` fixture（BLAKE3 锁定字节）+ 结构化
   期望输出比对，任何 canonical 输出漂移即失败（2026-07-26）。
2. ~~property/fuzz 覆盖~~ —— 已入库：固定种子确定性 property 套件（畸形行、
   Unicode 多字节 span、大字段、threading、codex 镜像去重），失败可由种子复现（2026-07-26）。
3. ~~source span~~ —— 已入库：schema v6 + `MessageEvent.span` 契约，golden/e2e
   round-trip 锁定（2026-07-26，见 `docs/operations/migration-v5-to-v6.md`）。
4. 真实历史数据回归（隔离沙箱、授权数据集、不外传）——**流程已入库，但首次
   授权本机运行判定 FAILED，缺口未闭合**。harness
   （`scripts/evidence/real_data_regression.py`）在抛弃式临时 data root 上跑
   sync → status/doctor → 逐会话 context → index rebuild 校验六条不变量，报告只含
   聚合计数（见 `docs/operations/REAL-DATA-REGRESSION.md`、证据行
   `IB-REAL-DATA-REGRESSION-001`）。2026-07-27 对 707 个真实源（605 MB）的运行在
   第一条不变量 `INV-SYNC-OK` 即失败：跨文件会话触发存储层冲突投影拒绝
   （见上「已知限制」与
   `docs/evidence/integration-beta/real-data-regression.md`）。**这是缺口 4 当前
   的首要阻塞**：产品需先支持会话跨源合并，harness 才可能跑通。
5. 跨正式 target（Windows/Linux/macOS）的 CI 认证——仍缺。`ci.yml` 的 `test` 与
   新增 `installer` job 已配置三平台矩阵（证据行 `IB-CI-INSTALLER-001`），但在
   PR 上跑绿并记录具体 run id 之前只能是 `ci_configured_only`；hosted runner 亦
   非 clean machine，不构成安装认证。
