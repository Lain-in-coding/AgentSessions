# Provider Maturity 与 Capability Matrix

> 对外可见的 Provider 状态清单，是 `0.3 Integration Beta` 的公开状态记录。
> - 术语与晋级证据要求见 `../architecture/RFC-0002-provider-adapter-contract.md` §6。
> - 本文件是**当前实现状态**的事实记录，不是承诺；晋级必须有证据，不由代码存在自动推断。
> - 最后更新：2026-08-16（16-provider evidence wave，`08-15-sixteen-provider-evidence-wave`）
> - 权威数据源：`crates/agent-session-grep-ports/src/capability.rs` 的
>   `ProviderCapabilityMatrix::current()`；本表与其保持一致，不一致以 capability.rs 为准。

## 术语

**整体成熟度**（Provider 级，独立于字段能力）：

| maturity | 含义 |
|---|---|
| `Experimental` | 可发现、可 probe、可解析小 fixture，限制明确 |
| `Beta` | 主路径 fixture、golden、contract、只读、增量、source span 均通过 |
| `Certified/GA` | 历史 variant、混合版本、未知字段、崩溃恢复、正式 target、性能与回滚证据齐备 |
| `Unsupported` | 明确不支持，附原因（含 deferred：无 transcript 证据，不实现不宣传） |

**字段能力**（capability，逐字段）：`native`（provider 原生提供）、`derived`（由内在内容确定性派生）、`partial`（部分场景可得）、`unsupported`（该 provider 无此概念）、`unknown`（尚未评估）。

## 成熟度总览（16 行，2026-08-16 事实）

| Provider | provider_id | variant | maturity | 证据 |
|---|---|---|---|---|
| Claude Code | `claude-code` | `claude-code/jsonl-v1` | **Experimental** | 单元 + e2e + golden（`crates/agent-session-grep-provider-claude/tests/golden.rs`）+ 确定性 property 套件（`tests/properties.rs`，固定种子）+ span round-trip |
| Codex | `codex` | `codex/rollout-jsonl-v1` | **Experimental** | 单元 + e2e + golden（`crates/agent-session-grep-provider-codex/tests/golden.rs`）+ 确定性 property 套件（含镜像去重性质）+ span round-trip |
| Grok Build | `grok-build` | `grok-build/acp-updates-v1` | **Experimental** | ACP `updates.jsonl`（`session/update` stream），主格式证据充分，variant 分层 |
| Antigravity | `antigravity` | `antigravity/transcript-jsonl-v1` | **Experimental** | 本机真实格式核验（2026-08-15）：`brain/<uuid>/.system_generated/logs/transcript.jsonl`；identity 在目录名，文件内无 session id 字段 |
| OpenCode | `opencode` | `opencode/sqlite-v1` | **Experimental** | `opencode.db` SQLite（session/message/part 表），只读打开（SQLITE_OPEN_READONLY + busy_timeout） |
| Pi | `pi` | `pi/session-jsonl-v1` | **Experimental** | session JSONL（`type:session` header + message），path override env 待补 fixture |
| Hermes | `hermes` | `hermes/session-json-v1` | **Experimental** | `~/.hermes/sessions/session_<id>.json`（session_id/messages），hstry@88b78b1 (MIT) 格式证据 |
| Cursor | `cursor` | `cursor/vscdb-chat-v1` | **Experimental** | `state.vscdb` SQLite KV（ItemTable `chatdata`/`prompts` key），hstry@88b78b1 (MIT) 格式证据，多代格式分层待补 |
| Kimi Code | `kimi-code` | `kimi-code/wire-jsonl-v1` | **Experimental** | wire.jsonl（`context.append_message`） |
| OpenClaw | `openclaw` | `openclaw/session-jsonl-v3` | **Experimental** | v3 JSONL header + message records，本机仅 config 无 transcript 样本 |
| Qoder | `qoder` | `qoder/transcript-jsonl-v1` | **Experimental** | JSONL（`session_meta` + `type:user/assistant`），官方路径已实现 |
| Tencent CodeBuddy | `tencent-codebuddy` | `tencent-codebuddy/cli-jsonl-v1` | **Experimental** | CLI OpenAI-style JSONL（`role`/`content`/`sessionId`），extension variant 待分层 |
| Cline | `cline` | `cline/api-conversation-history-v1` | **Experimental** | `api_conversation_history.json` JSON family |
| Aider | `aider` | `aider/chat-history-md-v1` | **Experimental** | Markdown chat history（`#### ` user prompts），`.aider.chat.history.md` 为候选 root 待核验 |
| DeepSeek Harness | `deepseek-harness` | — | **Unsupported（deferred）** | 无任何 transcript 证据（本机无 `~/.deepseek`，参考项目无 adapter）；决策见 `deferred-deepseek-zcode.md` |
| ZCode | `zcode` | — | **Unsupported（deferred）** | 无任何 transcript 证据（本机无 `~/.zcode`，参考项目无 adapter）；决策见 `deferred-deepseek-zcode.md` |

14 个已实现 provider 均为 **Experimental**：golden、property、source span 以及
关系化 Message/Placement/Edge 的合成与 e2e 证据已入库（见下），剩余 blocker
仍为授权真实数据全量绿色回归与跨 target CI 认证。两个 deferred provider
（DeepSeek Harness、ZCode）保留 16 行但不宣传为已实现、不设 maturity target。

## Capability Matrix（逐字段，2026-08-16）

字段对应 Canonical `Message`（`crates/agent-session-grep-domain/src/lib.rs`）与解析产出。

### 已实现 14 个 provider 的共同字段

| 字段 | 已实现 provider | 说明 |
|---|---|---|
| probe | `native` | 全部 14 个 adapter 均有 probe，歧义一律 `AmbiguousVariant` 拒绝（不低置信度猜测） |
| parse | `native` | 全部 14 个 adapter 均 streaming 到 `CanonicalEventSink` |
| search | `native` | 统一经 canonical 索引检索 |
| discover | `native`（claude-code/codex）；`unsupported`（其余） | 仅 claude-code/codex 注册了 discovery root；antigravity/opencode 已加入 `provider_data_root` |
| resume | `derived`（claude-code/codex/pi/opencode）；`unknown`（grok/kimi/qoder/codebuddy/hermes/antigravity/cursor）；`unsupported`（aider/cline/openclaw） | 未核验的 resume 命令一律不设默认值 |
| context / handoff / tool_activity / incremental | `unsupported` 或 `unknown` | 属后续全能力链任务（`08-15-structured-activity-context-facets`），不在本任务范围 |
| source_span | `native`（claude/codex/grok/pi/kimi/openclaw/qoder/codebuddy）；`derived`（cline/aider）；`unsupported`（opencode/hermes/antigravity/cursor） | SQLite/目录名身份类 provider 无文件内字节 span |

### 逐 provider 明细见 capability.rs（单源权威）

```bash
# 渲染当前矩阵（JSON）：
cargo run -q -p agent-session-grep-cli -- --help   # 入口层读取 capability.rs，禁止硬编码
```

## 已知限制

- **Claude Code**：只抽取 `user`/`assistant`/`system` 对话记录；工具调用块（无 text）被忽略；`cwd`/`gitBranch`/`version` provenance 尚未落库。
- **Codex**：只取权威 `response_item` + 内层 `message`，忽略 `event_msg` UI 镜像以避免重复计数；`world_state`/`turn_context`/工具调用记录未抽取；无 threading（线性）。
- **Antigravity**：文件内无 session id 字段（identity 在 `brain/<uuid>` 目录名），parse 时 `session_native_id`/`provider_session_id` 如实留缺；`span` 用字节区间。
- **Hermes**：`session_<id>.json` 为主格式；同目录 `<id>.jsonl` 仅含部分近期状态，忽略。
- **Cursor**：`state.vscdb` 为 chatdata/prompts 两 key 的多代格式，版本分层待补。
- **OpenCode / Hermes / Kimi**：SQLite 类 provider 一律只读打开（`SQLITE_OPEN_READONLY` + `busy_timeout`），绝不写 provider 数据库。
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
