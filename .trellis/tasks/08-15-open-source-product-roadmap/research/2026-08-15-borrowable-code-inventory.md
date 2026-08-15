# Research: 可直接借用代码清单(Borrowable Code Inventory)

- **Query**: 从 13 个 competitor 仓库中找出 agent-session-grep 可直接复制/改编的代码,按 8 大 feature area 映射到 08-15 子任务,并附 license 核验
- **Scope**: internal(全部为本地 clone,逐文件核验)
- **Date**: 2026-08-15
- **前置资料**: 同目录 `deep-read-*.md` 13 份精读报告(feature map + 行号线索);所有行号均经 Grep/Read 实测核验,未核验的明确标注

---

## 0. License 核验总表(2026-08-15 逐仓库读 LICENSE 文件头核验)

| 仓库 | License | 结论 | 核验方式 |
|---|---|---|---|
| hstry | MIT | 可复制+改编(注释/NOTICE 注明出处) | LICENSE 头 "MIT License Copyright (c) 2025 byteowlz" |
| fast-resume | MIT | 同上 | LICENSE 头 (c) 2025 Stanislas Lange |
| claude-historian-mcp | MIT | 同上 | LICENSE 头 (c) 2025 Claude Code Community |
| Recall | MIT | 同上 | LICENSE 头 (c) 2026 samzong |
| memex | MIT | 同上 | LICENSE 头 (c) 2024-2025 vimo-ai |
| sessiongrep | Apache-2.0 | 同上(需带 NOTICE) | LICENSE 头 Apache License 2.0 |
| cc-switch | MIT | 同上 | LICENSE 头 (c) 2025 Jason Young |
| agentsview | MIT | 同上 | LICENSE 头 (c) 2026 Kenn Software LLC |
| agent-sessions | MIT | 同上(仅 Swift/macOS,移植价值低) | LICENSE 头 (c) 2026 Alexander Malakhov |
| agf | MIT | 同上 | LICENSE 头 (c) 2025 subinium |
| AgentRecall | MIT | 同上(此前研究标注"私有 MIT"——实际仓库内 LICENSE 文件存在且为 MIT,已复核) | LICENSE 头 (c) 2026 AgentRecall contributors |
| ctx | Apache-2.0 | 同上 | LICENSE 头 Apache License 2.0 |
| coding_agent_session_search (cass) | MIT + OpenAI/Anthropic Restricted-Party Rider | **仅 clean-room idea,禁止复制任何代码/fixture** | LICENSE 头 "MIT License (with OpenAI/Anthropic Rider)";rider 原文:"no rights are granted to any Restricted Party"(OpenAI/Anthropic 及其关联方) |

复制义务:MIT 保留版权声明;Apache-2.0 需保留 LICENSE + NOTICE 文件。本项目 license 为 Apache-2.0(PRD Q26),借用 MIT 代码无冲突,借用 Apache-2.0 代码 (sessiongrep/ctx) 需在 NOTICE 中列出。

---

## 1. FTS5/SQLite 查询构建 → 子任务 #3 (semantic-hybrid-local-retrieval),兼 #2 检索质量

**I1. hstry `sanitize_fts_query`** — `hstry/crates/hstry-core/src/db.rs:3177`(纯函数,约 25 行)
- License: MIT(核验:读 LICENSE 头);复制方式:**verbatim**
- 内容:FTS5 查询净化——每 token 剥尾部 `*`、`"` 转义为 `""`、包成字符串字面量、前缀搜索保留 `*`;解决 `hp-z8` 被解析成 `hp NOT z8` 的问题
- 落点:adapters-sqlite FTS 查询构建;自带 5 条测试用例可一并照搬
- 风险:零依赖、纯字符串逻辑,直接可搬

**I2. hstry `detect_search_mode`** — `db.rs:3160`(约 15 行)
- License: MIT;复制方式:**verbatim**
- 内容:Auto 模式判别——query 含 `/` `\` `::` `->` `_` `.` camelCase 任一出则走 code 表,否则自然语言表(双 FTS5 表 porter / unicode61+tokenchars)
- 落点:adapters-sqlite 检索模式枚举;与本项目 CJK bigram(ADR-0007)可组合
- 风险:需确认本项目 FTS 表是否也分 code/NL 两表

**I3. hstry `search()` SQL 模板** — `db.rs:2102`(约 100 行)
- License: MIT;复制方式:**port with adaptation**
- 内容:`snippet({table}, 0, '[', ']', '…', 12)` + `bm25({table})` + source_id LIKE 前缀过滤 + after/before/role/model/harness 条件链 + `ORDER BY score ASC`
- 落点:adapters-sqlite 检索 SQL 构造
- 风险:与项目现有 schema 的列名对齐即可;`ORDER BY score ASC` 的 BM25 负分语义要保留注释说明

**I4. Recall `fts5_escape`** — `Recall/src/db/search.rs:295`
- License: MIT;复制方式:**verbatim**
- 内容:分词→过滤非 alphanumeric+_→小写→` OR ` 连接(与 hstry 加引号方案不同,是 OR 语义的宽召回)
- 落点:adapters-sqlite;可与 I1 并存(I1 保精度、I4 保召回)
- 风险:无

**I5. Recall `hybrid_search` + `rrf_merge`** — `Recall/src/db/search.rs:76,263`
- License: MIT;复制方式:**port with adaptation**
- 内容:FTS 腿(rank 聚合)+ vec 腿(fetch_k = limit*5)+ RRF(k 参数,`1/(k+rank+1)`)+ MatchSource(Fts/Vector/Hybrid)标记
- 落点:application 检索编排 + adapters-sqlite 查询;子任务 #3 的 hybrid 融合核心
- 风险:vec 腿依赖 sqlite-vec,与本项目本地 embedding 选型联动

**I6. memex `rrf_fusion` + `RRF_K=60`** — `memex/memex-rs/src/search/mod.rs:30,459`
- License: MIT;复制方式:**verbatim**(约 20 行)
- 内容:以 message_id 为 key 的 HashMap 累加 `1/(60+rank)`,k=60 经典经验值
- 落点:application;若 I5 的 RRF 实现与项目风格不合,用这个更小的替代
- 风险:与 I5 二选一,不要两套并存

**I7. Recall `claim_next_session_embedding_job`** — `Recall/src/db/semantic_store.rs:73`
- License: MIT;复制方式:**verbatim**
- 内容:单条 SQL 原子抢占(`UPDATE ... SET status='processing' WHERE session_id=(SELECT ... ORDER BY updated_at DESC LIMIT 1) RETURNING session_id`),并发安全任务领取
- 落点:adapters-sqlite embedding 队列;子任务 #3 后台语义索引 worker
- 风险:依赖 RETURNING,SQLite 3.35+ 支持

**I8. sessiongrep FTS 召回 + 应用层重排** — `sessiongrep/src/db.rs:281 (search), 395 (fts_candidate_ids)`
- License: Apache-2.0(需 NOTICE);复制方式:**port with adaptation**
- 内容:FTS 召回 limit*5 候选 → 应用层对 6 个 haystack(title/summary/cwd/repo/preview/transcript)加权子串打分 + 新鲜度 + 当前 repo 加分 + SkimMatcherV2 模糊分
- 落点:application 重排层;子任务 #3 的 lexical 侧质量
- 风险:SkimMatcherV2 引入 fuzzy-matcher 依赖;若不想加依赖,可只搬子串打分逻辑(权重表本身是独立数据)

---

## 2. JSONL 解析健壮性 → 子任务 #2 (sixteen-provider-evidence-wave),兼 #6

**J1. fast-resume `jsonl_health` 三级健康度** — `fast-resume/src/adapters/shared.rs:21 (enum JsonlHealth), 194 (fn)`
- License: MIT;复制方式:**verbatim**(约 30 行)
- 内容:Clean(全合法)/ Partial(坏行后仍有合法行)/ Invalid(尾部截断);Invalid→Retain 旧索引不删不更新,Partial→尝试解析,Clean→失败才 Delete
- 落点:provider-* 各 crate 共享的 JSONL 读取工具;直接解决"agent 正在写文件时误删"问题
- 风险:零依赖(纯 BufRead 逻辑)

**J2. fast-resume `failed_incremental_scan`(失败不删除原则)** — `src/adapters/shared.rs`
- License: MIT;复制方式:**verbatim**(约 10 行)
- 内容:返回空 IncrementalScan(new_or_modified 和 deleted_ids 都为空);任何 IO/解析错误不删已索引会话
- 落点:application sync;与 PRD "失败不删除原则"(Q41/Q46)完全一致,是现成实现
- 风险:无

**J3. fast-resume `session_needs_update`(mtime 容差)** — `src/adapters/shared.rs`
- License: MIT;复制方式:**verbatim**
- 内容:MTIME_TOLERANCE=0.001s 阈值 + known 缺失判定
- 落点:application sync 增量判断

**J4. claude-historian 两阶段预过滤** — `claude-historian-mcp/src/parser.ts:31 (SMALL_FILE_THRESHOLD=400_000), parseJsonlFile (~55-130)`
- License: MIT;复制方式:**port(TS → Rust)**
- 内容:文件级整文件 toLowerCase includes(不中直接跳过,省行分割)+ 行级 toLowerCase + 任一查询词 includes 才 JSON.parse(省 80-95% 解析);<400KB 走 readFile 快速路径
- 落点:provider-claude 的 transcript 读取;子任务 #2 的导入性能
- 风险:TS 语义直接映射 Rust 无难点;注意 UTF-8 大小写折叠与 Rust `to_lowercase` 行为差异

**J5. hstry `bulk_insert_messages_in_tx`(60 行/chunk + 999 参数断言)** — `hstry/crates/hstry-core/src/db.rs:2990`
- License: MIT;复制方式:**verbatim**(Rust)
- 内容:multi-row INSERT 60 行/chunk、15 列,编译期断言 `COLS * ROWS_PER_CHUNK <= 950` 不超 SQLite 999 参数上限
- 落点:adapters-sqlite 批量写入;子任务 #2 导入吞吐
- 风险:断言本身是 const fn 即可;列数变了要同步改常量

**J6. Recall 三层增量扫描 `run_file_scan_with_options`** — `Recall/src/adapters/file_scan.rs:46`
- License: MIT;复制方式:**port with adaptation**
- 内容:mtime_ms 比对 + usage parser_version + event parser_version 三层判断;已存在 session 更新 source_file_path(处理文件移动)、清除 import 标记
- 落点:application sync 骨架 + provider-* 的 scan_for_sync;子任务 #2 增量同步
- 风险:依赖 Recall 的 usage/event state 表结构,需按本项目 schema 改造;parser_version 触发 backfill 而非全量重建是核心价值

**J7. Recall `sync_state` 新鲜度判定** — `Recall/src/adapters/sync_state.rs`
- License: MIT;复制方式:**verbatim**(约 30 行)
- 内容:`usage_state_is_current` / `event_state_is_current`——parser_version >= required 且 source_updated_at == mtime
- 落点:application;与 J6 配套

**J8. Recall `RawUsageEvent` observed/derived 构造器** — `Recall/src/adapters/usage.rs:7 (observed), 16 (derived)`
- License: MIT;复制方式:**verbatim**(约 30 行)
- 内容:TokenSource(Observed/Derived/Estimated)枚举 + 构造器填默认值(model="unknown" 等),adapter 只填实际有值的字段
- 落点:provider-* 各 crate 的 token 事件构造;子任务 #2 的 usage 提取
- 风险:无

**J9. cc-switch `read_head_tail_lines`(头部/尾部局部解析)** — `cc-switch/src-tauri/src/session_manager/providers/utils.rs:13`
- License: MIT;复制方式:**verbatim**(Rust,约 35 行)
- 内容:<16KB 全读切分;>=16KB 头 BufReader 读 N 行 + seek 到末尾 16KB 取尾 N 行(跳过首行残片);Claude/Codex/OpenClaw 用 (10,30),Hermes 用 (30,10)
- 落点:provider-* 的元数据快速提取(sessionId/cwd/title 在头,last_active/summary 在尾),避免大文件全读
- 风险:依赖 std::io::Seek,零外部依赖

**J10. cc-switch `parse_timestamp_to_ms`(三格式时间戳)** — `utils.rs:51`(约 15 行)
- License: MIT;复制方式:**verbatim**
- 内容:毫秒整数 / 秒整数(<=1e12 乘 1000)/ RFC3339 字符串 统一转毫秒
- 落点:domain 或 ports 的时间解析;16 provider 通用

**J11. sessiongrep `minimal_record`(parse_warning 不丢弃)** — `sessiongrep/src/util.rs:329`
- License: Apache-2.0;复制方式:**verbatim**(约 15 行)
- 内容:parse 失败降级为 preview_text="(parse failed)"、message_count=0、parse_warning=错误信息 的记录,索引不崩溃、doctor 可统计
- 落点:provider-* parse 错误路径;与 PRD "非法事件隔离 + 诊断" 一致

**J12. sessiongrep `extract_text`(递归文本提取)** — `sessiongrep/src/util.rs:161`
- License: Apache-2.0;复制方式:**verbatim**(约 50 行,含测试)
- 内容:递归 serde_json::Value,Object 优先取 text 再递归 content/message/input/output 子键;统一 Claude/Codex/Pi 各异结构
- 落点:provider-* 共享的消息文本提取(cc-switch 的 extract_text 是同思路的 Rust 版本,`utils.rs:67`,可对照)

**J13. agentsview `isTruncated` 截断检测** — `agentsview/internal/parser/claude.go`(行号未逐行核验,见 deep-read-agentsview 报告 §4.2)
- License: MIT;复制方式:**port(Go → Rust)**
- 内容:`isTruncated = lastLine 非空 && 非空白 && !gjson.Valid && !fileEndsWithNewline`——以换行结尾的非法行只是畸形记录,不是截断写
- 落点:provider-claude 解析;与 J1 的 Invalid 判定互补(文件级 vs 行级)

**J14. agentsview `codexSafeResumeOffset`(O(1) 增量边界校验)** — `agentsview/internal/parser/codex.go` 尾部 reader 段(行号未逐行核验,见 deep-read 报告 §4.3)
- License: MIT;复制方式:**port**
- 内容:offset==0 恒安全,否则检查前一字节是否 `\n`;Codex 可能先写出合法 JSON 再追加换行,这种 EOF record 必须 full-parse fallback,绝不当作安全 cursor
- 落点:provider-codex 增量解析;防半行数据被当安全游标

---

## 3. Resume / 命令面 → 子任务 #5 (resume-metadata-execution)

**R1. fast-resume 12 provider resume 命令表 + yolo 探测** — `fast-resume/src/adapters/*.rs`(表见 deep-read 报告 §4.8;yolo 判定在 `adapters/codex.rs:43-140`:turn_context 的 `approval_policy=never` 或 `sandbox_policy.mode=danger-full-access` → yolo=true)
- License: MIT;复制方式:**verbatim 数据驱动**(每条 resume_command 是 3-8 行的纯函数)
- 内容:claude `--dangerously-skip-permissions --resume <id>`、codex `--dangerously-bypass-approvals-and-sandbox resume <id>`、opencode `<dir> --session <id>`(目录是位置参数)、vibe `--agent auto-approve`(非 --yolo)等
- 落点:provider-* 的 resume_command + application 的 yolo 元数据字段;PRD 要求"原 approval/permission mode 恢复"正是这个能力
- 风险:各 provider CLI 参数会演进,需要版本校验;`supports_yolo()` 能力位设计一并照搬

**R2. fast-resume `exec_resume` 进程替换** — `fast-resume/src/main.rs:197 (exec_resume), 229 (exec_resume_with)`
- License: MIT;复制方式:**port with adaptation**
- 内容:ExecBackend trait(测试替身)+ 先 set_current_dir(directory) 再 exec;Unix 用 CommandExt::exec() 进程替换,非 Unix 用 status() 后退出
- 落点:cli resume 执行;PRD 的 dry-run 预览 + 确认后执行在此处加一层
- 风险:Windows 路径无进程替换,用 status() 分支(原项目已处理)

**R3. Recall `ResumeCommand` + trait 方法** — `Recall/src/adapters/mod.rs`(trait 定义)
- License: MIT;复制方式:**pattern**
- 内容:`resume_command(source_id) -> Option<ResumeCommand>` / `app_command()`(app deeplink 如 codex://threads/)双通道 + imported session 不可 resume 的判定(is_import)
- 落点:ports 层 trait 设计;app_command 是 Web UI/resume 的补充通道
- 风险:仅结构借鉴,无代码可搬

**R4. sessiongrep MCP `get_resume_command` + shlex 引号** — `sessiongrep/src/mcp.rs`
- License: Apache-2.0;复制方式:**verbatim**(约 20 行)
- 内容:resolve_session + resume_plan + `shlex::try_quote(cwd)` 输出 `cd <cwd> && <cmd>`,cwd 含 `$(...)` 也不注入
- 落点:cli/mcp 的 resume 工具;命令注入防护是现成实现

**R5. cc-switch resume_command 模板** — `cc-switch/src-tauri/src/session_manager/providers/*.rs`(每 provider 的 `resume_command` 字段)
- License: MIT;复制方式:**pattern**
- 内容:SessionMeta.resume_command 字段 + 无 CLI 的 provider 返回 None(OpenClaw/Hermes);与 R1 对照补充覆盖面
- 风险:与 R1 信息重叠,取其一做权威表

---

## 4. 结构化工具活动 → 子任务 #6 (structured-activity-context-facets)

**T1. Recall `target_from_value` 优先级链 + `infer_tool_kind`** — `Recall/src/adapters/events.rs:94, 123 (command_target_from_array), 146 (infer_tool_kind)`
- License: MIT;复制方式:**verbatim**(约 80 行,含数组 `["bash","-lc",<cmd>]` 特判)
- 内容:target 优先级 `path > file_path > filePath > target > command > cmd > query > pattern > glob > glob_pattern > regex > url`;kind 推断 bash/shell/exec→command、grep/search/glob/find→search、edit/write/patch/delete+target→file_write、read/open/view+target→file_read、其余→tool_call
- 落点:provider-* 共享的事件抽取 + domain 的 kind 枚举;子任务 #6 的核心,与既有 08-13/08-14 "structured tool activity PRD" 直接合流
- 风险:零依赖;注意 `command_target_from_array` 只处理 bash/sh/zsh 三元组,其他数组用空格 join(行为注释要保留)

**T2. Recall 事件构造器三件套** — `events.rs:14 (tool_call_event), 43 (tool_call_event_from_text), 72 (tool_result_event)`
- License: MIT;复制方式:**verbatim**
- 内容:从 Value / 文本参数构造事件的统一入口;文本先试 JSON.parse 失败用 command_target 兜底
- 落点:provider-* 各 adapter 的事件产出

**T3. claude-historian `extractContext` 7-facet 结构** — `claude-historian-mcp/src/parser.ts:180`(约 170 行,31 条正则)
- License: MIT;复制方式:**port(TS → Rust,facet schema 设计照搬)**
- 内容:filesReferenced / toolsUsed / errorPatterns / bashCommands / editDiffs / skillInvocations / codeSnippets / actionItems / progressInfo 九类 facet(报告称 7 类,实际含 codeSnippets/actionItems/progressInfo 共 9);`_contentLower` 懒缓存;仅当 `query && relevanceScore >= 2` 才做提取(正则成本门槛)
- 落点:domain 的 message facet 模型 + provider-claude 提取器;子任务 #6 的 schema 依据
- 风险:正则集是 TS 风格,需翻译为 Rust regex;31 条正则的调参注释一并搬运;errorPatterns 截断 100 字符、editDiffs "old → new" 截断 60 字符等阈值是现成经验值

**T4. claude-historian `extractContentFromMessage` 通用 string 值遍历** — `claude-historian-mcp/src/utils.ts:310`
- License: MIT;复制方式:**port**
- 内容:tool_use 遍历 input 所有 string 值(各截断 500)而非硬编码字段名——注释记录旧版硬编码 7 字段漏 new_string/old_string 的教训
- 落点:provider-claude 内容抽取;"用通用遍历替代枚举"的经验直接可用
- 风险:TS 弱类型便利在 Rust 需 serde_json::Value 遍历,等价

**T5. memex `prune_and_merge_tool_calls`** — `memex/memex-rs/src/compact/service.rs:204`
- License: MIT;复制方式:**verbatim**(Rust,约 30 行)
- 内容:prune_empty(删无内容)+ merge_consecutive(阈值 3,连续同类合并,merged_id 格式 `merged_{first}_{count}`,name 加 `(x{count})`)——避免 20 次重复 Read 淹没摘要
- 落点:application context pack / L1 级摘要;子任务 #4 + #6
- 风险:无依赖;合并阈值 3 是经验值,做成常量

**T6. memex `tool_category` 多 provider 工具名归一** — `memex/memex-rs/src/compact/source.rs:259 (build_parsed_session 内含映射)`
- License: MIT;复制方式:**port(映射表 verbatim)**
- 内容:Read|read_file→Read、Edit|edit_file|str_replace_editor→Edit、Bash|execute_bash|run_terminal_cmd→Bash、Glob|list_files|find_files→Glob 等 + is_read/is_write 判定
- 落点:domain 工具名归一表;16 provider 共用
- 风险:映射表需按本项目 16 provider 名单扩展(如 DeepSeek Harness 的工具名)

**T7. agentsview `resolveClaudePersistedToolResults`(tool-result 外部化)** — `agentsview/internal/parser/claude.go`(行号未逐行核验,见 deep-read 报告 §4.2)
- License: MIT;复制方式:**port**
- 内容:行含 persisted-output/persistedOutputPath 时,从 `Full output saved to:` 正则或 toolUseResult 取路径,读回外部化文件替换 placeholder content
- 落点:provider-claude;解决大 tool_result 落盘后主 JSONL 只剩占位的问题

---

## 5. Web UI / HTTP server → 子任务 #7 (loopback-web-ui-parity)

**W1. agentsview 鉴权中间件(localhost + token + Host)** — `agentsview/internal/server/auth.go:22 (isRemoteAuth), 39 (isLocalhostRequest), 57 (hasForwardingHeader), 67 (protectedPath), 76 (authMiddleware), 180 (setCORSOnAuthError)`
- License: MIT;复制方式:**port(Go → Rust axum)**
- 内容:RemoteAddr 判定 localhost + 转发头防护(Host 校验防 DNS rebinding)+ Bearer token + 无 token 时仅 loopback 放行 + CORS 错误响应带 Origin 回显
- 落点:cli 的 `asg serve`(PRD Q27/Q36/Q40 要求"默认仅 loopback + 随机本地 token + Host/Origin 校验",该文件是现成实现蓝本)
- 风险:Go 中间件语义映射 axum middleware 直接;`setCORSOnAuthError` 的 Origin 回显是正确做法(不能固定 *)

**W2. hstry-api loopback token + body limit** — `hstry/crates/hstry-api/src/main.rs:51, 56, 79`
- License: MIT;复制方式:**pattern**
- 内容:`--token` / `HSTRY_API_TOKEN` env 回退 + 无 token 时 `/ingest` 接受任意 loopback 客户端 + `DefaultBodyLimit::max(64 MiB)`(ingest payload 大)
- 落点:cli serve 的写侧通道;与 W1 二选一(agentsview 的更完整)

**W3. sessiongrep 手写 JSON-RPC 2.0 MCP server** — `sessiongrep/src/mcp.rs`(466 行整文件)
- License: Apache-2.0;复制方式:**port**
- 内容:无 MCP SDK 依赖的 initialize/tools/list/tools/call/notifications/ping + 1500ms reindex 节流 + 错误统一 `{isError:true,...}` 封套
- 落点:cli/mcp;本项目 MCP 若想零依赖可整文件移植;若已有 rmcp 依赖则只借鉴节流与错误封套设计
- 风险:协议 2024-11-05 的演进需核对当前 MCP 规范版本

**W4. agentsview SPA embed + basePath** — `agentsview/internal/server/server.go`(huma 路由 + http.FileServerFS;行号未逐行核验,见 deep-read 报告 §7.2)
- License: MIT;复制方式:**pattern**
- 内容:内嵌前端资源 + basePath 注入 `<base href>` 支持 `/agentsview` 反代前缀
- 落点:cli serve;Web UI 以协议客户端形态挂 `asg serve`(PRD Q40)

**W5. memex `embedded.rs` 缓存策略** — `memex/memex-rs/src/embedded.rs`
- License: MIT;复制方式:**pattern**
- 内容:静态资源 immutable 1 年缓存、index.html no-cache、SPA fallback(未匹配路由回退 index.html)
- 落点:cli serve;小但完整,直接抄响应头策略

---

## 6. 隐私 / 脱敏 → 子任务 #8 (offline-privacy-hooks,ADR-0009)

**P1. cass `redact_secrets`(入库前脱敏)** — `cass/src/indexer/redact_secrets.rs:113 (redact_text), 136 (redact_json), 187 (redaction_algorithm_fingerprint)`
- License: MIT + OpenAI/Anthropic Rider —— **clean-room idea only,禁止复制代码/测试 fixture**
- 内容(仅思路):前缀模式(AWS AKIA、GitHub PAT、OpenAI sk- 等)在 `map_to_internal()` 入库前脱敏;`redaction_algorithm_fingerprint` 给脱敏算法做版本指纹(审计用)
- 落点:application/adapters-sqlite 的入库钩子;子任务 #8 可参考其"入库时脱敏而非事后扫描"与"算法指纹"两个设计点,自行实现

**P2. agentsview Go 层 snippet 脱敏** — `agentsview/internal/db/search_content.go`(buildSnippet + secrets.RedactWindow;行号未逐行核验,见 deep-read 报告 §6.2)
- License: MIT;复制方式:**port**
- 内容:snippet 在 Go 层而非 SQL 层构建,让脱敏器看到完整 secret 而非 SQL 预截断窗口(防泄露被劈开的 secret 片段);RevealSecrets 默认 false,只有 localhost-gated reveal 路径显式 opt-out
- 落点:application 的跨边界输出(Web UI/Handoff Pack/MCP/Robot/HTTP 默认脱敏,显式 reveal 才显示原文——与 ADR-0009 完全同构);"遗忘 flag 也 fail safe"是关键注释
- 风险:SQL 层截断 + Go 层脱敏的顺序问题要在 Rust 侧用 serde_json 层而非 SQL 层做

**P3. agentsview `secret_findings` 自然坐标存储** — `agentsview/internal/db/secret_findings.go` + `schema.sql`(行号未逐行核验)
- License: MIT;复制方式:**port**
- 内容:按 (session_id, message_ordinal, call_index, event_index, match_start, match_end) 定位而非 row ID,只存脱敏值——穿越 full-resync 的 orphan copy 后仍可定位
- 落点:adapters-sqlite 审计表设计;子任务 #8 的审计事件(不含 secret 原文)

**P4. memex `redact_before_llm` 默认 true** — `memex/memex-rs/src/knowledge/config.rs`(行号未逐行核验)
- License: MIT;复制方式:**pattern**
- 内容:送 LLM 前脱敏的配置默认值
- 落点:application 本地 LLM 摘要(PRD:inference 摘要仅显式启用且标记);一行配置的借鉴

---

## 7. 会话身份 / 去重 → 子任务 #1 (unified-release-contract),兼 #2

**S1. hstry `stable_message_id`(UUIDv5 内容寻址)** — `hstry/crates/hstry-core/src/lib.rs:45`(约 30 行)
- License: MIT;复制方式:**verbatim**(Rust,含测试)
- 内容:namespace(HSTRY_MSG_NAMESPACE)+ 优先 client_id,否则 hash `source_id:external_id:idx:role:content_prefix`;content 截断 4096 字节且在 UTF-8 字符边界截断;重放同一 JSONL 产生相同 row id,`ON CONFLICT(id)` 天然去重
- 落点:domain 的稳定消息 ID;子任务 #1 的 identity/contract 决策的现成实现
- 风险:`utf8_prefix` 的边界截断是防 panic 关键,照搬

**S2. sessiongrep 复合 ID `{provider}:{native_id}` + `resolve_session`** — `sessiongrep/src/models.rs`(id 字段)+ `src/db.rs:480 (resolve_session)`
- License: Apache-2.0;复制方式:**verbatim**(约 20 行)
- 内容:复合 ID 作主键跨 provider 永不冲突;resolve 同时支持精确/原生/前缀三种匹配,0 报错、1 返回、多报 ambiguous
- 落点:domain session 身份;与 PRD 的 provider-scoped identity(08-13 已定)一致,多匹配歧义报错是补充点
- 风险:无

**S3. Recall `RepoIdentityCache`(git remote 解析 + 两级缓存)** — `Recall/src/repo_identity.rs:18 (resolve), 47 (normalize_remote_url)`
- License: MIT;复制方式:**verbatim**(约 100 行)
- 内容:`git -C <dir> rev-parse --show-toplevel` + `remote get-url origin`;by_directory / by_toplevel 两级缓存;normalize_remote_url 支持 https/http/git@/ssh:// 多前缀,去 .git 与尾斜杠;只支持 GitHub(不解析其他平台)
- 落点:application 的 repo 维度检索 + session 元数据;子任务 #1/#2
- 风险:依赖 git 二进制(运行时),CI/离线环境需确认;只支持 GitHub 是已知局限,本项目可扩展

**S4. ctx `ProviderCaptureEnvelope` + idempotency_key** — `ctx/crates/ctx-history-capture/src/provider/mod.rs`(三层封套:source/session/event 各带 idempotency_key 与 cursor)
- License: Apache-2.0;复制方式:**pattern**
- 内容:导入幂等键模式 `provider-source:{provider}:{source_format}:{provider_session_id}`;ProviderCursorRange(before/after)断点续传;ProviderFidelityClaims 布尔位图声明 provider 支持哪些保真维度
- 落点:子任务 #1 的 release contract 设计参考(本项目 Provider Adapter Protocol 的 manifest 可借鉴 fidelity claims 位图)
- 风险:仅结构借鉴;文件行号未逐行核验(基于 deep-read-ctx 报告 + Glob 确认文件存在)

**S5. ctx `SyncMetadata`(fidelity/visibility/sync_state)** — `ctx/crates/ctx-history-core/src/sync.rs`
- License: Apache-2.0;复制方式:**pattern**
- 内容:visibility(LocalOnly/Reportable/SyncFull/Withheld)、fidelity(Full/Partial/Imported/Inferred/SummaryOnly)、sync_state(Pending/Synced/Failed)、RedactionState(raw/redacted/safe_preview/withheld)
- 落点:子任务 #1 的统一 release contract;与 ADR-0009 脱敏边界配套(跨边界输出 = RedactionState.safe_preview 默认)
- 风险:仅结构借鉴

**S6. hstry `validate_new_source` 五条 source 注册不变量** — `hstry/crates/hstry-core/src/source_registry.rs`
- License: MIT;复制方式:**port**(Rust,约 80 行)
- 内容:非目录 / 不在 canonical root / 跨 harness 领地 / 重复路径 / 子路径或父路径 五类校验统一 chokepoint;TS 侧 `isUnderCanonicalRoot`(adapters/types/index.ts)做 defense-in-depth
- 落点:application 的 source 注册校验;PRD Q41 的 canonical root 发现策略直接对应
- 风险:`isUnderCanonicalRoot` 刻意要求分隔符边界(避免 projects-other 误判为 projects 后代),这个坑要保留

**S7. agentsview `sync_marker` 触发器(防畸形时间戳污染水位)** — `agentsview/internal/db/db.go`(syncMarkerSchemaSQL;行号未逐行核验)
- License: MIT;复制方式:**port(SQL verbatim)**
- 内容:AFTER INSERT/UPDATE 触发器把 sync_marker 设为 MAX(COALESCE(strftime(created_at),''), file_mtime);刻意不让 created_at 回退到原始字符串——畸形 created_at("not-a-timestamp")字母序高于真时间戳会永久打败 MAX
- 落点:adapters-sqlite 的镜像/同步水位;子任务 #1 的增量窗口
- 风险:小 SQL,直接可用

**S8. Recall `(source, source_id)` UNIQUE + `is_import` 标记** — `Recall/src/db/session_store.rs` + `src/import.rs`
- License: MIT;复制方式:**pattern**
- 内容:稳定标识为 (source, source_id) 而非内部 UUID(replace 时重新生成);导入会话 is_import=true 不可 resume,本地扫描发现后清除标记
- 落点:adapters-sqlite session 表约束;子任务 #1
- 风险:仅结构借鉴

---

## 8. Handoff / context pack → 子任务 #4 (evidence-handoff-pack)

**H1. hstry `PeekBundle`(1-2KB 会话预览)** — `hstry/crates/hstry-core/src/peek.rs:46 (struct), 66 (build_peek)`
- License: MIT;复制方式:**verbatim**(Rust,约 100 行 + 测试)
- 内容:first_user_chars:240 / last_user_chars:240 / last_assistant_chars:400 / bash_sample_count:6 / bash_sample_chars:80 / files_touched_max:30 + tools: BTreeMap 统计;`has_text_content` 只把含 text part 的 user 消息算真实轮次(排除 tool_result 合成 user 消息);scan_paths_into 支持绝对/`~/`/`./` 拒绝 URL 与裸 dotfile
- 落点:application 的 context pack 预算与 session 摘要;子任务 #4 的 evidence 展示基础
- 风险:零依赖(纯 std);**更正:任务书写的 "memex Peek Bundle" 实际是 hstry 的**——memex 没有 PeekBundle,其 memex-lite `collect_context`(src/search.rs)是行级正则窗口拼接,价值较低

**H2. Recall handoff 流程** — `Recall/src/handoff.rs` + `src/session.rs:663 (handoff_working_directory)`
- License: MIT;复制方式:**port with adaptation**
- 内容:会话渲染为 plain transcript → 注入目标 agent 启动新会话;handoff_working_directory 只保留仍存在的目录
- 落点:application 的 handoff 执行;子任务 #4 的 `handoff-pack/v1` 的 Markdown projection 可参考其 transcript 渲染
- 风险:Recall 的 handoff 是"静默注入新会话",与 PRD "永远先展示 context pack 与 evidence,不静默注入"冲突——只借渲染部分,不借注入行为

**H3. sessiongrep 书挡式 transcript 摘要** — `sessiongrep/src/tui.rs`(build_transcript_summary:first_user 8 行 / first_assistant 4 行 / last_user 8 行 / last_assistant 14 行 + `⋯ N more turns hidden ⋯`)
- License: Apache-2.0;复制方式:**port**
- 内容:按 `[timestamp] role` 行分段识别 turn,首尾 4 个关键 turn + 省略计数
- 落点:cli/tui 的 preview;子任务 #4 的上下文预算展示
- 风险:行号未逐行核验(基于 deep-read 报告 §6.3)

**H4. claude-historian `estimateTokens`(ceil(len/4))** — `claude-historian-mcp/src/formatter.ts`
- License: MIT;复制方式:**verbatim**(1 行)
- 内容:token 估算写入输出 header 供调用方预算上下文
- 落点:application context pack 的 token 预算;与 H1 配合

---

## 9. Don't Borrow 警告(anti-pattern,禁止照搬)

1. **cass 巨型单文件组织** — `cass/src/lib.rs`(约 10 万行)+ `src/storage/sqlite.rs`(约 3 万行)+ `src/indexer/mod.rs`(约 5.2 万行)。单文件承载全部 CLI 命令与存储逻辑,IDE 索引慢、diff 噪声大、模块边界消失。本项目必须按子命令/模块拆分。
2. **claude-historian 双评分体系并存** — `utils.ts` 的 `calculateRelevanceScore`(加性:精确 10/词 2/短语 5)与 `search.ts` 内部私有评分(短语 15/词 3×ratio/工具 8)+ selectTopRelevantResults 多层乘性 boost(0.5/1.3/1.5/4/0.1 经验值),难以追踪排序原因且无测试固化。若借鉴其评分思路,必须统一为单一评分函数 + 权重常量集中 + 测试锚定排序。
3. **claude-historian 路径编码有损** — `decodeProjectPath` 把所有 `-` 当 `/`,`codex-mcp-historian` 被还原成 `codex/mcp/historian`;Windows 盘符+反斜杠完全不处理;代码多处用"编码名直接匹配"绕开。本项目 provider identity 必须无损编码,不得沿用此约定。
4. **memex 硬编码 5 分钟锁超时** — `memex-rs/src/compact/db.rs:1081 (LOCK_TIMEOUT_MS = 5*60*1000)`,长 LLM 调用可能误触 stale 清理。若借鉴其原子锁设计,超时必须可配置;另注意 memex `order_by` 时间排序会降级 FTS-only(search/mod.rs),这个静默降级与 PRD "禁止静默切换检索模式"冲突。
5. **cass contentless FTS5** — V14 切 contentless 后需 `contentless_delete=1` + shadow table 完整性探针 + `.recover` salvage 流水线,复杂度远高于收益。本项目用 external-content FTS + 触发器(hstry/memex/agentsview 同款)即可。
6. **cc-switch "command 不加校验直接执行"** — `launch_session_terminal` 把 command 直接交给 shell 执行,注释明确"已知并接受的风险"(renderer 视为可信边界)。与本项目 Q27 安全门(危险动作默认预览、首次安装强制预览)冲突,不可照搬该信任模型;其 `cwd` 的 shell_escape 单引号转义(terminal/mod.rs)可借用。
7. **fast-resume / sessiongrep 删库重建式 schema 迁移** — fast-resume `INDEX_SCHEMA_VERSION` 不匹配即 `remove_dir_all` 整个索引目录;sessiongrep 官方建议"schema 变了删 index.db 重建"。本项目 GA 门要求迁移证据(升级 rehearsal),索引作为可丢弃缓存可以,但"以删代迁"不能写进发布契约。

---

## Caveats / Not Found

- **agent-sessions / agf / AgentRecall**:agent-sessions 是 Swift/macOS 原生 App,无 Rust 可搬代码,仅 resume 命令表与 macOS 终端启动器有模式参考;agf 是纯读聚合器 + nucleo 模糊匹配,其 `cache.rs` 过期缓存先展示 + 后台刷新模式可借鉴(未列入正文条目);AgentRecall 的 session-loader.ts(1990 行)与 store/sessions.ts(2147 行)解析器粒度与本项目重叠,但 Electron/TS 栈移植成本高于其他 Rust 源,仅列 license 核验结论。三者均未产生正文条目,避免稀释高价值项。
- **行号未逐行核验的条目**(J13/J14/T7/W4/P2/P3/P4/S7/H3):路径经 Glob 确认存在,行号依据 deep-read 报告,搬运前需再读原文。
- **cass 全部条目**:因 OpenAI/Anthropic Rider("no rights are granted to any Restricted Party"),本项目作为面向所有用户开源的产品,一律 clean-room(仅借鉴思路,自行实现),已按此标注。
- 所有 MIT 条目按义务保留原版权声明(注释或 NOTICE);Apache-2.0 条目(sessiongrep/ctx)需在发布 NOTICE 文件列出。
