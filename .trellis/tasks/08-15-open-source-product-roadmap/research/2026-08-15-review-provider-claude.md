# Research: provider-claude 解析管线审查(性能 + 健壮性)

- **Query**: 审查 Claude Code provider adapter 的 parse 热路径(逐行 JSON.parse vs claude-historian 两阶段预过滤)、大文件元数据读取、JSONL 健康分诊、ToolActivity R1-R6 配对、title/summary 冲突规则、内存峰值、诊断有界性/隐私
- **Scope**: internal(本地源码 + 3 个参考克隆逐文件核验)
- **Date**: 2026-08-15
- **前置资料**: `research/2026-08-15-borrowable-code-inventory.md`(J1/J4/J9 条目已回读原文核验,行号见下)
- **审查对象**: `crates/agent-session-grep-provider-claude/src/lib.rs`(1090 行,含 89 行实现 + 测试);调用链 `cli/main.rs ingest_file/sync_files` → `adapters-sqlite/source_fs.rs capture/verify` → `application/lib.rs stage/select_and_stage`

---

## 结论摘要

| # | 严重度 | 发现 | 位置 |
|---|---|---|---|
| F1 | P1 | ToolActivity 设计稿 R2 封闭表只收 Claude 大写工具名,Codex 的 `read_file`/`write_file`/`edit_file`/`grep`/`glob`/`web_search` 等小写名全部落 Unknown → Codex 侧 facet 基本失效 | design.md:98-111 |
| F2 | P1 | 设计稿 R1-R6 未写 serde rename:真实 transcript 的配对字段是 `toolUseId`/`isError`(camelCase),tool_use 是 `id`;照设计稿字面实现会配对失败 | design.md:147;lib.rs:101-114 |
| F3 | P2 | `report.diagnostics` 数量无上限:每坏行一条(属性测试断言 len==skipped);10k 坏行 → 10k 条诊断缓冲+返回 | lib.rs:429-441;properties.rs:519-523 |
| F4 | P2 | 诊断内嵌 serde 错误全文 `{e}`,shape 不符时 serde 会回显原始标量值(如 `integer 42`),是诊断里唯一的内容泄漏面 | lib.rs:433-440 |
| F5 | P2 | claude-historian 两阶段预过滤是**查询词驱动**,与全量 ingest 工作负载相反,直接照搬无收益;type-gate 变体可行但会改变 skipped 语义,先测再动 | 见 §1 |
| F6 | P2 | sync 全批次缓冲:所有文件的 StagedBatch 同时存活到统一 commit,批内内存 ≈ 4×Σ文件字节;单文件 700KB 无虞,大语料批要留意 | cli/main.rs:1612-1681 |
| F7 | P2 | 内存模型为整文件读 + 全量文本物化,峰值 ≈ 2-3× 文件大小(700KB → ~2MB);无流式 parse | source_fs.rs:38;lib.rs:479-504 |
| F8 | P2 | read_head_tail_lines 对 ingest 无价值(反正全量 parse);价值在未来的"会话清单快速扫描"——且注意证据:summary/custom-title 在**尾部**不在头部 | §2 |
| F9 | P2 | 任务前提"ToolActivity 已落地 / MetadataResolution 冲突规则"与工作树现状不符:ToolActivity 在设计阶段(未实现),MetadataResolution 在兄弟分支(本工作树不可验证) | §4、§5 |

当前已落地代码(F1-F9 之外)未发现正确性缺陷:snapshot 复核、坏行 recoverable skip、span 语义、first-session 契约均有测试锚定。

---

## 1. 逐行 JSON.parse vs claude-historian 两阶段预过滤(F1/F5)

### 现状

`parse` 对每一行执行 `serde_json::from_str::<RawLine>`(lib.rs:425),无论该行是否对话记录。`RawLine` 含 `message: Option<RawMessage>` → `RawContent` untagged 枚举(lib.rs:74-99),serde 会把整行 tokenize 进类型树——**跳过行的字节也要付 tokenize 成本**(未知字段跳过不分配,但逐 token 扫描是 O(行长))。`is_conversational` 判定在完整反序列化**之后**(lib.rs:461)。

成本结构:serde_json 吞吐约 1-2 GB/s,700KB transcript 全 parse 约 0.4-1ms,文本抽取另计。**仓库无 parse 基准**(properties.rs 只测正确性,256KiB 大字段迭代也仅断言不变量,无计时)。

### claude-historian 原文核验(与清单 J4 一致)

`claude-historian-mcp/src/parser.ts`:
- 行级预过滤 parser.ts:84-94:`lineLower.includes(term)` 任一命中才 `JSON.parse`,注释自称"eliminates 80-95% of JSON.parse calls"——**但这是查询词过滤**:跳过的是与搜索词无关的行。
- 文件级预过滤 parser.ts:149-154:整文件 toLowerCase 无词命中 → 跳过行分割。
- 小文件快速路径 parser.ts:31,144-158:SMALL_FILE_THRESHOLD=400_000,`readFile`+`split` 快于流式(注:与我们无关,我们的字节已由调用方整读)。

### 判定:直接照搬不成立,type-gate 变体有讨论空间

- 我们的 ingest 要**全量**对话行,claude-historian 省下的 80-95% 恰恰是我们不能跳的行。它跳过的是 query 不命中的行——语义是"检索时召回",不是"入库全量"。
- 反例证明:长会话里最贵的是携带 MB 级 tool_result 的 user 行和 system prompt 行,**全部是对话行**,预过滤一行都省不了。
- 可转移点:type 前缀门。Claude Code 记录 type 恒为首字段(全部 fixture/属性测试/cc-switch 解析均依赖此写入习惯)。对每行先 memchr 找 `"type":"` 并读值:值 ∈ {user, assistant, system} → 全量 parse;否则直接跳过该行 tokenize。**前提是 fail-open**:找不到 `"type":"` 标记(如 `"type" : "user"` 带空格)必须照常 parse,零假阴性才成立。收益:长会话中 file-history-snapshot(可 MB 级)、display、summary 行的字节占比可观,能省 20-50% parse 时间。
- **代价/语义变化**(实施前必须决策):被跳过的坏行不再产生 skipped+diagnostic——现有契约是"每个坏行恰好一条诊断"(properties.rs:519-523 断言),且 skipped>0 驱动 relation_complete=false 与 tombstone 抑制(cli/main.rs:1299-1300,1514)。截断行通常 type 字段完整(`{"type":"assistant","message":...` 被截在尾部)→ 仍会被 parse 并计 skipped,所以截断检测不丢失;丢失的是"非对话行的损坏可见性"。这是可辩护的精化,但**改变了可观察行为**,需要同步改属性测试与 golden。
- 结论:先加一个简单计时基准(700KB 合成 transcript,测 parse 耗时),若 parse 在 sync 总时长占比可忽略则不做;若显著,按 type-gate + 语义变更评审实施,不要照搬 J4。

### 附加小发现

- `stage_with_registry` 在 select_and_stage 之后再对全部 adapter 重 probe 一次取 variant_id(cli/main.rs:1258-1262)——每次 ingest 双倍 probe(每 probe ≤16 行 × 2 adapter,成本微小,仅记录)。

## 2. read_head_tail_lines 快速路径(F8)

cc-switch 原文核验(与清单 J9 一致,实文件 `Github_src/cc-switch/src-tauri/src/session_manager/providers/utils.rs:13-49`):
- `<16KB` 全读切分;`≥16KB` 头 BufReader 读 head_n 行 + seek 末尾 16KB 取尾行、跳过首行残片(utils.rs:36-46);Claude 用 (10,30)(claude.rs:128)。

**元数据位置证据(与任务书描述相反)**:cc-switch claude.rs:135-224——**头部**取 session_id/project_dir/created_at/首个 user 消息(标题候选);**尾部逆序**取 last_active_at / summary / custom-title(claude.rs:187-224,注释 "Extract last_active_at, summary, and custom-title from tail lines (reverse order)")。custom-title 记录形态:`type:"custom-title"` + `customTitle` 字段(claude.rs:200-208);summary 跳过 `isMeta` 记录、取逆序首个非空(claude.rs:210-219)。

**判定**:对我们无 ingest 价值——ingest 反正要全量 parse,快照字节已在内存,头尾窗口不会省任何东西。价值场景是路线图里的**会话清单扫描**(16-provider 目录下列出 sessionId/title/最后活动而不全量解析,即 cc-switch `scan_sessions` 的用途,claude.rs:17-126)。若 evidence-wave 要加"未索引会话的 resume 预览/清单",verbatim 借用 utils.rs:13-49(MIT,归 CC-switch)。配套 J10 `parse_timestamp_to_ms`(utils.rs:51-65,三格式毫秒归一)可作 domain 时间解析借点。TITLE_MAX_CHARS=80/truncate_summary 160 是现成阈值(utils.rs:9,130-142)。

## 3. JSONL 健康分诊(F6 之外的第 3 项,无独立严重度)

fast-resume 原文核验(与清单 J1 一致,`fast-resume/src/adapters/shared.rs:21-25` enum,`194-224` jsonl_health,`169-185` 分诊调度):Clean(全合法)/ Partial(坏行后仍有合法行)/ Invalid(尾截断);Invalid→Retain(不 parse 不删旧索引)、Partial→parse 且 `partial_session_is_usable` 过滤、Clean→失败才 Delete。

**我方当前行为(agent 正在写文件时)**:
1. capture 读取中文件被追加 → 只认初始 len 范围截断(source_fs.rs:40)+ 提交前 verify 复核 len/mtime/fingerprint(source_fs.rs:56-83)→ `SnapshotChanged` → **整个 ingest 中止,零提交**(sync 在提交前统一 verify,cli/main.rs:1676-1678)。写中文件的脏读被快照门挡住,这是比 fast-resume 更强的防护。
2. 文件在 capture 时已稳定但尾行截断(崩溃残留)→ 尾行 parse 失败 → skipped+1 + 诊断(lib.rs:429-441)→ relation_complete=false(cli/main.rs:1514)→ 已观测事实可提交、**不推导 tombstone、撤销旧 completeness 标记**(cli/main.rs:1299-1300)。下次 sync 指纹变化 → 全量重 parse → 完整替换。语义上等于 fast-resume 的 Partial 分支。
3. 差异点:fast-resume 的 Invalid→Retain 是"**不 parse 直接保旧**";我们总是 parse(尾截断的会话仍会全量 parse 一遍再提交前缀)。且注意 fast-resume 的 jsonl_health 本身是 O(n) 全行 serde(shared.rs:201-216)——它用一次全量扫描换"避免 Session 物化",对我们是"健康检查与 parse 同价"。

**判定/建议**:我们的快照门 + incomplete 语义已经安全(无数据丢失:incomplete 时旧 claims 按 union 保留,不 tombstone)。真正可借用的是**尾行探针**而非全量健康扫描:parse 前 seek 末尾 ~64KB 检查最后一行是否完整 JSON、文件是否以换行结尾——对"正在写的文件"给一个 O(1) 预判(等价 agentsview `isTruncated` 的线级判定,见清单 J13),可在大批量 sync 时跳过已知无效文件,成本 ≈ 0。全量健康分诊(Clean/Partial/Invalid 三级)对我们无额外价值,因为 parse 本身已逐行容错,而 Invalid 分支的收益已被快照门覆盖。

## 4. ToolActivity 抽取 R1-R6 核验(F1/F2)

**现状核对**:任务描述称"ToolActivity extraction (just landed)"——**实际未落地**。当前 lib.rs 只 emit `MessageEvent`(lib.rs:491-504),ports 的 `CanonicalEventSink` 只有 `emit_message`(ports/lib.rs:379-386)。R1-R6 规则集在 `.trellis/tasks/08-15-structured-activity-context-facets/design.md:76-141`(设计稿,implement.md 尚未开工)。以下按"设计稿 vs 真实格式 vs 现有 parser 结构"核验,供实施时使用:

**配对机制(设计正确)**:
- 设计稿:解析器持 `tool_use_id -> PendingCall { name, input, caller_uuid, caller_is_sidechain }` 映射;tool_use(assistant 消息)→ 记录;tool_result(user 消息)→ 按 id 解析并 emit(design.md:147-155)。与现有单遍 parse 循环结构兼容(状态存活于 parse 作用域)。
- R3 actor 规则(design.md:113-117):caller 消息 `is_sidechain` → Subagent。`RawLine.is_sidechain` 已解析(lib.rs:61-62),无缺口。
- R4.3/R5.1 EOF 未配对:循环结束后把 pending 以 `status=Unknown` 发出并挂到 caller 消息(design.md:123-130)——挂在 parse 末尾(现有多会话诊断所在位置,lib.rs:509-527 之后),结构可行。
- R6 fail-closed(design.md:136-141):无配对 tool_result 静默跳过、空名跳过、未知形态不产生活动——与本 adapter 的"未知记录静默略过"哲学一致(lib.rs:461-463)。

**F1(P1)—R2 封闭表漏掉 Codex 工具名**:design.md:98-111 的表格只收 Claude 大写名(`Bash`/`Read`/`Write`/`Edit`/`MultiEdit`/`NotebookEdit`/`ApplyPatch`/`Glob`/`Grep`/`WebFetch`/`WebSearch`/`Task`)与 `shell`/`exec`。Codex transcript 的 custom_tool_call 工具名是小写 snake_case:`read_file`/`write_file`/`edit_file`/`str_replace_editor`/`apply_patch`/`grep`/`glob`/`web_search`/`web_fetch`/`notebook_edit`。按"大小写敏感精确匹配"(design.md:66-68,107),Codex 的文件/查询/Web 类活动**全部落 `Unknown`/`None`**——facet 过滤对 Codex 形同虚设,且违反设计自述"两 provider 均发出精确已知名"的前提。实施前必须扩展表格(或改为每 provider 一张映射表 + 交集闭包)。

**F2(P1)—serde rename 未写进设计**:真实 Claude Code JSONL 中 tool_use block 的配对字段是 `id`(tool_use)、`toolUseId`(tool_result)、`isError`(tool_result,布尔)。design.md:147 列的字段名 `id`/`tool_use_id`/`is_error` 是 Rust 侧命名,**未注明必须加 `#[serde(rename = "toolUseId")]` / `#[serde(rename = "isError")]`**(现有 RawBlock 已示范 camelCase rename 写法,lib.rs:101-114 的 `kind` rename="type")。这是实施时最可能静默失败的配对断点——照字面实现则 tool_result 永远解析不出 tool_use_id,全部活动按 R6 静默丢弃,且无诊断提示。

**P2 补充点**:
- 同一 `toolUseId` 出现多个 tool_result(Claude Code 偶发重放)时映射的幂等语义未定义:建议 first-resolve-wins、后续同 id 静默跳过(R6 精神),避免重复活动。
- R5.3(design.md:133-134)"锚消息未成为 canonical 消息则丢弃"——user 记录恒为对话行,不会触发;EOF 未配对挂 caller(assistant)消息,assistant 恒为对话行,同样不会触发。该分支实际不可达,保留作为防御即可。
- `ToolActivityEvent.message_native_id` 挂 user 记录 uuid(design.md:152-154)——user 记录已是 canonical 消息(其 text 含 tool 输出),关联成立。

## 5. title/summary MetadataResolution 冲突规则(F9)

**现状核对**:任务描述称"custom-title vs summary 冲突规则"——**本工作树无此代码**。`MetadataResolution<T>`/`ProviderSessionObservation` 等类型在兄弟分支 `session-metadata-search-08-15`(未提交,见 `.trellis/tasks/08-15-resume-metadata-execution/design.md:10-14,29-37` 的镜像说明),本工作树不可读不可验证。

**本工作树实际行为**(可验证部分):
- `type:"summary"` 记录被 `is_conversational` 排除后静默跳过(lib.rs:461-463;golden 第 3 行,PROVENANCE.md:24);`type:"custom-title"` 同样被跳过——**title/summary 全部在 adapter 边界丢弃**。
- 唯一的 first-wins 规则:session_native_id = 首个非空 sessionId(lib.rs:448-458),多会话诊断封顶 3 条(lib.rs:511-527)。

**参考证据(供 metadata 分支合流时对照)**:cc-switch claude.rs 的标题优先级链 `custom-title > 首个 user 消息 > 目录 basename`(claude.rs:229-238),summary 取尾部逆序首个非空且跳过 isMeta(claude.rs:210-219,240)。这是现成的"冲突消解"实现(MIT 可借),与任务书"custom-title vs summary records"的疑问直接对应:cc-switch 的规则是 **custom-title 绝对优先**,不存在平级竞争;summary 与 title 独立(标题只与首条 user 消息竞争)。若 metadata 分支的规则与之一致,则"first-wins/ambiguous 转换"无歧义;若不一致,建议以 cc-switch 为准(其规则经真实 corpus 调参,10/30 头尾窗口见 claude.rs:128)。

## 6. 内存(F6/F7)

- **单文件**:整文件 `read_to_end`(source_fs.rs:38)+ 逐行 parse + 每条消息 `to_plain_text` 物化 String(lib.rs:479-504)入 `StagingSink` 全量缓冲(application/lib.rs:381-388)。user 记录文本 = tool_result 全部输出拼接(设计使然,golden 第 7 行)。峰值 ≈ 输入字节 + Σ抽取文本 + 快照复核二次整读(source_fs.rs:73-75)≈ **2-3× 文件大小**;700KB transcript → 约 1.5-2.5MB,无压力。
- **sync 批次**:所有文件的 capture 字节 + StagedBatch 同时存活到统一 commit(cli/main.rs:1612-1681,原子批次是有意设计)。N 文件批 ≈ 4×Σ文件字节。100×700KB 批 ≈ 250-300MB 峰值。路线图有"大语料同步"目标时需关注;缓解方向:批内按大小分桶提交或 stage 阶段即丢弃 capture 字节(capture 返回的 bytes 在 stage 后可 drop,仅 snap 参与 verify——当前循环里 bytes 活到循环末尾,可显式 drop 省 1×)。
- 无流式 parse;probe 需要字节、parse 需要字节,streaming 收益只在"超大单文件"场景(sync 的指纹缓存已跳过未变文件,cli/main.rs:1635)。

## 7. 诊断有界性/隐私(F3/F4)

- **有界**:probe 样本窗口 16 行、破损容忍 3、行号列表 ≤5、会话 id 列表 ≤3(lib.rs:18-25,220-234);CLI 展示层再截断 16 条 × 512 字符(cli/main.rs:41-42,139-166)。adapter 只见字节、永不见路径——probe/parse 错误消息均不含源路径(有测试锚定:privacy-safe 措辞见 cli/main.rs:280-281,1590-1601)。
- **F3(P2)**:`report.diagnostics` 数量无上限,每坏行一条(lib.rs:429-441),属性测试断言 `diagnostics.len() == skipped`(properties.rs:519-523)。10k 坏行 → 10k 条诊断缓冲并随报告返回(CLI JSON 只报 count,cli/main.rs:1569,但 Vec 全程存活)。修法:adapter 内封顶(如 64 条 + "等 N 条"尾注),同步改属性测试与 golden。
- **F4(P2)**:shape 不符行的诊断含完整 serde 错误 `{e}`(lib.rs:434,440)。serde_json 的 Display 会回显标量值(实测 `content:42` → "invalid type: integer `42`")。这是诊断里唯一能回显源内容的面——量小(标量片段),但"诊断不携带源内容"原则(design.md:309-317 的边界声明)下应收紧:shape 错误只报类别 + 行号,不附 serde 详情(代码已区分 syntax/shape 两分支,去掉 {e} 即可,零成本)。

---

## 借用候选汇总(peer 证据核验后)

| 候选 | 来源(核验行号) | 判定 | 落点 |
|---|---|---|---|
| read_head_tail_lines | cc-switch providers/utils.rs:13-49(MIT) | 建议借(会话清单扫描,非 ingest) | evidence-wave 的未索引会话预览 |
| parse_timestamp_to_ms | cc-switch providers/utils.rs:51-65(MIT) | 建议借 | domain 时间解析 |
| title 优先级链 + summary 规则 | cc-switch providers/claude.rs:200-240(MIT) | 建议借 | metadata 分支合流对照 |
| type-gate 预过滤(非 J4 原版) | 我方设计,启发自 claude-historian parser.ts:84-94 | 条件实施(先基准,接受 skipped 语义变更) | provider-claude parse 热路径 |
| 尾行截断探针 | 启发自 fast-resume shared.rs:194-224 + agentsview isTruncated(J13) | 建议实施(seek 尾 64KB,O(1)) | sync 预判 |
| jsonl_health 三级分诊 | fast-resume shared.rs:21-25,169-224 | **不借**(全量扫描与 parse 同价,快照门已覆盖) | — |
| 两阶段查询预过滤 | claude-historian parser.ts:84-94,149-154 | **不借**(查询驱动,与 ingest 负载相反) | — |

## Caveats / Not Found

- **ToolActivity 未落地**:R1-R6 仍是设计稿(08-15-structured-activity-context-facets/design.md),F1/F2 是"实施前必须修"的设计缺口,不是已存在代码的 bug。
- **MetadataResolution 不可验证**:类型在兄弟分支 session-metadata-search-08-15(未提交),本工作树无对应代码;custom-title/summary 冲突规则仅能以 cc-switch 实现作对照,任务书中的具体规则集未在本工作树找到原文。
- 无 parse 性能基准(properties.rs 仅正确性);所有耗时估计为推理值,实施前应补一个计时测试。
- 本审查未运行测试/构建(只读任务);行号均经 Read/Grep 实测。
