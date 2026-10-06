# 竞品源码审计：fast-resume（T1 精读 + T2 过滤校验）

- Query: fast-resume 如何发现/索引会话（索引格式、增量、新鲜度、并发）；resume/launch 如何构造（shell、ID 转义、cwd/env）；provider 覆盖与归一化边界及失败行为（fail-open/fail-closed）；搜索/查询能力（全文、排序、分页）；与 ASG 的对照（强/弱、该学/不该学）。
- Scope: internal；只读 `C:\AgentSessions\Github_src\fast-resume`，规划研究，不实施整改。
- Date: 2026-10-06
- Recorded snapshot: `66e42cfd34bca4800161098d3b302a35a52ce69b`（git rev-parse 只读核对，与任务一致；工作树无产品文件改动）。包内版本 `2.5.0`（Cargo.toml:3），edition 2024（Cargo.toml:4）。
- Status: **T1 完成：9 个指定文件全文 + `src/adapters/opencode.rs` 关键区域（partial，缺 91-107、458-1578）。T2 仅做过滤查询。未运行 build/test/install/网络/git 写操作。**
- 凭证: 同目录 `coverage-fast-resume.json` 记录 20 个文件（T1 10 + 补充 10）的字节数、SHA256、逐段阅读区间、缺失区间与状态；sha256 在读取前/收尾各校验一次（7 个补充文件仅收尾两次校验，原因见账本），全部一致。

## 0. 方法与阅读边界

- 阅读方式：`codegraph node --file <相对路径> --offset <起> --limit <≤150>`，1-based 闭区间、逐行编号；哈希不计阅读。
- T1 指定 10 个文件：9 全文共 3993 行；opencode.rs 440/1578 行（被读区间 1-90、108-257、258-407、408-457）。
- 补充阅读（用于回答索引/搜索/launch 问题）：`refresh.rs`、`adapters/mod.rs`、`index/schema.rs`、`index/document.rs`、`index/queries.rs`、`model.rs`、`tui/input.rs`、`Cargo.toml` 全文；`tui.rs`（30-129、176-230）、`tui/text.rs`（185-224）局部。合计 5595 行。
- T2：`research/sweep-fast-resume.json`（68 文件、55 扫描、13 二进制排除、47 有命中）只按探针过滤查询；探针定义与 `sweep-index.json` 的 probes 字典核对过。
- 未审边界：其余 9 个 provider 适配器的解析主体未逐行读（只定向抓取 `resume_command`/`supports_yolo` 行，行号已记录）；opencode legacy storage（458-1578）未读；README/docs 未全文读——凡引用均标注为「grep 过滤命中行」证据。以上不冒充审毕。

## 1. 一页总览

- 形态：Rust 2024、单二进制 `fr`（Cargo.toml:8-10）；Tantivy 0.26.1 全文索引（Cargo.toml:28）；rusqlite bundled 用于 opencode（Cargo.toml:25）；rayon 并行扫描（Cargo.toml:23）；默认进入 TUI（main.rs:122-131）。
- 第一次成功路径：零配置启动 → `AppState::new` 先出结果 + 后台增量刷新（tui.rs:53-88、state.rs:89-113）→ Enter 恢复（input.rs:25-29）→ 退出 TUI 后在会话 cwd 内 `exec` 原生 CLI（main.rs:197-245）。
- 覆盖：静态注册 12 个 provider（adapters/mod.rs:80-95；config.rs:13-26）。
- 强项：① 增量「last-good」保留：坏 JSONL 不删旧条目、扫描失败不产生删除（shared.rs:158-185、38-57）；② 查询容错：lenient 解析 + 1 编辑距离 fuzzy + 目录子串（queries.rs:56-101）；③ 删除只在扫描完整时发生（codex.rs:278-285）。
- 弱项：① 包装层吞查询错误成空结果/0（search.rs:27-63）；② `--rebuild` 与「空索引重建」用一次全量结果整体替换，没有 last-good（index.rs:83-97；main.rs:80-96,153-157）；③ 增量只看 mtime，无 content hash/size（shared.rs:27-36；schema.rs:44-63 无该字段）；④ 内容扁平化、无消息身份/字节锚点（model.rs:8-19）；⑤ TUI 仅启动时刷新一次，无文件监听、无手动刷新键（tui.rs:54-81；sweep fs_watch=0；input.rs:16-58）；⑥ 结果上限固定 50/100、无分页（main.rs:116；state.rs:132-137；tui.rs:210-215）。

## 2. 它如何发现/索引会话

### 2.1 发现与数据源

- 12 个 adapter 静态注册（adapters/mod.rs:80-95），默认目录/环境变量（config.rs:152-279）：claude `~/.claude/projects`（152-154）；codex `~/.codex/sessions` + `session_index.jsonl`（156-162）；antigravity `~/.gemini/antigravity-cli`（164-166）；cursor `~/.cursor/chats`（168-170）；grok `$GROK_HOME|~/.grok/sessions`（172-176）；kimi `$KIMI_CODE_HOME|~/.kimi-code/sessions`（178-182）；opencode `~/.local/share/opencode`（184-186；`opencode.db` 优先，否则 legacy `storage/`，opencode.rs:41-55）；pi 依次取 `PI_CODING_AGENT_SESSION_DIR` → settings.json 的 sessionDir → `~/.pi/agent/sessions`（188-210）；vibe `~/.vibe/logs/session`（241-243）；crush `projects.json`（245-251）；copilot-cli `~/.copilot/session-state`（253-255）；copilot-vscode 按平台取 VS Code storage（257-279）。
- 文件发现：claude 只扫 `projects/<项目>/*.jsonl` 且排除 `agent-*`（claude.rs:160-200）；codex 对 sessions 树 WalkDir 全量 `*.jsonl`（codex.rs:210-225）；opencode 读 SQLite 三表（opencode.rs:114-175）。
- ID 归一化：claude=文件 stem（claude.rs:187-193）；codex=文件名末 36 位 UUID → 首行 session_meta.id → fallback stem（shared.rs:278-292；codex.rs:175-197、49-70）；索引键=`agent::id`（document.rs:44-46）。
- 会话模型：`Session{id,agent,title,directory,timestamp,content,message_count,mtime,yolo}`（model.rs:8-19）；content 为扁平文本，用户行加 `» `、助手行加两空格前缀（claude.rs:71,88,109；codex.rs:88-110）——没有消息 ID、父边、分支或源字节范围。

### 2.2 索引格式（Tantivy）

- 位置 `~/.cache/fast-resume/tantivy_index`（config.rs:144-150）；schema 版本常量 23（config.rs:11），写入索引目录 `.schema_version`（schema.rs:11,65-76）；版本不匹配→`remove_dir_all` 后重建（index.rs:51-68）。
- 字段（schema.rs:44-63）：id/session_key/title/directory/agent/content/timestamp/message_count/mtime/yolo；title、content 为 TEXT|STORED；directory/agent/id/session_key 为 raw tokenizer；timestamp 为 f64 fast field。
- 写入：`rebuild`=`delete_all_documents`+add+commit（index.rs:83-97）；`update_sessions`=按 session_key `delete_term` 后补写+commit+`reader.reload()`（index.rs:225-243）；删除同理（245-260）。Reader 为 `ReloadPolicy::Manual`（index.rs:71-74），提交后显式 reload。
- 边界：索引打开失败**不会**自动重建（只有 schema 版本不匹配才清空重建，index.rs:52-61）；「可删除缓存」是文档运维语义（README.md:72-78，grep 证据），不是代码自愈。

### 2.3 增量（mtime 驱动 + last-good 屏障）

- `known_sessions()` 读出 (agent,id)->mtime（index.rs:123-138）；adapter 生成 current_files；`session_needs_update` 判定 `abs(mtime-known)>0.001s`（shared.rs:27-36；MTIME_TOLERANCE=0.001，adapters/mod.rs:32）。mtime 回退同样触发更新（shared.rs:423-435 测试）。
- sidecar 参与 mtime：claude 取 sessions-index.json 的 max（claude.rs:194-197、291-318）；codex 取 session_index.jsonl `updated_at` 与文件 mtime 的 max（codex.rs:144-166、220-223）；opencode 取 max(time_created,time_updated,message/part 活动时间)（opencode.rs:243-254、382-447）。
- JSONL 健康分类（shared.rs:194-224；测试 400-420）：Clean→正常解析，parse None=删除；Partial（坏行之后仍有合法行）→仅当解析结果通过可用性检查才更新，否则 Retain；Invalid（尾部半个 JSON/全坏/不可读）→Retain 旧条目。对应测试：claude.rs:474-509、codex.rs:563-645。
- 删除屏障：`deleted_ids_for_agent` 只在扫描完整时生效；读目录/WalkDir/DB 失败→failed_incremental_scan（空更新、零删除）（shared.rs:38-57；codex.rs:278-285,320-327；claude.rs:244-250；opencode.rs:374-380；测试 codex.rs:703-723、claude.rs:511-524）。codex 的 `complete` 标志在 WalkDir 条目出错时置 false（codex.rs:199-227）。
- 批处理：索引批量常量 500（index.rs:23）；流式事件→单消费线程按批 flush（refresh.rs:87-158，flush 在 160-190）。
- 经典局限：无 content hash/size 字段（schema.rs:44-63）；mtime 语义被破坏（时间戳被保留的复制/同步）会漏更新；0.001s 容差对秒级回溯 mtime 有效，但极端情况可能误判相等。

### 2.4 新鲜度

- CLI：每次 `--list/--no-tui/--stats` 走 `refreshed_index()`：索引为空→全量重建；否则增量刷新（main.rs:113-119、153-162）。
- TUI：启动时后台线程做一次 `refresh_incremental_streaming`（tui.rs:54-81）；`Finished` 后主线程 reload+重搜（state.rs:541-552）；`Failed` 只把 "refresh failed …" 写入状态栏，继续用旧索引（state.rs:553-557）。
- 无文件监听（T2 sweep fs_watch=0）、键位无刷新（input.rs:16-58 全文）；长驻 TUI 不会出现后来新建的会话。`--rebuild` 是一次性替换（main.rs:80-96）。

### 2.5 并发

- 全量扫描用 rayon `into_par_iter`（refresh.rs:22-41）；增量每 adapter 一个 `thread::spawn`+mpsc 汇聚（refresh.rs:56-85）；消费侧单线程按批 commit（写者单线程、128MB buffer；删除 64MB，index.rs:229-230,249-250）。
- 读侧 SearchEngine 可 clone（Arc<IndexReader>，index.rs:38-44），Manual reload；TUI 搜索 worker 独立线程（tui.rs:176-225）。
- 未验证边界：多进程同时写会因 Tantivy 目录锁在创建 writer 时失败——CLI 会把错误上抛、TUI 显示 refresh failed；本轮未运行进程级并发实验，不做行为断言。

## 3. resume/launch 如何构造

- 命令是 `Vec<String>`（adapters/mod.rs:76），执行端 `Command::new(&command[0]).args(&command[1..])`，Unix `exec()`、非 Unix `status()`+退出码（main.rs:214-226）——**没有 shell 拼接，ID 以 argv 传递，无转义/注入面**。
- cwd/env：`env::set_current_dir(directory)`；空目录跳过（main.rs:237-241）；不修改环境变量，子进程继承。
- 链路：Enter→`begin_action`（input.rs:25-29,63-85）→（可选 yolo 弹窗，input.rs:72-83,87-121）→`adapter.resume_command(&session,yolo)`→`TuiExit::Resume{command, directory: session.directory}`（input.rs:124-139）→`exec_resume`（main.rs:197-245）。测试覆盖目录+argv 交接与空命令拒绝（main.rs:269-312）。
- Ctrl+Y 复制路径：`shell_join`/`shell_quote`（POSIX 单引号转义，`'`→`'\''`，text.rs:196-213）拼 `cd '<dir>' && <cmd>` 写剪贴板（input.rs:140-151）——只复制、不执行；Windows 上该字符串是 POSIX 风格（复制路径的跨平台边界）。
- 各 provider 命令（抓取行的锚点）：antigravity `agy [--dangerously-skip-permissions] --conversation <id>`（antigravity.rs:425,477）；claude `claude [--dangerously-skip-permissions] --resume <id>`（claude.rs:211,270-277）；codex `codex [--dangerously-bypass-approvals-and-sandbox] resume <id>`（codex.rs:235,331-338）；copilot-cli `copilot [--yolo] --resume <id>`（copilot_cli.rs:37,87-95）；crush `crush [--yolo] --session <id>`（crush.rs:36,60-68）；cursor `agent [--yolo] --resume <id>`（cursor.rs:169,218-226）；grok `grok [--always-approve] --resume <id>`（grok.rs:267,316-324）；kimi `kimi [--yolo] --session <id>`（kimi.rs:272,333-341）；vibe `vibe [--agent auto-approve] --resume <id>`（vibe.rs:37,96-104）；opencode `opencode <dir> --session <id>`（opencode.rs:69-76，忽略 yolo）；pi `pi --session <id>`（pi.rs:251-258，忽略 yolo）；copilot-vscode `code [dir]`（copilot_vscode.rs:89-95，无 session id、忽略 yolo）。
- yolo 语义：`supports_yolo=false` 的 copilot-vscode/opencode/pi（未覆写 trait 默认，adapters/mod.rs:44-48）由 UI 直接跳过弹窗执行（input.rs:72-77）；传入的 yolo 被静默忽略，不报错。

## 4. provider 覆盖、归一化边界与失败行为

- 归一化边界：
  - 唯一「角色/结构」表达是前缀约定（`» `/两空格）与拼好的 content；无法还原消息边界（model.rs:8-19）。
  - 降噪是启发式：claude 按 `isMeta`、`<command`/`<local-command` 前缀过滤（claude.rs:66-70）；codex 跳过 `<environment_context>`（codex.rs:91-94）；opencode 只取 part type=text（opencode.rs:160,309）。
  - codex 双源：正文同时收 `response_item`（codex.rs:85-96）与 `event_msg.user_message`（98-106）；`turns`/`user_prompts` 只来自 event_msg（103-104），且 `user_prompts` 为空即整体返回 None（122-124）。→ 真实日志（两种事件镜像同一 prompt）正文会重复一份；仅含 response_item 用户消息的日志会被判「无用户消息」而丢弃。
  - 时间兜底：claude/codex 用文件 mtime（model.rs:90-95）；opencode 无可用时间戳时用 `Local::now`（opencode.rs:191-192、348-349）。
- 失败行为矩阵（读侧）：
  - 全量/重建：错误→空集合（opencode DB 打不开→`Vec::new()`，opencode.rs:109-110,117-119,129-131；claude 打不开文件→None/跳过，claude.rs:38,49-51,237-239）；坏行→跳过（codex.rs:60-62）。
  - 增量：错误→Retain/no-op，不删数据（见 2.3）；坏行按健康分类处理。
  - 结论：**增量 fail-closed（宁保留旧数据），全量/rebuild fail-open（宁少数据不报错）**；两条路径语义不对称，是全项目最值得记录的行为边界之一。

## 5. 搜索/查询能力

- 全文范围：title+content+directory（queries.rs:56-58），`parse_query_lenient`（59）容忍半输入引号（search.rs:176-195 测试）；exact 命中 5 倍 boost（queries.rs:60）。
- fuzzy：单词≥3 字符、1 编辑距离、prefix=true，仅 title/content（queries.rs:64-79）；目录子串为 Single-word≥3 的 regex（91-101）。→ 拼写容错覆盖标题与正文（index.rs:409-445 测试），不含 directory 的 fuzzy。
- 语法（query.rs:67-217）：`agent:a,b`、`-agent:x`、`agent:!x`、`dir:`（大小写不敏感子串）、`date:today|yesterday|week|month`、`date:<3h|>1w`（m/h/d/w/mo/y）、`-date:…`；agent 值小写归一（106-119）；过滤落到 Tantivy TermSet/TermQuery/RegexQuery/RangeQuery（queries.rs:103-197）。
- 排序：有文本→score（`TopDocs::order_by_score`）；无文本→timestamp desc（`order_by_fast_field`）（index.rs:192-207）。score 由 exact boost+fuzzy+目录子串 Should 组合，无时间 tie-break。
- 分页：没有 offset/cursor；CLI 固定 limit 50（main.rs:116）并打印 "Showing N of total"（117-118,184）；TUI 固定 100（state.rs:132-137；tui.rs:210-215）。`count_matches` 独立计数（index.rs:210-219）。
- 错误吞：`SearchEngine::{all_sessions,total_len,count_matches,count_for_agent,agents_with_sessions,search}` 全部 `unwrap_or_default()/unwrap_or(0)`（search.rs:27-63）——索引/查询错误会变成「空列表/0 条」。TUI 搜索 worker 用 `search_result` 走 Result（tui.rs:204-217；state.rs:201-208 展示 "search failed: …"），但 CLI 路径与 TUI 初始同步搜索（state.rs:112,116-141）仍走吞错包装。

## 6. 与 ASG 对照

> 对照基准：review-report.md:40（fast-resume 行与结论句「某些 API 吞查询错误；索引当缓存可删除，不可照搬到权威 catalog」）与 sessiongrep-full-audit.md:215-223（ASG 契约背景）。本轮未重读 ASG 产品源码。

- fast-resume 强在哪（对 ASG 的压力）：
  1. **第一次成功路径**：零配置、开箱即搜、Enter 即在会话目录内 exec 恢复——没有 review/审批入口的摩擦。ASG 若要保持「简单任务入口」，这条体验基准必须正面对齐。
  2. **last-good 屏障**：JSONL 健康分级 + 不完整扫描零删除 + 扫描失败保留旧索引（shared.rs:158-185,38-57），这类"失败不推进水位"的语义与 ASG fail-closed 方向一致，是竞品里做得对的部分。
  3. **查询容错**：lenient 解析 + fuzzy，是「继续输入即可用」的交互基础（queries.rs:56-89）。
  4. **12 provider 覆盖**：含 opencode/kimi/pi/vibe 等长尾，值得核对 ASG 的 provider 矩阵缺口。
- fast-resume 弱在哪（ASG 的改良空间）：
  1. 错误吞吐：查询错误静默成空结果（FR-01）。
  2. 全量重建无 last-good（FR-02）。
  3. mtime-only 变更检测（FR-05）。
  4. 无消息身份/位置锚点（FR-04 之外的模型边界，model.rs:8-19）。
  5. 长驻视图新鲜度不足（FR-04）。
- ASG 该学：
  1. **成功路径闭环**：搜索→预览→恢复的键位链路（Enter=恢复、Ctrl+Y=复制命令、Tab=补全/切 agent）与 cwd 交接语义（main.rs:237-241）。
  2. **last-good 与「不完整扫描不删数据」的显式契约**（shared.rs:38-57,158-185；codex.rs:278-285）。
  3. **容错查询解析**作为交互层特性，同时保留错误分类。
  4. 增量 sidecar mtime 合并（claude sessions-index / codex session_index updated_at）——ASG 若已有 sidecar 标题源，可对照其失效策略。
- ASG 不该学：
  1. **吞错**：`unwrap_or_default` 把故障降级成"没有结果"，会污染信任与可观测性；ASG 应保持显式错误分类与 fail-closed。
  2. **把权威 catalog 当可删除缓存**：schema 不匹配即 `remove_dir_all`、`--rebuild` 整体替换（index.rs:52-55,83-97）在"缓存"定位下合理，但 ASG 的 catalog 是权威数据，不能照搬。
  3. **扁平化 + 镜像双写**：content 拼接丢消息身份、codex 双源重复（codex.rs:85-106），与 ASG「原生消息身份/只取权威事件」方向相反，不能作为归一化模板。
  4. **mtime-only 新鲜度**：对 must-not-miss 的目录需要内容指纹/尺寸或版本水位兜底。
- 反证与边界：fast-resume 自身文档承认索引是缓存（README.md:72-78、installation.md:73-82，grep 证据）且「只在扫描完整时推断删除」（how-it-works.md:68，grep 证据）——因此本文对 rebuild 的批评限定在"ASG 语境下不可照搬"，不把它列为 fast-resume 的谎言或遗漏；另据 review-report.md:40，本轮未复现其"首次成功"之外的任何性能/稳定性数字。

## 7. Findings（严重度：P0 阻断/P1 高影响/P2 中等边界/P3 轻微/ I 已核验信息）

#### FR-01 · P1 · 查询与索引错误被包装层吞成"空结果/0"

- 证据：`SearchEngine` 六个方法全部 `unwrap_or_default()/unwrap_or(0)`（search.rs:27-63）；CLI list 路径直接消费该包装（main.rs:113-119）；TUI 初始同步搜索同样如此（state.rs:112,116-141）。
- 可达：索引文件损坏、查询构造错误或 I/O 失败时，用户看到 "No sessions found."（main.rs:165-167）而退出码为 0；TUI 首屏显示空列表。
- 反证/界限：TUI 异步搜索 worker 用 `search_result`（Result 传播）且 `apply_search_error` 会显示 "search failed: …"（tui.rs:204-217；state.rs:201-208）；因此不是所有路径静默，且 errors 不影响 TUI 后续可用性。问题集中在 CLI 与初始路径的错误呈现。
- ASG 教训：结果 API 必须区分「空集」与「失败」；CLI 需要非零退出或显式错误分类。

#### FR-02 · P2 · 全量 rebuild 没有 last-good，单次扫描失败会静默缩小索引

- 证据：`rebuild` 先 `delete_all_documents` 再补写（index.rs:83-97）；`--rebuild`（main.rs:80-96）与「总长 0」路径（main.rs:153-157）走 `scan_all_sessions`（refresh.rs:22-41）；全量路径下 provider 错误返回空集合（opencode.rs:109-110,117-119），坏行被跳过后可能整体 parse 失败。
- 可达：`--rebuild` 时某 provider 暂时不可读（DB 锁/权限），该 provider 全部会话从索引消失；会话文件未动，下次增量扫描可恢复（自愈），但窗口期用户会认为会话丢失。
- 反证/界限：增量路径有 Retain/failed-scan 屏障（2.3 节证据）；索引被文档定义为可重建缓存（README.md:72-78，grep 证据）；`sort_and_dedupe` 与 commit 原子性使重建本身一致（index.rs:83-97）。因此不是数据损坏，而是「失败即缩水且无提示」。
- ASG 教训：权威 catalog 的 rebuild 必须快照对比 + 失败保护（last-good），或至少在结果中显式报告 provider 级失败。

#### FR-03 · P2 · Codex 正文双源双写；入选与计数只认 event_msg

- 证据：`response_item`（role=user/assistant）写入正文（codex.rs:85-96）；`event_msg.user_message` 再写一次正文并累计 turns/user_prompts（98-106）；`user_prompts` 为空即整体丢弃（122-124）。
- 可达：真实 rollout 中同一 prompt 常同时以两种事件出现→content 中重复；依赖解析出的 message_count（=turns，只数 event_msg）与正文条数不再对应（测试 codex.rs:422-445 用 "Replay" 文本掩盖了重复形态）。
- 反证/界限：`response_item` 提供压缩重放场景下的正文连续性；重复文本对"能不能搜到"无影响，只影响预览、内容长度与统计口径。
- ASG 教训：权威事件源唯一化（如只取 response_item/message），并在 need 时用映射而非二次写入获取用户消息。

#### FR-04 · P2 · TUI 新鲜度：仅启动刷新一次

- 证据：刷新线程只在 `run_tui` 启动时 spawn 一次（tui.rs:54-81）；状态机只响应 ScanMessage（state.rs:529-558）；输入映射无刷新键（input.rs:16-58）；T2 sweep fs_watch=0。
- 可达：TUI 长开（数小时）期间新建的会话不可见，除非退出重开或命令行触发刷新。
- 反证/界限：每次 CLI 调用都增量刷新（main.rs:153-162）；TUI 场景是"搜索历史并恢复"，新会话出现频率有限；未运行长驻实验，未测用户实际影响。
- ASG 教训：若提供长驻视图，需明确刷新触发（定时/热键/文件监听）并在 UI 呈现 freshness。

#### FR-05 · P2 · 增量变更检测只有 mtime（无内容指纹）

- 证据：`session_needs_update` 以 0.001s 容差比较 mtime（shared.rs:27-36；adapters/mod.rs:32）；索引 schema 无 content_hash/size 字段（schema.rs:44-63）；sidecar mtime 取 max 作为补偿（claude.rs:194-197；codex.rs:220-223）。
- 可达：内容变化但 mtime 未变（时间戳保留的复制、某些同步工具、粗粒度文件系统）会漏更新；mtime 回退则安全（shared.rs:423-435 测试）。
- 反证/界限：对"本地会话目录"这一用法 mtime 通常可靠；无删除/无哈希是缓存定位的合理轻量取舍；未观测到真实漏更新案例。
- ASG 教训：must-not-miss 场景需要内容指纹或版本水位，至少保留"解析器版本失效"通道。

#### FR-06 · P3 · 结果上限固定、无分页

- 证据：CLI limit 50（main.rs:116）、TUI limit 100（state.rs:132-137；tui.rs:210-215）；`TopDocs::with_limit` 硬截断（index.rs:194-206）；无 offset/游标参数。
- 反证/界限：CLI 打印 "Showing N of total"（main.rs:184）给出总量提示；"找会话"任务通常前 50/100 足够；未做真实大数据用户的可用性测试。
- ASG 教训：列表 API 提供 limit+total 之外，应给 continuation（cursor/offset）语义。

#### FR-07 · P3 · resume 语义的 provider 差异与复制字符串平台差异

- 证据：copilot-vscode 仅 `code [dir]`，无法恢复具体会话（copilot_vscode.rs:89-95）；opencode/pi 忽略 yolo（opencode.rs:69-76；pi.rs:251-258）；Ctrl+Y 复制用 POSIX 引号（text.rs:196-213），Windows 剪贴板字符串风格不符。
- 反证/界限：这些 provider 没有可用的"按会话恢复"原生能力或不需要提权，差异是合理的；执行路径不经过 shell（main.rs:214-226），复制文本仅供人工粘贴。
- ASG 教训：launch 能力矩阵应显式声明"会话级恢复/仅目录打开"；跨平台复制文本按目标 shell 生成。

#### FR-08 · P3 · 终端里 windows 路径的 `~` 展示不生效

- 证据：`display_directory` 只比较 `HOME`（model.rs:44-53）——Windows 通常无 HOME（用 USERPROFILE）。
- 反证/界限：仅影响列表显示宽度，不影响搜索/恢复；`env::set_current_dir` 用原始路径（main.rs:237-241）。
- ASG 教训：跨平台家目录缩写用平台 API，不用环境变量猜测。

#### FR-09 · I（已核验信息）· T2 探针命中不代表缺陷

- `sql_dynamic` 命中 opencode.rs（402、424 等）：表名来自固定集合 `message`/`part`（opencode.rs:384-385），时间列来自 PRAGMA 白名单（423-447），会话 ID 走参数绑定与 900 分块（275-284,306-315）——无用户输入拼接，非注入面。
- `shell_exec` 命中 src/main.rs 与脚本入口：main.rs 的 `Command` 是 argv 直接执行（214-226），无 `sh -c`；`python/fast_resume.data/scripts/fast-resume` 是启动脚本（T2 命中，未纳入本审计阅读范围）。
- `resume_launch` 373 处命中中 254 处在 CHANGELOG（T2 计数），不是代码行为证据。
- 结论：T2 命中用于定位，不单独定性；本报告所有结论以 T1 读到的行为为准。

## 8. Caveats / Not Found

- opencode.rs 仅读 440/1578 行：legacy storage 解析（458-1578）与测试段未读；其增量/删除行为若与 SQLite 路径不同，本文未覆盖。
- 除 claude/codex/opencode 外的 9 个适配器只抓取了 resume_command/supports_yolo/（部分）错误处理线索，未逐行读解析逻辑；provider 矩阵中它们的归一化细节不在本报告断言范围。
- 未运行任何行为验证：无 build/test/install、无真实会话恢复、无多进程并发、无性能测量；`--rebuild` 缩水窗口、TUI 长驻影响、Windows 复制字符串均为源码推理而非复现。
- 文档证据（README.md:72-78、installation.md:73-82、how-it-works.md:59-81）来自 grep 过滤命中行，未全文阅读；引用仅作语义佐证。
- 快照：本报告只代表 commit 66e42cf 的工作树内容；读取前/收尾 SHA256 全部一致（20/20，其中 7 个补充文件只有收尾两次独立校验），不保证未来修改。
- 所有权：本 worker 只创建 `research/coverage-fast-resume.json` 与 `research/audit-fast-resume.md`；未改动仓库、未改动其它 TASKDIR 产物、未执行 git 写操作。