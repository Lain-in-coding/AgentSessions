# Review: provider-codex adapter (agent-session-grep-provider-codex)

- **Query**: 审查 Codex CLI provider adapter 的解析性能与健壮性（7 个 focus 项），产出带严重级/修复方向的 findings
- **Scope**: internal（本仓库代码 + 本机真实 rollout 样本 + 13 个 competitor 固定 clone 交叉核验）
- **Date**: 2026-08-15
- **审查对象**: `C:/AgentSessions/crates/agent-session-grep-provider-codex/src/lib.rs`（870 行，含 tests）、`tests/properties.rs`（482 行）、`tests/golden.rs`、`tests/golden/basic.jsonl`
- **基准规范**: `.trellis/spec/agentsessions-provider-codex/backend/index.md`、`ports/src/lib.rs`（ParseReport §332-343）
- **方法**: 静态阅读 + 对 143 个真实 rollout 文件（`C:/Users/小Q/.codex/sessions/`，71,176 行，仅统计字段名/类型频率，未读取消息正文）做行类型分布量化 + peer 实现逐行核验

---

## 0. 任务前提与代码现实的差异（重要）

任务书 7 个 focus 项中，有 3 项描述的功能在本 crate（及全仓库任何 provider crate）**不存在**，经 Grep 全仓库核实：

| 任务书假设 | 实际状态 | 证据 |
|---|---|---|
| function_call pairing（call_id / tool_call_id→id fallback、anchor emission、R1-R6 fail-closed 规则） | **不存在**。`function_call`/`custom_tool_call` 等被静默跳过；全仓库 grep `function_call|tool_call_id|ToolActivity|deserialize_optional_string` 仅命中 properties.rs 的噪声生成；R1-R6 规则在全部 task PRD 中未找到 | lib.rs:372-380（非 message 一律 continue）；properties.rs:270-296（把 function_call 当噪声断言"静默跳过、不计数"） |
| `deserialize_optional_string`（summary:[] vs summary:"…" 容错） | **不存在**。RawPayload（lib.rs:77-95）无 summary 字段；reasoning 记录整条跳过，无字段抽取，故无该形状风险 | lib.rs:378-380 |
| session_meta title/summary 抽取（first-wins + `session_native_id == sid` guard） | **不存在**。session_meta 只抽 `session_id`（lib.rs:355-368）；真实数据确认 session_meta payload **无 title 字段**（258/258 条无 title key）。存在的是 `is_none()` first-wins（lib.rs:360-362）+ 全部 sid 收集 + 多会话诊断（lib.rs:426-442），无 `== sid` guard | lib.rs:355-368；真实字段统计见 §3 |

**结论**：这 3 项属于"未来工作"（08-15-structured-activity-context-facets 是 P1 子任务，未动工），不是本 adapter 的缺陷。本报告为这 3 项补上真实数据证据（§3/§4），供后续实现直接使用；其余 4 项（1/5/6/7）是本 adapter 现状的实质审查。

---

## 1. Envelope 解析预过滤 — **P1（性能，certified 门相关）**

**现状**：parse 对每一行无条件 `serde_json::from_str::<RawLine>`（lib.rs:341），RawLine 会分配 `type`/`id`/`role`/`timestamp` 四个 String + `content: Vec<RawBlock>`（lib.rs:60-95）。probe 有 16 行采样上限（lib.rs:31, 178-189），但 parse 无任何预过滤。

**真实数据量化（143 文件 / 71,176 行）**：

| 行类别 | 条数 | 占比 |
|---|---|---|
| 全部行 | 71,176 | 100% |
| `response_item`（含全部子类型） | 40,925 | 57.5% |
| `event_msg` | 28,535 | 40.1% |
| `session_meta` / `turn_context` / `world_state` / `compacted` / `inter_agent_communication_metadata` | 1,716 | 2.4% |
| **`response_item/message`（唯一能产出消息的行）** | **5,612** | **7.9%** |

即 **92.1% 的行永远不可能产出消息**，却全部经历了完整反序列化；`response_item` 内部 86.3% 是非 message 载荷（reasoning 9,535 / function_call 11,262 / function_call_output 11,256 / custom_tool_call±output 2,490 / web_search± 882 / tool_search± 276 / agent_message 53）。文件实测最大 13.3MB、中位 1.1MB。

**修复方向**（claude-historian 同款两段预过滤，inventory §J4）：
1. 行级原始子串检查（零反序列化）：`line.contains("\"type\":\"response_item\"")` 先筛掉全部 event_msg（40.1%）——真实文件由 codex-cli-rs 紧凑序列化写出，字段字节序稳定，无假阴性；假阳性（文本内容里出现该子串）只是多花一次 parse，无害
2. 对通过的行再做 `line.contains("\"payload\":{\"type\":\"message\"")`，只对 ~7.9% 的行完整反序列化
3. `session_meta` 行必须放行（占 0.4%，是 session_native_id 来源）；不放心格式化漂移（如手写带空格 JSON）可保留"子串命中失败但反序列化成功"的 fall-through（成本只是回到现状）
4. 预过滤必须保持在 span/offset 记账（lib.rs:325-337）之后、`continue` 之前，行号/字节坐标不受影响

claude-historian 实现参照：`claude-historian-mcp/src/parser.ts:31`（`SMALL_FILE_THRESHOLD = 400_000`）、`:84-94`（行级 toLowerCase + includes 才 JSON.parse，注释"eliminates 80-95% of JSON.parse calls"、"~10x faster"、"Zero false negatives guaranteed because JSONL = one JSON object per line with no literal newlines"）。本 adapter 语义相同（JSONL 每行一个对象、字符串内换行被转义），子串检查可安全复刻。

**为什么是 P1**：PRD 发布门（Q15/Q28/Q37）要求 certified provider 的公开 benchmark 有 parse loss / p50-p95 性能证据（prd.md:171-172），Codex 是 certified 首发目标；92% 无效反序列化是单点可测的热路径浪费，修复成本约 10 行。

---

## 2. 内存/流式 — 现状正确，无 P1/P2 问题

- parse 契约是 `&[u8]` 整文件入参（lib.rs:309），`from_utf8` 零拷贝借用，`split_inclusive('\n')` 逐行流式迭代（lib.rs:325），行内临时 RawLine 不跨行保留——**整文件缓冲由调用方决定，adapter 内部无二次拷贝**
- span 语义是"快照字节坐标系"（lib.rs:322-337, 417），streaming-by-contract 与 span 天然冲突——这是契约级约束，8-13MB 文件（实测最大 13.3MB）单缓冲完全可接受；spec index.md:74 "parse streaming, never load whole" 在 adapter 层已达成（逐行处理），整文件加载属调用方行为，如需真正"不加载整文件"需改 ports 契约（不在本 adapter 范围内）
- 256KiB 大字段行已有 property 覆盖（properties.rs:160-168, 197-199）；真实 `function_call_output.output` 是大 blob（实测含 549 行输出），大行路径被覆盖
- 增量解析边界安全（agentsview `codexSafeResumeOffset`，inventory §J14）属 application 层 sync 职责，非 adapter 契约，暂不适用

---

## 3. session_meta 解析与跨会话归属 — 正确，附真实数据

**现状**：first-wins 设置 `session_native_id`（仅 `is_none()` 时，lib.rs:360-362）+ 收集全部非空 sid（trim 后去重，lib.rs:363-365）+ 末尾多会话诊断（lib.rs:426-442，id 列表上限 3，SESSION_ID_LIST_LIMIT=38）。空/空白 sid 被 trim 拦截（lib.rs:356-358）。

**真实数据验证**：
- 143/143 文件首行都是 `session_meta`（first-wins 与现实完全一致）；0 个文件缺 session_meta
- 36/143 文件含多条 session_meta；**18/143 文件含 >1 个不同 session_id**（真实触发多会话诊断路径），18 个是同一 id 重复（compaction/rollback 场景，代码去重后不误报——正确）
- session_meta payload 同时含 `session_id` 与 `id`（258/258 相等）——adapter 读 `session_id` 正确（sessiongrep 读 `payload.id` 是同一值，两条路都对）
- 无跨会话泄漏：MessageEvent 不携带会话字段，归属由 ParseReport.session_native_id 在调用方决定；消息先于 session_meta 出现时归入"最终识别出的首个会话"，真实数据不存在该场景
- 被丢弃的可用字段：`cwd`（258/258）、`timestamp`（258/258，且 258/258 与封套外层 timestamp 不同——再次印证外层时间戳是 occurrence-local，spec index.md:28-34 的判断正确）

---

## 4. response_item/agent_message 被静默丢弃 — **P2（真实数据缺失 + 与自身诊断哲学不一致）**

**真实数据**：`response_item` 且 `payload.type == "agent_message"` 共 53 条——content 53/53（真实对话内容：sub-agent 通信文本，带 author/recipient 字段），id 35/53，**role 0/53**。当前代码在 lib.rs:378-380 以 `payload.type != "message"` 静默跳过，**无 skip 计数、无诊断**。

**问题**：这是"有 content 的对话形态记录"，与 adapter 自己的哲学冲突——"unknown role → recoverable skip + 诊断，绝不静默"（lib.rs:381-392, spec index.md:27-29）。同样是不可产出的记录，reasoning 被 spec 显式豁免（index.md:43-48），agent_message 没有豁免依据却走静默路径。

**修复方向**（二选一，最小改动）：a) 在 lib.rs:378 分支给 `agent_message` 加一条 skip 计数 + "sub-agent message without role, skipped" 诊断；b) 若 08-15-structured-activity-context-facets 要采 sub-agent 面（inter_agent_communication_metadata 顶层类型 53 条同现），把 `agent_message` 列为该任务的显式待处理形状。注意 emit 需要 role（MessageEvent 契约），当前无 role 无法发出——`is_sidechain: false` 硬编码（lib.rs:415）与 sub-agent 语义也不符，改动面属 #6 子任务。

---

## 5. 其它形状风险（对应任务书第 4 项）

- **payload 为 null**：正确跳过（lib.rs:375-377 `let Some(payload) = rec.payload else { continue }`）
- **type 未知/缺失**：非 response_item 一律 continue（lib.rs:372-374）；顶层 `type` 缺失 → `#[serde(default)]` 空串 → 与所有已知类型不匹配 → 静默跳过，合理
- **content 为 null / 缺失**（message）：recoverable skip + 诊断（lib.rs:394-401），与 spec 一致
- **content 为空数组或全无 text 的 block**：`to_plain_text()` 返回 `Some("")` → **以空文本 emit 并占 seq**（lib.rs:394-421；properties.rs:199-203 明确断言此行为）。与 sessiongrep 的做法分歧（`sessiongrep/src/providers/codex.rs:146-147` 对 trim 后空文本直接 continue 不计消息）。空文本行是"流式占位/纯图片输入"形态，计入 catalog 会产出无检索价值行——**P2**：建议与 spec 对齐为 skip（或至少文档化该语义），改动会牵动 properties 断言
- summary 字段形状：本 adapter 不抽取，无风险（§0）

---

## 6. 诊断质量 — 基本 privacy-safe，一处 spec 冲突 — **P2**

**安全项**：skip/committed 计数准确（损坏行逐条诊断 lib.rs:343-350；未知 role 诊断只含 role 值 lib.rs:386-390）；行号定位 bounded（BAD_LINE_LIST_LIMIT=5）；serde 错误文本不含正文内容；无路径/文本/消息 id 泄露。

**冲突项**：多会话诊断把 session id 原样写入诊断（lib.rs:436-441），与 spec checklist "Never expose paths, native ids, transcript text, or other source content in diagnostics"（index.md:76）冲突。session id 是 native id。该行为是 PRD R3.1 的显式要求（报告 id 供定位），且有测试钉死（lib.rs:594-598），provider-claude 同款（`provider-claude/src/lib.rs:522`）——属跨 provider 的有意设计。**建议方向**：改 spec checklist 加"bounded native session ids 除外（≤3，R3.1 要求）"，而不是改代码；或两者同时改为"只报数量不报 id"（会牺牲 R3.1 的定位价值）。未决，留给 owner。

---

## 7. state_5.sqlite 合并路径评估（任务书第 7 项）— P2 增强，可借，注意 OOM 坑

**现状**：adapter 只产出 session_native_id，title/cwd/created 全部缺失；真实数据确认 session_meta 有 cwd/timestamp 但**无 title**——title 只能来自 state_5.sqlite 或 session_index.jsonl。

**peer 三方实现核验**：

| 实现 | 打开方式 | 字段 | 问题 |
|---|---|---|---|
| sessiongrep `load_threads`（`sessiongrep/src/providers/codex.rs:233-261`） | `Connection::open`（**读写打开 Codex 的活库**） | id/title/cwd/created_at/updated_at/rollout_path/**first_user_message** | ① 读写打开活库，Windows 上锁冲突；② SELECT 全量 first_user_message blob → **OOM 风险**（cc-switch 注释引 openai/codex#29007："first_user_message can grow large enough to OOM"）——**此条目不可照搬** |
| cc-switch `load_thread_titles_from_db`（`Github_src/cc-switch/src-tauri/src/session_manager/providers/codex.rs:129-176`） | **`SQLITE_OPEN_READ_ONLY \| SQLITE_OPEN_NO_MUTEX` + `busy_timeout(2s)`** | title（SQL pushdown：`WHERE title <> '' AND (first_user_message IS NULL OR TRIM(title) <> TRIM(first_user_message))`，**从不 SELECT first_user_message**） | 注释原文："Codex keeps this DB open and write-locked while running; without a busy timeout a read during a write fails immediately and titles silently drop"——**这是可借的权威模式** |
| hstry `loadThreadIndex`（`hstry/adapters/codex/adapter.ts:196-220`） | `{readonly: true}`（Bun/BetterSqlite） | id/title/model/archived，`WHERE title != ''` | 全失败 fallback 注释："Missing DB, locked file, or older schema - fall back to derived titles"——**fail-soft 语义**可借 |

**Windows fs4 锁挑战**：即便 READ_ONLY，另一进程持有写锁时 Windows 上打开仍可能失败（SQLITE_CANTOPEN/BUSY）——三方一致做法是 busy_timeout + 捕获全部错误 + 静默回退到派生元数据，绝不硬失败。

**架构约束**：adapter 契约是字节级格式隔离（RFC-0002 §7），**state_5 读取必须放 application/discovery 层**（08-15-sixteen-provider-evidence-wave 或 metadata-enrichment 任务），不能进 adapter。

**推荐落地路径（按成本排序）**：
1. 零成本先行：session_meta 已含 cwd + timestamp——先透传即可补 created/cwd，不需要 sqlite
2. title 首选 `session_index.jsonl`（`~/.codex/session_index.jsonl`，JSONL 无锁问题，sessiongrep `load_index_titles` codex.rs:263-281 与 cc-switch codex.rs:115-127 都有现成实现）
3. state_5.sqlite 只作为 title 的最终权威源：照搬 **cc-switch 的 OPEN_READ_ONLY+NO_MUTEX+busy_timeout+SQL pushdown**（不 SELECT first_user_message），加 hstry 式 fail-soft 回退链；**明确不照搬 sessiongrep 的读写打开与全量 blob 读取**

---

## Findings 汇总

| # | 严重级 | 位置 | 问题 | 修复 |
|---|---|---|---|---|
| F1 | **P1** | lib.rs:341 | 每行全量反序列化，92.1% 的行永远无产出（真实统计） | 原始子串预过滤（§1），claude-historian parser.ts:84-94 模式 |
| F2 | **P2** | lib.rs:378-380 | response_item/agent_message（真实 53 条、有 content 无 role）静默丢弃，无 skip/诊断 | 加 skip+诊断，或列入 #6 子任务待处理形状 |
| F3 | **P2** | lib.rs:394-421 | content 空数组/全无 text 的消息以空串 emit 占 seq | 对齐 sessiongrep codex.rs:146-147 的 trim-empty skip（需改 properties 断言） |
| F4 | **P2** | lib.rs:436-441 vs index.md:76 | 多会话诊断含 session id 与 spec 隐私 checklist 冲突（跨 provider 有意设计，R3.1 要求） | 改 spec checklist 加 bounded 例外，或改为只报数量（owner 定） |
| F5 | **P2** | 增强项 | title/cwd/created 元数据缺失 | session_meta 透传 cwd/timestamp → session_index.jsonl → state_5.sqlite（cc-switch 只读模式），application 层实现 |
| F6 | 无 | — | 任务书 3 项前提（function_call pairing、deserialize_optional_string、title/summary 抽取 + `== sid` guard）在本代码库不存在 | 未来工作，真实数据证据见 §3/§4 供 #6 子任务使用 |

**已验证无问题项**：probe bounded 采样与容忍路径（lib.rs:178-298，PRD R2.1/R2.2）；BOM/CRLF/span 字节精确（lib.rs:173, 329-337，golden.rs 双测）；镜像不重复计数（properties 性质 1，真实数据 event_msg 镜像与权威消息比例约 1.4:1，不是记忆中的 1:1，但 adapter 只认权威形态，结论不变）；确定性/span 回切/seq 连续（properties 性质 2-4）；payload null / 未知 type / 破损行路径；超大行（256KiB 覆盖）。

## Caveats / Not Found

- 统计基于本机 143 个真实 rollout（2026-07 前后，codex-cli-rs 0.14x 写出版本）；字段频率随 Codex CLI 版本演进可能变化，但封套结构（type/payload 分类）跨版本稳定
- `inter_agent_communication_metadata` 顶层类型（53 条）不在 `is_known_envelope_type`（lib.rs:127-137）集合内——probe 不认它作封套证据，但实际文件首 16 行必有 session_meta，无影响；若未来该类型成为主要类型需补集合
- golden fixture 为合成数据（session_id `0198aaaa-...` fixture 前缀），未发现真实 transcript 混入
- 未验证：state_5.sqlite 的 schema 版本演进（state_5 → 未来版本）——cc-switch/hstry 均按 best-effort 处理，本项目照搬时需同款容错
