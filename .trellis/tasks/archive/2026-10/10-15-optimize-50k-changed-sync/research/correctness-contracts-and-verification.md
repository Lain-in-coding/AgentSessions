# Research: 必须保持的正确性契约、兼容性边界与验证清单

- Query: 优化提交路径时，哪些不变量/磁盘与 CLI 契约不能被破坏？每一条由什么测试或 oracle 守住？schema 变更（如需）要满足什么？
- Scope: internal（spec + 代码走读 + harness oracle）
- Date: 2026-10-15

## Findings

### 1. 不变量与现有守门测试

| 契约 | 权威位置 | 机制 | 守门测试 |
| --- | --- | --- | --- |
| Catalog 是权威、FTS/侧车是可重建投影 | `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md`（核心不变量） | `fts`/`fts_ids`/`session_fts` 只从 catalog + claims 派生 | adapter 套件、`index rebuild` 路径测试 |
| 两阶段 outbox：`begin_index_batch`（durable intent）→ 单事务 `commit_index_batch_with_relations`；崩溃只留无副作用 `building` 行 | 同上（Pre-Development Checklist） | `index_batches.state` + `recover_interrupted`（S:7958） | outbox/CAS/generation 测试、durable-intent tamper 测试、`invariant_failure_semantics.rs` |
| generation 推进与 activations 在同一事务（CAS on `store_metadata`） | 同上 | `verify_pending_in_tx`（S:6339） | generation CAS 测试、journal retention 的 CAS apply |
| durable intent 一致性：调用方 manifest ↔ `index_batches` 行的 5 个明细列逐字段相等 | spec "Batch-scoped commit state"（受理的取舍：不再从输入重算） | S:6423-6430 的字符串比较 | tamper 拒绝测试（篡改 DB 行必须报错） |
| 关系完整性/tombstone：只有 `relation_complete` 扫描能推导删除；不完整扫描 union 观察、清 marker | spec "SQLite v7 relation commit" | S:4202-4368 | relation completeness 测试、e2e shrink/tombstone（`e2e.rs`） |
| 跨源实体合并 alias 语义（union + 发散值置 null；singular alias 是兼容垫片） | spec "Cross-source entity merging" | `merge_*_payloads`（S:248/456）、`regenerate_compatibility_aliases_in_tx`（S:5576） | 跨源合并测试、多批等价 oracle |
| 身份保真：`fts_ids.id_json` 优先，`StableId::from_wire` 只降级 Unstable | spec Pre-Development Checklist | `ensure_stored_identity_metadata_matches`（S:3405） | identity fidelity 测试（含跨批 metadata 冲突 fail-closed） |
| Resume Metadata 隐私与 fail-closed（冲突不泄露任何值） | spec "Resume Metadata claims (ADR-0009)" | `resolved_session_claim`（S:6609）等 | `resume` 相关测试、e2e |
| Journal retention v19：未决行永不聚合；terminal 行聚合 `[] + detail_summary_json`；未知格式 fail-closed；CAS apply | spec "Journal retention compaction (schema v19)" | `preview/stage/apply/recover_journal_compaction` | `journal_retention.rs`（pin 数字/soak/killed-stage/drift/v18 迁移/未知格式） |
| 迁移 additive、按 `user_version` 门控、不得 drop/recreate；旧二进制拒绝更新 schema | spec Pre-Development Checklist | `migrate`（S:1859 起）、`SCHEMA_VERSION = 19`（S:8851） | 迁移往返测试、`SchemaIncompatible` 测试 |
| 源只读：适配器不回写 provider 源文件 | spec Pre-Development Checklist | CLI/解析层 | e2e/源指纹测试 |
| 同批语义：多批拆分 vs 单批必须产生相同 catalog/projection/relation 行 | spec "Batch-scoped commit state" §6 | `commit_source_batches_if_changed` | 25 表 equivalence oracle（`research/scripts/verify_equiv*` 系） |

### 2. 复测 harness 自带的正确性 oracle（R2/AC2 的"合成语料 census/关系/Session/FTS/生成号"）

基线的 baseline 扩展在 `run` 内对每个 phase 做快照与校验（`scripts/evidence/core_beta_benchmark.py`）：

- `sync_check(frame, files, messages, phase)`（:610）校验每次 sync 的 Robot frame 计数（sources/messages/committed/unchanged/generation 等）。
- `SyntheticOracle.scan`（:625-720）：只读打开 store，做未计时 census——documents/sessions/placements 数量与内容、`source_scans` 与 source 指纹/字节数、bindings 摘要；`validate_report` 用 `expected_census` 精确比对（:2053-2111），并要求 immediate no-op 的 bindings 不变。
- 查询侧：search 每个 query 必须命中（:419 一带），show/get 使用保留集 id。
- `validate-report`（:2334）逐份重算统计/校验哈希/样本数；`validate-baseline`（subparser :2575；实现 :2294）要求三份都通过并输出 run 间比较。

因此"优化不改变正确性"的最低证明 = 三份新报告全部通过 validate-report + validate-baseline，且 census/bindings/帧计数字段与基线一致。

### 3. 若采用 C1（journal 明细拆分）的 schema/兼容性检查表

- 版本：`SCHEMA_VERSION 19 → 20`，新增 `migrate_v19_to_v20`（单一事务、additive；失败回滚；`PRAGMA user_version` 最后写）。
- 读兼容：`detail_format='full'` 的历史行必须继续可读（重组路径 or 直接列路径二选一，未知值 fail-closed 报 `SchemaIncompatible`）。
- 写路径：Phase A 写明细（父行 + 分片）；Phase B 的 `UPDATE` 只触及小行；`verify_pending_in_tx` 的比较语义保持"存储字节 == 调用方 manifest"（可读回重组后比较）。
- B2/维护接口：`preview/stage/apply/recover_journal_compaction` 与 `journal_retention.rs` 的字节口径（含/不含审计行）必须同步适配并在文档中披露口径。
- 回滚：新二进制写出的 v20 库被旧二进制打开时拒绝（既有行为）；任务内回滚 = revert 代码 + 保留迁移测试覆盖，不做降级写。
- 审计披露：任何"明细已聚合/分片"的磁盘形态变化都要在 R6 的跟进记录里写明版本与回读条件。

### 4. 若采用 C2/C3/C4/C5（纯查询/装载/分块调整）的检查表

- 不改变任何写入集合：复测前后同一语料的 25 表 row-content hash 相等（等价 oracle），且 `active_generation` 一致。
- 不改任何 schema/user_version/磁盘格式；不新增依赖；不改 CLI flag/退出码/错误语义。
- 语句语义等价论证留在 PR 描述（例如 C2 的"读先于写、结果集合相同"、C3 的"第一次校验覆盖同一 id 集"）。

### 5. 质量门（R6/AC2）

- `cargo fmt --all --check`；`cargo clippy --workspace --all-targets -- -D warnings`；`cargo test --workspace`。
- 相关集成：`cargo test -p agent-session-grep-cli --test e2e`（sync/noop/shrink/tombstone、multi-session）、adapter 的 `journal_retention`/`invariant_failure_semantics`/`process_evidence`（若受影响）、MCP/serve 相关既有测试脚本。
- 依赖/治理：`cargo deny`/`cargo audit`（如仓库门要求）、隐私扫描（`scripts/evidence/privacy_scan.py`）、提交/PR 治理检查（`scripts/governance`）。
- 证据：`python -B scripts/evidence/core_beta_benchmark.py validate-report` ×3 + `validate-baseline` ×1 全绿。

## Caveats / Not Found

- `research/scripts/verify_equiv*.py` 是 upstream spec 提到的历史等价脚本；本研究未在本仓库定位到 `research/` 根（可能在产品仓库历史或私有 runtime），实现阶段需就近重找或重建等价检查（25 表 + generation）。
- 本文未穷举 CLI/Robot/MCP 的每个输出字段；R2 的"输出语义不变"以"提交路径改动不触及输出装配层 + 全量测试全绿"论证，若实现触及应用层装配，需要补充逐字段审查。
