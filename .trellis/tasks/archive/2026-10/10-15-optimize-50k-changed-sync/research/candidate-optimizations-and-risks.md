# Research: 候选优化、预期收益与风险（50k changed-sync）

- Query: 在保持全部正确性契约与磁盘兼容的前提下，哪些改动有望让 50k initial/shrink 的三跑 P50 中位数下降 >15%？各自的风险、验证方式与"必须先用证据钉住"的前提是什么？
- Scope: internal（基于 `commit-path-hotspot-map.md` 的阶段证据）+ external（SQLite WAL/limits 官方文档）
- Date: 2026-10-15
- 收益数字均为**估算**（基于阶段计时、字节体积与 I/O 量级），不是承诺；实现必须先做 E1-E3 实验，再决定投入。
- 命名与既有研究的关系：本文件用 **C1–C6** 指代候选（供提交信息/PR 引用）；`constraints-and-prior-optimizations.md` 引用的 09-29 报告"路线 A–D"与之部分重叠（路线 A ≈ C4+C6；路线 C 是 alias 物化投影，**不在**本清单内）。C1 是针对 durable journal 行布局的**新**候选项，不属于 09-29 的 B/C/D；本任务 PRD R5 允许带版本化迁移与回滚路径的 schema 调整（09-29 的"本轮禁止 schema"是该任务自己的边界，不是本任务的边界）。

## Findings

### 1. 候选清单（按"预期收益 ÷ 风险"排序）

#### C1 — durable journal 明细拆分，消除 activation 的整行重写（预期 initial ≈0.7–1.2 s、shrink ≈0.4–0.8 s；风险中高）

**证据**：initial 单行 48.6 MB / shrink 33.8 MB（`50k-journal-row-diagnostic.md`）；Phase A INSERT + Phase B `UPDATE` 各写一次整行；`generation_activate` 实测 652 ms（两个 UPDATE 中主项是该行的整行重写）；提交后 freelist ≈ 旧行体积。

**设计选项**（按侵入度递增）：

1. `index_batch_details(operation_id, part, ordinal, body)` 子表，明细按 ≤64 KB 分片成多行；`index_batches` 父行只保留小列（state/durable_point/时间戳/摘要）。Phase B 的 `UPDATE` 只重写父行 → 明细只写一次。
2. 等价变体：保留 `index_batches` 全部列，但把**可变状态**（state/durable_point/committed_at_ms/error_code）拆到 `index_batch_states(operation_id PK, …)` 小表；明细继续留在父行只写一次。
3. 仅对"大明细"走子表（阈值 + 新的 `detail_format` 值，例如 `chunked_v1`），小日志保持现状不变。

**必须保持的契约**（任一选项都要覆盖）：durable intent 在 Phase B 前可达且完整；`verify_pending_in_tx` 的"调用方 manifest ↔ 存储行"逐字段一致性检查语义等价（可读回重组后比较，或改为分片逐段比较）；`recover_interrupted` 对 `building` 行的收敛不变；journal retention v19 的 `full`/`aggregated_v1`、未决行永不聚合、CAS apply、旧二进制拒绝新 `user_version`；迁移 additive + 可回滚（新 `SCHEMA_VERSION = 20`）。

**验证**：adapter 既有 outbox/CAS/generation/durable-intent tamper 测试全绿；`journal_retention.rs` 全套（preview/soak/recovery/CAS/无收益行/未知格式 fail-closed）；`recover_interrupted` 相关测试；25 表 equivalence oracle（多批等价 + shrink/tombstone）；复测报告显示 `tx_commit`/`generation_activate` 下降且无其他类别回归。

**风险**：触碰崩溃恢复/审计核心面；若实现不完整（例如激活后明细不可读）会破坏 B2 维护接口。缓解：先落地"父行 + 子表"最小双格式读取（旧 `full` 行照旧可读），再切换写侧。

#### C2 — `affected_sessions` 批量化（预期 0.5–0.7 s，两个场景都吃到；风险低）

**证据**：S:6876-6912 对每个 message/placement upsert 以 1 元素 slice 调用 `collect_message_sessions`（函数本已按 500 分块）；trace 实测 721 ms / 880 ms。

**做法**：循环内只收集去的 message id（`BTreeSet` 去重；placement 侧用 `placement.message_id`），循环外一次 chunked `SELECT DISTINCT session_id … WHERE message_id IN (…)`。placement 的旧 owner 收集（`collect_placement_sessions`，S:6441）本就是批量调用，无需改。

**语义论证**：全部查询发生在任何写入之前（当前代码同样如此），结果集合与逐条调用完全一致（DISTINCT 并集）；唯一差别是语句数与准备开销。需覆盖"placement 移动 → 旧 Session 必须仍被纳入 affected"的既有测试。

#### C3 — 删除重复的身份校验（预期 0.2–0.25 s；风险低）

**证据**：`validate_incoming` 已对每个源 entries 校验（S:3671-3716 循环内 `ensure_stored_identity_metadata_matches`）；`manifest` 阶段 S:4428 对 upserts（同一 id 集）再校验一次；函数是每实体 1 次 `fts_ids` 点查（S:3405-3431）。

**做法**：去掉 S:4428 的第二次调用，或改为"仅对未在 validate_incoming 中出现过的 id"执行（当前实现下集合相同即整体跳过）。

**验证**：跨批 metadata 冲突用例（既有测试）必须仍然 fail-closed；加一个"仅第二次校验能发现的注入"负例不成立——需要证明第一次覆盖了全部 upsert id（合并只改变 payload，不改变 id 集合；重复 id 已在 `batch_manifest` 拒绝）。

#### C4 — alias 重建的批作用域化（预期 0.2–0.4 s；风险中低；09-29 路线 A 的延续）

**证据**：`stored_placements_from`（S:5417）/`stored_edges_from`（S:5549）为全表 `SELECT`+`ORDER BY`；在 50k 语料下各 5 万/4.5 万行，1M 语料下是 O(catalog) 内存/时间，与 spec 的 "batch-scoped commit state" 精神相抵（该 scenario 只点名了 `source_membership` 允许扫描但不得保留全量行）。

**做法**：把装载限制在 `fully_complete_entities` 关联的 message/session/document 候选集（`message_placements.message_id IN (…)` / `session_id IN (…)`，`message_edges.child_placement_id IN (…)`，均可走现有索引 `message_placements_message`/`message_placements_session_order`，S:2169-2175），或者复用本提交已加载的 `stored_placements`（按 placement_candidates 装载）再按实体分组。

**风险**：必须证明 alias 重建只依赖这些实体的 placements/edges（代码路径：`placements_by_message`/`placements_by_session` 仅按 entity_id 取用）；需覆盖跨源合并/发散 parent 置 null 的既有 alias 测试与 25 表等价 oracle。

#### C5 — 批量语句的行数上限提升（预期 0.1–0.3 s；风险低，但先核实编译值）

**证据**：`BULK_INSERT_ROWS_PER_CHUNK=100` 受 `8*100 ≤ 950` 断言约束（S:115、7056）；SQLite ≥3.32 默认 `SQLITE_MAX_VARIABLE_NUMBER=32766`（官方文档）。当前 SQLite 3.53.2 bundled。

**做法**：先在运行时读取连接的有效上限（`sqlite3_limit` / `PRAGMA compile_options`，rusqlite 侧确认 API），再把 placement/edge/catalog/fts/membership 分块提升到 256–512 行/块；保留编译期断言与 `in_placeholders` 逻辑。

**风险**：低（纯语句数减少）；但收益取决于"每语句固定开销 vs 每行 B-tree 成本"的比例，必须实验测，不能假设。

#### C6 — `manifest` 阶段 CPU/内存削减（预期 0.2–0.4 s；风险中；09-29 路线 A 的流式哈希部分）

**证据**：`batch_manifest` 序列化 95k 关系 + 250 源替换 ≈48.6 MB 字符串、并对 29 MB payload + JSON 做哈希（阶段 1.19 s，其中还含一次可去掉的身份校验，见 C3）。

**可能做法**：流式哈希（一边序列化一边 hash，避免整串驻留）；`upsert_ids` 免二次序列化（当前 `batch_manifest` 产出 `Vec<String>`、`begin_index_batch_with_manifest` 再 `to_string`）。任何"改变 canonical JSON 形状/摘要输入"的做法都属于磁盘格式变更，需要 `detail_format`/digest 域版本化 + 迁移说明——不建议在本任务顺带做。

#### 明确不推荐

- 关闭/降低 `synchronous`、改 `journal_mode`、把单事务拆成多事务：违反 spec 与 R5。
- `wal_autocheckpoint` 调参：总量不降（close 仍 checkpoint），只是把成本搬走；不入协议内收益。
- 跳过 alias 重建、跳过 integrity 校验、复用旧 placement id：语义/契约风险远大于收益。
- 以"过滤未变化 relation upsert"削减 shrink 的 47.5k 写入：**事实不成立**——shrink 截断导致 document/session 身份重派生，placement/edge id 全量变化（trace detail 显示 relation_deletes=95000 = 旧 50k placements + 45k edges 全部删除），无同 id 可复用。

### 2. 达标组合与失败预案

R4 门槛：initial P50 中位数 < 8604.5 ms、shrink < 8451.7 ms（三跑中位数）。组合估算：

| 组合 | initial 预期 | shrink 预期 | 说明 |
| --- | --- | --- | --- |
| C1+C2+C3+C4 | 10.12 → ≈7.6–8.5 s | 9.94 → ≈7.9–8.6 s | 最可能达标；C1 是主项 |
| C2+C3+C4+C5（放弃 C1） | ≈8.8–9.2 s | ≈8.6–9.0 s | 大概率**不达标** |

若 C1 实验收益低于 0.4 s 或风险不可控：退回 C2+C3+C4+C5，并按 PRD 的"允许以如实报告 + 保留现状结束"路径，给出后续建议（例如把 journal 明细治理并到维护任务，或做 detail 压缩的独立设计）。

### 3. 推荐先做的私有实验（不入产品提交）

- **E1 — 提交内 I/O 归因**：在 canonical checkout 里用临时 integration test/bin，驱动 `commit_source_batches_if_changed` 提交 50k 合成批次（用 harness 的 `synthetic_record`/`write_source` 逻辑生成到私有 scratch），在**同一进程内**提交前后读取 `-wal` 大小、`page_count`/`freelist_count`，并开 `ASG_INDEX_TRACE`；另一侧禁用 autocheckpoint（仅实验）观察 `tx_commit` 变化。目的：把 tx_commit 拆成 WAL 写、fsync、checkpoint 三块，验证 C1 的字节收益假设。
- **E2 — journal 子表原型**：在实验分支实现最小读侧重组（不改产品行为），测 initial/shrink 的 `generation_activate`/`tx_commit`/WAL 大小差异；确认收益 ≥0.4 s 再进入正式实现。
- **E3 — C2/C3/C4 微基准**：分别打补丁后跑 50k initial/shrink 各一遍（非正式三跑），确认单项收益方向与量级；注意 trace 插桩本身的偏移，比较同插桩条件下 before/after 相对值。
- **E4 — C5 上限核实**：运行时查询 bundled SQLite 的 `SQLITE_LIMIT_VARIABLE_NUMBER`，再决定块大小。

实验的 scratch、原始 trace、失败尝试保留在私有 runtime，不进入产品提交；正式复测按 `baseline-protocol-and-thresholds.md` 的协议三跑并发布复核过的聚合。

### 4. 证据绑定要求（对应 AC1）

每个最终实施的优化必须在任务 research/PRD 或 PR 描述里能指回：候选 ID（C1–C6）→ 具体 trace 阶段数字（`commit-path-hotspot-map.md` §2 表）→ 代码位置（同文件 §1 表）→ 实验 E* 的 before/after 原始记录（私有）→ 复测报告中的对应指标变化。没有实验支撑的"顺手优化"不应进入提交。

## External references

- SQLite WAL / automatic checkpoint：`https://www.sqlite.org/wal.html`（autocheckpoint 默认 1,000 页；自动 checkpoint 在触发它的 COMMIT 内、同线程执行，PASSIVE）。
- `PRAGMA wal_autocheckpoint` / `sqlite3_wal_autocheckpoint`：`https://sqlite.org/pragma.html`、`https://www.sqlite.org/c3ref/wal_autocheckpoint.html`。
- `SQLITE_MAX_VARIABLE_NUMBER` 默认值（<3.32 为 999，≥3.32 为 32766）：`https://sqlite.org/limits.html`。
- 行 UPDATE 重写整行（含 overflow 链）属 SQLite 存储实现事实；本文以 freelist 观测（48.6 MB ≈ 旧行体积）与 `generation_activate` 计时作为本地证据，未引用外部条文。

## Caveats / Not Found

- 所有收益区间是估算；C1 的净收益还取决于 Defender/fsync 与页面复用细节，必须由 E1/E2 证实或证伪。
- C1 两种设计对 journal retention B2 接口的影响面不同（B2 目前按 `index_batches` 列读/写）；实现前应先读 `journal_retention.rs` 与 `maintenance` 模块，确认子表方案是否需要在 B2 读侧做兼容层。
- 未验证任何改动对 **MCP/TUI/Web** 入口的间接影响；它们共用同一 `commit_source_batches_if_changed`，但复测矩阵只覆盖 CLI。R2 要求"CLI/MCP/Robot 输出语义不变"——实施后至少跑 workspace 全量测试与 MCP 相关测试（`cargo test --workspace`、`test-mcp-*` 既有脚本）。
- 未评估内存峰值变化；基线主矩阵 50k initial peak RSS ≈679.6 MiB、shrink ≈622 MiB（100 ms 采样；trace 试验插桩下 initial 677.1 / shrink 621.4 MiB），C1/C4 预计降低（少一份 48 MB 字符串/少全表装载），但要以复测报告为准。
