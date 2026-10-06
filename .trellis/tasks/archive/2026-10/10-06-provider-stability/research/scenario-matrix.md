# Provider 生命周期场景矩阵：Claude / Codex × 六类（B5）

> 任务：`.trellis/tasks/10-06-provider-stability`（PRD「Provider 稳定性」；父任务
> 10-05 第 7 节「Provider 与证据生命周期」）。
> 用途：实施侧逐项现状 + 缺口 + 本 wave 证据；公开声明只同步锚点/限定，不升等级。
> 记录时间：2026-10-06（工作树未提交；行号以本 wave 改动后的文件为准）。

## 0. 判分口径（provider 层 vs store 层）

- provider 是纯函数：`ProviderAdapter::parse(&[u8], sink)`——只有已验证快照字节，
  没有路径、没有上次解析结果（`crates/agent-session-grep-ports/src/lib.rs:1613`）。
  身份来自记录内 native id；`MessageEvent.span` 以快照字节为坐标系
  （`crates/agent-session-grep-ports/src/lib.rs:1268`），provider 不能归因时给
  `None`，上层 evidence 落 `precision=unknown`
  （`crates/agent-session-grep-application/src/evidence.rs:61`）。
- append/shrink/rewrite 因此是"同一路径不同快照"的 provider 层性质：前缀稳定
  （既有记录逐字段不变）+ 诚实降级（撕裂/改写只按当前字节产出，不缓存、不臆造）。
  水位推进 / tombstone / last-good 回滚是 store 层职责（见 §3 锚点）。
- fork：Claude 有 `parentUuid`（provider 层有意义，见 §2.4）；Codex 格式无父指针
  （provider 层 N/A，用反向测试钉住"绝不臆造"）。
- move / WAL：provider 层无路径输入、Claude/Codex 源也不是 SQLite —— 逐项给出
  N/A 理由与既有 store 层证据锚点（§2.5 / §2.6）。

## 1. 六类判定汇总（改动后）

| 场景 | claude-code | codex | 本 wave 证据（新增） |
|---|---|---|---|
| append | 覆盖 | 覆盖 | `tests/lifecycle.rs::append_keeps_existing_ids_spans_and_seq_byte_stable`；`tests/properties.rs::prop_append_keeps_prefix_byte_stable`（64 种子） |
| shrink（行边界/撕裂/清空） | 覆盖 | 覆盖 | `tests/lifecycle.rs::shrink_at_line_boundary_reproduces_prefix_parse_exactly`、`shrink_torn_tail_is_skipped_recoverably_without_shifting_prefix`、`shrink_to_empty_source_yields_empty_report_without_fabrication` |
| 同长改写 | 覆盖 | 覆盖 | `tests/lifecycle.rs::same_length_text_rewrite_updates_text_and_keeps_identity_and_span`、`same_length_uuid_rewrite_changes_identity_without_stale_cache`（codex 为 `same_length_id_rewrite_...`） |
| 分叉 parent 边 | 覆盖 | N/A（无父指针） | claude：`fork_siblings_keep_shared_parent_edge_and_sidechain_flag_verbatim`、`fork_dangling_parent_edge_is_preserved_for_cross_source_resolution`；codex 反向：`linear_rollout_never_fabricates_parent_edges`、`event_msg_mirrors_of_copied_prefix_stay_excluded` |
| 文件移动 | N/A（provider 无路径输入） | N/A（同上） | store 层既有：`crates/agent-session-grep-adapters-sqlite/src/relocation/tests.rs:468`（locator/身份/历史不变）、`:918`（反向移动）；cli e2e `crates/agent-session-grep-cli/tests/e2e.rs:7061`（relocate 保留 native 身份） |
| SQLite WAL | N/A（JSONL 源） | N/A（JSONL 源） | 既有：`crates/agent-session-grep-adapters-sqlite/src/source_fs.rs:309`（WAL 捕获 + WAL-only 变化检测 + 不写源）；`relocation/tests.rs:1579`、`:1618`（WAL 源搬迁/失效） |

## 2. 逐项：现状（改动前）→ 缺口 → 本 wave 处置

### 2.1 append

- 现状：两个 provider 都只有"单快照确定性"证据（`prop_parse_is_deterministic` /
  property 性质 4），没有"追加后前缀逐字段不变"的断言。store 端的增量 no-op
  只在 e2e 层覆盖（`crates/agent-session-grep-cli/tests/e2e.rs:1584`、
  `:1624`）。
- 缺口：增量 sync 依赖"未改动的字节前缀 → 相同消息/span"这一 provider 约定，
  此前无直接测试。
- 处置：两个 crate 各加确定性测试 + 复用既有种子生成器的 property（64 种子）。
  测试要求：追加一条合法记录后，既有消息 seq/native_id/parent/role/text/
  timestamp/is_sidechain/span 与旧快照逐字段相等；新消息 span 覆盖追加行
  （不含行尾）；`committed` 只 +1、`skipped` 不变。
  生成器细节：`case.bytes` 末行可能缺行尾（生成器 30%/25% 概率省略），测试先
  "封口"（补 LF）再追加，避免制造拼接行。
  锚点：claude `crates/agent-session-grep-provider-claude/tests/lifecycle.rs:126`、
  `crates/agent-session-grep-provider-claude/tests/properties.rs:551`；
  codex `crates/agent-session-grep-provider-codex/tests/lifecycle.rs:130`、
  `crates/agent-session-grep-provider-codex/tests/properties.rs:488`。

### 2.2 shrink（截断/清空）

- 现状：provider 层无截断测试；store 端有 tombstone/空源/撕裂尾保留三类 e2e
  （claude `sync_tombstones_message_removed_from_source` `e2e.rs:2014`、codex
  `sync_tombstones_message_removed_from_source_codex` `:2082`、空源
  `sync_empty_source_tombstones_all_messages`/`..._codex` `:2163`/`:2210`、
  撕裂尾 `sync_truncated_tail_retains_previous_index_without_churn` `:2261`——
  "保留旧索引不再解析"）。
- 缺口：① 行边界截断后，剩余消息必须与完整快照前缀逐字段一致（证明无整体
  依赖）；② 撕裂半行（崩溃写入）必须 recoverable skip，不得吞掉前缀、不得整体
  fatal；③ 清空源 parse 返回空报告（store 层把空源当合法空批次）。
- 处置：每种各有断言（codex 同构）。注意 probe 对空源仍返回
  `AmbiguousVariant`（`probe_rejects_empty`），parse 与 probe 的取舍不同是刻意
  契约：parse 只做记录级降级，不替调用方判断"该不该扫描这个源"。
  锚点：claude `.../provider-claude/tests/lifecycle.rs:155/171/193`；
  codex `.../provider-codex/tests/lifecycle.rs:160/176/198`。

### 2.3 同长改写（rewrite）

- 现状：无测试。风险：任何按 span/序号缓存身份或正文的实现都会在"字节长度
  不变、内容变了"的改写上静默复用旧解析结果。
- 处置：两条测试——① 同长文本改写：正文随当前字节更新、身份与 span 区间不变；
  ② 同长 id 改写：native id 立即变更（无陈旧缓存）。两者先用断言钉住"改写行
  字节长度确与原文相等"，避免测试自身漂移。
  锚点：claude `.../provider-claude/tests/lifecycle.rs:204/229`；
  codex `.../provider-codex/tests/lifecycle.rs:209/238`（codex 的 id 测试名为
  `same_length_id_rewrite_changes_identity_without_stale_cache`）。

### 2.4 分叉 parent 边（fork）

- Claude 现状：`parentUuid` 逐条透传（`src/lib.rs:1044`），既有测试覆盖链式
  父边（单元 `parse_extracts_conversational_messages`：`src/lib.rs:1443`；
  golden `basic.jsonl` 含 sidechain 分支）与随机 parent 透传 property；但没有
  "兄弟共享父边"与"悬空父边"的显式断言。
- 处置：① 兄弟分叉（三条记录共享同一 `parentUuid`，其中一条
  `isSidechain=true`）：每条都保留同一父边、文件顺序即 seq 顺序，不折叠不重排；
  ② 悬空父边（分叉/续写文件从中间开始，父不在本文件）：原样保留给跨源解析，
  provider 绝不按"父是否在本文件出现"裁剪；空串 `parentUuid` 归一为 `None`
  （`src/lib.rs:1044` 的显式语义）。
- Codex：格式无 `parentUuid`，`parent_native_id: None` 恒定
  （`src/lib.rs:1057`）——provider 层 N/A。反向测试钉住：即使分叉文件保留了
  被复制的响应前缀（native id 重复出现）与 event_msg 镜像，也不得臆造父边/
  sidechain，镜像不得重复计数。
  锚点：claude `.../provider-claude/tests/lifecycle.rs:251/303`；
  codex `.../provider-codex/tests/lifecycle.rs:260/292`。

### 2.5 文件移动（move）

- provider 层 N/A：`parse`/`probe` 只接收字节，`ReadOnlySource` 也只有
  `len/open`（`crates/agent-session-grep-ports/src/lib.rs:1344`），adapter 结构上
  拿不到路径；身份是记录内 native id（claude `src/lib.rs:1041`，codex
  `src/lib.rs:1055`），不掺路径。
- 移动语义（locator remap、历史恒可检索、别名过期）在 store 层，证据：
  `crates/agent-session-grep-adapters-sqlite/src/relocation/tests.rs:468`
  （身份/locator/context 全保留）、`:918`（反向移动需新计划）、
  `:1159`（Unicode 根）、`:1183`（Windows 大小写/分隔符等价）；
  cli e2e `crates/agent-session-grep-cli/tests/e2e.rs:7121`
  （`relocate_preserves_native_identity_claims_resume_and_incremental_discovery`）。

### 2.6 SQLite WAL

- provider 层 N/A：Claude/Codex 源是 JSONL 文件，不读 SQLite。
- 既有证据（读侧，不在本任务写域）：`source_fs.rs:309` 的
  `sqlite_capture_reads_wal_and_detects_wal_only_changes_without_writing_source`
  ——WAL 帧逻辑捕获、WAL-only 变化触发 `SnapshotChanged`、捕获/校验全程不写源
  （含 checkpoint 由外部 writer 执行后逻辑源仍当前）；搬迁路径的 WAL 变体在
  `relocation/tests.rs:1579`、`:1618`。
- 口径提醒：SQLite/整文件 JSON 类 provider 的 `source_span=unsupported`
  由 `crates/agent-session-grep-cli/tests/provider_matrix.rs:433`
  （pinned golden 逐条反证 span 存在性）与文档守
  `provider_matrix.rs:1888`（"无文件内字节 span"条目集合 == capability.rs
  unsupported 集合）共同守护；本任务不改这两处结论。

## 3. offset 伪造专项（precision=unknown）

- 约定：无 span 源绝不伪造 byte offset——`MessageEvent.span` 保持 `None`，
  evidence DTO 落 `precision=unknown`（`application/src/evidence.rs:61`；
  守护测试 `missing_placement_span_is_unknown_without_losing_document_identity`，
  `evidence.rs:147`；Byte 正例 `exact_placement_span_yields_byte_precision`，
  `evidence.rs:123`）。
- claude/codex 恒有真实字节 span（`claude src/lib.rs:1050`、`codex src/lib.rs:1067`），
  属 Byte 精度一侧；本 wave 未改动，也未给任何无 span 源补造 offset。
- 本次验证（2026-10-06 实跑，isolated target
  `C:/AgentSessions/.trellis/.runtime/target-b5`）：
  `cargo test -p agent-session-grep-application --lib evidence` → 13 passed
  （含 `missing_placement_span_is_unknown_without_losing_document_identity`、
  `exact_placement_span_yields_byte_precision`）；
  `cargo test -p agent-session-grep-cli --test provider_matrix` → 35 passed
  （含 `capability_source_span_claim_matches_pinned_golden_span_presence`、
  `matrix_no_span_limitation_bullets_list_exactly_the_unsupported_providers`）。
- 不完整解析的失败语义（PRD R4：不得推进成功水位/覆盖 last-good）属
  adapters-sqlite 写域，回归锁已有（`tests/invariant_failure_semantics.rs`，
  B4 并行），本任务只读引用，未改。

## 4. 门禁与验证（本 wave 实跑，2026-10-06）

    cargo fmt -p agent-session-grep-provider-claude -p agent-session-grep-provider-codex -- --check   # 绿
    cargo clippy -p agent-session-grep-provider-claude -p agent-session-grep-provider-codex --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b5 -- -D warnings   # 绿
    cargo test -p agent-session-grep-provider-claude -p agent-session-grep-provider-codex --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b5   # 绿（新增：lifecycle 8+8；properties 合计 6+2，本 wave 各 +1 条 append property）
    cargo test --workspace --locked --no-fail-fast --target-dir C:/AgentSessions/.trellis/.runtime/target-b5   # 终态 exit 0

- 报告口径：本任务写域（两个 provider crate + 两份产品文档 + 本 research/）全绿。
- 独立复核（trellis-check，2026-10-07，isolated `C:/AgentSessions/.trellis/.runtime/target-check-b5`）：上述命令块复跑一致——provider 两个 crate 全绿（claude lifecycle 8 / properties 6、codex lifecycle 8 / properties 2）；`provider_matrix` 35 passed；`application --lib evidence` 13 passed；`cargo fmt --all --check` 绿；workspace clippy `--all-targets -D warnings` 绿；`cargo test --workspace --no-fail-fast` 1811 passed / 0 failed / 20 ignored，exit 0。复核同时给 claude `prop_append_keeps_prefix_byte_stable` 补上 `skipped` 不变断言（与 codex 对称），并修正本文件 §2.2/§2.5 中随并行 e2e 编辑漂移的 `e2e.rs` 锚点（改为当前行号 + 测试函数名）。
- 并行边界如实记录（不属于本任务、未改动）：
  - `cargo fmt --all --check`（workspace 级）当时仍被并行 B4 在改的
    `crates/agent-session-grep-adapters-sqlite/src/lib.rs` 与
    `crates/agent-session-grep-cli/src/lib.rs` 两个文件挡住（fmt 差异均不在本
    任务文件）。
  - `cargo clippy --workspace` 当时唯一失败为
    `crates/agent-session-grep-application/src/ranking.rs:75` 的
    `neg_cmp_op_on_partial_ord`（`!(relevance > 0.0)`，B4 在写的证据门新行）；
    application 属本任务只读域，未动，转交 B4/main。
  - 首次 `cargo test --workspace` 曾出现 1 个失败
    `semantic_evidence_floor_env_override_is_explicit_and_validated`
    （B4 在 `crates/agent-session-grep-cli/tests/e2e.rs` 新增的语义阈值用例；该用例与 B4 正在写入的 ranking 证据门联动）；B4 修复落地后
    `--no-fail-fast` 复跑全绿、exit 0。故该失败与本任务改动无关，最终态已闭合。

## 5. 残余风险 / 边界

- 六类均为**合成 fixture** 证据；真实语料的增量/分叉行为仍依赖既有授权全量
  回归（`docs/evidence/integration-beta/real-data-regression.md`），本任务未新跑。
- native resume 未执行（环境限制），矩阵文档按 PRD 保持"未验证/不做声明"口径。
- fork 场景仅覆盖"文件内父边透传"；跨源父边解析（同一父出现在多个源）由
  store/application 层负责，其超集证据在 domain thread 测试与 e2e，不在本任务
  写域。
- append property 以"封口后追加"建模；真实写者若在无行尾的残行后直接追加
  （不补 LF），属源损坏场景，provider 按坏行 recoverable skip 处理——store 层
  撕裂尾保留策略（`e2e.rs:2201`）不依赖 provider 猜测。
