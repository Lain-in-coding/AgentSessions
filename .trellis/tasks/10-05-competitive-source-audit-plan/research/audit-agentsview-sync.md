# 竞品源码审计：agentsview 分片 B（sync 编排与 parser）

- 审计对象：`C:\AgentSessions\Github_src\agentsview`，commit `a84564ad6ea35f8fb80905da4693144f89f84229`（哈希与行数在本报告撰写前复核未变）。
- 任务：`.trellis/tasks/10-05-competitive-source-audit-plan`；日期 2026-10-06。
- 范围：T1 精读 `internal/sync/engine.go`（有界）、`internal/parser/codex.go`（全文 2343 行）、`internal/parser/claude.go`（全文 2283 行）；支撑性有界阅读 `internal/sync/parsediff.go`、`internal/sync/cwd_filter.go`、`internal/sync/watcher.go`、`internal/parser/provider.go`。机械扫描复用 `research/sweep-agentsview.json`（只做过滤查询，未整读）。
- 阅读凭证：`research/coverage-agentsview-sync.json`（逐文件 bytes/行数/SHA256/读区间/未读区间）。engine.go 为 **partial**：实际精读 5178 / 10245 行，未读区间在该 JSON 的 `missing_ranges` 完整列出；两个 parser 为 **full**（全部逻辑行有人工可见的编号窗口，虚拟 EOF 行不计）。
- 严重度口径：P1=核心正确性优先核查；P2=有条件的正确性/可用性/边界风险；P3=较低优先级或证据欠缺；I=已知产品取舍。**本报告没有确认到 P1 级数据丢失缺陷**；所有"删除"结论均为静态推导，未运行产品。

## 0. 覆盖统计

| 口径 | 数量 |
|---|---:|
| T1 文件 | 3（engine.go partial；codex.go/claude.go full） |
| T1 已精读逻辑行 | 5178 + 2343 + 2283 = 9804 |
| 支撑文件（有界） | 4（parsediff 200 行、cwd_filter 60 行、watcher 100 行、provider 241 行） |
| engine.go 未读区间 | 21 段 / 5067 行（见 coverage JSON） |
| 读取方法 | `codegraph node --file ... --offset N --limit M --path C:/AgentSessions/Github_src/agentsview`（编号窗口，每段 <=150 行）+ 对 engine.go 先做全文件 `Select-String` 符号/主题地图 |
| 复杂度指纹 | engine.go 10245 行/337 符号；syncAllLocked 约 304 行、resyncAllWithOptionsLocked 约 668 行；`engine_integration_test.go` 425KB、`engine_test.go` 188KB（静态尺寸，未运行） |

## 1. 同步编排与删除语义（必答 1）

### 1.1 编排入口与模式

- 入口全集（全部在 `syncMu` 下串行）：`SyncPaths/SyncPathsContext`（engine.go:592-656，watcher 驱动、只处理给定路径集）、`SyncAll`（2298-2319）、`SyncAllAfterWatcherOverflow`（2326-2346，先清空 skip/信任缓存再全量强制解析）、`SyncAllSince`（2355-2373，按 mtime 截断的快速同步）、`SyncRootsSince`（2378-2398，根范围）、`SyncSingleSession`（9550-9808）、`ResyncAll`/`ResyncAllWithOptions`（1231-1294）。
- 全量主循环 `syncAllLocked`（2465-2758）：provider 门面统一发现（2769-2893，含 VS Copilot 的 poll-tombstone 补充 2828-2835）→ 截断过滤与去重（2527-2554）→ 2..8 个 worker 解析（3900-3940）→ 单 goroutine 收集/批写（3946-4165）→ DB-backed provider 阶段（2675-2713）→ `LinkSubagentSessions`（2715-2722）→ skip cache 持久化（2724-2732）→ lastSync/lastSyncStats（2745-2750）→ 仅零发现失败时写 finish 水位（2752-2754）。
- 增量/全量：`SyncAllSince` 的 since 过滤只做发现后过滤（2348-2354 + 3154-3214）；"增量解析"另有一条 append-only JSONL 路径（Claude/Codex，5913-6152、5811-5895），失败即回退全量；其余 provider 以 `Fingerprint`+DB 尺寸/mtime/hash/data_version 做跳过判定（5475-5508、6199-6251）。
- 失败即中止的语义：ctx 取消或统计 Aborted 时 `syncAllLocked` 直接返回且**不更新** lastSync（2635-2644）；resync 另有一套更严格的换库守卫（见 1.3）。

### 1.2 源文件消失后发生什么（"扫描删数据"主问题）

结论先行：**常规全量/快速同步不会因为"某个源文件没被发现"而删除归档行**——这是 agent 会话归档的"非破坏重扫"设计。证据与边界如下。

| 场景 | 实际行为 | 证据 |
|---|---|---|
| 普通会话文件（Claude/Codex JSONL 等）被删除 | watcher 的 remove 事件在分类阶段被丢弃（`continue`），不进解析队列；全量同步只是不再发现它，已归档行保持原样 | engine.go:813-818（remove+非 regular+非物理容器→跳过）；engine.go:2465-2757 主循环只消费"发现的文件"，没有任何"未发现即删除"的清扫 |
| 全量重扫时的 presence 检查 | 只存在于 **parse-diff 报告引擎**（只读）：`parseDiffPresenceSweep` 把"stored 但本轮未 emit"的会话标为 DiffChanged/PendingResync 写进报告，从不落库 | parsediff.go:175-184、917-967；报告引擎拒绝一切写入口 engine.go:213-235 |
| 重扫/重建（ResyncAll） | 用临时新库全量重建后原子换库；**源文件已消失的会话通过 orphan copy 原样拷回**，拷贝失败则中止换库 | engine.go:1753-1782（CopyOrphanedDataFromExcluding，失败 abort）；1195-1229 换库守卫 |
| 源文件消失且该源是"整容器/成员"模型（Zed/ZCode/Shelley 物理 DB、成员墓碑） | **显式删除**：删除事件强制解析→解析器输出 SkipNoSession+ForceReplace→引擎把该源路径下已存会话 ID 交给 `DeleteParserExcludedSessions` 真正删行（"retire"） | engine.go:4461-4469（注释明言 retire every session）；4685-4701（SkipReason+ForceReplace+ResultSetComplete→providerSourceSessionIDsForForceReplace）；4016-4028（执行删除）；multi_session_container.go:542-566（成员墓碑 force-replace） |
| 同路径 ID 换代（单会话源，如 Vibe 的 meta.json ID 变化） | stale-row cleanup：旧 ID 行被加入 exclusion 后删除；有 resurrection guard（同路径存在 trashed/excluded 行时不写新行并保留旧状态） | engine.go:4879-4982；多会话源直接 early-return（4892-4894） |
| cwd 过滤（sync_include_cwd_prefixes）生效时 | "冻结"契约：某源没有任何在 allow-list 内的会话时，其 exclusion/清理名单被丢弃，归档行不删；防止过滤器把替换写 veto 掉后留下"被清空"的存档 | engine.go:4003-4029、9757-9767；cwd_filter.go:74-96 |
| VS Copilot（VS2026 poll 布局） | 存储在案但本轮未发现的会话可被生成"墓碑"强制删除——但仅在容器路径**不是 regular file**、其父目录与 root 均 `reachableDir`（真实存在且是目录）时才允许，避免目录不可达/未挂载时误删 | engine.go:2907-2915、2958-2962、2997-3015 |

**"扫描删数据"风险评估（P2，条件触发）**：删除面存在且是显式的，不是"全库 presence 清扫"。最需要盯的是三类触发：① 物理容器/成员的真实消失（含用户/同步工具误移走容器文件，只要目录仍可达就会被 retire）；② 解析器回归导致同名路径 ID 集合变化（stale-row cleanup 会删"不再 emit"的旧 ID）；③ 容器布局判定错误（`multiSessionSourceOwnsContainer` 边界）。反证/现有护栏：普通文件删除完全不删档；resync 有 orphan copy 兜底；cwd 过滤与 trashed/excluded 有冻结/复活护栏；墓碑路径有可达性检查；parse-diff 全链路只读。**未确认**存在"空目录/卸载目录→整库清空"的路径（read 范围内所有删除都经 per-source 或 per-container 定位）。

### 1.3 换库原子性与中止守卫

- `shouldAbortResyncSwap`（1195-1229）：取消、空发现（旧库有数据而本轮非容器发现为 0）、synced==0 且不属于"preserved-only/excluded-only/cwdFiltered-only"、以及 failed>ok 时都拒绝换库；contributor 另行评估后合并（1537-1607、1609-1641）。
- 换库序列（1900-1949）：`origDB.CloseConnections()` → copy 元数据（失败即 abort+重开）→ `os.Rename(temp, orig)` → reopen → `MarkDataCurrent` + WAL checkpoint。rename 成功后 reopen 失败会返回 Aborted 且不再回滚（1929-1939）——留下"磁盘已是新库、句柄未重开"的降级状态，属于可用性风险（P3），不是数据回滚丢失。
- 失败时不丢旧库：所有 fatal copy（orphan/recall/metadata/sync_state/insights/FTS）都走 `newDB.Close()+removeTempDB()+restoreSkipCache()+origDB.Reopen()` 的相同模式（1764-1781、1802-1818、1830-1845、1688-1704、1713-1729、1876-1893）。

## 2. Parser：身份 / 归一化 / 父子边 / 工具调用 / 半解析（必答 2）

### 2.1 codex.go（全文）

- 身份：`session_meta.payload.id` 优先，缺省用文件名 stem；统一前缀 `codex:`（1508-1515）。父边来自 `source.subagent.thread_spawn.parent_thread_id`，回退 `thread_source=subagent`+`parent_thread_id`，父 ID 规范化成 `codex:` 前缀、关系 `RelSubagent`（242-261）。子代理 ID 全库统一 `codex:<agentID>`（753-762）。
- 归一化：`response_item` 按 `type` 分派 function_call/function_call_output/agent_message/role user·assistant（278-345）；user 首条做系统噪音剥离（`# AGENTS.md`、`<environment_context>`、`<INSTRUCTIONS>`、`<skill>`、subagent 通知、`<turn_aborted>`、goal 上下文）（2208-2217、2223-2291）；`agent_message` 以 `recipient==agent_path` 识别入站消息（1333-1352）。token 归一把 Codex 的"含缓存 input"拆成 uncached input + cache_read，防止缓存部分被按全额计费（443-479）。
- 父子边/工具调用：`function_call` 建立 `callRefs[call_id]→(messageIndex,callIndex)`（481-523）；`function_call_output` 通过 callRefs 把结果事件挂回对应 tool call（534-599、657-694）；subagent 关联有两条来源——`collab_agent_spawn_end`/`sub_agent_activity` 事件与 `spawn_agent` 输出/`wait` 输出（416-441、549-578）；找不到 wait/spawn 的通知先入 `pendingAgentEvents`，收尾时冲洗：能归到 wait/spawn 就挂回调用，否则作为孤儿 user 通知消息插入并做去重（624-748）。`insertMessage` 会同步平移 callRefs 索引（776-798），最后 `normalizeOrdinals` 重排（764-774）。
- fork 重放门（#643）：`codexForkGate` 仅在 `forked_from_id` 存在且能用 uuidv7/payload 时间/信封时间锚定时激活；gate 内丢弃 replayed 父历史，直到遇到 mint 时间 >= fork 时刻的 `turn_context`（97-145）。**fail-open 设计**：无法锚定/无法解析 turn_id 时保留数据（119-121、136-139）。
- 标题/新鲜度：`session_index.jsonl` 的 mtime 折进有效 mtime（1523-1531），标题按 mtime+size+changeTime 缓存（1666-1708），rename 靠 index 标题对比触发重解析（6216-6259）。
- 半解析/失败：无效 JSON 行直接跳过（1477-1488）；读取限定在打开时的 size 快照（1467-1473）防并发追加混入；增量侧"OK 有效但无结尾换行"的 EOF 记录返回 `errCodexIncrementalNeedsFullParse`、绝不当作安全续点（1899-1966）；`consumed` 只推进到最后一个完整且合法的行（1951-1963、6053-6056）；未消费的尾巴 `noCacheSkip` 抑制跳过缓存，等写全后重试（6099-6104）。

### 2.2 claude.go（全文）

- 身份：会话 ID = 文件 stem（77）；`sessionId` 与文件名不同时视为父会话（233-241）；fork 会话 ID = `<原ID>-<首条uuid>`（1071-1077）；agent 标签/entrypoint 采用"首个非空即锁定"（127-144、584-597）。格式化 prompt 后的 `firstMessage/userCount` 跳过 command/系统注入（2127-2154）；纯 `/usage` 探针会话整条排除，ID 进 `ExcludedSessionIDs` 供 resync 孤儿保护使用（348-361、2156-2175）。
- 父子边/DAG：构建 uuid→children 邻接，校验"恰一个根且所有 parentUuid 可解析"才走 DAG，否则回退线性（935-979）；主分支优先，分叉处按"first child 子树 user turn 数"决定：<=3（forkThreshold）视为小间隔重试、跟随最后子节点；>3 视为大间隔 fork，其余子分支各拆为独立会话（`RelFork`，父指向拥有者）（997-1103）。多次重试/嵌套分叉的分支归属是启发式，不是硬保证。
- 工具调用/子代理：user 行的 `tool_result` 用 `ExtractTextContent` 归一；`toolUseResult.agentId` 建立 tool_use→`agent-*` 映射（1105-1155、1859-1883）；queue-operation/`progress.agent_progress` 两种历史形态都可补映射（157-192）；持久化大结果（persisted-output）在读取时按安全目录白名单回读并入 tool_result 内容（1534-1661）。
- 归一化细节：同一 `message.id` 的连续 assistant 记录做"累积快照/增量块"合并（先按块类型/tool_use id/文本前缀对齐，`end_turn` 后不再前缀合并）（1283-1497）；compact summary 独立成 `system/compact_boundary` 消息（752-774、2192-2209）；`isMeta` 跳过、系统注入消息升格为 `IsSystem`+subtype（continuation/resume/interrupted/task_notification/stop_hook/system_reminder）并保持 Role=user 以免污染按角色统计（776-831、2211-2283）。
- 半解析/失败：坏行计 `MalformedLines`（119-123）；"最后一行非空、非法 JSON、且文件无结尾换行"判定为截断并传递到终止分类（269-276、340-346）；增量解析遇到 fork/同 message.id 分块/跨增量 tool_result 未配对/queue 映射补旧行等 4 类情况一律返回"需要全量解析"（527-559、603-655、677-730）；`ErrClaudeIncrementalNeedsFullParse` 与 Codex 的同类错误共享回退入口（codex.go:2201-2206）。

### 2.3 provider 解析器清单（provider.go）

- 注册表驱动：`ProviderFactories()` 遍历 `Registry`（402-408），`providerFactoryForDef` 显式 switch **53 个 case**（413-521）+ 2 个 import-only（ClaudeAI/ChatGPT，425-426、439-440）。清单（按 switch 顺序）：antigravity、antigravity-cli、aider、amp、claude、openclaude、claude-ai(import)、commandcode、codex、copilot、cowork、cortex、cursor、chatgpt(import)、deepseek-tui、forge、devin、hermes、grok、iflow、gptme、gemini、kimi、kiro、kiro-ide、kilo、mimocode、icodemate、openhands、opencode、omp(pi 工厂)、openclaw、piebald、pi、positron、posit-assistant、qclaw、qwen、qwenpaw、qoder、reasonix、shelley、vs-copilot、vscode-copilot、windsurf、trae、vibe、zcode、warp、workbuddy、zencoder、zed、roocode。
- 契约（ParseOutcome，283-349）：`Results/ExcludedSessionIDs/SourceErrors/ResultSetComplete/ForceReplace/SkipReason` + 每结果 `DataVersionState`（Current/NeedsRetry）与 `RetryReason`。这套"完整集/排除集/按结果降版"是引擎删除与重试语义的输入契约——**ASG 目前没有等价的一等契约**。

## 3. 新鲜度 / 水位 / 水位回退 / 并发写与锁 / 失败分类与重试（必答 3）

### 3.1 新鲜度与水位的真实形态

- 水位有两层：① 内存 `lastSync/lastSyncStats`（524-535）；② 持久化 `pg_sync_state.started_at/finished_at`（3126-3147，ephemeral 引擎不写，83-84）。快速同步的 since 由调用方基于"上次同步开始时间"减安全边界计算（`LastSyncStartedAt` 2053-2063；2348-2354 注释），**不是**单调递增的严格水位。
- 跳过缓存（skip cache）= 持久化在 `skipped_files` 的 path→mtime 映射（NewEngine 258-268 加载；persistSkipCache 5441-5453）；用途是"上次故意跳过/永久失败"的文件不再解析；文件 mtime 变化即失效。Codex/Claude 另存内容 hash 用于 same-mtime 兜底（5054-5073、6058-6067）。
- 水位回退：没有增量水位可回退（每次同步都是对当前磁盘再发现）。与"回退"最接近的三个机制：a) 取消/中止时**不更新** lastSync（2641-2644）；b) watcher overflow 时清空 skip/信任缓存并提升为 FullSync（watcher.go:37-55、69-85；engine.go:2326-2345、5385-5400）——宁可重扫也不漏；c) resync 失败时恢复内存 skip 快照（1385-1400）并把临时库整体丢弃（1618-1641、1764-1781 等）。
- 多级信任闸门：OpenCode 容器 gate、storage gate、verified source gate 均是**内存态**，重启后必然重新深验一次（170-205）；写失败会"毒化"容器 pass、阻止本轮信任提升（4111-4125）。

### 3.2 并发与锁

- 全局 `syncMu`：所有同步/重扫/单会话重同步/secret 扫描读改写路径串行（112、613-624、2304、2332、2361、2385、1237、1282、9559、10225）；Emitter 回调在锁释放后触发（2308-2313）。`mu` 守护 lastSync/progress；`skipMu` 守护 skip cache；容器/storage/verified 三套 gate 各有独立互斥（170-205）；信号调度器的延迟 flush 也取 `syncMu`（324-328）。
- 解析并发度 = min(max(NumCPU,2),8) 个 worker，通道缓冲 workers*2（3900-3908）；**写库只有收集协程单写者**（3946-4165）。好处：状态机简单、无 SQLite 多写者竞争；代价：大库解析期间写入端吞吐受单收集器限制（P3 性能取舍，I）。

### 3.3 失败分类与重试

- 分类由 `processResult` 的字段隐式表达（4209-4257）：`skip`（合法跳过）、`err`+`cacheSkip`+`noCacheSkip`（错误及其可否缓存）、`needsRetry/retrySessionIDs`（按会话降版重试）、`forceReplace`、`suppressPresenceSweep`、`sessionErrs`（多会话源单会话错误）。
- 瞬时错误不落 skip cache：读/扫描失败、未完成 append 边界、解析/校验错误都置 `noCacheSkip=true`（4227-4232；4660-4682 两处 return），等下轮重试；持久缓存只覆盖"合法跳过/确定性失败"（3985-3990、4030-4041）。
- 按结果降级重试：`DataVersionNeedsRetry` → `data_version = Current-1` 写入，内容先可见、下一轮再取高分辨率源（4722-4728、6847-6858、4984-4997 的 clean-skip 抑制）。
- 重试调度：没有进程内退避/重试计数/告警面；"重试"= 下一次同步（watcher 事件、周期任务或手动）。发现层 failure 会被计入 stats 并保守阻止容器信任提升（2608-2624）。**P3/弱项**：缺少 retry 计数、dead-letter 与可观测的重试原因（RetryReason 有字段但引擎未在报告面呈现——未读区间 6260-6839 未排除该可能）。

## 4. 与 ASG 对照（必答 4）

对照基线：`research/asg-main.md`（2026-10-05，HEAD 5b232cd）与相关 ASG 报告；本分片未重审 ASG 源码，结论只用于方向性取舍。

### agentsview 强在哪（值得学）

1. **"归档不因扫描而删"的默认值**：全量/快速同步只做增量发现，presence 漂移只在只读 parse-diff 报告（parsediff.go:917-967；engine.go:2465-2757）。ASG 已有源快照+fingerprint 前后验证（asg-main.md 记录的 source_fs 路径），但可以再补一条"未发现≠删除"的显式契约测试。
2. **显式的排除/删除契约**：ParseOutcome 的 `ExcludedSessionIDs/ResultSetComplete/ForceReplace/SkipReason` 让"删什么、为什么删"在类型上可审计（provider.go:297-312；engine.go:4003-4029）；ASG 的删除/可见性目前更偏查询侧，摄取侧契约可以对齐这一思路。
3. **按结果降版重试**（4722-4728、6847-6858）：坏的新解析不遮蔽可用的旧内容，同时强制下轮再试——适合 ASG 的 parser 版本演进场景。
4. **瞬时错误不缓存**（noCacheSkip，4227-4232）：把"失败"与"负缓存"分离，避免一次临时 IO 错误让文件永久不解析。
5. **重扫的中止守卫 + 孤儿回拷**（1195-1229、1753-1782）：ASG 若要引入 rebuild/resync，应原样学习"可证明更差的库拒绝换库"和"源已消失的存档必须保留"。
6. **watcher overflow 降级为 FullSync 而非丢事件**（watcher.go:37-55；engine.go:2326-2345），且批集有双上限（8192 条/2MiB）。

### agentsview 弱在哪（ASG 不该学）

1. **单文件 1 万行的编排器**：engine.go 承载发现/跳过/新鲜度/写批/重扫/身份/secret 多职责，巨型函数（304/668 行）使审读与回归面巨大；ASG 现有"按职责拆模块 + 端口契约"的方向应坚持（asg-main.md 的 ASG-06 也点出 1.9 万行单文件的维护成本，属同类教训）。
2. **启发式删除**：容器墓碑与 ID 换代删除是有护栏的，但语义上仍是"扫描结果反推删除"。ASG 更稳妥的默认是**永不删除**：标记 absent/epoch，显式 prune/GC 命令才允许删（agentsview 的 parser-excluded 删除即"隐式删除"类）。若学，必须同时学 reachableDir 式的可达性闸门与 cwd 冻结契约（engine.go:2997-3015；cwd_filter.go:74-96）。
3. **无退避/无重试预算的"下次同步再试"**：低频周期同步足够，但 ASG 若面向交互式/服务化场景需要 retry 计数、上限与可观测原因。
4. **53 家 provider 的枚举 switch + 每家特判**：广度换来的是 engine/parser 里持续的 provider 分支和兼容 shim；ASG 若要扩 provider，宜用"能力声明 + 多会话/单会话/容器 source-set 抽象"的注册表（agentsview 的 provider.go 契约值得学，但不应复制 53 case 的巨型 switch 与散落特判）。
5. **swap 后 reopen 失败不回滚**（1929-1939）：重扫型机制要把"已换库但服务未恢复"当作独立可重入状态处理。

### ASG 已有的、对照后应保留的强项

以 asg-main.md 记录为准：源读取的范围+fingerprint 前后验证、语义过滤/删除可见性/坏向量 fail-closed、稳定 Message/Placement/relocation 契约、五界面共享 Application 契约（ASG-01/04/05 段）。这些是 agentsview 在本分片范围内未见等价物的方向（agentsview 强在广度与新鲜度工程，不在证据/引用契约层）。

## 5. Findings 清单（含反证）

| ID | 严重度 | 结论 | 证据锚点 | 反证/边界 |
|---|---|---|---|---|
| AGV-B-01 | P2 | 源"整容器/成员"消失会触发显式删行（retire/tombstone），是本分片确认的唯一"扫描可导致删除"路径族 | engine.go:4461-4469、4540-4548、4685-4701、4016-4028；multi_session_container.go:542-566；engine.go:813-818、2958-2962、2997-3015 | 普通文件删除不删档（813-818）；resync orphan copy 兜底（1753-1782）；墓碑有目录可达性检查（2997-3015）；注释与 ForceReplace 语义在 multi_session_container.go:542-566 存在张力，需动态测试钉死行为 |
| AGV-B-02 | P2 | 单会话源同路径 ID 换代时删除旧 ID 行（stale-row cleanup） | engine.go:4879-4982 | resurrection guard 保留 trashed/excluded（4935-4969）；多会话源跳过（4892-4894）；cwd 过滤冻结（4003-4015、cwd_filter.go:74-96） |
| AGV-B-03 | P1-正面 | 全量扫描不会因"未发现"删档；presence 只读报告；重扫以 orphan copy 保档 | engine.go:2465-2757（无未发现清扫；grep 全文件无 DeleteSessions 类调用）、213-235、1753-1782；parsediff.go:917-967 | 例外见 B-01/B-02；resync 会重建库，但 abort 守卫+orphan copy 使净效果为保档（1195-1229） |
| AGV-B-04 | P2 | 失败重试无退避/上限/告警面，重试时机依赖下一次同步；RetryReason 未在引擎报告面消费 | engine.go:3985-3991、4227-4232、4660-4682、4722-4728、6847-6858；2641-2644 | 瞬时错误 noCacheSkip 已保证不落负缓存；按结果降版保证内容可见；发现失败阻止信任提升（2608-2624） |
| AGV-B-05 | P2 | resync 换库后的 reopen 失败不回滚（磁盘已是新库），只报 Aborted | engine.go:1906-1949 | swap 前所有 fatal copy 均 abort+重开（1764-1893）；此场景是可用性降级而非数据丢失 |
| AGV-B-06 | P3 | codex fork gate fail-open：无法锚定 fork 时刻/无法解析 turn_id 时保留重放历史，可能重复计 usage；反向则可能丢 live 数据 | codex.go:97-145、1769-1851 | 仅 `forked_from_id` 存在且能锚定时才激活；fail-open 是避免丢数据的显式取舍（95-96、119-121） |
| AGV-B-07 | P3 | claude fork 拆分用阈值启发式（forkThreshold=3、小间隔跟随最后 child），分支归属变化会造成 ID/presence 漂移 | claude.go:997-1103、527-559 | 结构校验失败即回退线性（962-979）；漂移会被 parse-diff presence 报告捕获而非静默丢失 |
| AGV-B-08 | P3 | watcher overflow 以 FullSync 兜底并清空信任缓存：不漏事件，但极端风暴会放大为全量重验 | watcher.go:29-55、69-85；engine.go:2326-2345、5385-5400 | 常规事件按路径批处理；批集有 8192/2MiB 双上限；清除后的缓存会在下次全量重验中重建（170-205） |
| AGV-B-09 | P3 | engine.go 巨单文件/巨函数 + 53 case provider switch 是长线维护成本 | engine.go:10245 行（syncAllLocked 2465-2757、resyncAllWithOptionsLocked 1296-1963）；provider.go:412-521 | 测试体量极大（engine_integration_test.go 425KB）部分对冲；不代表当前行为不正确 |
| AGV-B-10 | P3 | 覆盖限制：engine.go 未读 5067 行，其中 6260-6839（codex index/身份辅助）、7159-8268（写批前处理/信号）、3222-3899（发现辅助）等区间仅凭地图推断 | coverage JSON missing_ranges | 删除相关调用点已用全文件 grep 覆盖（e.db.* 调用清单）；未读区间仍可能含与本报告结论无关的行为，如需钉死需追加阅读 |

## 6. 残余未决

1. `multi_session_container.go:542-566` 的注释（"whole-container ... preserve the stored sessions"）与 `ForceReplace:true` 实际会走引擎删除路径之间的张力——需要行为测试（删掉 threads.db 后跑一次全量同步，观察会话行是否消失）才能定论；本分片只有静态证据。
2. engine.go 未读区间含 provider 新鲜度辅助（6260-6839）、写批前处理与信号（7159-8268）、发现辅助（3222-3899）等，未逐段排除"额外删除/回滚入口"；已用全文件 `e.db.*` 调用清单降低风险，但不等于全文。
3. watcher.go 101-544 未读（事件风暴/去抖/周期触发细节），周期任务的 since 边界只能引用 engine 侧注释。
4. Zed/Shelley/ZCode 的成员布局（多行 vs 每文件一成员）细节在 zed_provider/shelley_provider 等文件，本分片未精读；B-01 的"整容器 vs 成员"分支依赖 multi_session_container 的通用实现推断。
5. 全部结论为静态审读；未 build/test/运行，哈希仅在阅读前后复核（coverage JSON `content_unchanged: true`）。