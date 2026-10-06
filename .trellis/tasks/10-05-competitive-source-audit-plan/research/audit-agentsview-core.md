# 竞品源码审计：agentsview 分片 A（Go 数据层与检索）

- Query: 对 `Github_src/agentsview`（Go 后端 + Svelte 前端，本分片只看 Go `internal/db`）逐段精读，核查 DB schema/迁移/新鲜度、会话与消息身份、FTS 与 vector、检索管线、sync/删除/重扫与并发语义，并给出 ASG 对照。
- Scope: `internal/db`；快照 commit `a84564ad6ea35f8fb80905da4693144f89f84229`。仅静态源码阅读；未 build/test/install/联网/git 写。
- Date: 2026-10-06
- Status: 分片 A 精读完成。`db.go`、`sessions.go`、`search_content.go`、`recall.go` 全文（1..EOF，≤150 行/段）；`messages.go` 按任务选择子系统区域，标 **partial**（未读区间精确列账）。以下结论均为静态推导，不声称运行复现或性能实测。

## 1. 覆盖与凭证

| 文件 | 行数 | 状态 | 读取 | 说明 |
|---|---:|---|---|---|
| `internal/db/db.go` | 3657 | full | 25 段（1–150 … 3601–3657） | 打开/迁移/FTS DDL/WAL/重开/同步标记。 |
| `internal/db/sessions.go` | 3470 | full | 24 段（→3451–3470） | 会话模型/过滤/游标/删除/增量更新。 |
| `internal/db/search_content.go` | 1462 | full | 10 段（→1351–1462） | 内容检索全模式（含 semantic/hybrid）。 |
| `internal/db/recall.go` | 1495 | full | 10 段（→1351–1495） | Recall 条目/证据/候选/排序。 |
| `internal/db/messages.go` | 2460 | partial | 14 段，2054 行 | 消息读写/分页/FTS 关联；缺 1951–2356。 |

- 机器凭证：同目录 `coverage-agentsview-core.json`（每文件 `read_ranges`/`sha256_initial`/`sha256_final`/`content_unchanged`；五次哈希读取前后逐字节一致）。CodeGraph 对文件报 `total_lines+1`（尾虚拟行），不计入逻辑行；实测与任务给定行数一致（3657/3470/1462/1495/2460）。
- 每段输出均为未截断的 codegraph node 区间（`--offset/--limit`，每段 ≤150 行）；首轮一次两段合并在中途被截断，受影响区间随后单独重读后才计入。
- 辅助只读（支撑证据，非 T1 覆盖主张）：`schema.sql`[1–150]、`query_dialect.go`[237–686,780–848]、`sort.go`[195–304]、`messages_diff.go`[111–343]、`vector.go`[1–119]、`search.go`[342–641]、`orphaned.go`[41–340]、`skipped.go`[1–83]、`session_deletions.go`[1–118]、`search_content_units.go`[1–229]；另用 `Select-String` 做函数/DDL 结构索引（仅定位，不算阅读）。
- T2 交叉参考：`sweep-agentsview.json` 对五文件已记 probe 命中（如 `db.go` sql_dynamic=30、concurrency=1；`sessions.go` truncation_limit=9；`messages.go` secret_redaction=23），本报告结论以人工精读为准，probe 未逐条展开。

## 2. DB schema / 迁移 / 新鲜度

**schema 结构。** `schema.sql` 在 `init()` 每次 Open 时以单事务执行（`db.go:2365-2384,3305-3309`）。核心表：`sessions(id TEXT PK, …, data_version, deleted_at, sync_marker)`（`schema.sql:2-77`）、`messages(id INTEGER PK, session_id REFERENCES sessions ON DELETE CASCADE, UNIQUE(session_id, ordinal))`（`schema.sql:80-107`）、`stats` 计数器由 AFTER INSERT/DELETE 触发器维护（`schema.sql:110-133`）。基础索引含 `idx_sessions_ended(ended_at DESC,id)`、`idx_messages_session_ordinal` 等（`schema.sql:136-147`）；151 行之后仅做结构索引（未逐行读）。

**迁移链。** 双层版本：SQLite `user_version`（= parser `dataVersion`，当前 **68**，`db.go:313`）+ `sessions.data_version`（行级，写入消息成功后才盖章，`sessions.go:1689-1724`，upsert 刻意不推进，`sessions.go:1403-1409`）。Open 流程 `probeDatabase → openAndInit → migrateColumns`（`db.go:807-873`）：列迁移用 `pragma_table_info` 探测后 `ALTER TABLE ADD COLUMN`（`db.go:1614-1986,1996-2040`）；legacy 表形修复在 init 前、并把"需重扫"标记写进同一事务（`db.go:2042-2076`）；`migrateColumns` 随后建 partial 索引、回填 `sync_marker`/is_automated/tool_calls 字段、建 usage/identity 表（`db.go:2081-2252`）。版本过新直接拒开且不改档案（`DataVersionTooNewError`，`db.go:334-353,1514-1519`）；只读打开不迁移，schema 缺口报 `SchemaUpgradeRequiredError`（`db.go:1243-1302,1411-1463`）。

**新鲜度语义。** 数据过期时 `dataStale=true` 且**不**盖新 `user_version`（重启后仍要求重扫，`db.go:854-870`；只有 `MarkDataCurrent` 在半程成功后清除，`db.go:3020-3032`）。`sync_marker` 由触发器维护为 `created_at/local_modified_at/ended_at/started_at/file_mtime` 的 ms 精度最大值（`db.go:2254-2314`），回填幂等（`db.go:2316-2334`）；关键防呆：坏的 `created_at` 不以原始串参与 MAX，回退空串，避免增量镜像窗口被永久推高（`db.go:2260-2270`）。文件级快跳：`GetSessionForIncremental`（唯一 file_path + size/mtime/inode/device，`sessions.go:1844-1933`）、`GetSessionVersion`（SSE 变更标记，`sessions.go:1741-1784`）、`ResetAllMtimes`（全量重扫开关，`sessions.go:2326-2339`）。

## 3. 会话与消息身份（去重/孤儿/provider 前缀）

- **会话 ID 是 `agent:raw` 文本**：ID 查询把 `':'||raw` 后缀当同实体的另一种表示（`sessions.go:1638-1687`，用 `SUBSTR` 而非 LIKE，避免 `%/_` 通配）；provider 前缀隔离跨源 raw ID 冲突，`agent` 列另做正交过滤。失败回退身份（vibe `session_` 目录名别名）在删除时一并写入排除表（`sessions.go:2399-2424,2450-2466`）。
- **消息身份**：`messages.id` 为手工分配的 `MAX(id)+1`（`messages.go:762-771`）；稳定身份是 `(session_id, ordinal)`（`schema.sql:106`）+ 溯源列 `source_uuid/source_parent_uuid/claude_message_id/request_id/is_sidechain/is_compact_boundary`（`schema.sql:98-105`）。
- **去重/覆盖规则**：会话 upsert 以 ID 冲突更新、`display_name`（用户改名）不被覆写（`sessions.go:1296-1342`）；`insertSessionIfAbsent` 供 recall 占位行使用、绝不覆盖真行（`sessions.go:1420-1453`）。消息替换分两档：可按 ordinal 做**原位 diff**（更新保留 rowid，FTS 触发器只重索引变更行）；出现截断/重排、重复 ordinal、源 UUID 不符或变更过半则退回全量删插（`messages_diff.go:179-235,237-296`）。
- **孤儿**：重同步把旧库中"新库没有、排除表没有、非 parser 排除"的会话作为孤儿拷回，且专门排除 Codex 中"同一 file_path 已被重新解析成新 ID"的陈旧行（避免把 #643 旧错行当存档救回）（`orphaned.go:141-173,191-234`）；侧栏可用 `IncludeOrphans` 把"父缺失的子行"提升为根（`query_dialect.go:436-452`）。
- **删除身份闭环**：`excluded_sessions` 只在确有行被删时写入（防幽灵排除，`sessions.go:2341-2389`）；`DeleteSessionIfTrashed`/`EmptyTrash` 先加锁再删，避免 TOCTOU（`sessions.go:2468-2536,2978-3044`）；parser 排除走独立路径、**不**写永久排除（`sessions.go:1221-1268`）。

## 4. FTS 与 vector：真实实现与失败行为

**FTS（消息）。** `messages_fts` 是 fts5 外部内容表（`content='messages', content_rowid='id', tokenize='porter unicode61'`），由 ai/ad/au 触发器同步（`db.go:377-394`）。批量重写路径在事务内先对 FTS 发批量 `'delete'` 命令、临时 DROP `messages_ad` 再删行、随后恢复触发器，避免逐行重分词（`db.go:364-375`；`messages.go:1277-1313`）。`DropFTS/RebuildFTS/HasFTS` 提供整体重建与能力探测（`db.go:3226-3275`）；首次在既有库上启用 FTS 时整表 rebuild（`db.go:3340-3357`）。fts5 模块缺失时 init **静默容忍**（只对 "no such module" 不报错，`db.go:3342-3347`），但 `searchContentFTS` 先查 `HasFTS()`，缺表返回显式 `errFTSUnavailable`（`search_content.go:617-634`）；用户查询语法错误用 SQLite 错误码映射成 `SearchInputError`(400)，其余错误原样透传（`search_content.go:700-714`）。

**FTS（recall）。** recall 条目/证据各有 fts5 表，init 失败降级 fts4（`db.go:396-486,3359-3435`）；查询期按 `sqlite_master` DDL 文本判型分派（`recall.go:768-804`），fts5 用 `bm25()/MATCH`、fts4 先 rowid 预筛 50000 再过滤（`recall.go:675-743,883-946`）。FTS 不可用或**零命中**时回退 LIKE 候选（`recall.go:612-636,855-866`），不可用判定基于固定错误串（`no such table/module/unable to use function MATCH`，`recall.go:1373-1385`）——正确性等价但调用方拿不到"本次是否走了降级引擎"。

**Vector 的真实边界。** `internal/db` 里只有 `VectorSearcher` 接缝与错误分类（`vector.go:1-119`）：未接线/后端不支持 → `ErrSemanticUnavailable`（带启用指引）；**查询期**嵌入端点失败 → `ErrSemanticTransient`（可重试、不伪装成"未启用"）。semantic/hybrid 在 `SearchContent` 提前分派（`search_content.go:180-185`）；searcher 报错一律向上抛，**没有** hybrid→lexical 的静默降级（`search_content.go:796-805,997-1013`）。可嵌入单元口径 = 非 system、非系统前缀的 user 消息 + 相邻 assistant 消息串（run），`since` 增量与"NULL/空 ended_at 必须重扫"约定（`messages.go:402-500,636-695`）。生成侧实现（`internal/vector`、`agentsview embeddings build` 调度）不在本分片范围，未读 → 残余。
## 5. 检索管线：候选获取 → 过滤 → 排序 → 分页

**会话列表（keyset）。** `ListSessions`：limit 钳制（`<=0 或 >500` 一律回落默认 200，`sessions.go:632-634`）→ 过滤 WHERE（`buildSessionFilter`，`query_dialect.go:385-512,531-675`：message_count>0、未删、子会话/孤儿/一次性/自动化作用域、项目/机器/分支/时间/健康/密钥等）→ COUNT（复用游标内 total，`sessions.go:641-662`）→ keyset 谓词（多键字典序展开，`query_dialect.go:307-345`）+ `ORDER BY`（末位自动补 id 决胜，`sort.go:228-241`）+ `LIMIT n+1` 判下一页（`sessions.go:664-703`）。游标为 HMAC-SHA256 签名 JSON，按 sort 误配直接拒收；兼容无签名的 legacy 令牌（`sessions.go:401-464`；`sort.go:270-292`）。侧栏：有 limit/cursor/starred 走递归 CTE 分页（root 活动度=子树 MAX，子行要求 message_count>0 且未删，`sessions.go:797-1046`）；**否则整表物化**（见 AGV-03）。

**内容检索（词法）。** 五模式入口统一做 limit 钳制与输入校验（`search_content.go:167-206`，substring 默认、未知 source/mode→400）。substring：UNION ALL 三源 + LIKE（escape `\`）+ 会话范围子查询，`ORDER BY julianday(sort_ts) DESC, session_id, ordinal, src, row_id`，`LIMIT n+1 OFFSET cursor`（`search_content.go:212-317`）；命中全文在 Go 里裁剪并**按整文脱敏**（防止窗口切断密钥，`search_content.go:536-567`）。regex：RE2 编译 + 可提取字面量做 LIKE 预筛（无字面量则全扫），分页靠"重扫候选、丢弃前 cursor 个已确认命中"，深页 O(cursor)（`search_content.go:367-432,439-533`）。fts：`MATCH ?` + 会话范围，`ORDER BY rank, ordinal, id`，同样 `LIMIT n+1 OFFSET cursor`（`search_content.go:625-659`）。三者过滤都在 SQL WHERE、cap 之前完成（相对 sessiongrep SG-01 的关键差异）。

**内容检索（semantic/hybrid）。** semantic：`k=max(limit*4,200)` overfetch（`search_content.go:716-720,801`）→ 一次批量解析允许会话（child 豁免一次性门槛、Scope 取代 IncludeChildren，`search_content.go:1290-1328`）→ 过滤 Scope → 从属惩罚 → 富化（本地 SQLite 全文）→ 截断 limit（`search_content.go:814-857`）。hybrid：两条腿各自 k overfetch、**合前**先做 Scope 过滤（`search_content.go:1022-1106`），FTS 命中经 ResolveMessageUnits 落到单元粒度、无单元命中保留消息粒度并按词法同规则分类（`search_content.go:1150-1206`），RRF(rank const 60、从属 +5) 融合后富化（`search_content.go:889-934,1218-1263`）。词法模式的单元范围/血缘是截断后的 O(page) 装饰（`search_content_units.go:25-56`），不会改变命中集合或排序。

**Recall 管线。** 文本查询：FTS 候选（bm25/预筛）→ 空或不可用回退 LIKE；再并合"证据命中、元数据命中（标题/项目/cwd 等）或时间信号全集"，全部以 `Limit=500` 截断（`recall.go:612-636,638-673,745-766,977-1007`）→ 合并去重 → `corerecall.Rank` → 稳定排序 → **来源多样化**（先每源取一，再回填）→ 最终 `recallLimit`（默认 50、上限 500）（`recall.go:1012-1045,1091-1198,1391-1399`）。默认只查 accepted；TrustedOnly 追加 human_reviewed+transferable+provenance_ok（`recall.go:1212-1281`）。

**对本任务三问的直接回答。**（a）cap 先于过滤：SQL 侧词法路径没有（过滤在 LIMIT 前）；但 semantic/hybrid 的 k 与 recall 每腿 500 是**候选截断先于范围过滤/排序**的有界变体（AGV-01/02）。（b）零证据升格：未观察到——semantic 命中全部来自 searcher 打分、hybrid 还要过 Scope/RRF，recall 候选必须命中文本/元数据/时间信号之一；没有"仅 recency/repo 相符即入选"的加分路径（与 sessiongrep SG-02 不同）。（c）静默降级：recall FTS→LIKE 不向调用方暴露引擎；fts5 模块缺失在 init 被静默容忍而在查询期对 fts/hybrid 显式报错；semantic 未接线/端点失败是两类显式错误，且无 hybrid→lexical 静默回退。

## 6. sync/ingestion 与删除/重扫语义；多 writer/并发

- **非破坏重扫。** 数据版本落后只置 stale + 日志"requires full resync"，不删数据；重扫= 建新库→拷孤儿/回收站/同步状态→交换（`db.go:29-34,854-870,3020-3032`；拷贝见 `orphaned.go:41-324,329-340`）。这与 sessiongrep "full 先 clear" 语义相反，是正向对照。
- **删除类路径全部显式**：`DeleteSession`/`DeleteSessions`/`EmptyTrash`（批量 500、先排除后删、消息级 FTS 优化删除）、`DeleteSessionIfTrashed`（TOCTOU 安全）、`PurgeExcludedSessions`（重同步后清残余）。唯一"由解析结论驱动"的删除是 `DeleteParserExcludedSessions(ids)`：把当前 parser 判定为非会话的旧行移除、**不**写永久排除，源文件不动（`sessions.go:1221-1268`）。
- **扫描缓存**：`skipped_files` 由 sync 每轮整体 DELETE+重插（`skipped.go:32-72`），只影响"下次是否再解析"，不含会话正文；`remote_skipped_files` 仅 mtime 缓存（`db.go:2129-2140`）。删除日志为镜像发布 tombstones（`session_deletions.go:14-90`；触发器 DDL 位于 `schema.sql:757-806`，仅结构索引）。
- **多 writer/并发。** 单写连接池（`MaxOpenConns(1)`，`db.go:3054-3060`）+ 进程内 `db.mu` 串行化所有写（`db.go:488-514,3566-3583`）；读者池 4 连接、5 分钟空闲（`db.go:798-805`）；DSN `busy_timeout=5000`、WAL、`synchronous=NORMAL`（`db.go:783-796`）。Reopen 采"先开新、原子换指针、旧池退休延后关闭"，避免在途读被切断（`db.go:3503-3564`）；CloseConnections 明确 writer 最后关以完成 WAL checkpoint（`db.go:3469-3498`）。DDL 竞态被专门处理：DROP+CREATE IF NOT EXISTS 允许双 Open 碰撞、`sync_marker` 触发器与回填单事务安装、schema.sql 单事务执行（防止删除触发器缺窗）（`db.go:2281-2285,2336-2363,2365-2384`）。边界：db 层没有跨进程锁文件；两个进程可同时以可写打开，靠 WAL+busy_timeout 语义互斥（设计上假定 daemon 持有写权）——编排侧证据未读，见 §9。

## 7. 发现清单（AGV）

#### AGV-01 · P2 · semantic/hybrid 在元数据过滤前先截断候选 k

- **证据**：`k=max(limit*4,200)` 先向 VectorSearcher 取 k 条，再按允许会话/Scope 过滤（`search_content.go:716-720,801-821,1043-1057`）；hybrid FTS 腿另有 `maxHybridFTSBatches=4` 的批数上限（`search_content.go:1061-1106`）。
- **可达**：窄 Scope（如 project=A）且探针 top-k 大量来自域外会话时，域内命中可能少出或少于 limit，即使域内更深排名仍有匹配；hybrid 的 FTS 腿在"折叠/丢弃占主导"时按注释自认可 under-fill（`search_content.go:1061-1067`）。
- **反证/界限**：相对 sessiongrep SG-01 是本集先取大再过滤（k≥200 且 4×），词法模式过滤仍在 LIMIT 前；Scope 过滤刻意在 RRF 前，避免"先截断再过滤只剩残片"（`search_content.go:763-768,1022-1027`）；残差已文档化。
- **ASG 教训**：语义候选截断必须与元数据过滤联合设计（或过滤下推 searcher），并用"域外高排名占满 k、域内仍有命中"的 fixture 验收。

#### AGV-02 · P2 · Recall 每腿 500 候选截断先于 Rank；FTS→LIKE 降级无引擎标识

- **证据**：`candidateQuery.Limit=MaxRecallEntryLimit(500)`（`recall.go:995-1007`，每腿 `recallLimit` 截断 `recall.go:689-743,745-766`），随后才 Rank/diversify（`recall.go:1019-1045`）；FTS 零命中/不可用静默回退 LIKE（`recall.go:612-636,855-866`），不可用靠错误串匹配（`recall.go:1373-1385`）。
- **可达**：单腿匹配数 >500 时，bm25/updated_at 之外的真实命中永远进不了排名；调用方无法区分本次结果是 FTS 还是 LIKE（无 engine 字段），延迟/召回画像在两种引擎间切换。
- **反证/界限**：500 是公开契约（默认 50/上限 500，`recall.go:16-21,1391-1399`）；LIKE 回退语义等价（escape+ESCAPE），错误分类特异性较高；最终来源多样化质量未实测。
- **ASG 教训**：候选截断位置要和排序契约一起定义；降级要么显式标志（engine/degraded），要么不允许。

#### AGV-03 · P2（条件性） · 侧栏索引无分页路径一次性物化全部匹配会话

- **证据**：仅 `Limit>0 || Cursor!="" || Starred` 才走分页；否则 SELECT 全部匹配行（含子行）并 `Total=len`（`sessions.go:711-795`，尤其 717-719、744-794）。
- **可达**：调用方不传 limit 时，大档案（万级会话）每次拉全量 skinny 行；分页路径已在同文件实现（`sessions.go:797-1046`）说明上限本可施加。
- **反证/界限**：行是瘦身投影（无正文），HTTP 路由可能强制 limit（未读）；这是"缺省参数"风险而非算法错误。
- **ASG 教训**：列表端点必须自证有界（默认 limit + 服务端上限），不能依赖客户端传参。

#### AGV-04 · P2 · 旧 `Search` 用 OFFSET 分页并在每页重跑整条 UNION

- **证据**：FTS 分支（ROW_NUMBER 取每会话最佳行）UNION 名称分支，`LIMIT ? OFFSET ?`，NextCursor=cursor+limit（`search.go:342-546`，尤其 419、496、541-544）。
- **可达**：深页 = 重复 FTS 扫描 + 丢弃前 N 行；并发写入时 OFFSET 页会漂移（无 keyset），用户可见重复/漏项。排序确定（rank→match_pos→时间→session_id，`search.go:350-360`），但确定性不解决漂移。
- **反证/界限**：这是会话级"找会话"入口，不是内容全量浏览；同库的会话列表已用 HMAC keyset（`sessions.go:401-464`），可作为改造样板。
- **ASG 教训**：检索分页一律 keyset；验收覆盖"翻页期间源数据变更"。

#### AGV-05 · P3 · 同一种能力（FTS 缺失）在不同入口的失败语义不一致

- **证据**：init 对 messages_fts 的 "no such module" 静默容忍（`db.go:3342-3347`，错误串匹配）；查询期 fts/hybrid 显式 `errFTSUnavailable`（`search_content.go:617-634`），而 recall 静默 LIKE 回退且无标志（`recall.go:612-636,1373-1385`）；语义检索则是两类显式错误（`vector.go:8-48`）。
- **可达**：同一无 FTS 部署，用户看到"fts 模式 501/400 错误"与"recall 悄悄变慢变浅"并存；运维无法从 API 层统一探测。
- **反证/界限**：`HasFTS()`/`HasSemantic()` 可提前探测（`db.go:3266-3275`；`vector.go:108-111`）；recall 回退保证功能可用，非错误吞没。
- **ASG 教训**：能力缺失用统一 capability/freshness 字段表达；降级必须可观测。

#### AGV-06 · P3 · `sync_marker` 对"仅含坏 created_at"的会话产生盲窗

- **证据**：坏时间戳一律以 `''` 参与 MAX（含 created_at），该会话 marker='' 且文档承认其"对增量窗口不可见，只有全量重建覆盖"（`db.go:2260-2270,2322-2334`）。
- **可达**：镜像增量推送会持续漏掉该行，直到任一真实信号出现或全量重建；数据在本地仍可查。
- **反证/界限**：这是防"坏串排序高于一切、永久推高 cutoff"的刻意设计；PG/DuckDB 两侧语义一致；本地查询不受影响。
- **ASG 教训**：坏输入防御要以"显式 fallback 时间戳/计数"补齐，而不是让行静默出窗；验收覆盖单信号腐烂行。

#### AGV-07 · P3 · 消息主键用 `MAX(id)+1` 手工分配，正确性依赖单写者

- **证据**：`nextMessageIDTx` = `SELECT MAX(id)+1`，随后多行显式 INSERT（`messages.go:723-771`）。
- **可达**：若另有进程/工具直写同库并交错插入，ID 冲突将报主键错误（**失败可见，不会静默串行**），但会中断本进程写入。
- **反证/界限**：db 层约定单写进程（单写连接池+互斥，`db.go:3054-3060,3566-3583`）；WAL 下并发写本身即被 SQLite 串行化。
- **ASG 教训**：稳定行 ID 用数据库自增/序列或显式 UUID，不依赖"读 MAX 再写"；若要保留，需把写者唯一性做成机制而非注释约定。

#### AGV-08 · P3/I · 跳过缓存全表重写（仅缓存，非会话数据）

- **证据**：`ReplaceSkippedFiles` 单事务 DELETE all + 重插（`skipped.go:32-72`）；`excluded_sessions` 语义独立、只在真实删除后写入（`sessions.go:2341-2389`）。
- **反证/界限**：`skipped_files` 只是"下次不再解析"的决策缓存，删它不删会话正文；单条修正已有 `DeleteSkippedFile`。
- **ASG 教训**：扫描状态与权威数据分离的边界要写清；缓存可丢/可重建 ≠ 目录可被"扫描清除"。

**正面反证（不应被上述发现淹没）**：重同步非破坏（`db.go:854-870,3020-3032`；`orphaned.go:41-241`）；排除表仅在真实删除后写入 + TOCTOU 安全（`sessions.go:2341-2536`）；FTS 批量删重优化（`db.go:364-375`；`messages.go:1277-1313`）；snippet 从全文脱敏（`search_content.go:536-567,1414-1462`）；keyset+HMAC 会话游标（`sessions.go:401-464`）；坏时间戳不得以原始串参与 MAX（`db.go:2260-2270`）。

## 8. 与 ASG 对照（强项/弱项/学与不学）

- **agentsview 强于 sessiongrep（SG 编号见 `sessiongrep-full-audit.md`）**：数据版本 fail-closed（拒开新版本档案，SG 无此概念）；非破坏重扫+孤儿/回收站/同步状态拷贝（对照 SG-04 的 clear-all）；keyset+HMAC 游标与 sort 误配拒绝（对照 SG-08 的裸 limit）；词法检索过滤在 cap 前完成（对照 SG-01）；snippet 全文脱敏、FTS 错误分类到 400/可用性判定（对照 SG-08 的吞错）；删除日志/身份修订日志支撑镜像增量（SG 无对应物）；单写者+原子 Reopen 的并发工程（对照 SG 的批量刷新无原子切换）。
- **agentsview 弱于/风险于 ASG 契约**：语义/recall 仍存在"候选截断先于过滤/排序"的有界变体（AGV-01/02）；侧栏缺省无界物化（AGV-03）；旧 Search OFFSET 深分页（AGV-04）；能力降级在不同入口语义不齐（AGV-05）；跨进程写者无显式锁（AGV-07 语境）；任何"新鲜度"字段（如镜头上是否 stale、FTS 是否降级）在 API 结果上没有统一表达（本分片看到的错误分类很好，但成功响应缺少 freshness/degraded 标志）。
- **ASG 该学**：①`user_version`+行级 `data_version` 双层与"过期不盖章、成功后 MarkDataCurrent"；②删除后写排除+幽灵防护+parser 排除独立通道；③keyset 游标（签名、类型化多键、sort 误配拒绝）；④外部内容 FTS 触发器 + 事务内"drop trigger→批量 delete→restore"重写协议；⑤"全信号 MAX + 坏时间戳不得以原始串参与比较"的窗口防呆；⑥删除日志（tombstone delta）与修订号传播；⑦失败分类学（输入错误 vs 能力缺失 vs 瞬态 vs 版本过新，各自 Is/As 判定）。
- **ASG 不该学**：把候选截断放在过滤/排序之前当作可接受的召回取舍（AGV-01/02）；无分页整表列表路径（AGV-03）；OFFSET 深分页（AGV-04）；无标识的静默降级（AGV-05）；以"单写进程假定"替代多 writer 验收（AGV-07）；用错误串匹配做能力判定（`db.go:3343-3345`、`recall.go:1380-1385`）——ASG 若实现同类判定，应落位到类型化错误/capability 探测。

## 9. 残余未决与未读边界

1. `messages.go:1951-2356`（parse-diff/PG-push 指纹构建器与 `SetToolCallSubagentSession` 系 helper）未读，维持 partial；不承载消息读写/分页/FTS 链路结论。
2. **重扫编排不在 `internal/db`**：`ResyncAll`/源文件发现/"源文件消失"的对账逻辑经函数索引确认不在本包（db 包提供 `NeedsResync`/`MarkDataCurrent`/`CopyOrphanedData*`/`ResetAllMtimes` 等原语）。因此"扫描删数据"风险只在 db 层被证伪为"非破坏 + 显式删除"，编排层（internal/sync 或 cmd）本轮未读，未能对"源目录被清空时的行为"下最终结论。
3. `internal/vector` 的索引生成/嵌入调度与故障重试未读（只审了 db 侧 `VectorSearcher` 接缝与 embeddable 单元口径）；`agentsview embeddings build` 的真实失败行为未验证。
4. Recall 子系统其余实现（evidence window、import、eval ingest、extract、query events）未读；本报告 recall 结论限于 `recall.go` 与其直接引用的 DDL。
5. `schema.sql:151-859`、`query_dialect.go`/`sort.go`/`messages_diff.go` 其余函数未逐行读；只对与本分片问题直接相关的窗口取证。
6. 未运行 build/test/查询/并发压测；一切并发与失败行为均为静态推导。`sweep-agentsview.json` 的 probe 命中（如 `sql_dynamic=30`/`concurrency=1`）未逐条回对源码行。
7. 快照仅代表 commit `a84564ad…` 与读取时哈希（读取前后五文件哈希逐一相等）；不声称上游最新或运行可用。