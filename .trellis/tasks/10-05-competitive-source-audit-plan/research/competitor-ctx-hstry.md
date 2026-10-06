# ctx / hstry 源码对照

证据范围：当前本地 tracked 源码快照，未更新远端、未运行竞品、未使用真实私人会话。全仓目录清点不等于逐行全文审读；本报告覆盖入口契约、检索、过滤、身份/批量写入和 SDK / adapter 边界。未读部分保持在总覆盖缺口中。

## ctx — 06bc5ed1：Rust、SQLite、面向 agent 的结构化历史接口

### 真实能力

1. `crates/ctx-history-search/src/packet.rs:14-74`：SearchPacket 是带 schema_version、filters、generation time、pagination、truncation 的结构化契约。每条结果包括 session/event 身份、why_matched、citations、links、visibility、provider/raw source 信息。ASG 不能把“结构化、有出处”说成无人具备。
2. `crates/ctx-history-search/src/search.rs:27-70`：空 provider/file scope 提前返回；fast path 或 ranked candidate path；明确区分 scan_budget / limit 截断。
3. `search.rs:73-169`：semantic/hybrid 候选合并，lexical rank 与 semantic rank/score 分离，有独立权重。
4. `search.rs:207-278`：multi-term 检索合并、去重、why_matched 和截断原因，结果按任务包装，而非只给数据库行。
5. `filters.rs:9-59`：source/session/time/agent/file filters；repo 采用 cwd/raw path/workspace substring 匹配，**不是** ASG canonical repo slug 逐字相等。对跑之前必须先统一查询意图。
6. `sdks/python/src/ctx_agent_history/transport.py:126-169`、`sdks/swift/Sources/CtxAgentHistory/AgentHistoryClient.swift:72-109`：本地 CLI transport 被 SDK 包装；安装 setup / discovery / query / locate 组成可集成的产品接口。
7. `sdks/typescript/src/index.js:253-332`：HostedAgentHistoryClient 是明确报 unsupported 的 placeholder，不能把 SDK 中有 hosted 类当成云服务已实现。
8. README 的 local/private 边界明确说保留路径和 secret-shaped strings，复制外发前需检查；这和 ASG 的跨边界默认脱敏形成真实差异，而不是“ctx 不安全”的简单结论。

### 值得借鉴

- 面向“找到先前的讨论/失败/文件操作”的任务契约；citation/links/why_matched 的可消费性。
- 一套用户能自行完成的 setup 流程，而不是把数据目录/索引初始化隐含在文档深处。
- 对预算截断、未实现 transport 如实标注。

### 不应盲目照搬

- `search.rs:94-103` 的 hybrid lexical 部分先取有限候选再做部分过滤；需要专项过度选择性 filter 测试，不能凭整体成熟度假定不会漏召回。
- SDK 数量、插件入口和 hosted placeholder 不是 ASG 首发必须追齐的清单。
- README 的“50x token-efficient”是某些输入输出条件下的宣传数字，本轮没有复现，不纳入 ASG 性能排名。

## hstry — 88b78b1：Rust SQLite core + TypeScript adapters

### 真实能力

1. `crates/hstry-core/src/db.rs:2102-2197`：FTS 查询返回 message + conversation + source/workspace；source/time/role/model/harness/tag 过滤先写进 SQL，再 LIMIT/OFFSET。
2. `db.rs:3144-3175`：natural language 与 code 分别用不同 FTS 表，路径、scope、snake_case、camelCase 等触发 code mode。这是 ASG 可评估的 code-aware 查询策略，而不是立即改 tokenizer 的理由。
3. `db.rs:3177-3194`：将特殊字符视为字面词项，保留尾部 prefix；已有避开 FTS 语法误解释的明确实现。
4. `crates/hstry-cli/src/main.rs:1266-1338,1364-1390`：source registry 校验、路径/adapter 变更时重置 cursor；默认并行度最多 4，结果集中汇总。
5. `crates/hstry-core/src/ingest.rs:63-93`：conversation 从 UUID v4 起步，利用 source_id + external_id 找回已有记录，并在 batch 内去重。不能粗略写成“所有身份都是 UUID v5”。
6. `ingest.rs:171-207`：message id 由 source、external conversation id、idx、role、content 派生；结构与 ASG 稳定消息 / Placement 分离并不相同。
7. `ingest.rs:211-245`：单事务批量写入、进程内 writer queue、约 60 行一个 INSERT；parent relation 在后续 pass 解析。批量路径值得学，但“有 transaction”不等于和 ASG durable intent / relation invariant 等价。
8. `adapters/cursor/adapter.ts:80-104,109-137,170-193`：TypeScript adapter 通过 readonly SQLite 读取；探测/解析错误存在 continue/日志路径，不应把它们当成 fail-closed 的同义词。
9. README 表明服务、SSH、跨格式 export/resume、可选 TUI 是不同能力；source families 含 ChatGPT 等 web-chat，provider 数不应与仅 coding-agent 的口径直接比大小。

### 值得借鉴

- 批量 transaction 和 source 独立同步，结果与故障按来源分组报告。
- 将格式适配与统一存储分离；对 natural-language / code 的不同查询意图建测试。
- 消息、结构化 parts、usage、附件等有明确域模型，不把所有数据塞成一段 text。

### 不应盲目照搬

- 为了抄 adapter 生态强行让 ASG 同时依赖 Rust 与 JavaScript 运行时。
- 将跨格式转换后启动新 agent 称作“原生恢复”；必须定义可保留/必丢失的状态。
- 根据 UUID 算法名判断整个身份系统的优劣；需要检验多来源同 ID、重放、改写、分叉、移动的完整生命周期。

## 本组对 ASG 的结论

ASG 的核心方向不是没人做过的蓝海。它可以竞争，但要证明：相同数据与用户问题下，citation 更可靠、过滤分页不丢、错误不掩盖、工作流更顺畅、首用更低成本。这比“我也有端口/规范/多个入口”更有说服力。

## 尚未完成

ctx/hstry 的全部 parser、SDK 测试、后台服务/远程操作、安全/安装链路尚未逐文件读完；没有完成跨语言 runtime、模型权重或跨平台实跑。此报告不得作为全量安全背书或性能优胜证明。
