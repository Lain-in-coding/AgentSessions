# Research: Application crate 审查(2026-08-15,面向开源路线图)

- **Query**: 审查 `agent-session-grep-application` crate(字节预算/检索装配性能/redaction seam/handoff pack 确定性/retrieval degradation/resume preview/CJK bigram),对照 hstry peek + claude-historian smart truncation,产出 P0/P1/P2 分级 findings 与借用候选
- **Scope**: internal(本仓库 + 本地固定 clone)
- **Date**: 2026-08-15
- **审查对象**: `crates/agent-session-grep-application/src/{lib,budget,cursor,evidence,cjk}.rs`(全部 5 个文件,本 worktree 无 redaction.rs/handoff.rs/thread.rs)

## 重要前提(必须先读)

**08-15 各子任务(redaction seam / handoff pack / ResumePreviewService / retrieval_mode)的代码均不在 main 分支**:
- `RetrievalMode` / `RetrievalCapabilities` / `retrieval_degradation` 在 `crates/` 下零命中 —— semantic-hybrid 设计自称 "contract-and-plumbing slice implemented (2026-08-15)",但代码在兄弟分支(`session-metadata-search-08-15`、`worktree-agent-*`),未合并
- `RedactionMetadata` / `OutputBoundary` / `CapabilityManifest` 同样零命中(unified-release-contract D3 的 ports 类型未落码)
- `application/src/handoff.rs` 不存在(evidence-handoff-pack design D1 的目标文件)
- `ResumePreviewService` 只在 `08-15-resume-metadata-execution/design.md` 出现

因此 **focus 3/4/5/6 是 design 级审查**(基于子任务 design.md + ADR + PRD),focus 1/2/7 是代码级审查(实测行号)。design 级结论标注了合并前的验证点。

---

## 1. ResponseBudget 字节核算(代码级)

### 1.1 正面结论

- `json_string_len`(lib.rs:42-51)与 serde_json 转义规则一致:引号/反斜杠 2 字节、0x00-0x1F 为 `\uXXXX` 6 字节;把 0x7F 按 6 字节计是保守方向(serde_json 不转义 0x7F,实际 1 字节)——方向上正确
- `lossy_payload_json_len`(lib.rs:59-95)精确建模 `String::from_utf8_lossy` + JSON 转义(U+FFFD 3 字节、无效序列逐字节推进),与 CLI 渲染 `String::from_utf8_lossy`(main.rs:1788)一致;`list_byte_gate_counts_serialized_payload_inflation` 测试(lib.rs:1591)用控制字节验证了膨胀计费
- ENVELOPE_RESERVE_BYTES=1024(lib.rs:34)+ MIN_RESPONSE_BYTES=4096(budget.rs:21)⇒ 净预算 ≥3072;reserve 是调用方义务的文档声明(budget.rs:5)与实际扣除(lib.rs:574, 648, 823)一致
- `clamp_items`(budget.rs:127-154)保序贪心、先条数后字节、reason 报字节闸的约定有测试锚定(budget.rs:264);context 把 max_items 映射回 max_messages 旋钮名(lib.rs:833-836)
- 空截断页终止分页、不产生死循环游标(lib.rs:588-590, 660-665 注释 + 语义正确)

### 1.2 [P2] Search 命中字节估算在字段为 null 时系统性低估 —— "最终序列化字节硬门"名不副实

估算(lib.rs:579-586)= `json_string_len(id) + [session_id? +14 : 0] + [text? +8 : 0] + 32`,而实际渲染(main.rs:1729-1738)`{"id":..,"score":<f32>,"session_id":<S>,"text":<T>}` 中 **session_id/text 为 null 时仍序列化为 `"session_id":null` / `"text":null`**,估算却计 0。逐情况核算(score 为 f32,ryu 最短表示 ≤13 字符,ports lib.rs:155):

| 字段状态 | 实际固定开销 | 估算固定开销 | 差(低估) |
|---|---|---|---|
| 两者都在 | 38+D,slack=16−D | 54 | 安全(≥3) |
| session_id=null,text 在 | 42+D | 40 | **2+D 字节/条**(≥3) |
| 都在 null | 46+D | 32 | **14+D 字节/条**(≥15) |
| session 在,text=null | 44+D | 46 | D≥3 时低估 D−2 |

text=null 很常见(payload 非 JSON / 无 text 字段,见 lib.rs:1127 测试 3/4 命中无 text)。页 1000 条 × 27 B ≈ **27 KB 超预算**,4 MiB 默认预算下越 0.6%。List 路径经复核**安全**(固定开销 20 vs 估算 22,余 2 B/条,此前草算有误,以此为准)。

- **修复**(任选):(a) 把 `+32` 提为 `+48`(覆盖 null 情形最坏 27 B)并补一条测试:真实渲染 `serde_json::to_string(&data).len() ≤ max_response_bytes`;(b) 仿 handoff design D3 的收敛式做法,渲染后实测裁尾。最小改动是 (a) + 固定关系测试
- 注意:handoff 的收敛式核算(design D3)已规避此问题,Search/List 路径还停留在估算常量 —— 收敛式与估算式的差距是本 crate 内的一致性隐患

### 1.3 [P3] Context 字节估算用原始存储字节数,响应嵌入的是重序列化 Value

lib.rs:801 `payload_len = bytes.len()`、lib.rs:828 `session_bytes.len()`,但响应嵌入的是 `serde_json::from_slice::<Value>` 后重序列化的结果 —— 数字格式(如 `1e3` → `1000.0`)可能改变长度,闸门双向漂移。96 的固定余量(lib.rs:810)覆盖常见情形;极端浮点 payload 下可能低估。P3,不必修,记录即可。

## 2. 检索路径性能(代码级)

路径:`index.query`(lib.rs:540)→ skip/take 切片(542-546)→ `get_many` + `session_of` 批量取(553-555)→ 每条解析 payload 取 text(558-571)→ clamp(575)。

### 2.1 [P2] 每条命中整树解析 payload 只为取一个 text 字段

lib.rs:561-569:`serde_json::from_slice::<serde_json::Value>(&bytes)` 把整条消息 JSON(可能 1 MB 级 tool_result)全量构树 + 全部字符串克隆,然后只读 `text` 并截 2000 字符。之后该 Value 即弃。热路径上每条命中 1 次峰值内存 2×payload。
- **修复**:派生轻量结构 `#[derive(Deserialize)] struct TextOnly { text: Option<String> }` 只提取该字段(serde 跳过其余树);更彻底用 `Deserializer` + visitor 只读 text 前 N 字符,连 text 全文克隆都省掉
- 该解析在字节闸裁剪**之前**执行(lib.rs:575),被闸丢掉的命中也白解析了 —— 可考虑先按估算裁剪再解析,但会破坏"text 字节计入闸门"的语义,需谨慎

### 2.2 [P3] full policy 下重复 placement 的 payload 深克隆

lib.rs:788-804:`payloads` BTreeMap 缓存了解析结果,但命中缓存时 `payload.clone()`(lib.rs:791)是**整树深克隆**;同一条消息 N 个 placement → N 次 O(size) 克隆。实际重复率低(正常会话 <2),P3。修复:Arc<Value> 或对未命中才 clone。

### 2.3 [P3] 其余热点可接受

- fetch 窗口 capped 于 MAX_FETCH_WINDOW=1M(lib.rs:39,539)防伪造 offset 溢出负 LIMIT —— 但伪造 cursor 仍会触发 1M 条 fetch,量级需 sqlite 层 LIMIT 兜住(端口契约外,记录)
- 每请求 `query_digest` 只算一次(lib.rs:525);cursor 字符串克隆(lib.rs:460-461,488-489)微不足道
- BTreeMap 仅在 context 装配(lib.rs:755-764)与 payload 缓存,无 HashMap 迭代序问题;`format!` 仅错误路径(lib.rs:362,772,781,848)

## 3. Redaction seam vs ADR-0009(design 级,代码未落)

### 3.1 [P1] 两个 P0 兄弟任务定义了互相冲突的"ADR-0009 字段集"

- **unified-release-contract D3**:`RedactionMetadata` = `mode`(`default`/`reveal`)、`status`(`applied`/`none`/`partial`)、`ruleset_version`、`redacted_count`、`audit_id: Option<String>`,自称 "exactly the PRD/ADR-0009 field set"
- **evidence-handoff-pack D4**:`RedactionConfig { mode, ruleset_version, audit_id, redactor: Box<dyn Fn(&str)->RedactionResult> }`、`RedactionResult { text, redacted_count }`、status 派生 4 值 `not-applied`/`applied`/`clean`/`revealed`、mode 含 `noop`
- **ADR-0009 本体**(docs/adr/ADR-0009-cross-boundary-output-redaction.md,全文已读)**没有定义任何字段形状** —— 只有决策与后果。两份 design 都声称"以 ADR 为权威",但权威源是空的,实际存在两套:status 词汇 `none` vs `not-applied` vs `clean`,`partial`(contract)在 handoff 侧无对应;mode 词汇 `default` vs `noop`
- 而 handoff design 明确把 `RedactionMetadata` 字段名/顺序 + `RedactionConfig` 形状称为"兄弟任务共建的 seam"(design.md:178-180),offline-privacy-hooks 将按其中一套实现 —— 合流必然撞车
- **修复**:合并前由 unified-release-contract(契约 owner)定一套词汇(建议:mode ∈ `default|reveal`,status ∈ `none|applied|partial`,handoff 的 `clean` 并入 `none`、`not-applied` 并入 `none`),并回填到 ADR-0009 正文(ADRs 目前只记决策不记形状,两份 design 的引用因此悬空);同时把 `Box<dyn Fn>`(非 Clone/Send)标注为不可跨线程/不可持久化,提示后续 Web UI 边界

### 3.2 [P3] handoff 侧"sums counts over kept entries only"的顺序依赖

design D4:redactor 对**保留条目**计数 —— 若 redactor 在 clamp 前运行、计数在 clamp 后,则"运行过的条目"与"计数的条目"不同,`clean`(运行 0 替换)语义在截断边界会失真。建议显式规定:先 clamp 后 redact(或先 redact 后 clamp,二选一并写进 seam 文档);redactor 闭包本身必须确定性(正则即可),否则破坏 D1 的 byte-identical 承诺。

## 4. Handoff pack 构建器确定性(design 级,代码未落)

### 4.1 确定性设计本身成立

- 纯函数、无时钟、`Vec` + `BTreeSet`(design D1)排除 HashMap 迭代序;pack 内无浮点、无路径(lib.rs evidence DTO 同:source_document_id 内容寻址、无绝对路径)
- 与既有代码一致:context 装配的 occurrence 顺序确定性(select_mainline/select_full 的 placement 顺序,lib.rs:724-746),`clamp_items` 保序
- `bytes_used` fixpoint 单调收敛:只删不增,长度单调降 ⇒ 数字位数单调降 ⇒ 终止;不会振荡。**需补显式迭代上限 + 超限 fail-loud**(design 只说 "bounded",未给界)

### 4.2 [P1] pack_id 内容寻址缺了"内容"

design D2:`pack_id = hp_v1_<blake3(pack_version+session+generation+query+window+policy)>` —— **不含 messages/evidence 内容,也不含 redaction 模式**。同会话、同元数据、不同预算(截出不同消息集)或不同 redaction mode(noop vs redacted)产出不同 pack 内容却同 pack_id —— 而 PRD 要求 pack "可离线保存/校验/重渲染/复现",pack_id 正是校验锚。当前定义是"请求寻址"而非"内容寻址",名不副实。
- **修复**(实现前成本最低):pack_id 输入加入序列化主体的 blake3(至少 messages+evidence+redaction mode+truncation),或直接对最终序列化字节取 blake3(与 bytes_used 收敛循环同轮完成,零额外开销);draft schema 尚在 unified-release-contract D4 注册期,现在改无迁移成本
- [P3] 输入是裸字符串拼接,理论上 `("a","bc")` vs `("ab","c")` 歧义 —— 结构化元组序列化后再哈希

### 4.3 [P2] 字节闸收敛循环是 O(n²)

design D3:"loop rebuilds the pack, measures to_string().len(), and drops the tail entry until it fits" —— 最坏 500 条消息 × 每次全量重序列化(4 MiB 级)= 数百 MB 序列化工作量,踩中路线图 p50/p95 延迟基准。**修复**:先按逐条预估(复用 clamp_items 估算)+ 贪心裁尾,再实测一次 + fixpoint 校正(通常 ≤2 轮);把全量重序列化收敛留给罕见边界。

## 5. Retrieval degradation 管线(design 级,代码未落)

### 5.1 设计方向正确

- `resolve_retrieval(mode, caps)` 纯函数(D2):Lexical 恒直通;Semantic/Hybrid + unavailable → served Lexical + `RetrievalDegradation{reason: SemanticUnavailable}` —— fail-visible,与 PRD Q39("禁止静默切换")一致,且规避了 memex 静默降级 FTS-only 的 anti-pattern(inventory Don't Borrow #4)
- cursor 绑定:非 Lexical 摘要加 `retrieval:<mode>\n` 前缀,Lexical 路径 `digest_query(query)` 逐字节不变(D3)⇒ 既有 cursor 行为不动;跨模式续读显式 `cursor_invalid`
- **"默认 lexical 字节级不变"声明核验**:成立的前提有三,均已满足设计:(a) `App::new`/`with_clock` 签名不变(新增字段走 Default);(b) CLI 渲染只在降级时产出 `data.retrieval`(D5:默认路径两者都不发);(c) `AppResponse::Search` 新增的 `retrieval_degradation: Option<...>` 字段**不得**经 serde 直序列化进 envelope —— 当前 render 是手写 `json!`(main.rs:1726-1742),天然满足,但合并时若改回派生序列化会破功。**验证点:合并后跑 e2e envelope-shape 断言 + smoke(design 自述的护栏),确认 0 diff**

### 5.2 [P3] PRD 措辞与设计形状不一致

PRD Q39 写 `retrieval_mode=lexical_fallback`(扁平键值),design D5 是 `data.retrieval = {mode_requested, mode_served, reason}`,`mode_served` 值域 `lexical/semantic/hybrid`(无 `lexical_fallback`)。功能等价,但宣传/文档口径要统一(契约任务 owns 字段名,已声明);建议 PRD 措辞更新或 reason 串里保留 "lexical_fallback" 字样便于 grep 审计。

## 6. ResumePreviewService 独立于 App(design 级)

### 6.1 合理性:成立,但定位是"合并期临时 seam"

design D2 的理由(App 在兄弟分支被大改,独立服务保持零重叠)是务实的合并策略;`ResumePreviewService<R: ResumeClaimsStore>` 泛型 + `NoResumeClaims` 占位 + 合流后一行换 `store_ref(store)`(D5)均可信。Fail-closed 三值枚举(NoClaims/ConflictingClaims/UnsupportedProvider)与 PRD resume 要求对齐。

### 6.2 [P3] 与"Application ADT 是唯一行为权威"原则的张力

roadmap(prd.md:38)声明 ADT + Robot Protocol 是唯一行为权威;独立 service 是 ADT 外的第二条路径,MCP/Robot 无法经 AppRequest 触达。作为 CLI-only、只读、不 spawn 的临时 seam 可接受;**合流时应折叠为 `AppRequest::ResumePreview` 用例**,否则后续 Web UI(首发项)要 resume 预览就得再开一个洞。记入 roadmap,合并时处理。

## 7. CJK bigram(代码级)

### 7.1 [P2] 实现分配密集,索引热路径可优化

`bigram_cjk`(cjk.rs:30-60)为每个 bigram 走 `format!("{}{}", pair[0], pair[1])`(cjk.rs:49)+ `Vec<String>` runs/parts/tokens 多层 + 两次 `join` —— N 字连续汉字产生 N−1 次堆分配(N=10⁴ 消息 ≈ 10⁴ 个 6 字节小字符串)。索引写入逐消息调用,是批量热路径;查询侧每查询一次(可接受)。**修复**:单一 `String` 输出缓冲 + `push` + 空格插入,一次预分配;runs 用 `Vec<char>` 可改为索引区间。

### 7.2 正确性

- 索引/查询两侧同一 transform、幂等(测试 cjk.rs:117-121)✓;非 CJK 纯文本路径逐字节不变(测试 80-89)✓
- `is_han`(cjk.rs:14-19)覆盖主区 + 扩展 A + 兼容区 + 扩展 B–F;**缺 CJK 扩展 G/H**(U+30000–0x323AF,Unicode 13/15)[P3],罕见,影响极小
- [P3] 混合输入(含汉字)时 `split_whitespace` + `join(" ")` 会把 \t/连续空格归一为单空格 —— 文档声称"非 CJK 文本原样保留"只在纯非 CJK 输入下成立;两侧同规归一,FTS 语义无害,但文档措辞应注明
- 单字查询 → 空串的已知边界(cjk.rs:10-11)与 App 空查询拒绝(lib.rs:518)分层,互不干扰 ✓

## 8. 借用候选(peer 对照,复用 inventory 核验过的行号)

| 候选 | 来源 | 用途 | 备注 |
|---|---|---|---|
| hstry `PeekBundle`/`build_peek`(peek.rs:46,66) | MIT | context pack / 会话预览的字符级旋钮(240/240/400/6/80/30)+ tools BTreeMap 统计 + `has_text_content` 排除 tool_result 合成 user 消息 | 本次实测核验:**字符级预算,无字节闸** —— 与 ResponseBudget 是互补层(预览内容选择 vs 输出字节门),不是替代;借用时保留 `truncate_chars` 字符边界截断与路径扫描的保守性 |
| claude-historian `smartContentPreservation`(parser.ts:378-481) | MIT | 当前 snippet 是朴素字符前缀截断(lib.rs:568);该实现按 code/error/technical/conversational 分型保内容(代码块、堆栈、首错误) | 提升 snippet 质量的空间;注意 inventory Don't Borrow #2 警告其**评分体系**不可搬,但截断启发式本身可搬;建议搬运时把阈值做成常量 + 测试锚定(记忆该仓库 `relevanceScore >= 2` 才做提取的门槛设计,parser.ts:116) |
| claude-historian `estimateTokens` ceil(len/4)(formatter.ts) | MIT | handoff 的 token 估算 header | inventory H4 |
| sessiongrep `build_transcript_summary` 书挡式摘要 | Apache-2.0 | preview 渲染 | inventory H3;需 NOTICE |
| fast-resume yolo 判定 + resume 命令表 | MIT | resume-metadata-execution 子任务 | inventory R1,非本 crate |

## 9. 未发现/风险清单

- 本 worktree 无 08-15 子任务代码:合并时按 §5.1 验证点 + §3.1 词汇收敛 + §4.2 pack_id 修正执行,可避免三处已预见的合流冲突
- 既有代码未发现 P0(无数据损坏/无限循环/非法内存风险);P1 均集中在 design 阶段的跨任务契约冲突
- List 字节闸经复核安全(+2 B/条余量),此前版本草算的 -1 为误算,以本文为准
- hstry clone 已实测 `peek.rs` 全文 512 行,行号 46/66 与 inventory H1 一致;"memex peek bundle" 归属 hstry 的更正成立(memex 无 PeekBundle)
