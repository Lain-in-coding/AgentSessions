# Hermes SQLite state 变体（`hermes/sqlite-state-v1`）

## Goal

在现有 `hermes/session-json-v1` 之外新增 `hermes/sqlite-state-v1` 变体：读取 `~/.hermes/state.db` 与 `~/.hermes/profiles/<name>/state.db`，覆盖真实 SQLite 会话格式。目标成熟度 beta——只有证据门全绿才提交晋级建议，否则如实保持 experimental 并记录缺口。

## Requirements

- 变体判别：按 schema（sessions/messages 列）识别，与 JSON 变体互斥；歧义必须显式拒绝，不得猜测。
- 读取：沿用现有 SQLite 只读快照约定（单次固定读事务；不得用竞态三文件复制）；全部输入有界（行/cell/总字节上限），超限显式失败而非截断后当成功。
- 映射：`sessions(id, source, model, title, started_at REAL 秒, ...)`；`messages(role, content, tool_calls, tool_call_id, tool_name, reasoning, timestamp REAL)`；排序 `timestamp, id`；profile 进入安装/来源命名空间（跨 profile 同名 native id 不得合并）。
- 身份：原生整数消息 id 原样保留；不得伪造 Stability/Native 证明（沿用本项目身份规则）。
- 工具调用：兼容精简 `{name,arguments}` 与 OpenAI `{id,function:{...}}` 两形态；歧义或未匹配关联一律非权威（`authoritative: false` 语义），不得合成原生 call id。
- 源只读：probe/parse 不得写源 DB/WAL/SHM；并发提交下观测一致。
- 不新增 resume 命令（矩阵 `resume=unknown`），不宣称 resume 能力。
- fixtures 全合成 + PROVENANCE，引用固定上游证据（NousResearch/hermes-agent schema v30 快照）；禁止真实 transcript。
- 成熟度只按 `.trellis/spec`/`docs/product/PROVIDER-MATURITY-MATRIX.md` 的证据规则变更；beta 晋级由 owner 在阶段评审决定。

## Acceptance Criteria

- [x] probe 判别：SQLite 变体与 JSON 变体互斥；歧义/缺表/坏 schema 显式拒绝（`src/sqlite_state.rs:1113-1164`、`tests/sqlite_golden.rs:181`、跨 provider 唯一性 `provider_probe_isolation`）。
- [x] parse/search golden：排序、profile 隔离、两种 tool-call 形态、NULL 时间戳/正文显式保留（`tests/sqlite_golden.rs` pin `sqlite.expected.json` BLAKE3 `2a21db8b…`；`crates/agent-session-grep-cli/tests/hermes_state_db.rs:85,129`）。
- [x] 源不可变（按修正后口径）：adapter 从不打开源文件、只在私有副本上单次固定读事务；DB/WAL 字节在 probe/parse 后不变、并发 WAL 提交不改变观测（`src/sqlite_state.rs:1460-1530`）。**限定说明**：整链路 `sync` 时既有 capture 层以只读挂载 live WAL 库会写 `-shm` 读标记（非本 adapter 行为），已在 `tests/golden/PROVENANCE.md` 与 spec 如实记录并在 spec 将断言口径限定为 database/WAL。
- [x] 有界预算：行/cell/总字节/工具数上限触发显式错误（`RecordTooLarge`/`SourceTooLarge`），坏行计入诊断而非静默为零（`src/sqlite_state.rs:1336-1439`；生产实测 rc=7，无截断）。**限定说明**：生产选择层先按 32 MiB 整源上限拒绝，adapter 内部 128 MiB 生产不可达 —— 已如实写入 PROVENANCE、manifest known_limitations 与 readiness 缺口。
- [x] 现有 16 provider golden 与 workspace 测试无回归（`provider_matrix` 35 passed、`provider_probe_isolation` 3 passed；workspace 82 suites / 1807 passed / 0 failed）。
- [x] PROVENANCE + 固定上游引用齐备（合成 `state.db` 由 `generate_sqlite_fixture.py` 生成、无真实 transcript；上游 `bac0c45d…` / blob `e35e61a0…`）；未认证项在矩阵中如实标注（`PROVIDER-MATURITY-MATRIX.md` 未被修改）。
- [x] beta 证据门未全绿 → 维持 experimental 并提交缺口清单，不做口头晋级（`docs/product/PROVIDER-BETA-READINESS.md` 记录 6 条缺口 + 3 条 follow-up 决策项）。

## Verification

- 提交：`c8a2f56` 实现 → `e9f01f6` 文档 → `ecb9783` 核查后修正（WAL 清理证据 + 文档口径）。
- 独立核查：verdict **PASS-with-findings**（5 条非阻断，无回归）；F1（PROVENANCE 覆盖表与 pinned 投影矛盾）、F2（有效上限 32 MiB）、F3（spec 要求的 WAL/SHM 清理证据缺失 + spec 未含第三个 SQLite provider）、F5（capture 层 `-shm` 副作用口径）已在 `ecb9783` 修正；F4（计数型上限复用 "bytes" 文案、>32 MiB 时选择层掩盖真实拒绝原因）作为 owner 决策项记录，未改共享文案。
- 关键设计决策（需 owner 复核）：① 消息不采用每次库的 rowid 作 canonical native id（跨 profile 会误合并；rowid 仅进诊断），rowid 采用与否依赖 document-scoped native message identity 决策；② 工具关联 fail-closed（产品侧无非权威关联表达位）→ 不产 `ToolActivityEvent`、不合成 call id。
- 已知限制（已钉住，非本任务修复）：先 sync 主库 `state.db` 会吸收其后更深的 `profiles/<name>/state.db` 源（`known_limitation_main_database_absorbs_a_profile_source`），profile↔profile 之间不合并。

## Notes

- 研究探针与证据：归档 `research/experiments/providers/`（35/35 合成用例）与 `research/upstream-evidence.md`（schema v30 行号）。
- 当前矩阵：Hermes `Experimental`、`resume=unknown`、`source_span=unsupported`；新变体是新增格式面，不改变既有变体行为。
