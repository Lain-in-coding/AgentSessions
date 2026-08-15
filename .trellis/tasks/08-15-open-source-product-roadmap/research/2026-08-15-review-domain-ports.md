# Research: Domain & Ports Crate Review (2026-08-15)

- **Query**: 审查 agent-session-grep 的 domain/ports 两个 crate（StableId/PlacementId/select_mainline/ToolActivity/traits/MetadataResolution/死代码），对照 4 个可借用 peer（hstry/sessiongrep/Recall/memex）
- **Scope**: internal（本仓库代码 + 分支 + peer clone 逐行核验）
- **Date**: 2026-08-15
- **审查对象**: `crates/agent-session-grep-domain/`（ids.rs / thread.rs / lib.rs）、`crates/agent-session-grep-ports/src/lib.rs`（main + 分支 `session-metadata-search-08-15` / `integration-08-13-four-features-v2`）

---

## 0. 前提修正（必须先读）

任务书假设 ToolActivity 模型、ResumeClaimsStore、EmbeddingProvider、VectorIndex、MetadataResolution 在 domain/ports crate 中——**实际均不在当前代码里**：

| 类型 | 实际位置 | 证据 |
|---|---|---|
| `ToolActivity` | **只存在于设计文档** `.trellis/tasks/08-15-structured-activity-context-facets/design.md`（R1-R6 规则集 + schema v12 计划）；git 全部分支的 domain lib.rs 均无此类型 | 分支扫描 0 命中；implement.md:9-13 仍为待办 |
| `MetadataResolution` / `ProviderSessionObservation` / `SessionResumeMetadata` / `ResumeClaimsStore` / `SourceResumeClaim` / `NoResumeClaims` | 只存在于分支 `session-metadata-search-08-15` 与 `integration-08-13-four-features-v2` 的 ports lib.rs（main 分支没有） | `git show` 核验 |
| `EmbeddingProvider` / `VectorIndex` | **任何分支都不存在**，仅 `.trellis/tasks/08-15-semantic-hybrid-local-retrieval/design.md:32-37` 的设计文字 | 分支扫描 0 命中 |
| `SearchProvider` / `SearchInstant` / `SearchFilters` / `SearchQuery` / `CatalogStore::list_sessions` | 同上，只在两个分支的 ports lib.rs（main 缺失） | `git show` diff 核验 |

另一个方向：main 上已 checked-in 的 `.trellis/spec/agentsessions-ports/backend/index.md:55-63` 已描述 `SearchQuery`/`SearchFilters`/`SearchHit.occurrences`——**spec 超前于 main 代码**（这些 API 只在分支上）。合并时 spec 与代码必须同时落地。

结论：本审查对 1/2/6 三项基于 main 实际代码；对 3/4/5 三项基于**设计文档 + 分支代码**，标注为设计级/分支级发现。

---

## 1. StableId / PlacementId 派生正确性（ids.rs）— 整体健全

**结论**: BLAKE3-128（32 hex，`ids.rs:98`）+ 长度前缀 framing 防边界碰撞（`ids.rs:140-157`）+ kind 前缀域分离（`ids.rs:144`），确定性有测试锚定（`ids.rs:318-345`）。PlacementId 用独立 `plc_v1_` 前缀 + 同样 framing（`ids.rs:240-265`）。对比 hstry UUIDv5（SHA-1 122-bit）更强，无内容截断碰撞问题。

### 与 hstry `stable_message_id`（hstry-core lib.rs:45-68）对比

- hstry 把 **content 前 4096 字节**（UTF-8 边界截断，lib.rs:72-81）纳入哈希做去重键；本项目的 message id 不含内容——native id 优先（`ids.rs:117-129`），缺 native 时由 CLI 组合根用 `session + seq` 派生（`cli/main.rs:1355-1365`）。两案等价：hstry 的 client_id 优先路径 = 本项目的 native 优先。**本方案避免了 hstry 的"4096 字节后内容差异碰撞"**，但代价是派生 id 不区分内容（同 session 同 seq 两条不同消息 → 坐标校验拒绝，`domain/lib.rs:319-328`，有测试 `lib.rs:594-604`）。
- 重放幂等：两者都靠"同输入 → 同 id + upsert"去重。本项目幂等链是 native id 原样透传 + `StagedBatch` 去重，成立。

### 风险点

- **[P1] session 身份未 provider-scoped（main 现状）**：`StableId::native(IdKind::Session, sid)` 对 Claude Code 与 Codex 都用同一命名空间直接收养原生 id（CLI `main.rs:1329` 起）。跨 provider 原生 session id 相同 → 静默合并。sessiongrep 的复合 id `{provider}:{provider_session_id}`（`sessiongrep/src/util.rs:348`）+ resolve_session 0 报错/1 返回/多报 ambiguous（`sessiongrep/src/db.rs:480-505`）正是为此而生。08-14-provider-scoped-session-identity 任务已规划但**未落地 main**；16 provider wave（子任务 #2）前必须落地，否则跨 provider 碰撞无防线。分支上的 `multi_session → ambiguous` fail-closed（见 §5）只在 resume 层兜底，不改身份层。
- **[P2] native() 净化是有损变换**（`ids.rs:117-129`）：trim + 剥控制字符 + 256 字符截断——两个恶意/病态原生 id 可净化成同一 wire id。有文档说明（"lossless for all real inputs"），可接受；但跨 provider 场景下这是 §P1 之外的第二个合并通道，需 provider-scoped 前缀兜底。
- **[P2] derive() 的确定性是调用方契约**（`ids.rs:131-139` 文档），类型层面不可强制（facts 是裸字节）；`Unstable` 层（`ids.rs:76-77`）就是为违约场景设计的诚实兜底。`from_wire` 一律回退 `Unstable`（`ids.rs:187-206`）是正确选择（wire 不编码 stability，`ids.rs:177-184` 文档明确）。
- **[P3] PlacementId 不含 is_sidechain/span**（`ids.rs:240-265`）：身份只取 (session, document, message, ordinal)，sidechain/span 是"出现时的上下文属性"而非身份——validate 的坐标唯一性键同样不含这两者（`domain/lib.rs:319-328`），同坐标不同 sidechain 的 pair 会被判重拒收，一致、无碰撞。有测试锚定（`lib.rs:578-604`）。
- **[P3] 128-bit 截断 vs hstry 122-bit**：本项目更强，无风险。

### Borrow candidates

- `hstry/crates/hstry-core/src/lib.rs:45-68`（stable_message_id：client_id 优先 + 内容前缀兜底）、`lib.rs:72-81`（utf8_prefix 边界截断防 panic）——**适用于派生 session 身份**（16 provider 中无 durable session id 者的兜底键设计），MIT。
- `sessiongrep/src/util.rs:348` + `src/db.rs:480-505`（复合 id + 0/1/many 解析）——provider-scoped 身份与歧义报错的现成模式，Apache-2.0 需 NOTICE。

---

## 2. select_mainline 算法（thread.rs）— 正确，文档有偏差

**结论**: 算法本身正确且经得起推敲：时间戳预解析 map（每消息一次，`thread.rs:42,153-160`）、按消息索引出现（`thread.rs:52,163-174`，把逐跳 O(n²) 扫描降为 O(1) 查找）、只在选中分支上惰性解析父链（`thread.rs:115-127`）——分支外的歧义父永不阻塞（Major-1 回归测试 `thread.rs:820-856`）。内部消息多副本排除在叶子外（BLOCKER-1，`thread.rs:71-83,780-817`）、edged 叶子优先于 edgeless 副本（Major-2，`thread.rs:85-104,741-777`）都是正确的启发式并有回归测试锚定。环终止靠 visited 集（`thread.rs:116-124,1101-1119`）；全 sidechain 诚实回退（`thread.rs:58-60,1122-1160`）。

### 400 次 LCG 属性测试（thread.rs:723-738）的边界

- 它把生产实现与**改前参考实现**（`select_mainline_reference`，thread.rs:526-605，作为 oracle）逐字节对比——证明的是"重构等价"，**不是规范正确性**：两个实现共享 parse_instant / compare_placements 语义，共同缺陷会同时出现在两边而测不出来。
- 图生成覆盖：6 消息 / ≤3 文档 / ordinal 0-7 / 25% sidechain / 2/3 出边 / 图内父或孤儿父（`thread.rs:628-706`）。盲区：跨会话坐标、edge 指向"出现在别的文档但同会话"的父、message 数 > 文档数的压力形状。建议补充 2-3 条规范级不变式断言（如"存在非内部消息时 leaf 必非内部消息出现"——现在只靠手写测试锚定）。
- 其余边界均有手写测试：真实叶子战胜 9999 年时间戳的内部消息（`thread.rs:1163-1188`）、同文档父消歧（`thread.rs:1002-1023`）、sidechain 出现可被选为父（`thread.rs:1026-1055`）、孤儿父停走（`thread.rs:1081-1098`）、空图（`thread.rs:1238-1248`）。

### 发现的问题

- **[P2] 文档与实现不符**：模块注释称"Parent Message IDs resolve within that candidate graph"（`thread.rs:28-31`），但 `placements_by_message` 索引的是**全部出现（含 sidechain）**（`thread.rs:52`），且测试 `same_document_parent_resolution_considers_sidechain_placements`（`thread.rs:1026-1055`）明确断言 sidechain 出现可被选为父。注释应改为"在会话全部出现内解析，同文档优先"。行为正确，注释误导（曾导致我误判为 bug）。
- **[P2] 时间戳不可解析时回退原始字节序**（`thread.rs:334-337`）：`"not-a-timestamp"` 与真实时间戳比较时排序任意（确定性但反直觉：`'2' < 'n'`，垃圾串排在真时间戳之前）。两个实现一致、可复现；建议文档明示"任一侧不可解析即字节序决胜"（测试集 `thread.rs:634-644` 已覆盖但注释未说清排序后果）。
- **[P3] parse_instant 接受不可能日历日**（如 `2026-02-31`，`thread.rs:296-307` 只查 month 1-12、day 1-31）：Hinnant 公式对非法日期仍产出确定线性值，排序一致，仅"日历合法性"不保真。与测试集里 `2026-13-40T99:99:99Z`（hour>23 被拒）行为层次不同，无需修，记录即可。
- **[P3] select_full 不是源序**：排序键是消息时间戳 → 文档 id → ordinal（`thread.rs:340-352`），"Full" 是确定性上下文序而非文档序。调用方（CLI context 输出）不得假设源序。
- **[P3] MessageEdge.parent_native_id 是信息性字段**：父解析只用 `parent_message_id`（`thread.rs:176-208`），native 父 id 从未参与消歧（同文档优先是唯一 tiebreak）。它被写入 catalog payload（`cli/main.rs:1478`）不是死代码，但未来可用作"重复父消歧"的第二输入（P3 备注）。

### Borrow candidates

无。peer 中无同类确定性分支选择算法（hstry/memex 都是线性 transcript 顺序），本实现无可借用对照，也无需借。

---

## 3. ToolActivity 模型（设计级审查 — 未落地）

**前提**: 代码不在任何分支（§0）。以下审查 `08-15-structured-activity-context-facets/design.md` 的 R1-R6 规则集。

### 与 Recall 事件模型（Recall/src/adapters/events.rs）对比

- **R1 target 链**（design.md:76-96）借 Recall `target_from_value`（events.rs:94-113）的键序，两处记录在案的偏差：`url` 提到 `query` 前（Web 工具只带 url）；追加 `description`（Task 类工具）。另一处偏差是**跳过数组**：Recall 的 `command_target_from_array`（events.rs:123-139）处理 `["bash","-lc",cmd]` 三元组，本设计 fail-closed 不猜。已核实 Claude/Codex 记录确无此形状（codex provider 目前只 parse message/reasoning 记录，`provider-codex/src/lib.rs:395-425`；custom_tool_call 尚未提取），偏差合理。
- **R2 kind 推断**（design.md:98-111）:**精确大小写匹配闭集**，明确拒绝 Recall 的 lowercase containment（`infer_tool_kind` events.rs:146-164：`"bash"` 会匹配 `"notbash"`）。严格 fail-closed 方向正确。

### 发现的问题

- **[P1] R2 闭集是 Claude 向（PascalCase），未覆盖 Codex 的小写工具名**：真实 Codex CLI 工具名为小写（`bash`、`read_file`、`write_file`、`edit_file`、`grep`、`glob`、`web_search`、`web_fetch`、`task` 等）；按设计"case-sensitive；`BASH`/`read` 都是 unknown"（design.md:107），Codex（certified 首发目标，PRD Q15）的工具活动将**几乎全部落入 Unknown**，提取层形同虚设。修复：按 provider 建精确名表（Claude PascalCase 表 + Codex 小写表，各含金样 fixture 锚定），或 adapter 边界先做文档化归一再精确匹配。这正是 Recall containment 想避免的坑，偏差是故意的，但必须把第二个表补上。
- **[P2] `emit_activity` 默认 no-op**（design.md:171-175）：sink 忘记实现时活动被静默丢弃（端口层 fail-open）。过渡期可接受，但实现任务里必须要求：staging sink 至少一条缓冲测试 + 第二个 sink 出现后去默认化。`query_faceted` 默认委托 `query`（design.md:182-184）同理——fake backend 无妨，真实 adapter 忘实现会静默返回未过滤结果，SQLite adapter 必须显式 override 并有测试。
- **[P2] activity_id 含 message_id 但不含 source**（design.md:252-256）：同 (message_id, kind, actor, name, target, status) 跨源合并为一行 + 双 claim（镜像 placements，是意图）。注意 target 在截断后才入 id——**截断常量必须冻结在 adapter 契约里**（design.md:257-260 已写明持久化边界，落地时加 wire 稳定测试）。
- **[P3] R3 actor 规则依赖 caller 的 is_sidechain**（design.md:113-117）：Codex 恒 Main；16 provider 波次中每个 provider 的 sidechain 语义都需要独立证据，规则表是 provider 无关的，但证据收集按 provider 走（子任务 #2 的 evidence wave 覆盖）。

### Borrow candidates

- `Recall/src/adapters/events.rs:94-113`（target 优先级链，已按 MIT 记入设计借用声明）、`events.rs:123-139`（bash/sh/zsh 数组特判——**建议保留参考**：未来 DeepSeek Harness 等 provider 可能产出该形状，现设计 fail-closed 跳过是合理第一步，但注释里要留这个扩展点）、`events.rs:146-164`（infer_tool_kind 的 lowercase 集合——**直接用作 Codex 表的最小起点**，MIT）。
- `memex/memex-rs/src/compact/service.rs:204-245`（prune_and_merge_tool_calls）+ `:266-290`（merge_tool_calls：merged_count/files 合并参数）——设计已正确判定为 compaction-tier（摘要层）移出 extraction 范围（design.md:72-74），作为后续 follow-up 保留，MIT。

---

## 4. Trait 设计（ports lib.rs）— 最小且诚实，但能力面不足

**结论**: main 上的端口是克制的。`ProviderAdapter`（lib.rs:392-411）只留 probe+parse，discover/fingerprint 显式委托 `SourceDiscovery`（lib.rs:64-72）并有文档说明从简原因（lib.rs:388-391）——不是藏缺口。`SearchIndex`（lib.rs:170-176）只有 index/query，tombstone 由 generation/claims 在 adapter 层处理，契约自洽。`&T` blanket impl（lib.rs:181-230）是组合惯用法的良好实践，spec 有对应要求（ports spec index.md:91-92）。**main 上所有 trait 方法都是必选实现，无默认方法藏缺口**——这是加分项。

### 发现的问题

- **[P1] 无 capabilities manifest（PRD Q42 未落地）**：任务书问的"Capabilities manifest (default Parse-only) adequate?"——**main 上根本不存在 manifest**。`ProbeResult` 只带 variant_id/confidence/evidence（lib.rs:317-327），`ProviderAdapter` 无法表达"支持增量/support span/resume 能力"（PRD 要求 manifest 声明 provider_id/variant/roots/**capabilities**/maturity/license/网络权限，Q42）。现代码里唯一的 "capabilities" 是 MCP 协议样板（`cli/src/mcp.rs:162,757`）。16 provider 能力矩阵（PROVIDER-MATURITY-MATRIX.md）缺一个代码载体，子任务 #1（unified-release-contract）必须落地；在此之前"16 provider 分级宣传"（Q53）无数据支撑。这是**路线图阻塞项**，不是代码 bug。
- **[P2] `SearchHit.session_id: Option<String>`**（lib.rs:159）：Application 从 `ContextGraphStore::session_of`（返回 `Option<StableId>`，lib.rs:143-144）填充时把类型剥成裸 String——端口边界丢掉了类型（spec 明言端口要"Domain values / stable typed candidates"，spec index.md:64-67）。wire id 字符串可绕过 validate 直接进入输出。建议 `Option<StableId>`（wire 序列化不受影响，`StableId` 有 Serialize）。
- **[P2] ensure_readable 把"其他端口故障"归为 `InvariantViolation`**（lib.rs:242-259）：磁盘满等意外后端故障在协议层表现为"bug 信号"（有测试锁定分类 `lib.rs:560-571`）。文档说是"稳定分类优先于细节"，可辩护；但建议至少在分类名上区分 `backend_unexpected` 与真 invariant（P2 建议，非必须）。
- **[P3] 计划中的默认方法将引入 gap-hiding**：§3 已述 `emit_activity`/`query_faceted` 默认实现（design.md:171-184）是即将进入 ports 的默认方法——实现任务须带"默认方法不得静默丢语义"的测试约束。

### Borrow candidates

- `Recall/src/adapters/mod.rs`（ResumeCommand trait + app_command 双通道，inventory R3）——ports 层 resume 能力面参考，仅结构。
- `ctx/crates/ctx-history-capture/src/provider/mod.rs`（ProviderFidelityClaims 布尔位图，inventory S4）——capabilities manifest 的现成位图设计参考，Apache-2.0 需 NOTICE；**P1 缺口的最短实现路径**。

---

## 5. MetadataResolution 三态语义（分支代码）— 健全，强于 peer

**结论**: 三态枚举（分支 ports lib.rs:464-473，`Missing`/`Resolved(T)`/`Ambiguous`，`#[default] Missing`）比 peer 的 Optional/nullable 更诚实：
- fast-resume / sessiongrep 用 `Option<provider_session_id>` 单值，**无歧义概念**（sessiongrep 靠 `(provider, provider_session_id)` 列 + 唯一索引兜底，db.rs:40,71）。
- Recall 用 `(source, source_id)` UNIQUE 约束硬拒多值（inventory S8）。
- 本项目用三态 + `multi_session` 折叠，fail-closed 路径正确：`SourceResumeClaim::from_observation`（分支 lib.rs:524-556）对 `Resolved(v) if !multi` 双字段守卫、`pair_observed && !multi`（lib.rs:549），同一 source 多个 native session id 时任何单个 id 都不宣称权威（lib.rs:530-533 注释明确）。`NoResumeClaims` 默认实现（lib.rs:560-576）诚实返回 `resume_available:false` + 明确 reason。`resume_of` 保序 + 反 N+1 契约写进 trait 文档（lib.rs:503-509）。

### 发现的问题

- **[P2] state 字符串与枚举重复**：`SourceResumeClaim.provider_session_id_state: String`（分支 lib.rs:529,531）是 `MetadataResolution` 的未类型化平行拷贝（"missing"/"resolved"/"ambiguous"）。单一构造点（from_observation）当前保证一致，但需一条 wire 稳定测试钉死字符串（仿 domain lib.rs:685-701 的 `MessageRelation` wire 测试），否则未来手工拼串漂移。
- **[P2] pair_observed 的保证依赖 provider 提取实现**：观察"来自同一条权威记录"（分支 lib.rs:478-479 注释）是 provider 侧契约，domain/ports 无法强制。合并前必须用真实 fixture 验证 Claude/Codex 两 provider 的 session_observation 填充路径（分支的 provider crates 在审查范围外，此处标记验证义务）。
- **[P3] Ambiguous 与 Resolved 之间无"多值但全等"态**：同一 source 观察到两个**相同**值的 native session id 会被 `multi_session` 折叠成 Ambiguous——fail-closed 可辩护（保守），但值相同的重复观察本可安全 Resolved。留作 P3 观察，不急于改。

### Borrow candidates

peer 无可借用代码（三态语义比全部 peer 更细）；仅记录对比结论。若需"多值去重后仍唯一则 Resolved"的宽松语义，参考 Recall 的 (source, source_id) UNIQUE 的折叠策略（`Recall/src/db/session_store.rs`，inventory S8）。

---

## 6. 死代码 / shim / 跨分支镜像

### StagedBatch.session_native_id — 确认是写-only shim

- 声明带 deprecated 文档（application lib.rs:257-262："must not be trusted independently; new code must read report.session_native_id"）。
- 生产代码**只构造不读取**：构造点 `cli/main.rs:1247,1249,1917,1926`；唯一读取是测试断言（application lib.rs:2374）；生产读路径走 `staged.report.session_native_id`（cli/main.rs:1329）。
- **[P2] 建议清理**：删字段 + 改 2 处 CLI 构造点 + 1 条测试（~10 行）。被文档化不是留下的理由——写-only 字段会让未来读者困惑（我就是被它引来的）。若坚持留到 CLI 重构，至少在字段上加 `#[doc(hidden)]` 防新代码引用。

### 跨分支字节一致镜像 — 已核实，合并风险中等

- `session-metadata-search-08-15` 与 `integration-08-13-four-features-v2` 的 **ports lib.rs 与 application lib.rs 均 md5 字节一致**（`36f7d3b6…` / `8f922092…`）——任务书"mirrored byte-identical"属实，且不止 resume claim 类型（SearchProvider/SearchFilters/SearchQuery/list_sessions 也一并镜像）。
- **合并含义**：
  1. main 的 ports lib.rs 落后两个分支一个完整功能面（§0 表格）。规范 index.md 已超前描述这些 API。**先合并 session-metadata-search-08-15（作为单一血统），再让 08-15 各 worktree 在其上 rebase**，避免第二份镜像被当新代码重放。
  2. structured-activity worktree 计划在**同一批文件**上再加码（ports: SearchFacets/emit_activity/query_faceted/extract/infer 助手；application: StagedBatch.activities/AppRequest facets）——hunk 与 metadata-search 的添加区相邻（都在 SearchIndex/CanonicalEventSink/ParseReport 周边），冲突概率中等；schema 版本已协调（v7→v12 依赖 v7 表、v11 之后才申请 12，design.md:225-228，"runs cleanly on any catalog at v7..v11"），**迁移层 merge-safe，代码层要按序合并**。
  3. main 的 spec index.md（工作区已改）与分支代码是一对，合并时 spec/代码同步落地，避免 spec 再超前。

### 其他

- `MessageEdge.parent_native_id`：非死代码（写入 catalog payload，cli/main.rs:1478），但 select_mainline 不读（§2）。P3 备注。
- provider-codex 恒发 `parent_native_id: None`（provider-codex lib.rs:406，注释"threading 由上层推断"）→ Codex 会话无边，select_mainline 退化为 max-timestamp 单点选择；Claude Code 有 parentUuid 链（provider-claude lib.rs:496）。这是两 provider 的能力差，不属于 domain 缺陷，但 16 provider 波次要逐个确认。
- 边界注意：CLI 组合根对空 native_id 消息用派生 id（main.rs:1355-1360），而其父边用 `StableId::native` 构造（main.rs:1375-1379）——**子派生 id 与父 native id 永不相等**，当父消息自身 native id 为空时链会静默断在孤儿处（domain 设计允许孤儿父，`domain/lib.rs:225-227`，行为确定，但链被截断）。P3 备注，16 provider 中无 durable message id 者（如部分 DeepSeek Harness 形态）会踩中。

---

## Borrow candidates 汇总（含 peer 精确定位）

| 用途 | 候选 | Peer 位置 | License | 风险 |
|---|---|---|---|---|
| 派生 session 身份兜底键（client_id 优先 + 内容前缀） | hstry `stable_message_id` + `utf8_prefix` | `hstry/crates/hstry-core/src/lib.rs:45-68`、`:72-81` | MIT | 无；照搬含测试 |
| provider-scoped 复合 id + 0/1/many 解析 | sessiongrep id 构造 + `resolve_session` | `sessiongrep/src/util.rs:348`、`src/db.rs:480-505` | Apache-2.0 | 需 NOTICE；模式直接套 |
| target 优先级链（R1 已借） | Recall `target_from_value` | `Recall/src/adapters/events.rs:94-113` | MIT | 已记录偏差（url 提前/加 description/不接数组） |
| 数组形状特判（bash -lc 三元组） | Recall `command_target_from_array` | `Recall/src/adapters/events.rs:123-139` | MIT | 现 fail-closed 跳过，留扩展点 |
| **Codex 小写工具名集（R2 P1 缺口修复的最小起点）** | Recall `infer_tool_kind` 的 lowercase 集合 | `Recall/src/adapters/events.rs:146-164` | MIT | 需按 Codex 实际工具名核验（golden fixture 锚定） |
| compaction-tier 工具调用合并（后续 follow-up） | memex `prune_and_merge_tool_calls` / `merge_tool_calls` | `memex/memex-rs/src/compact/service.rs:204-245`、`:266-290` | MIT | 阈值 3 做成常量（inventory 已注） |
| capabilities manifest 位图（P1 缺口） | ctx `ProviderFidelityClaims` | `ctx/crates/ctx-history-capture/src/provider/mod.rs`（inventory S4，行号未逐行核验） | Apache-2.0 | 仅结构；搬运前再读原文 |

## Caveats / Not Found

- **未找到**：`EmbeddingProvider`/`VectorIndex` 任何分支的代码（纯设计）；`ToolActivity` 任何分支的代码（纯设计）；main 上任何 capabilities manifest（不存在）。
- peer 中 `ctx` 条目行号未逐行核验（基于 inventory S4 + 文件存在确认），搬运前需重读。
- 本审查为只读；所有分支/peer 证据经 git show / 直接读取核验，未运行测试（cargo 全量测试不在本任务范围）。
- 审查范围限定 domain/ports（+CLI 组合根边界），provider crates 内部（claude/codex 提取实现）、adapters-sqlite 未逐行审查，仅标记验证义务。
