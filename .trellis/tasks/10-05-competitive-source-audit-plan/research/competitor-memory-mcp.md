# 记忆 / MCP / 多层上下文竞品对照

本组仅对当前本地快照做静态源码核对，没有执行竞品或联网读取用户会话。项目全量清点见 project-inventory.json；下列入口/存储/查询/输出链路已经深读，但不冒称整仓逐行读完。旧 deep-read 报告未作为事实依据。

## AgentRecall — 3c7bfa5，Electron + TypeScript + Node SQLite

**不是 ASG 旧表写的 Python CLI。**`package.json` 的 electron-vite / TypeScript / MCP bin 和 `src/core/session-store.ts:1-55` 的 node:sqlite 是直接证据。

- `src/core/session-store.ts:305-310`：对外 search/session page 统一委托 SessionsStore。
- `src/core/store/sessions.ts:461-510,664-699`：标题、收藏、置顶、隐藏、打开/恢复记录、tag 与 session-key 迁移组成真正的会话管理产品，而不只是把全文塞进搜索框。
- `src/core/store/sessions.ts:1058-1092`：FTS 匹配 + metadata/text fallback、hydrate、smart score、hasMore；结果面向 Session，而不是要求用户自己从实体字典拼页面。
- `src/core/store/sessions.ts:1389-1409`：有 query 时将符合 metadata filters 的候选全部载入应用层。查询体量大时有扫描/排序成本；这一点不能照抄为 ASG 的规模方案。
- `bin/agent-recall-mcp.mjs:82-115`：MCP 是独立 SQL 查询实现，FTS ORDER BY rank；和 desktop smartScore 不是同一条搜索函数。接口一致性不能只凭“共用 DB”判断。
- `package.json` 的 postinstall 指向 statusline 安装器，说明它还管理 agent 集成；本轮没有执行安装器，不声称安装无副作用。

**对 ASG 的压力**：结果要成为可管理、可继续工作的会话，不是只有 wire-id。**不要照抄**：将桌面查询全量 hydrate 或让另一个入口自行写第二套 SQL 排名。ASG 的共同 Application 契约值得保留。

## Recall — 22625bf，Rust CLI/TUI、FTS + sqlite-vec 检索、显式 handoff

- `src/db/search.rs:76-105`：lexical + optional embedding，RRF 合并后批量加载 Session，清楚面向“找哪场会话”。
- `src/db/search.rs:156-189`：sqlite-vec MATCH/k 查询、message→session 聚合，filters 参与 SQL；但 k 选择与过滤顺序应经引擎语义和压力测试验证，不能仅凭 SQL 长得对就宣布无漏召回。
- `src/db/search.rs:219-257`：source/time/directory/repo 过滤集中装配。
- `src/db/search.rs:263-292`：RRF 及 MatchSource（Fts/Vector/Hybrid）来源分型；末尾仅按浮点分数排序，等分序稳定性需专项检查，未动态复现。
- `src/db/search.rs:295-308`：FTS 输入清洗后 OR 查询；这和 ASG 的字面词项/CJK bigram 语义不同，不能把相同 query 字符串当成同等检索任务。
- **完整审读** `src/handoff.rs:1-112`：4 个 target，`build_prompt` 明确写“handoff, not a native resume”，交接的是 plain transcript；command 使用 program/args，含对应测试。此处没有 ASG 等价的 token/byte budget 契约，应同时评价方便与边界。
- `extensions/recall-reflect/src/main.rs:39-119`：project / personal scope 与 git-root 默认上下文；属于扩展入口，不能算核心必备功能。

**对 ASG 的压力**：给用户明确下一步，而不是输出 pack 后让人猜该如何使用。**应保留的 ASG 优势**：预算、source 证据与 native resume / handoff 的边界。**不该跟风**：为了追齐扩展数量引入额外“反思”产品线。

## memex — 9a88a5c，必须区分 Lite 与完整服务

- `memex-lite/src/search.rs:99-155,159-215`：现场枚举 Session，Regex 搜索 user/assistant，返回前后文；不是持久 FTS/hybrid 引擎。与完整服务不可混写一个“Rust CLI hybrid”标签。
- `memex-rs/src/api/mod.rs:554-628`：完整服务有 mode、raw/observations/talks/sessions/all、date/project/order 参数。
- `memex-rs/src/search/mod.rs:236-273`：原文与 compact 层分路，All 合并；多层上下文已经在实现里，不是 ASG 独占的新想法。
- `memex-rs/src/search/mod.rs:287-337`：时间排序降为 FTS-only，FTS 错误记录 warning 后继续；effective mode 的用户可见性不能靠日志代替。
- `memex-rs/src/search/mod.rs:401-449`：真正 EmbeddingProvider 调用，vector hit hydrate 时按命中逐个取 session/project，存在 N+1 查询结构（静态事实，未测其实际耗时）。
- `memex-rs/src/server/search.rs:1-4,39-95`：服务端本身没有 embedding provider，依赖客户端推送向量；API 限制 limit<=100。产品形态与依赖不同，不能说 ASG 没 RAG 就落后。

**应借鉴**：层次化上下文和“原文 vs 压缩表示”的明晰入口。**不要照抄**：RAG/远程/压缩层全家桶、隐藏降级或 per-hit 数据库往返。原文证据与生成摘要必须分栏，不能把摘要当原始证据。

## claude-historian-mcp — b627cdf，TypeScript/Node MCP

**不是 Python MCP。**实际入口/实现是 `src/index.ts`、`src/search.ts`、`dxt/server.js`。

- `dxt/server.js:40-213`：task-oriented 工具，如 search_conversations、find_file_context、find_similar_queries、get_error_solutions、list_recent_sessions、extract_compact_summary、find_tool_patterns。
- `src/search.ts:121-158`：缓存 search response；catch 返回空 messages/0 total。这会模糊“没有结果”和“搜索故障”，ASG 不应照抄。
- `src/search.ts:160-189`：query intent / semanticBoosts 是关键词规则，不是神经语义模型；不能凭函数名 semantic 就给它记“真实语义”。
- `src/search.ts:192-215`：query.length < 3 直接空结果，对两个汉字的查询是明确限制（JS UTF-16 code unit 长度；emoji 不能据此泛化）。
- `src/search.ts:223-276`：全项目候选、规则加权、内容长度门槛与非空兜底；质控阈值并不自动证明优质结果。
- `src/search.ts:95-107`：关键字探测直接 readFile 全文；大规模数据与持久索引产品不可用同一个性能数字比较。
- `dxt/server.js:390-441`：摘要和工具模式给 agent 的动作语义清晰，值得学习可发现性。

**对 ASG 的压力**：MCP 工具要表达“用户要找什么/下一步做什么”，不必让 LLM 先学会底层实体模型。**ASG 不应丢掉**：明确错误、严格预算、原文出处和中文短词可搜索性。

## 共通整改准则

1. 正确区分 keyword/fuzzy/vector/真实 embedding/摘要，不用 hybrid 或 semantic 字样做能力代理。
2. 原生恢复、拿 transcript 启动新会话、导出 evidence pack 是三种行为，指标与文案不能混为一谈。
3. 一个产品有 desktop/MCP/CLI 不代表各入口同等排序/过滤/预算，ASG 要保留并加强跨入口契约实测。
4. 增加个人整理功能是否值得做，取决于用户定位；它们不是为达到检索/恢复闭环必须立刻补齐的项目。

## 剩余审查边界

尚未逐行覆盖各仓库的所有 parser、远程同步、富文本渲染、安装脚本和所有测试；未做真实模型推理与同语料实跑。不能将本文当成这些项目的完整安全审计证书。
