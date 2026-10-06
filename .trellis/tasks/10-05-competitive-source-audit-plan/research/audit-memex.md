# Research: memex 竞品源码审计（T1 全文精读凭证与静态审计）

- Query: 对 memex（commit `9a88a5cb`）分别核查 Lite 与完整服务的能力边界、检索真伪与降级/失败行为、MCP 工具面与 Agent 注入契约，并给出与 ASG 的对照结论。
- Scope: internal；仅 `Github_src/memex` 指定本地快照的静态源码审读；未执行 build/test/install/网络/git 写操作。
- Date: 2026-10-06
- Recorded snapshot: `9a88a5cbeccd14ef2a526729e1815b8df0793414`；只读读取 `.git/HEAD` 与 loose ref `refs/heads/main` 校验一致（未运行 git 命令）。
- Status: T1 全文精读完成；本报告与 `coverage-memex.json` 同批交付。结论仅代表该本地快照。

## 0. 阅读凭证（摘要）

| 文件 | 行数 | 状态 | 阅读范围（1-based inclusive） |
|---|---:|---|---|
| `memex-rs/src/mcp/mod.rs` | 2556 | full | 1-150,151-300,301-450,451-600,601-750,751-900,901-1050,1051-1200,1201-1350,1351-1500,1501-1650,1651-1800,1801-1950,1951-2100,2101-2250,2251-2400,2401-2556 |
| `memex-rs/src/search/mod.rs` | 1136 | full | 1-150,151-300,301-450,451-600,601-750,751-900,901-1050,1051-1136 |
| `memex-rs/src/inject/mod.rs` | 990 | full | 1-150,151-300,301-450,451-600,601-750,751-900,901-990 |
| `memex-rs/src/db_reader.rs` | 450 | full | 1-150,151-300,301-450 |
| `memex-rs/src/domain/mod.rs` | 418 | full | 1-150,151-300,301-418 |
| `memex-lite/src/main.rs` | 381 | full | 1-150,151-300,301-381 |
| `memex-lite/src/output.rs` | 444 | full | 1-150,151-300,301-444 |
| `memex-rs/src/knowledge/matcher.rs` | 368 | full（预算内可选） | 1-150,151-300,301-368 |
| `memex-rs/src/vector/mod.rs` | 390 | full（预算内可选） | 1-150,151-300,301-390 |

- 合计 7133 行 / 9 文件全部读完；每段 ≤150 行、逐段回显编号原文并记录精确行区间；SHA256 仅用于确认内容未变，不计为阅读。
- 辅助上下文读取单列于 `coverage-memex.json` 的 `auxiliary_reads`：`memex-lite/src/search.rs`（304 全文）、`memex-rs/src/server/search.rs`（95 全文）、`README.md`（105 全文）、`memex-lite/Cargo.toml`（41）、`memex-rs/Cargo.toml`（99）、`memex-rs/src/compact/mod.rs`（35）、`memex-rs/src/indexer/mod.rs`（仅 1-150，**partial**，missing 151-585）。
- 未读＝未引用：凡本报告未给锚点的行为均不构成结论；源码未改动，9 个 T1 文件初始/最终哈希一致。

## 1. 能力边界：Lite 与完整服务必须分开评价

Lite 与完整服务是两个独立 crate/workspace，只有一个薄 git 依赖交集（解析/DB 客户端），不得混写成一个能力标签。

| 维度 | Lite（`memex-lite`，包名 `memex` 0.1.0，独立 workspace） | 完整服务（`memex-rs` 0.0.4-platform.3，lib `memex`） |
|---|---|---|
| 索引 | **无索引**。每次查询现场枚举会话并逐个解析（`memex-lite/src/search.rs:99-155`；`memex-lite/src/main.rs:163-187`） | 读：`DbReader` 包装 git 依赖 `ai-cli-session-db`（`memex-rs/src/db_reader.rs:1-52`），正文写入由外部 Agent 负责（:1-4）；写：向量索引（`memex-rs/src/indexer/mod.rs:1-143`，LanceDB + `vector_indexed` 状态回写）+ Compact 层索引（`memex-rs/src/compact/mod.rs:11-35`） |
| 检索 | 正则（`regex`）直接匹配解析后的 user/assistant 消息；上下文截 200 字符、摘要 100 字符（`memex-lite/src/search.rs:101-104,173-207,234-245,255-297`）；时间过滤用**文件 mtime**（:130-140） | FTS（外部 db crate）+ 真 LanceDB 向量 + RRF（`memex-rs/src/search/mod.rs:1-30,236-273,400-559`）；L0-L3 多层 + L4 knowledge 并入 L3（`memex-rs/src/mcp/mod.rs:563-654`） |
| 注入 | **无**（纯 CLI：search/list/view/sources；`memex-lite/src/main.rs:26-112`） | Claude Code Hook：SessionStart / UserPromptSubmit（`memex-rs/src/inject/mod.rs:1-12,117-172`）；输出 `hookSpecificOutput.additionalContext`（:31-72） |
| compact | **无** | L1 Observations/L2 Talk/L3 Session 的 LLM 压缩，原文始终保留、层可重做（`memex-rs/src/compact/mod.rs:1-9`） |
| 网络 | **运行时不联网**（依赖清单 `memex-lite/Cargo.toml:13-33`；唯一 git 依赖是构建期 `ai-cli-session-collector`，:24） | HTTP 服务/MCP（axum，`memex-rs/Cargo.toml:28-35`）、LLM/embedding provider（reqwest 指向 Ollama/OpenAI 形态，:40-41；`memex-rs/src/llm/providers/`）、远程 sync 与 peers（`memex-rs/src/mcp/mod.rs:425-464`） |

**结论句：** Lite 是“无索引现场 grep 式本地搜索 CLI”；完整服务是“本地多层记忆平台（FTS+向量+RRF、LLM compact、Hook 注入、HTTP/MCP、含远程与 peers 数据面）”。旧材料中任何 “Rust CLI hybrid” 的单标签写法都应作废。

## 2. 检索实现核查：FTS/向量/RRF 是否真实、降级是否静默、失败行为

1. **FTS 真实但属外部依赖**：`HybridSearchService` 经 `DbReader` 委托 `ai_cli_session_db` 的 `search_fts_full*`（`memex-rs/src/search/mod.rs:316-339`；`memex-rs/src/db_reader.rs:239-300`）。FTS 引擎内部（分词/CJK/rank 语义）在外部 git crate，本轮未审计。
2. **向量真实**：LanceDB 0.15 表 `embeddings`，1024 维（bge-m3 默认），存 message_id/chunk_index/content/_distance，支持 Cosine/L2/Dot 映射（`memex-rs/src/vector/mod.rs:18-25,47-70,196-269`；`memex-rs/src/Cargo.toml:46-49`）。注意：表未创建时 `search_with_distance_type` 直接 `Ok(vec![])`（`memex-rs/src/vector/mod.rs:215-218`）。
3. **RRF 真实**：K=60，按 rank 求和（`1/(60+rank)`），以 message_id（raw）/source_id（compact）聚合，并保留 `sources.fts/vector`、`fts_rank`、`vector_distance` 归因字段（`memex-rs/src/search/mod.rs:29-30,455-559,873-966`）。局限：单测只验证标量公式（:1127-1135），未验证跨源融合/去重行为。
4. **降级矩阵（静默度是主要问题）**：

| 触发 | 行为 | 用户可见性 |
|---|---|---|
| `order_by=time_desc/time_asc` | 强制 `effective_mode=Fts`，跳过 RRF、丢弃向量结果（`search/mod.rs:287-296,366-394`） | 仅 `tracing::info`；`HybridSearchResult` 无 effective/degraded 字段（:51-94） |
| FTS 出错 | `warn!` 后继续（只有向量结果）；两者皆空则 `Ok([])`（:335-364） | 无区分字段 |
| 向量出错 | `warn!` 后继续（:348-355） | 无区分字段 |
| embedding/vector 未配置 | `debug!` 后按纯 FTS 走（:356-358） | 无区分字段 |
| MCP L3/L2 缺 compact_db | sessions→talks→raw 逐级回退（`mcp/mod.rs:571-575,712-730`） | **响应 level 仍写请求层**（:772-777） |
| server 端 | 无 embedding provider，向量依赖客户端推数据；REST limit≤100（`memex-rs/src/server/search.rs:1-4,59`） | 由文档性注释说明 |

5. **失败行为对比**：MCP 的 raw 检索直接 `map_err` 成工具错误（`mcp/mod.rs:801-813`）——硬失败可见；但 `HybridSearchService`（REST /api/search 等消费方）把上述失败折叠进“空结果”，`server/search.rs:68-88` 只在返回 `Err` 时给 500。即：**“检索坏了”与“没命中”在部分入口不可区分**（详见 MX-03）。

## 3. MCP 工具面与 Agent 注入契约

- **工具面**：HTTP JSON-RPC 2.0；5 个工具 `search_history` / `get_session` / `get_recent_sessions` / `list_projects` / `extract_knowledge`（`mcp/mod.rs:173-248,354-364`）；仅 `tools` 能力，无 resources/prompts；`initialize` 固定 `protocolVersion=2024-11-05` 且不读客户端参数（:306-316）；notifications 直接忽略（:257-259）。`extract_knowledge` 是 L4 知识抽取的后台任务启动器（:1253-1384）。
- **检索→证据锚（本产品最强处）**：raw 结果带 `at`=message_id（:850-866），并被设计为可直接用于 `get_session(at|around)`；集成测试验证 round-trip 与“找不到就报错、不静默 fallback”（:2050-2134）。这是把“搜索命中”连接到“原始消息证据”的真实机制。
- **注入契约**：SessionStart 按 sources 优先级 fallback（sessions→talks→observations→messages，`inject/mod.rs:186-275`）；UserPromptSubmit 走向量 combine/fallback（:164-172,327-430）。注入项文本只有 **8 字符会话前缀 + 层标签 + 相对时间**（:670-676,711-717,845-852）；**没有** message id、文件/行号、生成模型/生成时间；L0 原文与 L1-L3 摘要同栏混排；单条消息体超 500 字符仅以省略号截断（:802-806）。
- **预算**：`max_items` + `max_tokens`；token 估算为 chars/4；超预算直接 `break`，无截断标记或续读位置（:196-197,296-314,622-641）。MCP `get_session` 在有效页 >5 条时同样 500 字符截断（`mcp/mod.rs:1004-1008,1067-1071`），`at` 模式全文不截断（:965-987）。
- **隐私边界**：注入仅在传入 project_path 时按项目过滤（`inject/mod.rs:190-204`），且项目匹配是 `ends_with` 双向启发式（:175-184）；数据面存在 remote sync（`mcp/mod.rs:425-446,881-894`）与 `peers.db`（:448-464，`memex pull` 产物）；`source=local` 在本地结果不足时也会补查远端（:520-558）。README 的 “Local storage”（`README.md:19`）只覆盖默认本地模式。

## 4. Findings

### MX-01 · P2 · 注入记忆没有逐条证据锚点，摘要与原文不可分辨

- **证据：** `memex-rs/src/inject/mod.rs:670-676,711-717`（头部只有 8 字符会话 ID）、`:802-806`（消息体 500 字符截断）、`:845-852`（向量结果仅层标签+短会话 ID）、`:624-625`（"Matched based on your current query" 不含命中依据）、`:643-645`（仅提示 `get_session(session_id)`）。
- **可达：** agent 收到注入后无法判断某条是 L0 原文引用还是 LLM 生成的 L1-L3 摘要，也无 message_id/时间戳/文件可核验；被截断条目不告知其余部分在哪里。
- **反证/界限：** 层标签确实存在（`inject/mod.rs:284-290,837-843`）；MCP 的 `search_history(at)`→`get_session(around)` 链路本身可逐条定位（`mcp/mod.rs:850-866,2050-2134`）——缺的是把该能力接进注入格式，而非系统没有证据锚。
- **ASG 教训：** 注入契约 = “可回到证据的引用 + 生成物标注”：每条注入项至少带 session/placement 引用、层、生成 vs 原文、生成时间；摘要与原文必须分栏。

### MX-02 · P2 · 时间排序静默降级 FTS-only

- **证据：** `memex-rs/src/search/mod.rs:287-296`（order_by≠Score → effective_mode=Fts，仅 info 日志）、`:366-394`（跳过 RRF，直接以 FTS 结果集充当融合产物，向量命中被丢弃）；`HybridSearchResult` 无降级字段（:51-94）。
- **可达：** hybrid+time_desc 的调用方拿到“正常形状”的结果；仅在向量召回中出现的命中静默消失；REST/MCP 响应无 effective_mode。
- **反证/界限：** 选项注释与 MCP 工具描述均已声明该降级（`search/mod.rs:137`；`mcp/mod.rs:186-188`），属有意设计；默认 `order_by=Score` 路径是真 hybrid。
- **ASG 教训：** 降级必须进响应契约（effective_mode/degraded_reason）；或为时间排序定义“对融合结果排序”的新语义，而不是换检索器。

### MX-03 · P2 · 混合检索失败与“零命中”不可区分

- **证据：** `memex-rs/src/search/mod.rs:335-338`（FTS Err→warn）、`:348-354`（向量 Err→warn）、`:356-358`（缺组件→debug）、`:362-364`（皆空→`Ok([])`）；`memex-rs/src/vector/mod.rs:215-218`（表未建→`Ok([])`）；对比 `mcp/mod.rs:801-813`（MCP raw 路径把 FTS Err 转工具错误）。
- **可达：** 消费 `HybridSearchService` 的入口（含 `server/search.rs:68-88` 的 /api/search）在检索故障时可能 200+空数组；agent 将“检索坏了”读成“没有记忆”。
- **反证/界限：** MCP raw 查询的硬错误会显式返回（:801-813）；compact 级错误在 MCP 多数 `map_err`（:579-582,722-725）。问题集中在融合层的降级语义，而非所有路径。
- **ASG 教训：** 空结果只能来自成功执行的检索；响应需 status=ok|degraded|error 与来源可用性位。

### MX-04 · P2 · MCP 回退到低层后 `level` 标记不更新

- **证据：** `memex-rs/src/mcp/mod.rs:571-575`（无 compact→sessions 直接返回 talks）、`:712-730`（talks 无数据→raw 消息）、`:772-777`（响应 `level` 仍写 "talks"，即使内容是 raw）。
- **可达：** 按 `level` 字段理解数据粒度的客户端会把 L0 原文当 L2 摘要消费；对 L3 的调用也可能收到 L2 数据。
- **反证/界限：** 回退链是有意的可用性设计；条目字段形状不同（summary/keyPoints vs role/snippet/at），细心调用方可分辨；MCP 工具描述提示 level 语义（:186-188）。
- **ASG 教训：** `effective_level`/granularity 必须显式返回；回退是数据语义变化，不能当作实现细节。

### MX-05 · P2 · 注入预算与截断：硬 break + 静默 500 字符

- **证据：** `memex-rs/src/inject/mod.rs:296-314,629-641`（下一条超 `max_tokens*4` 即 break，后续条目全部丢弃；无 truncated/omitted 字段；estimated=chars/4）、`:802-806`（消息 500 字符截断）；`mcp/mod.rs:1004-1008,1067-1071`（大页 500 字符截断）。
- **可达：** 头部一条长摘要可吃掉预算，导致其后更旧但在预算内的条目全部不注入；agent 无从得知有截断，也无续读位置。
- **反证/界限：** chars/4 在注释中明示为粗略估计（:51-52）；`max_items` 限制条目数（:196）；`at` 模式提供不截断全文（`mcp/mod.rs:965-987`）。
- **ASG 教训：** 预算应为条目级装填 + omitted 计数 + 可续读引用；截断带原文字符区间，而不是省略号。

### MX-06 · P3 · 注入向量管线的 time_decay 不影响输出；Dot 复用 cosine 公式

- **证据：** `memex-rs/src/inject/mod.rs:547-601`：先按 similarity 过滤、时间窗过滤、乘 decay 改 `score`，最后按 `distance` 排序；`score` 之后不参与排序/格式化（:607-653,857-864）；`:554-568` 对 Cosine|Dot 共用 `1-distance/2` 且注释自认 cosine 区间；测试仅覆盖公式（:966-977）。
- **可达：** 配置 `time_decay=true` 不产生“新近优先”效果；Dot 距离若不在 [0,2]，阈值过滤会失真。
- **反证/界限：** `time_window_days` 过滤真实生效；RRF 路径不受影响；LanceDB Dot 距离定义未在本轮核对，故只报“静态不一致 + 配置无效”，不宣称必然错误。
- **ASG 教训：** 衰减要并入真实排序键并有可观测测试；距离→相似度换算按距离类型定义。

### MX-07 · P3 · MCP 协议面窄、注释漂移

- **证据：** `memex-rs/src/mcp/mod.rs:3-7` 注释称 4 个工具，实际 `get_tools()` 为 5（:173-248），测试写 5（:1819-1821）；`initialize` 固定版本（:306-316）；无 resources/prompts；notification 丢弃（:257-259）；GET 简化（:265-277）。
- **可达：** 以注释为准会漏 `extract_knowledge`；新协议客户端的能力协商形同虚设（仍可用）。
- **反证/界限：** 工具 schema 描述详细；2024-11-05 为稳定协议版本；未做真实客户端互通测试。
- **ASG 教训：** 能力清单以运行时函数为唯一真源并配契约测试；协议版本协商至少回显兼容集合。

### MX-08 · P3 · Lite 检索语义边界：无索引、mtime 过滤、仅正文消息

- **证据：** `memex-lite/src/search.rs:99-155`（每查询枚举全部会话并解析）、`:130-140`（时间过滤用 file_mtime 而非消息时间戳）、`:173-207`（仅 user/assistant，Tool 跳过）、`:234-245`（上下文每条 200 字符）、`:255-297`（摘要 100 字符窗口）；`memex-lite/src/main.rs:339-361`（days/since/until 只落到文件级时间段）。
- **可达：** 大库每次搜索 O(会话数×解析)；时间过滤语义与完整服务的消息级时间不同；"hybrid/semantic" 类表述不适用于 Lite。
- **反证/界限：** 定位即 zero-dependency 现场 grep（`README.md:32-41`；`search.rs:1-3` “无需数据库”），是清晰取舍；正则搜索本身真实可用。
- **ASG 教训：** 能力标签绑定实现语义（regex grep ≠ FTS ≠ 语义检索）；时间过滤注明作用于文件还是消息。

### MX-09 · P3 · 隐私/出网边界需显式契约（remote、peers、路径启发式）

- **证据：** `memex-rs/src/mcp/mod.rs:425-446`（source=remote 直查 sync server）、`:448-464`（source=peers 读 peers.db）、`:520-558`（**source=local 本地不足时也补查远端**）、`:881-894`（get_session remote）；`inject/mod.rs:175-184`（项目匹配 ends_with 双向，可能把父/子目录项目都命中，用于注入范围）；`README.md:19` “Local storage”。
- **可达：** 默认参数下 search_history 可能把查询发给已配置的 sync server；peers.db 中他人会话可被搜到；注入项目过滤的启发式可能选错项目范围。
- **反证/界限：** remote 需显式配置 sync_server/sync_api_key（:441-444）；peers 需显式 source 或已 pull 数据；无“未配置即出网”的路径；local→remote 仅补差量（:524-529）。
- **ASG 教训：** 响应/注入头标注数据面（本地/远端/他人）；默认只读本地；出网或跨项目注入均需显式开关与审计字段。

### MX-10 · P3 · “只读”DB 封装实际暴露写入方法

- **证据：** `memex-rs/src/db_reader.rs:1-4,15-22` 声明只读（“写入由 Agent 统一负责”），但 `:366-416` 暴露 `mark_messages_indexed`、`mark_message_index_failed*`、`reset_failed_indexed_messages`、`update_sessions_project_id`、`delete_project`、`deduplicate_projects` 等 `db.write()` 操作；日志自称 "read-only mode"（:42-45）。
- **可达：** 维护者按注释理解写者边界会出错；`vector_indexed` 状态确实由 memex-rs 自己回写（`indexer/mod.rs:114-143`）。
- **反证/界限：** 写范围限于向量索引 bookkeeping 与 admin 维护；正文写入确由外部 Agent 负责；连接层是否强制只读取决于外部 `ai-cli-session-db`（未审计）。
- **ASG 教训：** 契约按真实施加声明（“正文只读 + 索引状态可写”），注释与 API 面一致。

### MX-11 · P3 · 多 provider 产品却把 DTO 的 source 硬编码为 claude

- **证据：** `memex-rs/src/domain/mod.rs:194,210`（Project::to_dto 固定 `source="claude"`）、`:244-246`（Session::to_dto 固定 source=claude、channel=code、status=completed）、`:370`（`From<DbSession>` 同样固定 claude）；`README.md:15` 声称四 CLI 同库。
- **可达：** 若 REST 走这些 DTO（domain 被 `api/mod.rs`、`lib.rs` 引用），Codex/Gemini 会话会被标成 claude。
- **反证/界限：** MCP 工具面自建 JSON 并带真实 source（`mcp/mod.rs:1213-1218`），不受影响；本轮未读 `api/mod.rs`，不宣称线上错误，仅静态事实与风险。
- **ASG 教训：** source/channel/status 是证据元数据，禁止占位常量默认；多 provider DTO round-trip 纳入契约测试。

## 5. 与 ASG 对照

**memex 强在哪（相对 ASG 当前公开能力）：**
1. 完整服务已把“多层记忆”做成产品：L0 原文常驻 + L1-L3 LLM 摘要可重建 + L4 knowledge 聚类（`compact/mod.rs:1-9`；`mcp/mod.rs:563-654`）。
2. 检索结果天然带归因：`sources.fts/vector`、`fts_rank`、`vector_distance`（`search/mod.rs:51-94,455-559`）——RRF 融合过程可解释。
3. “搜索命中→定位原文”做成闭环：`at` 锚 + `around` 上下文 + 测试证明 round-trip（`mcp/mod.rs:2050-2134`）。

**memex 弱在哪：**
1. 注入的记忆**丢锚**：8 字符会话前缀 + 摘要/原文混排 + 静默截断（MX-01/MX-05）——与 ASG “稳定身份/可追溯”方向相反。
2. 降级不透明：时间排序换检索器、回退后 level 不更新、故障折叠为空结果（MX-02/MX-03/MX-04）。
3. 多 provider 元数据在部分 DTO 上失真（MX-11），隐私/出网边界未进入响应契约（MX-09）。

**ASG 该学：**
- 层级化记忆的**存储形态**：原文层不可变、派生层可重建、层间可回溯（但要给每个派生品挂上游引用）；
- `at`→`around` 这类“可回到证据”的渐进披露契约，把它作为注入项的最小要求；
- RRF 的 per-source 归因字段作为融合透明度的先例。

**ASG 不该学：**
- 把 LLM 摘要与原文丢进同一无类型文本块、无逐条出处（MX-01）；
- 任何“换检索器/降层/失败→空结果”的静默降级（MX-02/03/04）；
- chars/4 估算 + 超限硬 break + 省略号截断的预算契约（MX-05）；
- 热路径逐命中查库（`search/mod.rs:427-434`；`inject/mod.rs:657-666,698-707,779-787,820-829`；`mcp/mod.rs:1537-1545`）；
- 把“本地存储”营销语留给远端/peers 数据面去兜底（MX-09）。

**重点（注入/记忆 vs 证据可追溯）结论：** memex 已经拥有“可追溯”的所有底层零件（message_id、at、层标签、RRF 归因），但在**注入**这一最靠近 agent 的界面把零件全部丢弃了。ASG 的方向应是：注入 = 引用（可回到原文、带稳定 ID 与区间）+ 标注（生成物/摘要层/时间/预算），而不是把整段自然语言记忆当作不可核验的上下文倾倒。

## 6. Caveats / Not Found

- 未读模块：`memex-rs/src/api/mod.rs`（Hook/REST 接线、各端点对 DTO 的实际消费）、`rag/mod.rs`、`llm/*`（provider 行为）、`compact/{service,queue,indexer,db}.rs`、`pull.rs`、`search/remote.rs`、`server/ingest.rs`、`server/register.rs`、`auth/*`、`backup/*`、`archive/*`、`web/*`；`indexer/mod.rs` 仅 1-150（partial）。
- 外部依赖未审计：`ai-cli-session-collector`、`ai-cli-session-db`（均为 git 依赖）内部 FTS 语义、`resolve_session_id` 前缀歧义行为、连接是否强制只读、sync 推送实现，均由本轮范围外决定。
- 未执行 build/test/安装/网络/git 写；无运行时证据；未验证 release 产物、Docker、跨平台与上游最新版本。
- 目录排除：`.git/`（仅只读 HEAD/ref 校验）、`.codegraph/`、构建缓存；未审其内容。
- T2 机械账本 `sweep-memex.json`（130 文件、探针统计）仅用于过滤查询与文件清单核对，不整读、不替代本报告阅读凭证。

## 7. 已读上下文（非 memex 事实）

- `research/coverage-sessiongrep.json`、`research/sessiongrep-full-audit.md`：格式基准（未修改）。
- `research/competitor-memory-mcp.md:30-39`：memex 旧结论，仅作待验证假设；本报告已用 T1 全文逐条核实/证伪。
- `.trellis/spec/agentsessions-domain/backend/index.md:1-138`、`.trellis/spec/agentsessions-provider-claude/backend/index.md:1-94`：ASG 对照参考（稳定身份/placement/只读解析契约），不代表对 ASG 产品的重新审计。
- 快照校验：`.git/HEAD` + `refs/heads/main` 只读结果为 `9a88a5cbeccd14ef2a526729e1815b8df0793414`，与任务记录一致；未运行 git 命令。

**Language**: 中文（锚点与代码标识保留原文）。