# 竞品源码审计：agent-sessions 分片 A（Swift 索引 / Search / 解析核心）

- Query: 对 agent-sessions 指定快照的索引/解析/Search 核心做逐行静态审计，回答 SQLite/FTS schema、索引发现与刷新新鲜度、sidecar 更新、身份与解析归一化、失败处置、搜索管线与结果投影、恢复/终端动作，并给出 ASG 对照结论。
- Scope: internal；仅 `C:\AgentSessions\Github_src\agent-sessions`（commit `af4793d317ab83e73ea205ff3f91feee32a3f0dd`，455 个 .swift）。规划研究，不实施整改。
- Date: 2026-10-06
- Status: **分片 A T1 五文件全文阅读完成；两个可选 indexer 为带精确 missing_ranges 的部分阅读；所有发现均为静态推导，未构建/未运行/未联网。**
- 阅读凭证: 同目录 `coverage-agent-sessions-core.json`（逐文件 bytes/lines/SHA256/精确 read_ranges/missing_ranges）。T1 五个文件共 9721 行全部 1..EOF 覆盖（bounded CodeGraph ranges，每段 ≤150 行，中途截断批次均补读后才计入）；哈希仅用于核对内容未变，不计为阅读。

## 0. 摘要（TL;DR）

agent-sessions 的索引核心是"**SQLite 单库 + FTS5 双层语料（会话正文 + 工具 IO）+ per-source 轻量元数据先行**"的桌面架构：

1. **冷启动强**：启动先只读 `session_meta` 水合列表并立即可交互，再后台做 changed/new/gap 三路增量扫描；≥8MB 的会话在打开时还能用 tail-only 解析先画出最后一屏（`isPartiallyHydrated` 一次性标记，随后整段替换）。
2. **新鲜度有明确契约**：FTS 命中只采信"mtime/size/format_version 三元组命中当前文件统计"的行（`indexedSessionIDsCurrent`），过期行被踢出索引集合、走 legacy 重扫；热文件有 120s quiet gate + 按体积分级的再入库冷却，并显式把"新鲜度延迟"写进注释。
3. **最大的正确性隐患是"失败被当成成功"**：Codex 读文件失败会产出一个仅含 error 事件的会话并可整段替换旧事件；Claude 解析返回 nil 时被 scan 静默丢弃，而文件统计水位照常前进——失败文件在不变更前不会再被重试，且 UI 无错误面。
4. **重索引有明确 guardrail**：新迁移必须走"只清 `session_meta`、不碰 FTS 语料"的 `reindexSessionMeta`，注释里写明这省的是重解析成本而不是可用性（记录了一次 5GB/3363 会话约 149s 的不可见窗口）；但两个历史标记仍会整体清空语料，是"不要复制"的形态。
5. **恢复/终端动作克制且可审计**：命令经过 shell 引号封装走 osascript argv（不是字符串拼接脚本），Codex 先探测 CLI 版本是否支持 resume-by-id，失败链 `resume || -c experimental_resume=path || 组合`；Claude `--resume/--continue` 有 resume→continue 的降级。

## 1. SQLite / FTS schema 与索引契约

### 1.1 表结构与写入面（DB.swift）

- 单库位于 `~/Library/Application Support/AgentSessions/index.db`，`IndexDB` 是 Swift actor（DB.swift:12），pragma 为 `journal_mode=WAL / synchronous=NORMAL / busy_timeout=5000`（DB.swift:55-59）。
- `files`：path 主键 + mtime/size/source/indexed_at（67-74），是全库"文件水位"的唯一事实源。
- `session_meta`：session_id 主键 + source/path/mtime/size/start_ts/end_ts/model/cwd/repo/title + 子代理列（parent_session_id、subagent_type、custom_title）+ Codex 表面元数据（codex_internal_session_id/codex_originator/codex_source/codex_surface）+ 归一化的 originator/origin_source/surface/reasoning_effort + is_housekeeping/messages/commands（81-104）。列扩展全部走 `PRAGMA table_info` 预检 + duplicate-column 容忍的 best-effort 迁移（111-219）。
- `session_search`：session_id 主键 + source/mtime/size/updated_at/text/format_version（276-284）；`session_search_fts` 是 external-content FTS5（content='session_search'），tokenize=unicode61，配 ai/ad/au 三个同步触发器（301-323）。**FTS 不可用时静默降级**（`catch {}`，325-327）。
- `session_tool_io` 同构（332-345）+ `session_tool_io_fts`（348-373），列 ref_ts 记录会话引用时间用于回收。
- 分析表：`session_days`（day/source/session_id 主键 + model/messages/commands/duration_sec/meta_mtime）（230-240）；`rollups_daily`（day/source/model 主键 + sessions/messages/commands/duration_sec）（247-257）；`rollups_tod` 预留（264-269）。
- 状态表：`schema_migrations`（222）、`index_state`（225，KV：analytics backfill 标记、`core_file_stats_v1:codex` 水位 JSON）。
- 关键写入路径（均在 actor 内单语句 prepare/bind）：`upsertFile`（1572-1583）、`upsertSessionMeta`（全量覆盖式 ON CONFLICT，1585-1627）、`upsertSessionMetaCore`（核心 indexer 用；`custom_title`/`originator`/`origin_source`/`surface`/`reasoning_effort` 用 COALESCE 或 CASE 保留旧值，1633-1649）、`upsertSessionSearch`（1681-1704）、`upsertSessionToolIO`（1706-1731）。

### 1.2 迁移与"重索引 guardrail"

- 375-401 有一段罕见的**显式工程契约注释**：以后的重索引标记必须走 `reindexSessionMeta(db, sources:)`（只 DELETE `session_meta`，1375-1383），不得 DELETE `session_search`/`session_tool_io`；并明确写明这个模式"保存语料不等于保持可用"——`searchSessionIDs` inner-join `session_meta`，在该表为空期间该 source 的列表与搜索会**静默归零**，实测 5GB/3363 会话约 149s（392-396 的公开记录）。
- 两个历史标记 `subagent_reindex_v2`（406-415）与 `custom_title_reindex_v1`（418-427）仍执行 files/meta/search/tool_io/days/rollups 的全量 DELETE，属于被注释点名"不要模仿"的形态；新的 `codex_guardian_subagent_reindex_v1` 已改用保语料路径（482-486）。
- 删除路径：`deleteSessionsForPaths` 按 200 一批清 session_days/search/tool_io/meta/files（1405-1540）；`purgeOrphanedSessionMeta` 清"files 里已无此 path"的 meta 及派生行（2193-2233）；`purgeSource` 全清某 source（1322-1332）。这些删除**没有包在显式事务里**（对比 965-981 的单事务写入），中断时可能留下半清理状态（见 AF-06）。

### 1.3 索引发现与 watch/刷新/新鲜度

- **没有 OS 级文件监视器**：机械扫描（sweep-agent-sessions.json，probe fs_watch）在 953 个可扫描文件中 0 命中；刷新全部由显式触发 + 周期轮询构成。
- 启动路径：`AgentSessionsApp.runStartupTasksIfNeeded` 在 AppReadyGate 之后调用 `unified.refresh(trigger: .launch)`（AgentSessionsApp.swift:831-838）。
- 每个 provider indexer 的 refresh 先尝试 DB 水合（hydration），空结果 250ms 后重试一次，然后无论水合是否成功都继续增量扫描（SessionIndexer.swift:743-770）。
- 增量发现：`CodexSessionDiscovery.discoverDelta(previousByPath:scope:)` 对比上次水位得到 changedFiles/removedPaths/currentByPath；scope 在 `.fullReconcile`/manual/launch 时为 full，否则 recent（SessionIndexer.swift:818-821）。再补一路 **gap 检测**：`diskPaths - hydratedSessionPaths - changedPaths` 强制解析"磁盘有但水合快照没有"的文件（833-848）。水位持久化在 `index_state` 的 `core_file_stats_v1:codex`（94, 1253-1284）。
- 周期监视：`UnifiedSessionIndexer` 有两条 Task 轮询——新会话监视（前台 300s / 后台 60s，`runNewSessionMonitorLoop` 1484-1497）与**聚焦会话监视**（按 source 不同节拍：Codex 4/8/20/60s，Claude 6/10/25/60s，默认 8/12/20/60s，按 活跃/后台 × AC/电池 选择；47-81、1509-1513）。目录快照变化才触发 provider refresh，且受最小间隔限制（前台 10 分钟、后台 3 分钟，1564-1575），refresh 本身经 `ProviderRefreshCoordinator` 10s 合并窗口（681）。
- 目录快照只覆盖 Codex（近 3 天 rollout-*.jsonl）与 Claude（projects 目录）两个 source（1534-1537）；其余 provider 依赖 launch/manual/provider-enabled/cleanup 等显式触发（1261-1278、1402）。
- **FTS 新鲜度契约**：`indexedSessionIDsCurrent` 用 files⋈session_meta⋈session_search 且 `s.mtime=f.mtime AND s.size=f.size AND s.format_version=?` 才认"当前"（1791-1848）；SearchCoordinator 同时取"存在"集合与"当前"集合，差值 `staleIDs` 走 legacy 全扫并绕过体积门槛（SearchCoordinator.swift:296-305、633-649）。热文件再入库受 120s quiet gate + 按文件体积分级的冷却（SearchIngestService.swift:105、198-245），且冷却以 `session_search.updated_at` 持久化、失败事务回滚不推进（DB.swift:745-752 的注释 + 1681-1704 写入点）。

### 1.4 sidecar（标题等）更新路径

- **Codex 标题双通道**：
  1. `~/.codex/session_index.jsonl` 的 `thread_name`：`loadCodexThreadNames` 读全量（≤2MB）或尾部（>2MB），缓存键 = path+mtime+size+头尾各 8KB 的 FNV-1a 指纹+120s 最大年龄（SessionIndexer.swift:1693-1711、1733-1847）；`applyCodexThreadNames` 把差异名覆盖为 `customTitle`（1849-1886），随后经 `sessionMetaRow`（1187-1197）落库，`upsertSessionMetaCore` 的 COALESCE 保证不被 NIL 冲掉（DB.swift:1643）。
  2. `state_<n>.sqlite` 的 threads 表（title/first_user_message/cwd/git_branch/git_origin_url）：每轮 refresh 读取（1509-1525），`applyCodexStateMetadata` 回填 hydrated 会话的标题/cwd/repo（1596-1651）。
- **Claude 标题三通道**：转录内 `custom-title` > `ai-title` > `agent-name`（ClaudeSessionParser.swift:94-108，长会话还有 8MB 预算的 gap 扫描 899-972）；Claude Desktop sidecar（`ClaudeDesktopSessionMetadataReader`）在解析后合并 title/cwd/model/时间（151-192）；hydrated 会话有受 `titleRepairLimit` 限制的修复遍（ClaudeSessionIndexer.swift:662-705）。
- 标题优先级最终收敛在 `Session.title`：customTitle 绝对优先，其次 lightweightTitle（带 preamble/本地命令日志过滤），再按首用户/首助手/工具名回退（Session.swift:442-555）。

## 2. 会话 / 消息身份与解析归一化

### 2.1 身份模型

- **会话身份 = SHA256(文件路径)**：Codex 与 Claude 都用 `hash(path:)` 生成 64-hex ID（SessionIndexer.swift:2772-2775；ClaudeSessionParser.swift:1172-1176），因此同一文件稳定、跨启动稳定；重命名/移动文件 = 新会话，复制同一会话 = 两个会话。DB 对遗留的非 64-hex ID（旧版 Swift hashValue）有 `hasUnstableIDs` 检测供重建（DB.swift:1309-1319）。
- provider 原生 ID 作为**附属标识**保存：Claude 的 `sessionId` 存入 `codexInternalSessionIDHint`（ClaudeSessionParser.swift:62-64、140）；Codex 的 `payload.id`（新版本优先于 parent 语义的 `payload.session_id`）由 `deriveCodexInternalSessionID` 提取（Session.swift:713-747，注释解释 0.145+ guardian 场景）。
- DB upsert 冲突键是 `session_id` 单键（DB.swift:1637），查询 join 用 `(source,path)`/`(source,session_id)` 双条件；在"路径哈希即 ID"的前提下不会撞键，但若未来 ID 生成语义变化，这个不对称需要重新审视（AF-05）。
- **消息级身份存在于事件层**：`SessionEvent` 携带 messageID/parentID（Claude 从 `uuid`/`parentUuid`，ClaudeSessionParser.swift:284-285、418-422；Codex 从 `message_id`/`parent_id`/`id`，SessionIndexer.swift:2459-2461）与 isDelta 标记；但 Session/ParsedSession 没有结构化父边/分支模型（parent 只用于事件字段与 subagent 层级），也不保留源字节偏移。

### 2.2 解析归一化要点

- Claude：逐行 JSON 解析，坏行静默跳过（ClaudeSessionParser.swift:54-59）；顶层 tool_use/tool_call/tool_result 特判（288-328）；`message.content[]` 按块拆分——text 合流、thinking 变 `[thinking]` meta 事件、tool_use 变 tool_call、tool_result 按 5 类 disposition 归类（ok/runtimeError/rejectedOrPermissions/notFoundOrMismatch/otherToolFailure，583-700），文件型 payload 不因内容里出现 "exit code" 被误判为运行错误（639-643）。
- 大内容防护：单事件 rawJSON 的字符串字段 >8KB 替换为 `[OMITTED bytes=...]`，非字符串 JSON >64KB 替换为 `[OMITTED large JSON payload]`；图片按块摘要 `[image omitted: ...KB]`（8-9、702-736、1126-1165）。这意味"保留原文"是**有界保真**，不是事件级证据副本。
- 时间归一化：秒/毫秒/微秒按数量级启发式（`>1e14` 微秒、`>1e11` 毫秒，1109-1113、2827-2831）；ISO8601 有无小数两种 + 4 种 fallback 格式（1097-1107、2804-2825）；Claude 的 caveat/local-command 文案有专门清洗（Session.swift:970-1016）。
- 子代理层级：Claude 从 `.../subagents[/workflows/wf_*]/agent-*.jsonl` 的最后一个 `subagents` 前一段取父 UUID，并读相邻 `*.meta.json` 的 `agentType`（ClaudeSessionParser.swift:1178-1214）；Codex 从 `session_meta.payload.source.subagent` 的 thread_spawn.parent_thread_id/agent_role、`{"other":"guardian"}` 及顶层 parent_thread_id 三种形态提取（SessionIndexer.swift:2004-2040）。
- 轻量解析：两家都是"头 256KB 起步（Claude 可扩到 2MB）+ 尾 256KB + 各 300 行"采样，事件数按 head 平均行长估算（ClaudeSessionParser.swift:769-1041；SessionIndexer.swift:2175-2402）。Claude 头尾切片用 lenient UTF-8 解码，避免 CJK/emoji 边界截断导致整文件被判空（803-820 的 issue #49 注释）。

### 2.3 失败与不完整解析的处置（关键差异）

- **Claude 文件级失败 = nil，静默丢弃**：`parseFileFull` 读错误直接 `return nil`（ClaudeSessionParser.swift:115-120）；`SessionIndexingEngine` 的 worker 只用非 nil 结果（SessionIndexingEngine.swift:143-165），nil 文件不进入 changedSessions，progress 照常前进。
- **Codex 文件级失败 = 合成 error 事件，会话仍然"成功"**：读失败产出 1 个 `kind == .error` 的事件并照常构造 Session（SessionIndexer.swift:2056-2060）；因为 error 事件非 meta，`nonMetaCount >= 1`，"失败会话"看起来像有内容。
- **合并策略只保"新结果为空"的情况**：refresh 的 mergedByPath 仅当 `existing.events 非空 && session.events 为空` 时保留旧事件，否则用新结果覆盖（SessionIndexer.swift:910-947；ClaudeSessionIndexer.swift:371-405 同构）。因此 Codex 的 1-event error 会话会**覆盖**原有的完整事件数组。
- **失败会推进水位**：`applyKnownFileStatsDelta` 在扫描后无条件把当前磁盘 stats 写入水位（SessionIndexer.swift:1308-1324，调用点 962；ClaudeSessionIndexer.swift:412），而 discoverDelta 的 changed 判定基于这个水位——一个解析失败但 mtime/size 已记录的文件，在文件再次变化前不会再被解析/重试，UI 也没有错误面。对照 sessiongrep 审计的 ASG 教训（SG-03："失败不能推进成功水位"），这是本分片最值得 ASG 吸取的问题（AF-01）。
- **不完整解析的正面设计**：Task 9e 的 tail-only 预绘会显式打 `isPartiallyHydrated=true`（不可持久化），所有"已水合"判定都必须排除它，随后整段替换（Session.swift:115-121、SessionIndexer.swift:446-501、2101-2172）；这个"部分结果有名字、且不会短路全量解析"的模式是清晰的。

## 3. 搜索管线与结果投影

### 3.1 管线（SearchCoordinator.swift）

1. 入口 `start(query:filters:include*:enableDeepScan:all:)`（165-460）：先取消在飞任务并换 runID；纯元数据过滤（无自由文本）直接内存过滤返回（213-227）。
2. Phase 0 FTS（enableFTSSearch=true，FeatureFlags.swift:49）：Cursor 被排除出 FTS（242）；`hasSearchData` 为假时整体回退 legacy（258-267）。
3. 自由文本路径（292-412）：取 `indexedIDsCurrent`/`indexedIDs`，算出 staleIDs；全部过期→legacy；否则构造 FTS 查询（`makeInstantFTSQuery`：多词→短语引号包裹；单词 ≥3 字符且 ASCII 简单词→前缀 `*`；显式 FTS 语法不重写，655-700），SQL 内完成 model/路径前缀/时间过滤 + bm25 排序 + LIMIT（DB.swift:1866-1930），`dbResultLimit` 默认 2000、当 repo/archived 过滤无法下推时提升为 `max(limit, all.count)`（SearchCoordinator.swift:735-742）。
4. 命中 ID 经 `byID`（由内存元数据过滤后的候选构建，269-277）投影为 `[Session]` 并**先发布**（333-341）；tool IO FTS 命中在限内去重追加（345-374，默认开关关闭）；未索引/过期候选与 deep 候选转入后台深扫逐批 append（376-410、744-973）。
5. Legacy 路径（462-622）：按 10MB 阈值分小/大两队列，小批 64 并发扫描 + 大队列逐条全解析，支持 promote 抢占（121-126、536-542），进度 10Hz 节流。
6. 深扫只在用户开启对应偏好时有效：`enableDeepToolOutputSearch` 与 `enableRecentToolIOIndex` 默认 OFF（85-97）。

### 3.2 结果投影与排序边界

- 结果不是"搜索命中模型"，而是 `[Session]` 行对象：FTS 只回 ID，投影依赖 `session_meta` 水合出的行（byID）；**FTS 命中但不在内存候选（或缺 meta 行）的会话会被直接丢弃**（335）。
- 输出顺序是分段拼接：bm25 序 → tool IO 追加（FTS 序）→ 后台未索引/深扫追加（候选的 modifiedAt 降序/批序）→ legacy 全量为 modifiedAt 降序。**没有最终的全局重排或统一 rank 契约**（333-374、496-527、793-973）。
- 过滤与截断的交界：repo/archived 有专门限额提升（735-742），但 `sideChatsOnly` 仅存在于内存交集，未提升限额（见 AF-03）。

## 4. 恢复 / 终端动作（锚点）

- 命令构造：
  - Codex：内部 session_id 优先、否则文件名 UUID；`codex resume <quoted>`，带路径回退时 `resume || -c experimental_resume=<quotedPath> || 两者组合`；VSCode surface 拒绝恢复；`cd <cwd> && ...`（CodexResumeCommandBuilder.swift:22-61）。
  - Claude：`claude --resume <quotedID>` 或 `claude --continue`，`cd <cwd> && ...`（ClaudeResumeCommandBuilder.swift:19-48）。
  - 引号统一走 `ShellQuoting.quote`（CodexResumeCommandBuilder.swift:63-64；ClaudeResumeCommandBuilder.swift:51-52）。
- 终端执行：
  - Terminal.app / iTerm2 用 `/usr/bin/osascript -e ... argv` 且**命令作为 argv 传入**，不把命令插值进 AppleScript 源码（AgentTerminalLauncher.swift:8-47、136-151）；非 0 退出把 stderr 作为错误抛出（147-151）。
  - Warp/WarpPreview 走临时 tab-config TOML + URL scheme，30s 后清理临时文件（50-112）；TOML 转义仅处理反斜杠/引号/换行（128-134）。
  - `TerminalKind.infer` 用 `__CFBundleIdentifier` 优先于 `TERM_PROGRAM` 区分 warp/warp-preview（TerminalKind.swift:12-27）。
- 门控与降级：
  - Codex quick launch 先探测 CLI 版本，`supportsResumeByID` 不满足则要求配置回退（CodexResumeCoordinator.swift:42-57）；launchMode 决定 iTerm/Warp/default Terminal（71-90）。
  - Claude 先探测 `--resume`/`--continue` 能力，策略 `resumeByID` → `continueMostRecent`；**Terminal 启动失败**也会再降级到 continue（ClaudeResumeCoordinator.swift:29-99）。
  - 会话文件不存在时拒绝启动（CodexResumeCoordinator.swift:43-45）。
- 未审计边界：ResumeHealthCheck、CodexResumeSheet（交互 UI）、各 provider 自己的 TerminalLauncher 适配层（Antigravity/Copilot/Cursor/Hermes/OpenCode/Pi）本轮只做定位，未逐行阅读。

## 5. Findings（严重度 / 证据 / 反证）

> 严重度口径沿用本次审计：P1=核心正确性优先核查；P2=有条件的正确性/可用性风险；P3=较低优先级或需特定条件；I=明文取舍/边界记录。全部为静态推导，未运行复现。

### AF-01 · P2 · 解析失败被当成成功，并推进文件水位（吸取 SG-03 教训）
- **证据**：Codex 读失败→1 个 error 事件仍构造 Session 并在合并中覆盖旧事件（SessionIndexer.swift:2056-2060、910-947）；Claude 读失败→nil 被 scan 静默丢弃（ClaudeSessionParser.swift:115-120；SessionIndexingEngine.swift:143-165）；两家都在扫描后把当前磁盘 mtime/size 写入水位（SessionIndexer.swift:1308-1324+962；ClaudeSessionIndexer.swift:412），失败文件在再次变化前不会被重试；UI/DB 无"该文件解析失败"字段。
- **可达**：权限/IO 抖动或坏行导致的新文件失败后，Claude 侧该会话可以一直不出现（水位已前进）；Codex 侧旧完整事件可被 1 事件 stub 覆盖直到再次变化或手动 reload。
- **反证/界限**：反例是"能解析"的绝大多数路径；Codex 的 error 事件在 UI 可见（不是静默），`reloadSession` 有 force/manual 通道可重读（SessionIndexer.swift:359-430）；Claude 已有的条目在 nil 时因 mergedByPath 保留而不会丢（只有全新华档会缺席）；原始 JSONL 文件从未被改写。
- **ASG 对照**：解析完成度与错误计数要显式传播；失败不得推进成功水位；"保留 last-good"要覆盖"新解析失败"而不只是"新结果为空"。

### AF-02 · P2 · 重索引窗口内该 source 的搜索/列表静默归零（已被源码记录）
- **证据**：guardrail 注释明确 `searchSessionIDs` inner-join `session_meta`，表空期间"missing from the list AND every search returns zero hits for them — silently, not as an error"，实测 5GB/3363 会话 ~149s（DB.swift:388-396、1346-1354）；保语料路径 `reindexSessionMeta` 仍清空 meta（1367-1383）；两个历史标记仍整体清空语料（406-427）。
- **可达**：升级后首次 bootstrap 触发任一标记；或未来新标记沿旧形态复制。
- **反证/界限**：新标记已收敛到保语料路径（482-486）；一次性标记不是每轮 refresh；成本节省真实（避免 GB 级重解析）；149s 是实测样本非普遍承诺。
- **ASG 对照**：需要"重建中"状态面（freshness/partial 字段）或在切换时保留可查询的 last-good 列；"省成本"不能以静默零结果为代价。

### AF-03 · P3 · `sideChatsOnly` 自由文本检索可被 FTS 候选截断饿死
- **证据**：ftsResultLimit 只为 archivedCodexDesktopOnly/repo 提升（SearchCoordinator.swift:735-742）；sideChatsOnly 仅通过内存 byID 交集生效（269-277、715-733），而 FTS 在全库 bm25 序上先 LIMIT=2000（DB.swift:1901-1909；FeatureFlags.swift:50）。
- **可达**：>2000 条更相关命中时，域内匹配的 side chat 不在候选集，结果为空/偏少。
- **反证/界限**：2000 很大、sideChatsOnly 是低频过滤；cursor/stale/未索引有独立补扫；同一问题在 sessiongrep 的 SG-01 更严重（过滤后截断），这里只剩这一条窄缝。
- **ASG 教训**：把"不能下推到 SQL 的资格过滤"统一纳入限额提升条件，并用"域外高排名 > cap、域内仍有命中"fixture 验收。

### AF-04 · P3 · FTS 故障/降级没有健康面，静默走 legacy 或空结果
- **证据**：FTS5 DDL 失败被空 catch 吞掉（DB.swift:325-327、371-373）；SearchCoordinator 对 FTS 查询一律 `(try? ...) ?? []`（319-329），ids 为空但 indexedIDs 非空时靠 unindexed 候选兜底 legacy 扫描；没有 freshness/partial 字段回报 UI。
- **可达**：FTS5 不可用或 FTS 查询异常时，性能与召回形态静默改变；用户无法区分"无命中"与"索引坏了"。
- **反证/界限**：legacy 路径仍能找到内容（正确性兜底存在）；session_search 表与 FTS 分离，语料还在；异常路径不报错是为了可用性（设计取舍）。
- **ASG 教训**：保留可用性可以，但要在结果/状态面区分 last-good、重建中、降级（这正是 sessiongrep SG-08 的同款建议）。

### AF-05 · P3 · `session_id` 单键冲突 vs `(source,path)` 语义的不对称
- **证据**：upsert 冲突键只有 `session_id`（DB.swift:1637、1688、1711）；而几乎所有读取 join 都是 `source+path` 或 `source+session_id` 双条件（717-743、1810-1848、2193-2233）；`hasUnstableIDs` 说明历史上 ID 生成语义确实变过（1309-1319）。
- **可达**：若两个 source 因配置/路径差异产出同一 ID（或未来再次更换 ID 生成），后写会覆盖先写的 source 行，而 files/search 各行仍按 source 独立存在，出现跨表不一致。
- **反证/界限**：当前 ID=SHA256(路径) 且路径全局唯一，实际不可达；这是防御性边界而非已发生缺陷。
- **ASG 教训**：冲突键应与所有读取路径的身份口径一致（(source,id) 复合键或显式唯一约束）。

### AF-06 · P3 · 部分删除/重算路径无事务，被中断可留半清理状态
- **证据**：`deleteSessionsForPaths`（逐表 DELETE，1405-1540）、`purgeSource`（1322-1332）、`purgeOrphanedSessionMeta`（2193-2233）、`recomputeAllRollups`/`recomputeRollups`（DELETE+INSERT 分开执行，2398-2411、2503-2523）都没有显式 begin/commit 包裹，而正常写入有单事务（965-981）。
- **可达**：用户在删除/重建期间退出 App，或磁盘错误中断，会留下"search 已删而 meta 未删"或"rollups 当日缺失"。
- **反证/界限**：这些是派生数据，下一次 refresh/recompute 可自愈；单条 SQL 自身原子；中断窗口小。
- **ASG 教训**：多表一致性操作统一进事务，验收故障注入（对照 SG-04 的"批量刷新与 freshness 不是同一契约"）。

### AF-07 · P3 · 搜索结果序无统一契约（bm25 → toolIO → 增量扫描 → legacy 分段拼接）
- **证据**：初始发布 bm25 序（333-341），随后 tool IO 按 FTS 序追加（345-374），后台深扫按候选批序/时间序追加（793-973），legacy 全量按 modifiedAt 降序（483-484、496-527）。
- **可达**：同一查询在不同索引状态下命中顺序不同；深扫命中永远排在 bm25 命中之后。
- **反证/界限**：这是"快速可见→渐进补全"的 UX 取舍，注释可见；每段内部有序；不是错排而是无全局承诺。
- **ASG 教训**：对"渐进结果"显式定义分段语义（phase/rank 字段），不要让阶段顺序伪装成相关性排序。

### AF-08 · I · rawJSON 有界截断不是"完整转录"
- **证据**：Claude 单事件 rawJSON 字符串字段 >8KB 变 `[OMITTED bytes=N]`、整体 >64KB 变 `[OMITTED large JSON payload]`、图片只留摘要（ClaudeSessionParser.swift:8-9、1126-1165、702-736）；Codex 对 >100KB 行做图像/encrypted_content/instructions 清洗（SessionIndexer.swift:1999-2000、2556-2756）。
- **可达**：任何以 rawJSON 为"原始证据"的下游（导出、证据定位）都会拿到省略文本。
- **反证/界限**：这是内存/性能的明文取舍，清洗发生在事件副本而非源文件；toolOutput/toolInput 字段保留可用文本；对"会话召回"目标成立。
- **ASG 教训**：保留 ASG 的 placement/字节级证据契约，不要把竞品的"有界保真"当同等级数据。

### AF-09 · I · Claude sidecar 标题晚到时的修复是有界抽样
- **证据**：hydrated 会话的标题修复只处理 `i < titleRepairLimit` 且标题"像本地命令日志"的条目（ClaudeSessionIndexer.swift:662-705）；Desktop metadata 只在解析时合并（ClaudeSessionParser.swift:151-192）。
- **可达**：sidecar 后写、且转录内没有 custom-title 的老会话，标题可能长期停留在旧值（直到文件变化触发重解析）。
- **反证/界限**：`custom-title` 记录会随转录重解析恢复；Codex 侧 thread_name 每轮 refresh 全量应用（对比之下 Claude 没有同款主动回填）。
- **对 ASG**：sidecar 驱动的可变更元数据（标题/归档位）应有独立的低水位回填任务，不依赖主日志 mtime。

### AF-10 · 正面证据（应保留的强项）
- **S1 冷启动**：meta-only 水合（不依赖 rollups，SessionIndexer.swift:1326-1334）→ 先发布后扫描（778-794）→ gap 检测（833-848）→ ≥8MB 会话 tail-first 预绘（446-501、FeatureFlags.swift:127）且预绘不短路全量解析（Session.swift:115-121）。
- **S2 新鲜度**：currency-aware 成员集合 + stale 绕体积门槛 + quiet/cooldown 明文契约（见 1.3）。
- **S3 重索引治理**：保语料原语 + 显式"省成本≠保可用 + 实测停机时间"注释（DB.swift:375-401、1334-1363）。
- **S4 恢复安全**：argv 版 AppleScript、shell quoting、CLI 能力门控、resume→continue 降级、VSCode surface 拒绝、文件存在性门禁（见第 4 节）。
- **S5 事件解析**：tool_result 五类 disposition、`is_error` 显式优先、文件 payload 防误判、thinking 单独 meta、delta 标记（ClaudeSessionParser.swift:281-700；SessionIndexer.swift:2459-2465）。

## 6. 与 ASG 对照

> 参照物是 sessiongrep 审计（`sessiongrep-full-audit.md` 的 SG-01…SG-10）与 ASG 既有契约（原生消息身份、严格形状归一化、失败水位不前进）。配额/额度工作台的专门实现（ClaudeStatusService/CodexStatusService/PreferencesView+Usage）不在本分片阅读范围，下面对照限定在**索引与分析核心**能支撑的层面。

### 6.1 冷启动

- **agent-sessions 强在**：两阶段启动（水合→扫描）让 UI 秒开；水位持久化 + gap 检测避免"水合快照缺行就永远缺"；tail-first 预绘把超大会话首屏压到毫秒级且标记为一次性部分结果；扫描用有界 worker group 并节流进度。
- **agent-sessions 弱在**：水合行没有事件，列表标题质量完全依赖轻量解析的采样与 sidecar 修复（AF-09）；升级触发的重索引窗口会静默清空该 source 的可用性（AF-02）。
- **ASG 该学**：水合即用 + gap 差集重扫 + 部分结果显式命名（isPartiallyHydrated 且不得短路全量）；**不该学**：把重索引的可用性代价留给注释而不给用户状态面。

### 6.2 连贯性（一致性）

- **强**：三元组（mtime/size/format_version）的新鲜度判定贯穿 ingest 跳过、搜索资格与路径集合；`upsertSessionMetaCore` 的 COALESCE 保留策略；"保留完整事件"合并规则；source+path 双条件 join；provider 前缀化 ID（Claude session_id 进 hint 而非主键）。
- **弱**：失败=成功 + 水位推进（AF-01，本分片最大差距）；删除/重算非事务（AF-06）；ID 冲突键不对称（AF-05）；结果序无契约（AF-07）。
- **ASG 该学**：新鲜度判定用"当前性"而不是"存在性"；过期条目显式走重扫而不是返回旧文本。**不该学**：失败静默、旧事件被 stub 覆盖、按分段顺序当排名。

### 6.3 配额工作台

- **本分片可见的强项基础**：`session_days`（day×source×session_id，含 meta_mtime 新鲜度列）+ `rollups_daily`（day×source×model）为"按天/按模型/按来源"的额度型 KPI 提供即取聚合；多天会话用最大余数法分摊消息/命令数**保证总数守恒**（DB.swift:2318-2387）；`findSessionsNeedingDayUpdate` 用 meta_mtime 做增量重算（2424-2447）；tool IO 有 30 天窗口 + 8MB 老数据上限的回收（1997-2045、FeatureFlags.swift:57-61）；hideZero/hideLow/housekeeping 过滤在 UI 层有对应字段（SessionIndexer.swift:260-262）。
- **弱项**：分析重派生期间的可用性缺口（AF-02 同源）；每 (day,source) 的 rollups 重算 delete+insert 无事务（AF-06）；Analytics 的构建完成标记是 per-source+version 的 KV（DB.swift:602-648），坏标记下的重派生成本可见。
- **无法下结论的部分**：额度抓取/展示（usage/status/overage）本分片未读，列为残余。

### 6.4 该学 / 不该学（清单）

**该学**：
1. currency-aware 索引资格 + stale 绕体积门（DB.swift:1791-1848；SearchCoordinator.swift:296-305、633-649）。
2. 保语料重索引原语 + "省成本≠保可用"的显式风险注释与实测数字（DB.swift:375-401、1334-1363）。
3. tail-first 预绘 + `isPartiallyHydrated` 一次性标记（SessionIndexer.swift:446-501、2101-2172；Session.swift:115-121）。
4. 失败水位不前进、last-good 覆盖"失败"而不仅是"空结果"（反向吸取 AF-01）。
5. 不可下推过滤的限额提升 + 分段结果的显式语义（AF-03、AF-07 的正向解法）。
6. 终端启动 argv 化 + CLI 能力门控 + 降级链（第 4 节）。

**不该学**：
1. 整体清空 session_search/tool_io 的历史迁移形态（DB.swift:406-427）。
2. 静默零结果/静默降级（DB.swift:388-396、325-327；SearchCoordinator.swift:319-329）。
3. 解析失败视为成功并推进水位（AF-01 证据链）。
4. 有界 rawJSON 当完整摘录对外承诺（AF-08）。
5. 用执行阶段（FTS→toolIO→扫描）隐式充当结果排名（AF-07）。

## 7. Caveats / 残余未决

- **部分阅读**：UnifiedSessionIndexer（读 475/2867，missing `1-39,120-679,746-1254,1345-1401,1641-2867`）与 ClaudeSessionIndexer（读 174/1017，missing `1-329,434-659,730-1017`）。关于这两文件的结论仅限已读区间；其余章节未声称审计。
- **仅定位未全文**：SearchIngestService（quiet/cooldown 仅按 grep 锚点+DB 注释引用，未逐行）、SessionIndexingEngine 除 130-179 外未读、ResumeHealthCheck/CodexResumeSheet/各 provider 专属 TerminalLauncher、Claude/Codex Status 与 Usage 工作台。
- **无运行时证据**：依任务禁令未 build/test/install/启动 App/网络；所有可达性与严重度为静态推理，任何"实测 149s"均为源码注释中的自述数字，不是本 worker 的测量。
- **T2 机械扫描只作定位**：sweep-agent-sessions.json 的 probe 统计（fs_watch=0、resume_launch=246 文件等）仅用于导航与交叉核对，不作为行级阅读凭证。
- **快照时点**：结论对应 commit af4793d317ab83e73ea205ff3f91feee32a3f0dd；七个被读文件在初读与交付前的 SHA256 两次核对均一致（见 coverage-agent-sessions-core.json）。