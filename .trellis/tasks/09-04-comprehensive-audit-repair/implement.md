# 全面修复审计发现：实施计划

## 0. 开工与通用门

- [ ] 读取 `prd.md`、`design.md`、各目标 package spec 与 shared thinking guides。
- [ ] `git status --short`，确认 clean；建立本任务变更 allowlist，provider source/transcript 不得进入 diff。
- [ ] 每一小步先添加回归测试并确认旧实现会失败，再实现；每个逻辑 commit 只 stage 明确文件。
- [ ] 每阶段至少运行受影响 crate 的 fmt/clippy/test；阶段结束运行 workspace gate。
- [ ] 任何技术细节/API 版本疑问先用 Grok MCP 查官方文档；不将未验证猜测写入代码或合同。

## 1. 阶段 A：输入、Unicode、时间、身份安全

### A1 Redaction

- [ ] 在 `ports/src/redact.rs` 增加 `AKIA` + CJK/emoji/组合字符边界回归测试和 `catch_unwind`/property 测试。
- [ ] `standalone_secret` 去掉 `s[..20]` 非边界切片，保持 ruleset v1.1、secret markers、计数和现有合法 key 行为。
- [ ] 运行 `cargo test -p agent-session-grep-ports` 与 clippy。

### A2 时间 parser

- [ ] 在 `domain/src/thread.rs` 增加非法 offset、短月份/闰年、epoch/边界日期测试；确认现有 char-boundary 守卫保持不动。
- [ ] 为 domain offset 增加 `<=23:59` 与 checked 计算；为 domain calendar 增加实际月长/闰年校验。
- [ ] 将 `application/src/lib.rs` 的 `days_from_civil` 改为 checked arithmetic；增加 `i64` 极端年份测试，并运行 debug/release。
- [ ] 在两处 doc/test 说明排序宽 parser 与过滤严 parser 的有意差异，不合并无时区策略。

### A3 IDs 与 graph

- [ ] 在 `domain/src/ids.rs` 先补控制字符/空白/超长/不同 preimage 的 alias regression。
- [ ] 选择 checked native-id 入口：异常 native ID 显式拒绝新写入，合法旧 ID byte-compatible；不普通同步静默重写历史 wire ID。
- [ ] 在 `domain/src/lib.rs` edge parent 分支增加 `validate()`；用 serde 构造 kind/value 不一致和裸前缀，保留合法 orphan 测试。
- [ ] 不改 `installation_namespace`；仅登记 Windows case 债务和未来 `ses_v2/id_alias` 迁移入口（如需测试，使用明确 ignore/迁移测试）。

### A4 Provider integer 与 hook budget

- [ ] 在 `provider-grok/src/lib.rs` 补 `u64`→`usize` 边界 fixture；改为 `usize::try_from`，按已有 provider recoverable/error 语义处理。
- [ ] 确认 `handoff_pack.rs:414` estimator 的 owner 与可见性；设计共享 Unicode-aware conservative budget helper。
- [ ] 改 `cli/src/hooks.rs` 让预算判断与截断共用同一单位；测试 ASCII/CJK/emoji/ZWJ/combining、0/极大值、JSON 完整性和 truncation 标志。
- [ ] A 阶段 gate：workspace fmt/clippy/test + release targeted tests；可行时 32-bit check。

## 2. 阶段 B：存储、检索、语义与 cursor

### B1 Readiness 与 vector tombstone

- [ ] 先为 ready/not-ready/backend-error 添加 fake/store 测试。
- [ ] 将 `SemanticIndex::is_ready` 与所有 implementation/callers 改为 `PortResult<bool>`，让 backend/schema/busy 错误保留；只对合法 `Ok(false)` 走既有 fallback。
- [ ] 在 `commit_index_batch_with_relations` 的 delete chunk 添加同事务 `DELETE FROM message_vec WHERE wire_id IN (...)`，复用 chunk helpers。
- [ ] 测试 tombstone 后 lexical/semantic/hybrid 都无命中、事务失败整体回滚、孤儿清理不误删活跃 vector。
- [ ] open/init 统一设置有限 `busy_timeout`，并测试读/写/迁移锁竞争不无限等待且 WriterLease 语义不变。

### B2 Facet、score 与 plain-text query

- [ ] 增加 metadata-only Session fixture；先测试非默认 facet 仍保留合同允许的 session hit。
- [ ] 重构 `query_faceted` 复用 metadata candidate 装配和最终 limit 前过滤；保持 system/developer、provider/time/repo semantics。
- [ ] 为跨 FTS corpus 的排序建立 ranking golden；各 list 先 rank，再用 RRF(k=60) 合并，最后 wire-ID tie-break；同步 sort digest version。
- [ ] 语义分数进入排序前 `is_finite()`；坏 vector 明确 error/skip，不使用 `partial_cmp` Equal fallback。
- [ ] 按 plain-text-only 合同锁定 ASCII/CJK/混合 `*` 字面行为；若改变 token transform 则先 bump projection version，不能隐式引入 wildcard 语言。

### B3 No-op 与 cursor state

- [ ] 构造“stored source current + duplicate message/placement/ordinal/overlap”输入，确认旧实现会错误 Ok(false)。
- [ ] 将轻量 integrity validation 提到 `sources_are_current` 快路径之前；current 合法输入仍不推进 generation。
- [ ] 将 query digest 升级为版本化 search-state，绑定 normalized query/filters/facets、requested/effective mode、model/embedding context、ranking/sort version、result-set；保留 generation/TTL/signature fail-closed。
- [ ] 测 lexical↔semantic/hybrid、model/ranking/facet/repo/generation 改变、合法 continuation、错误 cursor 不回第一页。
- [ ] B 阶段 gate：adapters-sqlite、application、ports、CLI targeted tests，再 workspace gate。

## 3. 阶段 C：跨入口能力与协议

### C1 Provider registry/filter

- [ ] 以 capability matrix/registry 为 canonical ID/alias/filterable source；列出实际实现 provider，deferred 不伪装。
- [ ] 扩展 SearchProvider/filter parser、CLI `--provider`、MCP schema/runtime、Web selector；provider OR canonicalize（canonical ID、排序、去重）。
- [ ] 双向 drift tests：matrix 可过滤项可接受，SearchProvider 无 matrix 外值；未知/deferred 明确错误。

### C2 Semantic resolver/dispatch result

- [ ] 先写 CLI/MCP characterization test：同 query、同 fake semantic backend 的 requested/effective mode、model、error、cursor 是否一致。
- [ ] 将模型目录、Candle/BigramHash、model ID、embedding context、readiness 和 Application 注入集中到一个 resolver/request builder；MCP 不再生成 embedding 后调用 NoSemanticIndex 路径。
- [ ] 模型 cache 生命周期/并发/失败策略测试化，不凭注释。
- [ ] 用私有 typed `DispatchResult` 替换 raw tuple，显式携带 command/outcome/data/page/warnings/effective mode/redaction/model context；维持 Robot/CLI 外部兼容。

### C3 Web/MCP/TUI projection

- [ ] Web route 统一映射 provider OR、facets、max_bytes、mode/cursor 到同一 AppRequest；未接线能力返回明确 subset/unsupported。
- [ ] 明确 Robot 完整 envelope 与 MCP/Web transport projection 的版本/共同 canonical fields，补 schemas/contracts 测试。
- [ ] MCP 继续冻结 2025-06-18；测试 opaque tools/list cursor、invalid cursor、notification 无响应、既有 batch 策略，不盲升 2026-07-28。
- [ ] TUI capability metadata 明确 lexical/read-only subset，不把未支持 semantic/hybrid/provider/time/repo/group/include_system 当 full parity。
- [ ] C 阶段 gate：CLI/MCP e2e、Web route tests、TUI tests、schema validation、workspace gate。

## 4. 阶段 D：性能与运维

### D1 Grouped SQL

- [ ] 先建立现有 SQL 结果 golden 和 NULL/system/developer/filter fixtures。
- [ ] 用 bounded CTE/derived candidate set 替换 per-wire `NOT EXISTS`，复用 `chunk_ids`/`BATCH_IN_CHUNK`/`in_placeholders`；representative 只求一次。
- [ ] 用 SQLite `EXPLAIN QUERY PLAN` 验证索引路径；极端 grouped window 验证 host parameter 上限和无 O(limit) SQL 膨胀。

### D2 Streaming reads

- [ ] `build_embeddings` 改为 Message-only wire-ID keyset pagination，固定 batch，结果字段形状与重跑语义不变。
- [ ] 必要时 `reproject_from_catalog` 改 key-range 分块读取；验证 durable intent 与 generation 语义。
- [ ] `get_session_context` 使用已有 `get_many` 分块，消除 N+1；重复 parse/clone 仅在 benchmark 证明后修。
- [ ] 记录最大 payload batch bytes/rows，验证不会全库驻留；失败重跑不留半成 projection。

### D3 index_batches retention

- [ ] 书面冻结 retention 条件：只删老 terminal activated/aborted；永不删除 building/recovery evidence。
- [ ] 通过显式、幂等、事务化 maintenance path 清理，不在 read/open 隐式删除；如缺安全字段另开 additive migration。
- [ ] 测保留窗口、building/restart recovery、空跑、失败回滚、generation 不变和 doctor 可观测计数。
- [ ] D 阶段 gate：SQL explain/perf、memory bound、SQLite contention、workspace gate。

## 5. 阶段 E：rehearsal、mutation 与 release gate

- [ ] 扩展 `scripts/rehearsal/compare_entrypoints.py` 与相关测试：baseline、provider/filter、time/repo/facets、metadata-only、semantic/hybrid/fallback、cursor mismatch、budgets、MCP errors、Web auth、Robot schema、TUI subset、tombstone/NaN。
- [ ] canonical comparison 忽略 transport-only request ID/duration/绝对路径，但不忽略 effective mode、IDs、error code、page/count；空结果/缺字段/not_applicable 不能 vacuous pass。
- [ ] 为每个关键修复运行 targeted mutation：恢复裸 slicing、unchecked arithmetic、as cast、漏 vector delete、吞 readiness error、删 mode digest、raw BM25、删 facet metadata、list(MAX)、删 busy_timeout、删 building guard；mutant 必须被测试杀死。
- [ ] 使用 synthetic/redacted fixtures；确认 provider source hash 不变，无真实 transcript/path/token 进入 diff。
- [ ] 最终 clean fresh clone 在 Windows/Linux/macOS 执行 fmt、clippy、debug/release test、release build、rehearsal；CI 账单失败与代码失败分开记录。

## 6. 独立延期项验收

- [ ] `ses_v2`/installation namespace/id_alias/Windows relocation migration：additive schema、双读、backfill、冲突 fail-closed、迁移前快照恢复。
- [ ] 历史 lossy native ID repair：拆分/alias/重建，不在普通 sync 静默改变 ses_v1。
- [ ] Web full parity 与 TUI full parity：各自独立 PRD、capability matrix、跨入口证据。
- [ ] MCP 2026-07-28 升级：独立版本迁移与 client compatibility matrix。
- [ ] CJK 真正 prefix query language：独立 query/index contract、projection bump 和 cursor version。

## 7. 最终验收

- [ ] 所有阶段 checklist 完成，所有逻辑 commit 目标 crate 与 workspace gate 全绿。
- [ ] `git diff --check`、status clean、变更 allowlist 复核、source hash 不变。
- [ ] 由 owner 决定是否 commit/push；未经明确指令不执行远端或仓库 visibility 操作。
