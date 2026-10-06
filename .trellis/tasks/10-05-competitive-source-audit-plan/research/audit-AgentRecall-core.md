# Audit: AgentRecall 核心存储/加载/MCP（分片 A）

- Query: 对 `Github_src/AgentRecall`（commit `3c7bfa59`）的会话存储 schema、FTS/排序、会话加载、Electron 主进程、元数据链路与 MCP stdio server 做只读源码审计；核实旧报告"MCP 与桌面检索不一致"的说法，并修正事实表中"Python CLI"的形态记录。
- Scope: internal；分片 A（核心存储/加载/MCP）；纯静态源码审计（分片 B/C 的领域不在本报告）。
- Date: 2026-10-06
- Recorded snapshot: `3c7bfa59bc52e5fa39affabcb267dc73ae6858d2`（`git rev-parse HEAD` 只读核实）；工作树除未跟踪 `.codegraph/` 外干净（`git status --porcelain` 只读检查）。
- 方法：`codegraph node --file <path> --offset N --limit 150` 有界逐段精读，每段 ≤150 行；哈希不作为"已读"凭证。机器凭证见同目录 `coverage-AgentRecall-core.json`。
- 禁令遵守：未执行 build/test/install/network/git 写操作，未启动 app；全部为只读读取。

## 覆盖统计

| 口径 | 数量 | 说明 |
|---|---:|---|
| files[] | 9 | 6 个 T1 必读 + MCP server + quota（可选）+ schema（补充） |
| full / partial / missing_ranges | 9 / 0 / 0 | 所有文件均覆盖到逻辑 EOF |
| 总阅读行数（逻辑行） | 10,114 | 全部实际显示阅读，非哈希推断 |
| T1 必读 | 6 文件 / 8,309 行 | sessions.ts 2147、session-loader.ts 1990、main/index.ts 1611、platform.ts 1303、session-activity.ts 626、session-summarizer.ts 632 |
| MCP server | 1 文件 / 523 行 | `bin/agent-recall-mcp.mjs`（grep "mcp" 定位后全文精读） |
| 预算内可选 | 894 行 | `src/core/quota.ts`（全文） |
| schema 补充 | 388 行 | `src/core/store/schema.ts`（全文，回答存储 schema 问题） |

严重度口径：P1 核心功能/数据错误（有源码证据）；P2 可达的一致性/规模/边界风险；P3 低风险或文档漂移；I 信息（正面实现或边界记录）。本分片未发现达到 P1 的问题。

## Findings

### F-AR-01 · P2 · MCP 检索与桌面检索是两套实现、两套排序（旧报告说法核实：成立）

**结论：** `bin/agent-recall-mcp.mjs` 自带 raw SQL 查询，排序为 FTS5 `ORDER BY rank`；桌面 `SessionsStore.searchSessionPage` 是"FTS 命中仅做闸门 + JS 端 smartScore 排序"。两者不是同一函数、不是同一排序，过滤面与标题优先级也不同。

**证据：**
- MCP：`bin/agent-recall-mcp.mjs:96-107`（`session_fts MATCH ?` + `ORDER BY rank`）、`:110-114`（无 query 时 `ORDER BY s.file_mtime_ms DESC`）；`:178-181` 注释自认 "reimplemented in raw SQL ... can't import SessionStore"。
- 桌面：`src/core/store/sessions.ts:1058-1093`（最终 JS 排序）、`:1407-1409`（查询路径候选 SQL 无 LIMIT）、`:1729-1737`（`smartScore = relevance × (0.08 + 0.92 × 0.5^(ageDays/30)) × pinnedBoost`）。
- 过滤差异：桌面默认 `hidden = 0`、favorites/pinned 视图谓词（`sessions.ts:1422-1425`）、subagent 由设置排除（`src/main/index.ts:280-282`）；MCP 无 hidden/is_subagent 谓词（`bin/agent-recall-mcp.mjs:82-115`）。
- source 过滤差异：桌面 claude/codex 家族映射（`sessions.ts:1440-1447`）vs MCP 精确 `s.source = ?`（`bin/agent-recall-mcp.mjs:87-89`）。
- 标题优先级差异：MCP `custom_title || first_question || original_title`（`bin/agent-recall-mcp.mjs:57`）；桌面 `custom_title || original_title || first_question`（`sessions.ts:1661`）。
- 旧报告：`research/competitor-memory-mcp.md:13` 已写明"不是同一条搜索函数……不能只凭'共用 DB'判断"；本审计以当前 commit 源码核实该说法成立。

**反证/界限：** 数据层同库同表——MCP 直接读 app 写的 `session_fts`/`sessions`，且 MCP 无缓存，app 写入即可被看到；桌面内部的 AI assistant `search_sessions` 工具与桌面 UI 走同一 `store.searchSessions`（`src/main/index.ts:1296-1311`）；`migrate_session` 经 bundle 复用真实 `SessionStore`（`bin/agent-recall-mcp.mjs:315-333`）。问题定位是"独立实现 + 独立排序的跨入口漂移面"，不是两套数据库。
**ASG 教训：** 跨入口需要"单源排序/过滤契约 + 交叉验收"；共用 DB 不足以证明一致性。

### F-AR-02 · P2 / I · 桌面查询路径在 JS 侧全量 hydrate + 全量排序（规模敏感）

**证据：** 带 query 时候选取全部满足 WHERE 的 `sessions` 行、无 LIMIT（`src/core/store/sessions.ts:1407-1409`）；逐行 `hydrateRow`（`:1062-1076`）；标签批量取（`:1063,1632-1657`，>900 keys 退化为全表取再 JS 过滤）；全量 `sort` 后 `slice(0, limit)`（`:1079-1086`）；`totalCount` 直接取排序后长度（`:1085`）。

**反证/界限：** 无 query 的 list 路径有 SQL LIMIT 且 pinned 优先（`:1392-1404`）；"全量排序再截断"避免分页截断误排，是正确性优先的取舍；未运行性能基准，不宣称实测慢。
**ASG 教训：** 过滤先于 cap 的正确性保留；同时给候选集设置显式上限或把排序下推到 SQL 层。
### F-AR-03 · P3 / I · MCP 读写同库、hidden 语义与过期注释

**证据：** MCP 以读写方式打开数据库并注释 "Read-write"（`bin/agent-recall-mcp.mjs:365-370`，含 `PRAGMA busy_timeout = 5000`）；写工具 tag/favorite/visibility 直接改同库（`:209-267`）；桌面默认视图过滤 `hidden = 0`（`src/core/store/sessions.ts:1422-1425`）；而 `src/core/store/schema.ts:5` 的注释仍称 MCP 是 "read-only consumer"，与实现矛盾。

**可达性：** `hidden = 1` 的会话仍可被 MCP `search_sessions` 命中并返回标题/摘要（MCP 查询无 hidden 谓词）。
**反证/界限：** 写工具是刻意设计（`:178-181,229-267`）；WAL + busy_timeout 是常规并发写方案；未实测锁竞争；"hidden 对 agent 可见"也可能是有意行为。本项按契约/注释漂移处理，不上升为数据风险。
**ASG 教训：** 跨入口可见性语义要显式契约化；文档注释随实现同步更新。

### F-AR-04 · P2 / I · 元数据更新链路与新鲜度（标题/收藏/项目/"下一步"）

**写入链路：** 渲染层 → IPC（`src/main/index.ts:1410-1424` 的 `title:set`/`favorite:set`/`pin:set`/`hide:set`）→ SessionsStore：`setCustomTitle` = UPDATE + refreshFts（`sessions.ts:461-465`）；favorited/pinned/hidden 为纯 UPDATE（`:467-477`，不进 FTS——非检索字段，合理）。MCP 侧 tag/favorite/visibility 为 raw SQL 直写同库（`bin/agent-recall-mcp.mjs:209-267`），无通知机制，但每次调用现查、无缓存。
**摘要新鲜度（实现完整）：** `setAiSummary` 以写入时 `file_mtime_ms` 记录 `ai_summary_basis`（`sessions.ts:890-900`）；stale = `file_mtime_ms > basis`（`:1698`）；批补候选 = 近 N 天且缺失/过期（`:906-917`）；入口见 `src/main/index.ts:1159-1179`（自动，25 个/次）与 `:1226-1269`（手动，上限 500、3 并发）；与 `src/core/session-summarizer.ts:93-110` 的 freshness/needsBackfill 语义同构。摘要内容会重刷 FTS（`sessions.ts:1140-1150`）。
**项目：** 无独立 projects 表；项目 = `sessions.project_path` 聚合（`schema.ts:18`；`sessions.ts:762-876`），含 codex 任务工作区 label 启发式（`:1929-1937,1963-2031`）。
**"下一步"：** schema 中不存在 next-action/下一步字段（`schema.ts:13-42`；`SessionRow` `sessions.ts:51-85`）。实现层面的"下一步"= resume 动作：`getResumeCommand`（`src/core/platform.ts:485-503`）与 `markResumed`（`sessions.ts:668-670`）写 `last_resumed_at`。
**反证/界限：** 未运行 UI 与 MCP 客户端，写入可见延迟仅有源码结构证据；终端标题同步副作用（`session-title-sync.ts`）属分片外未读。
**ASG 教训：** 摘要 basis 版本化（记录摘要覆盖的会话版本）值得学习；跨入口写入要明确"谁负责刷新 FTS/通知"。

### F-AR-05 · I（正面） · 全文检索真实可用：FTS5 trigram + 摘要并入索引 + LIKE 兜底

**证据：** `schema.ts:184-191`（`session_fts(session_key UNINDEXED, title, first_question, content_text, project_path, tokenize = 'trigram')`），并有 unicode61→trigram 重建迁移（`:304-322`）。索引内容 = 全部消息拼接 + AI 摘要前缀（`sessions.ts:1132-1150`；`schema.ts:360-388` 为同一逻辑的第二份实现）。查询 token 化并加引号（`sessions.ts:1845-1852`，等价 AND 语义）；FTS 异常静默回退（`:1527-1540`）；无 FTS 命中时按 displayTitle/originalTitle/firstQuestion/projectPath/rawId 子串（`:1492-1500`）或 messages LIKE（`:1502-1522`）兜底召回。
**结论：** 支持内容级全文检索（消息正文 + 标题 + 项目路径 + 摘要）。
**代价/边界：** `session_fts` 是普通 FTS 表（非 contentless/external-content），`content_text` 与 `messages.content` 双份存储；trigram 的 CJK 行为未运行验证；LIKE 转义 helper 已存在（`:1859-1861,1503`）。
**ASG 教训：** 摘要进 FTS 提升"概念召回"；多路召回要统一转义与大小写语义（对照 sessiongrep SG-05 的教训）。
### F-AR-06 · I · MCP 工具面与预算边界（8 + 1 tools）

**工具面：** `search_sessions` / `get_session` / `list_projects` / `list_tags` / `get_latest_sessions` / `tag_session` / `toggle_favorite` / `set_visibility`（`bin/agent-recall-mcp.mjs:382-490`）+ 条件注册的 `migrate_session`（bundle 可加载才注册，`:492-513`）；stdio only（`:361-362,515`）。
**预算：** `MAX_RESULTS = 50`、`MAX_MESSAGES = 200`（`:14-15`）、`clamp`（`:39-43`）；`get_session` 返回 `nextOffset` 供分页续读（`:128-136`）。
**失败面：** DB 缺失给明确 stderr 并 exit 1（`:352-358`）；工具错误走 `isError`（`:347-349,410`）；migration bundle 缺失只禁用迁移工具（`:373-380`）。
**边界：** 结果无 freshness/partial 字段；project LIKE `%...%` 未转义 `%`/`_`（`:90-93`）；无 hidden/subagent 过滤。
**反证：** 未运行任何工具或协议交互测试，不评价 SDK 合规性/客户端兼容性。

### F-AR-07 · I · 存储 schema 摘要与两处 upsert 的字段差

**表结构（`src/core/store/schema.ts`）：** `sessions`（28 列：身份/文件快照/PR/用户态/时间戳/6 个 token 度量/AI 摘要 4 列/subagent 2 列，13-42）；`environments`（44-60）；`messages`（62-70，(session_key,message_index) PK，ON DELETE CASCADE）；`message_events`（72-78，毫秒时间戳）；`token_events`（80-91，(session_key,dedupe_key) PK）；`trace_events`（93-106）；`tags`/`session_tags`（108-119）；`session_migrations`（168-177）；`session_sync_bindings`（151-158）；`data_migrations`（179-182）。
**身份：** `session_key = prefix:rawId`（`src/core/session-loader.ts:760`）；live key 映射（`sessions.ts:31-43`）。
**更新语义：** 完整 upsert 会 DELETE+重插 4 个子表并刷新 FTS（`sessions.ts:212-283`）；摘要型 upsert `upsertIndexedSessionSummary`（`:356-459`）不写 messages，且 INSERT/ON CONFLICT 列表**不含 `is_subagent`/`parent_session_id`**（`:369-394`）——若首次经该路径插入会话，将落 schema 默认值（0/NULL）。该路径的实际调用范围超出本分片，标记待验证。
**反证/界限：** `database.ts`/`session-store.ts` 门面未读，连接参数与迁移触发时机细节不在本分片。

### F-AR-08 · I · loader 覆盖面与身份模型

**覆盖面：** 默认迭代器装配 14 源（`src/core/session-loader.ts:1960-1990`）：claude-cli/app、codex-cli/app、可选包（openclaw、hermes、opencode、codewiz、cursor、trae×2、qoder、claude-internal、codex-internal、tclaude、tcodex、codebuddy）。
**防御：** sqlite 源只读打开并做表/列存在性检查（`:25,1163-1192`），对上游 schema 漂移有保护。
**身份/内容模型：** 消息扁平 `(index, role, content, timestamp)`（`:154-164,1216-1218`）；子代理关系在会话级 `parent_session_id`（codex `:87-99`；claude `:1005-1030`；cursor `:1875-1891`）；无消息级 ID/父边/字节区间。
**反证：** 解析保真需 fixture 级验证，本分片未做（`format-adapters.ts` 未读）。

### F-AR-09 · I · 形态修正证据（供事实表 B0）

**结论：AgentRecall = TypeScript/Electron/React 桌面应用 + Node 内建 SQLite（node:sqlite）+ 独立 Node stdio MCP server。不是 "Python CLI"。**

**锚点：**
- `package.json`：electron 42.3.0、react 19、@modelcontextprotocol/sdk、electron-vite；`"main": "out/main/index.js"`；engines `node >= 22.13.0`。
- `src/main/index.ts:1-15`（Electron imports：app/BrowserWindow/ipcMain/Tray）；`:567-586`（BrowserWindow + preload + contextIsolation）；`:1559-1562`（app.whenReady → `new SessionStore(path.join(app.getPath("userData"), "session-search.sqlite"))`）。
- `src/core/session-loader.ts:24-25`（createRequire → `require("node:sqlite")` 的 DatabaseSync）。
- `src/core/store/schema.ts:5`（WAL 注释）+ `:184-191`（FTS5 trigram DDL）。
- 全仓 `*.py` 文件数 = 0（文件系统扫描，排除 .git/.codegraph）。

**更正边界：** Python 仅作为可选运行时助手出现——macOS/Linux 的 Codex 配额请求走 python3 子进程（`src/core/quota.ts:646-733`）；远程文件写入/路径检查在远端执行 python3（`src/main/index.ts:1122-1143`）。这不能支撑"Python CLI"的形态描述。

**T2 交叉验证（只读引用 sweep-AgentRecall.json）：** `bin/agent-recall-mcp.mjs` 命中 `surface_mcp=18`、`sql_dynamic=6`；`sessions.ts` 命中 `sql_dynamic=14`；`schema.ts` 命中 `index_storage=19`。机扫方向与本次精读一致（MCP 独立 SQL、store 动态拼接 SQL、FTS/索引集中在 schema）。
## 必答问题汇总

**Q1 会话存储 schema、FTS/规则排序的真实实现；query 候选是否全量 hydrate；可否全文检索？**
- schema：见 F-AR-07（9 张会话域表 + FTS；`schema.ts:13-191`）。
- FTS：trigram 分词器，索引 title/first_question/全量消息(+摘要)/project_path（F-AR-05）。
- 排序分两层：SQL 层（无 query：`pinned DESC` + activity DESC + LIMIT，`sessions.ts:1392-1404,1886-1908`）与 JS 层（有 query：smartScore 30 天半衰期 + 置顶加成，`:1729-1737`）。**FTS rank 在桌面不参与排序**，只做命中闸门（`:1061,1067-1074`）。
- 候选 hydrate：**是**——带 query 时全部可见候选行都被 hydrate + 排序后才截断到 limit（`:1407-1409,1079-1086`）。
- 全文检索：**可以**（内容级 FTS + LIKE 兜底；F-AR-05）。

**Q2 MCP 工具面与桌面检索是否同一函数/同一排序？**
- **否。** MCP 是独立 raw SQL（`ORDER BY rank` / mtime），桌面是 SessionStore + JS smartScore；title 顺序、hidden/subagent 过滤、source 映射均不同（F-AR-01 全部锚点）。旧报告"不一致"的说法成立。桌面内部（UI 与 AI assistant）共用同一 `store.searchSessions`（`main/index.ts:1200,1296-1311`）。

**Q3 元数据更新链路与新鲜度；远程同步边界。**
- 链路/新鲜度：见 F-AR-04（IPC→Store；title/summary 触发 FTS 刷新；basis 版本化；favorited/pinned/hidden 不触发 FTS；无"下一步"字段，resume 记 `last_resumed_at`）。
- 远程边界（源码证据）：远程会话与本地**同库同 schema**（`environment_id` 区分；`schema.ts:17,226`）；元数据由 SSH 同步写入（`main/index.ts:788-800`）；**详情懒加载**：桌面按需 SSH 拉 payload → `upsertIndexedSession` 落本地 messages（`main/index.ts:463-486`）；未 hydrate 时消息页直接 SSH 直读（`:1206-1218`）、trace-events 直接空数组（`:1219-1224`）。
- 推演边界（未运行验证）：MCP server 不含任何 SSH/远程逻辑（bin 仅 db+SDK 导入），对未 hydrate 的远程会话 `get_session` 只能看到 0 条本地 messages（`bin/agent-recall-mcp.mjs:117-138`）——同一会话在桌面与 MCP 的"详情完整度"可能不同，除非桌面先前打开过。
- 未覆盖：`remote-sync.ts`/`remote-session-loader.ts` 协议细节属分片外。

**Q4 与 ASG 对照：强在哪、弱在哪、该学/不该学什么。**

| 维度 | AgentRecall 强点（证据） | AgentRecall 弱点/边界（证据） | ASG 该学 | ASG 不该学 |
|---|---|---|---|---|
| 检索 | trigram FTS + 摘要入索引 + 标题/路径/正文多路召回（F-AR-05） | 桌面/MCP 双实现双排序漂移（F-AR-01）；桌面查询路径全量 hydrate（F-AR-02） | 多路召回；摘要入索引（概念召回） | 双实现排序、无共享契约 |
| 加载 | 14 源加载器；sqlite 源只读 + 列存在性防御；subagent 会话级建模（F-AR-08） | 消息扁平（无消息 ID/父边/字节区间） | 多源覆盖与 read-only 防御 | 扁平消息丢失结构身份 |
| 元数据 | 用户态迁移合并策略（OR 布尔/取新时间戳，`sessions.ts:510-655`）；title/summary 刷新 FTS | summary upsert 路径字段集合不一致（F-AR-07） | 用户态迁移保序 | 同表两套 upsert 字段面 |
| MCP | 写工具幂等 + isError；`nextOffset` 分页；migrate 复用真 store | hidden 仍可命中；无 freshness/partial 字段；远程不 hydrate（F-AR-01/03/06, Q3） | 幂等写 + 分页契约 + 复用真 store | 独立再造一套查询/排序 |
| 新鲜度 | `file_mtime_ms` vs `ai_summary_basis` 显式 stale（F-AR-04） | MCP 输出无新鲜度信号 | 显式 staleness 表达 | 新鲜/过期不可区分 |

**Q5 形态修正（B0 用）：** 见 F-AR-09——AgentRecall 是 TS/Electron/React + node:sqlite + 独立 Node MCP server，非 Python CLI；Python 仅出现在可选运行时助手路径（quota/remote 脚本）。

## Top-5 发现

1. **F-AR-01（P2）**：MCP 与桌面检索是两套实现、两套排序——旧报告"不一致"成立（`bin/agent-recall-mcp.mjs:96-114,178-181` vs `sessions.ts:1058-1093,1729-1737`）。
2. **F-AR-02（P2/I）**：桌面查询路径对全部可见候选行做 hydrate + JS 全量排序后才截断（`sessions.ts:1407-1409,1079-1086`）。
3. **F-AR-04（P2/I）**：元数据链路与摘要新鲜度体系（IPC → Store → FTS/basis；`sessions.ts:461-477,890-917`），且无"下一步"字段（resume 记 `last_resumed_at`）。
4. **F-AR-05 + F-AR-07（I）**：存储/检索真实实现——FTS5 trigram + 全文双份存储 + 9 表 schema（`schema.ts:13-191`）。
5. **F-AR-09（I）**：形态修正证据齐备（Electron/TS/node:sqlite；全仓 0 个 .py）——事实表 B0 应改"Python CLI"为"TypeScript/Electron 桌面应用 + Node SQLite + Node MCP 服务"。

## Caveats / Not Found

- **未运行证据：** 未 build/test/install/network/启动 app；性能、MCP 协议兼容性、解析保真、远程 SSH 行为均无运行验证，本报告仅陈述源码结构可证事实。
- **未读清单（本分片边界）：** `src/core/store/database.ts`、`src/core/session-store.ts`、`src/core/indexer.ts`、`src/core/format-adapters.ts`、`remote-sync.ts`、`remote-session-loader.ts`、`session-title-sync.ts`、`mcp-server.test.ts`、`renderer/App.tsx` 等；相关结论已标"推演/未验证"。
- **quota.ts：** 已全文阅读，但其网络/代理路径未运行。
- **证据时点：** 仅对 commit `3c7bfa5` 与 `coverage-AgentRecall-core.json` 所列 SHA256 负责。
- **所有权：** 本 worker 仅创建 `coverage-AgentRecall-core.json` 与 `audit-AgentRecall-core.md`，未修改任何其它文件；`sweep-AgentRecall.json`、`competitor-memory-mcp.md` 等为只读引用。