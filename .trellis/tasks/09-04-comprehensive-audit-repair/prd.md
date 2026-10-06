# 全面修复审计发现

## Goal

把 2026-09-04 完成的 10-Agent 对抗式审查中确认的高风险 correctness、security、protocol、performance 与 maintainability 问题，按“契约优先、Fail-Closed、分层收敛”原则收敛为可验证的生产实现；不以单个测试通过或局部补丁冒充全项目已修复。

## Requirements

### 安全与输入边界

- 修复 redaction 中固定 byte slicing 触发的 UTF-8 非字符边界 panic；保持现有 secret kind、marker、ruleset v1.1、计数和隐私边界不变。
- 修复用户可达时间过滤路径的 checked calendar arithmetic；debug/release 对极端年份都返回明确错误而不 panic 或 wrap。
- domain 排序时间解析与 application 严格过滤解析的有意策略差异必须保留并书面化；domain 解析拒绝非法 numeric offset 和不存在的日期，不引入宿主时区依赖。
- relation graph 的 edge parent Message ID 必须验证 kind 与 wire 形状；合法 orphan parent 仍被接受。
- 新写入不得继续通过 trim、删除控制字符或截断制造 native ID alias；合法历史 wire ID 不得被普通同步静默改写。
- provider-grok 不得将 provider-controlled `u64` 索引静默 cast 为可能截断的 `usize`。
- hook 的预算判断与截断必须使用同一、文档化的 Unicode-aware conservative estimator；输出必须保持合法 JSON 和 Unicode 边界。

### 存储与检索正确性

- source/entity tombstone 必须在同一事务内清理对应 `message_vec`，已删除消息不得再被 semantic/hybrid 命中；历史 orphan vector 只能通过显式维护路径收敛。
- `SemanticIndex::is_ready` 必须区分 ready、合法 not-ready 与 backend/schema/busy error；错误不得被转换成 lexical fallback。
- 非默认 facet 查询不得静默丢弃 session metadata-only hit；过滤必须在最终 limit 前生效。
- 不得直接比较不同 FTS corpus 的 raw BM25；使用固定、可测试的 rank-based fusion，并给排序/cursor 版本变化明确失效语义。
- semantic ranking 不得将 NaN/Infinity 送入结果；坏向量必须显式拒绝或按既有 bounded error/skip 合同处理。
- source batch 的 no-op 快路径不得绕过 duplicate、overlap 和跨 source integrity validation。
- cursor 必须绑定所有会改变结果集或排序的 normalized retrieval state，包括 requested/effective mode、model/embedding context、ranking/sort version、filters/facets 与 result-set；错误不得静默回到第一页。

### 跨入口契约与能力

- provider filter 的 canonical IDs、历史 aliases、filterable/implemented 状态以 capability/registry 为唯一事实源；不得把 deferred provider 宣称为已实现。
- CLI、Robot、MCP、Web 使用共同 Application request/semantic resolution；不得各自复制 model selection、ranking、pagination 或 budget 规则。
- 内部 dispatch 使用具名 typed result，避免从 JSON payload 反推 effective mode 或其他 metadata；外部 Robot/MCP/Web projection 的差异必须在合同中明确。
- Web/TUI 若仍是受限 subset，必须诚实声明 subset 与 unsupported 能力，不得把静态 rehearsal 通过当作 full parity。
- MCP 继续以当前实现支持的 2025-06-18 为冻结版本；明确 opaque cursor、notification、batch 与工具业务分页的边界，不在本任务盲升协议。

### 性能与运维

- grouped/session metadata SQL 使用有界候选集合和已有 chunk helpers，避免每个 message hit 拼接一个 correlated subquery。
- embedding rebuild 与必要的 catalog reprojection 按 wire ID 流式分页，不使用全库 `list(usize::MAX)` 物化；context 等明显 N+1 只在有结果等价测试后优化。
- SQLite 每条相关 connection 使用有限 `busy_timeout`，但不替代 WriterLease 或引入无限重试。
- `index_batches` 只在明确 retention policy 下清理 terminal rows，不误删 building/recovery evidence；维护操作幂等、事务化、可观测。

## Constraints

- `catalog` 是唯一 authoritative payload store；FTS、session projections、message vectors 都是可重建 projection。
- domain 不得依赖 ports/application/adapter；ports 不得依赖 rusqlite、filesystem 或具体 provider；application 不得依赖 concrete adapter。
- provider transcript/source 文件只读；测试与 benchmark 只使用合成或已经脱敏的 fixture，不写入真实路径、身份、token 或 source bytes。
- 不直接修改 `installation_namespace`，不直接把 `ses_v1` 切换为 `ses_v2`，不在没有 `id_alias`/schema migration/backfill 设计时重写历史 identity；该项作为独立迁移任务。
- 不盲目升级 MCP 版本；不为未知 capability 添加 undocumented fallback；不全局启用 `deny_unknown_fields` 破坏已有兼容性。
- 保持现有 Robot/CLI exit code、redaction status、cursor fail-closed、writer lease、durable outbox 与 source snapshot 语义，除非在变更合同中明确记录。
- 每个逻辑修改只 stage 明确文件；未经用户明确要求不 commit、push、merge 或改变仓库 visibility。

## Acceptance Criteria

- [ ] 阶段 1 的 malformed Unicode、极端时间、非法 ID、32-bit index、NaN/Infinity 与 hook budget fixture 在 debug/release 下无 panic、无静默截断，且错误/skip 语义可观察。
- [ ] 删除消息后 lexical、semantic、hybrid 三种路径均不返回该消息；事务失败时 catalog、FTS、sidecar、vector 与 generation 一致回滚。
- [ ] Semantic ready/not-ready/backend error 三态在 CLI、Robot、MCP 中分别产生正确 effective mode、warning/error code，不把 backend error 伪装成 fallback。
- [ ] facet 查询保留按合同应保留的 session metadata-only hits；FTS ranking 使用稳定 rank fusion；NaN/Infinity 不进入 SearchHit。
- [ ] cursor 对 mode、model/embedding、ranking version、filter/facet、generation 变化显式拒绝，合法 unchanged continuation 仍工作；旧 token 不会静默重启第一页。
- [ ] 所有实际可过滤 provider 的 CLI/MCP/Web canonical ID 来自同一 registry，deferred/unsupported 仍明确拒绝；Web/TUI subset 在 capability metadata 与 rehearsal 中显式呈现。
- [ ] grouped SQL、embedding rebuild、context reads 的内存/SQL 参数有明确上界，并通过 EXPLAIN QUERY PLAN、批次界限和结果等价测试。
- [ ] index_batches retention 不删除 building/recovery rows，重复维护幂等且不无故推进 generation。
- [ ] cross-entry rehearsal 覆盖 baseline、filters、modes/cursors、budgets、protocol/errors、tombstone/NaN 与 subset；空结果、缺字段和未实现状态不能产生 vacuous pass。
- [ ] 最终通过 `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、release tests、32-bit check（可行时）和 fresh-clone gate；关键修复的 mutation 不存活。

## Notes

- 已完成的 Grok/官方资料和源码证据写入实施设计；实现阶段遇到具体 API 版本问题必须再次使用 Grok/官方文档核对，不凭记忆猜签名。
- 当前优先级：可达 panic/删除幽灵命中/静默错误结果 > cursor/身份/跨入口一致性 > SQL/内存/retention > 更大范围抽象与完整 parity。
