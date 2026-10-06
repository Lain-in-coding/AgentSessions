# 全面修复审计发现：技术设计

## 1. 设计原则与边界

采用“契约优先、Fail-Closed、分层收敛”而不是一次性重写搜索系统。数据流保持：

```text
provider read-only source
  → snapshot/probe/parse
  → canonical staged entities + relations
  → SQLite catalog（唯一事实源）
  → fts / session projections / message_vec（可重建 projections）
  → Application request/ranking/cursor
  → CLI / Robot / MCP / Web / TUI projections
```

每条边界先用当前合同的类型和错误语义表达，再修实现。任何不确定的库行为必须用 Grok 官方资料核对；外部项目只提取可复用的架构经验，不复制无 LICENSE 或受限 license 的代码。

不在本任务直接处理 `installation_namespace` 的现有 `ses_v1` 身份重写。它需要独立的 `ses_v2`、namespace registry、id_alias、placement backfill 和恢复快照设计。

## 2. 阶段设计

### 阶段 A：输入与纯函数安全

**目标**：不触及持久化 schema，先清除可达 panic、溢出和脏输入进入 canonical model 的路径。

#### A1 Redaction

Owner：`ports/src/redact.rs`。

`standalone_secret` 改为字符迭代或 checked char-boundary 逻辑；不能用 `s[..20]` 直接切 UTF-8。识别范围、marker、ruleset、计数和其它 embedded span 行为不变。测试使用 2/3/4-byte Unicode scalar、CJK、emoji、组合字符，覆盖 `AKIA` 短前缀与合法 key。通过 `catch_unwind`/property 证明任意 Unicode 字符串不 panic。

#### A2 日期与 offset

Owner：`domain/src/thread.rs` 与 `application/src/lib.rs`。

保留两个解析器的职责差异：domain 是排序用宽 parser（失败可 byte fallback），application 是用户过滤用严 parser（拒绝 naive local time）。共享或同构 checked 的 epoch-day primitive，但不强行统一接受语法。

- domain offset 校验 `hours <= 23`、`minutes <= 59`，偏移计算 checked。
- domain calendar 校验实际月长与闰年，拒绝 02-30/04-31。
- application `days_from_civil` 每一步 checked，极端年份返回 `None`。
- 合法既有格式、epoch/year boundary、fraction 规则保持；宽/严差异在注释/测试中交叉说明。

不要修改已经存在的 `is_char_boundary` 守卫；`thread.rs:291` 的“非 ASCII timezone panic”候选已被核实为误报，真实问题是 offset 范围。

#### A3 ID 与 relation validation

Owner：`domain/src/ids.rs`、`domain/src/lib.rs`。

`StableId::native` 当前的静默 trim/filter/truncate 会导致非单射。推荐最小安全策略是新增 checked native-id 入口：空白仍表示“无 native id”并由调用方走既有 fallback；包含控制字符、清洗后为空、超过上限的 native id 拒绝当前写入，不在 `ses_v1` 路径自动改变 wire ID。合法、短、无控制字符 ID 保持 byte-compatible。历史异常 identity 由后续迁移处理。

`SessionContextGraph::validate` 的 edge parent 分支在 kind 检查后增加 `parent_message_id.validate()`；孤儿但形状合法的 parent 继续允许。用 serde 从 JSON 构造裸前缀和 kind/value 不匹配的真实输入，确保错误是 `InvariantViolation` 而不是父链静默断开。

#### A4 Provider integer 与 Hook budget

Owner：`provider-grok/src/lib.rs`、`cli/src/hooks.rs`、application 现有 budget owner。

- provider-controlled `u64` 使用 `usize::try_from`；溢出按既有 recoverable diagnostic/skip，不截断。
- 复用 application 已有 Unicode-aware token estimate；把 estimator 置于正确共享 owner，hook 的判断、截断和 `truncated` 标记使用同一 conservative unit。不要把近似 estimator 宣称为具体模型 tokenizer。
- `max_tokens == 0`、极大值、CJK、emoji/ZWJ/combining marks、边界和 JSON 结构都要测试。

### 阶段 B：存储、检索、语义与 cursor

**目标**：让结果集合、删除语义和语义错误诚实。

#### B1 Semantic readiness

优先选择端口签名升级：`SemanticIndex::is_ready(&self) -> PortResult<bool>`。同步 `&T` blanket、NoSemanticIndex、testkit fake、Application 和 CLI/MCP caller。

- `Ok(true)`：可以走 semantic/hybrid。
- `Ok(false)`：只在合同允许的“未配置/无向量”情况下执行既有 lexical fallback，并携带 effective mode/warning。
- `Err`：保留 `Backend`/`SchemaIncompatible`/`WriterBusy` 等来源，直接映射错误，不能转成 false。

如果改签名的波及量过大，短期可在 store 内记录 readiness error 并让下一次 semantic query fail-closed；但最终仍应收敛到端口错误通道，避免隐式状态。

#### B2 Tombstone vectors

在 `commit_index_batch_with_relations` 删除 chunk 中，与 catalog、fts、fts_ids 同一事务执行：

```sql
DELETE FROM message_vec WHERE wire_id IN (...)
```

复用 `in_placeholders`、`BULK_INSERT_ROWS_PER_CHUNK`。事务失败整体回滚。历史孤儿向量只由显式 maintenance/rebuild 清理，不在 read/search 隐式写库。测试删除后 lexical/semantic/hybrid 都无命中，并验证 rollback、重复清理和仍被 catalog 引用的向量。

#### B3 Facet 与 FTS ranking

`query_faceted` 的非默认路径也必须装配 session metadata candidates；provider/time/facet 过滤在最终 limit 之前应用。保持 representative 的稳定选择和 system/developer 排除。

不要直接合并 `bm25(session_fts)` 与 `bm25(fts)`。两张 FTS 表的 corpus statistics 不同，按各自 list 排名后使用固定 RRF（`k=60`）合并，最后以 canonical wire ID 做确定性 tie-break。adapter 不依赖 application 时，把极小、无业务依赖的 RRF primitive/常量放 ports，或用等价本地 primitive 并以 cross-layer golden 锁定。

语义 score 进入排序前必须 `is_finite()`；NaN/Infinity 走明确坏向量错误或 bounded skip，不能 `partial_cmp(...).unwrap_or(Equal)` 继续输出。finite 值可用 `total_cmp`。

plain-text 合同优先：用户输入的 `*` 不是 FTS grammar。CJK `*` 的未来 prefix 语义另立 query-language/index contract；本任务只修复不一致的隐式行为并补合同测试。

#### B4 Current/no-op 与 cursor

`commit_source_batches_if_changed` 必须在 `sources_are_current` 早退前执行轻量 duplicate/overlap/integrity validation；只有合法 current batch 才返回 `Ok(false)`。

cursor 的 query-state digest 版本化，绑定：

- normalized query/filters/facets/repo/time/provider；
- include_system、group_by_session；
- requested/effective retrieval mode；
- semantic model ID/version/dimension 或 embedding context；
- ranking/RRF/sort version；
- result-set discriminator。

已有 generation、TTL、签名和 invalid-cursor 错误保持。排序算法变化必须 bump sort digest；旧 token 明确失败，不能静默第一页或错误 offset。只有证实不会削弱绑定的默认 lexical 历史 digest 才保留 byte compatibility。

### 阶段 C：统一能力、semantic resolver 和 projections

#### C1 Provider registry

以 capability matrix/registry 作为 canonical provider ID、alias、implemented/filterable 状态、显示名和能力的唯一来源。覆盖当前实际实现的 14 个 provider；deferred provider 不伪装成可过滤实现。CLI `--provider`、MCP `providers`、Web selector、`list_providers` 使用同一 canonicalization（排序、去重、OR）。增加双向 drift test。

#### C2 Semantic resolver 与 typed result

把模型目录解析、Candle/BigramHash 选择、model ID、embedding context、readiness、requested/effective mode 收敛到一个 shared application policy seam。CLI/MCP/Web 使用同一种 AppRequest；MCP 不得在准备 embedding 后又调用不带 SemanticIndex 的 `resume_app`。模型 cache 的进程生命周期、并发和失败策略用测试锁定。

将 CLI 内部 raw tuple 换成私有 `DispatchResult`，显式携带 command/outcome/data/page/warnings/effective mode/redaction/model context。外部 envelope 继续按现有 schema 输出，不依赖 payload 字符串反推 metadata。

#### C3 Web/MCP/TUI projection

Web 选择一个明确定位：本任务按 loopback-only read-only canonical projection 处理。补齐当前可支持的 provider OR、facets、max bytes、mode/cursor 到同一 AppRequest；未实现能力返回明确 subset/unsupported，不在 route handler复制搜索逻辑。

MCP 保持 `2025-06-18`。明确 tools/list cursor 是 opaque、invalid cursor 的协议错误、notification 无 response、项目对 JSON-RPC batch 的既有选择；业务 `page.next_cursor` 与 MCP 顶层分页分开。Robot/MCP/Web 的 transport envelope 可以不同，但 canonical data/page/error fields 的比较合同要明确。

TUI 明确为 lexical/read-only subset，现有 sidechain/tool facet cycle 保留；不宣称 provider/time/repo/semantic/hybrid/include_system/group_by_session full support。

### 阶段 D：有界性能与运维

#### D1 Grouped SQL

将 append_session_metadata_hits 的 per-message correlated `NOT EXISTS` 改为有界 CTE/derived candidate set，使用既有 `chunk_ids`、`BATCH_IN_CHUNK=500` 和 `in_placeholders`。代表消息只求一次。先以结果 golden 固定 NULL/system/developer/filter 语义，再运行 `EXPLAIN QUERY PLAN`，没有确证前不增加 schema index。

#### D2 Streaming context/embedding/reprojection

- `get_session_context` 先收集 message IDs，调用已有 `get_many` 分块，避免 N+1。
- `build_embeddings` 删除 `store.list(usize::MAX)`，按 wire ID keyset pagination，只取 Message，固定 batch；每批处理完释放 payload/embedding。
- `reproject_from_catalog` 若仍需全库事务，按 key range 分块读取，证明峰值上界；不改变 durable intent 语义。
- alias regeneration、search payload parse、context clone 只在 benchmark 证明收益后优化，避免扩大风险面。

#### D3 index_batches retention

先定义 retention：只删 terminal `activated/aborted` 的老行，保留最近窗口和诊断证据；绝不删 `building` 或 recovery 所需 intent。使用显式、幂等、事务化 maintenance path，不在普通 read/open 隐式删除。若需要新字段，另开 additive migration。

### 阶段 E：证据门

扩展 `scripts/rehearsal/compare_entrypoints.py` 与 e2e：baseline、filters、modes/cursors、budgets、protocol/errors、tombstone/NaN、性能与 subset。共同 canonical fields 必须非空或有合法 empty reason；不能让空结果、缺字段或 `not_applicable` 产生 vacuous pass。

关键 mutation 必须被测试杀死：恢复裸 UTF-8 slicing、unchecked calendar、`as usize`、遗漏 message_vec delete、`unwrap_or(false)`、删除 mode digest、raw BM25、删除 facet metadata、恢复 list(MAX)、删除 busy_timeout、删除 retention building guard。

最终 gate：

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --release
```

并执行 32-bit target check、SQLite EXPLAIN regression、memory-bound regression、fresh-clone 与三平台 release rehearsal。代码修改前后确认 provider source hash 不变、无真实 transcript/path/token 进入提交。

## 3. 迁移、兼容与回滚

- redaction、checked arithmetic、grok conversion、hook budget、tombstone vector delete、readiness、busy timeout、facet/ranking、cursor digest、provider filter、流式读取通常无需 schema migration。
- ranking/RRF 变化必须同步 sort/search-state digest；旧 cursor 显式失效。
- `session_repo_slugs` 不可从 catalog 重建；open-time self-heal 保留它，显式 `index rebuild` 才重派生。
- native/installation identity 变化不能普通 sync 静默重写；`ses_v2`/namespace registry/id_alias/placement backfill 另案，迁移前快照回滚，不手改 user_version。
- MCP 仍 frozen 2025-06-18；Web/TUI 未实现能力用明确 subset/unsupported。
- 每个逻辑 commit 可单独回退；所有写入在 durable transaction；provider source 永远只读。

## 4. 提交与验证顺序

1. `test: freeze invalid-input and projection contracts`
2. `fix(input): make redaction, calendar, native IDs and hook budgets safe`
3. `fix(provider): reject narrow-index overflow and validate graph parent IDs`
4. `fix(storage): remove tombstone vectors and propagate semantic readiness errors`
5. `fix(search): repair facets, finite scores, ranking and cursor state`
6. `refactor(capability): unify provider filter and semantic request resolution`
7. `fix(protocol): type dispatch results and freeze Web/MCP/TUI projections`
8. `perf(sqlite): bound grouped queries, context reads and embedding/reprojection memory`
9. `chore(sqlite): add explicit index-batch retention maintenance`
10. `test(rehearsal): exercise supported entrypoint contracts and mutation gates`

每一步先读当前代码和目标 spec，再补测试（新代码应先让测试对旧实现失败），再改最小范围，先跑目标 crate 再跑全 workspace gate。当前仓库规则要求未收到明确“commit/push”指令不自动提交或推送。
