# Journal 保留合同（B2 / D1 有约束治理）

- 日期：2026-10-06。父任务：`.trellis/tasks/10-05-competitive-source-audit-plan`（D1 已批准：有约束治理）。
- 范围：`agentsessions-adapters-sqlite` 的 durable outbox（`index_batches`）明细。catalog、FTS、关系表与活跃 generation 不在本合同内（本合同不改写它们）。
- 实现镜像：`crates/agent-session-grep-adapters-sqlite/src/lib.rs` 的
  `JOURNAL_COMPACTION_FIELDS` / `JOURNAL_DETAIL_FORMAT_*` / `JOURNAL_COMPACTION_PLAN_VERSION`
  与 `SqliteStore::{preview,stage,apply,recover}_journal_compaction`。文档与常量不一致时以常量为准并回改本文。

## 1. 不可妥协条款（D1）

1. 未决记录不可删：`building` / `search_built` / `cleanup_pending` 行的全部明细永久保留，仍参与恢复与 CAS 校验。
2. catalog / FTS / 关系 / 活跃 generation 必须原子一致：compact 只改 `index_batches` 的明细列，不进入这些表的写集合，也不推进 generation 水位。
3. 可验证摘要：被聚合的内容必须留下字节级承诺，审计方可核对摘要未被伪造。
4. 审计损失边界可解释：丢失什么、保留什么、谁能验证，逐字段写明（见 §3/§4）。
5. 禁止：删除整行/整个 outbox、静默丢日志、把失败记录当 terminal 处理、以"库没空间"为由无解释丢历史。

## 2. 状态与资格

| 状态 | 语义 | 明细处置 |
|---|---|---|
| `building` / `search_built` / `cleanup_pending` | 未决/待收敛 | **永久保留**（不进入任何聚合计划） |
| `activated` | 已激活的 generation 变更 | terminal，可聚合 |
| `aborted` | 崩溃/失败收敛（无副作用） | terminal，可聚合 |
| `superseded` | 被更新基线取代 | terminal，可聚合 |
| 其他（未知） | 本二进制不认识 | **fail-closed**（`SchemaIncompatible`，不解释、不跳过） |

另有一条收益约束：**聚合无收益的行保持 `full`**（明细字节 ≤ 摘要字节时跳过）。有约束治理只做减法。

## 3. 逐字段保留合同表

`index_batches` 行字段：

| 字段 | 角色 | 行为消费者 | 处置 | 损失边界 |
|---|---|---|---|---|
| `operation_id` | batch 身份（主键；`installation_relocations` 外键） | 恢复/诊断/relocation 审计 | 永久保留（逐字节不变） | 无损失 |
| `base_generation` / `target_generation` | 代数边界 | `verify_pending_in_tx` CAS、诊断 | 永久保留 | 无损失 |
| `state` | 状态机 | `recover_interrupted`、`interrupted_batch_count`、聚合资格 | 永久保留 | 无损失 |
| `operation_digest` | 完整 manifest（含 payload/正文哈希、关系与 source 替换）的承诺 | 幂等/审计/测试快照 | 永久保留 | 无损失（聚合不改写） |
| `durable_point` / `created_at_ms` / `committed_at_ms` / `error_code` | 恢复点与审计 | 恢复、doctor、诊断 | 永久保留 | 无损失 |
| `relocation_json` | relocation manifest（搬迁审计一手证据） | pending 行 `verify_pending_in_tx`；终端行仅审计 | **永久保留**（刻意不进聚合） | 无损失 |
| `upsert_ids_json` / `delete_ids_json` | 本批目标实体 id 集 | 仅 pending 行的 `verify_pending_in_tx`（提交前校验）；terminal 行无行为消费者 | terminal：可聚合；未决：永久 | 丢失逐 id 列表；保留元素个数 + `detail_digest` + `operation_digest` |
| `relation_upserts_json` / `relation_deletes_json` | 关系 manifest（placement/edge/activity/usage 的完整投影） | 同上 | terminal：可聚合；未决：永久 | 丢失逐条关系明细；保留计数 + 承诺 |
| `source_replacements_json` | source 替换 manifest（entity membership、placement/activity/usage claims、resume claims、scan 长度/指纹/provider） | 同上 | terminal：可聚合；未决：永久 | 丢失逐条 claim/membership 与 resume 声明正文；保留计数 + 承诺 |
| `detail_format`（v19 新增） | 布局版本标记：`full` / `aggregated_v1` | 读路径与聚合资格 | 永久保留 | 无损失；未知值 fail-closed |
| `detail_summary_json`（v19 新增） | 聚合后的可验证摘要 | 读路径、诊断、审计 | 仅 `aggregated_v1` 行 | 见 §4（它是被聚合内容的全部残余证据） |

聚合摘要（`aggregated_v1`）字段：

| 摘要字段 | 含义 |
|---|---|
| `format` | 固定 `aggregated_v1`（未知值 fail-closed） |
| `compaction_id` | 产生该摘要的计划 id（`journal_compactions` 主键，可追溯） |
| `items` | 五个字段各自的 JSON 数组元素个数（占位 `[]` 不代表事实） |
| `bytes` | 五个字段聚合前的字节数（每行损失了多少体积） |
| `detail_digest` | 五个字段原始文本的域分隔 blake3 承诺（`journal-detail-v1`） |

`journal_compactions`（计划/审计表）：

| 字段 | 含义 |
|---|---|
| `compaction_id` | 计划身份（`cmp_v1_<ms>_<pid>_<seq>`） |
| `state` | `staged` / `committed` / `abandoned` |
| `plan_json` | 版本化计划（`JOURNAL_COMPACTION_PLAN_VERSION = 1`）：逐行状态、代数、明细承诺与规模 |
| `plan_digest` | 计划承诺（stage/apply 的 CAS 依据；不含时钟，可重现） |
| `affected_batches` / `detail_bytes_before` / `detail_bytes_after` / `saved_bytes` | 保留合同的可解释规模（精确值） |
| `created_at_ms` / `resolved_at_ms` / `reason` | 审计轨迹（放弃原因固定为 stale-preview 文案，无路径/id/内容） |

## 4. 审计损失边界（聚合 terminal 行之后）

**丢失（仅存在于 compact 前备份）**：逐 upsert/delete id 列表、逐 placement/edge/activity/usage 明细、source replacement 的逐条 membership/claim 与 resume claims、五个列的原始文本字节。

**保留（活库内）**：批次身份、代数边界、状态、时间与错误码、`operation_digest`（对完整 manifest 的承诺）、`detail_digest`（对被替换五个列原文的承诺）、逐字段元素个数与字节数、`relocation_json`、计划/审计行。

**可验证性**：持有 compact 前备份的一方按 `journal-detail-v1` 域重算五个列文本的 blake3 与摘要中的 `detail_digest` 比对，即可证明摘要未被伪造；`operation_digest` 另行承诺完整 manifest（含 payload/正文哈希），两者叠加构成审计链。

**不适用/未决**：pending 行任何时候都不丢弃明细；`relocation_json` 不聚合；本任务不提供"导出/还原丢失明细"的工具（Stage-2 不在本波交付内）。已知成本：`journal_compactions` 每次显式维护新增一行（KB 级、随维护次数线性），本波不回收它——它是审计轨迹，且增长由操作者显式触发而非 sync 热路径。

## 5. 布局版本与 fail-closed 矩阵

| 情况 | 行为 |
|---|---|
| 行无 `detail_format`（v18 旧格式） | 迁移回填 `full`（旧格式可读），参与后续聚合 |
| `detail_format` 为未知值 | `index_batch()` 与 preview 报 `SchemaIncompatible`；零改写 |
| `aggregated_v1` 缺摘要/摘要不完整/字段集合不符 | `SchemaIncompatible` |
| `full` 行却带摘要 | `SchemaIncompatible` |
| `plan_json` 不可读/版本未知 | apply 报 `SchemaIncompatible`，计划保持 `staged` |
| preview 之后 journal 变化 | stage / apply 报 `GenerationMismatch`，零改写 |
| 计划内行消失、状态变化或明细承诺不符 | `GenerationMismatch`，整事务回滚 |
| 聚合无收益的行 | 保持 `full`（不进计划） |

## 6. 维护操作（默认不自动执行）

```rust
pub fn preview_journal_compaction(&self) -> PortResult<JournalCompactionPreview>;
pub fn stage_journal_compaction(&self, preview: &JournalCompactionPreview)
    -> PortResult<JournalCompactionStage>;
pub fn apply_journal_compaction(&self, compaction_id: &str) -> PortResult<JournalCompactionOutcome>;
pub fn recover_journal_compactions(&self) -> PortResult<JournalCompactionRecovery>;
pub fn journal_compaction_event(&self, compaction_id: &str) -> PortResult<Option<JournalCompactionEvent>>;
```

- **preview**：只读。展示将被聚合的内容（逐批操作 id/状态/代数/逐字段规模与承诺）、受影响记录数、预计体积收益（摘要是确定性的，收益为精确值）、被聚合字段清单。为计算承诺它会把 terminal 行的五个明细列读入内存——这是显式维护操作，不是热路径；盘上字节不变。
- **stage**：把预览固化为 durable `staged` 计划（单条 INSERT，不碰明细）；事务内重算 `plan_digest` 做 CAS，过期即拒。
- **apply**：单事务逐行 CAS（状态、`operation_digest`、`target_generation`、明细承诺四项一致）→ 改写五列为 `[]` + 写摘要 → 标记 `committed`；已提交计划重入是幂等空操作。不推进 `active_generation`（水位不变）。
- **recover**：显式收敛仍 `staged` 的计划（提交 / 幂等确认 / 漂移则 `abandoned` 并计数）；未知格式向上报错不放弃。
- **open 路径不自动 compact**：`open_for_write` 只做既有 `recover_interrupted`（building → aborted）与投影自愈。

## 7. 中断、重入与并发

- **中断**：stage 与 apply 各自是独立 durable 边界。apply 是单事务，崩溃只会整段回滚；stage 之后被杀（或 apply 失败）留下 `staged` 计划，重跑 `recover_journal_compactions()` 收敛。
- **重入**：`apply` 对已提交计划幂等；`recover` 对 plan 集合逐条收敛；漂移计划显式 `abandoned`（原因入审计行），不改写任何明细。
- **并发读**：SQLite WAL 快照下读者只会看到 compact 前或 compact 后的一致视图，绝不看到半聚合；写者由既有 data-root writer lease 独占。
- **generation 水位**：compact 不触碰 `store_metadata`；pending 句柄的 CAS 与重放结论不变。

## 8. 实测（合成数据，coverage harness 见 `tests/journal_retention.rs`）

父实验基线（`journal-growth.json`，200 消息固定集合、21 次单条改写）：
`journal_manifest_chars` 194,315 → 4,739,455（21 次），store 含侧车 1.37MB → 6.07MB。

本波测试 `soak_200_messages_21_revisions_reports_sizes_and_keeps_semantics`
（同一形状的合成 harness：202 实体 + 200 placement / 批次，21 批全部 activated）：

| 指标 | compact 前 | compact 后 |
|---|---|---|
| 五个可聚合列字符量 | 2,444,589 | 210（21 × `[]` 占位） |
| 摘要列（`detail_summary_json`） | 0 | 9,114 |
| journal 明细合计（五列 + 摘要） | 2,444,589 | 9,324（保留 0.381%） |
| `journal_compactions` 审计行（`plan_json`，本 soak 为 1 条） | 0 | 12,103（随显式维护次数线性） |
| 计入审计行后的全部保留字符 | 2,444,589 | 21,427（保留 0.876%） |
| store 字节（含 wal/shm 侧车） | 3,346,432 | 3,346,432（未 VACUUM，不增长） |
| `PRAGMA freelist_count`（页） | 28 | 613–614（约 2.4MB 可复用空间，逐次运行小幅浮动） |

同时断言：21 条的 `operation_id`/`operation_digest` 不变、generation 仍为 21、catalog 202 行、最新改写的唯一词元仍可检索、旧改写标记不再可检索、apply 重入幂等。其余证据：
中断（子进程 stage 后被杀）→ `recover` 收敛；漂移的 staged 计划 → `abandoned`（原因入审计行、明细零改写，apply 拒绝且回滚到 staged）；并发读快照隔离；v18 旧格式迁移后可读可聚合；未知格式在读/preview/apply 三处 fail-closed（未决行与已聚合行无豁免）；无收益 terminal 行保持 full；未决行不被聚合且可继续 CAS 提交；冲突投影在 compact 前后拒绝一致；重放可观察结果在 compact 前后一致。

## 9. 非目标与 Stage-2

- 不做自动/周期 GC；不新增默认触发的维护动作；不改 sync 对外语义；不动 FTS/catalog 结构。
- Stage-2（另任务）：CLI 表面命令（`journal compact --preview/--apply` 之类）、自动策略（阈值/周期/预算）与人可读预览渲染，需要在 CLI 层复用本文件的 API 与合同数字口径。