# 竞品源码审计：cc-switch（分片 A：会话管理 / 检索 / 用量）

- Repository: `C:\AgentSessions\Github_src\cc-switch`
- Commit: `f748f3ac4afbbbc6554a3ca2bb7e56e4036fedfa`（审读期间工作树未被本 worker 改动；未做 git 写操作）
- Date: 2026-10-06
- Task: `.trellis/tasks/10-05-competitive-source-audit-plan`
- 阅读凭证: `coverage-cc-switch-sessions.json`（20 个文件；17 full + 3 partial；11 675 行有界读取，T1 主文件 9 739 行全部 full）
- 方法: 只读源码审读（codegraph node 行号输出，每段 ≤150 行）；未运行 build/test/install，未联网。严重度是审计排序：P1=核心事实/正确性优先核对；P2=有条件风险或能力边界；P3=低优先不一致；I=已知取舍/信息项。全部为静态推导，非运行复现。

## 0. 结论摘要

1. **「无检索」说法不成立**：cc-switch 有明确的会话检索——FlexSearch 内存索引 + 会话列表搜索框；但索引只含元数据字段（title/summary/projectDir/sourcePath/sessionId），**不含消息正文、无持久化**。ASG 竞品表第 30/49 行应改写为「元数据检索」，而非「无检索/不可比（整体）」。
2. 会话管理覆盖 **7 家** CLI 工具（Codex/Claude/OpenCode/OpenClaw/Gemini/Hermes/Grok Build）：并行全量扫描，无持久索引；统一字段为 provider_id/session_id/title/summary/project_dir/created_at/last_active_at/source_path/resume_command（`src-tauri/src/session_manager/mod.rs:9-28`）。
3. resume（继续执行）能力：**5/7 家**能构造恢复命令；**实际拉起终端仅 macOS**（Terminal/iTerm/Ghostty/Kitty/WezTerm/Kaku/Alacritty/Warp），其它平台是「复制命令」兜底；Hermes/OpenClaw 源码显式无 resume（网关托管）。
4. 搜索结果是「元数据命中 → 打开会话」；正文搜索词只在**已打开会话内**做高亮，不提供跨会话正文检索，也不会自动跳到首个命中（目录/TOC 只跳用户消息）。
5. 用量统计（`session_usage*.rs`）是独立子系统：5 个 app 的 token/费用增量导入，游标、去重、Codex 重放去重等工程化程度较高；与「会话检索」不是同一件事，不应混为一谈。

## 1. 阅读凭证与覆盖统计

| 类别 | 文件 | 行数 | 状态 |
|---|---|---:|---|
| T1 主文件 | `src-tauri/src/session_manager/mod.rs` | 359 | full（1-150/151-300/301-359） |
| T1 主文件 | `src-tauri/src/session_manager/providers/codex.rs` | 997 | full |
| T1 主文件 | `src-tauri/src/session_manager/providers/claude.rs` | 500 | full |
| T1 主文件 | `src-tauri/src/session_manager/providers/opencode.rs` | 1001 | full |
| T1 主文件 | `src-tauri/src/session_manager/providers/hermes.rs` | 603 | full |
| T1 主文件 | `src-tauri/src/session_manager/providers/openclaw.rs` | 473 | full |
| T1 主文件 | `src-tauri/src/services/session_usage_codex.rs` | 3086 | full |
| T1 主文件 | `src/components/sessions/SessionManagerPage.tsx` | 1753 | full |
| T1 主文件 | `src/hooks/useSessionSearch.ts` | 72 | full |
| T1 主文件 | `src-tauri/src/services/session_usage.rs` | 895 | full |
| T1 可选 | `src-tauri/src/codex_history_migration.rs` | 2630 | partial（1-300/429-578/961-1110；未读 301-428/579-960/1111-2630） |
| 补充（完整） | `session_manager/providers/mod.rs`、`utils.rs`、`gemini.rs`、`grokbuild.rs`；`components/sessions/SessionItem.tsx`、`SessionMessageItem.tsx`；`commands/session_manager.rs` | 1 091 | full |
| 补充（部分） | `src/lib/query/queries.ts`（300-325；未读 1-299）；`session_manager/terminal/mod.rs`（1-150/277-345；未读 151-276/346-440） | 245 | partial |

- 上述每行的精确读区间、字节数、SHA256（initial/final）、缺失区间均记录在 `coverage-cc-switch-sessions.json`；哈希仅作完整性指纹，不是阅读凭证。
- 说明：T1 主文件与 `codex_history_migration.rs` 的初始哈希在审读前采集（校验一致，`content_unchanged=true`）；9 个补充文件的初始哈希为审读完成时计算（该文件未在预读时采集），本 worker 对仓库只执行了只读操作。

## 2. 七家 scanner / 会话管理覆盖矩阵

公共模型：会话元数据 9 字段（`mod.rs:9-28`），消息仅 role/content/ts（`mod.rs:30-37`）。扫描调度：`mod.rs:58-94` 用 `std::thread::scope` 并行跑 7 家，单源 panic 用 `unwrap_or_default()` 隔离降级，合并后按 `last_active_at.or(created_at)` 倒序。消息加载分派与删除统一入口见 `mod.rs:96-143`；删除前统一 `canonicalize` 校验 source 必须位于 provider roots 内（`mod.rs:151-210`）。

| provider | 扫描根与格式 | 抓取的字段/标题与时间 | resume_command | 特有限制与防护 |
|---|---|---|---|---|
| codex | `~/.codex/sessions` + `archived_sessions` 递归 `*.jsonl`（`codex.rs:43-49,504-522`） | session_meta 取 id/cwd/created（头 10 行）；标题叠加 `session_index.jsonl` + state DB `threads`（75-202）；首条真实用户消息作标题候选，跳过 `# AGENTS.md`、`<environment_context>`，VS Code IDE 上下文取最后一个 `## My request for Codex:` 段（318-320,425-497）；尾部 30 行取 last_active 与 summary（362-403） | `codex resume <id>`（414） | 跳过 subagent session（318-320）；state DB 只读 + busy_timeout，避免与运行中 Codex 抢锁（147-155）；显式不 SELECT `first_user_message` 大字段防 OOM（157-164） |
| claude | `~/.claude/projects/**/*.jsonl` 递归（`claude.rs:17-30,268-286`） | 标题优先级 custom-title > 首条真实用户消息（跳过 `<local-command-caveat>`/`<command-name>`）> 目录名（156-177,200-238）；head/tail 取时间 | `claude --resume <id>`（251） | 跳过 `agent-*` 子代理文件（255-260）；删除时连带同名 sidecar 目录（88-121,288-300） |
| opencode | legacy `storage/session/**/*.json` + `opencode.db`（SQLite 优先、ID 去重合并）（`opencode.rs:36-62`） | SQLite：id/title/directory/time_created/time_updated（96-156）；JSON：title 或目录名，summary 取首条用户消息 parts（423-530） | `opencode -s <id>`（152,476） | SQLite source_path 形如 `sqlite:<db>:<id>`（83-94）；SQLite 删除在事务内并校验 db 路径等于预期 opencode.db（382-421） |
| openclaw | `~/.openclaw/agents/*/sessions/*.jsonl` + 同目录 `sessions.json` displayName（`openclaw.rs:30-75,158-181`） | displayName > 首条用户消息（去尾部 `[message_id: ...]`）> 目录名（227-287）；head/tail 取时间 | 无（298 行注释：gateway-managed，无 CLI resume） | 删除同时从 `sessions.json` 索引里移除对应条目（125-154,302-338） |
| gemini | `~/.gemini/tmp/<project>/chats/*.json`（项目根读 `.project_root`）（`gemini.rs:11-55`） | sessionId/startTime/lastUpdated；标题=首条用户消息（截断 160 字符）（140-174） | `gemini --resume <id>`（172） | 消息加载合并 content 数组与 toolCalls 为 `[Tool: ...]`；info/error 类目跳过（69-97）；删除前 ID 校验（115-138） |
| hermes | `~/.hermes/state.db`（sessions 表）+ `sessions/*.json(l)` 浅扫一层（`hermes.rs:25-50,285-308`） | SQLite：id/title/cwd(title 或 directory)/started_at/ended_at；JSONL：`type=session` 元数据 + 首条用户消息（107-148,310-429） | 无（146,427） | SQLite 扫描 **`SELECT * FROM sessions ORDER BY rowid DESC LIMIT 500`**（84）；消息读取固定列 role/content/created_at（199-200）；JSONL 删除不校验 session_id（488-496） |
| grokbuild | `~/.grok/sessions` + `archived_sessions` 递归 `summary.json`，消息在 `chat_history.jsonl`（`grokbuild.rs:34-51,53-90`） | generated_title > session_summary；created_at/last_active_at/updated_at（157-194）；消息跳过 reasoning（63-73） | `grok --resume <id>`（192） | 删除整个会话目录，多重校验（root 前缀、文件名、ID、目录名）（92-134）；有「拒绝越界删除」测试（279-295） |

补充事实：
- UI 的 ProviderFilter 类型包含 7 家（`SessionManagerPage.tsx:83-91`），但下拉框实际只列出 6 家（**没有 hermes 选项**，1058-1131）——hermes 会话仅在「全部」视图可见。见 F8。
- 共享解析工具（`utils.rs`）：标题上限 80 字符（9）；head/tail 读取小文件全读、大文件首段 + 尾部 ~16KB（13-49）；时间戳兼容秒/毫秒/RFC3339（51-65）；`extract_text` 将 tool_use/tool_result 归一为文本（67-128）。

## 3. 检索链路（FlexSearch 索引了什么、能做什么）

**索引实现**（`src/hooks/useSessionSearch.ts`）：
- `new FlexSearch.Index({ tokenize: "full", resolution: 9 })`（27-31）。
- 每条会话写入的检索串 = `sessionId + title + summary + projectDir + sourcePath` 拼接（33-45）。**只有元数据，消息正文不进入索引**（与 15-16 行注释一致）。
- 索引按「provider 过滤后的会话数组」在 `useMemo` 中重建（22-25,27-48）；纯内存，不落盘（全仓仅此一处 `flexsearch` 导入，T2 sweep 与源码 grep 一致）。
- 查询：空串返回全量列表并按 lastActiveAt 降序（54-60）；非空走 `index.search(needle, {limit: filteredByProvider.length})` 映射回 SessionMeta（62-66）。

**入口**（`src/components/sessions/SessionManagerPage.tsx`）：
- 列表标题栏的搜索按钮打开输入框（997-1017），输入实时过滤（802-836），Esc/清空关闭。
- Provider 过滤先于索引（`useSessionSearch.ts:22-25`）；列表视图支持 flat/grouped 与折叠状态持久化（localStorage，78-113,162-169）。

**结果能做什么**：
- 打开：点击结果 → 右侧详情 → `get_session_messages` 拉取该会话全部消息（`SessionManagerPage.tsx:305-327`；Tauri 命令 `commands/session_manager.rs:13-25`；React Query `lib/query/queries.ts:315-324`）；消息列表用 @tanstack/react-virtual 虚拟化渲染（331-337,1655-1680）。
- 跳转：会话内对已加载消息按 `searchQuery` 高亮（`SessionMessageItem.tsx:39-48,89-93`），长消息命中时抑制折叠；但**不会自动滚动到首个命中**；TOC 仅收集 user 消息（可含 Codex 清洗规则），点击可 `scrollToIndex` 跳转（`SessionManagerPage.tsx:366-391,1686-1698`）。
- 恢复/删除：选中会话后可单删/批量删除，删除有确认对话框与缓存一致性处理（442-545,1706-1750）；恢复见 §4。

**边界**：这是「元数据检索 + 已打开会话内的正文高亮」。它不能回答「哪条消息含关键词 / 证据在哪个字节」，也不提供跨会话正文匹配、命中排序或片段跳转。
## 4. resume / 继续执行能力（命令如何构造、何时可达）

**命令构造**：全部由 provider 模块用 `format!` 拼字符串写入 `SessionMeta.resume_command`——
- `codex resume {session_id}`（`codex.rs:414`）
- `claude --resume {session_id}`（`claude.rs:251`）
- `opencode -s {session_id}`（`opencode.rs:152,476`）
- `gemini --resume {session_id}`（`gemini.rs:172`）
- `grok --resume {session_id}`（`grokbuild.rs:192`）
- Hermes / OpenClaw：`None`（`hermes.rs:146,427`；`openclaw.rs:298` 注释「gateway-managed, no CLI resume」）

**执行链路**（仅 macOS 可真正拉起终端）：
- UI 恢复按钮仅在 `isMac()` 时显示（`SessionManagerPage.tsx:1521-1548`）；点击 `handleResume`（418-440）：非 macOS 直接复制命令到剪贴板并 toast；macOS 调 `sessionsApi.launchTerminal({command, cwd: projectDir})`。
- Tauri 命令 `launch_session_terminal`（`commands/session_manager.rs:61-93`）：读全局偏好终端（`iterm2`→`iterm`，缺省 `terminal`），转 `session_manager::terminal::launch_terminal`。
- `terminal/mod.rs:3-29`：`if !cfg!(target_os = "macos") { return Err("Terminal resume is only supported on macOS") }`；支持 Terminal/iTerm/Ghostty/Kitty/WezTerm/Kaku/Alacritty/Warp/custom（17-28）。

**转义与安全边界**：
- cwd 经 `build_shell_command` 拼成 `cd '<cwd>' && <command>`（`terminal/mod.rs:319-326`），`shell_escape` 使用 POSIX 单引号转义并处理单引号自身（328-338）——源码注释说明这是为防 `projectDir` 中的 `$(...)` 在用户终端展开。
- `command` 字符串本身**不做校验**，接受 renderer 任意字符串并最终交给 shell；`commands/session_manager.rs:27-60` 将这一点明确记录为「已知并接受的风险」，给出 4 条支撑事实（唯一 `dangerouslySetInnerHTML` 仅渲染受控图标名、无 eval、无远程 webview、CSP `script-src 'self'`）与「什么会推翻该结论」的触发条件（一旦渲染富文本/放宽 CSP，就必须改成后端按会话标识重建命令）。
- `custom` 终端模板分支当前无 UI 入口、前端从不传 `customConfig`，源码注记「接线前必须换掉该方案」（`terminal/mod.rs:288-298`）。

**结论**：具备 resume/继续执行能力——5/7 家命令可构造；macOS 可一键在新终端继续，其它平台以复制命令兜底；无恢复命令的会话按钮禁用/隐藏（`SessionManagerPage.tsx:1528-1545`）。

## 5. 新鲜度 / 缓存 / 并发

### 5.1 会话管理（读路径）
- **无持久索引、无 watcher**：`list_sessions` 每次调用都 `spawn_blocking(scan_sessions)` 全量重扫（`commands/session_manager.rs:6-11`；`mod.rs:58-94`）；T2 sweep 的 `fs_watch` 命中 3 处均不在会话/用量代码路径。
- **并发**：7 家 scanner 并行 `std::thread::scope`（`mod.rs:59-76`）；SQLite 源以 `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX` 读取（`opencode.rs:102-105`、`hermes.rs:60-63`）；Codex 标题库设 `busy_timeout` 防止运行中的 Codex 写锁导致标题静默丢失（`codex.rs:147-155`）。
- **扫描抽样**：head/tail 读取——<16KB 全读；否则读首段与尾部 ~16KB（`utils.rs:13-49`）。这意味着超大文件的中间内容不参与元数据（标题/摘要/最后时间）生成——是固定抽样，不是流式全解析。
- **UI 缓存**：React Query `useSessionsQuery` 与 `useSessionMessagesQuery` 均 `staleTime: 30*1000`（`queries.ts:307-324`）；手动刷新按钮触发 `refetch`（`SessionManagerPage.tsx:1134-1146`）；删除后手工维护缓存（setQueryData/removeQueries/invalidate，442-545）。
- **FlexSearch 索引**：内存态，随 sessions/providerFilter 变化重建（§3），窗口关闭即消失。
- **删除防越界**：统一 canonicalize 校验 source 位于 provider roots 内（`mod.rs:151-178`），批量删除逐条返回 success/error（`mod.rs:221-254`）。

### 5.2 用量统计（session_usage*.rs，独立子系统）
- **调度与互斥**：`sync_all_unlocked` 顺序跑 Claude/Codex/Gemini/OpenCode/Grok Build 5 家，调用方必须持有全局 `session_sync_mutex`（`session_usage.rs:53-56,71-96`）。
- **增量游标**：每文件 `session_log_sync(file_path, last_modified, last_line_offset)`（433-489）；mtime 用纳秒（旧秒级数据触发一次重扫，靠行 offset 防重复，443-454）；文件未变化直接跳过（257-267）。
- **去重**：同一 `message.id` 多条时按 stop_reason 优先/输出 token 较大者保留（361-380）；`request_id`（`session:<msg_id>` / `codex_session:thread-v1:<thread>:<index>`）+ `INSERT OR IGNORE`（408-421,560-598）；与代理日志用指纹（model/tokens/时间）去重（514-525）。
- **Codex 专项**（`session_usage_codex.rs`）：token_count 累计→delta（587-606）；优先精确的 `last_token_usage`，否则 total 高水位差（903-929）；按 rate-limit `limit_id` 来源抑制重复快照（884-901）；fork/spawn 的父子 rollout 用重放前缀扣除父用量（1077-1091,1204-1271）；父时间线缓存以文件戳（mtime+size+inode/volume serial+file id）失效（89-140,969-1051）；pending 状态（MissingParent/Stable/Retryable）延迟重试（220-232,1093-1125,1155-1180）。
- **批写与游标原子性**：1000 条/批事务；最后一批把游标推进与数据同事务提交，避免「游标已推进但数据缺失」（1130,1295-1346）；`reset_codex_usage` 用 SAVEPOINT + 清空内存缓存（305-383）。
- **边界**：这套缓存/游标只服务「用量统计」，不参与会话列表或检索；另注意 Hermes/OpenClaw 没有用量同步（5/7 家）。

## 6. Findings

### F1 · P1 · 「无检索」被推翻：存在元数据检索（事实表必须修正）
- **证据**：依赖 `flexsearch@^0.8.212`（package.json dependencies）；`useSessionSearch.ts:27-48` 建 FlexSearch 索引（tokenize full、resolution 9），写入 sessionId/title/summary/projectDir/sourcePath；UI 搜索按钮/输入框（`SessionManagerPage.tsx:997-1017,802-836`）；列表项标题高亮（`SessionItem.tsx:88-90`）；空查询排序逻辑（`useSessionSearch.ts:54-60`）。
- **反证/边界**：索引不含消息正文、不落盘、无 source span；命中只能定位到「会话」而不是「消息/字节」；并且只覆盖已扫描列表（7 家、字段受 provider 解析能力限制）。
- **ASG 教训**：竞品表结论必须按「索引字段 + 检索单位 + 证据能力」分级描述；不能因为竞品不是全文证据检索就写成「无检索/不可比」。见 §8 的具体改法。

### F2 · P2 · 检索结果动作完整度：能打开/高亮，不能跳转到命中
- **证据**：结果点击 → 详情加载全部消息（`SessionManagerPage.tsx:305-327`；`commands/session_manager.rs:13-25`）；正文高亮（`SessionMessageItem.tsx:39-48,89-93`）；TOC 只收 user 消息并可 scrollToIndex（`SessionManagerPage.tsx:366-391`）；没有「search hit → scroll to message」代码路径。
- **可达**：任何「搜到摘要/标题但与正文命中位置相距很远」的会话，用户需要手动滚动；TOC 不能定位 assistant/tool 命中。
- **反证/界限**：正文高亮确实降低了阅读成本；对大多数「找会话」需求，元数据检索已够用——这是产品定位差异，不是缺陷。
- **ASG 教训**：若 ASG 做 GUI，命中应带消息级 ID + 滚动定位 + 片段折叠上下文；只有元数据索引时不要把「高亮」宣传成「全文检索」。

### F3 · P2 · resume 的平台与来源限制：macOS-only + 命令字符串免校验
- **证据**：`terminal/mod.rs:13-15` 非 macOS 直接报错；UI 按钮 `isMac()` 条件（`SessionManagerPage.tsx:1521-1548`）；非 macOS 走复制（418-427）；`command: String` 从 renderer 直通 shell（`commands/session_manager.rs:61-93`），风险与前提条件在 27-60 行显式记录。
- **可达**：Linux/Windows 用户只能手动粘贴命令执行；若未来 webview 引入远程内容/富文本（推翻条件之一），该命令通道会变成实际注入面。
- **反证/界限**：cwd 已单引号转义（`terminal/mod.rs:328-338`）；当前 CSP/无远程内容前提下风险被源码论证为「已接受」；删除/拷贝等其它能力跨平台可用。
- **ASG 教训**：恢复执行优先传结构化 argv/cwd；命令字符串与 shell 执行解耦；平台能力差异要在 UI 明示（cc-switch 的复制兜底是值得学的降级模式）。

### F4 · P2 · 扫描覆盖面与静默截断/抽样
- **证据**：Hermes SQLite `LIMIT 500`（`hermes.rs:84`）——超过 500 个会话时旧会话静默不可见；Hermes JSONL 仅浅扫一层（291-307）；Codex 扫描递归无深度上限（`codex.rs:504-522`），而用量子系统同类采集限深 3（`session_usage_codex.rs:741-755`）；head/tail 抽样（`utils.rs:13-49`）。
- **可达**：重度 Hermes 用户、深层目录结构或超大会话文件会得到「不完整但无提示」的列表/元数据。
- **反证/界限**：LIMIT 500 保最新（rowid DESC），可能是有意上限；头部/尾部抽样对「会话是什么/最近何时用」够用；这不是数据删除，只是展示截断。
- **ASG 教训**：任何截断/抽样都要有可见的「已截断/总数 N」信号；列表默认 metadata-only 也要报告覆盖完整性（哪些来源读了、哪些被跳过）。

### F5 · P2 · 新鲜度模型：全量重扫 + 30s 缓存 + 无 watcher（简单可靠但成本线性）
- **证据**：每次 `list_sessions` 触达全部 7 源（`mod.rs:58-94`、`commands/session_manager.rs:6-11`）；UI staleTime 30s（`queries.ts:307-313`）；FlexSearch 重建时机（`useSessionSearch.ts:22-48`）；无文件监听。
- **可达**：会话目录非常大时，每次过期刷新都产生 O(源数量 × 文件数) 的 stat + head/tail 读；连续搜索触发列表重算（索引仅随 sessions/过滤变化重建，查询本身是内存查找）。
- **反证/界限**：无索引=无陈旧索引问题；删除/迁移后无需失效协议；本轮未测性能，不能据此断言「慢」。
- **ASG 教训**：若做 GUI，可保留「显式刷新」，但应加 mtime 短路或后台增量；新鲜度要有 last_scan/last_error 状态，而不是只靠 30s TTL。
### F6 · P2（工程强项，I 类信息）· 用量子系统的增量/去重/重放工程化
- **证据**：5 源调度与全局互斥（`session_usage.rs:53-56,71-96`）；mtime+行 offset 游标与事务化推进（433-489；`session_usage_codex.rs:1295-1346`）；多 rate-limit 通道重复快照抑制（`session_usage_codex.rs:884-901`）；父子 rollout 重放前缀扣除（1077-1091,1204-1271）；pending/deferred 与文件戳缓存（89-140,1093-1125,1155-1180）；reset/rebuild（305-383）；真实语料回放 harness（2984-3085，`#[ignore]`，本任务未运行）。
- **反证/界限**：这些是启发式规则（快照去重、重放前缀对齐），依赖 Codex 日志形态假设；代码内有对应单测（如 1729-1797、2204-2276），但本轮未运行、未做真实语料复现。
- **ASG 教训**：ASG 若做 usage/统计类能力，「游标+幂等键+重放防护+同事务推进游标」是可直接借鉴的成熟模式；检索主链路不应耦合此类状态。

### F7 · P3 · Hermes 的一致性问题
- **证据**：sessions 表用 PRAGMA 动态列名适配（`hermes.rs:151-165`），但 messages 读取写死 `SELECT role, content, created_at`（199-200）——schema 变体下表现为「列表可见、消息加载失败」；JSONL 删除不校验 session_id（488-496），与 codex/claude/gemini/openclaw/opencode 的删除校验不一致。
- **可达**：Hermes schema 变化时局部功能失效；若 renderer 传入错误 sourcePath（仍在 hermes root 内），可能删除非目标 JSONL 文件（正常 UI 流程不会构造这种请求）。
- **反证/界限**：上层 `mod.rs:151-178` 已做 roots 归属校验，删除仍限制在 provider 根内；JSONL 路径来自本机扫描结果，攻击面有限。
- **ASG 教训**：删除路径必须在 provider 级同样校验 ID/身份（ASG 的 StableId/CAS 契约同理）；schema 适配要么全量要么显式报错，不做半适配。

### F8 · P3 · UI 过滤下拉缺 Hermes 选项
- **证据**：`ProviderFilter` 联合类型含 `hermes`（`SessionManagerPage.tsx:83-91`），初始值来自 `appId`（212-214）；但 Select 列表只渲染 all/codex/grokbuild/claude/opencode/openclaw/gemini（1058-1131），无 hermes item。
- **可达**：用户在「全部」里能看到 hermes 会话，但无法从下拉切到 hermes；当 `appId === "hermes"` 进入时初始过滤仍生效（不阻断）。
- **反证/界限**：无功能破坏（hermes 仍可浏览/删除）；可能是刻意隐藏或遗漏，无法从源码判定意图。
- **ASG 教训**：类型域与可选项要由同一数据源驱动（如 provider registry），避免「类型里有、UI 里没有」的漂移。

### F9 · I（信息项）· Codex 历史迁移子系统会改写会话文件（与新鲜度相邻）
- **证据**：迁移把 `session_meta.model_provider` 从官方/旧第三方 id 改写为统一的 ccswitch id（`codex_history_migration.rs:1075-1099`）；执行前备份、双重 `ensure_codex_session_file_unchanged`、`atomic_write`（1022-1073）；marker 与 Codex 目录绑定、迁移期间开关被关则不写标记（212-272）；还原用备份账本按 session id 精确翻回（429-483）。
- **反证/界限**：失败不写标记、下次启动重试（1-4）；「文件在迁移期间被修改」会直接报错而不是覆盖；与检索无直接关系。本轮只读了选择性区域（1-300/429-578/961-1110）。
- **ASG 教训**：任何对来源会话文件的「原地改动」都应有备份 + 未变更校验 + 原子写 + 幂等标记；ASG 的「只读证据层」本身不引入该风险。

## 7. 与 ASG 对照

### 7.1 它在「顺手可用」上强在哪
1. **零配置聚合 7 家 CLI 会话**：桌面 App 一个列表就能看 Codex/Claude/OpenCode/OpenClaw/Gemini/Hermes/Grok Build 的会话，含标题、摘要、项目目录、活跃时间、provider 图标。
2. **点击即用**：详情页全量消息（虚拟化）、复制消息、用户消息 TOC、项目路径/源路径一键复制、单删/批量删除（确认 + 缓存维护）。
3. **恢复闭环**：5/7 家给出 resume 命令；macOS 一键在偏好终端拉起；其它平台复制兜底；恢复命令在 UI 中可见可复制（1583-1612）。
4. **视图与组织**：flat/grouped、provider/目录分组、折叠状态记忆、批量选择（`SessionManagerPage.tsx:78-113,547-788`）。
5. **用量面板**：5 家 CLI 的 token/成本统计（独立子系统，工程化程度高；见 F6）。

### 7.2 metadata 检索 vs 全文证据检索的边界
| 维度 | cc-switch | ASG（对照口径） |
|---|---|---|
| 索引对象 | 会话元数据串（title/summary/projectDir/sourcePath/sessionId） | 正文/证据（FTS；source span；handoff pack） |
| 检索单位 | 会话 | 消息/证据片段（含稳定 ID 与来源定位） |
| 摘要来源 | 首条/尾条消息截断（title ≤80、summary ≤160） | 原文引用 + 推断分栏 |
| 持久化 | 无（内存索引，随组件重建） | 持久化索引 |
| 结果动作 | 打开会话；会话内高亮；无命中跳转 | 命中定位/证据导出等证据级动作 |
| 可比结论 | 「会话管理顺手度」可比较；「证据检索」不可比（不是「无检索」） | — |

### 7.3 ASG 该学 / 不该学
**该学**：
- 单源隔离与降级（`mod.rs:68-75` 的 `unwrap_or_default()`，一个 provider 失败不拖垮列表）；
- resume 命令 + 复制兜底 + 平台能力显式化（macOS 拉起，其它平台复制）；
- 删除的「校验 → 确认 → 批量结果逐条反馈 → 缓存一致性」闭环（`SessionManagerPage.tsx:442-545`）；
- 用量子系统的游标/幂等键/重放防护/批事务（F6）；
- 迁移的备份 + 未变更校验 + 原子写 + 幂等标记模式（F9）；
- 30s staleTime + 手动刷新这种低复杂度新鲜度契约（作为 baseline，而非上限）。

**不该学**：
- 把「元数据检索」当全文检索卖点（也不该在竞品表把它写成「无检索」）；
- 静默 `LIMIT 500`（F4）与固定列假设（F7）；
- provider 级删除缺 ID 校验（F7）；
- macOS-only 的执行面与 renderer 直通 shell 的命令通道（F3，除非满足其记录的前提条件并接受风险）；
- 用内存索引 + 全量重扫替代可复现的检索契约（ASG 的持久索引/证据定位是不同层级能力）。

## 8. 对 `docs/product/COMPETITOR-COMPARISON.md` 的修正建议（第 30/49 行）

现状（ASG 仓库，逐字）：
- 第 30 行：`| cc-switch | 桌面 App | MIT | 7 | 无检索(配置切换器) | 非检索工具,「不可比」 |`
- 第 49 行：`- **cc-switch**: 配置切换器,无检索能力。`

建议改法（保持表格结构，仅替换表述；行号不变）：

- 第 30 行改为：
  `| cc-switch | 桌面 App | MIT | 7 | 元数据检索：FlexSearch 内存索引，仅标题/摘要/项目路径/SourcePath/会话 ID，不含消息正文、不持久化 | 7 家 CLI 会话管理 + resume 命令生成（macOS 可一键拉起终端）；检索为元数据级，不能当全文/证据检索比较 |`
  - 主要锚点：`src/hooks/useSessionSearch.ts:27-48`（索引字段）、`src/components/sessions/SessionManagerPage.tsx:229-237,802-836,997-1017`（入口）、`src-tauri/src/session_manager/providers/mod.rs:1-8` 与 `src-tauri/src/session_manager/mod.rs:58-94`（7 家扫描）、`providers/codex.rs:414`、`providers/claude.rs:251`、`providers/opencode.rs:476`、`providers/gemini.rs:172`、`providers/grokbuild.rs:192`（resume 命令构造）。
- 第 49 行改为：
  `- **cc-switch**: 配置切换器 + 7 家 CLI 会话管理器。会话检索为 FlexSearch 仅元数据的内存索引（useSessionSearch.ts:27-48；不索引消息正文、无持久化、无 source span），与 ASG 的持久 FTS5 全内容 + 证据定位是不同域——「不可比」应限定为「证据级检索不可比」，而不是「无检索」。`
- 第 30 行备注列的「非检索工具」必须一并删除/改写；不要删除该竞品行——其桌面会话管理与恢复能力仍是有价值的参考基线。
- 若后续有其它文档引用「cc-switch 无检索」，需同步改为「元数据检索（非全文证据检索）」。

## 9. 残余未决

1. **用量三源未审**：`session_usage_gemini.rs`(497)、`session_usage_grokbuild.rs`(1234)、`session_usage_opencode.rs`(580)、`usage_stats.rs`(4359) 未读——「5 源用量同步」仅验证了调度层（`session_usage.rs:71-96`）与 Codex/Claude 两个实现。
2. **terminal 细节未全读**：`terminal/mod.rs` 151-276、346-441（各终端分支实现与测试）未读，仅按函数大纲与 1-150/277-345 区段判断。
3. **迁移其余区域未读**：`codex_history_migration.rs` 301-428、579-960、1111-2630（状态库迁移细节、还原写路径、测试）未读；F9 只覆盖入口/门控/文件改写防护/账本收集。
4. **前端复用件**：`src/components/sessions/utils.ts`（`highlightText`、标题格式化、Codex 消息清洗）未逐行读；正文高亮行为由调用点（SessionItem/SessionMessageItem）证实，但实现细节（正则/大小写/HTML 转义）未经源码核实。
5. **FlexSearch 运行时语义**：`tokenize:"full"` 在 flexsearch@0.8.212 下对中文/混合文本的分词与评分未做依赖源码或运行验证（本任务禁 build/install/运行）。
6. **穷尽性**：未对全仓做超出 T2 sweep 的独立检索入口搜索；当前证据（唯一 flexsearch 导入 + UI 检查）支持「无第二检索入口」的结论，但不构成形式化穷尽证明。
7. **无性能数据**：全量重扫/FlexSearch 重建/父时间线缓存的实际耗时、内存未测量。

## 10. 方法与免责声明

- 本报告只基于静态源码审读与既有 T2 机械扫描（`sweep-cc-switch.json`）过滤查询；未运行 build/test/install，未联网，未做 git 写操作。
- 行号均为 commit `f748f3ac4afbbbc6554a3ca2bb7e56e4036fedfa` 快照的 1-based 行号；`coverage-cc-switch-sessions.json` 提供逐文件读区间、哈希与缺失区间（partial 文件的 missing_ranges）。
- 「严重度」是本次审计内部的排序而非产品缺陷定级；F 类发现均非运行复现结论。