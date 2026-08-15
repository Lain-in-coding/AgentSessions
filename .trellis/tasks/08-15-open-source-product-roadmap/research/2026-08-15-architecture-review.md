# Research: Architecture-Level Review（agent-session-grep，Rust workspace）

- **Query**: 对 C:/AgentSessions（8-crate hexagonal workspace）做架构级评审，对照 6 个 peer（hstry/Recall/memex/fast-resume/sessiongrep/cass）的架构模式，输出按维度的现状/对标/建议/风险与 top-10 优化机会
- **Scope**: internal（本地代码 + peer deep-read 报告 + borrowable-code-inventory）
- **Date**: 2026-08-15
- **前置资料**: `2026-08-15-borrowable-code-inventory.md`（参考地图）、`C:/AgentHub/project/Github_src/deep-read-{hstry,Recall,memex,fast-resume,sessiongrep,coding_agent_session_search}.md`
- **方法**: 全部 file:line 均经 Read/Grep 实测核验；workspace 总代码量 26,851 行（wc -l 实测）

---

## 0. 总体判断（一句话）

架构骨架（hexagonal 分层 + catalog-as-truth + generation/outbox 一致性 + 稳定错误码）**优于全部 6 个 peer**；主要问题不在"设计"而在"文件粒度"——`adapters-sqlite/src/lib.rs` 单文件 9,216 行（其中测试约 4,850 行）是唯一逼近 cass 反例的风险点，另有 3 个 P0/P1 缺口（provider-scoped identity 迁移、busy_timeout 缺失、release profile 未配置）。

---

## 1. Workspace / crate 拆分

### 现状

- 8 crate，依赖方向严格单向：domain ← ports ← application ← adapters-sqlite / provider-* / cli / testkit（`crates/*/Cargo.toml`，ports 的 doc 注释明确该不变量 `ports/src/lib.rs:6`）。
- 代码分布（wc -l 实测，2026-08-15）：

| 文件 | 行数 | 备注 |
|---|---|---|
| adapters-sqlite/src/lib.rs | **9,216** | 其中 `mod tests` 4,368-9,216 ≈ 4,850 行测试；非测试代码 ≈ 4,360 行 |
| cli/src/main.rs | 2,727 | 已拆出 mcp.rs(1,275)/human.rs(1,001)/protocol.rs(985)/tui/(1,337) |
| application/src/lib.rs | 2,595 | 已拆出 cursor.rs(476)/budget.rs(327)/evidence.rs(198)/cjk.rs(122) |
| provider-claude | 1,089 | — |
| provider-codex | 869 | — |
| domain（lib 702 + thread 1,286 + ids 451 + error 73） | 2,512 | — |
| testkit | 783 | — |
| ports | 572 | — |
| 其余小模块 | lease 282 / source_fs 218 / cas 127 | 已从 lib.rs 拆出，证明拆分路径可行 |

- lib.rs 内部结构（实测行号）：投影/合并函数 89-398 → relation manifest 399-908 → `SqliteStore` 主体 909-3,768（含 DDL 981-1,222、migrate 956-1,275、commit 路径 1,483-2,893、outbox/index-batch 状态机 2,895-3,768）→ `impl CatalogStore` 3,771-3,869 → `impl ContextGraphStore` 3,870-4,290 → `impl SearchIndex` 4,291-4,350 → `safe_fts_query` 4,351-4,367 → 测试 4,368 起。

### 对标

- **hstry**（6 crate）：hstry-core/db.rs 单文件 ~3,400 行是最大文件；hstry-mcp/hstry-tui 是 `//! Placeholder` 占位 crate（deep-read §1）。核心层把 db/ingest/schema/peek/source_registry 拆成独立模块。
- **Recall**（1 主 crate + 2 扩展）：70 个源文件，`db/` 拆 9 个文件、`adapters/` 17 个文件——同规模（11 provider）单 crate 靠模块粒度取胜。
- **sessiongrep**（1 crate，~4,600 行）：db.rs 641 行 + 13 个核心文件，极致克制。
- **cass**（反例）：lib.rs 10 万行 + storage/sqlite.rs 3 万行 + indexer/mod.rs 5.2 万行，deep-read §10 明确列为不可复制。
- **fast-resume**（1 crate 34 文件）：Tantivy 索引层拆 schema/document/queries/stats 四文件。

### 建议

8-crate 拆分本身**合适**（我们 2 个 provider，未来 16 个；crate 边界 = 编译边界 = 发布边界，与 PRD 的 provider promotion train 匹配；peer 中无一使用严格 hexagonal，但我们证明它能维持）。问题在 intra-crate 文件粒度，建议按以下边界机械拆分 lib.rs（零行为变更，先搬代码后跑测试）：

- **P0** `schema.rs`：DDL + `init/migrate`（lib.rs:956-1,275，含 v1-v7 迁移与 `SCHEMA_VERSION=7` 3,769）
- **P0** `projection.rs`：`searchable_text`/`hash_field`/`merge_message_payloads`/`merge_session_payloads`（89-398）
- **P0** `manifest.rs`：`RelationUpsertManifest`/`RelationDeleteManifest`/`SourceReplacementManifest`/`CanonicalBatchManifest`（399-908）
- **P0** `commit.rs`：`commit_batch`/`commit_source_batches_if_changed`/`sources_are_current`/alias 再生成（1,483-2,893，约 1,400 行）
- **P0** `index_batch.rs`：outbox 状态机 + `rebuild_index`(3,598)/`recover_interrupted`(3,693)（2,895-3,768）
- **P0** `catalog_store.rs`（3,771-3,869）、`context_graph.rs`（3,870-4,290）、`search.rs`（4,291-4,367，含 `safe_fts_query`）
- **P1** 测试搬迁：`mod tests`（4,368-9,216）按被测模块就近拆分或移 `tests/` 集成目录；4,850 行测试留在 lib.rs 是拆分后单文件仍 >4,000 行的主因
- **P2** cli/main.rs 2,727 行：hstry main.rs 2,650 行同量级有先例，可暂缓；命令面增长（16 provider + serve + handoff）后再按命令族拆

### 风险

- 拆分必须**纯搬移**：lib.rs 内部依赖密集（`SqliteStore` 单 impl 块），拆模块时 `pub(crate)` 可见性要一次到位，否则二次返工；建议拆完一个模块跑一次 `cargo test -p agent-session-grep-adapters-sqlite`。
- Windows Defender 已知 hazard（memory: agentsessions-windows-defender-hazard）：写后必须 Read 回读确认留存。

---

## 2. 数据流：catalog-as-truth + FTS-as-rebuildable-projection

### 现状

- `catalog` 表 + `fts5(id UNINDEXED, text)` 普通 FTS 表（lib.rs:981, 985），非 external-content、非 contentless——ADR-0001 spike 验证过 contentless（50MB→16MB）但未采用，与 borrowable-inventory §9.5（cass contentless 复杂度反例）一致，**这是对的**。
- 同一事务提交 catalog + FTS + generation：`commit_batch`（1,483）"durable outbox 包裹 upsert，原子提交 catalog + FTS + generation"；`upsert_fts_row_in_tx`（3,541）在写事务内维护 FTS 行；`fts_ids` 表（1,025）+ `ensure_fts_ids_rowid`（1,136）做 rowid 追踪，删除按 rowid 而非内容扫描（`large_store_delete_is_rowid_scoped_not_content_scanned` 6,858）。
- 可重建性：`INDEX_PROJECTION_VERSION=b"sqlite-fts5-v1"`（89）参与批次指纹（689）；`rebuild_index`（3,598）从 catalog 重投影 + 推进 generation；测试钉死 `rebuild_restores_search_after_index_data_wiped`（9,060）、`rebuild_removes_orphan_fts_rows_not_in_catalog`（9,003）。
- 投影确定性：跨源合并"确定性取更长投影"（125-139, 325），冲突拒绝（148, 250）；CJK bigram 在写入前变换（ADR-0007，`searchable_text` 101）。
- ADR-0001（FTS5 vs Tantivy）：单存储同事务提交 = 无跨存储漂移；Tantivy 被 `ort-sys` 供应链问题否决——与 ctx fork 的教训一致，决策稳固。

### 对标

- **hstry**：FTS **触发器自动同步**（ai/ad/au 三触发器）+ `ensure_fts_schema_optimized` 完整性自愈（默认 `LIMIT 1` 探针跳过全扫，`HSTRY_FTS_INTEGRITY_CHECK=1` 时每小时 integrity-check；tokenizer/prefix 不匹配自动 drop+重建 FTS 表）（deep-read §5.4）。
- **memex**：external-content FTS + 触发器（compact/db.rs）；trigram 支持中文子串（我们 CJK bigram 思路等价、token 数约 2x 是已知代价 ADR-0007）。
- **agentsview**：`sync_marker` 触发器防畸形时间戳污染水位（borrowable-inventory S7）。
- **sessiongrep**：FTS 召回 + 应用层重排（limit*5 候选 + 6 字段加权 + 新鲜度 + repo 加分）（deep-read §5.4）——这是我们的 `SearchHit` 装配层（ADR-0008）可对照的检索质量升级，属于 roadmap 子任务 #3 范围。

### 建议

投影策略本身**成立且优于 peer**（应用层同事务提交比触发器隐式同步更可审计；触发器方案的优势仅剩"写入方忘记维护 FTS 也能自愈"）。可借用两点：

- **P1** FTS 完整性自愈（hstry 模式）：open 时做轻量探针（`SELECT count(*) FROM fts` vs catalog 计数或 `LIMIT 1` integrity-check），不匹配则提示/自动 `rebuild_index`——我们已有 rebuild + fts_ids rowid 追踪，成本低；
- **P2** 投影函数版本已入指纹（`INDEX_PROJECTION_VERSION` + batch_manifest 689），将来 bigram/分词规则升级可直接触发重建，无需 schema 迁移——保持这个机制即可，不用学 hstry 的运行时 tokenizer 检测。

### 风险

- **P1 缺口**：`open()` 只设 `PRAGMA journal_mode=WAL`（957），**未设 busy_timeout**。peer 全部设置（hstry 30s / Recall 5,000ms / memex 5,000ms）。虽有 `sqlite_busy_maps_to_retryable_writer_busy` 测试（4,465），但生产路径没有重试循环——跨进程"写进行中另一进程读"会直接 Backend 错误。单写者 lease 下风险有限，但 `asg serve`（roadmap）后成为必踩坑。
- CJK bigram 词元约 2x（ADR-0007）→ 索引体积膨胀，已知且接受。

---

## 3. 并发：WriterLease + durable outbox + CAS

### 现状

- **WriterLease**（lease.rs，282 行）：权威是 OS 独占文件句柄（fs4，`data_root/writer.lock`），非 PID 超时抢锁；进程崩溃 OS 自动释放；Windows 强制锁语义用 error 32/33 分类归一（`is_lock_contention` 30-39）；进程内 held-path 集合解决 Windows 同进程二次 open 行为不一致（58-61, 90-101）；lease record 带 pid/fencing_token（127-140）；`Drop` 释放（192-199）。测试含"不披露路径"（253-270）。
- **Durable outbox**（index_batches 表，999-1,014，`CHECK(target_generation = base_generation + 1)`）：两阶段状态机——`begin_index_batch`（2,915）写 `building` durable intent（base 必须 == 当前活动 generation，否则 intent 作废）；`commit_index_batch`（2,980）事务内 verify（2,998：generation CAS + intent 状态 + manifest 指纹匹配）后同一事务提交 catalog+FTS+generation 切换并置 `activated`；`recover_interrupted`（3,693）中止孤儿 building intents；`CanonicalBatchManifest`（640）+ `batch_manifest`（653）指纹化防篡改/撕裂（测试 8,843/8,880）。
- **CAS** 两层：DB 内 `active_generation` CAS（store_metadata 表，993-997；verify 3,005-3,016）+ data-root 文件级 `CURRENT` CAS（cas.rs，`cas_activate` 54-74，tmp+rename 原子替换 + sync_all）。证据来源 spikes/data-root-locking。
- **无硬编码超时**：全 crate 扫描生产路径无 timeout 常量（仅测试 3s 断言 lib.rs:6,897/8,227 与 TUI 250ms 事件 poll `tui/mod.rs:68`）——这是与 memex 的关键差异，**不是 bug，是优点**。

### 对标

- **memex**：`try_lock` processing 标志 + `LOCK_TIMEOUT_MS=5*60*1000` 硬编码 5 分钟 + 启动时 `unlock_stale_locks`（compact/db.rs:1,081）——borrowable-inventory §9.4 明确列为反例：长 LLM 调用会误触 stale 清理。
- **hstry**：`ingest_writer: Mutex<()>` 单 writer gate 串行化 bulk ingest 事务 + 连接池并发 WAL reader + busy_timeout 30s（deep-read §5.1）。
- **Recall**：fs2 文件锁保证后台 worker 单实例（semantic.rs）。
- 结论：我们的"OS 锁权威 + durable intent + generation CAS"是 peer 中最强的组合（memex 的原子锁有超时缺陷，hstry 的 Mutex 只护单进程）；outbox 的"先 durable intent 再同事务提交"与 memex/hstry 无对应物，是我们独有的崩溃一致性保障。

### 建议

- **P0**（与 §2 合并）补 `busy_timeout`（建议 5,000ms，对齐 Recall/memex）——对**读**连接尤其重要，写侧已有 lease 保护；
- **P2** 文档化两层 CAS（fs CURRENT + DB active_generation）的职责边界——目前只有 spike 证据链，发布前应在 ADR 或架构文档一处说清"为什么两层"；
- **P2** 并发 `sync` 现在立即返回 WriterBusy（`try_acquire` 不阻塞）——作为 CLI UX 这是合理的 fail-fast，但 roadmap 若加 `--wait` 选项需要改 try_acquire 为带超时的 acquire，预留接口即可。

### 风险

- Windows 强制锁：持锁期间不得对同一路径另开句柄（lease.rs:3-5 注释）——任何未来代码若直接碰 `writer.lock` 路径（如诊断命令）会踩 error 33，需复用持锁句柄（`read_record` 159-164 已示范）。
- outbox 两阶段若在 writing 阶段崩溃，`recover_interrupted` 依赖重启后调用——若进程被 kill -9 且无人再启动，孤儿 intent 会永久占位（`interrupted_batch_count` 3,711 可观测）；建议 `doctor` 命令面把它纳入诊断输出（若尚未）。

---

## 4. 错误模型：CanonicalCode + robot envelope

### 现状

- 三层错误 + 一层协议映射：
  - `DomainError` 5 码（domain/error.rs:11-31）：`not_found`/`invalid_request`/`invariant_violation`/`unstable_identity`/`ambiguous_graph`，`code()` 是稳定对外契约（36-44，测试 `code_is_stable` 54）。
  - `PortError` 6 码（ports/lib.rs:17-41）：`backend`/`source_io`/`schema_incompatible`/`not_found`/`snapshot_changed`/`writer_busy`；**错误消息剥载荷**（`ensure_readable` 242-259 把 PortError 映射为 DomainError 且不泄漏路径，测试 534-571 断言 `!message.contains("secret")`）。
  - `ProviderError` 4 码（ports/lib.rs:282-298）：`ambiguous_variant`/`structural_fatal`/`source_changed_during_read`/`io`。
  - 协议层 `CanonicalCode`（cli/protocol.rs:48-155）：`as_str`/`exit_code`/`retryable`/`operator_action` 四维；`From<DomainError/AppError/ProviderError/PortError>` 统一映射（179-259）；`success_envelope`（335）/`error_envelope`（371）/progress/diagnostic frame（400-410）带 request_id 回声（306-320）；`SCHEMA_VERSION="1.0"`（21）；**发布 schema 与运行时映射交叉校验测试**（`published_error_catalog_matches_runtime_mapping` 787、`published_envelope_schema_contains_runtime_contract` 824）。
  - ADR-0005（缺失实体统一 exit 4 not_found）、ADR-0006（机器模式 help/version 走 success envelope）。
- 契约三处同源：protocol.rs + `docs/contracts/CONTRACT-cli-robot-mcp-draft.md` + `schemas/robot/v1/error-catalog.json`，由 787/824 测试钉住一致性。

### 对标

- **cass**：robot 模式 stdout 只发纯数据、错误封套走 stderr `{"error":{code,kind,message,hint,retryable}}`（deep-read §7）——结构最接近我们，但无 exit code 契约、无 request_id 回声。
- **sessiongrep**：MCP 错误统一 `{isError:true,...}` 封套；**无 CLI 级错误码体系**。
- **hstry**：`JsonResponse{ok,result,error}` 一层包，无码表、无 retryable 语义。
- **Recall**：anyhow 错误链 + 退出码 1，最弱。

### 建议

- **无需改动**——我们是 peer 中唯一具备"稳定码表 + exit code + retryable + operator_action + request_id + 发布 schema 对拍测试"五件套的。仅一条：
- **P1** roadmap 的 Robot 1.1（retrieval_mode）扩展时必须继续走"先改 error-catalog.json → 改 protocol.rs → 跑 787/824 对拍"的顺序，三处同源是当前最强的防漂移机制，别引入第四处（如 MCP 侧独立错误码）。

### 风险

- `PortError::Backend(String)` 保留载荷用于日志，但 `From<PortError> for ProtocolError`（229）若未来新增 PortError 变体忘记映射会静默走默认分支——建议保持现有测试矩阵覆盖每个变体（现有 `port_error_categories_map_to_catalog` 532 已覆盖）。

---

## 5. 身份：ses_v1_ StableId（BLAKE3 内容寻址）

### 现状

- `StableId`（domain/ids.rs）：typed 前缀 `src_v1_`/`doc_v1_`/`ses_v1_`/`msg_v1_`（48-55）；派生 ID = BLAKE3（128 位截断，32 hex）+ length-prefixed fact framing 防边界碰撞（140-157，测试 334）+ kind 域分离（144，测试 341）；`native()` 原样采纳 provider 原生 id（sanitize：去控制字符、256 字符上限，117-129）；`Stability` 三档 Native/Reconstructed/Unstable（70-78）；`from_wire` 恒返回 Unstable（诚实——stability 不上线，187-206）；`PlacementId` plc_v1_（99, 234-281）承载"上下文发生"身份。
- **已知缺口：`ses_v1_` 未按 provider 作用域**——`native()` 只加 kind 前缀，两个 provider 若产生相同原生 session id 会碰撞为同一 `ses_v1_<id>`。迁移计划已排期：`docs/product/OPEN-SOURCE-ROADMAP.md:53` 将 **provider-scoped identity 列为 08-15-unified-release-contract 的 P0**；alias 再生成机制已存在（adapters-sqlite 2,473 `regenerate_compatibility_aliases_in_tx`，测试 5,534 起）。

### 对标

- **hstry**：`stable_message_id` = UUIDv5(namespace) 内容寻址，fact 含 `source_id:external_id:idx:role:content_prefix`，content 截 4,096 字节且 UTF-8 边界截断（deep-read §5.5）——**fact 含 source_id，天然 provider-scoped**；`ON CONFLICT(id)` 天然去重。
- **sessiongrep**：复合主键 `{provider}:{native_id}`（models.rs），跨 provider 永不冲突 + resolve 三策略（精确/原生/前缀），多匹配报 ambiguous（deep-read §5.5）——是我们 AmbiguousGraph 码的同构物。
- **Recall**：`(source, source_id)` UNIQUE + 内部 UUIDv4（replace 时重新生成，**id 不稳定**）——我们显著更强。
- **cass**：`(source_id, agent_id, external_id)` 溯源三元组 + BLAKE3 双哈希 packet（semantic_hash 排除 row ID）。
- 结论：我们的 typed-prefix + stability-tier 设计是 peer 中最完整的；唯一不及格项正是 provider 作用域——hstry（fact 含 source_id）和 sessiongrep（复合键）都从构造上解决，我们**推迟了**它。

### 建议

- **P0**（跟随 roadmap）：迁移方案建议 = 派生侧在 fact 序列头部加入 provider id（`derive()` 调用处逐个加 fact，机制零改动——ids.rs:140 的 framing 天然支持），native 侧改为 `{provider}:{raw}` 复合采纳或对原生 id 哈希；借用 sessiongrep 的"多策略 resolve + ambiguous 报错"（我们已有 `AmbiguousGraph` 码和 `resolve_session` 语义，ports 层可对齐）。hstry 的 UTF-8 边界截断教训（S1，borrowable-inventory）已在 `NATIVE_SUFFIX_MAX_CHARS` 中体现，无需再搬。
- **P1** 迁移上线前，在 catalog 写入路径加一条防碰撞断言（同 wire id 不同 provider → 拒绝而非覆盖），把碰撞从"静默"变"可观测"——`shared_wire_id_with_conflicting_identity_metadata_is_rejected`（8,681）已有同构测试可扩展。

### 风险

- 迁移是**破坏性变更**：旧 `ses_v1_` 行要 alias 再生成（2,473 机制可用）或重建 catalog；PRD GA 门要求升级 rehearsal，此迁移必须在 0.x 阶段完成（发布后冻结）。
- `from_wire` 恒 Unstable 的语义对 MCP 消费者可能造成困惑（wire 回环的 id 不能持久引用）——契约文档已写明，保持。

---

## 6. 依赖

### 现状

- Cargo.lock 共 **120 个包**。运行依赖仅：blake3 1.5、thiserror 2.0、serde/serde_json 1.0、ratatui 0.29、crossterm 0.28、rusqlite 0.40（bundled + trace，trace 仅测试用，注释明示 lib.rs:13-14）、fs4 0.13（sync）；dev 依赖 tempfile 3。
- **无 clap**（CLI 手写参数解析，protocol.rs:261 `parse_output_mode` 等）、无 chrono/anyhow/regex/tokio/async——时间戳用自研 `unix_ms()`（lib.rs:71）与 provider 原串透传（MessageEvent.timestamp 为原串，ports 366）。

### 对标

| 项目 | 依赖画像 |
|---|---|
| hstry | sqlx+tokio+tonic+axum+clap（重，gRPC+HTTP+TS runtime 三通道） |
| memex | axum+lancedb+tokio+reqwest+rust-embed（最重；LanceDB 需 protoc——被 ADR-0002 明确否决的模式） |
| Recall | candle+tokenizers+hf-hub+sqlite-vec+clap+chrono+anyhow（模型推理栈，重） |
| fast-resume | tantivy+rayon+clap+chrono+regex（中等） |
| sessiongrep | clap+chrono+regex+fuzzy-matcher（中等） |
| **cass（反例）** | **自研 franken* 系列 git rev pin**（frankensqlite/frankensearch/franken-agent-detection/ftui/asupersync）——deep-read §10.2 明确为反例：生态封闭、升级与维护风险全部自担 |

- 结论：我们是**全部 peer 中最 lean 的**，且全部是成熟 crate（rusqlite bundled 内嵌 SQLite 免系统依赖，ADR-0002 冻结；fs4 是跨平台锁的成熟选择）。cass 的"pin 自研 crate"反例我们零接触——连 cass 的 clean-room 边界（Rider）都因此无实际影响。

### 建议

- **P1** ratatui 0.29 → 当前线（0.30+）/ crossterm 0.28 → 0.29：TUI resume 任务启动前做一次小升级核对（0.30 有 API 调整），避免在 TUI 功能开发中途升级；
- **P2** 无 clap 是**有意的**（robot 输出契约要求前缀位置 flag 解析，`parse_output_mode` 测试 890-935 钉死行为）：命令面膨胀（16 provider + serve + handoff pack）后重估一次，若仍不引入，须保持"解析器测试钉死 + 不裸暴露 FTS 语法"的现状；
- **P2** `asg serve`（roadmap）若引入 axum/tokio，注意 ADR-0002"禁止联网构建依赖"与"依赖树干净"两个硬门——axum 生态本身不触网，但需重新跑 cargo-deny/license 审计（ADR-0001 的 ort-sys 教训）。

### 风险

- rusqlite 0.40 bundled 的实际 SQLite 版本由 crate 决定，ADR-0002 说 spike 实测 3.53.2——**doctor 不输出 SQLite 版本**（ADR-0002:38 明示），发布前应在 `doctor` 或 `--version` 输出带上，否则用户报 bug 无法对齐版本。

---

## 7. Release profile

### 现状

- **完全未配置**：workspace Cargo.toml 无 `[profile.*]` 段（实测确认，.cargo/config.toml 也不存在）→ cargo 默认 release（opt-level 3、lto off、codegen-units 16、panic=unwind、debuginfo 无 strip）。

### 对标

| 项目 | release profile | 取舍 |
|---|---|---|
| hstry | opt-level "z" + lto true + codegen-units 1 + **panic="abort"** + strip true | 最小二进制；deep-read §10 自注"牺牲 panic unwind 能力" |
| memex | opt-level "s" + lto thin + codegen-units 4 + strip | 体积优先，保留 unwind |
| Recall | opt-level 3 + lto true + strip | 均衡偏性能 |
| cass | opt-level 3 + lto + codegen-units 1 + strip + **panic="abort"** + profiling/release-perf 派生 profile | 性能优先；bench 用独立 profile 是好实践 |

### 建议

- **P1** 增加最小 release profile：`lto = "thin"` + `codegen-units = 1` + `strip = "symbols"`。理由：ADR-0002 承诺 4 个 target 分发单二进制，体积直接影响分发体验；thin lto 比 full 编译时间温和；**不建议 panic="abort"**——我们的错误模型是 Result-first，panic 是 bug 信号，保留 unwind 才能拿到 backtrace（hstry 放弃这个能力的代价在崩溃诊断时显现）；binary size 测量在 bench/evidence 流程中补一次（memex/hstry 的体积数据可作参照系）。
- **P2** 学 cass 加派生 profile（如 `release-perf` 供 benchmark harness 用），避免 benchmark 与发布配置互相污染。

### 风险

- codegen-units=1 会让增量 release 构建变慢（CI 发布 job 时间上升），若 CI 预算敏感可只做 lto="thin" + strip 起步，量化后再收紧。

---

## 8. Top-10 优化机会（按 effort/reward 排序）

| # | 事项 | 优先级 | Effort | Reward | 依据 |
|---|---|---|---|---|---|
| 1 | adapters-sqlite lib.rs 拆 8 模块（schema/projection/manifest/commit/index_batch/catalog/context_graph/search）+ 测试搬迁 | P0 | M（纯搬移，4,850 行测试是体力活） | 高——消除唯一逼近 cass 反例的风险点，后续所有 sqlite 工作受益 | §1 |
| 2 | provider-scoped identity 迁移（fact 加 provider / native 复合采纳 + 防碰撞断言） | P0 | M-H | 高——正确性缺口，roadmap 已排期，须在发布冻结前完成 | §5 |
| 3 | open() 补 `busy_timeout=5000`（读连接）+ 文档化两层 CAS | P0/P2 | S | 中-高——`asg serve` 前必踩的跨进程读写坑 | §2/§3 |
| 4 | FTS 完整性自愈探针（hstry 模式：LIMIT-1 探针 + 自动 rebuild） | P1 | S-M | 中——我们已有 rebuild/rowid 追踪，加探针即闭环 | §2 |
| 5 | release profile：lto="thin" + codegen-units=1 + strip="symbols"（保留 unwind）+ 体积测量进 bench | P1 | S | 中——4 target 分发直接受益 | §7 |
| 6 | Robot 1.1（retrieval_mode）走"catalog→protocol→对拍测试"三处同源流程 | P1 | S | 中——防错误码漂移 | §4 |
| 7 | doctor/--version 输出 SQLite bundled 版本 | P1 | S | 中——bug 报告对齐基线 | §6 |
| 8 | cli/main.rs 2,727 行按命令族拆分（serve/handoff 落地时同步做） | P2 | M | 中——hstry 2,650 行有先例，可缓 | §1 |
| 9 | ratatui 0.30 / crossterm 0.29 升级核对（TUI 工作启动前） | P2 | S | 低-中——避免中途升级 | §6 |
| 10 | clap 重估 + async 依赖引入前的 cargo-deny/license 复跑 | P2 | S | 低——保持 lean 现状即可 | §6 |

---

## Caveats / Not Found

- **未核验项**：adapters-sqlite 内部"两阶段 outbox 在 writing 阶段崩溃后无人重启"的孤儿占位行为（只有 `recover_interrupted` 测试 8,943/9,169，未验证"从不调用 recover"的观测路径是否进 doctor）；`source_fs.rs`（218 行）未逐行精读，仅确认其为 SourceDiscovery 实现（不影响本报告结论）。
- **peer 行号**：hstry/memex/Recall 引用均基于 deep-read 报告行号，未逐一回读原文；borrowable-inventory 已核验过的条目直接引用其结论。
- **非结论**：cass 全部条目仅 clean-room（Rider 限制），本报告对 cass 的引用（依赖反例、release profile 派生 profile）均为**架构模式**引用，无代码借用。
