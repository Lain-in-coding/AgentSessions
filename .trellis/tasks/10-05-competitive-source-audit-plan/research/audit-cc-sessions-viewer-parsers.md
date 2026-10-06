# 竞品源码审计：cc-sessions-viewer 分片 B —— 7 家 provider parser 核对

- Query：对 `Github_src/cc-sessions-viewer`（无 Git 元数据，package 0.3.25 目录快照）做分片 B 静态审计：逐家核对 `src-tauri/src/agents/` 下 7 家 provider（pi / opencode / agy / claude / codex / grok / kimi）的存储形态、ID 提取、消息/角色判定、“只索引用户消息”的真实语义、tool/工具结果是否参与检索、resume/恢复命令与 fork/树能力、失败行为与能力缺失处理，并给出与 ASG（agent-session-grep）的「广度 vs 保真深度」对照。
- Scope：internal；仅该目录快照的静态源码，不 build / test / install / 网络 / git 写；不实施整改。分片 A 产物（`coverage-cc-sessions-viewer-core.json` / `audit-cc-sessions-viewer-core.md`）只引用不修改。
- Date：2026-10-06。
- Snapshot：`commit = null`（无 `.git`）；版本 0.3.25（`package.json:4`、`src-tauri/Cargo.toml:3`，来自分片 A 记录）。身份 = 7 个文件读前/读后 SHA256（见 `coverage-cc-sessions-viewer-parsers.json`），读后全部一致。
- Status：T1 阅读完成（3 full + 4 partial）；所有结论为静态证据，未运行产品，未验证上游最新状态。

## 1. 阅读凭证与覆盖统计

| 文件 | 行数 | 状态 | 已读行数 | 未读行数 |
|---|---|---:|---:|---:|
| `src-tauri/src/agents/pi.rs` | 2546 | full（1..EOF 连续窗口） | 2546 | 0 |
| `src-tauri/src/agents/opencode.rs` | 1870 | full（1..EOF 连续窗口） | 1870 | 0 |
| `src-tauri/src/agents/agy.rs` | 1321 | full（1..EOF 连续窗口） | 1321 | 0 |
| `src-tauri/src/agents/claude.rs` | 4101 | partial（发现/ID/消息解析/user 判定/resume/fork 区间） | 1500 | 2601 |
| `src-tauri/src/agents/codex.rs` | 3950 | partial（同上 + 工具/apply_patch 区间） | 1948 | 2002 |
| `src-tauri/src/agents/grok.rs` | 3182 | partial（发现/ID/消息解析/tool upsert/resume） | 955 | 2227 |
| `src-tauri/src/agents/kimi.rs` | 3178 | partial（发现/ID/消息解析/检索/统计/resume） | 1298 | 1880 |
| **合计** | **20148** | 3 full + 4 partial | **11438** | **8710** |

- 每个读取窗口 ≤150 逻辑行、未截断、PowerShell 行号输出（`[System.IO.File]::ReadAllLines`，1-based 闭区间）；哈希从未当阅读。精确 read_ranges / missing_ranges 见 JSON 凭证。
- 读后 SHA256 与读前一致（7/7，`content_unchanged=true`）：pi `9E612D…8335`、opencode `82DBE5…846A`、agy `A48EE3…6DF2`、claude `6AFB7E…058C`、codex `27BBF8…D4C7`、grok `9C7339…5CFA`、kimi `34BA5E…1206`（全文见 JSON）。
- T2 交叉核对（主会话机械产物，未整读）：`research/sweep-cc-sessions-viewer.json` 93 文件扫描；本分片仅按路径过滤查询 7 个 parser 文件的 hits（`provider_paths / resume_launch / inline_tests / ignored_result / unwrap_or / truncation_limit / sql_dynamic / concurrency / secret_redaction / surface_mcp`），未以探针计数替代阅读。
- 抽查补充（T1 外、只作交叉引用，不计入覆盖）：`agents/mod.rs` 的 251-253/257-264/274-289/323-347/438-447/508-542/950-1060（共享检索层与能力默认值）；`research/asg-provider-matrix.json`（ASG 对照口径）。

## 2. 七家逐家事实（存储 / ID / 角色 / 检索 / 恢复 / 失败）

### 2.1 Pi（`pi.rs`，full）

- 存储形态：可配置根下的 append-only JSONL（环境变量 `PI_CODING_AGENT_SESSION_DIR` > `settings.json::sessionDir` > `~/.pi/agent/sessions`；`pi.rs:94-157`）；首行 `{type:"session",id,version,cwd}`（`216-255`）；明确不读 auth/model/credential 文件（`1-5,107-112`）。
- ID 提取：会话 ID = header `id`（`220-234`）；条目 ID = `id` 字段，v1 回退 `v1:{ordinal}`（`385-393`）；重复 ID 置 `duplicate_ids`（`394-397`）。
- 消息/角色判定：`message.role` 驱动：`user`/`assistant`/`toolResult`/`bashExecution`/`custom`（`783-928`）；`hookMessage`/`custom_message` 仅在 `display=true` 时渲染（`929-972`）；`compaction`/`branch_summary` 的 summary 以 role=user 呈现（`973-996`）；中断回合标记 `cancelled`（`826-844`）。
- 只索引 user 消息？检索层结论（见 §5）：正文可检索集合 = role=user 且 `kind=="text"` 的块。Pi 的 `bashExecution` 文本（`889-904`）与 compaction summary（`973-996`）会命中；`toolResult` 是 `kind=tool_result`（`859-887`）不会命中。
- 树/fork：唯一提供完整树能力：`session_tree`（`1526-1529`）、`read_session_at` 按 leaf 沿 parentId 回溯（`439-471,1503-1520`）、`session_export_json`（`1530-1562`）、结构安全校验 `tree_is_unsafe`（重复/环/悬空父，`498-522`）；rename 是 append `session_info`（`1563-1589`），v1 或结构不安全时拒绝（`1569-1574`）。
- resume：`pi --session <path>`（`1595-1597`）；终端启动要求 cwd 与 header 一致（`1687-1705`）。`fork_session` 未实现 → trait 默认 Err（`mod.rs:274-281`）。
- 失败行为：坏行 JSON 解析失败即跳过（`377-380`）；>64MB 文件不进发现（`32,294-296`）；读取用 size+mtime+identity 快照，最多重试 3 次后 Err（`330-343`）；工程上偏 fail-closed 的写入/精确读取 vs fail-open 的目录扫描。

### 2.2 opencode（`opencode.rs`，full）

- 存储形态：单 SQLite 库 `~/.local/share/opencode/opencode.db`（XDG_DATA_HOME 感知；表 `project/session/message/part`；`opencode.rs:1-27,55-67`）；`SessionMeta.path` 是虚拟路径 `opencode://<session_id>`（`51,73-81,136`）；一律只读连接 + 2s busy_timeout（`87-99`）。
- ID 提取：session 行 `id`（`ses_…`）；path 反解 = strip `opencode://`（`77-81`）。
- 消息/角色判定：message 信封 `$.role`（`789-793`）；part 类型 → 块：`text/reasoning/tool/file`（`494-605`）；tool 的 `state.status` ∈ completed/error 才产出结果块（`532-583`）；user 消息里的 `<system-reminder>` 拆为独立系统 Msg（`805-830`）；`@file` Read 注入剥离并还原 file chip（`697-906`）；长 `# ` 文本标 `context`（`682-695,910-916`）。
- 只索引 user 消息？预筛为 SQL `instr(lower(part.data))`（`1370-1385`，全 part 过宽预筛）；精确层同上（role=user 且 text 块）。tool 的 `tool_result` 块（kind 不同）不命中；但系统提醒/context 写进 role=user 的 text 块，会命中。
- 写路径（唯一对上游库写）：rename = 单列 `UPDATE session.title`（`1220-1235`）；软删除 = dump 全行为 JSONL → 事务内 DELETE（失败回滚并删 dump，`333-377`）；恢复 = 动态列 INSERT（`391-466`）。统计发现包含子 agent 会话（`1143-1173,1344-1360`），列表只含 `parent_id IS NULL`（`1103-1141`）。
- resume：`opencode --session <session_id>`（`1264-1268`）。
- 失败行为：库不存在 → 列表空 Ok（`1188-1191,1204-1208`）；打开/查询失败 → Err 上抛；part/envelope JSON 解析失败 → 跳过或 Null（`638-640,662`）；companion 查询错误静默吞掉（`1352-1360`）。

### 2.3 agy / Antigravity CLI（`agy.rs`，full）

- 存储形态：`~/.gemini/antigravity-cli/brain/<uuid>/.system_generated/logs/transcript.jsonl`（或 `transcript_full.jsonl`，取更大文件；`agy.rs:1-10,95-109`）；`history.jsonl` 提供 conversationId→workspace（`113-133`）；IDE 变体 `~/.gemini/antigravity` 与 CLI 合并、同 UUID CLI 优先（`61-93`）。
- ID 提取：会话 = UUID 目录名（`56-93`）；`SessionMeta.id=conv_id`（`974-976`）。
- 消息/角色判定：`type` + `source` 双键：`USER_INPUT`+`USER_EXPLICIT` → user（`<USER_REQUEST>` 剥壳 `187-196`；`562-579`）；`PLANNER_RESPONSE`+`MODEL` → assistant（thinking/text/tool_use，`582-637`）；`VIEW_FILE/LIST_DIRECTORY/GREP_SEARCH/RUN_COMMAND/SEARCH_WEB/ASK_QUESTION` → role=user + `tool_result` 块（`639-655`）；`CODE_ACTION` → 结构化 diff 或文本兜底（`657-693`）；`CHECKPOINT/SYSTEM_MESSAGE` → role=user text + meta system（`695-713`）。
- 只索引 user 消息？正文侧：USER_INPUT、CHECKPOINT/SYSTEM 文本可命中；工具结果块（`tool_result`）不命中。
- 历史恢复（保真亮点+坑）：CHECKPOINT 会截断 transcript；`recover_early_lines` 在读取路径内启动 `git log/show` 二分找“最后一个从 step 0 开始的 commit”，拼接 `step_index<first_step` 的行（`434-526`）；`read()` 无条件尝试（`528-546`）。任一失败静默返回空。

- resume：`agy --conversation <uuid>`（`1030-1034`）；IDE 会话不承诺 CLI resume（`61-63`，IDE cwd 标 `ide://antigravity-chat` `967-969`）。
- 失败行为：目录/条目缺失 → 跳过/空；行 JSON 坏 → 跳过（`550-553`）；transcript 打不开 → Err（`531-532`）；`read_turns` 文件不存在 → Err（`1053-1056`）；usage 恒零占位（`1044-1046`）；git 恢复失败静默降级。

### 2.4 claude / Claude Code（`claude.rs`，partial）

- 存储形态：`~/.claude/projects/<encoded-dir>/<sessionId>.jsonl`（`32-34`）；子代理 `<sessionId>/subagents/*.jsonl`（`162-198,1065-1073`）。
- ID 提取：文件名去 `.jsonl`；subagent 文件的 id 折叠为父 UUID（`1315-1327`）；rename 写 `custom-title`+`agent-name` 双记录（`200-227,1329-1391`）。
- 消息/角色判定：`record_to_msg` 统一归一：`attachment`（queued_command/file）→ user（`1541-1565`）；`system`+`subtype:local_command` → user + command-output（`1566-1599`）；常规 user/assistant 的 content 块 → text/thinking/tool_use/tool_result/image（`1621-1707`）；`<synthetic>` “No response requested.” 丢弃（`1715-1725`）；`[Image: source:]` meta 去重（`1726-1731`）；系统注入分类 `classify_meta_kind`（`838-911`）与 `is_injected_user`（`916-949`，含内容前缀兜底）。
- 只索引 user 消息？正文侧：真实用户文本 + 命令输出/技能注入等伪 user 文本（仍 role=user、kind=text）可命中；tool_result/thinking/image 不命中。
- fork/树：磁盘级 fork：整文件克隆 + `sessionId/uuid/parentUuid/leafUuid` 重写 + 尾部 title（`1082-1094,1196-1239`）；`fork_session_before_user_turn` 截断到目标提问之前（`1097-1191`）；实现 trait `fork_session/fork_session_before_user_turn`（`229-246`）。无通用会话树。
- resume/chat：`claude --resume <id>`（`252-254`）；`chat_command` 长驻 stream-json（`264-308`），`fork=true` 时追加 `--fork-session`（`301-305`）。
- 失败行为：projects 目录不存在 → Ok 空（`38-44`）；行坏 → 跳过（`1465-1468`）；usage 文件打不开 → 默认零（`382-385`）；scan 读文件失败 → 全默认 meta（`1337`）。

### 2.5 codex / Codex CLI（`codex.rs`，partial）

- 存储形态：`~/.codex/sessions/**/*.jsonl`（归档另在 `archived_sessions`；`41-47,211-233`）；首行 `session_meta`（id/cwd/时间，`449-475`）；标题索引 `session_index.jsonl`（`477-513`）；`state_<N>.sqlite` 提供 internal/archived flags（`49-70,113-183`）；列表页可选 `codex app-server` 线程列表（`299-447`）。
- ID 提取：`session_meta.payload.id`，空则回退文件名（`1378-1382`）；rename 时文件名与 meta id 都可作 fallback（`2206-2231`）。
- 消息/角色判定：`response_item.message(role=user)` 只抢救图片（`1489-1496`）；`event_msg.user_message` → user 气泡（含附件/占位符绑定，`1497-1544`）；`agent_message` → assistant（`1546-1562`）；`item_completed` 的 UserMessage/AgentMessage/Plan（`1564-1637`）；`function_call/custom_tool_call` → assistant tool_use（`1648-1698`）；`function_call_output` → role=user 的 tool_result/image（`1699-1786`）；`patch_apply_end` 回填 apply_patch diff（`1788-1817`）；`thread_name_updated` → rename 系统消息（`1639-1647,1211-1223`）。
- 只索引 user 消息？user_message 文本可命中；rename 系统提醒（role=user+text，`1217-1223`）也会命中；tool 输出块（kind=tool_result）不命中。
- resume/chat：`codex resume <id>`（`2328-2330`）；GUI chat 走 app-server（`2388-2392`），`chat_turn_command` 为 `codex exec [resume <id>] --json`（`2394-2410`）。
- rename 写路径三联：rollout 追加 `thread_name_updated`（`2233-2251`）+ `session_index.jsonl` 原子替换（`2253-2306`）+ `state sqlite UPDATE threads.title`（`2308-2320`；库缺失静默跳过）。
- 失败行为：首行不是 `session_meta` → 该文件在所有列表入口被静默跳过（`450-475` + `2123-2125,2168-2178`）；flags sqlite 打开/语句失败 → 全默认 flags（`113-154`）；行坏 → 跳过（`1447-1450`）；token_count 累计被 reset（resume 边界）时按新基线继续（`2674-2684`）；usage 取 last_token_usage 优先、total 差值兜底（`2444-2463`）。

### 2.6 grok / Grok Build（`grok.rs`，partial）

- 存储形态：`~/.grok/sessions/<group>/<session_id>/` 目录：`updates.jsonl`（权威 UI 事件流）+ `summary.json`（列表元数据）+ `signals.json`（上下文用量）；`chat_history.jsonl` 明确不作 UI transcript（`1-6,33-35,74-76,216-285`）。
- ID 提取：会话目录名（`251-257`）；`find_updates_path` 用 id+cwd 反查（`41-51`）。
- 消息/角色判定：JSON-RPC `method=session/update`（`470-475`）；`user_message_chunk` → user text（system-reminder 转系统注记，`1021-1027,911-922`）；`agent_message_chunk` → assistant text；`agent_thought_chunk` → assistant thinking（`1029-1044`）；流式 chunk 按 promptIndex/streamStartMs 合并（`500-589`）；`tool_call` → assistant tool_use，`tool_call_update` completed/failed → tool_result 与错误态（`1045-1115,944-978`）；plan/retry/hook/recap/unsupported 事件 → 文本或系统注记（`1117-1211`）；系统注记由 `meta_message` 写成 role=user+text（`880-905`）。
- 只索引 user 消息？user chunk 可命中；系统注记/retry/recap（role=user+text）也会命中；tool_result 块不命中。
- resume：`grok --resume <id>`（`1939-1941`）。
- 失败行为：sessions 目录缺失 → Ok 空（`222-224`）；无 `updates.jsonl` → 跳过（`247-249`）；JSONL 尾行可能半截写入 → 解析失败跳过（`996-1003`，注释明示 streaming 容忍）；summary 损坏 → 默认值仍可发现（`155-161`；测试 `2229+`）；列表过滤 hidden/subagent，统计包含它们（`258-271,1988-1993`）。

### 2.7 kimi / Kimi Code（`kimi.rs`，partial）

- 存储形态：`~/.kimi-code/sessions/<group>/<id>/` 目录：`state.json`（id/cwd/title/lastPrompt/时间）+ `agents/main/wire.jsonl` 主 transcript（+ 子代理 wire）；根 `session_index.jsonl`（`37-47,87-97`）。
- ID 提取：`state.id` 优先、目录名兜底（`1001-1009`）；hook 用 `session_id+cwd` 反查主 wire（`1070-1085`）。
- 消息/角色判定：`turn.prompt` 且 `origin.kind=="user"`（缺失视为 legacy user；`506-515`）→ user（`851-872,522-548`）；`context.append_loop_event`：`content.part` → assistant text/thinking（`902-924`）；`tool.call` → assistant tool_use（含 AskUserQuestion 归一与 diff，`925-971,584-648`）；`tool.result` → role=user 的 tool_result 块（`972-994`）；无主事件时回退 `context.append_message`（`777-814,830-837`）。
- 只索引 user 消息？contains_text 覆写为**仅扫 user prompts**（`2006-2014`）；精确层同为 role=user+text。tool_result 不命中；系统触发 prompt 由 `is_user_prompt` 排除（`506-515`）——7 家中语义最严格。
- resume：`kimi --session <session_id>`（`1948-1950`）。
- 写/删除能力：trash_metadata / before_soft_delete（revision 校验）/ after_soft_delete（同步 session_index）/ after_restore / hard_delete_session（`2056-2082`）。
- 失败行为：sessions 目录缺失 → Ok 空（`1042-1045`）；目录不合法 → 跳过（`1054-1056`）；坏行 → 跳过（`525-527,1724-1726`）；读取用 size+mtime+identity 快照重试 4 次（`46-47,133-206`）；输入/结果有字节上限截断（`41-45,557-573,678-700`）；contains_text 出错 → false（fail-open，`2006-2014`）。

## 3. resume / fork / 树能力总表（Pi 特例）

| provider | resume 命令（锚点） | fork | 树 / 分支 | 备注 |
|---|---|---|---|---|
| pi | `pi --session <path>`（`pi.rs:1595-1597`） | 无（默认 Err，`mod.rs:274-281`） | **唯一**：parentId 树 + leaf 读取 + 导出 + 环/重复检测（`pi.rs:1526-1562,439-522`） | resume 前校验 cwd 与 header 一致（`1687-1705`） |
| opencode | `opencode --session <id>`（`opencode.rs:1264-1268`） | 无 | 无（parent_id 只用于子 agent 归属/统计） | 虚拟路径，无文件 fork 语义 |
| agy | `agy --conversation <uuid>`（`agy.rs:1030-1034`） | 无 | 无 | IDE 会话不承诺 resume；早期历史用 .git 恢复 |
| claude | `claude --resume <id>`（`claude.rs:252-254`） | **唯一磁盘 fork**：克隆 + uuid 重写（`1082-1239`） | 无通用树 | GUI `--fork-session` 侧聊（`301-305`） |
| codex | `codex resume <id>`（`codex.rs:2328-2330`） | 无（trait 层） | 无（thread/turn 线性） | GUI fork 走 app-server `thread/fork`（`agent_chat.rs`，分片 A） |
| grok | `grok --resume <id>`（`grok.rs:1939-1941`） | 无 | 无 | — |
| kimi | `kimi --session <id>`（`kimi.rs:1948-1950`） | 无 | 无 | — |

- 能力缺失处理：trait 以显式默认值收口 —— `read_session_at` 回落 `read_session`（`mod.rs:251-253`）；`session_tree`/`session_export_json` 默认 Err（`257-264`）；`fork_session`/`fork_session_before_user_turn` 默认 Err（`274-289`）；`chat_command`/`chat_turn_command` 默认 None（`323-332,438-447`）；`parse_chat_line` 默认 Ignore（`339-341`）；`chat_process_model` 默认 LongLivedStdin（`345-347`），Codex 覆写为 CodexAppServer（`codex.rs:2388-2392`）。即：缺失能力统一“显式报错/隐藏”，不做静默假成功。

## 4. 失败行为与能力缺失逐家差异（fail-open / fail-closed）

| provider | 发现层 | 行级解析 | 快照/并发 | 定性 |
|---|---|---|---|---|
| pi | 坏 header/超限/非 jsonl → 跳过；根不存在 → 空（`257-315`） | 坏行跳过（`377-380`） | 3 次 revision 重试，仍不稳定 → Err（`330-343`） | 扫描 fail-open / 精确读写 fail-closed |
| opencode | 库缺失 → 空；打开失败 → Err（`1188-1191,1204-1212`） | part/envelope 坏 → 跳过/Null（`638-640,662`） | busy_timeout 2s + WAL 并发读（`18-20,87-99`）；删除事务回滚（`365-377`） | 库级 fail-closed，行级 fail-open |
| agy | 目录/条目缺失 → 空/跳过 | 坏行跳过（`550-553`） | 无快照；git 恢复失败静默（`434-526`） | 整体 fail-open（静默降级最多） |
| claude | 目录缺失 → 空（`38-44`） | 坏行跳过（`1465-1468`） | 无快照；scan 全默认容错（`1337`） | 整体 fail-open |
| codex | 首行无效 → 该会话静默消失；sqlite 不可用 → 默认 flags（`450-475,113-154`） | 坏行跳过（`1447-1450`） | 无快照 | 发现层静默 fail-closed（丢文件不报错） |
| grok | 目录缺失 → 空；无 updates → 跳过 | 半截行跳过（`996-1003`） | 无快照；容忍 streaming 尾行 | 整体 fail-open（有注释说明） |
| kimi | 目录缺失 → 空；非法目录跳过（`1042-1056`） | 坏行跳过（`525-527,1724-1726`） | 4 次 revision 重试；部分写入拒绝（`46-47,133-206`） | 扫描 fail-open / 快照 fail-closed；contains_text 失败 → false |

- 共同点：7 家都以“读尽量成功、坏行跳过”为基调（viewer 定位）；没有一家在行级解析失败时向 UI 报告“该会话有 X 行未能解析”。
- 差异点：同样“源文件不可信”，codex `meta()` 让会话整体消失（`450-475`），opencode/pi/kimi 则返回 Err 或明确跳过；agy 的 git 恢复失败无任何信号（`434-526`）。

## 5. 检索语义专项：是否只索引 user 消息、tool 结果是否参与

- 设计意图（共享层注释，分片 A 范围、此处引用）：`mod.rs:988-991` 明确“**仅匹配「用户消息的 text 块」** —— 助手回复 / thinking / tool_use / tool_result / 图片全部跳过”。
- 实现（精确层）：`mod.rs:1008-1011` 过滤 `msg.role == "user"`，`1016-1019` 过滤 `block.kind != "text"`；**没有 meta_kind/sidechain 过滤**（`1008-1030`）。Pi 专用路径同样只取 role=user 的 text 块（`mod.rs:963-981`）。
- 结论一（tool/工具结果）：**所有 7 家的工具结果都不参与检索**——因为各家工具结果都落在 `kind=="tool_result"` 块（agy `644-654`；opencode `571-582`；pi `859-887`；claude `1663-1692`；codex `1744-1765`；grok `1099-1115`；kimi `985-993`），被 kind 过滤挡掉。
- 结论二（伪用户文本会参与）：凡被 provider 归一成 role=user+text 的系统性内容都会命中检索，例如 grok 的系统注记/recap/retry（`880-905,1143-1163`）、agy 的 CHECKPOINT/SYSTEM_MESSAGE（`695-713`）、pi 的 bashExecution 与 compaction summary（`889-904,973-996`）、claude 的 local-command 输出（`1574-1599`）、codex 的 rename 系统提醒（`1217-1223`）、opencode 的 system-reminder/context（`805-830,910-916`）。
- 预筛层（contains_text）范围不一致：claude/codex/grok 未覆写 → 默认整文件字节扫描（`mod.rs:508-515`）；opencode 覆写为全 part `instr`（`1370-1385`）；kimi 覆写为**仅 user prompts**（`2006-2014`）。过宽预筛只增加成本不致漏检；kimi 的窄预筛与其精确层一致。
- 反证/界限：`msg.role=="user"` 是各 provider 归一后的语义角色，不等于“人类手打”；把 command-output/系统注记标为 role=user 是渲染管线的选择（前端按 meta_kind 低调展示，`claude.rs:1732-1738` 等），检索层没有读 meta_kind——两件事实叠加才是“伪用户文本可检索”的根因。未发现助手正文/tool 结果被检索的路径。

## 6. 关键发现（严重度 / 证据 / 反证）

### CCVP-01 · P2 · 检索语义宽于“只匹配用户消息”：伪用户系统文本可被检索

- **证据：**精确层无 meta_kind 过滤（`mod.rs:1008-1019`）；grok 系统注记 role=user+text（`grok.rs:880-905,1022-1026,1143-1163`）；agy CHECKPOINT/SYSTEM_MESSAGE（`agy.rs:695-713`）；pi bashExecution/compaction（`pi.rs:889-904,973-996`）；claude local-command 输出（`claude.rs:1574-1599`）；codex rename 提醒（`codex.rs:1217-1223`）；opencode system/context（`opencode.rs:805-830,910-916`）。
- **反证/界限：**设计注释明确目标是“用户最常见检索轴”（`mod.rs:988-991`）；kimi 两层都严格限定 user prompts（`kimi.rs:506-515,2006-2014`）；以上文本在前端是折叠的系统卡片而非用户气泡——这是“可检索范围”的语义边界问题，不是渲染错乱；未实测某条具体查询在 UI 的呈现。
- **ASG 教训：**若 ASG 声明“user-only 检索”，应对“provider 归一后的 user 角色”和“人类输入”给出各自的显式契约与可配置范围。

### CCVP-02 · P2 · agy 早期历史恢复依赖未文档化的每会话 .git，且读路径内启动 git 进程、失败静默

- **证据：**`recover_early_lines` 在 `<uuid>/` 内执行 `git log --reverse` / `git show <commit>:…` 并二分（`agy.rs:452-526`）；`read()` 每次打开会话都尝试（`528-546`）；无 `.git`/命令失败/首行解析失败 → 返回空（`449-451,456-465,500-501`）；`preferred_transcript` 以文件大小选“更全”（`95-109`，注释承认两者都会被 CHECKPOINT 截断）。
- **反证/界限：**只有存在 `.git` 目录才尝试；`git log/show` 是只读查询；失败只损失被截断的早期消息，不阻断当前内容读取；这是快照内静态事实，未运行验证任何真实会话目录。
- **ASG 教训：**从“工具目录里的偶然产物（.git）”恢复历史属于越界依赖；即便使用也应报告本次恢复是否降级，而不是静默。

### CCVP-03 · P2 · codex 发现层静默丢会话、flags 静默降级

- **证据：**`meta()` 要求首行是 `session_meta`，否则 None（`codex.rs:450-475`）；list_projects/list_sessions 全部 `if let Some`（`2123-2125,2168-2178`）——整文件无声消失；`state_<N>.sqlite` 缺失/打开失败/查询失败 → 默认 flags（`113-126`），`flags_for` 兜底 default（`172-183`），此时不会识别 internal 子代理/审查会话（判定逻辑 `156-170`）。
- **反证/界限：**被丢弃的文件很可能本就不是合法 rollout；archived 仍可凭路径判定（`180-182`）；该降级只在 sqlite 不存在/不可读时发生（旧版 codex 或从未运行）；未实测“internal 会话泄漏进列表”的具体现象。
- **ASG 教训：**发现层应有“跳过原因/计数”契约；flags 这类增强元数据缺失应可被下游感知。

### CCVP-04 · P2 · 快照/并发读保护跨 provider 不对称

- **证据：**pi 3 次 revision（size+mtime+inode）比对（`pi.rs:317-343`）；kimi 4 次重试、部分写入直接拒绝（`kimi.rs:46-47,133-206`）；opencode 依赖 SQLite WAL + busy_timeout + 事务（`opencode.rs:18-20,87-99,365-377`）；claude/codex/grok 直读并按行容忍：claude（`claude.rs:1458-1468`）、codex（`codex.rs:1443-1450`）、grok 注释明确“半行是 streaming 常态”（`grok.rs:996-1003`）。
- **反证/界限：**JSONL 追加写入下，半行窗口极小且 watcher 会再次刷新；跳过坏行是“最终一致”的合理取舍；未做实机并发压测，不断言用户可见损坏。
- **ASG 教训：**对“正在写入的源”应统一快照/重读策略，或至少把“读到过未完成尾行”纳入错误分类。

### CCVP-05 · P3 · opencode 以长度阈值+`# ` 前缀判定“注入上下文”，可能误标真实长 Markdown 输入

- **证据：**`is_injected_context` = 单 text 块 && `len>500` && `trim_start().starts_with("# ")`（`opencode.rs:682-695`），命中即 meta_kind=`context`（`910-916`）；`last_prompt` 预筛同款启发式（`670-673,947`）。
- **反证/界限：**注释目标是 skill/context 文件注入（`680-681`）；误标最多影响卡片样式与标题选择，不丢内容；该消息仍可检索；未找到实际误判样本或复现。
- **ASG 教训：**结构性识别优先于尺寸阈值；确需启发式时至少保留原文与标记原因。

### CCVP-06 · P3 · `message_count` 口径跨 provider 不可比（user-only / user+assistant / 子串计数）

- **证据：**opencode 只数 role=user 信封（`opencode.rs:121-123`）；kimi 只数 user prompts（`kimi.rs:1087-1090`）；claude 数 user+assistant 行 + queued attachment（`claude.rs:1388-1389,1418-1424`）；pi 数 lineage 上的 user|assistant message（`pi.rs:1342-1354`）；codex 按 item_completed 去重计 user+agent（`codex.rs:1318-1320,1357-1374`）；agy 用行内子串 `"USER_INPUT"` 计数（`agy.rs:377-389`）；grok 数非 meta 且含 text/thinking/image/file 块的消息（`grok.rs:1218-1228`）。
- **反证/界限：**该字段是列表/统计展示口径，各 provider 内部自洽；agy 子串计数在合法 step 形状下与类型计数等价；这不是检索或数据损坏问题。

### CCVP-07 · P3 · codex rename 三联写入中存在“写入降级”与非常规只读连接

- **证据：**rename = rollout 追加（`codex.rs:2233-2251`）+ `session_index.jsonl` 原子替换（`2287-2306`）+ `state sqlite UPDATE threads.title`（`2308-2320`）；sqlite 不存在则跳过（`2310-2311`）；供只读查询用的 flags 索引也走普通 `Connection::open`（`117`），未用 SQLITE_OPEN_READ_ONLY（对照 opencode `opencode.rs:92-95`）。
- **反证/界限：**sqlite 更新语句只改 title/updated_at；跳过是显式设计（旧版兼容）；`Connection::open` 不写数据本身不等于会改动库；未运行验证对 live CLI 的任何影响。

### CCVP-08 · I · 能力矩阵（事实）：树/导出唯一 Pi、磁盘 fork 唯一 Claude、其余命中 trait 默认值

- **证据：**Pi 树/导出/leaf 读取（`pi.rs:1526-1562,439-522`）；Claude 磁盘 fork 与 fork-before-user-turn（`claude.rs:1082-1239,229-246`）；默认能力收口（`mod.rs:251-253,257-264,274-289,323-332,438-447`）；只有 Claude 提供 `chat_command`（`claude.rs:264-308`）、只有 Codex 覆写进程模型与 `chat_turn_command`（`codex.rs:2388-2410`）。
- **反证/界限：**其余 5 家并非“缺功能即 bug”：产品把 fork 定义为 Claude 专属派生语义（`mod.rs:270-273` 注释）；对缺失能力统一 Err/None，不伪装成功。

### CCVP-09 · P2 · 工具结果“不检索”是统一事实，但工具保真度全 7 家齐备（与 ASG 的深度差）

- **证据：**每家都把工具调用/结果结构化进 transcript：agy（`agy.rs:639-693`）、opencode（`opencode.rs:511-583`）、pi（`pi.rs:810-825,859-887,1001-1027`）、claude（`claude.rs:1645-1692`）、codex（`codex.rs:1648-1817`）、grok（`grok.rs:1045-1115`）、kimi（`kimi.rs:925-994`）。而检索层统一只看 user text（§5）。
- **反证/界限：**“不检索工具结果”是产品决策（`mod.rs:989-990` 明示跳过 tool_result），不是引擎做不到；与 ASG matrix 中 5/7 家 `tool_activity=unsupported` 相比，CCV 的渲染与检索是刻意分离的两层。

## 7. 与 ASG 对照：provider 广度 vs 保真深度

ASG 侧口径来自 `research/asg-provider-matrix.json`（导出快照：16 行 = 14 个有 variant 的 provider + deepseek-harness/zcode 两行 unsupported 占位；关键列 discover/parse/search/resume/tool_activity/usage/context/source_span 取值为 native/partial/derived/unsupported/unknown）。

- **广度：ASG 明显领先。**ASG 覆盖 aider、claude-code、codex、grok-build、pi、kimi-code、qoder、openclaw、tencent-codebuddy、opencode、cline、hermes、antigravity、cursor 共 14 个活跃 provider；CCV 注册 7 家（`agents/mod.rs:103-109`，分片 A 记录）。CCV 没有 aider/cursor/cline/hermes/qoder 等。
- **保真深度：CCV 全面领先于 ASG 矩阵自报值。**ASG 矩阵中 `tool_activity` 仅 claude-code/codex 为 partial、其余 12 家 unsupported，`usage` 仅 claude-code native（codex derived，其余 unsupported），`context` 仅 claude-code native；CCV 的 7 家每家都有：工具调用/结果结构化渲染（含 diff）、角色与注入分类、rename/trash/restore 等写路径、逐家 resume 命令构造、部分家 usage/cost 解码（pi/claude/codex/opencode/grok/kimi；agy 为零占位 `agy.rs:1044-1046`）。ASG 自报 opencode 的 `source_span`=unsupported（CCV opencode 走虚拟路径，同样没有文件级定位）；tencent-codebuddy 等则标 native。
- **该学的（优先）：**
  1. 伪用户内容分类与显式 meta_kind（`claude.rs:838-949`；`opencode.rs:668-695`；`grok.rs:880-905`）——可用于 ASG 的“检索范围契约”；
  2. 工具结果的结构化形状（file_path/diff/is_error；`codex.rs:1699-1817`、`opencode.rs:511-583`、`kimi.rs:705-775`）——ASG `tool_activity` 从 partial 走向 native 的参照；
  3. 写路径前置校验/事务（`pi.rs:498-522,1563-1588`、`kimi.rs:2060-2082`、`opencode.rs:365-377`）；
  4. 逐家 resume 命令事实库（本报告 §3 表）——ASG `resume=derived` 的落地参照（注意需按其自身 CLI 版本复核）；
  5. Pi 的树模型 + 结构安全校验（`pi.rs:439-522`）——对“分支/跳转”能力的直接竞品样例；
  6. 字节上限与截断标记（`kimi.rs:41-47,557-573`）防止超长注入拖垮解析。
- **不该学的：**
  1. 静默降级无可观测性（`codex.rs:450-475,113-154`；`agy.rs:434-526`；`opencode.rs:1352-1360`）——ASG 已有机器错误分类契约时应保持；
  2. 尺寸/前缀启发式当语义分类（`opencode.rs:682-695`）；
  3. 预筛层与精确层范围不一致（`mod.rs:508-515` vs `1008-1019`；`opencode.rs:1370-1385`）——只在性能上无害，语义上误导；
  4. “更大的文件更完整”（`agy.rs:95-109`）；
  5. 无持久索引的全量扫描（CCV 是 viewer 定位，此为其代价；ASG 若已有 FTS/DB，不应退回全量解析作为默认检索路径——该结论来自分片 A 的调用链记录，本报告不复述其细节）。
- **定位差异总结：**CCV 是“7 家 × 深水区”的本地会话浏览器（保真、可写、可恢复、可驱动 chat）；ASG 是“14 家 × 统一契约”的检索/交接层（广度、显式能力矩阵、可取证）。两者不是同一产品的缩放版；ASG 若要补深度，优先补 `tool_activity` 与 `resume 命令事实表`，而不是追平 CCV 的全部写路径。

## 8. Caveats / 残余未决

- **未运行任何东西**：无 build/test/install/network/git 执行（agy 的 git 子进程行为仅为源码事实，未实际触发）；行号锚点与结论仅对本次 SHA256 标识的快照成立；CLI 版本行为（opencode 1.17.x 等注释声明）未实测。
- **未读区间明确保留**：claude `1-29,630-829,980-1059,1810-4101`（含 slash 扫描/skill 扫描/read_turns/测试）；codex `1-34,335-439,739-1123,1874-2111,2411-2439,2740-3950`（含 app-server 细节/usage/测试）；grok `81-215,366-469,620-859,1285-1849,2000-3182`（含 tool_input/output 细节、read_turns、锁/原子写正文、测试）；kimi `151-365,815-824,1125-1714,2114-3178`（含 rename API/索引恢复正文、测试）。这些区间内的行为不在本报告断言范围。
- **grok 写路径**：`rename_session_at`/`write_json_atomically`/`summary lock`（函数索引 `grok.rs:1702,1743,1818`）未精读正文，只有函数名级证据，本报告未据此下结论。
- **T2 探针口径未决**：`sweep-cc-sessions-viewer.json` 显示 opencode.rs 的 `resume_launch=0`/`sql_dynamic=0`，但本文件确认 opencode 构造 resume 命令（`1264-1268`）并存在动态列名 SQL（`391-420`）——疑为探针模式与源码形态差异，未深挖；不影响本报告的源码事实。
- **ASG 对照为矩阵口径**：只读了 `research/asg-provider-matrix.json` 导出快照，未复核 ASG 实现；provider 数量取“14 活跃 + 2 占位”的静态计数。
- **分片边界**：`agent_chat.rs`/`turn.rs`/`agents/mod.rs`/`util.rs` 的深度结论以分片 A 为准；本报告对 `mod.rs` 的引用仅为检索层契约（§5）与能力默认值（§3）的交叉锚点。