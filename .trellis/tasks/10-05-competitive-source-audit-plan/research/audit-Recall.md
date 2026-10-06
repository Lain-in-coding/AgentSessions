# Recall 竞品源码审计（T1 全文精读）

- 审计对象：`C:\AgentSessions\Github_src\Recall`，commit `22625bf8979a185d9913c1bfb20290ac466d492b`（包版本 0.3.0，`Cargo.toml:3`）。
- 日期：2026-10-06。任务：`.trellis/tasks/10-05-competitive-source-audit-plan`。
- 方法：`codegraph node --file --offset --limit`（每次 ≤120 行、逐段记录区间）全文精读 35 个文件 + 2 个文件的有界区间；T2 机械扫描（`sweep-Recall.json`，154 文件）只按需过滤、不作阅读凭证。
- 证据等级：**纯静态源码推导，未 build / test / 运行 / 联网**；未动态复现的点均标注“静态推导”。旧结论（`competitor-memory-mcp.md:18-28`）仅作待验证假设，凡与本次精读冲突处以本次源码为准。
- 覆盖统计：37 个文件（35 full / 2 partial）、13,244 行可见阅读；全部 SHA256 前后一致；工作树无跟踪改动（仅预先存在的未跟踪 `.codegraph/`）。明细见 `coverage-Recall.json`。

## 1. 系统形态（可验证的入口链）

- 入口 `src/main.rs:3-12` → `recall::init()`（`src/lib.rs:32-34`，注册 sqlite-vec 扩展）→ `cli::run`（`src/cli.rs:187-264`）。
- CLI 命令面：`search / session(list|show|export|share|resume|open|handoff) / sync / info / usage / export / import / share init / skill install / extension(list|install|remove|upgrade) / completions`，另有隐藏 `__background-worker` 与 4 个 `__bench-*`（`src/cli.rs:19-131,194-260`）；**没有任何 MCP 服务**（全仓 `mcp` 仅出现在 `src/adapters/codex.rs:1168,1192` 的测试字符串）。
- 无子命令时进入 TUI（`src/cli.rs:260`）；TUI 在 release 下启动后台嵌入 worker（`src/tui/runner.rs:28-31`），搜索由独立线程执行（`src/tui/search_worker.rs:38-45`）。
- 扩展机制：未知子命令转发到 `data_dir/recall/extensions/bin/recall-<name>`（`src/extension.rs:198-213`、`src/cli.rs:254-259`）；官方目录经 HTTPS 拉取、sha256 校验后安装（`src/extension.rs:13,215-268,406-412`）。
- 恢复/交接双入口：CLI（`session resume/open/handoff`）与 TUI（Ctrl-R / Ctrl-O / 详情页 `h`）。两者共用 `handoff.rs` 与 `session_action.rs`。

## 2. 数据模型与 DB schema

`SCHEMA_VERSION = 10`（`src/db/schema.rs:3`），`init()` 顺序执行 v1–v10 迁移（`schema.rs:14-47`）。核心表：

| 表 | 关键列/约束 | 锚点 |
|---|---|---|
| `sessions` | `id` PK（UUIDv4，非稳定）、`UNIQUE(source, source_id)`、title/directory/started_at/updated_at/message_count/entrypoint/custom_title/summary/duration_minutes/source_file_path/is_import/repo_* | `schema.rs:52-67,270-306` |
| `messages` | 自增 id、FK→sessions ON DELETE CASCADE、role/content/timestamp/seq（无消息 UUID/父边） | `schema.rs:69-78`、`src/types.rs:80-87` |
| `messages_fts` | FTS5 外部内容表，`tokenize='unicode61'`；仅 INSERT/DELETE 触发器 | `schema.rs:80-93` |
| `message_vec` | `vec0`，`message_id` PK，`embedding float[384]`（维度写死） | `schema.rs:95-98` |
| `session_embedding_state` | pending/processing/done/failed + units/last_error | `schema.rs:100-111` |
| `usage_events` / `usage_session_state` | token 五元组、`token_source∈{observed,derived,estimated}`、`UNIQUE(session_id,event_key)`、parser_version | `schema.rs:129-181` |
| `session_events` / `event_session_state` | 结构化工具/命令/检索事件 + parser 状态 | `schema.rs:217-257` |
| `background_job_state` | 单行 job 状态（同步/嵌入进度） | `schema.rs:113-118` |

- 连接与并发：单连接 `Store { conn }`；`PRAGMA journal_mode=WAL; busy_timeout=5000; foreign_keys=ON`（`src/db/store.rs:81-96`）；写入用 `unchecked_transaction`（`src/db/session_store.rs:167,193,209`）。
- 持久化语义：`replace_session_with_usage_and_events` = 先 `delete_session_data_tx` 再插入（`session_store.rs:182-206,477-499`）；每次刷新生成**新 UUID**（`src/sync.rs:456`）；usage 用 `ON CONFLICT(session_id,event_key) DO UPDATE`（`session_store.rs:568-582`）；持久化后把嵌入状态重置为 pending/done（`session_store.rs:645-679`）。
- 迁移测试覆盖 v6–v10 与幂等（`schema.rs:341-520`）；v10 会删除既有 grok usage 行（数据纠正型迁移，`schema.rs:312-328`）。

## 3. Provider 覆盖与差异

注册 11 个适配器（`src/adapters/mod.rs:173-187`）。恢复命令逐条核对（机械核对到 `fn resume_command` 行）：

| provider | resume 命令 | app 打开 | 备注 |
|---|---|---|---|
| claude-code | `claude --resume <id>`（`claude_code.rs:34-38`） | — | usage v5 / event v2（`:23-24`） |
| codex | `codex resume <id>`（`codex.rs:33-37`） | `codex://threads/<id>`（`:40-42,81-101`） | usage v4 / event v1（`:22-23`） |
| gemini | `gemini --resume <id>`（`gemini.rs:25-29`） | — | usage 版本存在（`:32-33`） |
| pi | `pi --session <id>`（`pi.rs:33-37`） | — | |
| grok | `grok --resume <id>`（`grok.rs:35-39`） | — | 唯一实现 `prune()`（`:72-77,129-141`） |
| opencode | `opencode --session <id>`（`opencode.rs:55-59`） | — | |
| antigravity | `agy --conversation <id>`（`antigravity.rs:31-35`） | — | |
| copilot | `copilot --resume=<id>`（`copilot.rs:32-36`） | — | |
| cline / kiro / cursor | 无（返回 None；`cline.rs:25-27`、`kiro.rs:18-20`、`cursor.rs:63-65`） | — | 只能检索/分享，不能在本机续跑 |

Claude Code 深读要点（`claude_code.rs` 全文）：三处来源（`~/.claude/sessions/*.json` 索引、`projects/**/*.jsonl`（**递归包含 subagents 嵌套目录**，测试 `:1081-1099`）、`transcripts/*.jsonl`）；仅 `type∈{user,assistant}`；`isSidechain/isCompactSummary/isMeta` 剔除正文但**保留 usage**（`:403-405,420-440`，测试 `:1183-1209`）；usage 按 `requestId+messageId` 去重并取 max（`:552-581`）；`updated_at` 一律等于文件 mtime（`:323`）；持续时间=首末时间戳差（`:313-316`）；项目目录名 dash 解码是启发式（`:630-648`）。

Codex 深读要点（`codex.rs` 全文）：扫 `~/.codex/sessions` + `archived_sessions`，文件名尾部 UUID 即 source_id（`:138-197`）；`session_meta` 取 id/cwd/model_provider，`turn_context` 取 model；消息来自 `event_msg(user_message/agent_message)` 与 `response_item(message role=assistant)` 并抑制连续重复 assistant 文本（`:833-845`）；usage 用 total/last 双口径求 delta、`derived` 来源、fork 子会话继承基线跳过、陈旧回退保护（`:609-796`，测试 `:1429-1459`）；`updated_at` 覆盖为 mtime（`:183-185`）；source_file_path 不写库（`:427`）。

## 4. 索引、同步新鲜度与并发

- 增量算法：`file_scan.rs` 统一实现。对每个现行文件：stat mtime（毫秒，`:124-129`，**不看 size**）→ 已存在则总是刷新 source_file_path 并清除 is_import（`:76-90`）→ `since_ts` 过滤（`:92-97`）→ 仅当 `updated_at==mtime` 且 usage/event parser 状态（版本 + source_updated_at）都当前才跳过，否则重解析（`:99-118`；`sync_state.rs:3-23`）。
- 同步决策：`decide_existing_session_action`——usage_only 只回填；未变且无 parser 升级则 Skip/BackfillOnly；有变化或 force 则 Refresh（`sync.rs:645-678`）。变化的判定 = 消息数变 / 目录与文件路径变 / `raw.updated_at` 与旧值不同（`sync.rs:409-414`）。
- 失败保留：适配器扫描失败只打印错误并跳过该 provider，旧索引继续服务（`sync.rs:273-279,286-294`）；单文件解析失败 debug 后跳过（`claude_code.rs:289-295`、`codex.rs:175-181`，测试 `codex.rs:1590-1613`）。
- 删除对账：不对账。除 grok 的子会话清理（`grok.rs:129-141`）与 `excluded_paths` 主动清除（`sync.rs:226-235,704-739`）外，`prune()` 默认空（`adapters/mod.rs:40-42`）。源文件被删除/归档后，其会话行不会从库中消失。
- 并发：CLI/sync 单线程顺序执行（`sync.rs:196-246`）；TUI 主线程 + 搜索线程各持一个 SQLite 连接（`search_worker.rs:56-74`），搜索请求合并为最新（`:78-91`）；嵌入 worker 为独立进程，用 `fs2` 排它锁保证单实例（`semantic.rs:14-24,120-140`）；WAL+busy_timeout 承担跨连接/跨进程并发（`store.rs:89-93`）。用法仪表盘的首次 sync 在主线程执行（`runner.rs:100-110`）——静态看会阻塞 UI 刷新。

## 5. 检索实现（FTS + sqlite-vec）与过滤顺序

- 合并器：`hybrid_search`（`search.rs:76-109`）：`fetch_size=limit×multiplier` → FTS 命中 + （可选）向量命中 → `rrf_merge(k=10)`（`:90,263-293`）→ 取前 limit → 批量载入 session。CLI 默认 `limit=20, multiplier=3`（`query.rs:51`）；TUI 为 `200×3`（`search_worker.rs:9-10,107-115`）。
- FTS 分支（`:111-150`）：`WHERE messages_fts MATCH ?` **在 WHERE 中先套 sources/time/directory/repo 过滤**（`:133`），再 `GROUP BY session, ORDER BY MIN(rank) LIMIT n`；snippet=命中会话最优行内容的**前 200 字符**（`:123`，SQLite min/max 裸列取最优行语义）。
- 向量分支（`:152-190`）：`message_vec MATCH ? AND k = limit×5`（`:158-168`），随后 JOIN sessions 才应用同类过滤（`:173`）并按 session 聚合 `MIN(distance)`。
- 查询清洗 `fts5_escape`（`:295-308`）：按空白分词，只保留字母数字/下划线字符，全部小写，`OR` 连接；无短语、无前缀、无 bigram/trigram、无 fuzzy 回退。
- 过滤字典：source IN、`started_at >= cutoff`（时间用**开始时间**而非更新时间，`:236-239`）、目录精确或 `dir/%` 前缀（`:241-250,259-261`）、repo 精确列匹配（`:251-256`）。

## 6. 嵌入来源与失败行为

- 模型：`intfloat/multilingual-e5-small`（384 维），经 `hf-hub` 从 HuggingFace 拉取，**首次使用需要联网**；candle BERT 推理；query/passage 前缀；512 token 截断；mean-pooling+L2 归一化；设备 CUDA→Metal→CPU（`embedding.rs:9,18-37,39-49,88-98,110-133`）。
- 文本单元：嵌入只针对 **user 角色的消息**且 `LENGTH(content)>2`（`semantic_store.rs:28-58`）；文本 = `"{title}: {content}"`，超 500 字符截断（`semantic.rs:115-118`）；批次 8（`:11,96-110`）。
- 队列：新/刷新会话被置 `pending`（`session_store.rs:645-679`）；worker 每轮只 claim 一个 `pending` 会话（`semantic_store.rs:73-95`）；完成置 done、失败置 failed+last_error 并**退出 worker**（`semantic.rs:59-82`）；进度写入 `background_job_state` 供 TUI 显示（`semantic.rs:53-57,108-109`）。
- 查询侧失败分层：
  - CLI：无任何 done/processing 会话 → 直接不加载模型、静默 FTS-only（`query.rs:117-120`）；模型加载失败 → 打印 “Semantic unavailable” 后 FTS-only（`:121-134`）；查询嵌入调用失败 → 硬错误（`:125`）。
  - TUI：先返回 FTS 结果、再尝试 hybrid；模型加载/嵌入任一失败 → 该进程内永久 `embedding_unavailable`，hybrid 阶段以状态消息 “Semantic unavailable - using text search only” 降级（`search_worker.rs:117-152`）。
  - 评测：逐查询 `embed_query(...).ok()` 吞错 → 单条查询静默降级为 FTS-only（`bench.rs:441-444`）。

## 7. handoff / 恢复契约（4 目标、构造、preview vs 执行、预算）

- **4 个交接目标**：`codex`、`grok`、`claude-code`、`opencode`（`handoff.rs:13-18`）。
- **内容构造**：`build_prompt = "Use this Recall indexed session transcript as context for a new session. This is a handoff, not a native resume.\n\n" + transcript::render_plain(session, messages)`（`handoff.rs:20-25`）；`render_plain` 输出会话头（title/id/source/source_id/directory/date/messages 数）+ 逐条 `## Role [seq]` 全文（`transcript.rs:3-28`）。**没有任何截断、token/byte 预算、证据标记或分段/落盘机制**。
- **命令构造**：codex→`codex <prompt>`；grok→`grok <prompt>`；claude-code→`claude <prompt>`；opencode→`opencode run -i <prompt>`（`handoff.rs:27-38`）。prompt 作为**单个 argv 元素**传给 `Command::args`（`session_action.rs:40-49`），不经 shell，因此无注入面；但进程参数长度受 OS 限制（Linux ARG_MAX、Windows 32767 字符）约束。
- **执行**：CLI `--print-prompt` 打印完整 prompt 到 stdout 而不执行（`session.rs:654-657`）；默认执行时 cwd 仅在目录存在时设置（`session.rs:659-665`，测试 `:962-977`）。TUI：详情页 `h` → 目标选择 → `start_handoff_confirmation` 预构建 prompt 与命令（`app.rs:1473-1515`）→ 确认框（`popups.rs:400-474`）→ Y/Enter 写入 `exec_on_exit` 并退出，由 runner 在还原终端后执行（`app.rs:1517-1525`、`runner.rs:125-127,132-159`；Unix 用 `exec` 替换进程）。
- **preview vs 实际**：TUI 弹窗只显示 `command.display()` 截断到 `width-14` 字符的一行文本（`popups.rs:429-441`；display 为程序名+参数空格拼接、无引号，`adapters/mod.rs:162-171`），**无法在 TUI 中核对完整 prompt**；实际执行的是完整 argv（含全量 transcript）。CLI 侧完整预览仅 `--print-prompt`。
- **imported 语义**：导入会话 `is_import=true`（`import.rs:203`）在 CLI/TUI 都禁止 resume/open（`session.rs:608-610`、`app.rs:1450-1454`，测试 `app.rs:3344-3368`）；但 handoff 允许（handoff 从库内文本重建，不依赖本机源文件；CLI 与 TUI 一致，测试 `app.rs:3372-3390`）。
- 原生 resume 命令：`claude --resume` / `codex resume` / `pi --session` / `grok --resume` / `opencode --session` / `copilot --resume=` / `agy --conversation`（见 §3 表）；`cline/kiro/cursor` 无恢复命令。

## 8. 发现清单（严重度 / 证据 / 反证）

严重度口径：P1=可能造成明显错误或不可用且无兜底；P2=有条件触发的正确性/边界风险；P3=较低优先级或静态证据有限。所有条目**未动态复现**。

### R-01 · P1 · handoff 全量 transcript 直塞 argv，无预算/证据边界
- 证据：`handoff.rs:20-25`（整段原文 + 固定前言）；`transcript.rs:3-28`（所有消息全文，无截断）；`session.rs:651-652,659-660`（一次性构建并执行）；`session_action.rs:40-54`（单参数块整体传入）。目标 CLI 只能拿到重排原文，无“哪些内容被裁剪/来源在哪”的证据标记。
- 反证：prompt 以 argv 传参无 shell 注入；`--print-prompt` 提供完整人工预览；小会话（绝大多数）不受影响。E2BIG/Windows 长度上限为 OS 语义推导，未复现。
- 边界：仅对超长会话或需要可审计证据的场景构成 P1；与 ASG 的 byte budget/证据边界形成最直接对照。

### R-02 · P2 · 向量分支 k 截断先于过滤，强过滤时会漏召回
- 证据：`search.rs:158-168` 在 vec0 上 `MATCH + k=fetch_k`（CLI 路径 k=300）先取全局 KNN，`:161-175` 才 JOIN sessions 套 source/time/directory/repo 过滤；过滤列（s.source 等）不在 `message_vec` 中，无法下推进 KNN 子查询。对比 FTS 分支过滤在 WHERE 前置（`:133`）。
- 反证：FTS 分支过滤正确，RRF 仍能给出 FTS 命中；无过滤或弱过滤时向量召回正常；sqlite-vec 0.1 引擎语义为文档+静态推导，未动态压测。
- 边界：影响“过滤条件下的语义召回率”，不会报错；与 sessiongrep SG-01 同类，但 Recall 仅向量侧受损。

### R-03 · P2 · CJK 无分词：unicode61 整串 token + OR 清洗，中文短查询易漏
- 证据：`schema.rs:80-85`（`tokenize='unicode61'`，无 bigram/trigram 配置）；`search.rs:295-308`（按空白切词、`is_alphanumeric` 保留中文、OR 连接）。连续中文段落会成为单个 token，查询串若不等于语料中的连续片段即无 FTS 命中；无 fuzzy/子串兜底（全文件确认）。
- 反证：多语言 E5 向量分支可语义兜底（需嵌入就绪）；含空格/标点的中文片段或整段复制仍可命中；未做中文实测。
- 边界：静态可断言的是“索引与查询分词规则”，实际漏召回率未测。

### R-04 · P2 · 无孤儿对账：源文件删除/迁移后索引永久陈旧
- 证据：`sync.rs:196-246` 只遍历本轮 scan 发现的会话做决策；`adapters/mod.rs:40-42` 默认 `prune()` 为空；`file_scan.rs:71-118` 仅处理现行条目。只有 grok 子会话（`grok.rs:129-141`）与 `excluded_paths`（`sync.rs:704-739`）会删除行。搜索/列表/统计将一直包含已删除文件中的会话。
- 反证：对“历史会话召回”而言，保留已删源文件可能是产品意图（Recall 的卖点之一是历史留档）；删除源文件后也确无对应源可校验。未读 README 全文，无法确认文档意图。
- 边界：若产品定位是“可长存的历史索引”，此条应降级为设计取舍而非缺陷；本审计按“索引新鲜度”要求记录。

### R-05 · P2 · 嵌入失败/中断无自动重试与恢复
- 证据：`semantic_store.rs:73-95` 只 claim `pending`；失败后状态置 `failed`（`:147-158`）且 worker 直接退出（`semantic.rs:65-82`）；没有把 failed 重新入队或重试的循环（全文确认）；进程在 `processing` 中途崩溃会留下永久 processing（无超时/接管逻辑，`:73-95`）。TUI 单次嵌入失败后进程内永久降级（`search_worker.rs:121-136`）。
- 反证：下次该会话被刷新/force sync 时会重新置 pending（`session_store.rs:645-679`），实际可恢复但需事件触发；CLI 对加载失败有可见提示（`query.rs:131-133`），不是全静默。
- 边界：影响“失败即长期缺语义召回”，非数据损坏。

### R-06 · P2 · RRF 等分无 tiebreak，offset 分页在无稳定序上
- 证据：`search.rs:291` 仅 `sort_by(score desc, partial_cmp)`；来源是 `HashMap::into_iter`（`:278-289`），同分次序随机化；列表搜索路径用 `skip(offset)/take(...)`（`session.rs:353-359`），JSON `next_offset` 基于行数（`:815-819`）。跨进程重复同查询时同分结果次序可能变化，分页可重复/跳过。
- 反证：不同 rank 的 RRF 分数严格递减（`1/(10+rank+1)`），仅多来源同时命中且排名对偶时等分；小数据集常见结果同分概率低。未动态复现。
- 边界：影响结果列表可重现性与分页一致性，不影响命中集合本身。

### R-07 · P3 · 会话主键非稳定：每次刷新重建 UUID
- 证据：`sync.rs:456` 每次构建 `Uuid::new_v4()`；`session_store.rs:194,477-499` 先删旧行再插新行；import 同样用新 UUID（`import.rs:183`）。
- 反证：`UNIQUE(source,source_id)` + `get_session_by_source_id`（`session_store.rs:245-262`）提供稳定寻址路径，CLI 也支持 `--source/--source-id`（`session.rs:667-694`）；外部“按 recall id 引用”的破坏真实但可绕开。
- 边界：影响 exports 中的 id 引用、TUI 选中项跨 refresh 等；未见内部逻辑直接依赖 id 跨刷新稳定。

### R-08 · P3 · TUI 交接预览被截断、display 无引号，preview≠执行内容
- 证据：`popups.rs:428-441` 命令文本截断到弹窗宽度-14；`adapters/mod.rs:162-171` display=空格拼接、无转义；`app.rs:1499-1525` 执行的却是完整 argv。
- 反证：CLI `--print-prompt` 提供完整预览（`session.rs:654-657`）；命令显示仅用于人读，不影响真正执行。
- 边界：体验/可审计性问题；无注入风险（argv 直传）。

### R-09 · P3 · 搜索 snippet 是消息开头 200 字符，未必包含命中处
- 证据：`search.rs:123` `SUBSTR(m.content,1,200)`；长消息中命中位于中后部时，提示文本不含查询词（SQLite min/max 规则仅保证取自最优行，不保证命中窗口）。
- 反证：多数消息 <200 字符时足够；终端/JSON 同时输出标题与目录。未动态验证比例。
- 边界：预览误导风险，非召回错误。

### R-10 · P3 · 时间过滤用会话开始时间；CC 目录解码启发式有歧义
- 证据：`search.rs:236-239` 用 `s.started_at >= cutoff`（老会话近期活跃会被排除）；`claude_code.rs:630-648` 把目录名 dash 还原为路径（`-tmp-my-project` 无法区分 `/tmp/my-project` 与 `/tmp/my/project`）。
- 反证：时间过滤语义可能就是“会话发生范围”；目录解码是兜底（优先使用 sessions 索引/JSONL 内 cwd，`:301-312`）。
- 边界：均为有意的低成本近似；未动态验证影响到多少会话。

### R-11 · P3 · 配置解析失败静默回退默认
- 证据：`config.rs:102-104` `Self::load().unwrap_or_default()`；配置文件损坏时会以空配置运行（所有源启用、无 excluded_paths）且不提示。
- 反证：`build_path_excluder` 对非法 glob 显式报错（`:136-146`），并非所有配置错误都被静默；只有读取/反序列化失败走默认。
- 边界：静态确认；TUI 设置分支未读区间（`app.rs` 261-429 等）可能显示配置状态，未验证。

### R-12 · P3 · FTS 表缺少 UPDATE 触发器（防御性缺口）
- 证据：`schema.rs:87-93` 仅 `messages_ai/messages_ad`；若未来出现 `UPDATE messages SET content`，FTS 将静默失配。
- 反证：当前所有写入路径为 delete+reinsert 或纯 INSERT（`session_store.rs:182-206,535-549`、`import.rs:271-278`），不存在 UPDATE content 调用；现状无实际失配。
- 边界：纯前瞻性备注。

## 9. 与 ASG 对照

前提说明：ASG 侧特征来自本任务既有研究文档（`competitor-cli-resume.md`、`competitor-memory-mcp.md`、sessiongrep 审计等对 ASG 的既有陈述），**不是** Recall 源码证据；Recall 侧全部基于本文档 §1–§8。

**Recall 强在哪（ASG 需正视）**
1. provider 广度与矩阵化：11 家适配器、统一 trait + parser 版本状态机（`adapters/mod.rs:22-47,173-187`；`sync_state.rs:3-58`），且 resume/app 能力逐家显式声明。
2. 语义检索产品化程度：内置后台嵌入队列 + 状态可视化 + RRF 混合检索（`semantic.rs:26-63`；`search.rs:76-109`），无需外部服务。
3. “给下一步动作”明确：4 目标 handoff 一键可用 + TUI 确认→退出后执行（`handoff.rs:13-38`；`runner.rs:125-127`），对“找到会话后继续干活”的闭环比继续输出 pack 更直接。
4. 恢复的可审计性细节：imported 会话禁止 resume/open 但允许 handoff（`session.rs:608-610`；`app.rs:3344-3390`）——跨机器内容不被误当成原生会话。
5. 内置评测 harness：dataset + Hit@5/@10 + MRR（`bench.rs:305-360,406-478`），把检索质量当回归对象。
6. 扩展供应链校验：sha256 + manifest 一致性 + protocol/min_recall（`extension.rs:231,302-336,338-365`）。

**Recall 弱在哪（ASG 不应倒退的点）**
1. 预算与证据边界缺失（R-01）：prompt 无 token/byte 上限、无来源标记；ASG 的预算 + source 证据 + native resume/handoff 边界应保留。
2. CJK/过滤/排序的正确性机制明显弱于 ASG：unicode61 整串 token（R-03）、向量 k 先于过滤（R-02）、等分无 tiebreak（R-06）——ASG 的 CJK bigram 与 prefilter SQL 不应被“竞品也这么做”说服倒退。
3. 失败语义偏“静默降级”：多处把失败折成 FTS-only 或空结果（`query.rs:117-134`、`search_worker.rs:121-152`、`bench.rs:441-444`），与“失败≠零命中”的契约相冲突。
4. 索引新鲜度模型只有 mtime+parser 状态，无删除对账（R-04）；无 MCP/程序化工具面（有 JSON CLI）。
5. 每次刷新重建会话 id（R-07），对“稳定引用/证据定位”不友好。
6. 首用联网下载模型（`embedding.rs:120-133`）——若 ASG 承诺离线优先，不应跟随。

**ASG 该学（可执行）**
- 学“任务导向的工具命名与确认流”：像 `resume/open/handoff` 一样把建议动作显式化，但输出必须是带预算与来源的证据包（保持 ASG 既有契约）。
- 学后台嵌入队列的状态机形态：`pending/processing/done/failed + background_job_state`（`semantic.rs:53-57`、`schema.rs:100-111`），但补上失败重试/崩溃接管（R-05）后才是完整版。
- 学内置评测 harness（dataset + Hit@k + MRR）作为回归基线（`bench.rs:305-360`）。
- 学“同源多入口共用同一 Application 契约”：Recall 的 CLI/TUI/bench 共用 `SearchEngine`（`search.rs:71-109`、`bench.rs:8-13`），没有第二套 SQL 排名。
- 学扩展供应链校验的三件套（sha256 + manifest 自证 + protocol/min_recall 门），仅在 ASG 将来做插件化时。

**ASG 不该学**
- 不该照抄无预算的 argv 直塞 handoff（R-01）；不该让 RRF 无阈值/无 tiebreak（R-06）；不该引入“失败即静默 FTS-only”路径；不该用可再生的 UUID 充当跨刷新证据 id（R-07）；不该把 mtime-only、无删除对账当索引新鲜度模型（R-04）；不该为了语义检索把首用路径绑定到联网下载。

## 10. 残余未决与边界

- 未 build/test/运行/联网（任务禁止）；R-01/R-02/R-03/R-06 均未动态复现。
- `src/tui/app.rs` 仅精读 660/3701 行（缺失：261-429、475-1079、1110-1199、1275-1399、1550-3299、3400-3701）；`src/tui/ui/popups.rs` 仅精读 75/567 行（缺失 1-399、475-567）。两个 partial 文件在 `coverage-Recall.json` 中列明 missing_ranges。
- 其余 9 个 provider 适配器（gemini/pi/grok/opencode/antigravity/copilot/cline/kiro/cursor）未逐行；仅机械核对 `resume_command`/`prune`/版本声明。
- 未读：`share/`、`skill_audit.rs`、`info.rs`、`usage.rs`、`export.rs`、`integration/`、`tui/ui/*` 其余文件等（不在 T1 清单内）；README/AGENTS 未作为证据使用。
- sqlite-vec `k` 与 JOIN 过滤的执行计划语义按引擎文档+SQL 结构推导，未做压力测试；CJK 漏召回率未实测。
- import/export 往返仅由源码测试声明（`import.rs:408-424`），本次未运行。
- grok prune 仅做 grep 级核对（`grok.rs:72-77,129-141`），未逐行读 grok.rs。

---

**交付物**：本文件（`audit-Recall.md`）与 `coverage-Recall.json`。除这两个文件外，审计过程未写入或修改任何仓库文件；所有结论为静态源码推导。