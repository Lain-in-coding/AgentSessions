# 竞品源码审计：cc-sessions-viewer 分片 A —— 会话聊天 / 回合 / 恢复核心

- Query：对 `Github_src/cc-sessions-viewer`（无 Git 元数据，package 0.3.25 目录快照）做分片 A 静态审计：会话检索语义、「只匹配用户消息」的真实实现、turn/fork/编辑/恢复的命令构造与安全、7 家 provider 的共性与差异、许可证状态、以及与 ASG 在「跳到正确消息 / 分支」上的对照。
- Scope：internal；仅该目录快照的静态源码与文本，不 build / test / install / 网络 / git 写；不实施整改。
- Date：2026-10-06。
- Snapshot：`commit = null`（无 `.git`）；版本 0.3.25（`package.json:4`、`src-tauri/Cargo.toml:3`）；`coverage-index.json` 的 cc-sessions-viewer 条目：93 文件、`tree_content_sha256 = 0ee8993caf13b19ef4d7d0d31d0bb1d3d71708877cabe6f5b3df30ba4ee4afd7`。分片提示词引用的 `4b3b8c6c...` 未能在 research 目录按字面检索到（见 §9）。
- Status：T1 阅读完成（3 full + 1 partial）；所有结论为静态证据，未运行产品；未验证上游仓库最新状态。

## 1. 阅读凭证与覆盖统计

| 口径 | 实际数量 |
|---|---:|
| T1 文件 | 4 |
| `full` | 3：`agent_chat.rs`(4087)、`turn.rs`(2322)、`agents/mod.rs`(1360)，行 1..EOF 全部带行号显示 |
| `partial` | 1：`util.rs`，已读 1483 / 2043 行（缺 560 行，全部为内联测试模块 + 996-997 注释缝） |
| T1 总行数 / 已读行数 | 9812 / 9252 |
| 每个读取窗口 | ≤150 逻辑行、未截断、PowerShell 行号输出；哈希从未当阅读 |
| 内容校验 | 4 个文件读前/读后 SHA256 一致，`content_unchanged=true` |

- `util.rs` 的选择性口径：函数索引显示 722 起为 `mod tests`、1728 起为 `mod path_lifting_tests`；所有非测试产品函数都落在已读区间 `1-750` 与 `998-1730`。未读 `751-997`（测试 + 996-997 注释缝）与 `1731-2043`（全部测试）。
- T2 机械层（主会话产物，未整读）：`research/sweep-cc-sessions-viewer.json`，93/93 文件扫描、0 二进制排除、76 个文件有任一探针命中；探针总命中 `shell_exec=50, sql_dynamic=26, provider_paths=210, fs_watch=13, catch_empty=19, unwrap_or=624, ignored_result=288, truncation_limit=26, concurrency=83, index_storage=4, resume_launch=177, secret_redaction=339, inline_tests=887, surface_mcp=933`。
- 抽查补充（T1 外、只作证据，不计入 T1 覆盖）：`agent_command.rs:40-186`（命令转义 helpers）、`runtime.rs:1-95`（会话租约）、`lib.rs:640-700,845-915,950-1010,1225-1290`（搜索包装、PTY resume/new 透传、参数校验）、`src/api.ts:330-395` 与 `src/App.vue:1-29,3270-3280,3345-3355`（前端 extraArgs 调用链）、`pty.rs` 的过滤检索（grep 定位，非全文）。
- 机器凭证：同目录 `coverage-cc-sessions-viewer-core.json` 记录每个文件的字节数、总行数、SHA256、实际阅读窗口、状态、未读区间、读取方法。

### 1.1 快照完整性告警（影响所有「不存在」类结论）

- `src/App.vue:9-29` 从 `./settings` 导入 `launchArgs` 等符号，但 `src/settings.ts`（及任何 settings 模块）在磁盘与 93 文件清单中都不存在。
- `README.md:12` 链接 `README.ja.md`，该文件不存在。
- 结论：该快照不是自包含/可构建的完整仓库副本。因此「没有 LICENSE 文件」只能断言为「本快照内没有」，不能据此断言上游仓库没有。

## 2. 检索语义：「只匹配用户消息」的实际实现

### 2.1 调用链与范围

- 入口 `search_sessions`（`lib.rs:666-689`）：把请求放进 `spawn_blocking`，写全局 `SEARCH_GEN` 代际号，`cancel_search`（`lib.rs:694-696`）只做代数 +1；panic 被 `JoinError` 转成 `Err("search task panicked: …")`（`lib.rs:687-688`）。
- `agents::search()`（`agents/mod.rs:789-859`）：`list_projects(false,false)` → 对每个项目 `list_sessions(..., 0, usize::MAX, ...)` 拉全量会话元数据（`mod.rs:813`）→ 展平 → `SEARCH_POOL.install` 内 `into_par_iter` 并行分类（`mod.rs:834-851`）。
- **是全量扫描**：每次搜索枚举当前 agent 的全部项目、全部会话；没有持久化索引，没有增量倒排。优化手段只有：元数据先行短路、字节粗筛、缓存、4 线程池（`mod.rs:23-29`）、可取消、结果截断 200 条（`mod.rs:98,857`）。
- 取消检查散布在项目循环（`810-812`）、每会话开始（`837-839`）、文本阶段（`892-894,919-921`）、Pi 分支循环（`964`）。取消返回 `Ok(vec![])`，靠前端 reqSeq 丢弃旧结果。

### 2.2 两个匹配层：元数据 → 用户正文

`classify_hit`（`agents/mod.rs:864-941`）顺序：

1. ID：`session.id` 包含 lowercase query（`883-884`）；
2. 标题：`session.title` 包含 query（`885-886`）；
3. 正文：`id` scope 直接返回 None（`889-891`）；否则进入文本阶段。

文本阶段的「只匹配用户消息」是硬约束（`agents/mod.rs:988-1034`）：`msg.role != "user"` 跳过（`1009-1011`）；只拼接 `block.kind == "text"` 的块（`1016-1026`）；`thinking` / `tool_use` / `tool_result` / `image` / `diff` 全部不参与。助理回复因此**永不**命中。命中只取第一条命中消息（`scan_user_text`，`1037-1048`），返回 `match_msg_index`（解析后 `Msg[]` 下标）与 `match_msg_uuid`（有则带，没则 None）。

需要注意两个语义细节：

- `keyword` scope 并非纯正文：`SearchScope::Keyword` 只排除 ID，仍先匹配标题（`885-891`；范围文档见 `765-773`）。标题大多由首条用户消息派生，所以「标题命中」经常等价于「正文命中」的再次曝光。
- ID/标题命中不读文件；只有两者都没中才走正文，正文才需要磁盘。

### 2.3 冷路径与缓存

- 预筛：`contains_text` 默认 `file_contains_ci`（`agents/mod.rs:513-515,1056-1085`）——`fs::read` **整文件读入**，ASCII 查询走 `windows().eq_ignore_ascii_case`，非 ASCII 才 `to_lowercase().contains`。库型 agent（opencode）覆写成 SQL。
- 解析：`read_session` 全量 JSON 解析（`923`）。
- 缓存：`USER_TEXT_CACHE: Mutex<Option<HashMap<path, (mtime, Vec<(msg_index, uuid, user_text)>)>>>`（`mod.rs:39-51`）。`source_mtime` 是失效锚（`502-508`；opencode 覆写为库文件 mtime）。命中直接 `clone()` 整段 Vec（`63-72`）；miss 解析后立即重建并写回（`1002-1033`）。因此同一会话第二次搜索任意关键词都是纯内存 substring，不再 I/O。
- 命中片段：`match_snippet` 前后各 60 字符 + 省略号 + 空白折叠（`1089-1112`）。
- 用量另有一套同构缓存 `USAGE_CACHE`（`637-671`）。
- Pi 特例：不查缓存；`find_pi_text_hit` 遍历所有 `terminal` 叶子 lineage，逐叶 `read_session_at` 找命中并返回 `pi_leaf_id`（`895-902,956-986`）。这是本仓库唯一的「按分支搜索」实现。
- 结果：按 `session.modified` 降序，截断 200（`855-858`）。

### 2.4 失败行为

- 单个项目 `list_sessions` 失败：`continue`，搜索继续（`813-816`）。
- 单个会话正文解析失败：`read().ok()?` → 该会话静默 miss（`1006`）。
- 无任何 partial/失败/新鲜度标记：前端拿到的是「没有这条」，而不是「可能有但读失败」。
- 命中上限 200 是**结果**上限，不限制扫描成本；预算型查询只影响扫描进度，不影响读多少文件。

## 3. turn / fork / 编辑 / 恢复：命令构造与安全性

### 3.1 进程模型与 PTY

- GUI chat 明确**不走 PTY**：`agent_chat.rs:1-8` 注释与 `build_piped_command`（`335-405`）用 `Stdio::piped()` 三路管道；PTY 只服务窗口内 TUI resume / shell。
- POSIX：`$SHELL -l -i -c "cd '<posix_quote(cwd)>' && <command.to_posix_shell()>"`（`344-382`）；独立进程组时去掉 `-i`（macOS job-control 卡死的注释 366-374）。
- Windows：`powershell.exe/pwsh -NoLogo -Command <powershell_set_location_and_run>`，`CREATE_NO_WINDOW`（`384-405`）。
- 进程生命周期：长驻进程 stdin 持续收 `{"type":"user",...}`（`349-428` encode）；OneShot 每轮一进程（`3433-3512`）；Codex app-server 长驻 JSON-RPC（`1666-1848`）。所有子进程经 `process_tree::register`/`terminate` 与 `runtime::spawn_permit` 闸门（`599-603,1687-1692,3462-3479`；`runtime.rs:15-32`）。

### 3.2 命令字符串与转义

- `AgentCommand` 把 program 与每个 arg 用 `posix_quote`（单引号，`'` → `'\''`，`agent_command.rs:81-83`）或 `powershell_quote`（单引号，`'` → `''`，`86-88`）转义后拼接（`47-56,61-77`）；cwd 同样被引号包裹（`93-105`）。
- **例外：`extra_args` 不走引号**。`to_posix_shell`/`to_powershell` 在末尾直接 `parts.push(self.extra_args.clone())`（`agent_command.rs:52-54,73-75`）。该字符串来自前端 `launchArgs`（`App.vue:3277,3352`；`api.ts:333-393`）并经 Tauri 命令透传（`lib.rs:852/872、896/905、1226/1246、1256/1264`）。session id 有白名单校验（`lib.rs:859-865,1233-1239`），chat 的 model/effort/permission 也有校验（`lib.rs:964-991`），但 PTY / 外部终端 / new-session 路径的 `extra_args` 没有长度或字符限制 → 见 CCV-01。
- `command_exists_in_login_shell` 也用 shell 字符串，但 program 是内部常量 `codex`/`reclaude`（`agent_chat.rs:407-431,466-474`），风险可忽略。

### 3.3 fork / 编辑 / 恢复

| 动作 | 实现 | 证据 |
|---|---|---|
| Claude 续聊 | 长驻 `claude --print --input-format stream-json … --resume <id>`；`fork=true` 时 `--fork-session` | `claude.rs:252-254,261+`；`agents/mod.rs:310-332` |
| Codex 续聊 | app-server `thread/resume`（有 threadId）或 `thread/start`（无） | `agent_chat.rs:1806-1835` |
| Codex 侧聊 fork | `thread/fork` + `excludeTurns=true` + `ephemeral=true` | `agent_chat.rs:951-979,1790-1805` |
| Codex 取消后编辑 | `thread/read(includeTurns)` → 取「最后一轮的前一轮」为 `lastTurnId` → `thread/fork(excludeTurns=false)` | `agent_chat.rs:981-990,3377-3431` |
| 磁盘级 fork | trait `fork_session / fork_session_before_user_turn`，默认 Err；实现方（Claude）不在本分片 | `agents/mod.rs:266-292` |
| 恢复命令 | agy `--conversation`；claude `--resume`；codex `resume`；grok `--resume`；kimi `--session`；opencode `--session`；pi `--session <path>` | `agy.rs:1030-1033`、`claude.rs:252-254`、`codex.rs:2328-2330`、`grok.rs:1939-1941`、`kimi.rs:1948-1950`、`opencode.rs:1264-1267`、`pi.rs:1595-1598` |

- **互斥写租约**是恢复路径的亮点：GUI chat 与内嵌 TUI 按 `(agent, session_id)` 争同一个 `SessionLease`，冲突时报「已由 GUI chat / in-app terminal 打开」并拒绝（`runtime.rs:55-91`；`agent_chat.rs:154-173,552-565`）。Codex 终止时先发 `thread/unsubscribe` 再杀进程树（`1616-1643`），启动失败 guard 会释放租约（`1651-1663`），避免下次 resume 收到 `thread already has an active writer`。
- `stop` 幂等：先摘除注册表再 kill+wait 再释放租约（`3784-3815`）；`interrupt` 对 Claude 写 ESC 字节，对 OneShot 直接 stop，对 app-server 发 `turn/interrupt`（`3830-3871`）。
- 权限模式显式映射：`ask/approve/default/acceptEdits` → `approvalPolicy=on-request` + `workspace-write`；`plan` → `read-only`；`fullAccess/bypassPermissions` → `never` + `danger-full-access`；`custom` 留给 config.toml（`agent_chat.rs:798-855`）。
- OneShot 审批有一条需警惕的自动重跑链：识别「被 sandbox 拒绝」靠错误文本 contains（`3514-3538`）；用户批准时把**整条命令**推入 `approved_command_prefixes`（`3917-3926`），之后 `command.starts_with(prefix)` 即自动以 `fullAccess` 重跑（`3569-3581,3630-3647`）。前缀比较语义比「同一条命令」宽：批准 `git status` 后，`git status; …` 这类以之开头的命令同样会命中自动放行。见 CCV-03 附加项。

## 4. 7 家 provider：共性、差异与失败行为

注册清单：`agy / claude / codex / grok / kimi / opencode / pi`（`agents/mod.rs:103-109`；`source()` 1114-1126，`kimicode|kimi` 均映射 KimiSource）。

### 4.1 共性（协议抽象）

- 全部实现 `trait SessionSource`（`mod.rs:224-635`）：`list_projects / list_sessions / read_session / rename_session / trash_title / resume_command / new_session_command / image_src / usage_summary / read_turns`。
- 可选能力用显式默认值收口：`read_session_at` 回落 `read_session`（`249-253`）；`session_tree / session_export_json / fork_session / fork_session_before_user_turn / chat_command / chat_turn_command` 默认 Err/None（`255-292,323-332,438-447`）；`parse_chat_line` 默认 Ignore（`334-341`）；`chat_encode_input` 默认 Anthropic stream-json 形状（`349-428`）。
- 统一输出 `Msg/Block`（`246-248`），GUI chat 统一归一为 `ChatEvent`（`160-194`），会话存储边界统一为 File/Directory 单元（`111-158`，删除/恢复/清空的校验钩子 `531-634`）。
- 差异被 trait 收口：`source_mtime`、`contains_text`、`watch_target(s)`、`validate_session_path`、`session_storage_unit`（`502-553`）。

### 4.2 差异（从 trait 覆盖点与本次抽查可见的部分）

| provider | 存储 / 路径 | GUI chat 进程模型 | 树 / 分支 | 备注 |
|---|---|---|---|---|
| claude | 单 JSONL；统计额外发现 `subagents/*.jsonl`（`mod.rs:483-500`） | LongLivedStdin（默认） | 无通用树 | 唯一提供长驻 stdin 控制协议（Permission/Question/Delta） |
| codex | JSONL + session_index/sqlite 旁路（探针 `sql_dynamic=3`，实现未读） | CodexAppServer（`mod.rs:343-347`） | 无（线性 thread/turn） | 唯一走 JSON-RPC 审批/提问回合 |
| agy | transcript_full 优先作 watch target（`mod.rs:517-519`） | 无（chat_command 默认 None） | 无 | resume：`--conversation` |
| grok | 目录型会话（`updates.jsonl` + sidecars；`mod.rs:111-117`） | 无 | 无 | hooks 存 `config.toml`；路径经 `find_updates_path`（`turn.rs:335-344`） |
| kimi(kimicode) | wire 文件 + 会话目录；`find_main_wire_path`（`turn.rs:338-344`） | 无 | 无 | hooks 是顶层 `[[hooks]]` 数组 |
| opencode | DB 型，虚拟 `opencode://` 路径 | 无 | 无 | 覆写 `source_mtime/contains_text(SQL)/watch_target(-wal)/validate_session_path`（`mod.rs:502-541`） |
| pi | v3 parent-linked JSONL | 无 chat（有 slash commands） | **有**：`session_tree/read_session_at/session_export_json`（`mod.rs:249-264`） | 搜索按 terminal lineage 逐叶找 `pi_leaf_id`（`895-902,956-986`） |

### 4.3 失败行为

- 能力缺失是 fail-closed：不支持 fork/树/导出时返回中文 Err（`274-292,257-264`），调用方不能静默当空。
- 搜索/统计是 fail-open 静默降级：项目列举失败跳过（`813-816`）；正文解析失败 miss（`1006`）；Pi 某条 lineage 读失败 `continue`（`965`）；统计里单项目出错当 0（`684-703`）、`session_usage(...).unwrap_or_default()`（`706-709`）。
- Codex app-server reader 跳过非法 JSON 行（`agent_chat.rs:2909-2916`）；RPC 等待有 10/15/5s 超时（`757-796`）。长驻路径的解析容错由各 provider 的 `parse_chat_line` 决定（本分片未读 provider 实现）。

## 5. 许可证状态（README badge vs 实际文件）

- README 声明：`README.md:10` `[![License: MIT](…)](LICENSE)`；`README.md:186-188` 「## License / [MIT](LICENSE) © jerrywu001」；`README.zh-CN.md:10,186-188` 同。
- 实际文件：快照内**不存在** `LICENSE / LICENSE.md / LICENSE.txt / COPYING / NOTICE`（对仓库递归按文件名 `(?i)licen[cs]e|copying|notice` 检索，排除 `node_modules/target/.git`，零命中；`Test-Path` 三项均为 False）。
- 清单声明：`package.json` 与 `src-tauri/Cargo.toml` 全文检索 `license` 字段均无命中（Cargo `[package]` 段 `1-8` 行无 license）。
- 反证/边界：本快照已被证明不完整（§1.1：`src/settings.ts`、`README.ja.md` 被引用却缺失），且无 `.git` 无法对历史/上游取证。因此结论限定为：**本快照确实没有 LICENSE 文件，README 的 MIT 徽章与 License 链接指向一个不存在的文件；但不能据此断言上游仓库真的从未包含 LICENSE**。

## 6. Findings（证据 → 影响 → 反证）

### CCV-01 · P2 · 安全 · 前端 `extra_args` 未转义直拼 shell

- 证据：`agent_command.rs:52-54,73-75`（`extra_args` 原样 append，program/args 却逐个引号）；`lib.rs:845-885,890-905,1225-1248,1253-1265`（PTY/外部终端 resume/new 透传，session id 有白名单、extra_args 没有）；`api.ts:333-393`；`App.vue:3277,3352`（来自 settings 的 `launchArgs`）。
- 影响：`extra_args` 中若含 `;`、`|`、`$()`、反引号等，会由被拉起的登录 shell / PowerShell 解释执行。可达面：任何能影响该设置字符串的路径（用户设置文件、前端注入、被篡改的持久化状态）。同类风险在 `spawn_terminal` 外部终端路径同样存在（`1244-1248`）。
- 反证/边界：该值由应用自己的前端经本地 IPC 传入、通常是用户自己填写的「附加启动参数」，shell 语法甚至可能是高级用户的预期；本轮未证明存在远程输入或前端注入。定级 P2 为「边界不一致 + 注入面存在」，不是已发生的 RCE。

### CCV-02 · P2 · 可用性/崩溃 · `match_snippet` 在小写膨胀字符上可 panic

- 证据：`agents/mod.rs:1089-1097` —— `char_idx` 在 `hay.to_lowercase()` 上计算，`chars` 来自原 `hay`；`start = char_idx - 60`、`end = min(char_idx + qlen + 60, chars.len())`，最后 `chars[start..end]`。当小写映射膨胀（如 U+0130 `İ` → `i`+U+0307，Rust 全量小写按 Unicode SpecialCasing）且命中点靠后时，`char_idx > chars.len()`，`start > end`，切片 panic。
- 静态推演：hay = `'İ'×70 + 'x'`（71 字符）、query=`x` 时，`hay_l` 140 字符、命中 byte 210 → `char_idx=140`；`start=80`、`end=min(201,71)=71` → `chars[80..71]` panic。含义：一条包含大量 `İ` 的用户消息 + 一个其后命中的查询即可让该次搜索整体失败。
- 反证/边界：`lib.rs:673-688` 用 `spawn_blocking`，panic 变 `JoinError` → 命令返回 `Err("search task panicked: …")`，不崩进程；触发字符较罕见；本轮未运行 Rust，推演基于 `str::to_lowercase` 的 Unicode 语义，不是实测。同因还会让膨胀点后的 snippet 窗口整体偏移（即使不 panic）。

### CCV-03 · P2 · 状态一致性 · 发送失败后 turn 永显 running + 幽灵用户消息

- 证据：`agent_chat.rs:3256-3299` —— `send()` 先 `remember_user_input` + `mark_turn_started`，再写 stdin / spawn OneShot；`spawn_oneshot_turn` 的多个早退（`3446-3461`：agent 解析失败、无 chat 命令、已有回合）与 spawn/注册失败（`3471-3492`）都直接 `return Err`，不清 `turn_started_at_ms`。`turn_state` 完全由该时间戳派生（`4058-4069`），`mark_turn_finished` 只在收到 `ChatEvent::Result`（`3151-3152`）或 app-server `turn/completed`（`3020-3040`）时调用。LongLived 的 write 失败路径同样（`3278-3285`）。
- 影响：一轮都未真正跑起来时，后端消息列表多出一条用户消息（`list_running_chats` 会回给刷新后的页面），`turn_state` 永为 `running`，直到 stop/关闭该 chat。若早退原因是「已有回合在跑」，则幽灵消息照加，且本轮输入实际丢失。
- 反证/边界：前端可能在 `send` 报错后主动 stop/restart 或重发，从而掩盖；本轮未运行 UI，无法证明用户可见症状持续多久。

### CCV-04 · P2（合规）· MIT 声明缺 LICENSE 文件与 manifest 字段

- 证据与边界见 §5。README 声明 MIT 且链接 `LICENSE`，但快照内无该文件、`package.json`/`Cargo.toml` 无 `license` 字段。
- 影响：再分发者拿不到许可正文与版权行；徽章/链接是坏链。对「可不可以拷贝/学习其代码」的合规判断只能停在「README 声称 MIT」。
- 反证/边界：快照不完整（§1.1）——不能断言上游无 LICENSE；README 的意图声明明确。

### CCV-05 · P3（资源）· 会话文本 / 用量缓存无界且命中即 clone

- 证据：`agents/mod.rs:39-80`（`USER_TEXT_CACHE` 只 insert，无淘汰/容量上限；命中 `entry.msgs.clone()`）、`637-671`（`USAGE_CACHE` 同构）、`1032`（解析后写入）。缓存以进程生命周期为界；被删除会话的条目不会被清理；文件被改写仅在下次命中查找时按 mtime 判旧覆盖。
- 反证/边界：单条通常很小（每条用户消息一行）；mtime 失效避免了跨版本读旧内容；未测量内存增长。边缘：mtime 精度为毫秒（`53-61`），极端同毫秒改写是理论盲区，未实测。

### CCV-06 · P3（性能/资源）· 冷搜索整文件读入、每查询每会话一次

- 证据：`agents/mod.rs:1056-1085`（`fs::read` 整文件；无文件大小上限）、`916`（预筛调用点）、`834-851`（4 线程池并行）、`98,857`（200 是结果上限，不是工作量上限）。
- 影响：大 JSONL 会话的冷查询会把整文件读进内存做字节扫描，解析再读一遍；多项目全量扫描成本随会话总量与文件大小增长。库型 agent 单列（SQL 预筛）。反证：元数据命中短路、缓存命中后纯内存、取消可让位；未测量真实耗时/内存。

### CCV-07 · P3（安全边界/跨平台）· hook 命令引号只处理 `\` 与 `"`

- 证据：`turn.rs:1211-1223`（所有 JSON 配置的 hook 命令）与 `turn.rs:1244-1246`（`shell_string_arg`：仅 `\`→`\\`、`"`→`\"`）。POSIX 双引号内 `$`、反引号、`!`（历史展开）仍可插值；若 app data 目录路径含这些字符，hook 执行时会被解释。Windows 侧同一转义把 `C:\…` 写成 `C:\\…`：Git-Bash/sh 语义下可还原，cmd 语义下不是转义符（`2007-2019` 的测试只验证「检测得到」，未验证执行）。
- 反证/边界：这些路径来自 home / `dirs::data_local_dir()`，普通用户环境不可控概率低；各 agent hook 实际由哪个 shell 执行未在本分片核实；未实测。

### CCV-08 · P3（配置完整性）· Claude/Codex/Agy JSON 配置直接覆写，非原子

- 证据：`turn.rs:561-563,583-585,594-596` 用 `fs::write` 覆写 settings.json / hooks.json；同文件 Grok/Kimi TOML 是「备份 + 临时文件 + rename + 目录 sync」（`1387-1449`），Pi 也是备份前 revision 校验 + 原子替换 + 失败清理（`769-807,906-933`）。
- 影响：写 JSON 期间崩溃/断电可能留下截断的 Claude/Codex/Agy 配置（损坏用户其他工具的配置文件）。反证：写入内容小、先解析后写、失败窗口窄；未观察实际损坏。

### CCV-09 · P3/I（可观测性）· 大量失败静默降级，无 partial/freshness 标记

- 证据：`agents/mod.rs:813-816,1006,965,694-709`（跳过/None/default）；`turn.rs:515-519`（信号行解析失败跳过且 `emit_turn_signal` 错误被 `let _ =` 忽略）；结果截断 200 无提示（`857`）。
- 影响：用户看到「无结果 / 数字偏低」而不知道是文件坏了、读失败还是真没有；统计与搜索都可能静默不完整。
- 反证：对「一个坏文件不拖垮全局」是有意设计（注释 473-481、700）；没有 freshness 契约就不是「错误结果」，而是「无从判断」。

## 7. 正面证据（值得保留的设计）

- 互斥写租约 + Codex `thread/unsubscribe` 后再终止 + 启动失败 guard（`agent_chat.rs:154-197,1616-1663,3784-3815`；`runtime.rs:55-91`）：把「同一会话并发写」变成显式拒绝，而不是损坏 transcript。
- program/arg 级引号（`agent_command.rs:47-88`）+ session id 白名单（`lib.rs:859-865,1233-1239`）+ chat 参数白名单（`lib.rs:964-991`）：typed 路径的转义是正确习惯（对比 CCV-01 的例外）。
- 搜索的元数据先行 → 字节粗筛 → 解析 → (path, mtime) 用户正文缓存 → 代际取消 → 200 条截断（`agents/mod.rs:789-859,988-1034,1056-1085`）：在「无索引」前提下做到了可用时延与明确上限。
- Codex app-server 结构化审批/提问、权限模式显式映射、超时失败（`agent_chat.rs:757-855,2662-2847,2901-3082`）。
- provider trait + 存储单元边界（File/Directory）+ 删除/恢复校验钩子（`agents/mod.rs:111-158,531-634`）让 trash/worktree 等外部模块不感知格式差异。

## 8. 与 ASG 对照：跳到正确消息 / 分支细节

### 8.1 cc-sessions-viewer 在这件事上强在哪

1. 命中结果自带跳转坐标：`SearchHit { match_msg_index, match_msg_uuid, pi_leaf_id }`（`agents/mod.rs:931-949`）。前端可用 uuid 或下标在加载后的 `Msg[]` 中定位并闪烁。
2. Pi 真正做了「分支感知搜索」：遍历所有 `terminal` 叶子，按 lineage 找正文命中，并把 `pi_leaf_id` 一并返回（`mod.rs:895-902,956-986`）；读取侧有 `read_session_at(path, leaf)` 钩子（`249-253`），即「跳到正确分支」的雏形。
3. 关键词缓存按 (path, mtime) 失效、取消按代际让位、结果硬上限 200 —— 跳转体验的工程配套完整。

### 8.2 弱在哪（相对 ASG 的既有契约）

1. **没有一等公民的消息身份模型**。跳转坐标是「解析后数组下标」+ 可选 uuid；多数 provider（codex/agy/kimi/opencode/…）`Msg.uuid=None`（`remember_msg`/`simple_msg` 生成路径 `agent_chat.rs:234-276`、`util.rs:347-357`）。数组下标会随文件被改写、sidechain 过滤、解析策略变化而漂移；搜索与加载之间没有 occurrence/placement 复核。ASG 侧有 native id + parent 边 + placement 身份可对照（见 8.4 锚点）。
2. **首命中即止**：`scan_user_text` 只返回第一条命中消息（`1037-1048`），同一会话多处命中无法在结果里跳到第二处；对「我记得在哪个分支问过两次」的场景信息不足。
3. **分支处理是 Pi 特例而非通用能力**：Claude 的 sidechain/分支、Codex 的线性 thread/turn 都没有等价的「选取真实叶子再跳」语义；`read_session_at` 默认忽略 `leaf_id`（`251-253`）。
4. **搜索语义混入标题**：title 通常派生自首条用户文本，`keyword` scope 又在正文之前查 title（`883-891`），同一内容可能以「标题」字段先被报告，正文定位信息（msg_index/uuid）反而是 None——对「跳到那条消息」不友好。
5. 快照本身可证明「快照内无 LICENSE / 组件缺失」，因此任何「可学」都必须止于行为与结构，不能把代码文本当作可复制的 MIT 资产（CCV-04）。

### 8.3 ASG 该学 / 不该学

该学：

- 命中结果附带「打开时要落到哪个分支/叶子」的稳定坐标（cc-sessions-viewer 的 `pi_leaf_id` 思路），而不只是消息数组下标；
- 二级搜索结构（元数据 → 粗筛 → 精确匹配）与「按文件 mtime 失效的用户正文缓存」这类不改变语义的加速层；
- 取消代际（`SEARCH_GEN`）与显式结果上限、可预期 snippet 窗口（±60 字符 + 省略号 + 空白折叠）；
- 若 ASG 将来做 resume/写路径：按 (agent, session) 的互斥写租约与「先优雅退订再杀进程」的收尾顺序。

不该学：

- 用「数组下标 + 可选 uuid」当跳转身份，没有 occurrence/placement 复核 —— ASG 的 placement/parent 模型更强，不要退化；
- provider 特例式的分支搜索（只给 Pi 实现），ASG 应保持统一的图/分支选择语义；
- 进程生命周期内无界、无淘汰的缓存；
- `extra_args` 之类的「用户附加参数」直接拼进 shell 字符串；ASG 若保留恢复字符串，应继续走结构化 argv/cwd + 目标 shell 单独转义（与 sessiongrep 审计 SG-08 的教训一致）；
- 静默 partial（无 freshness/失败标记）与「标题/正文混在一个 scope」的含糊语义；
- 对用户配置文件非原子的覆写（Claude/Codex/Agy JSON 路径）。

### 8.4 ASG 既有契约锚点（本分片只读 spec，未重审实现）

- `.trellis/spec/agentsessions-provider-claude/backend/index.md:24-25,28,60-63,89-90`：native `uuid` 身份、`parentUuid` 线程边、`isSidechain` 标记、native id 经存储到 show 的保留要求。
- `.trellis/spec/agentsessions-provider-codex/backend/index.md:31-36,72-73`：occurrence-local 时间戳；Codex 线性序列、`parent_native_id` 恒 None，禁止伪造线程。
- `.trellis/spec/agentsessions-domain/backend/index.md:17-28,46,66,74`：PlacementId occurrence 身份；placement-aware 分支选择、真实叶子、`AmbiguousGraph`、跨文档父解析；`StableId::native_checked`；孤儿父边仍有效、父放置歧义要显式暴露。

## 9. 残余未决与 Caveats

- `util.rs` 未读区间 `751-997,1731-2043`（内联测试与注释缝）没有进入证据面；本报告对 util.rs 的结论仅覆盖产品函数。
- 7 家 provider 的解析器实现（`claude.rs` 等 10 万+ 字节文件）只通过 trait 覆盖点、resume 命令片段与 T2 探针命中观察；其字段级解析保真、出错细节、chat 协议容错未在本分片展开。
- `pty.rs`、`lib.rs`、`runtime.rs` 为抽查而非 T1 全文；PTY 侧的完整命令面（`spawn_terminal`、shell PTY）仍有未读区域。
- 所有 Finding 均为静态分析：未 build/test/install、未联网、未运行任何 resume/fork/搜索；CCV-01 的注入与 CCV-02 的 panic 都未实际执行验证。
- 快照完整性：`src/settings.ts`（`App.vue:9-29` 引用）与 `README.ja.md`（`README.md:12` 引用）在快照/清单中缺失；`package.json` 声明 `private: true` 且无 `license` 字段。
- 哈希口径差异：分片提示词写 `4b3b8c6c...`，但 `research/*.json|*.md` 字面检索无此串；`coverage-index.json` 现记录 `tree_content_sha256=0ee8993caf13b19ef4d7d0d31d0bb1d3d71708877cabe6f5b3df30ba4ee4afd7`、`commit=null`、93 文件。本报告采用后者的逐文件 SHA256 回执；该差异未解决，不影响本次 T1 行级阅读凭证。
- 许可证问题无法对上游取证（无 `.git`、快照不完整）；「没有 LICENSE」仅限本快照。
- 未验证：启动/关闭时 child 订阅清理的实际行为、审批前缀自动放行的真实触发、缓存内存曲线、搜索性能与准确性基准、各 provider CLI 版本差异。

## 10. Related specs / 已读上下文

- 任务 `prd.md:15,18-24,26-34`：T1/T2/T3 覆盖模型、证据等级与反证要求；`worker-queue.md:18` 是本分片清单与交付物来源。
- `.trellis/spec/agentsessions-provider-claude/backend/index.md:19-90`、`agentsessions-provider-codex/backend/index.md:18-76`、`agentsessions-domain/backend/index.md:17-74`：ASG 身份/父边/分支契约（只作对照，未重审实现）。
- `research/coverage-sessiongrep.json` 与 `sessiongrep-full-audit.md`：本报告采用的覆盖回执格式与「静态 vs 行为验证」边界基准。
- `research/sweep-cc-sessions-viewer.json`：T2 机械命中（只作定位，不作行为证据）。

