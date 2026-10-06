# 竞品源码审计：Wake（0.8.5 快照）

- 任务目录：`.trellis/tasks/10-05-competitive-source-audit-plan`
- 审计对象：`C:\AgentSessions\Github_src\Wake`，commit `71aeca67ec80f8645d1f9d5199290c2c732036ce`（`chore: release Wake 0.8.5`）
- 日期：2026-10-06
- 方法：PowerShell 带行号逐段阅读（每次 ≤150 行）；无 build/test/install/网络/git 写操作；Wake 仓库无 `.codegraph` 索引，未使用 CodeGraph，hash 未当作阅读。
- 凭证：同目录 `coverage-Wake.json`（每文件 bytes/行数/sha256/实际阅读范围/缺失范围）。本报告锚点均为相对 `C:\AgentSessions\Github_src\Wake\` 的路径。

## 0. 覆盖统计

| 文件 | 行数 | 状态 | 阅读行数 |
|---|---:|---|---:|
| crates/wake-core/src/scanner.rs | 1170 | full | 1170 |
| crates/wake-core/src/adapters/mod.rs | 1371 | full | 1371 |
| crates/wake-core/src/adapters/codex.rs | 1368 | full | 1368 |
| crates/wake-core/src/models.rs | 1035 | full | 1035 |
| crates/wake-core/src/mcp/tools.rs | 1424 | full | 1424 |
| crates/wake-core/src/cli.rs | 1407 | full | 1407 |
| crates/wake-core/src/db.rs | 3408 | partial | 1056 |
| crates/wake-core/src/adapters/claude.rs | 811 | full | 811 |
| crates/wake-core/src/adapters/opencode.rs | 804 | partial | 146 |
| crates/wake-core/src/watcher.rs | 396 | full（补充） | 396 |
| crates/wake-core/src/adapters/remote.rs | 237 | full（补充） | 237 |
| crates/wake-core/src/remote.rs | 734 | partial（补充） | 361 |
| crates/wake-core/src/cleanup.rs | 2349 | partial（补充） | 584 |

合计 13 个文件、11366 行实际阅读。T1 指定的六个核心文件（scanner/adapters-mod/codex/models/mcp-tools/cli）全部逐行读完；db.rs 按 FTS5/LIKE/删除安全主题做有界精读；claude.rs 为可选全文阅读；opencode.rs 为可选有界阅读。watcher.rs、adapters/remote.rs、remote.rs、cleanup.rs 为支撑“监视/远程/cleanup 安全性”问题的补充阅读。所有内容在阅读前后哈希一致（见 coverage-Wake.json 的 sha256_initial/sha256_final/content_unchanged）。

## 1. FTS5 trigram 与短词/LIKE 退化的真实语义与性能边界

### 1.1 结构

- 三张 FTS5 虚表，统一 `tokenize="trigram case_sensitive 0"`：`messages_fts`（external content，`content='messages'`、`content_rowid='id'`）`db.rs:65-69`；`titles_fts` `db.rs:128-132`；`memories_fts` `db.rs:168-172`。
- 检索词解析：空白切分（`fts_terms`）`db.rs:3255-3257`；每词双引号转义（内部 `"` 翻倍）后 AND 连接（`fts_match_expr`）`db.rs:3264-3271`。用户输入不可能注入 FTS5 布尔/前缀语法。
- 派生规则版本 `FTS_FORMAT = "8"` `db.rs:199-211`；换代时置 `fts_reindex` 旗并强制下一轮全量重解析 `db.rs:251-272`，scanner 消费并在一轮后清旗 `scanner.rs:328-333, 625-630`。

### 1.2 真实语义（CJK / 子串 / 大小写）

- trigram 分词把正文切成连续 3 码点单元；**≥3 码点的查询词在 MATCH 下等价于任意位置子串命中**，大小写不敏感（`case_sensitive 0`）。CJK 天然可用（汉字逐码点参与三元组）；代码子串如 `useEffect(` 同样成立——这是 MCP 工具描述对 agent 的公开承诺（`mcp/tools.rs:157, 161`）。
- 短词规则是**整条查询降级**：只要任意词项 <3 码点（`needs_like_fallback` 按 `chars().count()` 判定，`db.rs:3259-3262`），`degraded=true`，正文/标题/记忆三条检索全部改走 LIKE（`search_with` `db.rs:2570-2571, 2618-2688, 2694-2736`；`search_memories` `db.rs:2220-2267`）。
- LIKE 路径语义：每词每列一个 `LIKE ? ESCAPE '\'`（列间 OR、词间 AND）`db.rs:3275-3289`；参数为 `%term%` 且对 `\ % _` 转义 `db.rs:3248-3252, 3291-3300`。snippet 由 Rust 逐字符大小写折叠加字符下标生成（`make_like_snippet` `db.rs:3302-3345`；注释记录了 2026-09-21 修复的字节/字符偏移 panic——⌘K 逐字输入 CJK 曾经以此路径 abort 整个 app）。
- 排序差异：FTS 路径按 bm25 × 近因加权（`RECENCY_BOOST`：今天 ×2、约 30 天前 ×1.5、一年前 ×1.08）`db.rs:2572-2578, 2626, 2700`；LIKE 路径按时间倒序（正文 `m.ts DESC` `db.rs:2659`；标题/记忆 `updated_at DESC` `db.rs:2723, 2254`）。
- degraded 标志一路传回 MCP 并在结果里明说“terms shorter than 3 characters use a slower substring scan”（`mcp/tools.rs:632-640, 658-660, 679-681`）。

### 1.3 是否“总有全表 LIKE 兜底”？

不是，两个方向不对称：

- **短词 → LIKE：是**。存在 <3 码点词项即触发，且是整条查询一起退化为全表扫描：messages 全表 + sessions 连接（`db.rs:2655-2660`）、sessions 全表（标题 `db.rs:2720-2723`）、memories 全表（`db.rs:2251-2254`）。
- **FTS 零命中 → LIKE：否**。模式在查询前按词长一次性决定（`db.rs:2570, 2618-2653`）；trigram MATCH 返回空行集时没有第二段 LIKE 重试、也没有针对性的“换词”提示；只有“词项本身 <3”才带降级提示。代价：≥3 码点查询在索引陈旧/重索引未生效的窗口内会静默返回零结果。

### 1.4 性能边界

- LIKE 是 `%...%` 中缀匹配，无法走索引；源码注释直言 messages 表规模“can contain millions of rows”（`db.rs:1121-1122`）。最坏成本约为 O(行数 × 文本长度)，且在标题、记忆路径上同样成立。
- 触发面比“英文短词”宽得多：所有 1–2 码点查询都触发，包括**常见 CJK 双字词**（“搜索”“登录”“报错”……）。CJK 用户的日常查询大量落在此路径。
- degraded 路径无 bm25/近因加权，排序与 FTS 路径不同，同一查询在不同词长组合下结果排序会变化（语义一致性边界）。
- 3+ 码点查询走 trigram MATCH（索引检索）；且 agent/项目/since 过滤条件拼进同一 SQL 的 WHERE，在 LIMIT 之前生效（`db.rs:2583-2598, 2624-2636`）——不存在 sessiongrep SG-01 式“先截断候选再过滤”的错失命中。
## 2. 会话发现 / 扫描 / 监视 / 新鲜度 / 降级；sidecar 与 remote

### 2.1 发现（roster 与数据根）

- 21 家 agent（`models.rs:7-57`），构造 23+ 个实例（`adapters/mod.rs:293-318`：含 Cursor 双源、Pi + omp、Codebuddy + workbuddy 等多源）。
- `data_roots()` 是路径唯一真源（`adapters/mod.rs:166-171`）；`watch_paths()` 默认由数据根中现存目录派生（`adapters/mod.rs:172-182`）；env 根（`CODEX_HOME` / `WAKE_HOME`）只当**候选**，探到真实数据才采信（`adapters/mod.rs:258-279`；`adapters/codex.rs:36-38` 用 sessions/archived 存在性过滤）。
- 真实索引库入口必须走 `create_adapter_roster_for`（`adapters/mod.rs:398-459`）：默认实例 + 自定义 location（含单根裁撤）+ 停用过滤 + 远程装饰实例追加尾部；注释记录 scan bin 曾用默认 roster 让删除检测误删自定义根（`adapters/mod.rs:452-456`）。
- `list_session_files` 契约：枚举必须廉价（多数纯 stat、SQLite 型只跑元数据查询），故障**就地降级为空列表**，绝不外溢炸整轮（`adapters/mod.rs:53-56`）。
- 归属裁决：路径归最长数据根（`path_owns`/`adapter_ix_for` `adapters/mod.rs:461-517`）；scanner 对跨家重叠根同样取最长根（`scanner.rs:345-362`）；watcher 同判据（`watcher.rs:40-52`）。
- 墓碑双轨（file_path + 逻辑 key）在枚举与写入两处过滤，防已删会话经副本复活（`scanner.rs:373-382, 542-545, 1101-1111`）。

### 2.2 扫描与新鲜度

- 并发闸门：进程内 `SCAN_GATE`（`scanner.rs:56-64, 271-273`）+ 进程间 `db::IndexLock`（`scanner.rs:143-146, 191-194`）；GUI 全程持锁，CLI 写命令干活时持锁，互不替代。
- 首次建库走 staging + `hard_link` 占位（`scanner.rs:121-170`）：中途死掉不留半截索引，已存在即退让（Skipped: Exists）；refresh 只对已存在且通过 `is_wake_index` 的库工作（`scanner.rs:182-200`）。
- 增量判据：`(mtime, size)` 与库内 known_files 相等即跳过（`scanner.rs:461-464, 485-488`）；`FTS_FORMAT` 换代与 Grok 回填用强制旗覆盖一轮（`scanner.rs:328-333, 441-442`）。
- sidecar 刷新**独立于** mtime 门槛：`update_sidecar_meta` + adapter `sidecar_updates`（`scanner.rs:447-451`；契约注释 “sidecar can be written after the transcript's final write” `adapters/mod.rs:70-79`；Codex state DB 标题经 quick_meta/merge_quick_meta 覆盖 `adapters/codex.rs:1080-1150, 1195-1214`）。
- 删除检测：库里 known 但本轮未枚举到 → `remove_session`（`scanner.rs:508-515`）；镜像路径不在库旁时整轮退让（`mirrors_elsewhere` `scanner.rs:202-223`），避免远程会话被误删。
- 解析失败：保留 last-good、不删行；胜者副本坏了按 (dedup_rank, mtime, path) 顺位回退下一份副本（`scanner.rs:389-423, 561-605`）；副本裁决在写事务内进行（`write_session_guarded`）。
- 降级纪律（系统性强项）：`None` = 这一刻读不出来 vs `Some(空)` = 确定没有——父边（`adapters/codex.rs:273-299`；`adapters/mod.rs:151-158`）、认领（`adapters/mod.rs:225-235`）、记忆来源（`scanner.rs:642-755`）全部遵守“读不出就冻结库内旧值，决不当删除”；单来源失败只冻结该来源（`scanner.rs:699-730`）。
- 记忆同步：按 (agent, host) 整组替换；失败冻结、停用即出库、消失即出库（`scanner.rs:733-754`）。

### 2.3 监视

- `SessionWatcher`：notify 递归监听 watch_paths，800ms 去抖批处理，drop 时先撤事件源再 join 线程（`watcher.rs:12-38, 202-256`）；`watched_roots` 是“真正挂上的根”的唯一真话（`watcher.rs:22-29`）。
- 事件种类不作数，按处理时刻现状裁决：文件在 → 增量重解析，不在 → `remove_by_path` + 幸存副本上位（`watcher.rs:114-171`；`promote_survivors` `watcher.rs:54-112`，按 native_id 反查，超 32 个才放弃解析比对）。
- 快照事件（关系边车/认领首行）：写库前 `refresh_claims`、写库后 `refresh_parent_links`（`watcher.rs:172-196`）。
- 后端丢事件（FSEvents MustScanSubDirs / inotify 溢出）→ `on_rescan_needed` 由 GUI 排一轮增量兜底（`watcher.rs:138-144, 197-199`；`scanner.rs:21-24`）。
- 测试固化：unlink→rename 原子替换按修改处理（`watcher.rs:316-339`）、改名走掉的旧路径出库（`watcher.rs:341-352`）、认领随删除撤销（`watcher.rs:354-371`）、drop 释放 Arc（`watcher.rs:373-395`）。

### 2.4 sidecar / remote

- sidecar：见 2.2；SQLite 型侧档“库在但打不开/读一半出错”返回 None（不缓存、下次重试）——`adapters/codex.rs:961-976, 1170-1193` 注释与实现（`read_spawn_edges` `adapters/codex.rs:281-299`）。
- remote 同步 = 镜像到 `<db 目录>/remotes/<host>/`：ssh 白名单探测后 rsync 增量镜像（`remote.rs:3-27, 292-300, 327-341, 351-379`）；探测失败整台不变量、缓存一字不动（`remote.rs:405-407`）；远端整项消失才本地清理（`remote.rs:417-423, 435-460`）；rsync exit 23 且错误行全为 ENOENT 时重探一轮（`remote.rs:480-506`）；host 名禁以 `-` 开头防参数注入（`remote.rs:260-267`）；删除 host 后孤儿缓存幂等清理（`remote.rs:270-290`）。
- `RemoteAdapter` 装饰器在每个 SessionMeta 出口插入 host 段并写 `meta.host`（`adapters/remote.rs:33-50, 95-131, 152-168, 178-186`）；远程会话 cleanup 明确只读（`cleanup.rs:404-406`；`adapters/remote.rs:141-145`）；scanner 对远程删除有镜像存在性保护（`scanner.rs:204-223`）。
- 远程限制（源码自述 stage-1）：只支持 Linux/macOS 默认布局（`remote.rs:52-54`）；Cursor IDE 库（路径含空格）不在同步名单（`remote.rs:94-101`）；同步由 GUI 线程发起，MCP/CLI 不触发（`remote.rs:385-403`）。
## 3. MCP/CLI 接口面与 agent 契约；cleanup 的安全性

### 3.1 MCP 面（mcp/tools.rs）

- 五个只读工具（`mcp/tools.rs:97-110, 152-235`）：`wake_search` / `wake_list_sessions` / `wake_get_session` / `wake_list_projects` / `wake_list_memories`；annotations 全为 readOnlyHint=true、destructiveHint=false、idempotentHint=true、openWorldHint=false（`mcp/tools.rs:120-127`），单测卡住（`mcp/tools.rs:1415-1422`）。
- 输出为面向 LLM 的 Markdown 文本（文件头注释明确不做 structuredContent）；错误分两类：形状错误 → JSON-RPC -32602（InvalidParams），执行失败 → isError 结果；“没匹配/没结果”不是错误（`mcp/tools.rs:1-6, 422-438`）。
- 参数上限：search limit≤30、sessions≤100、projects≤200、memories≤100（`mcp/tools.rs:112-117`）；get_session 分页 from_seq / max_messages≤200 / max_chars≤100k / max_message_chars / include_tools / include_thinking / subagent（`mcp/tools.rs:190-202, 1116-1132`）。
- 引用契约：`wake://session/<key>#<seq>`、`wake://memory/<key>`（`mcp/tools.rs:570-607`）；key 形如 `{agent}:{id}` 或 `{agent}:{host}:{id}`（`models.rs:970-993`），同 UUID 跨 host 用 `find_by_native_id` 反查并在多义时报错（`mcp/tools.rs:994-1014`）。
- 会话树契约：`wake_list_sessions` 只给根（roots_only，`mcp/tools.rs:902-924`），子会话在父会话 get_session 页脚列出（`mcp/tools.rs:1022-1067, 1271-1289`）；子代理转录按 id 读且校验 id 不含路径分隔符 / `.` / `..`（`mcp/tools.rs:1161-1168`）。
- 新鲜度提示：结果尾部带 `index_note`（“Index covers activity up to …；Wake keeps it fresh while it is running” / 空索引提示 / 未知原因）（`mcp/tools.rs:471-480` 及 665、716 等调用点）。
- TranscriptCache：单槽、键=路径+mtime+size+索引项目/模型；SQLite 虚拟路径用 db+wal mtime（`mcp/tools.rs:27-80`）。`get_session` 是**现场重解析**（parse_transcript），索引只服务检索/列表；seq 是搜索与详情的对齐锚（`adapters/mod.rs:37-38` 契约注释）。
- 记忆面：list/search/get 三处共用同一套 filter；读取时文件型先读磁盘、失败回退库内 body（`adapters/mod.rs:1087-1094`；`mcp/tools.rs:851-899`）。

### 3.2 CLI 面（cli.rs）

- CLI 是 argv → 一次 MCP 工具调用的薄层（`cli.rs:1-10`）；值一律原样字符串，数字解析/裁剪/agent 校验只在 tools.rs 一处（`cli.rs:1-7, 505-520`），保证 CLI 与 MCP 两条路同一句话、输出可逐字节相同（注释指 tests/cli.rs 卡着）。
- 命令表 `COMMANDS` 与 `tools::definitions()` 必须双射，单测卡死（`cli.rs:220-256, 959-1012`）；`setup/index/refresh` 为不收参数的原生命令（`cli.rs:101-117`）。
- `index`/`refresh` 是仅有的写命令：index 只在库不存在时建；refresh 只在 Wake 未运行时增量（`cli.rs:88-117, 265-268`；help 609, 653）。
- 退出码/流契约：0=跑完（含 no match）、1=会话读不出（Failed/Internal）、2=命令行错/索引缺失或过旧（`cli.rs:325-348, 648-652`）；BrokenPipe 必须冒出为 Err 由 bin 分类（`cli.rs:350-359, 1307-1335`）。
- agent 契约文本：AGENT_MEMO（`cli.rs:766-781`，教 agent 用 sessions/search/show/memories 且始终 `--project "$PWD"`）、skill 安装一行（`cli.rs:786`）、跨 agent handoff 复制文本（`cli.rs:799-818`）。

### 3.3 cleanup 安全性（cleanup.rs 有界精读）

- 评审阶段：目标必须是绝对路径且 canonical 等价——**祖辈含链接也拒绝**（`cleanup.rs:357-362`）；禁符号链接/非常规文件（`cleanup.rs:316-330`）；Unix 拒硬链接 nlink!=1（`cleanup.rs:332-343`）；adapter 必须本地、启用、`cleanup_paths` opt-in、路径在启用数据根内（`cleanup.rs:400-418`）；清理目标不得覆盖未选中会话或共享边车（`cleanup.rs:477-492`）；会话树环检测（`cleanup.rs:436-441`）；每个路径过 `validate_trash_path`（`cleanup.rs:449, 457`）。
- 执行阶段：save journal（`cleanup.rs:829`）→ 每目标 revalidate（会话签名 + 文件快照 + parse unknown==0，`cleanup.rs:653-710`）→ 计算并持久化 sha256 内容指纹（`cleanup.rs:841-856, 900-930`，注释“Recovery evidence must be durable before the first move”）→ 逐个移入系统废纸篓并复核路径已消失（`cleanup.rs:857-871`）→ 全部 Moved 才做索引侧原子删除 `complete_cleanup`（`cleanup.rs:883-892`；`db.rs:967-1032` 事务内删行+写墓碑，失败整体回滚）。
- 恢复阶段：用户先用 OS 恢复文件；validate 拒绝与该批 timestamp 不同的**更新**墓碑（`db.rs:1145-1169`）；逐文件类型/逻辑大小/内容 sha256 校验（legacy journal 用 type+length+mtime），拒绝验证期间再变更（`cleanup.rs:932-978`）；会话重解析且 id 必须一致（`cleanup.rs:997-1004`）；只删本批墓碑（`db.rs:1155-1169`）。
- 失败面：journal 每步落盘，部分移动失败留在 journal、可重试索引更新（`cleanup.rs:782-808`）；删除的会话树含子会话整体纳入（`cleanup.rs:432-445`）；远程/无 opt-in 源直接不可清理（`cleanup.rs:400-418`）。
## 4. 与 ASG 的对照结论

### 4.1 Wake 强在哪（ASG 该学）

1. **过滤先于截断**：agent/项目/since 全部在 FTS SQL 的 WHERE 内，先过滤再排序与 LIMIT（`db.rs:2583-2598, 2624-2636, 2698-2710`）。sessiongrep 的 SG-01（先取全局候选再过滤）在 Wake 不存在。
2. **降级要明说**：短词降级不是静默切换——结果文本显式注明“slower substring scan”（`mcp/tools.rs:658-660, 679-681`），agent 可据此换词。
3. **None ≠ 空**：所有“快照型”数据（父边、认领、记忆来源、SQLite 记忆库）都以 None 表示“读不出来”，scanner 一律冻结旧值而非整组清空（`adapters/codex.rs:273-299`；`adapters/mod.rs:151-158, 225-235`；`scanner.rs:642-755, 771-790`）。这是防“瞬时故障=整家清空”的系统性纪律。
4. **写事务内副本裁决 + last-good**：全量与 watcher 增量共用 `write_session_guarded`，防旧快照覆盖新副本；胜者坏了按顺位回退（`scanner.rs:389-423, 546-605, 1136-1151`）。
5. **删除流水线**：review→revalidate→hash journal→trash→verify→索引原子删/墓碑；恢复要内容证据；跨会话包含/共享边车拒删；墓碑双轨防复活（第 3.3 节）。sessiongrep 无对应产品面；ASG 若做删除应有同级纪律。
6. **监视语义**：事件种类不裁决、按现状收敛；unlink→rename 原子替换有测试；溢出→rescan；drop join 防旧 roster 回写（`watcher.rs:114-200, 316-395`）。
7. **单表面契约**：CLI 与 MCP 共用定义 + 双射测试 + 同输出承诺（`cli.rs:1-10, 959-1012`），agent 走哪条接入语义一致。
8. **身份单点**：`session_key` 唯一构造点覆盖本地/远程两段或三段 key（`models.rs:970-993`），scanner 易主检测、watcher 反查、远程装饰器全部复用。

### 4.2 Wake 弱在哪（ASG 不该学 / 需改造）

1. **短词即全表 LIKE**（WK-01）：无 bigram/前缀索引，1–2 码点（含 CJK 双字）查询线性扫全库且丢 bm25 排序。
2. **零命中无兜底 + 一次性旗标**（WK-02）：重索引失败窗口内 ≥3 码点查询可静默返回空；`fts_reindex` 一轮后无条件清旗（`scanner.rs:328-333, 625-630`）。
3. **新鲜度仅 mtime+size**（WK-03）：无内容哈希二次确认（sidecar 除外），保时间戳的改写会被漏掉。
4. **健康度不可见**（WK-04）：解析/写库失败仅 eprintln；MCP 只报“内容新鲜到何时”，不报跳过的文件数/坏行数（unknown_lines 已入库但不出面）。
5. **消息身份薄**（WK-05）：模型只有 seq（+32KiB 截断文本），无原生消息 id / 父边 / 源字节区间；FTS 只索引裁剪后的文本。ASG 的 message identity / placement 契约更强，不应为对齐 Wake 而退化。
6. **远程与清理面收窄**（WK-06 / WK-08）：stage-1 远程仅默认布局 + GUI 触发；cleanup 面不进 MCP/CLI（有意为之的安全取舍，但能力覆盖是差距）。

### 4.3 总判

Wake 在“检索正确性 / 降级边界 / 删除安全 / 监视收敛”上明显强于 sessiongrep，多项纪律（None≠空、写事务裁决、墓碑双轨、删除内容证据）值得 ASG 直接借鉴；它在“短查询性能、重索引成功水位、索引健康度可见性”上弱于理想形态，也在消息身份模型上弱于 ASG 既有契约。**该学纪律，不该学 seq-only 身份与全表 LIKE 退化。**

## 5. 发现清单（锚点 / 严重度 / 证据 / 反证）

严重度沿用本系列研究口径：P1=核心正确性优先核查；P2=有条件的正确性/可用性/边界风险；P3=较低优先级或证据欠缺；I=已知产品取舍。

| ID | 严重度 | 摘要 | 关键锚点 |
|---|---|---|---|
| WK-01 | P2/I | 任意 <3 码点词项使整条查询退化为全表 LIKE（含 CJK 双字），丢 bm25 | `db.rs:3259-3262, 2618-2688, 2250-2258` |
| WK-02 | P2 | ≥3 码点 trigram 零命中无 LIKE/提示兜底；fts_reindex 旗一轮后无条件清 | `db.rs:2565-2571, 2618-2653`；`scanner.rs:328-333, 625-630` |
| WK-03 | P2 | 新鲜度仅 mtime+size，无内容哈希（sidecar 独立刷新为反证） | `scanner.rs:461-464, 485-488, 447-451` |
| WK-04 | P2/I | 解析/写库失败只有 eprintln，MCP/CLI 无健康度面 | `scanner.rs:558, 562, 594-597, 1153`；`mcp/tools.rs:471-480` |
| WK-05 | P3/I | 消息身份只有 seq；索引文本被 32KiB 截断；无原生 id/字节锚 | `models.rs:324-340, 372-380, 712-720` |
| WK-06 | P3/I | 远程 stage-1：默认布局、ssh/rsync 外部依赖、GUI 触发、IDE 源缺席 | `remote.rs:52-54, 94-101, 315-317, 385-403` |
| WK-07 | P3 | 监视依赖 notify；单文件增量沿 mtime+size；溢出靠 rescan（回退路径完整） | `watcher.rs:138-144, 197-199`；`scanner.rs:1080-1170` |
| WK-08 | I | cleanup 硬链接防护 cfg(unix)；Windows 走 windows_fs identity（未读，不判缺陷） | `cleanup.rs:316-343` |

反证要点（对应上表，避免夸大）：

- WK-01：LIKE 路径对 `\ % _` 转义、snippet 字符安全，且对用户明说降级；功能正确，问题只在规模与排序。
- WK-02：清旗为有意设计（注释：解析失败的文件不会因重试自愈，文件真变了自然走增量）；零命中在索引新鲜时本不该发生。
- WK-03：sidecar 有独立的“晚到元数据”刷新通道（`scanner.rs:447-451`）；FTS 派生换代有强制重解析。
- WK-04：会话级读取失败会在 `wake_get_session` 以 Failed 文本呈现给调用方（`mcp/tools.rs:1145-1150`）；索引整体定位为“可丢弃 cache”。
- WK-05：seq 由扫描与详情共用同一解析器保证对齐（`adapters/mod.rs:37-38`），对“搜索→跳转”契约足够；缺的是原生消息身份/字节锚。

## 6. 残余未决（Residual）

- **db.rs 未读约 2330 行**：未读区间 `[1,54],[176,195],[326,960],[1051,1105],[1191,2199],[2271,2539],[2782,2860],[2911,3100],[3221,3239]`。其中含 list_sessions 排序/分页细节、Insights 统计、location/remote_host 管理表操作、写事务其余部分与绝大多数测试；本报告未就这些区间下结论。
- **opencode.rs 未读 [61,718]**（schema 生成 SQL、v1/v2 解析、工具块累加器）；**remote.rs 未读 [1,29],[106,254],[540,734]**（其余布局表项与测试）；**cleanup.rs 未读 [1,290],[546,652],[763,809],[1029,2349]**（选项/serde、共享路径盘点、save/history、大量测试）。
- **未执行**：build/test/install/网络/git 写。FTS5 trigram 短词行为只做静态判定（代码用 `chars().count()<3` 主动规避）；全表 LIKE 的性能量级来自代码路径与注释，未做本机基准。
- **未读文件**（可能影响细节、不影响主结论）：`mcp/mod.rs`（协议层）、`services/context.rs`（since 解析/项目匹配）、`services/exporter.rs`（compact 渲染）、`parse_utils.rs`、`sqlite_ro.rs`、`windows_fs.rs` 与其余 19 家 adapter。
- **证据时点**：commit `71aeca67ec80f8645d1f9d5199290c2c732036ce`；交付时复核 SHA256 与前一次捕获一致，逐文件结论见 `coverage-Wake.json` 的 `sha256_initial/sha256_final/content_unchanged`。
- **所有权**：本 worker 只创建/更新 `audit-Wake.md` 与 `coverage-Wake.json`；未改动 Wake 仓库，未改动 TASKDIR 其它文件。