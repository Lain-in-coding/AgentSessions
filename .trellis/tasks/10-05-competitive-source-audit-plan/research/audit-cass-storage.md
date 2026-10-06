# cass 存储与索引管线审计（分片 C · T1 回执）

- 审计对象：`C:\AgentSessions\Github_src\coding_agent_session_search`（cass），commit `aa92a45311e10a3ac9c40c3730664a7366af2fe8`（与任务记录一致；本分片未做任何 git 写操作）。
- 日期：2026-10-06。范围：T1 精读 3 个文件 —— `src/indexer/semantic.rs` 全文（6229 行）、`src/storage/sqlite.rs` 与 `src/indexer/mod.rs` 的有界精读（schema/迁移、FTS、写入事务、批量/并发、删除/清理、索引编排）；T2 机械扫描仅按 path/probe 过滤查询 `sweep-coding_agent_session_search.json`。
- 读取方式（如实记录）：`semantic.rs` 用 `codegraph node --file … --offset … --limit 150` 分 42 段读完全文；`sqlite.rs`（1.18MB/30047 行）与 `mod.rs`（2.16MB/52111 行）**不在 codegraph 索引内**（`No indexed file matches`，超尺寸上限），改用 PowerShell 行号有界读取（`Get-Content | Select-Object -Skip/-First`，每次 ≤150 行）——45 段 / 34 段。单次读取从未超过 150 行；读前/读后 SHA-256 一致（3/3）；未把哈希当阅读。机器凭证见同目录 `coverage-cass-storage.json`（含精确 read_ranges / missing_ranges / 并集行数）。
- 覆盖概览：`sqlite.rs` 6610 / 30047 行（partial）；`semantic.rs` 6229 / 6229 行（full）；`mod.rs` 4762 / 52111 行（partial）。T2 扫描背景：3641 文件，扫描 3632，二进制排除 5，超尺寸跳过 4，有命中文件 836；本分片三文件命中概览——`sqlite.rs` index_storage=206 / sql_dynamic=10 / concurrency=13 / ignored_result=27；`semantic.rs` index_storage=292 / concurrency=5；`mod.rs` index_storage=889 / concurrency=61 / fs_watch=26 / ignored_result=64。

## 一、结论速览（Top-5）

| ID | 级别 | 一句话结论 | 关键锚点 |
|---|---|---|---|
| CS-1 | P2（有界可达） | 增量失效判据的“内容指纹”实为 `content-v1:{会话数}:{最大会话ID}:{最大消息ID}`，是基数+ID 值而非内容哈希；保量改写（同数同 max id）不会使 completed lexical checkpoint / semantic db_fingerprint 失效 | `src/indexer/mod.rs:8695-8747,2801-2808,8762-8772,10338-10363,14093-14128`；`src/indexer/semantic.rs:3156-3161,4920-4983` |
| CS-2 | P2 | begin-concurrent 写者显式关闭 FK（失败仅 debug），而 #202 注释明确“事务中途断连会留孤儿”，孤儿清理 preflight 默认被跳过 → 孤儿可长期积累 | `src/indexer/mod.rs:26128-26135,25978-25984,13459-13501`；`src/storage/sqlite.rs:4639-4656,4657-4739,5923-6222` |
| CS-3 | P2 | in-DB FTS 影子写入是 best-effort（错误吞掉仅 warn）、routine preflight 校验默认关闭；FTS parity 修复主要发生在 full run/doctor，增量 run 默认不修 | `src/storage/sqlite.rs:14102-14236,11127-11167`；`src/indexer/mod.rs:13342-13419,14842-14911` |
| CS-4 | P3（未运行验证） | 批量消息插入用 `last_rowid-(n-1)` 反推 message id，snippets/FTS 依赖这些推断 id；若 rowid 分配非连续（删除复用、并发交错）会错绑 | `src/storage/sqlite.rs:13577-13586,13713-13722,10031-10050,13536-13544` |
| CS-5 | P3 / 信息 | `forget_conversations_by_source_glob` 把匹配 id 直接拼进 SQL 字符串（无绑定、无上限）；`delete_source` 的 `cascade` 参数被忽略 | `src/storage/sqlite.rs:7723-7764,9833-9844` |

> 上表为严重度主观排序；CS-1/CS-2/CS-3 是发布语义与恢复语义的系统性风险，CS-4/CS-5 是静态可达但未证实的实现风险。逐项证据/可达/反证见第七节。

## 二、schema 与迁移策略

### 2.1 版本与兼容判定
- 双账本：`_schema_migrations`（MigrationRunner，`schema_version()` 取 `MAX(version)`，`sqlite.rs:4742-4754`）+ 兼容列 `meta.schema_version`（`sync_meta_schema_version` 仅在需要时写，`sqlite.rs:4757-4783`）。
- 兼容矩阵 `check_schema_compatibility`（`sqlite.rs:3102-3182`）：`meta` 缺失且有表 → NeedsRebuild；版本 == 当前 → Compatible；`MIN_IN_PLACE_MIGRATION_SCHEMA_VERSION..CURRENT` → NeedsMigration；更老或更新 → NeedsRebuild（附原因）。`open_current_schema_storage_with_timeout` 要求版本**精确等于**当前值，否则返回 None（`sqlite.rs:564-602`）。
- 打开即修复：`open_or_rebuild` 在 NeedsRebuild / 结构性损坏时 `create_backup → cleanup_old_backups(MAX_BACKUPS=3) → remove_database_files` 后返回 `MigrationError::RebuildRequired`（`sqlite.rs:4821-4856`）。

### 2.2 迁移序列（v13–v20）
- 全新库走 combined v13 `MIGRATION_FRESH_SCHEMA`（避免 v5 `DROP TABLE conversations` 触发 frankensqlite autoindex 缺陷，`sqlite.rs:4863-4877,4966-4975`）。
- 基座：`add(13 full_schema_v13)` → `add(14 fts_contentless)`（**DROP fts_messages**，改懒重建，`sqlite.rs:3560-3568,4874-4878`）。
- 手工事务 v15：`_schema_migrations` 打点 + `conversations.last_message_idx/last_message_created_at` 列 + `conversation_tail_state` 表，一条 `BEGIN IMMEDIATE` 提交、失败显式 ROLLBACK（`sqlite.rs:4899-4942`）。
- 尾部：v16 删冗余 `idx_messages_conv_idx`、v17 删全局 `idx_messages_created`、v18 `conversation_tail_state` 热表单飞（无 FK，注释说明为躲开 frankensqlite rowid 更新路径）、v19/v20 外部查找表（`conversation_external_lookup` / `conversation_external_tail_lookup`）（`sqlite.rs:3560-3673,4880-4887`）。
- meta → `_schema_migrations` 过渡：读 meta 版本，创建 `_schema_migrations` 并按 `MIGRATION_NAMES` 回填 1..=current（v10–v12 一律记为 v13）；meta 版本为 0 但表存在时只打 info 并跳过，让 MigrationRunner 后续报错（`sqlite.rs:5734-5849`）。
- 版本标记漂移修复：`repair_missing_current_schema_objects` 按必需表探针找出缺表，走 `current_schema_repair_batches_for_missing_tables` 的幂等 batch 重建并复验；另有 `repair_missing_conversation_token_columns` 逐列 `ALTER TABLE`（cass#222）（`sqlite.rs:4575-4637`）。

### 2.3 备份/隔离（崩溃恢复的安全网）
- `create_backup`：优先 `VACUUM INTO` 到唯一 staging 再 rename；**transient contention 时拒绝 raw WAL bundle 拷贝**（避免复制不一致的 WAL），非瞬态失败才落到 raw 证据拷贝（带 symlink 与非普通文件拒绝、文件与父目录 fsync）（`sqlite.rs:1383-1434,1436-1463,1527-1632`）。
- 损坏 bundle 走 `move_database_bundle`（db + `-wal` + `-shm` 一起改名隔离，孤儿 sidecar 也搬走）（`sqlite.rs:1482-1525`）；`remove_database_files` 同删三件套（`sqlite.rs:1634-1658`）。

## 三、FTS 结构（in-DB fallback shadow）

### 3.1 建表与影子表
- 生产注册 SQL：`CREATE VIRTUAL TABLE IF NOT EXISTS fts_messages USING fts5(content,title,agent,workspace,source_path,created_at UNINDEXED,content='',contentless_delete=1,tokenize='porter')`——**contentless + porter**，无独立 `message_id` 列（rowid 即 message id）（`sqlite.rs:1105-1116`）。
- 必需影子表 5 张：`fts_messages_config/content/data/docsize/idx`；`FTS5_DELETE_ALL_SQL = DELETE FROM fts_messages`（对 contentless_delete=1 是事务性删除；老影子会拒绝语句从而保住已发布内容）（`sqlite.rs:1142-1155`）。
- 健康标记：`meta.fts_frankensqlite_rebuild_generation=1` + `fts_frankensqlite_archive_fingerprint`，仅在校验 Healthy 后写入（`sqlite.rs:1118-1120,10673-10700,10781-10792`）。

### 3.2 完整性/奇偶校验
- `validate_fts_messages_integrity_for_connection`：先探测 `SELECT * FROM fts_messages LIMIT 0`；表存在但 5 张影子表缺失 → 结构化损坏报错（含恢复提示）；影子齐全但探针失败时**容忍**（避免把 frankensqlite vtable 路径误判为损坏）（`sqlite.rs:1157-1251,1264-1336`）。
- `inspect_search_fallback_fts_parity`（精确三数）：`indexable = COUNT(*)`（经 `(conversation_id,idx)` 索引的活会话消息）`intersection`（docsize 点查 join messages 的活行）`indexed = COUNT(fts_messages_docsize)`；分类 Absent / Unqueryable（schema 行≠1 或 docsize 计数失败）/ Partial / Healthy / Excess / Divergent，并对“同数不同行”的隐蔽分歧给出专门 detail（`sqlite.rs:1121-1137,10945-11100`）。

### 3.3 重建与失败原子性
- `ensure_fts_consistency_via_frankensqlite` 的策略（`sqlite.rs:11127-11167`）：Healthy→记 generation；Partial→`stream_fts_rows(missing_only=true)` 可续修复；Absent→整表重建；**Unqueryable / Excess / Divergent 一律 fail-closed 拒绝原地重建**（要求保全 bundle、走 staged recovery）。
- `rebuild_fts_via_frankensqlite`：`BEGIN IMMEDIATE` →（Absent 则注册；否则 `DELETE FROM fts_messages`）→ 分批流式插入 → `require_healthy_fts_parity` → 记 generation → `COMMIT`；任何失败显式 `ROLLBACK` 并报告“未发布部分影子”；rollback 未确认时要求保全 bundle（`sqlite.rs:11169-11240`）。
- 流式插入按 `FTS_ENTRY_BATCH_MAX_DOCS/CHARS` 分块，跳过孤儿消息（`conversation_by_id` 无父行）与 missing_only 已存在行；分页查询用 `SAVEPOINT cass_fts_rebuild_page` 保证“计划与正文取自同一快照”（`sqlite.rs:11253-11378,11392-11406`）。

### 3.4 写入与查询边界
- 增量写入：单会话/批量路径把 FTS 条目攒到 `FTS_ENTRY_BATCH_MAX_DOCS/CHARS` 再批量插入；`franken_batch_insert_fts` 对错误**吞掉只 warn/debug**并提前返回（“Tantivy is authoritative”）；`_on_connection` 变体（重建流）则把错误上抛（`sqlite.rs:14102-14236` 与调用点 `9847-10091,12000-12584`）。
- `sqlite.rs` 全文（已读范围）**没有 `MATCH`/`bm25()` 查询点**：in-DB FTS 的查询消费在检索层（分片 B 已覆盖 query.rs 的 SQLite FTS5 降级路径），本分片只审计其产生/维护/校验。
- 语义：in-DB FTS 是 Tantivy 的**降级影子**；写入 best-effort、校验与修复有精确 parity 门，但 routine 路径默认不跑门（见 CS-3）。

## 四、事务、并发、WAL 与崩溃恢复

### 4.1 单会话事务
- `insert_conversation_tree`（`sqlite.rs:9847-10092`）：先规范化 + `ensure_source_for_conversation`（事务外），随后**一个事务**内：按 (source_id,agent,external_id) 查既有会话→有则 tail-append，无则插入会话行（若撞已有则转 append）；按 `idx` 与“replay 指纹”双重去重；消息按 100 行/批（append 50 行）多行 INSERT；snippets 逐条；FTS 攒批；`conversation_tail_state` + 外部查找表更新；`daily_stats` 增量；最后单次 `tx.commit()`。
- 失败语义：任何 `?` 错误直接展开（tx drop → ROLLBACK），`InsertOutcome` 只有提交后才返回。

### 4.2 批量事务
- `insert_conversations_batched`（`sqlite.rs:12000-12584`）：**整批一个事务**。事务内先 `ensure_agents_in_tx / ensure_workspaces_in_tx / ensure_sources_in_tx`（#167：补偿 autocommit ensure 在事务快照不可见），再用 `pending_conversation_ids` / `pending_message_fingerprints` / `pending_message_replay_fingerprints` 做批内去重；FTS、daily_stats、token_usage、message_metrics、usage_* 汇总全部并入同一事务，末尾一次 commit。
- 连接池：`FrankenConnectionManager` 预开 4 个读连接、写者令牌 = `available_parallelism`；`WriterGuard` 未 `mark_committed()` 时 drop 即 `ROLLBACK`（`sqlite.rs:764-984`）。重试分类：Busy/BusyRecovery/BusySnapshot/DatabaseLocked/LockFailed/WriteConflict/SerializationFailure + busy/locked/contention 文本（`sqlite.rs:706-736`）。

### 4.3 并发写（begin-concurrent）与重试
- `persist_conversations_batched_begin_concurrent`（`mod.rs:26068-26297`）：rayon `par_chunks(chunk_size)`，每 chunk 独立 `FrankenStorage::open_writer` + 调优 + **`PRAGMA foreign_keys = OFF`（失败仅 debug）**；每条会话的 ensure_agent/workspace/insert 整体包进 `with_concurrent_retry`（4→256ms 指数退避 + jitter；DatabaseCorrupt **明确不重试**，`mod.rs:25841-25856`）；chunk 冲突耗尽 → `ChunkPersistResult::RetryableFallback`，剩余范围转串行 fallback（重试下限 12 次，`mod.rs:25963-26037`）；结果按输入下标排序后统一推进水位/语义 delta。
- 有重复会话键（key = agent+source+external_id 或 agent+source+source_path）时整体回退串行路径，避免批内自撞（`mod.rs:26039-26065,26632-26661`）。

### 4.4 WAL 与检查点
- 写连接 PRAGMA（`sqlite.rs:4428-4500`）：`journal_mode=WAL`、`synchronous=NORMAL`、`cache_size=-65536`、`foreign_keys=ON`、`busy_timeout=5000`、`wal_autocheckpoint=4096`，并尝试 `concurrent_mode=ON` 与 `autocommit_retain=OFF`（失败分支：能识别的 FTS shadow reload 场景降级 warn，其余上抛）。
- 批量导入期 `defer_checkpoints = !watch`（`mod.rs:13289`）→ `apply_index_writer_checkpoint_policy`（`mod.rs:13339`）；索引结束恢复策略并做**最终 `wal_checkpoint(TRUNCATE)`**：正常路径容忍 Blocked（保持历史宽松契约）；abort 路径另有专门的 checkpoint 结果分类与 best-effort abort 函数（fn 地图中的 `classify_final_wal_checkpoint` / `best_effort_abort_wal_checkpoint`，未逐行读）。关闭前设置 `progress.finalizing`，防 stall watchdog 把分钟级 checkpoint 误杀。

### 4.5 孤儿与异常恢复
- 孤儿检测/清理（`sqlite.rs:5923-6222,4657-4739`）：根 `messages` 无父（NOT EXISTS），分块删除（`ORPHAN_FK_ID_CHUNK_SIZE`），**OOM 时二分重试**；直接子表 `message_metrics/token_usage/snippets/conversation_tags` 用有界页探针 + 分块删除；每块一个事务提交；只在成功清理后发 warn（cass#202 根因是“连接中途 drop 会留孤儿”，清理是防御纵深）。
- 默认行为：该清理挂在 `watch_startup:cleanup_orphan_fk_rows`，**默认跳过**（需 `CASS_PREFLIGHT_CLEANUP_ORPHAN_FK_ROWS=1` 才跑；显式跳过用 `CASS_SKIP_…`），失败会中止 index run（`mod.rs:13459-13501`）。
- 崩溃残留：`staging_reclaim::reclaim_orphaned_staging_dirs_for_data_dir` 在持有独占 `index-run.lock` 后、磁盘余量预检前回收（`mod.rs:13087-13093,16961-16963`）；语义 staging `.staging-*.fsvi` 由 checkpoint 续写；FTS 重建前拒绝不可证明可恢复的 Unqueryable 影子。

## 五、索引编排：全量/增量、manifest/generation、失败与半成品

### 5.1 `run_index` 主流程（`mod.rs:13046-15263`，全文精读）
1. `acquire_index_run_lock_with_job_kind` + 心跳 + staging 回收（13075-13100）；
2. 只读预检快路径：不可续传的 pending lexical rebuild 可只读重启或按 DB 尺寸**defer**（13197-13283）；
3. `open_storage_for_index(full)`；writable 预检（无副作用 UPDATE）失败则重开（13305-13336）；`validate_fts_messages_integrity` 为 opt-in 且可 skip（13342-13419）；
4. full 前的“拒绝破坏”门：canonical 不健康 / historical salvage 需重启 → 直接报错不动归档（13421-13435,13507-13525）；
5. 只读探测 lexical checkpoint → 决定 `resume_lexical_rebuild` / restarts / 保留 completed checkpoint（13544-13636）；
6. Tantivy reader/schema 预检 → `needs_rebuild`（13677-13747）；大库上把“重权威重建”降级为 defer（13748-13779）；
7. 策略判定：`resolve_lexical_population_strategy(needs_rebuild, full, salvage)`；full/salvage → `DeferredAuthoritativeDbRebuild`，否则 InlineRebuildFromScan / IncrementalInline（1687-1727,13983-14248）；
8. 扫描：`run_streaming_index`（默认）或 `run_batch_index`（`CASS_STREAMING_INDEX=0`）；扫描期间若 lexical 更新被 defer → 丢弃 t_index，改为从 canonical DB 重建（14305-14390）；
9. 全量扫描后的 no-op 跳过：live doc 数与 completed checkpoint 完全匹配则不做权威重建（14392-14465）；
10. 水位：全局 `last_scan_ts` 仅当“扫描发生 && 未启用排除/活跃源保留 && 无扫描错误”才推进；每连接水位只对成功完成的连接推进（14743-14773）；
11. 完成 lexical checkpoint 刷新（多重跳过条件避免无谓重写，14775-14833）；
12. full run 后 best-effort 修复 fallback FTS（14842-14911）；
13. `--semantic`：全量重放→保留非噪声→embed→建 FSVI→可选 HNSW→manifest 发布→设置 `last_embedded_message_id`（14494-14734）；
14. watch：进入 `watch_sources` 循环，增量语义 embedding 带 60s cooldown（失败 warn + 重置 cooldown，不杀循环）；每 50 次回调回收长活 storage 句柄（14915-15245）；
15. 终局：`close_storage_after_index` = 恢复 checkpoint 策略 + close + `wal_checkpoint(TRUNCATE)`（15248-15263）。

### 5.2 全量与 canonical-only
- 普通 `--full` **必须重扫文件系统**（#153）；`--force-rebuild` 且 canonical 非空时跳过扫描，直接 `rebuild_tantivy_from_db`（canonical-only 快路径，13526-13531,14075-14128）。
- 全量**不 eager 删除** SQLite/Tantivy：“An empty canonical archive can be populated in place… Eagerly deleting or replacing anything is both redundant and dangerous”（13862-13872）——与 ASG“失败不删数据”原则一致。

### 5.3 增量与水位
- `last_scan_ts` 缺失且库很大时（均默认 1GiB 阈值），自动 bootstrap 为 `scan_start_ts-1`（只前向扫描、不回溯全库），日志提示用 `--full` 做有意回溯（`mod.rs:14250-14287,1801-1832`）。
- 扫描错误/排除/活跃源跳过时保留全局水位（14743-14773）；stale ingest quarantine 重试记录在“水位被保留”时**不标记已重试**（14472-14491）。

### 5.4 generation / manifest 原子性
- lexical generation：`gen-{16位毫秒}-{fingerprint 前16}` + `attempt-{毫秒}`；构造时状态判定 Built→Validated→Published；与 equivalence 证据一起 `store_manifest` 到 **generation 目录**（`mod.rs:6672-6769`）；发布走 staged swap（“Keep the published lexical generation live until the authoritative rebuild has produced and validated its replacement… rolls it back if publication fails”，13786-13794）。
- 语义 manifest（`semantic.rs:2938-2973,2977-2996`）：cursor 未耗尽 → 只存 checkpoint（staging 保留）；耗尽 → 先 compact staging WAL → `rename(staging, final)` → `sync_parent_directory` → `manifest.publish_artifact(...)` + `manifest.save`（同一路径内顺序执行，manifest 写失败会让整批返回错误，不吞）。
- 直接 CLI 语义路径（`mod.rs:14691-14709`）是例外：FSVI 已发布后 manifest 更新**仅 warn**，随后 watermark 照常推进（见第七节 P3-1）。

### 5.5 失败与半成品处理
- 半成品从不冒充成品：语义 staging 只在 `cursor_exhausted` 时 rename 发布（`semantic.rs:2934-2966`）；checkpoint 只在 `!complete` 时保存（3034-3048）；FTS 只有 parity Healthy 才记 generation；lexical checkpoint 只有 live doc count 完全等于 canonical 才刷新（`mod.rs:10382-10390`）。
- 失败节奏：单批冲突重试→串行 fallback→仍失败则整批报错（无部分提交）；OOM 在 orphan 清理/lexical 更新上有专门 defer 逻辑（`mod.rs:26772-26804` 的 `record_deferred_lexical_update` 只对 OOM 降级）。

## 六、语义索引：模型、维度、生成与失败重试、与 lexical 的一致性

### 6.1 模型与维度
- Embedder 选择（`semantic.rs:2020-2043`）：`fastembed|minilm|snowflake-arctic-s|nomic-embed`（FastEmbedder，需 data_dir）与 `hash`（HashEmbedder）；维度来自 `embedder.dimension()`，FSVI 写入为 f16 量化（2478-2500），每个 vector 写入前校验维度（1990-1996,2505-2510）。
- 文档身份 `SemanticDocId`：message_id + chunk_idx + agent_id + workspace_id + source_id + role + created_at_ms + canonical content_hash（590-622）；`EmbeddingInput` 携带 storage 内部 id 与 role。

### 6.2 生成、发布与续传
- 批次：窗口 = 4×batch_size；`length_aware_batches` 同时约束行数与 `row_count×max_len ≤16KiB`（#309 防 ONNX 张量爆炸）；canonicalize+hash 有内容寻址 memo（默认容量 4096，并行 prep 默认关）（47-72,1922-1964,2099-2258）。
- checkpoint caps：默认 10_000 msg / 8MiB，且**只在整会话边界停**（`select_checkpoint_capped_conversations`，1467-1529）；`BuildCheckpoint` 记 tier/embedder/last_offset/docs/conversations/total/db_fingerprint/schema/chunking/`last_message_id`/cursor_exhausted（3034-3047）。
- 续传：staging 文件按 (tier,embedder,crc32(db_fingerprint)) 命名（515-527），resume 时 append 到 staging；分片构建写 `shards/{tier}-{embedder}-{blake3_16}` 生成目录 + `SemanticShardManifest`（537-571,2289-2381）。
- 对账：`reconcile_index_with_canonical_documents`（2600-2820）把 live FSVI+WAL 复制到私有 staging 目录，先校验 replacement 全部属于 `current_doc_ids` 且无重复/非有限向量；清 stale（不是 canonical 的 doc id）、补新增、compact/vacuum 后校验 `staged_doc_ids == current_doc_ids`；最后探测“旧 live WAL 不得被新候选接受”，再原子 rename（Windows 走 backup→rename→失败回滚恢复）。
- 选择器：按全局 `messages.id` 游标流式扫描取“最小的 N 个合格会话 id”（BTreeSet 限界），避免稀疏 checkpoint 的父表全扫（选择器 875-1008；稀疏回归测试 5058-5216，432k 消息验收测试 5218-5350，后者 `#[ignore]`）。

### 6.3 失败与重试
- embed 失败 → 整批 Err，不发生 staging 写入；写索引失败 → 删除部分 FSVI；batch watchdog 超过 30s warn、超过 300s 直接 bail（`CASS_SEMANTIC_EMBED_BATCH_WARN/FAIL_AFTER_MS`）（2208-2224,2522-2531）。
- 主流程中 semantic 失败使 `run_index` 在设 watermark 前返回；watch 模式增量语义失败只 warn + 重置 cooldown（不中断 watch）（watch 循环内 `mod.rs:15174-15193`；主流程语义失败则整体返回，`14598-14734`）。
- `embedding_jobs` 表记录 pending/running/completed/failed/cancelled，活跃唯一索引保证同 (db_path,model_id) 只有一个活跃任务；`upsert_embedding_job` 用 UPDATE→INSERT→UniqueViolation 再 UPDATE 的竞态处理（`sqlite.rs:1416-1434,11695-11805`）。

### 6.4 与 lexical/FTS 的一致性
- 三份派生资产（Tantivy lexical、in-DB FTS 影子、FSVI/HNSW 语义）**分别发布、无跨资产事务**：各自有 generation/checkpoint/gate，失败互相不阻塞（lexical 可 defer 等 canonical 重建；FTS best-effort；semantic staging）。
- 一致性锚点：lexical 与 semantic 的“当前数据版本”都取同一 `content-v1:N:M:K` 指纹（`mod.rs:8695-8701`；`semantic.rs:3156-3161` 的 total 缓存与 `mod.rs:8830-8837` 的 direct publish）——优点是可以相互比较，缺点是**指纹本身弱**（CS-1）。
- 语义侧还有逐 doc 的 reconcile 校验兜底（2600-2780），但只在 reconcile 路径执行；正常 backfill 的续传/跳过决策依赖弱指纹。lexical 侧删除/去重后有显式 `rebuild_lexical_index_after_dedup`（`sqlite.rs:11242-11251`）降低漏重建概率。

## 七、逐项发现（证据 / 可达 / 反证）

### CS-1 · P2 · “内容指纹”不是内容哈希：保量改写不使 generation/checkpoint 失效
**证据**：`lexical_rebuild_content_fingerprint_value` = `format!("content-v1:{total_conversations}:{max_conversation_id}:{max_message_id}")`，输入只来自 `COUNT(*)`/`MAX(id)`（`mod.rs:8695-8747,8762-8772`）；匹配只比较 total_conversations、storage_fingerprint、路径（`mod.rs:2801-2808,6236-6248`）；completed checkpoint 据此判“已发布且仍有效”并跳过重建（`mod.rs:10338-10363,14093-14128`）；semantic 的 `db_fingerprint` 与总会话缓存复用同一字符串（`mod.rs:8830-8837`；`semantic.rs:3156-3161,4920-4983`）。
**可达**：任何“等量替换”都能命中——例如删除一条 + 插入一条使会话/消息计数与 max id 不变（`cass forget` 后重新导入同规模、或对会话做等价条数的内容替换）；此时 lexical checkpoint 会被判 current，增量 run 可能不重建 lexical；semantic 的续传位置也可能停在旧 checkpoint。
**反证/界限**：正常 ingest 只追加，message id 递增 → 指纹变化；`forget`/dedup 路径在 SQLite 侧显式调用 `rebuild_lexical_index_after_dedup`（`sqlite.rs:11242-11251`）；semantic reconcile 逐 doc 校验会使 identity 集合收敛。真正裸奔的是“不经过这些显式重建/对账入口”的保量改写。未运行实证，属于静态可达。

### CS-2 · P2 · FK 关闭 + 孤儿清理默认跳过
**证据**：begin-concurrent 写者 `PRAGMA foreign_keys = OFF`，失败仅 debug（`mod.rs:26128-26135,25978-25984`）；孤儿清理的动机注释明确写着“Connection dropped mid-transaction 会留子行无父行，随后每次写入都被 FK 拒绝，backlog 无界增长”（`sqlite.rs:4639-4656`）；清理 pass 默认跳过（`CASS_PREFLIGHT_CLEANUP_ORPHAN_FK_ROWS=1` 才启用），失败时才 abort（`mod.rs:13459-13501`）。
**可达**：并发 chunk 路径在 OOM/kill/连接异常时留下孤儿的概率高于 FK=ON 路径；随后普通索引持续撞 `FOREIGN KEY constraint failed`，而在默认配置下自动自愈被跳过。
**反证/界限**：同事务提交本身原子；`with_concurrent_retry` + serial fallback 覆盖多数冲突；显式开启环境变量即可自愈；孤儿只影响本地归档的派生一致性，不删源文件。

### CS-3 · P2 · 派生 FTS 的“写入吞错 + 默认不校验”
**证据**：`franken_batch_insert_fts` 对任何插入错误 warn/debug 后 `return Ok(inserted)`（`sqlite.rs:14148-14174`）；routine 的 `validate_fts_messages_integrity` 预检默认关闭（`mod.rs:13342-13419`，`preflight_validate_fts_messages_enabled()` 未开启时 debug 跳过）；full run 后才 best-effort 修复 fallback FTS（`mod.rs:14842-14911`）。
**可达**：增量 run 中若 fts_messages 缺失（如 v14 DROP 后未重建、或重建中断）或写入失败，默认不会在 run 内发现；检索降级行为改走 SQLite message-scan 兜底（分片 B 证据），属于“可用性降级而非结果错误”。
**反证/界限**：parity 检查是精确的（三数+交集），Partial 可 resumable 修复，Unqueryable/Excess/Divergent 明确拒绝破坏性原地重建（`sqlite.rs:10989-11100,11127-11166`）；`doctor check` 可显式发现。问题在“默认不跑门”而不是“门不严”。

### CS-4 · P3（未运行验证）· 批量插入的 rowid 反推
**证据**：多行 INSERT 后取 `franken_last_rowid`，`first_id = last_id - (n-1)`，按偏移生成 message id 列表（`sqlite.rs:13577-13586,13713-13722`）；这些 id 紧接着用于 snippets 与 FTS 条目（`sqlite.rs:10031-10050,13536-13544`）。
**可达**：若底层存储的 rowid 分配在该语句内并非严格连续（例如删除后 rowid 复用策略变化、或同表并发写者交错），推断 id 会指向错误的行，导致 snippets/FTS 与消息错绑。
**反证/界限**：同一事务、同一连接、单条多行 INSERT 在常规 SQLite/frankensqlite 语义下分配连续 rowid；有 `checked_sub` 溢出保护；begin-concurrent 每个 chunk 使用独立写者但同一写者的语句串行执行。本轮禁止运行，未验证。

### CS-5 · P3 · forget 的 SQL 拼接与 delete_source 的 cascade 空转
**证据**：`forget_conversations_by_source_glob` 把匹配 id `join(",")` 后直接 `format!` 进 DELETE/COUNT SQL，无绑定参数、无数量上限（`sqlite.rs:7723-7764`）；`delete_source(id, _cascade)` 完全忽略 cascade 参数，仅删除 `sources` 单行（`sqlite.rs:9833-9844`）。
**可达**：超大匹配集会生成超长 SQL 语句（解析/语句尺寸风险）；若调用方依赖 cascade 语义（级联删除该 source 的 conversations），行为与命名不符——FK ON 时删除会被引用拒绝，FK OFF 时可能留下悬挂 `source_id`。
**反证/界限**：id 全部来自数据库整数集合，不构成注入面；匹配上限受库内会话数约束；`cascade` 的调用方未在本分片审计（调用点在 `mod.rs`/CLI，本轮未读），不能断言存在依赖该语义的真实调用。

### 其他观测（P3/信息）
- **P3-1 直接 CLI 语义发布与 watermark 不同步**：`publish_direct_semantic_artifact` 失败仅 warn，随后 `set_last_embedded_message_id(max_id)` 照常执行（`mod.rs:14691-14709,14722-14731`）→ manifest 可能停留在旧 artifact 而水位已推进，状态显示 stale/unavailable；下次 backfill 可修，但状态与真实资产存在窗口性偏差。反证：backfill 路径的 `manifest.save` 是错误上抛的（`semantic.rs:2977-2996`），仅直连 CLI 路径有该偏差。
- **P3-2 embed watchdog 会中止整批**：单批超过 300s 直接 bail（`semantic.rs:2219-2224`）。界限：char budget 已把最坏 batch 压到 ~8 行，阈值可用 env 调整；staging/checkpoint 不推进，重试从旧 checkpoint 再来。
- **P3-3 语义噪声过滤与 lexical 的噪声定义需保持同源**：整库语义构建在内存里 `retain(!is_hard_message_noise(...))`（`mod.rs:14585-14587`）；lexical 侧噪声期望用 `lexical_rebuild_noise_role`/同一 predicate 计算（`mod.rs:8900-8947`）。两处若漂移，语义 reconcile 的精确集合校验会 fail-closed（`semantic.rs:2779-2781`），不会发布错集，但会造成反复失败的维护面。

## 八、ASG 对照：强在哪、弱在哪、该学/不该学什么

### 强（cass 值得学）
1. **失败原子发布 + 拒绝不可证明的破坏性修复**：FTS Unqueryable/Excess/Divergent 拒做原地重建（`sqlite.rs:11157-11166`）、语义 reconciliation 先私有 staging+全量校验再 rename（`semantic.rs:2600-2820`）、lexical staged publish 失败回滚（`mod.rs:13786-13794`）。
2. **水位纪律**：全局 `last_scan_ts` 仅在成功且未启用排除时推进；连接级水位只对成功连接推进（`mod.rs:14743-14773`）；`stale quarantine` 在水位保留时不标记已重试（14472-14491）。这是 ASG “权威扫描/失败不推进 cursor”的同类契约。
3. **对账删除有界化**：孤儿 FK 清理分块 + OOM 二分 + 每块事务（`sqlite.rs:5923-6222`）；staging 残骸在独占锁下回收（`mod.rs:13087-13093`）。
4. **并发写工程化**：begin-concurrent 分片重试（明确不重试 corruption）+ 串行 fallback + 按输入序归并（`mod.rs:25836-26037,26174-26209`）。
5. **大库保护**：自动权威修复按 DB 尺寸 defer 而非硬做；full rebuild 不 eager 删（`mod.rs:1834-1863,13862-13872`）。

### 弱（ASG 不该学）
1. **把基数+max id 当内容身份**（CS-1）：ASG 的 generation/cursor 失效必须用内容哈希/变更日志，而不是 `COUNT+MAX(id)`。
2. **FK OFF + 默认跳过孤儿对账**（CS-2）：ASG 的非破坏扫描更应保持 FK ON，或把低成本孤儿对账纳入常备水位检查。
3. **派生索引写入吞错 + 默认不校验**（CS-3）：ASG 若建派生索引，失败必须进入结构化 `degraded/stale` 状态并默认可感知，而不是 debug 日志。
4. **产出与元数据不同步的窗口**（P3-1）：发布产物成功但 manifest 仅 warn，同时推进 watermark。
5. **无绑定参数的 ID 列表拼接**（CS-5，影响面小但模式不应带走）。

### ASG 对照结论（面向非破坏扫描/generation/cursor/失败不删数据）
- **可直接借鉴**：staged publication + “无法证明可恢复就不原地重建”的 fail-closed 规则；扫描水位与连接水位分离并带 preserves 条件；generation manifest 的 Built/Validated/Published 状态机与 attempt/generation id；OOM 二分删除与 staging 残骸回收；重试白名单（corruption 不重试）。
- **需要改造后再用**：cass 的 fingerprint 机制换成 ASG 已有的内容/世代标识；FTS 类派生索引的 best-effort 语义必须补默认可见的 parity 状态；并发写不要用关 FK 换吞吐。
- **与 ASG 现有原则的一致性**：cass 在 full rebuild 中“不 eager 删除”、拒绝不健康归档被替换、语义/fusion 失败不删旧数据——方向和 ASG“失败不删数据、失败不推进 cursor”一致；分片 A/B 的结论（asset_state 锁/心跳、quantized 元数据、fail-open 到 lexical 的结构化 fallback）与本分片互补，三片合起来才构成完整对照。

## 九、残余未决与精确剩余账本

### 已读账本（本轮）
- `src/indexer/semantic.rs`：6229/6229 行，full（42 段，全部 1-6229 顺序覆盖；codegraph 虚拟 EOF 行 6230 不计）。
- `src/storage/sqlite.rs`：6610/30047 行，partial。未读区间（精确）：301-544、1744-3101、3852-4419、5020-5699、5850-5922、6223-7563、7864-9818、10597-10656、11407-11642、11793-11999、12600-12701、12852-13448、13749-13956、14240-30047。其中 14240-30047 含大量 query helper、analytics 重建、historical salvage 与内联测试（测试自 17104 起）。
- `src/indexer/mod.rs`：4762/52111 行，partial。未读区间（精确）：1-1684、1985-2796、2861-6219、6270-6428、6579-6671、6822-8689、8840-8887、9038-10280、10581-12370、12521-12579、12730-13045、15296-16879、17030-25835、26368-26610、26827-52111。未读含分片规划器/等价性机器、streaming consumer 内部、watch helper、quarantine 接线与全部内联测试（29546-52111）。

### 未决/未验证（明确不做断言）
- 本分片禁止 build/test/run，所有并发/崩溃/rowid 推断类结论均为静态可达性，未做运行复现（CS-4、CS-2 的孤儿概率、CS-1 的保量改写场景）。
- `quarantine.rs` / `quarantine_retry.rs` / `staging_reclaim.rs` / `parallel_wal_shadow.rs` 未逐行读（属分片外），只按调用点转述。
- in-DB FTS 的查询消费与降级语义在检索层（分片 B 的 query.rs）覆盖，本片只审产生/维护/校验。
- `delete_source` 的调用方语义、`forget` 的 CLI 参数流在 `lib.rs`/CLI 层（未读）。
- 依赖侧：frankensqlite 的 rowid 分配、MVCC BEGIN CONCURRENT 语义、`VACUUM INTO` 失败模式均为库外行为，本分片未做第三方实现审计。

### 外部参考与所有权
- 本报告只引用已读源码行；未复制竞品实现；未执行网络/git 写操作；不修改 `coverage-cass-pack.json`、`coverage-cass-query.json`、`sweep-coding_agent_session_search.json`、sessiongrep 基准与任何竞品文件。
- 机器可核凭证：同目录 `coverage-cass-storage.json`（每文件 bytes/total_lines/sha256_initial/read_ranges/status/reason/sha256_final/content_unchanged/description/read_line_count/read_method/missing_ranges + 顶部 schema_version/date/task/repository/commit/scope/status/t1_scope_rule + summary）。