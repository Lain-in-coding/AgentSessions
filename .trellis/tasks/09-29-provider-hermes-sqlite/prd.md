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

- [ ] probe 判别：SQLite 变体与 JSON 变体互斥；歧义/缺表/坏 schema 显式拒绝。
- [ ] parse/search golden：排序、profile 隔离、两种 tool-call 形态、NULL 时间戳/正文显式保留。
- [ ] 源不可变：DB/WAL/SHM 字节与元数据在 probe/parse 后不变；固定读事务下并发提交不改变观测。
- [ ] 有界预算：行/cell/总字节/工具数上限触发显式错误；坏行计入诊断而非静默为零。
- [ ] 现有 16 provider golden 与 workspace 测试无回归。
- [ ] PROVENANCE + 固定上游引用齐备；未认证项在矩阵中如实标注。
- [ ] 若 beta 证据门未全绿：保持 experimental 并提交缺口清单，不做口头晋级。

## Notes

- 研究探针与证据：归档 `research/experiments/providers/`（35/35 合成用例）与 `research/upstream-evidence.md`（schema v30 行号）。
- 当前矩阵：Hermes `Experimental`、`resume=unknown`、`source_span=unsupported`；新变体是新增格式面，不改变既有变体行为。
