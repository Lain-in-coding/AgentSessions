# Research: 不可违反的契约、既有优化历史与可复用的收窄

- Query: 提交路径优化受哪些 spec/契约约束？历史上已经做过哪些优化（避免重复劳动/回退）？哪些"未走完的路线"可以在本任务复用？
- Scope: internal
- Date: 2026-10-15

## Findings

### 1. 硬性契约（来自 spec 与 ADR，改动前必须逐条对照）

`.trellis/spec/agentsessions-adapters-sqlite/backend/index.md`：

- **Pre-Development Checklist（开写前）**：保持 catalog 为唯一真源、FTS 可重建；
  schema 变更必须 `PRAGMA user_version` 前向迁移、非破坏；
  写入走 durable outbox 两阶段（`begin_index_batch` → `commit_index_batch`），
  崩溃只留 side-effect-free `building` 行；generation 推进由 CAS 守卫；
  StableId 身份优先读 `fts_ids.id_json`；源文件只读。
- **"Batch-scoped commit state (measured 2026-09-29)" 场景**：
  - 禁止把整库 map 重新引入提交路径；无候选索引的表可以扫描但内存里只允许
    保留候选行（O(batch)）；
  - durable manifest 每次提交只计算一次，`verify_pending_in_tx` 与
    `index_batches` 行比对（"从输入重算 manifest"的防守已被有意放弃，
    篡改拒绝测试仍在）；
  - **`cache_size` 只是性能旋钮：不得为吞吐交换 `synchronous`、
    journal 模式或单事务原子性**；
  - 未变化（指纹命中）的源不产生提交批次，跳过其 verify_snapshot 重读。
- **Session projection (schema v11)**：`session_fts` 是派生投影；受影响
  Session 在**同一事务**内重建（删除经 `session_fts_ids` 边车载 rowid）；
  privacy 规则（不索引 source_path/native ID 之外字段的既有约束）、
  claim 冲突 fail-closed 不得放宽。
- **Quality check**：`cargo fmt --all --check`、
  `cargo clippy --workspace --all-targets -- -D warnings`、
  `cargo test --workspace`（含 SQLite adapter 全部测试）。

任务 PRD 追加约束（`prd.md` R2/R5 + Constraints）：

- 不改基准 harness 协议语义、不改 CI/发布门/审批；
- 不新增用户可见功能、不改 provider 契约、**不引入新依赖**；
- 磁盘 schema 若调整必须有版本化迁移与回滚路径；
- 默认行为、退出码、错误语义兼容；
- 不读真实 transcript；私有 runtime/原始 trace/失败尝试不进入产品提交；
- 平台范围限本机 Windows，不宣称跨平台收益、不承诺未测得加速。

### 2. 已经做过的优化（不要重复做、不要回退）

归档证据：`.trellis/tasks/archive/2026-09/09-29-index-throughput/research/report.md`
与 `.trellis/spec/.../index.md`「Batch-scoped commit state」。

08-13 波（`docs/performance-baseline-0.2.md`"2026-08-13 post-optimization"）：
fts_rowid 边车（O(1) FTS 行删除）、fingerprint skip + early no-op、
batch-scoped 提交与完整性检查、prepared statements 复用、IN 查询 500/批分块。

09-29 波（6 个已保留候选，均在当前代码中）：

| # | 机制 | 现位置（workspace adapter lib.rs） |
| --- | --- | --- |
| 1 | 未变化源跳过 verify_snapshot 双读/双哈希 | CLI 侧指纹缓存路径 |
| 2 | 写连接 `PRAGMA cache_size=-131072` | :1816-1817 |
| 3 | late no-op 探测复用已载入状态 | `source_batches_are_current` :5965 |
| 4 | durable manifest 每次提交只算一次 | 调用点 :4421 / 复用至 `begin_index_batch_with_manifest` |
| 5 | 别名再生候选集过滤 + 内存 payload 快路径 | `regenerate_compatibility_aliases_in_tx` :5570-5963 |
| 6 | 提交状态按批内候选 id 范围化（8 张表不再整库载入） | `load_catalog_state` :3725-3803、`stored_*_for` :5464-5541 |

该报告的"可选路线"（§7）里与本任务仍相关的两条：

- **路线 A 部分未完成**：`regenerate_compatibility_aliases_in_tx` 仍会
  整表读 placements/edges（5732-5733）并持有两份克隆索引——本任务若做 C3
  属于继续收窄，需引用 50k trace 证据。
- 路线 B–D（拆批、schema 投影改造、增量校验）与 PRD"不新增用户可见功能/
  不引入破坏性变更/不改测量口径"的边界冲突，除非有 AC1 证据支撑，
  否则不作为本任务首选。
- 09-29 报告 §9 记录的空源重复 sync relocation 缺陷**是独立任务**，
  与本任务无关，不得顺手修。

### 3. 可复用的既有资产（直接引用，不复制实现）

- trace 脚手架：`crates/agent-session-grep-adapters-sqlite/src/trace.rs` 与
  `crates/agent-session-grep-cli/src/trace.rs`（`ASG_INDEX_TRACE` env 门控，
  默认零开销）；新增阶段计时必须同样 env 门控且不进主延迟。
- 分段计时点样式：adapter `lib.rs` 3662/6853-7450 的
  `trace::begin()`/`trace::add`；`adapter:fts`（7761）细分是范例。
- 合成语料 + oracle：canonical harness `scripts/evidence/core_beta_benchmark.py`
  的 `SyntheticOracle`/census 校验、`validate-report`/`validate-baseline`。
- 等价性脚本（09-29 遗产）：`research/scripts/verify_equiv_extended.py`
  （25 张非影子表逐行哈希 + `active_generation`），可作 AC2 的补充证据，
  但注意其语料不含 tool_use/usage（那 4 张表为空比较）。
- 归属/复用审计：新代码若借鉴外部项目模式（如 hstry bulk-insert 模式，
  现有注释已标注 MIT 出处），必须登记到 NOTICE /
  `docs/operations/REUSE-LICENSE-AUDIT.md` 的既有条目规则下。

### 4. 每个被实施优化都必须能对回证据（R1/AC1 的判据）

实施时建议在提交信息或研究文档中逐条登记：
`候选 ID（本目录 commit-path-hotspot-map.md 的 C*） → trace 阶段名 + 基线
毫秒数 → 改动文件:行 → A/B 结果（三跑中位数）→ 保留/回滚`。
未挂到任何 trace 阶段实测数字的优化，按 PRD 视为"对猜测做优化"，不应实施。

## Caveats / Not Found

- spec 中"accepted trade-off（manifest 不重算）"是既有决定；本任务不得借
  "加固"名义扩大改动面。
- 未找到 50k 规模下 tool_use/usage 非空的合成负载；关系表 activity/usage
  的逐行 upsert（7125-7184）在 50k 合成语料里恒为 0 行，不要用合成 trace
  给这两段下"可以慢"的结论。
- 09-29 的 1M 数字与 50k 单批不可直接换算（批数/库大小/缓存效应不同），
  所有 A/B 预期都必须在 50k 矩阵上实测。
