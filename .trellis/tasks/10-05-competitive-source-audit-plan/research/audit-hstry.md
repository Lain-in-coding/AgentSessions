# Research: hstry 竞品源码审计（T1 深读 + T2 扫描对照）

- Query: 对 hstry 指定快照做静态源码审计，覆盖「事件/parts 数据模型与 DB schema、跨 provider 归一化边界（TS adapters ↔ Rust core）、会话发现/导入/导出/搜索/peek/恢复链路、新鲜度/增量/并发/迁移」，并给出 ASG 对照。
- Scope: internal；只读 `C:\AgentSessions\Github_src\hstry`；未运行 build/test/install/网络/git 写操作。
- Snapshot: commit `88b78b1f3a84723e31a2c444bec326829fd02459`（只读 `git rev-parse` 与任务给定一致；`git status` 仅显示未跟踪的 `.codegraph/` 索引目录，无源码改动）。
- Date: 2026-10-06
- Status: **T1 精读完成，结论均为静态推导**；覆盖凭证见同目录 `coverage-hstry.json`（T1 8 文件 + 补充读取 26 文件，全部带 SHA256/精确行区间；不把哈希当阅读）。
- T1 实际阅读量：8 文件中 7 个全文、`crates/hstry-cli/src/main.rs` 选择性部分（5130/6946 行，missing_ranges 已列账）；T1 合计 11991/13807 行（86.8%）。补充读取 3325 行（含 13 个 migration SQL 全文）。

## 0. 一页结论

hstry 是「统一会话库 + 多 adapter 采集 + SQLite FTS5 检索 + 恢复/导出 + 服务化同步」的平台型竞品：数据层（migrations、版本计数、outbox、watermark、snapshot/summary cache、reseed/verify）明显比 sessiongrep 厚，搜索的资格过滤在 SQL 内先于 LIMIT（直接优于 sessiongrep SG-01）。但它把 adapter 生态做成了宽而浅的「单协议 + 16 个目录」：provider 覆盖面（含网页导出与 GUI 应用）强于 ASG 当前矩阵，深度契约（source span、tool activity、maturity、逐 provider fixture/版本矩阵）弱于 ASG 的方向；且存在一类会改变权威正文的投影截断（H-01）与若干契约性失败模式（H-02/H-03/H-04/H-05）。

## 1. 数据模型、DB schema 与 TS ↔ Rust 边界

### 1.1 Rust 域模型

- `Conversation`（`crates/hstry-core/src/models.rs:21-59`）：UUID 主键 + `source_id` + `external_id`（agent 原生 session id）/`readable_id`（人读 id）/`platform_id`（编排平台 id，migration 010）；`harness`、`version`、`message_count`、`parent_conversation_id`/`parent_message_idx`/`fork_type`（会话树，migration 011/012）。
- `Message`（`models.rs:63-89`）：`(conversation_id, idx)` 位置语义、`role`（user/assistant/system/tool/other）、`content` 文本、`parts_json` 结构化 parts、`sender`（多人/多 agent 归属）、per-message `provider`/`harness`/`client_id`。
- `Part`（`crates/hstry-core/src/parts.rs:162-262`）：`text`/`thinking`/`tool_call`/`tool_result`/`file_ref`/`image`/`audio`/`video`/`attachment`；媒体三态 `MediaSource::{Url, AttachmentRef, Base64}`（`parts.rs:73-96`）；`Sender` 线格式（`parts.rs:390-409`）。这是跨 provider 归一化的目标形状，但没有消息级字节/来源定位（source span）字段。
- wire 类型 `ParsedConversation/ParsedMessage/ParsedToolCall`（`crates/hstry-core/src/parsed.rs:10-69`）与 TS `Conversation/Message/CanonPart`（`adapters/types/index.ts:24-84`）逐字段对齐（camelCase）。

### 1.2 DB schema（migrations 为真源）

- `001_initial_schema.sql`：`sources`(:4-10)、`conversations`(:13-28，`UNIQUE(source_id, external_id)`)、`messages`(:31-44，`UNIQUE(conversation_id, idx)` + `ON DELETE CASCADE`)、`tool_calls`(:47-55)、`attachments`(:58-68)、tags(:71-80)、embedding 表(:83-95)、两张 FTS5 外部内容表(:98-112)、`search_state`(:115-118)、FTS 触发器(:121-147)、索引(:150-160)。
- 004 message_events + conversation_snapshots；005 summary cache（首条 user 消息 + count）；006 sender_json/provider；007 harness；008 client_id；009/013 索引；010 platform_id；011 version+message_count（回填）；012 会话树；013 `indexer_outbox`(:18-32) 与事件保留索引。
- 运行时行为：`Database::open` WAL + `busy_timeout(30s)` + FK on + 5 连接池（`crates/hstry-core/src/db.rs:63-90`）；`init` 先执行 SCHEMA（仅 schema_migrations 表，`schema.rs:5-12`）再跑 migrations，再做 ensure/backfill（`db.rs:110-118,309-397`）。

### 1.3 事件与 parts

- `message_events` 是可选 append-only 镜像（默认 **off**，`config.rs:99-108`；开启后 `db.rs:1875-1898` 写整条 Message JSON），保留策略年龄+每会话条数（`db.rs:2619-2664`）。
- `conversation_snapshots` 是读缓存：`insert_message` 使快照失效（`db.rs:1559-1560,1900-1906`），`get_messages_cached` 只在 message_count 匹配时用快照（`db.rs:1779-1794`）。
- `conversation_summary_cache` 由单条插入路径增量维护（`db.rs:1959-1983`）或批量路径显式 `rebuild_conversation_summaries`（`db.rs:1704-1763`）。
- `tool_calls`/embedding 表在已读导入链路中**未见写入**：工具活动只存在于 `parts_json`；`cmd_show` 返回空 tool_calls（`crates/hstry-cli/src/main.rs:2562-2571`），export 亦留 `TODO: load from tool_calls table`（`main.rs:4417`）。schema 与实现存在表级落差（H-15，信息级）。

### 1.4 TS adapters ↔ Rust core 的边界

- 每个 adapter 是独立 TS 入口（`adapters/*/adapter.ts`，`runAdapter` 于 `adapters/types/index.ts:314-380`）；Rust 侧 `AdapterRunner` 每个请求 spawn 一次 JS 运行时进程，请求走环境变量 `HSTRY_REQUEST`，>100KB 改走 stdin（`crates/hstry-runtime/src/runner.rs:287-341`），响应要求 stdout 恰好是一段 JSON。
- 协议方法只有 `info/detect/parse/parseStream/export`（`runner.rs:113-129`）；`parseStream` 不支持时由 TS 返回 `{error: "...parseStream..."}`（正常退出），Rust 映射为 `Ok(None)` 并回退整文件 parse（`runner.rs:393-420`；`crates/hstry-cli/src/sync.rs:75-95`）。
- 归一化在 TS 内完成（role 映射、parts 组装、时间戳 ms、workspace 推导），Rust core 只做「落库 + 幂等 + 树关系」：`ingest_batch`（`crates/hstry-core/src/ingest.rs:34-266`）批内先按 `external_id` 复用会话 id、生成 UUIDv5 stable message id（`lib.rs:45-81`）、单事务 upsert + 多行 INSERT（`db.rs:2990-3062`），最后第二遍解析 `parent_external_id → parent_conversation_id`（`ingest.rs:230-263`）。
- 边界失败模式：adapter 抛错时 TS 把错误 JSON 打到 **stdout** 后 `exit(1)`（`types/index.ts:375-378`），而 Rust 在退出码非 0 时只读 **stderr**（`runner.rs:329-332`）——错误消息可能以空串形式丢失（H-03）；adapter 任何多余 stdout 输出都会破坏 `serde_json::from_str`（`runner.rs:337-338`）。

## 2. 各链路行为与失败模式

### 2.1 会话发现（scan/quickstart/service watcher）

- `scan_hits` 只遍历各 adapter `info().default_paths`，路径存在且 `detect()>0.5` 才计入（`main.rs:4120-4148`）；`quickstart` 对命中逐个过 source-registry 校验，失败则跳过并不写配置（`main.rs:3675-3802`）；`source add` 无显式 adapter 时取 detect 最高置信（无阈值，`main.rs:2622-2640`）。
- 失败模式：detect 抛错被 `if let Ok(...)` 静默跳过（`main.rs:1685-1690`、`4126-4145`）；不在默认路径、目录名不含 `claude/codex/gemini/opencode/aider/cursor/chatgpt/assistant` 的（service 工作区启发式，`service.rs:1973-1984`）不会被自动发现，只能手工 add。
- 服务 watcher：notify 递归监听默认路径+已注册 source+config 文件（`service.rs:1872-1908`），事件 2s 冷却、只同步路径相关的 source（`service.rs:1188-1213,1819-1855`）。

### 2.2 导入 / 同步 / reseed

- 一次性 `import`：探测/指定 adapter → `runner.parse`（`main.rs:1762-1774`）→（非 dry-run）source 注册 → **逐会话** upsert、**逐消息** insert，全部 autocommit，结束才写 `last_sync_at`（`main.rs:1851-1946`）。`insert_message` 的返回值（是否真正写入）被忽略，统计按提交条数计（H-09）。
- 增量 `sync`：`sync.rs` 用 `parse_stream`（batch_size=200）循环，把 `since=source.last_sync_at(ms)`、`cursor=source.config.cursor` 发给 adapter；每批 `ingest_batch` 单事务写入；结束后统一 rebuild summary，并把新 cursor 与 `last_sync_at` 写回 source（`sync.rs:58-157`；`ingest.rs:211-228`）。`parseStream` 不支持则回退整文件 parse（`sync.rs:75-95`）。
- 并发：CLI 按可用并行度（上限 4）并发源（`main.rs:1341-1345,1413-1450`）；DB 侧用 `ingest_writer` 互斥把 SQLite 写事务串行化（`db.rs:49-53,94-95`；`ingest.rs:214-227`）；`ingest.rs:309-356` 有 8 源并发批写入测试（本轮未运行）。
- reseed：purge（保留或删除 source 行）→清 cursor/fingerprint/watermark→（可选 bulk 模式）→复用 `sync_source_with_progress` 重导→可选 dedup→可选 FTS rebuild（`main.rs:5685-5883`）；bulk 模式 `PRAGMA synchronous=OFF` 并临时删重索引（`db.rs:2944-2962`）。失败模式见 H-08/H-09。
- 失败模式（增量细节）：`since` 语义由 adapter 各自实现——codex/claude-code 都检查 createdAt 与最后消息时间，二者都早于 since 才跳过（`adapters/codex/adapter.ts:399-405`；`adapters/claude-code/adapter.ts:138-144`）；cursor 只在 sync 正常结束后持久化，超时/中断后批次已提交但进度回退，靠 `(conversation_id, idx)` + stable id 幂等重放（服务端超时见 2.5）。

### 2.3 导出

- `export` 支持 `markdown/json` 通用格式 + 各 adapter 自有的 codex/claude-code/pi/opencode 等格式（`main.rs:4324-4353`）；`all` 或按 id 列表加载会话（`main.rs:4358-4377`），会话不存在直接报错中止。
- 多文件模式只在非 json 输出且 format 为 markdown/json 时启用 `--session-files`（`main.rs:4441-4486`）；写出路径直接 `fs::write`（`main.rs:4462,4508,4524`），同名会覆盖。
- 保真限制：导出把 DB 的 `content` 直接放入 `ParsedMessage.content`（`main.rs:4409-4416`），而 `content` 可能已被投影截断（H-01）；`tool_calls` 字段为 None（`main.rs:4417`），工具信息只在 `parts` 中（codex/claude export 的文本形态只写 content，`adapters/codex/adapter.ts:501-519`；`adapters/claude-code/adapter.ts:422-436`）。

### 2.4 搜索

- DB 层：两张 FTS5 外部内容表（porter / unicode61+`_./:`），`bm25()` 排序、`snippet()` 截断 12 token；资格过滤（source/workspace/时间/role/model/harness/tag）全部拼进同一 SQL 且**先于 LIMIT**（`db.rs:2101-2236`；对照 FTS 定义 `migrations/001:98-112`）。模式启发式：含 `/ \ :: -> _ .` 或 camelCase 走 code 表（`db.rs:3160-3175`）；查询 token 加引号、保留尾 `*`（`db.rs:3177-3206`，4 个单测在 `db.rs:3371-3400`）。
- FTS 维护：触发器同步；`ensure_fts_table` 依据 `sqlite_master` 定义 canary 判断重建，空表但有消息时 rebuild；完整性检查默认**排除**，仅 `HSTRY_FTS_INTEGRITY_CHECK=1|true|yes` 时按小时检查（`db.rs:2256-2462`，H-12）。
- CLI 层：本地（gRPC service→HTTP API→直接 DB 三种路径，`main.rs:2019-2039`）或 remote 结果合并后按 score 排序（`main.rs:2042-2062`），再做系统上下文/角色/no_tools/dedup/compact 后过滤再截断（`main.rs:2064-2141`，H-05）；`config.service.enabled && search_api` 但服务不可达时**硬报错**不回退本地（`main.rs:2020-2031`）。
- 失败模式：`fetch_limit=limit*4` 为 i64 未检查乘法（`main.rs:2003`）；跨本地/远端 score 直接混排可比性未声明；远端返回经 proto 空串→None 转换（`crates/hstry-core/src/service.rs:271-317`）。

### 2.5 peek / list / show

- `list` 默认跨 source 去重（identity key：harness+external/readable/platform/id，`main.rs:2284-2296,2360-2433`），先多取 4×（上限 2000）再截断（`main.rs:2353-2358`）。
- `peek`（`main.rs:2435-2507`；`peek.rs:66-181`）输出固定预算包：first/last user、last assistant、工具计数、bash 样本、files touched、时长；`list --peek` 对每个会话 `db.get_messages`（非 cached），N 次全量读（`main.rs:2474-2478`）；`show` 只打印 `content`（`main.rs:2589-2593`），JSON 模式才带 `parts_json`（`main.rs:2561-2571`）。
- 失败模式：peek 的 `files_touched` 来自 bash 命令文本扫描（`peek.rs:227-301`），是启发式；`duration_min` 依赖 `conv.updated_at`，缺失时为 0（`peek.rs:161-164`）。

### 2.6 恢复（resume）

- 解析目标：完整 UUID→DB；否则**全表加载**后前缀/external/readable 匹配，多义报错（`main.rs:5209-5251`，H-13）。`--search` 先标题/首条消息模糊匹配，再回退 FTS；无 id/search 时交互列表选择（`main.rs:4834-5040`）；`--pick` 走 fzf（`main.rs:4651-4775`）。
- 同 agent 快速路径：source adapter == 目标格式且 `metadata.file` 存在 → 直接构造命令启动（`main.rs:5051-5098`）；否则经目标 adapter export→写入目标 `session_dir`→启动（`main.rs:5100-5205`），dry-run 输出计划（`main.rs:5067-5088,5165-5192`）。
- 失败模式：`launch_agent` 在 `--json` 分支直接返回 `{"launched": true}` 但**不 spawn**（`main.rs:5400-5419` 先于 `5424-5431` 的 actual spawn），机器消费者会被误导（H-02）；命令按空白切分且占位符不加引号，路径含空格会拆参（`main.rs:5315-5331,5401-5407`，H-14）；`place_exported_session` 直接覆盖同名文件（`main.rs:5355-5394`）。

## 3. 新鲜度 / 增量 / 并发 / 迁移

- 新鲜度分层：适配器 `since`（基于会话时间）→ source `last_sync_at` → source.config `cursor`（流式适配器）→ 服务端 `file_fingerprint`（仅**文件**型 source，`service.rs:1575-1592`；目录 source 返回 None 不参与跳过）→ 自适应调度（min 30s / max 1800s / idle_backoff 1.5，`config.rs:683-691`；调度判断 `service.rs:1646-1651,1709-1728`）→ 失败指数退避 30s*2^n 上限 300s（`service.rs:1785-1796`）→ 事件静默 30s（`service.rs:1749-1756`）。
- 并发：CLI 源级并发（默认 min(CPU,4)，`main.rs:1341-1345`）；服务端 semaphore `max_concurrent_syncs`（默认 4）+ 每源时间预算（默认 60s，超时 drop future）（`config.rs:697-714`；`service.rs:1679-1705`）；DB 单写者互斥 + WAL 读并发（`db.rs:49-53`）。
- 超时/中断语义：预算超时只回滚当前未提交批（batch tx），已提交批次保留；cursor/last_sync_at 未推进，下次全量重放（幂等）。这是「至少一次重放 + 唯一键收敛」设计（`sync.rs:97-157`；`ingest.rs:211-228`）。
- 迁移：migrations 目录优先级 `HSTRY_MIGRATIONS_DIR` → `CARGO_MANIFEST_DIR/migrations` → `dirs::data_dir()/hstry/migrations` → `./migrations`，目录不存在才用**内嵌**集合（`db.rs:121-150`）；按文件名前缀取版本、`schema_migrations` 跳过已应用、每个迁移单事务（`db.rs:153-208`）；内嵌列表 001-013 手工维护（`db.rs:210-307`）。风险：只要磁盘目录存在，内嵌的新迁移不会补跑（H-06）。
- 运行时 schema 兜底：readable_id/provider/parts_json 列的 ALTER+回填（`db.rs:309-397`）；FTS 表定义 canary+重建（`db.rs:2292-2495`）；版本/计数各路径递增（`db.rs:1535-1550,1751-1759`）。

## 4. 发现清单（严重度 + 证据 + 反证）

严重度口径：P1=核心正确性/数据保真；P2=有条件的正确性或契约性风险；P3=较低优先级/证据缺口；I=已知取舍或信息。全部为静态推导，未运行复现。

### H-01 · P1 · 长消息 `content` 被投影截断，污染 FTS/show/markdown 导出

- 证据：`project_content` 在每个 text/thinking part 上先截到 **500 字符**（`db.rs:3346-3354`），累计 >4000 字符即停止（`db.rs:3322-3327`）；仅当 `content.trim().is_empty()` 或 `char_count>2000 || line_count>80` 时触发投影（`db.rs:3336-3344`）。所有写入路径（`insert_message` `db.rs:1468-1470`、tx 版 `1584-1585`、bulk 版 `3033-3034`）都存投影后的 content，而 FTS5 外部内容表索引的正是 `messages.content`（`migrations/001:98-104`）；`show` 只打印 content（`main.rs:2589-2593`），markdown/codex/claude 导出也用 content（`main.rs:4410-4416`；`adapters/codex/adapter.ts:501-519`；`adapters/claude-code/adapter.ts:422-436`）。
- 可达：任何 provider 的 >2000 字消息（adapter 普遍用 `textOnlyParts/`textPart 携带全文）都会在库内以 ≤4000 字（单段 ≤500 字）形式存在；搜索只能命中投影后的前缀文本。
- 反证/界限：`parts_json` 原样落库（`db.rs:1505,1597`；读回 `3253-3256`），`show --json`/gRPC 消费者仍能取回全文；≤2000 字消息不受影响；未运行验证实际截断输出。
- ASG 教训：权威正文不得被展示/索引层重写；检索应索引全文，摘要/预算只作用于展示层。

### H-02 · P2 · `resume --json` 报告 `launched: true` 但实际不启动

- 证据：`launch_agent` 先处理 `json_output` 分支，直接 emit `{"launched": true, ...}` 并 return（`main.rs:5400-5419`）；真正 `ProcessCommand::spawn/status` 仅在不走该分支时执行（`main.rs:5421-5437`）。调用点无条件把它的结果当成功（`main.rs:5095-5097,5203-5205`）。
- 可达：`hstry resume <id> --json`（含同 agent 快速路径与转换路径）会完成 export/落盘，但不会启动 agent，同时机器输出声称已启动。
- 反证/界限：`--dry-run --json` 的计划字段是诚实的（`action/dry-run`，`main.rs:5067-5088,5165-5192`）；非 JSON 路径正常启动；未运行复现。
- ASG 教训：副作用声明必须与真实执行一致；plan/execute 分离，execute 失败不得返回 launched=true。

### H-03 · P2 · adapter 抛错的错误消息在 Rust 侧丢失（stdout/exit-code 双通道不一致）

- 证据：TS `runAdapter` catch 分支把 `{error}` 打到 stdout 后 `process.exit(1)`（`types/index.ts:375-378`）；Rust `call` 在退出码非 0 时只取 stderr 拼错误（`runner.rs:329-332`），成功路径才解析 stdout JSON（`runner.rs:334-340`）。若运行时 stderr 为空，用户只看到 `Adapter failed: `。
- 可达：任一 adapter parse/export 抛异常（坏 JSON、schema 变动、权限错误）。
- 反证/界限：`parseStream` 不支持是「正常返回 {error} + exit 0」，能被正确识别并回退（`types/index.ts:350-356`；`runner.rs:409-416`）；detect 错误被调用方静默跳过属有意降噪；未运行观察真实 stderr 内容。
- ASG 教训：错误单一通道 + 结构化错误码；子进程协议要定义「非零退出时读什么」。

### H-04 · P2 · `external_id` 为空时每次导入/同步都会新建会话（NULL 不参与唯一冲突）

- 证据：`conversations` 的唯一约束是 `UNIQUE(source_id, external_id)`（`migrations/001:27`），SQLite 对 NULL 不去重；upsert 冲突目标同列（`db.rs:577-598`）。`ingest_batch` 只有在 `external_id` 为 Some 时才查重/复用（`ingest.rs:63-93`），否则先给随机 UUID 再 upsert；`cmd_import` 同样仅在存在 external_id 时复用（`main.rs:1859-1865`）。合约上 `externalId` 可选（`adapters/types/index.ts:27`）。
- 可达：自定义/未来 adapter（或旧导出）不提供 session id 时，重复 sync 产生同内容多会话；list 的跨源去重（`main.rs:2284-2296`）按会话键合并，但 DB 已膨胀。
- 反证/界限：内置 codex/claude-code 都用文件名/sessionId 兜底（`adapters/codex/adapter.ts:414`；`adapters/claude-code/adapter.ts:155-158`）；`conversation_exists_for_session` 会用 readable_id 兜住历史 Octo 场景并跳过（`db.rs:912-929`；`ingest.rs:84-89`）；未运行构造无 id fixture。
- ASG 教训：身份缺失必须 fail-closed 或显式报「不可去重」，不能拿 NULL 唯一键当幂等。

### H-05 · P2 · CLI 层后过滤发生在 limit×4 截断之后，结果可欠填充

- 证据：`fetch_limit = limit * 4`（未检查 i64 溢出，`main.rs:2003`）；系统上下文、多角色、no_tools、dedup 均在取回后做 retain（`main.rs:2064-2097`），最后才截到原始 limit（`main.rs:2139-2141`）。若前 4N 命中都被过滤，用户拿到的条数少于请求值，即使库内仍有匹配。
- 可达：`--no-tools`、多 `--role`、`--dedup`、大量 AGENTS.md 类系统消息时；与 sessiongrep SG-01 同形但加成系数 4×、且只影响这几类后过滤。
- 反证/界限：单角色过滤已下推 SQL（`main.rs:1994-1999`），source/workspace/时间/model/harness/tag 均在 SQL 内先于 LIMIT（`db.rs:2135-2158`）——这正是 hstry 优于 sessiongrep 的地方；4× 超取降低了触发概率；未运行构造命中分布。
- ASG 教训：资格过滤必须在 cap 之前完成；后过滤要么分页补足，要么返回「还有更多」标记。

### H-06 · P2 · 存在 migrations 目录时内嵌迁移集合被整体遮蔽

- 证据：迁移目录选择逻辑只要任一目录存在即使用该目录（`db.rs:128-150`），仅目录**不存在**时才 `run_embedded_migrations`（`:136,143,150`）；随后只遍历该目录文件（`:153-205`）。若目录缺少某个较新版本文件，该版本永远不会被应用，也没有「内嵌集合 vs 已应用集合」对账。
- 可达：开发环境 `CARGO_MANIFEST_DIR`/`HSTRY_MIGRATIONS_DIR`、或用户数据目录里留有旧的一组 SQL 时升级二进制。
- 反证/界限：生产默认路径通常不存在该目录，走内嵌 001-013；迁移逐个事务并写入 schema_migrations（`db.rs:190-204`），部分迁移失败不会半应用单文件；未运行旧目录升级场景。
- ASG 教训：迁移以二进制内嵌版本为权威，磁盘目录仅可作为显式覆盖；启动时对已应用集合做单调性校验。

### H-07 · P2 · Web sync 只实现了 ChatGPT；Claude/Gemini 可登录但 sync 抛未实现

- 证据：`web-runner.ts` 的 `sync()` 仅 `chatgpt` 分支，其余 provider 直接 `throw new Error("... not implemented yet")`（`crates/hstry-cli/assets/web-runner.ts:73-91`）；CLI 仍接受 `--provider claude/gemini` 并把错误抛给用户（`main.rs:3388-3436`）。README 亦声明 Claude/Gemini planned（`README.md:86-87`）。
- 可达：`hstry web sync --provider claude|gemini`。
- 反证/界限：login 已覆盖三家（`web-runner.ts:48-71`）；手动导出的 claude-web/gemini adapter 照常可用；这是「计划中」的诚实声明而非隐藏缺陷，但对外功能面容易出现「web 支持三家」的误读。
- ASG 教训：广度声明要按 provider×能力（login/sync/parse）标注成熟度，不能按「目录存在」计数。

### H-08 · P3 · bulk reseed 中途崩溃会留下被删的重索引（数据可恢复，索引不自动恢复）

- 证据：`begin_bulk_reseed` 关闭 synchronous 并 DROP 4 个重索引（`db.rs:2944-2962`），只有 `end_bulk_reseed` 重建（`db.rs:2967-2980`）；`Database::open` 的 ensure 逻辑不含这些索引。文档承认中途崩溃由操作员重跑 reseed 恢复（`db.rs:2940-2943`）。
- 可达：reseed 进程被 kill/断电。
- 反证/界限：数据真源在磁盘，重跑 reseed 会走到 `end_bulk_reseed` 完整恢复；SQLite 仍保证单事务原子性；未运行崩溃注入。
- ASG 教训：破坏性 PRAGMA 与索引生命周期应有 guard/恢复清单；崩溃后下次启动应能自动对账重建。

### H-09 · P3 · `import` 逐条 autocommit、计数不区分「提交」与「实际写入」

- 证据：`cmd_import` 对每个会话 upsert、每条消息 `insert_message`（返回 bool 被丢弃）并累加计数（`main.rs:1856-1941`），`last_sync_at` 在全部结束后才写（`:1944-1946`）；中途失败留下部分导入且无回执。对照 `ingest_batch` 的单事务批量路径（`ingest.rs:211-228`）与 `IngestOutcome` 注释「submitted, duplicates dedupe at DB layer」（`ingest.rs:24`）。
- 反证/界限：主键/唯一键 + stable id 使重跑可收敛；import 是一次性显式命令；reseed 明确复用流式 sync 而非 cmd_import（`main.rs:5781-5784`）。
- ASG 教训：批量入口默认事务化 + 回报 inserted/updated/skipped 三态。

### H-10 · P3 · `verify` 只比对会话/消息计数，不能发现内容漂移

- 证据：`cmd_verify` 解析磁盘后只比较 `on_disk_convs/msgs` 与 DB 计数（`main.rs:5959-5965`），drift 判定为纯计数不等；`--repair` 直接 reseed（`:5968-5974`）。
- 反证/界限：docstring 明示 count-based（`main.rs:510-515`）；对「重复回放/丢行」类漂移有效；未运行内容篡改场景。
- ASG 教训：drift 检查至少加入每条消息的稳定 id/内容哈希抽样。

### H-11 · P3 · `dedup` 以整会话内容哈希分组，可能删除同内容的不同会话

- 证据：分组键 = source_id + 全部 (role, content) 序列（`main.rs:5580-5596`），同组保留 updated_at 最新、其余批删（`:5602-5633`）；不比较 external_id/时间戳。
- 反证/界限：需要整会话逐条内容与顺序完全一致才误伤；命令显式、支持 `--dry-run`；未运行构造。
- ASG 教训：去重键应优先身份（external_id/平台 id），内容哈希只作辅助证据。

### H-12 · P3 · FTS 完整性检查默认关闭（按需环境变量开启）

- 证据：`ensure_fts_schema_optimized` 仅在 `HSTRY_FTS_INTEGRITY_CHECK∈{1,true,yes}` 时做按小时 `integrity-check`（`db.rs:2256-2290,2401-2418`）；默认只走 schema canary（定义串不匹配才重建）与空表 rebuild 探针（`:2396-2399,2425-2437`）。
- 反证/界限：触发器保持增量同步；schema 变化与空索引场景仍会重建；损坏但定义串不变的 FTS 才可能长期存在；未运行损坏注入。
- ASG 教训：索引损坏检测可作为可配置但需有 last-check 状态与告警。

### H-13 · P3 · `resolve_conversation_by_id` 全表加载内存前缀匹配

- 证据：非完整 UUID 时 `list_conversations(limit=None)` 加载全部会话后在内存里匹配 id 前缀/external/readable（`main.rs:5218-5249`）。
- 反证/界限：先走 UUID 精确查询（`:5211-5215`）；本地库规模通常有限；多义会显式报错而非错选。
- ASG 教训：前缀解析用索引化查询或限定候选集，避免无界全表加载。

### H-14 · P3 · resume 启动命令按空白切分，路径含空格会拆参

- 证据：`build_resume_command` 直接字符串替换占位符、不加引号（`main.rs:5315-5331`）；`launch_agent` 用 `split_whitespace` 拆成 program/args（`:5401-5407`）。
- 反证/界限：`ProcessCommand::new(program).args(args)` 不经 shell（`:5424-5430`），无注入面；`current_dir(workspace)` 不受切分影响；仅 session_path/session_id 含空格时失败。未运行。
- ASG 教训：恢复命令保存结构化 argv（程序 + 参数数组），字符串模板仅用于展示。

### H-15 · I · 文档/表结构与实现漂移（信息级）

- `db.rs:2791-2792` 注释声称 「ON CONFLICT(id) 阻止 literal-replay」，但三处写入实际都用 `ON CONFLICT(conversation_id, idx)`（`db.rs:1502,1594,3016`）——真正幂等键是位置而非 id；stable id 的 client_id 优先分支在已读导入路径传 None（`ingest.rs:180-187`）。
- `tool_calls` 与 embedding 表（`migrations/001:47-55,83-95`）在已读导入/导出链路未见使用：`cmd_show` 返回空 tool_calls、export 留 TODO（`main.rs:2568,4417`）；`config.embedding_endpoint`（`config.rs:31`）未在已读 DB 代码中消费。
- 反证：这些可能是给 gRPC/API/未来 mmry 集成预留；不做「死代码」断言。

## 5. 与 ASG 对照

对照物：本任务目录 `asg-provider-matrix.json`（ASG `providers` 输出，schema 1.1；14 个 native-parse provider + 2 个 unknown）与先前 sessiongrep 审计引用的 ASG spec（provider-claude/codex、adapters-sqlite error-handling、cli error-handling）。本节不重新审计 ASG 实现。

### 5.1 hstry 强在哪

1. **来源广度（对 ASG 构成压力）**：16 个 adapter 目录（aider、chatgpt、chatgpt-teams、claude-code、claude-cowork、claude-web、codex、cursor、gemini、goose、hermes、jan、lmstudio、opencode、openwebui、pi），含网页导出（ChatGPT/Claude.ai/Gemini）与 GUI 应用（Jan/LM Studio/Open WebUI/Goose），且 README 列了默认路径表（`README.md:325-346`）。ASG 矩阵当前没有这些 web/GUI 来源。
2. **统一平台层**：单一 SQLite 库 + FTS5 双索引（自然语言/代码 tokenizer）+ bm25/snippet + 跨源身份去重 + tags + 会话树/父子边 + snapshot/summary 缓存 + remote sync + outbox/watermark。ASG 的 lexical `bigram-hash-v1` 与逐 provider native source_span 是另一种路线；hstry 的「一库多源」工程完成度值得参考。
3. **运维闭环**：migrations+schema_migrations、version/message_count、purge_source、dedup、reseed、verify/repair、FTS rebuild、事件保留压缩都有具体实现与锚点（第 3 节）。
4. **搜索资格过滤顺序正确**：SQL 内过滤先于 LIMIT（`db.rs:2135-2167`），是 sessiongrep SG-01 的正面反例。
5. **peek 预算与恢复路径**：`peek` 的 token 预算包（`peek.rs:16-36,66-181`）与 resume 的「同 agent 直连 / 转换落入原生目录」双层路径（`main.rs:5042-5205`）是 ASG 可以直接吸收的 UX 形态。

### 5.2 hstry 弱在哪（对 ASG 的参照价值）

1. **深度契约缺失**：ASG 矩阵按 provider 声明 `discover/incremental/handoff/source_span/tool_activity/usage/maturity/maturity_target/variant_id`；hstry 只有通用 Adapter 接口（`types/index.ts:224-245`），`Message` 无 source span/字节定位（`models.rs:63-89`），tool activity 只隐含在 parts，无 per-provider 成熟度/版本/变体登记。
2. **保真纪律弱**：H-01 的正文投影截断与「统一库=完整保真」的期待冲突；ASG 的 provider spec 强调稳定消息身份、父边与只读快照（先前审计引用），方向相反。
3. **错误与副作用契约弱**：H-02/H-03/H-09；ASG 已有 CLI/adapters 错误处理 spec（stdout/stderr、错误分类、fail-closed），这类契约正是 hstry 缺的。
4. **广度≠成熟度**：T2 扫描显示 16 个 adapter 中仅 9 个有 inline test 命中；testdata 目录 15/16（`claude-cowork` 无）；多数 fixture 是最小样本，无版本矩阵；web sync 仅 ChatGPT 实现（H-07）。
5. **身份与去重边界**：H-04/H-11 显示「身份缺失/内容相同」两类边界没有 fail-closed。

### 5.3 ASG 该学 / 不该学

**该学：**
- 搜索的「先过滤后排序/截断」SQL 形态与双 FTS tokenizer（`db.rs:2101-2236`；`migrations/001:98-112`）。
- 写入幂等三件套：stable message id（UUIDv5+UTF-8 前缀，`lib.rs:45-81`）、`(conversation_id, idx)` 冲突收敛（注意真实键不是 id，H-15）、batch 单事务 + 多行 INSERT + 单写者互斥（`ingest.rs:211-228`；`db.rs:2990-3062`）。
- 增量状态：`since` + `cursor` + `last_sync_at` + `file_fingerprint` + 自适应调度/退避（`sync.rs:58-157`；`service.rs:1620-1800`），并配 `verify/reseed/repair` 运维闭环（`main.rs:5685-6007`）。
- peek 的预算化会话摘要（bookends + counts + files + bash sample）与 resume 结构化计划/dry-run（去掉 H-02/H-14 缺陷后）。
- 正面反例：FTS 过滤器在 cap 前、DB 单写者串行化、`insert_message` 内容相同跳过写（`db.rs:1485-1496`）。

**不该学：**
- H-01 正文投影截断（权威内容不得被展示层重写）；H-02 未执行却汇报 launched；H-03 stdout/exit-code 双通道；H-04 NULL 身份重复；H-05 后过滤欠填充；H-06 磁盘迁移目录遮蔽内嵌集合；H-07 半实现能力对外按目录计数；H-09 逐条 autocommit 且计数不分三态。

### 5.4 广度 vs 深度结论

hstry 用「一个 adapter 协议 + 16 个目录」换来源广度，用统一 DB/服务层换平台能力；ASG 矩阵用逐 provider 能力字段 + maturity 目标换深度可信度。对 ASG 的正确姿势是：**继续按 ASG 矩阵的 maturity/variant 纪律做 provider 深度，不追 adapter 数量；把 hstry 的 DB/同步/检索/运维层设计当参考实现（去掉上述缺陷），把 hstry 的 web/GUI 来源清单当广度路线图输入。**

## 6. 测试与正面反证（T2 扫描回执读数）

- T2 receipt：`sweep-hstry.json`（sha256 5a5dafebb0bf9584a3be47f6b3dc8835ae6a1f838f13d94654f58082ac5c599c；186 文件，105 文件有命中），本报告仅按需过滤查询。
- adapter 内联测试命中（正则计数，非覆盖率）：codex=3、pi=3、aider=2、claude-code=2、chatgpt-teams/goose/hermes/jan/claude-cowork 各 1；chatgpt/claude-web/cursor/gemini/lmstudio/opencode/openwebui=0。testdata 目录 15/16（无 claude-cowork）。
- 本轮**未执行**任何测试；T1 代码内可见测试包括：db.rs FTS 查询转义 4 个（`db.rs:3371-3400`）、parts 10 个（`parts.rs:411-564`）、peek 7 个（`peek.rs:304-511`）、lib.rs 2 个（`lib.rs:83-104`）、ingest 2 个（含 8 源并发，`ingest.rs:268-384`）、readable_id 2 个（`readable_id.rs:63-83`）、core service 无。
- 正面反证：`ingest.rs:309-356` 的并发写测试与 `db.rs` 的单写者设计一致；`db.rs:1485-1496` 内容幂等跳过；`db.rs:2135-2158` 过滤前置；`runner.rs:410-416` 对不支持 parseStream 的显式回退；`main.rs:5067-5088` dry-run 计划诚实。这些限制了部分发现的外推范围。

## 7. 残余未决（本 worker 未读/未证）

- 未运行任何 build/test/复现：H-01（截断实际输出）、H-02（--json resume 行为）、H-03（真实 stderr）、H-04/H-05（构造分布）、H-06（旧目录升级）均为静态推导。
- 未逐行读取：`crates/hstry-cli/src/service.rs` 的 gRPC handlers/发现持久化区段（仅读 987-1286、1575-2007，共 733/2062 行标 partial）、`crates/hstry-core/src/remote.rs`、`source_registry.rs`、`adapter_manifest.rs`、TUI/MCP、runner_tests、其余 14 个 adapter（依赖 T2 扫描回执）。`main.rs` 未读区段以 coverage JSON missing_ranges 为准（1816 行）。
- ASG 对照基于研究目录的 provider matrix 与既有 spec 引用，未重审 ASG 实现；两边的成熟度比较是「声明面 vs 实现锚点」的定性比较。
- 依赖安全/MSRV/许可证/发布流水线未审计（`Cargo.lock`、release workflow 等不在 T1 范围）。

## 8. Related specs / 已读上下文

- 格式基准：`research/coverage-sessiongrep.json`、`research/sessiongrep-full-audit.md`（只读，未修改）。
- T2 回执：`research/sweep-hstry.json`（按需过滤）。
- ASG 对照输入：`research/asg-provider-matrix.json`（providers 输出，schema 1.1）。
- 方法边界来自活动任务 `prd.md`（完整覆盖/严重度/反证/保留未完成范围）；本报告只交付 `coverage-hstry.json` 与 `audit-hstry.md`，不修改其它任务产物。