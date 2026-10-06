# Research: ctx 竞品源码审计（分片 A：capture / store / search packet）

- Query: 对 ctx@06bc5ed1 快照的 capture/store/search packet 分片做第一方静态审计：事件模型与迁移、身份/去重/时钟、增量与新鲜度、packet 字段级契约、provider 覆盖与归一化边界、capture 失败/半写处理，并与 ASG（handoff-pack 与证据锚定）对照。
- Scope: internal；仅 `Github_src/ctx` 源码与契约文本；规划研究，不实施整改。
- Date: 2026-10-06
- Recorded snapshot: `06bc5ed17ce4d0f6c8255981e009f86802dbe32f`（工作树仅 `.codegraph/` 未跟踪，见下）。
- Status: T1 必读 7 文件全文阅读完成 + packet 契约补充验证完成；本报告只记录静态证据，未 build/test/run/联网/改 git。
- 阅读凭证: 同目录 `coverage-ctx-core.json`（25 个文件、11333 行阅读区间、逐文件 SHA256 初/终校验）。行号均为 1-based inclusive；codegraph 的“虚拟 EOF 行”不计入。

## 1. 覆盖统计

| 必读文件 | 行数 | 状态 | 段数 |
|---|---:|---|---:|
| `crates/ctx-history-store/src/search/projections.rs` | 2021 | full | 15 |
| `crates/ctx-history-store/src/catalog.rs` | 956 | full | 9 |
| `crates/ctx-history-store/src/schema/migrations.rs` | 1031 | full | 7 |
| `crates/ctx-history-capture/src/provider/importer.rs` | 1011 | full | 7 |
| `crates/ctx-history-capture/src/provider_sources/discovery.rs` | 977 | full | 7 |
| `crates/ctx-history-capture/src/provider/native.rs` | 1272 | full | 9 |
| `crates/ctx-history-search/src/packet.rs` | 123 | full | 1 |
| 合计 | **7391** | 7 full / 0 partial | 55 |

补充阅读（用于回答 packet/store 问题，逐段有界）：`results.rs`(393 full)、`search.rs`(466 full)、`query.rs`(140 full)、`events.rs`(531 full)、`importer/batches.rs`(409 full)、`importer/identity.rs`(388 full)、`importer/ids.rs`(326 full)、`common/identity.rs`(40/60 partial)、`dtos.rs`(350/781 partial)、`sync.rs`(135/219 partial)、`ddl.rs`(132/621 partial)、`fts.rs`(45/70 partial)、`connection.rs`(75/359 partial)、`ctx-cli/src/main.rs`(118/1107 partial)、`search_render.rs`(130/326 partial)、`docs/contracts/json.md`(111/683 partial)、`contracts/.../search.results.json`(92 full)、`sdks/.../SearchPagination.java`(61 full)。未读区间见 coverage JSON 的 `missing_ranges`。

T2 机械交叉核对（只做过滤查询，不计入阅读）：sweep-ctx.json 对 7 个必读文件的探针命中为 projections(index_storage 22 / secret_redaction 60 / surface_mcp 33)、importer(provider_paths 4 / inline_tests 5)、native(inline_tests 7 / surface_mcp 41) 等；探针名是机械分类，本报告的结论只以逐段原文为准。

## 2. 事件模型与 schema / 迁移

### 2.1 实体与事件模型

- `Event`（`crates/ctx-history-core/src/dtos.rs:522-549`）：`id`、`seq`、`history_record_id`、`session_id`、`run_id`、`event_type`、`role`、`occurred_at`、`capture_source_id`、`payload`(JSON)、`payload_blob_id`、`dedupe_key`、内嵌 `sync`(visibility/fidelity/sync_state/sync_version/deleted_at/metadata)。事件没有字符/字节 span、行号或父消息边字段。
- `EventType` 11 值（`dtos.rs:98-112`）：message / tool_call / tool_output / command_started / command_output / command_finished / file_touched / vcs_change / artifact / summary / notice；`EventRole` 5 值（`dtos.rs:115-123`）。
- DDL（`crates/ctx-history-store/src/schema/ddl.rs:358-377`）：`events.id TEXT PK`、`seq INTEGER NOT NULL UNIQUE`（全局唯一序号）、`payload_json TEXT NOT NULL DEFAULT '{}'`、`dedupe_key TEXT` **无 UNIQUE**、`visibility/sync_state` 有 CHECK（**不含 withheld**，见 CTX-04）。
- 会话/运行/边：`sessions`（`ddl.rs:284-308`，含 parent/root/capture_source、agent_type/status CHECK）、`runs`（`ddl.rs:334-356`）、`session_edges`（`ddl.rs:317-332`；`SessionEdgeType` 6 值，`dtos.rs:62-71`）。
- 搜索投影：FTS5 `event_search(event_id UNINDEXED, history_record_id UNINDEXED, session_id UNINDEXED, role UNINDEXED, preview_text, rank_bucket UNINDEXED)` + `event_search_scriptgram`（`schema/fts.rs:16-43`）；另有 `event_search_lookup` 关系表（`ddl.rs:386-393`）与 `ctx_history_search`/`artifact_search`。
- `catalog_sessions` 以 `source_path` 为 PK（`migrations.rs:911-941`），`source_import_files` 以 `(provider, source_root, source_path)` 为 PK（`migrations.rs:969-987`）。

### 2.2 schema 与迁移

- `SCHEMA_VERSION = 47`（`crates/ctx-history-store/src/lib.rs:49`，grep 定位）；只读快照打开时 `user_version != SCHEMA_VERSION` 直接报 `UnsupportedSchemaVersion`（`connection.rs:49-52`）；可写打开执行 migrate → recover bulk mode → `ensure_search_projection_initialized`（`connection.rs:80-96`）。
- 迁移驱动（`migrations.rs:27-96`）：按 `user_version < N` 顺序执行 1..16，然后跳至 42..47；17..41 无升级分支（仅对预发布版本号有意义，未验证实际分布）。`prepare_provider_session_migrations` 在入口先行（`migrations.rs:28`）。
- 每个迁移自带 `BEGIN IMMEDIATE` + 成功 COMMIT / 失败 ROLLBACK（如 `migrations.rs:98-121`）；需要重建表时先 `PRAGMA foreign_keys=OFF`，结束/失败后按原值恢复（`migrations.rs:173-201,259-288,290-341,455-493,495-530,532-567,569-604,606-640,642-680,682-717`）。
- 直接与 schema 相关的重要迁移：v11 在事务内 `rebuild_search_projection`（`migrations.rs:388-408`）；v12 `invalidate_provider_import_indexes` + 重建投影（`migrations.rs:410-431`，失效逻辑 719-749）；v14 回填 `catalog_sessions.last_imported_* = indexed_*`（`migrations.rs:752-769`）；v44 重建当前 schema 表、drop/recreate FTS、按需重建投影，但 `run_migrations` 传入 `rebuild=false`（`migrations.rs:83-85,642-680`）；v47 是独立模块 `v47_provider_session_repair`（`migrations.rs:3-5,25,92-94`，本分片未读）。
- provider 允许列表在重建 DDL 中硬编码为长 CHECK（`migrations.rs:860,914,971`，约 46 个值，含 codex/claude/.../custom/unknown/mimocode）。新增 provider 必须同时改迁移重建函数，否则写入被 CHECK 拒绝。

## 3. 身份、去重、时钟

### 3.1 身份与去重

- 稳定 UUID 不是随机 UUID：`stable_capture_uuid` 用两段 FNV-1a64 拼 16 字节，第 7 字节置 `0x70`（UUIDv7 形状）、第 9 字节置 variant 位（`crates/ctx-history-capture/src/common/identity.rs:8-20`）；`compute_payload_hash` 同样是 `fnv1a64`（`common/identity.rs:22-25`）。
- source 身份 = JSON 元组 `("provider-source-v2", provider, provider_session_id, source_format, raw_source_path)` 经 `stable_capture_uuid(..., "source")`（`importer/ids.rs:18-49`，调用点 `importer.rs:483-489`）。source_root/identity 组件按 root→path→metadata(source_id/native_source_id/...)→idempotency_key 优先级取值（`ids.rs:90-120`）。
- session 身份解析顺序（`importer.rs:399-439`）：先查 `(capture_source_id, provider, provider_session_id)` 已有行；否则用 source-scoped UUID（`ids.rs:197-205`）；旧库回退到 provider-scoped UUID（`ids.rs:190-195`），但必须通过 `legacy_session_matches_source` 校验（`importer.rs:429-434`）。
- event 去重键两种格式：`provider-source:{source_id}:{index}:{payload_hash}` 与 legacy `provider:{provider}:{external_session_id}:{index}:{payload_hash}`（`events.rs:18-39`）；event UUID 由 `(source_id,index)` 派生（`ids.rs:262-267`）；seq 采用 `(fnv1a64(source_id) & 0x7fffffff) << 32 | index` 打包（`ids.rs:290-294`），冲突时用掩码 XOR + 1..1024 盐轮询换 seq（`importer/identity.rs:250-283`）。
- 真正的一致性靠三层：写前按 dedupe_key 查询（`events.rs:41-58`）；快速路径 `INSERT OR IGNORE`（`events.rs:119-163`）；同身份同 index 但 hash 不同则显式 `ProviderEventConflict`（`events.rs:366-427`，前缀区间扫描）。`dedupe_key` 列本身没有 UNIQUE 约束，重复防护不依赖数据库唯一索引。
- Pi 特殊：`entry_id` 可把已有事件身份重绑（`identity.rs:66-125`；调用 `importer.rs:705-739`）；file-touch 复用已解析 event 的 session，避免生成第二个 session 身份（`importer.rs:319-330`）。

### 3.2 时钟

- provider 时间统一走“秒/毫秒启发式”转换：`|value| > 1e12` 视为毫秒，否则秒×1000（`native.rs:266-279`），越界返回 None；`provider_required_*` 对必需字段报 `InvalidPayload`（`native.rs:290-299,310-319`）。字符串字段支持 RFC3339 或数字字符串（`native.rs:321-339,861-879`）。
- 缺失/非法时间回退到 `context.imported_at`（导入时刻），而不是当前时刻（`native.rs:281-288,301-308`；`importer.rs:602-604,759`）；`timestamps(at)` 令 created_at=updated_at=导入时刻（`ids.rs:310-315`）。事件 `occurred_at` 保留 provider 时间（`importer.rs:759`）。
- 结论：occurred_at（事件时序）与 last_imported_at_ms（文件级水位）是两条时间线，ctx 没有把导入时刻混入 occurred_at（fixture 路径 `observed_at=imported_at`，`importer.rs:923-945`）。

## 4. 增量与新鲜度

- `catalog_sessions` upsert 冲突键是 `source_path`（`catalog.rs:146`）。size/mtime 变化时 `indexed_at/indexed_file_*/indexed_status/indexed_error/indexed_event_count` 全部回退（NULL 或 pending，`catalog.rs:161-196`）。
- `last_imported_*` 只在“文件增长 + 旧基线一致”时保留（append-only 语义：`file_size 变大` 且旧 indexed 与旧 last_imported 都匹配旧 size/mtime，`catalog.rs:197-256`）；其余情况清空重导。更新只在实际有字段变化时执行（`catalog.rs:258-271`）。
- 成功导入：`mark_catalog_source_indexed` 同时写 `indexed_*` 与 `last_imported_*`（含 `last_imported_file_sha256`，`catalog.rs:399-436`）；失败：`mark_catalog_source_failed` 清 `indexed_*`、置 failed+error，但不动 `last_imported_*`（`catalog.rs:438-470`）。last-good 与本次尝试因此可区分。
- pending 判定（`catalog.rs:887-913`）：`indexed_status != 'indexed'` 或 size/mtime 不匹配，或**对应 sessions 行不存在**（provider+external_session_id，capture_source 存在时再校验 source_root）。indexed 计数同样要求 sessions 行存在（`catalog.rs:929-952`）。
- `source_import_files` 的失效条件更细：format/size/mtime 变化，或 inventory_unit=source_root 且 metadata 变化时清 indexed_*（`catalog.rs:526-576`）；缺失路径按 temp 表 set is_stale=1（`catalog.rs:600-638`）。
- 搜索侧新鲜度不在核心 packet 内：packet 只有 `generated_at`/`truncation`（`packet.rs:13-22`）；`freshness`/`retrieval` 是 CLI 包装层字段（契约文档 `docs/contracts/json.md:392-425`，实现未审）。Store 打开时若投影为空会重建（`connection.rs:95`；`projections.rs:757-799`），但非空即不校验（见 CTX-02）。
- 语义索引有一个可调统计缓存键 `semantic_searchable_lite_turn_items_v3`（`projections.rs:25,359-361,880-926`），计数增减由 `adjust_semantic_searchable_item_stats` 维护（`projections.rs:928-957`），不是逐事件全量统计。

## 5. capture 失败与半写处理

- envelope 版本闸门：`schema_version` 不在 [MIN_SUPPORTED, CURRENT] 直接 `InvalidPayload`（`importer.rs:465-473`）。
- “无真实会话消息”策略：provider-native/export 信任级别下，若某 session 的 capture 中没有任何 `EventType::Message` + user/assistant/system + 非空 text 的事件，则整组 capture 与 file-touch 被丢弃并计数（`importer.rs:125-189`）；若整批都没有真实消息且无其他失败，追加一条 `line:0` 的失败记录（`importer.rs:182-188`，批级同逻辑 `batches.rs:110-122`）。Custom provider 与非 native/export 信任不适用（`importer.rs:191-233`）。
- 批事务：单位/字节双阈值轮转——`IMPORT_TRANSACTION_BATCH_UNITS=64`、`IMPORT_TRANSACTION_BATCH_BYTES=8MiB`（`batches.rs:11-12,87-89`）；`prepare_unit`/`record_unit` 超限时 `rotate()` = commit + WAL checkpoint 请求 + 立即新开（`batches.rs:341-387`）。
- 错误分类（`batches.rs:194-234`）：`CaptureError::Store(_)` → 回滚当前批次并中止整个导入；其他（payload 解析类）→ 计数失败、继续处理后续行。游标只在 `summary.failed == 0` 时写入（`batches.rs:180-188,235-245`）。pending 父子边在事务内统一解决（`batches.rs:282-299`；解析逻辑 `importer.rs:827-894`）。
- 半写边界：轮转意味着**较早批次已经提交**；中途 store 错误只回滚当前批次，已提交行仍在库里；但游标未推进、去重键/insert-if-absent/冲突拒绝保证重试收敛（`events.rs:41-58,119-163,366-427`；`importer.rs:788-817` 对重复/冲突计 skipped 而非报错）。当前没有“已提交到哪个批次”的持久回执，只能从 catalog 的 last_imported_* 与 pending 条件间接判断（CTX-05）。
- 重复/冲突降级：非快速路径的 `ProviderEventConflict` 计 `skipped_events` 并提前返回（`importer.rs:801-806`）；快速路径靠 `INSERT OR IGNORE` 返回 false 计 skipped（`importer.rs:788-793,811-817`）。
- 投影重建的半写：`rebuild_search_projection` 先逐表 `DELETE` 再重建、函数内部无 BEGIN（`projections.rs:580-660`）；直接调用点 `import.rs:830`、`import/native.rs:66,91` 均无外层事务；打开时的自愈只在投影计数为 0（或 lookup 表空）时触发（`projections.rs:757-799`），混合半重建不会被自动修复（CTX-02）。
- provider 读取的资源边界：SQLite 只读 + `SQLITE_LIMIT_LENGTH=16MiB` + `query_only` + 5s busy timeout（`native.rs:243-254`）；JSONL 单行上限 16MiB（`lib.rs:10` + `native.rs:840-849`）；软链接 transcript 明确拒绝（`native.rs:819-838`）。

## 6. provider 覆盖与归一化边界

- 发现层：`discover_provider_sources` 遍历 `PROVIDER_SPECS` 的 default_locations，再叠加各 provider 的 env/config 特例（Kilo/MiMoCode/ForgeCode 独立函数；Crush/Goose/Qwen/Kimi/Junie/Warp/Trae/Cline/Roo 等；`discovery.rs:20-70,72-377`）；结果按 `(provider, path, source_format)` 去重（`discovery.rs:792-798`）。用户给定路径时用 `provider_source_for_path` 映射 source_format（`discovery.rs:825-903`）。
- 状态模型：`Unsupported / Missing / Empty / Available / Unknown`，其中 Empty/Unknown 由有界探测 `BoundedProbe::{NotFound,BudgetExhausted,IoError}` 决定（`discovery.rs:930-977`）；`try_exists` 出错时 `exists` 保守地记为 true（935-936）。探测预算常量在 `probes.rs`（本分片未读，见残余未决）。
- 可导入集合由 spec.import_support 决定（`discovery.rs:904-925`）；DDL 允许列表（~46 provider，`migrations.rs:860`）只约束可写值，不代表都实现了导入——两者差集需读 `specs.rs` 才能确定。
- 归一化边界（native 通用层）：role 映射 user/assistant/system/tool/unknown（`native.rs:782-790,894-903`）；block 类型判断 tool_result→ToolOutput、tool_use→ToolCall、system→Notice、其余 Message（`native.rs:905-928`）；message id 缺失时生成 `message-{index}`（`native.rs:881-892`）；文本提取覆盖 text/content/message/prompt/response/output/summary 与 parts（`native.rs:943-1006`）。
- 保留策略是显式且有损的：Message/Summary 文本上限 16000 字符、Tool* 输出预览上限 4000 字符（`lib.rs:55-56`；策略 `native.rs:454-489`）；成功的 ToolOutput/CommandOutput 文本**整体不保留**（limit=None → 空文本，`native.rs:505-509`），失败输出保留有界预览并剔除 patch/diff（`native.rs:469-480,648-671`）；body 字段级省略（output/stdout/stderr/diff/patch/** 等键，`native.rs:555-599`），省略处写入 `field_retention{original_bytes,contained_patch_or_diff}`（`native.rs:602-610`）与 `text_retention{mode,limit_chars,truncated,omission_policy,omission_applied}`（`native.rs:437-452`）。模块内 5 个测试断言了这些行为（`native.rs:1078-1271`）。
- 事件入库时的 payload 是包装体：`{provider, provider_session_id, provider_event_index, provider_event_hash, cursor, artifacts, body}`（`importer.rs:761-769`）；搜索投影只从中抽取文本预览（`projections.rs:1679-1686`）。

## 7. packet 契约字段级事实

### 7.1 结构与字段

- `SearchPacket`：`schema_version(=1)`、`query`、`filters`、`generated_at`、`results[]`、`pagination`、`truncation`（`packet.rs:11-22`）。空包 `pagination(Some(0),false)` → cursor=None、has_more=false（`packet.rs:102-112`）。
- `SearchPacketResult`（`packet.rs:24-74`）：`record_id`；可选 `session_id/event_id/event_seq`；`title/snippet/rank`；`result_scope`（Session|Event，默认 Event，序列化 snake_case，`packet.rs:76-82`）；`more_matches_in_session`、`session_importance`；provider/session/source 身份五元组（provider、provider_session_id、history_source(_plugin)、provider_key、source_id、source_format）；`timestamp/cwd/raw_source_path/raw_source_exists/cursor`；`why_matched[]`、`citations[]`、`links{}`、`visibility`。
- CLI 层再包一层：`payload_type:"search_results"`、`freshness`、`retrieval`，把 citation 改名为 snake_case（`search_render.rs:21-71`；字段契约 `docs/contracts/json.md:345-388`）。

### 7.2 citations 怎么锚定

- 结构（`dtos.rs:742-760`）：`type`(event/session/run/vcs_change/artifact/summary/file/history_record，`dtos.rs:362-373`)、`id`(Uuid)、`label`、`time`、`provider`、`session_id`、`event_seq`、`raw_source_path`、`raw_source_exists`、`cursor`。
- 生成（`results.rs:241-276`）：每条 event 结果至少带一个 Event citation（id=event_id、event_seq、time=occurred_at、cursor 来自 `event_search_cursor`），若有 session 再加一个 Session citation；`raw_source_exists` 在查询时用 `Path::new(path).exists()` 现算（`results.rs:247-250`）。
- cursor 来源（`projections.rs:1817-1845`）：`payload.cursor` → `payload.body.cursor` → `source_metadata.cursor.after.cursor`，取不到为 None。
- 证据粒度边界：锚点是“事件身份 + provider 游标 + 原始路径”，**没有**字符/字节 span、行号、内容哈希；snippet 不是从原始 payload 现取，而是从索引期就被裁到 2048 字符的 `preview_text` 切片（`projections.rs:1639-1666`；`results.rs:284`）。`links` 恒为空对象（`results.rs:391-393`；`dtos.rs:762-763`）。
- `why_matched`：事件类型词（message/tool_call/.../command_event/file_touched/vcs_change/artifact/summary/notice，`results.rs:349-363`）+ 多词查询时的 `term:{term}`（`search.rs:239`）+ 语义侧的 `semantic_similarity`/`semantic:{reason}`（`search.rs:149-156`）。

### 7.3 visibility 过滤在哪一层

- 模型层：`Visibility` 有 5 值（local_only/reportable/sync_metadata/sync_full/withheld，`sync.rs:9-18`）；`SyncState`/`RedactionState` 也各有 withheld。
- 索引层（lexical 的真正关卡）：`event_searchable_event_parts` 在写投影时排除 deleted、`visibility==Withheld`、`sync_state==Withheld`、redaction Raw/Withheld（`projections.rs:1600-1622`）；空预览跳过（`projections.rs:1311-1313,1423-1425`）。lexical FTS 查询 SQL 内没有 visibility 谓词（grep 全仓 withheld 只落在 core/store 投影/测试与 sync.rs），因此 FTS 的可见性完全依赖索引时过滤。
- 语义层：semantic-lite CTE 在查询 SQL 里再次断言 `deleted_at_ms IS NULL AND visibility != 'withheld' AND sync_state != 'withheld'`（`projections.rs:1005-1020`，并出现在 422-423、1090-1092、1115-1117、1141-1143、1172-1174 等分支）。
- 结果层：`SearchPacketResult.visibility` 是硬编码 `Visibility::LocalOnly`（`results.rs:195,304`），既不来自行数据也不做聚合——SDK 不能拿它做过滤。
- 原始读取层：`get_event`/`events_for_session`/`events_for_record`/`list_events` 及 `event_select_sql` 无 visibility/deleted 过滤（`events.rs:185-200,228-234,303-328,451-509`）。
- DDL 矛盾：`events/sessions/session_edges/artifacts` 与 `capture_sources` 的 CHECK 均只允许 4 个值、不含 withheld（`ddl.rs:275,303,326,371-373`；`migrations.rs:873-874`），所以当前 schema 下带 withheld 的行根本写不进去，投影谓词对当前库不可达（对旧库可能有意义）。详见 CTX-04。

### 7.4 分页/截断语义

- 形状：`ContextPagination{cursor?, has_more}`（`dtos.rs:766-771`）；`pagination(cursor_base, has_more)` 仅在 has_more 时生成 `cursor="offset:{n}"`（`packet.rs:114-123`）。四处生成点都把 `n` 设为**已返回结果条数**（`search.rs:61-68,186-194,268-276,456-463`），不是扫描游标。
- 截断：reason 三值——`scan_budget`（fast path 翻页预算耗尽，页大小 500、最多 20 页，`search.rs:354-366,420-422,440-445`）、`limit`（结果数超过 limit，`search.rs:55-59,195-200,446-451`）、`source_limit`（多词合并时子查询本身被截断，`search.rs:259-267`）。`omitted_results` 语义不一致：慢路径是候选差值、fast/semantic 路径写死 1、多词路径累加（`search.rs:57,199,237,254,263`）。
- **cursor 没有消费者**：`PacketOptions` 只有 limit/snippet_chars/filters/result_mode（`query.rs:24-30`）；CLI `SearchArgs` 明确没有 offset/page/cursor（`main.rs:317-434`，读到结构结尾 434 行）；MCP search 工具只暴露 `limit`（grep `mcp.rs:719`，且 mcp.rs 无 offset/cursor/page 命中）。`offset:` 字符串在全仓只有 packet.rs:117 生成处（grep）。limit 被 clamp 到 1..200（`query.rs:103-110`），所以超过 200 的命中没有任何取回路径。
- 契约分裂：Rust packet 序列化为 `cursor/has_more`；`contracts/agent-history-v1/fixtures/search.results.json:85-87` 的 pagination 是 `{"limit":20}`；JVM `SearchPagination.java:18-55` 读 `limit/offset/total/nextCursor/hasMore`；TS 测试期望 `next_cursor/has_more`→camelCase（`sdks/typescript/test/client.test.js:130,175-176`，grep）。三种形状没有一层做转换（CLI `search_render.rs:69` 原样嵌入 packet 对象）。详见 CTX-01。

### 7.5 失败行为

- 唯一错误类型是 store 错误透传（`SearchError::Store`，`query.rs:16-22`）；`search_packet`/语义/多词路径对 candidate/semantic/term 查询都用 `?` 冒泡（`search.rs:44,101,235,371`）。
- 提前空包（不报错、无降级标记）：provider 过滤且 `has_provider_data=false`（`search.rs:29-33`）、file 过滤 scope 为空（`search.rs:34-37`）、语义查询空串（`search.rs:82-84`）。
- 静默降级：`event_search` 表不存在时 `search_event_hits_page_with_ranking` 返回 Ok(vec![])（`projections.rs:134-136`）；重建函数在表缺失时直接 Ok（`projections.rs:580-583`）。消费者无法区分“没有命中”和“索引缺失/半重建”。
- packet 内没有 freshness/partial 字段（`packet.rs:13-22`）；陈旧/刷新失败信息只在 CLI 包装的 freshness 里（文档语义 `docs/contracts/json.md:392-409`，实现未审）。

## 8. 发现清单（file:line + 严重度 + 证据 + 反证）

### CTX-01 · P2 · pagination 只写不读，且 Rust/SDK 三种 pagination 形状不一致
- 证据：`packet.rs:114-123` 生成 `offset:{n}`；`search.rs:61-68,186-194,268-276,456-463` 把 n 定义为已返回条数；`query.rs:24-30` 无 cursor 输入；`main.rs:317-434` 无 offset/page/cursor 参数；`contracts/.../search.results.json:85-87` 与 `sdks/jvm/.../SearchPagination.java:18-55` 期望 limit/offset/total/nextCursor/hasMore；`search.rs:55-59` 明确 reason=limit 截断。
- 反证/界限：`--limit` 可提升到 200（`query.rs:10,103-110`），且排序在同一快照内稳定（`results.rs:96-106`），小结果集不受影响；cursor 无消费者不等于数据丢失，只是“契约承诺了取回能力但实现是死字段”。`docs/contracts/json.md:355` 只列出 `pagination` 字段名，未定义语义（读到 440 行仍未给 cursor 规则）。
- ASG 教训：handoff-pack 若要给 cursor，必须定义输入/输出闭环，并用“第 1 页→第 2 页无重无漏”验收；不能只序列化一个无人解析的字符串。

### CTX-02 · P2 · `refresh_search_index` 无事务快照；失败/中断可留半重建索引且自愈条件过窄
- 证据：`projections.rs:580-660` 依次 DELETE 五张投影表再逐表重建，函数内无 BEGIN；调用点 `import.rs:830`、`import/native.rs:66,91` 未见外层事务（读取范围 `native.rs:52-94`、`import.rs:818-835`）；打开时自愈只在投影总行数为 0 或 lookup 为空时触发（`projections.rs:757-799`）；`event_search` 缺失时查询静默返回空（`projections.rs:134-136`）。
- 反证/界限：迁移路径的 `rebuild_search_projection` 都在 BEGIN IMMEDIATE 内（`migrations.rs:391,414`，v44 的 656 行同一事务）；正常导入走逐事件投影更新（`events.rs:107`）并在批次事务内提交；未做崩溃注入实测，“半重建可持久”是静态可达性判断。

### CTX-03 · P2 · citation 锚定只到事件粒度；snippet 来自 2048 字符索引预览，无 span/内容哈希
- 证据：`dtos.rs:742-760`（字段）；`results.rs:247-262`（exists 现算、cursor 透传）；`projections.rs:1665`（preview 截 2048 字符）；`results.rs:284`（snippet 来自 hit.preview）；`events.rs:526`（完整 payload 仍在 events 表，但搜索不读它）。
- 反证/界限：event_id+session_id+seq+provider 外部 id+provider cursor+raw path 的组合已足以定位到事件并在源文件中复查（`search_render.rs:91-130` 提供 `show/locate` 命令）；2048 字符内的命中证据密度不低；本报告没有实测“超过 2048 字符的命中会丢”的端到端用例，是从“索引文本=裁剪预览”直接推导。
- ASG 教训：handoff-pack 的 citation 至少要有 (a) 权威 payload 中的精确 span/quote，(b) 源快照指纹（大小+内容哈希），(c) 渲染时从权威数据取文而不是从搜索投影反读。

### CTX-04 · P2/P3 · `withheld` 在枚举/投影/DDL 三层不一致；packet.visibility 恒为 local_only
- 证据：`sync.rs:9-18` 有 Withheld；`projections.rs:1600-1622`、`1005-1020` 等有 withheld 谓词；但 DDL CHECK 只允许 4 值（`ddl.rs:371-373,303,326,275`；`migrations.rs:873-874`），带 withheld 的写入会被 SQLite 拒绝；`results.rs:195,304` 把结果 visibility 硬编码为 LocalOnly。
- 反证/界限：产品明示本地/私有默认为主（`docs/contracts/json.md:390`），本地原始读取不过滤 withheld 可能是有意设计；旧 schema 测试里存在 legacy `redaction_state` 含 withheld 的 v44 迁移用例（`schema/tests.rs:783-787`，grep），因此谓词对历史库仍有防御价值；未做真实写入复现。

### CTX-05 · P3 · 批量导入的半写窗口：已提交批次无回执、失败后只保证“游标不推进”
- 证据：`batches.rs:11-12,341-387`（64 单位/8MiB 轮转提交）；`batches.rs:199-209`（store 错误回滚当前批次并中止；解析错误跳过继续）；`batches.rs:180-188,235-245`（只有 failed==0 才写游标）；`catalog.rs:438-470`（失败清 indexed、留 last_imported）。
- 反证/界限：游标不推进 + 稳定去重键 + `INSERT OR IGNORE` + 前缀 hash 冲突拒绝（`events.rs:41-58,119-163,366-427`）使重试收敛，不会产生重复行；解析错误行有失败清单（`ProviderImportFailure`）。缺的是“已提交到哪”的可读水位，而不是安全性崩溃。

### CTX-06 · P3 · 身份与负载哈希使用 FNV-1a 64（非加密）
- 证据：`common/identity.rs:8-25`；`ids.rs` 全部派生 ID；`events.rs:366-427` 冲突检测也基于同族哈希比较。64 位碰撞可导致同 source+index 的两个不同 payload 被视为同一身份（先写入者胜出，后者计冲突/跳过）。
- 反证/界限：这是本地去重与幂等，不是安全边界；要利用需要能控制转录内容且命中同 source/index；UUID 字符串稳定且来源隔离（provider/source/format/path 都在 seed 里）。

### CTX-07 · P3 · 成功工具输出不保留正文（显式保留策略，但搜索/回放不可见）
- 证据：`native.rs:454-489,505-509`（成功 ToolOutput/CommandOutput → limit=None → text 为空）；`native.rs:555-599`（output/stdout/stderr/diff/patch 等键省略）；`native.rs:1103-1207` 测试断言成功输出 oracle 不出现在 payload；搜索预览只对失败的输出事件生成文本（`projections.rs:1659-1661`）。
- 反证/界限：省略是显式且可审计的——`text_retention`/`field_retention` 记录了 mode/limit/truncated/omission/original_bytes（`native.rs:437-452,602-610`）；原始 provider 文件仍在磁盘上。ctx 的定位更像“失败诊断”而非全量回放，这不是隐瞒性丢弃。

### CTX-08 · I · “空结果”与“索引不可用”在 packet 层不可区分
- 证据：`search.rs:29-37,82-84` 提前空包；`projections.rs:134-136,580-583` 缺表静默空；store 错误才冒泡（`query.rs:16-22`）；packet 无 freshness（`packet.rs:13-22`），刷新状态在 CLI freshness（文档 `docs/contracts/json.md:392-409`）。
- 反证/界限：CLI/SDK 有 freshness 字段可携带 `failed/budget_exhausted/read_only`，所以“不可区分”限定在核心 packet 契约；MCP/直接 store 调用不走该包装，是否受影响未实测。

## 9. 与 ASG 对照（packet / citation / pagination；面向 handoff-pack）

| 维度 | ctx 事实（锚点） | ASG 该学 | ASG 不该学 |
|---|---|---|---|
| packet 结构 | 单入口 `SearchPacket`，schema_version/query/filters/generated_at/results/pagination/truncation 全结构化（`packet.rs:11-22`）；结果含 why_matched/citations/visibility（`packet.rs:24-74`） | 学：handoff-pack 用固定 schema+版本号；每条证据带 why_matched 与 citations | 不学：字段承诺>实现（cursor、visibility 常量） |
| citations | event/session 双引用，带 event_seq、provider 外部 id、raw path、exists、cursor（`dtos.rs:742-760`；`results.rs:251-276`） | 学：引用对象字段清单；源存在性是一等字段 | 不学：无 span/hash；snippet 从 2048 字符索引裁剪反读（`projections.rs:1665`） |
| pagination | `offset:n` 只写不读（`packet.rs:114-123`）；无 CLI/MCP 消费者（`main.rs:317-434`）；SDK 形状三种（fixture:85-87、JVM:18-55） | 学：truncation reason 的机器可读性（`search.rs:51-59`）；把“还有更多”显式化 | 不学：死 cursor；`omitted_results` 语义漂移（1 vs 计数） |
| visibility | 索引期 + 语义 SQL 双层过滤（`projections.rs:1600-1622,1005-1020`）；packet 字段恒 local_only（`results.rs:195,304`）；DDL 拒绝 withheld（`ddl.rs:371`） | 学：过滤给在检索层而不是渲染层；语义谓词防御式重查 | 不学：模型/DDL/结果字段三层不一致；把 visibility 做成常量 |
| capture 增量 | indexed/last_imported 双水位 + pending SQL（`catalog.rs:161-256,887-913`）；cursor 失败不推进（`batches.rs:180-188,235-245`） | 学：append-only 水位保留、失败不清 last-good、pending 含“sessions 行缺失” | 不学（如追求强证据）：FNV-1a 64 身份/哈希（`common/identity.rs:11-24`） |
| 保真/省略 | 逐事件类型保留策略 + 显式 retention 元数据（`native.rs:437-452,602-610`）；成功输出不保留（`native.rs:505-509`） | 学：省略必须落元数据（mode/limit/truncated/omission/reason） | 不学：把“成功输出不保留”当默认，除非先定义手递包需要哪些证据 |
| 失败/半写 | store 错误回滚当前批+中止、解析错误计数续跑（`batches.rs:199-209`）；整索引刷新无事务（`projections.rs:580-660`） | 学：错误分类 + 游标不推进 + 重试幂等 | 不学：DELETE+重建型 refresh 的裸奔路径；半重建不可自愈（`projections.rs:757-799`） |

**给 ASG handoff-pack 的落地结论（建议，不是已批准实现）**：
1. 证据锚定采用 ctx citation 的字段骨架（id/类型/时间/provider/外部 id/源路径/源存在性/游标），但补上 span（行或字符区间）与源快照指纹（大小+强哈希），并在渲染时从权威 payload 取原文。
2. 分页必须是可消费协议：cursor 定义输入语义、稳定排序键与版本，验收“翻页无重无漏”；若首版不做翻页，就删掉 cursor 字段，只保留 `truncation{truncated,reason,omitted}`。
3. `visibility` 要么做真字段（来自行数据、驱动过滤与输出一致），要么从 packet 删除；不要保留常量字段。
4. 增量沿用“indexed 水位 + last_imported 水位 + pending 判定”的分层，但为失败导入存“已提交到哪”的回执，避免只能靠重试收敛。
5. 所有有损归一化（截断、省略 diff、丢弃成功输出）写入 handoff-pack 的 retention/omission 元数据，让消费者能判断证据完整性。

## 10. 残余未决（未读/未证清单）

1. `migrations.rs` 的 v47 模块 `v47_provider_session_repair`（及 `v47_provider_session_repair_tests`）未读；v44 的 `rebuild_v44_current_schema_tables` 具体重建语义未读。
2. `crates/ctx-history-search/src/ranking.rs` 未读：慢路径候选/`scan_budget` 的具体预算常量与过滤顺序（`search.rs:41-54` 只显示标志位）。
3. `provider_sources/probes.rs`（706 行）与 `specs.rs` 未读：有界探测的预算数值、哪些 provider 真正 importable、source_format 全表。
4. `ctx-cli/src/commands/search.rs`、`mcp.rs` 只做定点 grep（未计阅读）：MCP search 无 offset/cursor 的结论来自 grep（mcp.rs:719 与全文件无 offset/cursor/page 命中），未逐段读工具实现。
5. `docs/contracts/json.md` 只读 330-440：pagination 语义、truncation 文档定义若在此范围外，本报告未引用。
6. CLI 侧 freshness（`SearchRefreshReport::to_json` 实现）未读，只有契约文档；因此“刷新失败如何在 JSON 中表达”只按文档记录。
7. 未执行 build/test/运行：所有保留策略测试、迁移测试、分页环回、withheld 写入、半重建恢复均为静态阅读结论；`withheld` 不可写结论基于当前 DDL，未在旧库上验证。
8. T2 `sweep-ctx.json` 只做了目标文件过滤查询；其余 600+ 文件的机械命中未消费。
9. 阅读事故留痕：一次两段批量读被工具输出中段截断（projections.rs 约 255 行、catalog.rs 215-227 行），已单独补读并以“实际显示区间”记账；见 coverage JSON 的 reason/read_method 字段。
10. `contracts/agent-history-v1` 的 fixture 与当前 CLI 输出形状不一致（CTX-01）未用运行验证是“陈旧 fixture”还是“另有序列化层”；本报告按源码事实并列呈现，不裁决意图。
