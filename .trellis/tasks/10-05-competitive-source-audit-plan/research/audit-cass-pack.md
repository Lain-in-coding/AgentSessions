# cass 分片 A 审计：证据包（pack）与资产状态（asset state）

- 审计对象：`C:\AgentSessions\Github_src\coding_agent_session_search`（cass），commit `aa92a45311e10a3ac9c40c3730664a7366af2fe8`（与任务记录一致；本分片未做任何 git 写操作）。
- 日期：2026-10-06。范围：T1 全文精读 4 个文件 + 1 个辅助文件（provider 归一化核对）；T2 机械探针仅按 path 过滤查询，不整读 sweep JSON。
- 凭证：5 个文件共 8,421 行全部逐段有界读取（单次 ≤150 行，共 59 个区间），读前/读后 SHA-256 一致（5/5）；未执行 build/test/install/网络/git 写操作；未改动任何竞品源码与其它 TASKDIR 文件。机器凭证见同目录 `coverage-cass-pack.json`。

| 文件 | 行数 | 区间数 | 状态 |
|---|---:|---:|---|
| `src/search/mod.rs` | 76 | 1 | full |
| `src/search/pack_planner.rs` | 3491 | 24 | full |
| `src/search/asset_state.rs` | 4132 | 28 | full |
| `src/search/proof_log.rs` | 473 | 4 | full |
| `src/connectors/mod.rs`（辅助） | 249 | 2 | full |

> 阅读方式：`codegraph node --file <路径> --offset <起> --limit ≤150`（1-based 行号）。两次批量输出被截断时出现可见缺口，均立即用重叠区间补读（288-437、438-454、745-775），未把哈希当阅读。

## 一、pack/证据契约（src/search/pack_planner.rs）

### 1.1 字段与上限
- 默认上限：`max_tokens 12_000 / max_sessions 8 / max_evidence 24 / context_lines 3 / max_excerpt_chars 1_600`（:66-76）；`validate` 范围：tokens 1,024–200,000、sessions 1–64、evidence 1–256、context_lines 0–40、excerpt 80–8,000（:79-87）。
- 候选抓取上限公式：`fetch = clamp(max(max_evidence×8, max_sessions×16), 64, 2048)`（:734-743，cap 常量 :21）。
- `PackCandidate` 字段：candidate_id、source_path/source_id/origin_kind/origin_host、workspace(+original)、agent、line_start/line_end、conversation_id、message_index、content_hash、span_hash、created/indexed_at_ms、match_type、excerpt、role、lexical/semantic/hybrid 分、matched_terms/phrases、query 词数、source_readiness、source_explicitly_requested（:215-244）。`candidate_id = "{source_id}:{source_path}:{line_start}"`（:264-269）；会话键 `(source_id, source_path)`（:306-308）。

### 1.2 预算（选材预算 ≠ 输出上限）
- 静态四分：metadata 15% / outline 15% / evidence 60% / omitted 余量；另计算 `max_output_tokens_with_overflow = max_tokens + max_tokens/20`（:752-768）。
- 只有 evidence 段在选材循环里被强制（:897-905）。仓库级检索确认 `max_output_tokens_with_overflow` 全库仅 3 处出现：定义（:369）、计算（:766）、它自己的测试（:3154）——**无任何执行路径读取或强制它**。
- 输出 `limits.estimated_tokens` 只回填 `plan.estimated_tokens`＝所选证据 token 之和（:922、:969-977、:1477-1485）；metadata/outline/omitted 的渲染体积不受该预算约束，且 omitted 列表为全部被淘汰候选（:1527-1530）。token 估算为 `chars/4`（:19、:1339-1343），仅作用于 excerpt 文本。

### 1.3 排序与确定性
- 贪心逐轮取当前最高分（:814-950）；评分权重：`0.35 relevance + 0.20 coverage + 0.15 freshness + 0.10 source_diversity + 0.10 role + 0.05 source_authority + 0.05 citation_quality − duplicate_penalty`（:1049-1056）。
- 并列决胜链：score → relevance → newer created_at → source_id → source_path → line_start → content_hash（:1264-1293）；有「与输入顺序无关」和分页边界测试（:3413-3456）；空时间戳在并列时排后（:3372-3385）。
- 不是全局最优：它按轮贪心；但 oversized 高分候选会因预算放不下而被跳过，有测试（:3258-3277）。空结果/零候选行为见 :3105-3114。

### 1.4 去重
- 三重判定：`span_hash` 命中、`content_hash` 命中、同一 source_path 的行区间重叠 → 硬淘汰 `DuplicateContent`（:991-1004）；对未选中候选另计 1.0/0.5/0.25 的重复惩罚（:1221-1242）。三种去重各有测试（:3164-3208）。
- 区间重叠只在两端 `line_start` 都存在时生效（:1019-1031）；因 `from_search_hit` 令 `line_end = line_start`（:280），cass 的去重实际是「单行锚点＋内容哈希」级。

### 1.5 证据锚定与可追溯性（重点）
- citation 字段：source_path/source_id/origin_kind/origin_host/workspace(+original)/agent/line_start/line_end/message_index/conversation_id/content_hash/span_hash/excerpt_sha256/created/indexed/freshness_age/match_type/verified（:656-676）。
- `from_search_hit`：`line_start = hit.line_number`、`line_end = line_start`（:252-280）；`message_index` 恒为 `None`（:282）；`span_hash = content_hash`（:284）；excerpt 取 `hit.content`，空则 fallback `hit.snippet`（:288-292）。
- `verified` 的判据仅 `line_start.is_some() && !source_path.trim().is_empty()`（:1927）——不代表渲染时回源核对；`excerpt_sha256` 是渲染时对「当前 excerpt」现算（:1920），没有与入库原文的比对。
- `evidence_id = "ev_" + blake3(source_id|source_path|line_start|line_end|span_hash)[..16]`（:1345-1357）。
- 机械验证：`message_index` 在全文只有 4 类出现——字段声明（:227）、置空（:282）、渲染透传（:1916）、测试夹具（:2460）；没有任何赋值来源。

**结论**：证据可回到「文件 + 行号 + conversation_id + 内容哈希」，但**回不到稳定消息身份**（message_index 空、无 message id、行跨度=单行）；`verified` 语义弱于字面暗示。

### 1.6 渲染与脱敏
- 格式：Json/CompactJson/Jsonl/Toon/Markdown（:433-451、:1360-1379）；schema 固定 `cass.pack.v1`（:1463）；JSONL 空包固定 4 行（:2582-2602）；MARKDOWN 的 Evidence 行形如 `[ev_x] agent source_id path:line(-end)`（:2264-2289）。
- 脱敏管线：encrypted_payload → secret（`redact_text`）→ remote_host → private_path（:1681-1728）；对 source_id/origin_host/omitted 的 candidate_id 另有专用规则（:1730-1754、:2161-2179）；计数进 `privacy.redaction_counts`（:2181-2199，测试 :2936-2955）。
- `RenderedRedaction` 声明 `start_char/end_char`（:701-707），但所有 redaction 由 `push_full_redaction` 写成 `0..len(original)`（:1777-1792）——只能表达「整串被替换」，不能定位字符区间。

### 1.7 新鲜度与就绪度
- Policy：PreferRecent/Strict/AllowStale，默认 PreferRecent、窗口 30 天（:20、:117-122）；Strict 下超窗或时间戳缺失 → 硬淘汰 `StaleUnderStrictPolicy`（:1008-1017）；PreferRecent 下窗口内 1.0、窗口→4×窗口线性衰减、之外 0（:1146-1168）；未知时间戳按 0.25/0/1 分档（测试 :3387-3400）。
- readiness 快照：index_generation/lexical/semantic/active_rebuild/lock_state/missing_database/source_sync_gaps/recommended_action（:187-197）；health 汇总 + warnings + recommended_action 的失败翻译（:1542-1656）；`PackLexicalReadiness` 默认值是 `Ready`（:147-154）——默认构造的请求会得到 `healthy=true`（:1542-1554），属乐观默认。

### 1.8 契约中「声明但不消费/不产出」的字段（发现）
- `PackOmittedReason::SameSessionLowerRank`（:401）与 `FieldMaskExcluded`（:405）在 planner 内**无产出点**（仅枚举、标签 :2364/:2368、序列化测试 :3475-3484）；「同会话更低排」概念实际被 duplicate/区间重叠与 max_sessions 取代。
- `PackPlanRequest.explain_selection`（:329）在规划阶段从未被读取；唯一消费点是渲染侧同名字段 `PackRenderRequest.explain_selection`（:471、:1938）。
- `context_lines`（:62/:72/:83）从未参与 excerpt 构造；excerpt 只来自 SearchHit（:288-292）并按 `max_excerpt_chars` 截断（:1329-1337），context_lines 仅在输出 limits 回显（:1482）。
- 硬淘汰候选的打分调用传 `token_cost=0`（:824-836），因此其 `OmittedPackCandidate.estimated_tokens` 恒为 0（:1313-1327）——对下游审计是失真字段。

## 二、asset_state：新鲜度、失效、并发、fail-open（src/search/asset_state.rs）

### 2.1 维护锁与过期回收
- 单飞载体：`index-run.lock`（key=value 元数据）+ `index-run.lock.meta` 侧车；读取上限 64 KiB，防恶意/损坏大文件（:120-126、:190-195、:207-212）。
- Windows 分享冲突（raw OS err 32/33，:197-205）时改读侧车并视元数据为「活跃」（:218-222）；非冲突打开失败直接返回 Default（`active=false`，:223）。
- 过期回收：读取端在**成功抢占 flock** 后截断陈旧元数据并清侧车（持有锁→证明无人持有→就地清、:261-305）；显式不做 `kill(pid,0)` 存活探测，避免 PID 复用导致永久 orphan（:281-285）；测试覆盖死主回收（:2186-2236）与活主保真（:2239-2294，含「元数据不得被截断」断言）。

### 2.2 心跳 vs 前向进度（stall）
- `updated_at_ms` 由心跳线程每秒刷新（只证存活）；`last_progress_at_ms` 才是索引线程「有进展」信号（:435-447、:1382-1397）。
- `CASS_REBUILD_STALL_DETECT_SECS` 默认 120s、0 关闭（:373-385）；对无进度回调的 `semantic:hnsw` 阶段显式豁免 stall（:398-408）；coordination 层在心跳新鲜但进度停滞时也会判 `Stale`（:1709-1723、测试 :2966-2996）；毫秒精度不做秒量化有回归测试（:2906-2960）。

### 2.3 lexical 新鲜度/失效
- stale 触发条件：年龄超阈值、checkpoint 指向其它 DB、checkpoint 不完整、schema/page_size 契约不符、DB 存储指纹不符（:1351-1416、:1418-1428）；「索引缺失」优先归 `missing` 而非 stale（:1422-1424）。
- 存储指纹 = db_len/wal_len 精确相等 + mtime 容差 ±1000ms（:610-651）；重建进行中即使指纹漂移也保留进度可见性（:1451-1459、测试 :2459-2517）；重建中的 stale 只看 exists/契约（:1404-1408）。
- 进度可用性 gated 在「DB 匹配 + schema 匹配 + page_size 兼容」（:1451-1467）；不兼容时进度字段全部隐藏（测试 :2520-2567）。

### 2.4 semantic 资产
- tier 可查询判定：`ready && current_db_matches != Some(false) && embedder_id 存在 && availability 兼容`（hash 与 fastembed 走不同分支，:1052-1078）。
- 完整分片代可提升为 ready：逐片校验 shard_index 连续、ready、mmap_ready、model_revision/schema/chunking/dimension/total 一致，且路径必须安全（防目录逃逸，:1125-1127、:1129-1167、:1171-1215，测试 :3328-3447）。
- 运行面：quality 优先于 fast（:924-937）；任一 tier 可查即 `can_search=true`（:952-989）；有 backfill 时 status=building 但仍可查（:954-972、:991-1007）；只有存量且 DB 不匹配 → stale/update_available + lexical fallback（:1009-1025）；DB 不可用/加载失败 → error + lexical fallback（:897-915）。
- manifest 读取失败走 `SemanticManifest::load_or_default(...).unwrap_or_default()`（:1093）——资产检查对 manifest 损坏是静默 fail-open（当空资产），不报错。

### 2.5 并发协调与 fail-open（重点）
- 心跳 30s 过期阈值（:1640、:1698-1707）；判定 `Stale` 的 maintenance 走 `AttachOrWait`（:1784-1791）。
- 搜索侧 `decide_search_failopen`：lexical 可用时，无论 Active 还是 Stale 都返回 `FailOpen{reason}`（:1816-1822、:1839-1844）；lexical 不可用时一律 `AttachOrWait`（:1824-1830、:1846-1852）。**回答「失败时是否 fail-open」：只在 lexical 索引可用时 fail-open 到搜索；否则等待锁持有者。** 有双向测试（:3779-3880）。
- `Idle → Launch`（:1785、:1815）；但「锁文件不可读/损坏」被折叠成 Default（active=false）→ Idle → Launch（:223、:310），与「确无维护任务」不可区分。模块头自述的 single-flight/「never duplicate」（:7-10）在本文件内**不被证明**：真正互斥依赖下游 flock 获取点，不在本文件。
- 有界等待默认 5s、轮询 250ms（:1642-1644、:1866-1901，测试 :3626-3777）；事件日志单行 append、无 fsync（:1938-1949）、读取静默丢弃解析失败行（:1964-1973）、截断为 read→write 非原子、上限 500 条（:1912、:1977-1997）；yield 信号（:2005-2040）与 unified view（:2047-2101，测试 :4002-4075）。

## 三、跨 provider 归一化与覆盖清单
- **有归一化，但在仓外依赖内**：`src/connectors/mod.rs:3-4` 明言「多数 connector 实现住在 franken_agent_detection」；:12-44 再导出归一化类型 `NormalizedConversation / NormalizedMessage / NormalizedSnippet / Origin / SourceKind / LOCAL_SOURCE_ID / ScanContext / Platform…`；:200-223 列出 23 个 connector 存根模块；:225-249 的 `get_connector_factories()` 直接取 FAD 工厂，仅 codex 套 CASS 增强 wrapper（:228-247）。依赖钉在 `Cargo.toml:92`（franken-agent-detection 0.1.10，git rev 6d24c532…）。
- pack 层不做 provider 归一化：`PackCandidate.agent` 为直传字符串（pack_planner.rs:223、:278）；origin_kind 以字符串判本地/远端（:1756-1758），并在 :1804 与 `sources::provenance::LOCAL_SOURCE_ID` 比较；`sources/provenance.rs` 再导出 `Origin/SourceKind` 且区分 local/remote/ssh（机械查询 :29、:80、:112、:222-230）。
- 仓内可见 provider 存根（23，目录清点）: aider、amp、antigravity、chatgpt、claude_code、clawdbot、cline、codex、copilot、copilot_cli、crush、cursor、factory、gemini、grok、hermes、kimi、openclaw、opencode、openhands、pi_agent、qwen、vibe。
- T2 sweep 的 `provider_paths` 探针正则为 `.claude/.codex/.gemini/.cursor/.opencode/.aider/.continue/.windsurf/.zed/.goose/.pi/.kiro/.hermes/Antigravity/roo-?code`（sweep-index.json 定义，机械查询）；其中 continue/windsurf/zed/goose/kiro/roo-code 在仓内没有同名存根——探针族谱与运行时适配器命名**不是一一对应**，不能把探针命中当运行期覆盖清单。
- 结论：跨 provider 归一化=有（FAD 提供 `Normalized*` 类型，cass 再导出并消费）；**FAD 内部实现与逐 provider 解析保真未在本分片审读**（残余未决）。

## 四、发现清单（锚点 + 严重度 + 证据）
> 仅记录上游问题/缺口，不改竞品代码。除注明外均为本分片逐行阅读所得；机械项注明「机械验证」。

- **CASS-A-01（P1，证据锚定/验证语义）**：`verified` 仅查锚点存在（pack_planner.rs:1927），`message_index` 恒空（:282、:2460，机械验证无写入点），`line_end=line_start`（:280）。证据回不到稳定消息身份，也没有渲染期回源比对；把 `verified:true` 当「已核对」会误导。正面：content_hash/span_hash/excerpt_sha256 三哈希仍在（:668-670、:1920）。
- **CASS-A-02（P2，预算承诺不兑现）**：只有 evidence 段被强制（:897-905）；`max_output_tokens_with_overflow` 全库无人读取（:369、:766、:3154，仓库级机械验证）；omitted/metadata/outline 渲染无上限（:1527-1530）。宣称的 max_tokens 是「选材预算」而非「输出硬上限」。
- **CASS-A-03（P2，字段无效）**：`context_lines` 通过校验但零消费（:62/:72/:83 与 :1482 回显；全文无上下文扩展逻辑），调参不产生行为变化。
- **CASS-A-04（P2，契约死条目/失真字段）**：`SameSessionLowerRank`、`FieldMaskExcluded` 永不产出（:401/:405、:2364/:2368、:3475-3484）；硬淘汰候选 `estimated_tokens` 恒 0（:824-836、:1313-1327）。
- **CASS-A-05（P2，资产状态竞态边界）**：锁不可读（非 Windows 分享冲突）折叠为 Default→Idle→Launch（asset_state.rs:223、:310、:1785、:1815）；「single-flight/不重复」在本文件内无证明，互斥依赖下游 flock 获取点。
- **CASS-A-06（P3，事件日志）**：append 无 fsync（:1938-1949）；读取静默丢非法行（:1966）；截断 read→write 非原子（:1977-1996），并发 append 可能丢失；上限 500（:1912）。
- **CASS-A-07（P3，脱敏元数据精度）**：`RenderedRedaction.start_char/end_char` 恒为整串（:1777-1792），与字段命名暗示的「替换区间」不符。
- **CASS-A-08（P3，请求字段）**：`PackPlanRequest.explain_selection`（:329）无消费点；渲染侧同名字段（:471、:1938）易混淆。
- **CASS-A-09（P3，默认乐观）**：`PackLexicalReadiness` 默认 `Ready`（:147-154）＋健康判定（:1542-1554）⇒ 默认构造即报 healthy；契约层应改 `Unknown` 或强制必填。
- **CASS-A-10（P3，证据记录未闭环）**：proof_log.rs 模块整体 `#![allow(dead_code)]`，自述 schema 先行、生产/消费后置（proof_log.rs:1-7）；六态 outcome、保留策略、env 密钥守卫已定义并有测试（:29-97、:191-251、:325-473），但 pack 渲染不引用它——「pack 引用结构化 proof」尚不成立。
- 正面证据（防误伤）：确定性并列决胜（pack_planner.rs:1264-1293＋:3413-3456）、三重去重测试（:3164-3208）、oversized 跳选测试（:3258-3277）、freshness 三态测试（:3387-3400）；资产侧死锁回收（asset_state.rs:2186-2294）、stall 毫秒精度（:2906-2960）、fail-open 双向测试（:3779-3880）均有回归测试，不能概括为「未实现」。

## 五、cass 的 pack/证据契约 vs ASG 对照

对照基线（ASG 侧）：review-report.md:55-64（P0-01：cass 已接入 `cass pack`，`pack_planner.rs` 已有预算/引用/去重/漏因/来源/脱敏，「无人 pack」不成立）；asg-main.md:33（cass pack 经 `src/lib.rs:23592-23609,23794-23863` 接入）；asg-main.md:139-141≡ASG-12（resume 指标只测 dry-run preview 与 handoff 有 evidence，需拆 preview/native resume smoke/handoff pack validity/接收完成率）；review-report.md:39（cass 非普通 MIT，附加条款含 analysis/benchmark 等，不能直接复用代码）。

### 5.1 cass 比 ASG 强在哪（字段/行为具体）
1. **成文 schema + 版本位**：`cass.pack.v1`（pack_planner.rs:1463）＋ limits/validate（:58-87）；ASG 侧「handoff pack validity」尚未定义字段级契约（ASG-12）。
2. **选区确定性**：加权打分（:1049-1056）＋七级并列决胜链（:1264-1293）＋输入顺序不变测试（:3413-3456）；ASG 目前没有等价的排序契约。
3. **漏因账本**：`OmittedPackCandidate{reason,score,estimated_tokens}`（:384-392）＋ snake_case reason 枚举（:396-406）；ASG 没有对应结构（也应注意其中 2 个死条目）。
4. **新鲜度三态**：PreferRecent/Strict/AllowStale＋窗口/4×窗口线性衰减＋未知时间戳显式分档（:1146-1168、:3387-3400），把「出处失效时的行为」做成参数化策略。
5. **健康/建议块**：health{warnings, recommended_action, source_sync_gaps, source_readiness}（:561-573、:1556-1656），失败被翻译成可执行建议而非静默。
6. **去重粒度**：span/content/行区间三重判定＋重复惩罚（:991-1004、:1221-1242），比「同会话只留一条」更细。

### 5.2 cass 弱在哪
1. **消息身份/验证**：message_index 恒空、行跨度=单行、verified 仅锚点存在（CASS-A-01）——弱于 ASG 侧保留的 Stable Message / contextual Placement / cursor generation / relocation（review-report.md:81）与源读取前后指纹（review-report.md:79）。
2. **预算只是选材预算**：overflow 字段死、omitted 无界（CASS-A-02）。
3. **契约哑火位**：context_lines 无效、2 个漏因死枚举、explain_selection 双生（CASS-A-03/04/08）——照抄结构会把不一致一起带走。
4. **可移植性受限**：核心在外部依赖（FAD/frankensearch，Cargo.toml:92 等）＋非普通 MIT 附加条款（review-report.md:39）⇒ 只能借鉴契约设计，不能复制实现。

### 5.3 ASG 能借鉴什么（可落地到字段/行为）
1. **预算分区与抓取上限**：15/15/60/余量＋evidence 段硬约束（:752-768、:897-905）＋ fetch 公式（:734-743）。落地到 ASG handoff pack 的 limits 默认值；overflow 字段要么配执行点要么不暴露。
2. **漏因账本**：三元组结构（:384-392）＋ snake_case 原因表；要求「每个声明 reason 必须有产出路径」（避免 CASS-A-04），硬淘汰候选保留真实 token 估算。
3. **并列决胜链**：score→relevance→recency→source_id→path→line→content_hash（:1264-1293）＋顺序不变测试（:3413-3456），可直接作为 ASG 分页/cursor 稳定性的验收模板。
4. **新鲜度策略**：三态 policy＋窗口×4 衰减＋未知时间戳显式分档（:1146-1168、:3387-3400），把 ASG 的 cursor generation/时间约束映射到同一语义。
5. **就绪度投影**：warnings + recommended_action + source_sync_gaps 的失败翻译（:1556-1656），对照 ASG-12 的「preview / handoff validity / 完成率」拆分，让每种失败成为可测字段。
6. **验证三哈希 + verified 要“更严”**：取 excerpt_sha256/content_hash/span_hash（:668-670、:1920），但用 ASG 的稳定 Message ID/placement 替代 cass 的单行 line_start/line_end（:280），并在渲染时回源比对而非仅查锚点存在（修正 :1927）。
7. **不要照抄**：整串脱敏区间（:1777-1792）、乐观默认 Ready（:147-154）、事件日志 read→write 截断（asset_state.rs:1977-1996）。

## 六、残余未决
1. `src/lib.rs`（4.0MB，含 pack CLI 接线）未在本分片阅读——CLI 实际传入的 limits、readiness 填充、context/截断端到端行为未验证；本分片结论止于 pack_planner 自身契约。
2. `SearchHit.line_number` 在各 provider/格式下的语义（消息序号 vs 文件行号）未核查（query.rs/indexer 未读），这直接决定 citation 的可追溯强度。
3. franken_agent_detection（FAD）内部逐 provider 解析保真与版本覆盖未审——provider 清单只到「仓内 23 存根 + FAD 工厂再导出」层。
4. 未运行任何 build/test/网络/git 写操作；行为判断来自源码与既有 `#[cfg(test)]` 文本，未执行测试套件；锁竞争/Windows 分享冲突等运行时行为无真机实测。
5. proof_log 与 pack 是否存在其它接线属 lib.rs 范围；本分片只确认 proof_log 模块自身 dead-code 容忍且 pack_planner 未引用它（机械验证）。
6. 许可证与沿用边界（review-report.md:39 所述附加条款）未经本分片独立核验（未读 cass LICENSE 全文）。