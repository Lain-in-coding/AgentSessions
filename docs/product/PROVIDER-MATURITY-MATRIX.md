# Provider Maturity 与 Capability Matrix

> 对外可见的 Provider 状态清单，是 `0.3 Integration Beta` 的公开状态记录。
> - 术语与晋级证据要求见 `../architecture/RFC-0002-provider-adapter-contract.md` §6。
> - 本文件是**当前实现状态**的事实记录，不是承诺；晋级必须有证据，不由代码存在自动推断。
> - 最后更新：2026-08-13

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
| Claude Code | `claude-code` | `claude-code/jsonl-v1` | **Experimental** | 单元 + e2e + golden（`crates/agent-session-grep-provider-claude/tests/golden.rs`）+ 确定性 property 套件（`tests/properties.rs`，固定种子）+ span round-trip |
| Codex | `codex` | `codex/rollout-jsonl-v1` | **Experimental** | 单元 + e2e + golden（`crates/agent-session-grep-provider-codex/tests/golden.rs`）+ 确定性 property 套件（含镜像去重性质）+ span round-trip |
| CodeBuddy | — | — | **Unsupported（未实现）** | 无 adapter |
| Pi | — | — | **Unsupported（未实现）** | 无 adapter |
| Cursor | — | — | **Unsupported（未实现）** | 无 adapter |

两个已实现 provider 均为 **Experimental**：golden、property、source span 以及
关系化 Message/Placement/Edge 的合成与 e2e 证据已入库（见下），剩余 blocker
仍为授权真实数据全量绿色回归与跨 target CI 认证。RFC-0001、RFC-0002 与共享接口
合同继续保持 **Draft**，本次证据更新不改变治理状态。

## Capability Matrix

字段对应 Canonical `Message`（`crates/agent-session-grep-domain/src/lib.rs`）与解析产出。

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
| git branch / cwd / version | `partial` | `partial` | Claude Code 对话行携带（格式上可用）；Codex 在 `session_meta` / `turn_context`。两者均**尚未抽取落库** |

## 已知限制

- **Claude Code**：只抽取 `user`/`assistant`/`system` 对话记录；工具调用块（无 text）被忽略；`cwd`/`gitBranch`/`version` provenance 尚未落库。
- **Codex**：只取权威 `response_item` + 内层 `message`，忽略 `event_msg` UI 镜像以避免重复计数；`world_state`/`turn_context`/工具调用记录未抽取；无 threading（线性）。
- **共同**：真实历史数据回归自 2026-07-27 起有可重复 harness（抛弃式临时 store +
  aggregate-only 报告），但语料留本机不可共享，故任何单次运行结果第三方无法复核；
  跨平台真实数据回归（CI 上无真实语料）仍不存在。
- **共同（关系模型已实现，语料级回归已闭合）**：稳定 `Message` 与上下文
  `MessagePlacement` / `MessageEdge` 已分离，session-scoped graph、精确 placement
  evidence、不同上下文 parent 以及相应合成/e2e 覆盖均已实现。全量授权运行
  （`2026-08-09T21:10:16Z`，1,242 源、1,177,479,794 字节）六条不变量全绿、harness
  exit 0：sync 164,136 emitted / 0 skipped、no-parse-loss 164,136 claims、
  231 sessions 全 context 成功、659/659 byte 精度、rebuild 稳定。最新两次全量
  运行同样全绿：`2026-08-12T23:51:23Z`（1,328 源、1,253,494,481 字节；180,218
  emitted / 0 skipped；242 sessions；630/630 byte 精度；rebuild 166,380 →
  166,380）与 `2026-08-13T00:26:57Z`（1,330 源、1,255,049,984 字节；180,718
  emitted / 0 skipped；242 sessions；630/630 byte 精度；rebuild 166,882 →
  166,882）——后者由改名后的 `agent-session-grep` 二进制执行，验证改名无功能
  回归。早期 `2026-07-31T10:04:17Z` 运行（879 源，`INV-SYNC-OK` exit 5 失败，
  79,958 emitted、0 skipped，其余不变量未评估；aggregate 报告未保留精确 canonical
  code，故 `source_changed` 未证实）与 2026-08-10 子集运行（137 源全绿）如实保留
  在 `docs/evidence/integration-beta/real-data-regression.md`。真实数据 Gate D 已
  闭合。

## 晋级到 Beta 的缺口

1. ~~golden 测试~~ —— 已入库：`tests/golden/` fixture（BLAKE3 锁定字节）+ 结构化
   期望输出比对，任何 canonical 输出漂移即失败（2026-07-26）。
2. ~~property/fuzz 覆盖~~ —— 已入库：固定种子确定性 property 套件（畸形行、
   Unicode 多字节 span、大字段、threading、codex 镜像去重），失败可由种子复现（2026-07-26）。
3. ~~source span~~ —— 已入库：schema v6 + `MessageEvent.span` 契约，golden/e2e
   round-trip 锁定（2026-07-26，见 `docs/operations/migration-v5-to-v6.md`）。
4. ~~真实历史数据回归（隔离沙箱、授权数据集、不外传）~~ —— **已闭合**：最新全量
   授权运行（`2026-08-13T00:26:57Z`，改名后的 `agent-session-grep` 二进制，1,330
   源、1,255,049,984 字节）六条不变量全绿、harness exit 0：sync 180,718 emitted /
   0 skipped、no-parse-loss 180,718 claims、242 sessions 全 context 成功（11
   zero-placement，0 failed）、630/630 byte 精度、rebuild 稳定（catalog 166,882 →
   166,882，ids match）。此前运行均如实保留：2026-08-09/10 全量（1,242 源、164,136
   emitted、231 sessions、659/659 byte、rebuild 151,562 → 151,562）、2026-08-12
   v3（1,328 源、180,218 emitted、rebuild 166,380 → 166,380）、2026-08-10 子集
   （137 源）与 2026-07-31 失败运行（exit 5）见
   `docs/evidence/integration-beta/real-data-regression.md`。harness
   （`scripts/evidence/real_data_regression.py`）在抛弃式临时 data root 上跑
   sync → status + catalog walk → 逐会话 context → index rebuild，报告只含聚合计数
   （见 `docs/operations/REAL-DATA-REGRESSION.md`、证据行 `IB-REAL-DATA-REGRESSION-001`）。
   真实数据 Gate D 已闭合；Provider 晋级仍需独立审查与 owner 决策。
5. 跨正式 target（Windows/Linux/macOS）的 CI 认证——仍缺。`ci.yml` 的 `test` 与
   新增 `installer` job 已配置三平台矩阵（证据行 `IB-CI-INSTALLER-001`），但在
   PR 上跑绿并记录具体 run id 之前只能是 `ci_configured_only`；hosted runner 亦
   非 clean machine，不构成安装认证。
