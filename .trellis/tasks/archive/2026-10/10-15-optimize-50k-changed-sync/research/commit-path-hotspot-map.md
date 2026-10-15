# Research: 50k changed-sync 提交路径 — trace 阶段 → 代码热点映射

- Query: 50k initial/shrink changed-sync 的嵌套阶段各对应哪段代码？哪些是规模敏感的真实瓶颈、哪些可以证明不是？
- Scope: internal（对 S=b0fc44d75690 源码走读 + 基线 run-03 原始 trace + 冻结二进制诊断测量）
- Date: 2026-10-15
- Path/line convention: 产品路径与行号相对 canonical checkout `.trellis/.runtime/attribution-closeout-20261012/canonical-repaired/`（源码 == S）。协调根 `.` 的同名文件在该区域行号 ≈ S 行号 − 6；内容仅注释差异。

## Findings

### 1. 提交管线总览（代码级）

CLI `sync` 把一次调用的全部源（250 个文件）作为**一个批次**交给应用层，最终进入 `SqliteStore::commit_source_batches_if_changed`（S `lib.rs:3649`）。整个过程是两段事务：

**Phase A — 意图与状态装载（`adapter:catalog`，提交前读 + 1 次小事务）**

| trace 阶段 | S 位置 | 代码行为 |
| --- | --- | --- |
| `validate_incoming` | S:3668-3718 | 每源校验：`installation_for_source_commit`、`batch_manifest`（单源空关系）、`ensure_stored_identity_metadata_matches`（每实体 1 次 `fts_ids` 点查，S:3405-3431）、placement/edge/claim 去重 |
| `noop_probe` | S:3719-3721 | `sources_are_current`（S:4567）：指纹未变的源整体短路 |
| `load_catalog_state` | S:3731-3809 | 按批作用域装载 memberships/claims/stored placements/edges/activities/usages（候选 id 集合） |
| `merge_sources` | S:3818-4095 | 批内跨源同 id 合并（`merge_message_payloads` S:248 / `merge_session_payloads` S:456） |
| `merge_stored` | S:4096-4127 | `get_many` 分块读取已存 payload 再合并 |
| `tombstones` | S:4128-4368 | claimer 分析推导 entity/placement/edge/activity/usage 删除 |
| `manifest` | S:4426-4443 | `batch_manifest`（S:994，排序 + blake3 + serde_json 序列化）+ **再次** `ensure_stored_identity_metadata_matches(upserts)`（S:4428）+ `source_batches_are_current`（S:5971，内容级 late no-op 检查，本轮负载下通常提前 false 退出） |
| `outbox_intent` | S:4453-4456 | `begin_index_batch_with_manifest`（S:6266）：向 `index_batches` INSERT 完整 durable intent |
| `apply_commit` | S:4457-4461 | 调用 Phase B（下） |
| `finalize` | S:4462-4469 | `clear_installation_reservations` |

**Phase B — 单事务数据提交（`adapter:commit`，S `lib.rs:6848`）**

| trace 阶段 | S 位置 | 代码行为 |
| --- | --- | --- |
| `verify_outbox` | S:6858-6869 | CAS（`store_metadata.active_generation`）+ 读回 `index_batches` 全部 5 个明细列（`upsert_ids_json`/`delete_ids_json`/`relation_upserts_json`/`relation_deletes_json`/`source_replacements_json`）并逐串与内存 manifest 比较（S:6339-6430） |
| `affected_sessions` | S:6876-6933 | 收集本批触碰的 Session（见 §4 发现 2） |
| `catalog_upserts` | S:6944-6967 | 分块多行 `INSERT … ON CONFLICT`（`BULK_INSERT_ROWS_PER_CHUNK = 100`，S:115） |
| `fts_projection` | S:6968-6970 | `batch_upsert_fts_in_tx`（S:7659）：删旧 fts/fts_ids（rowid 边车定位）、`fts_tokens_cjk` 变换、批量插入；内部再 emit `adapter:fts`（`fts_projection_rows` / `fts_ids_insert`） |
| `catalog_deletes` | S:6971-6995 | 分块删除 catalog/fts/fts_ids |
| `relation_upserts` | S:6999-7192 | 先逐条 DELETE `relation_deletes`（edge/placement/activity/usage），再 placement/edge 多行 upsert（100 行/块）、activity/usage 逐行 upsert |
| `source_replacements` | S:7194-7364 | 每源：清并重写 `source_membership`/`source_placement_membership`/activity/usage membership、`source_scans`、`source_relation_scans`、installation、resume claims |
| `session_projection` | S:7365-7387 | `regenerate_compatibility_aliases_in_tx`（S:5576，整批实体 alias 重建）+ 每个 affected session `rebuild_session_search_row_in_tx`（S:6723：session_fts/session_titles/session_repo_slugs） |
| `integrity_checks` | S:7388-7438 | `verify_relational_integrity_in_tx`（S:7476）+ `verify_deleted_entities_unreferenced_in_tx`（S:7573，按 chunk） |
| `generation_activate` | S:7439-7453 | `UPDATE store_metadata` + `UPDATE index_batches SET state='activated', durable_point, committed_at_ms` |
| `tx_commit` | S:7454-7456 | `tx.commit()`（含 WAL 落盘与触发阈值后的 autocheckpoint） |

### 2. 基线 trace 数值（run-03 独立试验；单样本、带插桩，仅用于排序，不可相加）

50k **initial**（250 源 × 200 消息；entities=50500，relation_upserts=95000，relation_deletes=0，deletes=0）：

| 标签 | 阶段 | ms |
| --- | --- | ---: |
| `adapter:commit` | tx_commit | **3350.893** |
| `adapter:commit` | session_projection | **2216.943** |
| `adapter:commit` | relation_upserts | **2015.248** |
| `adapter:commit` | affected_sessions | 720.814 |
| `adapter:commit` | source_replacements | 670.612 |
| `adapter:commit` | generation_activate | 652.565 |
| `adapter:commit` | integrity_checks | 539.619 |
| `adapter:commit` | fts_projection | 498.623 |
| `adapter:commit` | catalog_upserts | 61.211 |
| `adapter:commit` | verify_outbox | 61.773 |
| `adapter:catalog` | apply_commit（父） | 10802.788 |
| `adapter:catalog` | manifest | 1193.693 |
| `adapter:catalog` | outbox_intent | 506.338 |
| `adapter:catalog` | merge_sources | 350.972 |
| `adapter:catalog` | validate_incoming | 241.362 |
| `adapter:catalog` | load_catalog_state | 158.715 |
| `adapter:catalog` | tombstones | 150.042 |
| `adapter:catalog` | finalize | 14.982 |
| `adapter:fts` | fts_projection_rows / fts_ids_insert | 311.151 / 180.156 |
| `cli:sync` | commit（父） | 13660.269 |

50k **shrink**（每文件保留前半；entities upserts=25500，deletes=25500，relation_upserts=47500，relation_deletes=95000，source_replacements=250）：

| 标签 | 阶段 | ms |
| --- | --- | ---: |
| `adapter:catalog` | apply_commit（父） | 7205.070 |
| `adapter:commit` | tx_commit | 1241.465 |
| `adapter:commit` | relation_upserts | 1592.620 |
| `adapter:commit` | session_projection | 959.194 |
| `adapter:commit` | affected_sessions | 879.924 |
| `adapter:commit` | source_replacements | 686.519 |
| `adapter:commit` | fts_projection | 615.059 |
| `adapter:commit` | integrity_checks | 515.749 |
| `adapter:commit` | generation_activate | 104.858 |
| `adapter:catalog` | manifest | 605.694 |
| `adapter:catalog` | merge_stored | 601.187 |
| `adapter:catalog` | load_catalog_state | 600.636 |
| `adapter:catalog` | merge_sources | 336.347 |
| `adapter:catalog` | outbox_intent | 311.727 |
| `adapter:catalog` | tombstones | 247.162 |
| `cli:sync` | commit（父） | 10310.472 |

原始记录：`evidence-output/current-source-20261014-b0fc44d75690/run-03/index-trace-{initial,shrink}.jsonl`（`asg.index-trace/v1`，含 detail 计数与 adapter:bytes 体积），聚合在 `index-trace-observation.json`。

### 3. 结构性发现 1：durable journal 行是最大的单对象，且每次提交被整行写两次

对同一冻结二进制 + 同一生成器语料的诊断测量（详见 `50k-journal-row-diagnostic.md`）：

- initial 提交后 `index_batches` 单行明细：`relation_upserts_json` **38,022,500 B** + `source_replacements_json` **8,210,751 B** + `upsert_ids_json` **2,321,001 B** ≈ **48.6 MB/行**（`detail_format='full'`）。
- shrink 第二行：19.0 MB + 8.2 MB + 1.2 MB + 1.2 MB ≈ **33.8 MB**。
- 该行在 Phase A 由 `INSERT`（`outbox_intent`，506 ms）写入，在 Phase B 由 `UPDATE index_batches SET state='activated'…`（`generation_activate`，652 ms）再次写；SQLite 的 `UPDATE` 会重写整条行记录（含 overflow 链），即该行字节每次提交进 WAL 两次。
- `verify_pending_in_tx`（`verify_outbox`，62 ms）把 5 个明细列整串读回内存再比较——读放大但实测代价小。
- 独立佐证：initial 结束后主库 `freelist_count = 11,863 页`（≈48.6 MB），与"旧行 overflow 页被重写释放"一致；shrink 后 `freelist_count = 18,105 页`（≈74 MB）。
- WAL/checkpoint 机制（外部事实，sqlite.org）：`wal_autocheckpoint` 默认 1,000 页；自动 checkpoint 在**触发它的 COMMIT 内、同一线程**执行（PASSIVE），把 WAL 页拷回主库。该语料 initial 提交后主库 238 MB（58,178 页，其中 freelist 11,863 页 ≈48.6 MB），即曾有 ≈70k 页（≈287 MB）经过 WAL；因此 `tx_commit`（3.35 s）包含"WAL 写入 + `synchronous` 默认 FULL 的 fsync + 自动 checkpoint 回拷"。减少被写字节数（含 journal 行的第二次整行重写）直接作用于该项。

### 4. 其余可证据化的热点

1. **`manifest` 阶段 1.19 s（initial）**：`batch_manifest` 对 50k 实体排序/拼 `upsert_ids`、对 29 MB payload + 48 MB 关系 JSON 做 blake3、把 95k 关系 upsert 与 250 源替换序列化成 48.6 MB 字符串；末尾的 `source_batches_are_current` 与第二次身份校验追加成本。注意 `batch_is_current_with_derived_context`（S:3550）在 changed-sync 中通常**首个不匹配即返回**，不构成本负载瓶颈（noop 路径另有批量化实现，本任务不动）。
2. **`affected_sessions` 0.72 s（initial）/ 0.88 s（shrink）**：S:6877-6905 对每个 message upsert、每个 placement upsert 各调一次 `collect_message_sessions`，且每次传入 **1 元素 slice**——函数内部虽有 `chunk_ids`（500/块，S:154-157），单元素调用退化为 ~50k + ~50k 条单行查询。
3. **身份校验重复**：`validate_incoming` 已对每源 entries 调 `ensure_stored_identity_metadata_matches`（S:3674 附近的 3665-3710 循环）；`manifest` 阶段 S:4428 对同一批 upserts **再查一遍**（每实体 1 次 `fts_ids` 点查，S:3405-3431）。合计 ~100k 点查。
4. **`session_projection` 2.22 s（initial）/ 0.96 s（shrink）**：`regenerate_compatibility_aliases_in_tx` 里 `stored_placements_from`/`stored_edges_from`（S:5417 / S:5549）是 **全表 `SELECT … ORDER BY`** 装载（50k/45k 行；1M 规模下就是 1M/900k 行），并对 50.5k 个实体逐个 `serde_json::from_slice` → 重建 alias → `to_vec` → 与已存字节比较。人工探针（见诊断文件 §3）显示本语料下 payload 已是"排序键 + 紧凑分隔符"的规范形态，重建结果与存入字节一致 → 该循环大多**不做 UPDATE**，成本集中在 JSON 往返与全表装载；每 session 的 `rebuild_session_search_row_in_tx`（250 次）再叠加 session_fts/titles/repo_slug 的重建查询。
5. **`relation_upserts` 2.02 s（initial）/ 1.59 s（shrink）**：placement 8 列 × 100 行 / edge 4 列 × 100 行的多行 upsert；`BULK_INSERT_ROWS_PER_CHUNK=100` 由 `8*100 ≤ 950` 的编译期断言封顶（S:115、7056），而 SQLite ≥3.32 的默认 `SQLITE_MAX_VARIABLE_NUMBER` 是 32766（官方文档；bundled 编译值需运行时确认）。shrink 中 95k relation_deletes + 47.5k upserts 是语义必需的（文件截断导致 document/session 身份重派生 → placement id 全量变化），不能靠"过滤未变化行"消掉。
6. **次要但真实**：`source_replacements` 0.67/0.69 s（每源 membership 清空重写）；`integrity_checks` 0.54/0.52 s（按 chunk 的存在性校验）；`merge_stored`（shrink 0.60 s，读取 25.5k 已存 payload）与 `load_catalog_state`（shrink 0.60 s）；`tombstones` 0.15/0.25 s。

### 5. 明确不是本任务目标的项

- **noop 路径**（77.6 ms 中位数）与 `batch_is_current_with_derived_context` 的逐实体回读：changed-sync 下提前退出；noop 由 `sources_are_current` 覆盖，不在本轮改。
- **`synchronous` / `journal_mode` / 单事务原子性**：spec 明确"cache_size 才是性能旋钮，不得交易这三者"（`.trellis/spec/agentsessions-adapters-sqlite/backend/index.md` "Batch-scoped commit state" §3）。
- **FTS 删除/重建**：已有 `fts_rowid` 边车按 rowid O(1) 删除；`fts_projection` 与 `adapter:fts` 合计 <0.6 s（initial）。
- **`wal_autocheckpoint=0` 类调参**：只是把回拷推迟到 close/后续操作；suite 度量的是进程 wall（含 close checkpoint），不减少总 I/O，且属于测量口径外的手工调参，不应作为"协议内"收益。

## Caveats / Not Found

- trace 是**单样本 + 插桩**：`tx_commit`/`session_projection` 等的绝对值不应当作复测基线；排序关系（tx_commit > session_projection ≈ relation_upserts）是本文件的核心信息。
- 阶段内部拆分（如 `manifest` 中 serde/哈希/校验的占比、`session_projection` 中 JSON 往返 vs 全表装载的占比）来自代码走读 + 体积测量推断，未被逐段计时；实现阶段应先用一次私有实验证实（见 `candidate-optimizations-and-risks.md` 的 E1-E3）。
- `tx_commit` 内 WAL 字节/fsync/autocheckpoint 的三项占比未测得；本文件只给出"写字节规模 ≈ 百 MB 级"的量级证据与官方机制说明。
- bundled libsqlite3-sys 的实际 `SQLITE_MAX_VARIABLE_NUMBER` 未在本机运行时核实（须在实现前用连接级查询确认后再敢提高分块）。
- 未做跨平台验证；结论只绑定本机 Windows 10.0.22631 + NVMe + Defender 环境（与基线一致）。
