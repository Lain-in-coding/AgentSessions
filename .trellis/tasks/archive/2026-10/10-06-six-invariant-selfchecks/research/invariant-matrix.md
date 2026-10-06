# 六项不变量自查矩阵（D3）

> 任务：`.trellis/tasks/10-06-six-invariant-selfchecks`（父任务 10-05 第三节·B）。
> 口径：本任务**只验证、只记录**；不改生产行为。新增测试均为合成数据、注入
> 时钟/显式 fixture，无网络、无真实用户数据。
> 行号锚点为 2026-10-06 工作树实测（CLI `src/lib.rs` 同时被 B1 热路径任务修改，
> 锚点以本文件所记行文本为准）。

## 1. 结论总表（6 行）

| # | 不变量 | 结论 | 证据（新增测试，优先） | 锚点（file:line） | 失败修复建议 |
|---|--------|------|------------------------|-------------------|--------------|
| 1 | cap 先于过滤（lexical/semantic/hybrid） | **通过** | `invariant_search_gates.rs::provider_filter_precedes_the_cap_in_lexical_semantic_and_hybrid_paths`；SQL 级既有测试 `filtered_predicates_apply_before_limit_in_one_statement`、`semantic_and_hybrid_apply_provider_time_repo_and_facets_before_limit` | 应用层：`application/src/lib.rs:1831-1888`（三路检索调用均带 `filters`）、`1808-1827`（取数窗口）；适配层：`adapters-sqlite/src/lib.rs:7973-7987`（MATCH+谓词+ORDER+LIMIT 同一语句）、`8037-8113`（谓词）、`8172-8230`（先过滤再 top-k） | 无（若回归：归属 B4 检索，把谓词拉回 LIMIT 之前） |
| 2 | 零证据升格（不得仅凭 recency/repo boost 入选） | **通过**（含 2 处边界记录） | `invariant_search_gates.rs::zero_evidence_recent_repo_candidate_is_not_admitted_by_boost` | 准入在检索端口：`adapters-sqlite/src/lib.rs:7994-7997`（FTS 命中恒正分）、`8201-8230`（语义 top-k）；重排只作用于已准入命中：`application/src/lib.rs:1915-1986`、`ranking.rs:118-143`、`ranking.rs:67-79` | 无当前缺陷。边界一：`final_score(0.0,*,*,true) = +CURRENT_REPO_SCORE_BOOST`（`ranking.rs:44,67-79`）——今天不可达（生产 FTS 命中分 > 0），B4 若引入新候选源需补证据门。边界二：语义 top-k 无相似度下限，0 相似度候选可在 hybrid 中以 RRF 名次分出现（`hybrid.rs:19-73`、`adapters-sqlite/src/lib.rs:8201-8230`）→ 建议归属 **B4** |
| 3 | 失败固化（失败不得推进水位/覆盖 last-good/标 current） | **通过** | `invariant_failure_semantics.rs::failed_source_commit_keeps_watermark_fingerprint_and_last_good_rows`；既有 `source_no_op_rejects_duplicate_facts_without_advancing_generation`、`recover_aborts_orphan_building_intents`、e2e `sync_truncated_tail_retains_previous_index_without_churn` | 先校验后单事务：`adapters-sqlite/src/lib.rs:3328-3396`、重复 id 拒绝 `969-990`；水位（`source_scans`）仅在提交事务内写：`6339`；失败时指纹不变经 `source_fingerprints`（`2914-2935`）断言 | 无（同族失败注入由 B2 journal 任务继续扩展） |
| 4 | 裁剪视图进缓存（裁剪不得持久化为"新鲜全量"） | **通过** | `invariant_failure_semantics.rs::budget_trimmed_response_is_not_persisted_as_a_fresh_full_view`、`invariant_failure_semantics.rs::interrupted_intent_never_becomes_a_fresh_full_view_after_reopen`；CLI 侧既有 `sync_truncated_tail_retains_previous_index_without_churn`（`cli/tests/e2e.rs:2186`） | durable outbox 两阶段：`adapters-sqlite/src/lib.rs:5336,5402`；写打开收敛 orphan intent：`1551-1577`、`6977-6993`；响应裁剪只在装配期：`application/src/lib.rs:2092-2093`、`budget.rs:127-152`；discover 仅完整扫描才合成 tombstone：`cli/src/lib.rs:3762-3766`；截断尾 Retain：`cli/src/lib.rs:4672` | 无。边界：CLI 级"部分 root scan 不改新鲜度/不 tombstone"本轮未新增 e2e（既有 e2e 已覆盖截断尾 Retain、完整 discover tombstone 与部分扫描不 tombstone），如需加强归 **B2/B7** |
| 5 | resume 全参数化（typed intent + argv/cwd 分离，无 shell 拼接） | **通过** | `invariant_resume_args.rs::descriptor_keeps_argv_and_cwd_separate_and_refuses_unsafe_display`、`invariant_resume_args.rs::resume_executes_adversarial_session_id_as_one_argv_without_a_shell` | typed intent：`application/src/resume.rs:17-30,46`；执行层 argv+cwd 直启：`cli/src/lib.rs:2803-2806`；display-only 引用/拒绝：`resume.rs:196-232` | 无（若回归：归属 B5 provider/resume） |
| 6 | 投影截断共病（不得共用截断投影还声称全文可检索） | **通过**（含 1 处文档措辞建议） | `invariant_projection_consistency.rs::capped_projection_is_identical_across_write_and_rebuild_paths`、`invariant_projection_consistency.rs::app_get_returns_full_body_while_search_stays_inside_the_projection`；既有 `message_fts_body_is_capped_at_char_boundary_on_index`、`catalog_put_and_rebuild_project_capped_fts_body` | 有界 FTS 投影：`adapters-sqlite/src/lib.rs:168-200`、`application/src/retention.rs:20-28`；catalog 全文不改写（同上测试断言 `get`）；rebuild 同投影：`adapters-sqlite/src/lib.rs:6882`；CLI 以有界文本构造三元组：`cli/src/lib.rs:4253`（B1 并发修改后实测行号） | 无行为缺陷。边界：README 的 “full-text search” 未带 16k 限定（`README.md:7,24`），而 release 记录已披露 16,000 chars/message（`docs/release/go-no-go.2026-08-16.md:631`）→ 归属 **B0（文档口径）/B4（检索声明）** |

## 2. 复现命令与原始日志

隔离 target（必须；默认 `target/` 可能含陈旧缓存假错）：

```
cargo test -p agent-session-grep-application --test invariant_search_gates --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-invariants
cargo test -p agent-session-grep-adapters-sqlite --test invariant_failure_semantics --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-invariants
cargo test -p agent-session-grep-adapters-sqlite --test invariant_projection_consistency --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-invariants
cargo test -p agent-session-grep-cli --test invariant_resume_args --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-invariants
cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-invariants
```

原始日志（本目录）：

- `focused-invariant-tests.log` —— 四个新增测试文件，9 个测试全绿
  （2 + 3 + 2 + 2），无 warning。
- `workspace-tests-isolated.log` —— 全工作区 85 个套件、1768 passed / 0 failed
  （exit 0），其中包含四个新测试与全部支撑锚点测试
  （`filtered_predicates_apply_before_limit_in_one_statement`、
  `semantic_and_hybrid_apply_provider_time_repo_and_facets_before_limit`、
  `sync_truncated_tail_retains_previous_index_without_churn`、
  `message_fts_body_is_capped_at_char_boundary_on_index`、
  `catalog_put_and_rebuild_project_capped_fts_body`、
  `recover_aborts_orphan_building_intents`、
  `source_no_op_rejects_duplicate_facts_without_advancing_generation`）。

## 3. 逐项证据说明

### 1）cap 先于过滤 —— 通过
- 应用层把 provider/time/repo/facet 谓词随检索调用传入端口，取数窗口
  （`RANK_SCAN_WINDOW` / `offset+page+1`）与重排都作用在"过滤后集合"上；
  新测试合成 40 条域外高排名（9.0）+ 2 条域内低排名（1.0）命中，`limit=2`，
  断言 lexical/semantic/hybrid 三路都只返回域内命中，且端口调用携带 provider
  谓词；若应用层漏传谓词，fake 索引会按全局 top-2 返回域外命中，测试立即失败。
- SQL 级 pushdown 由适配层既有测试直接锚定（谓词与 `LIMIT` 同一语句；
  semantic 先 SQL 过滤再堆 top-k）。本轮未发现"先 cap 后过滤"路径。

### 2）零证据升格 —— 通过（边界见下）
- 准入（结果是否存在）由检索端口决定：FTS `MATCH` 命中分恒正（
  `1/(60+rank)`），语义为向量 top-k；`apply_lexical_signals` 只对已准入命中
  重算 `final = max(0, relevance×decay − sidechain + repo_boost)` 并排序，
  **没有新增候选的代码路径**。
- 新测试构造"最新时间 + 当前 repo + 不含查询词"的诱饵消息，注入固定时钟与
  当前 repo slug：诱饵从未进入结果；同文本的两条命中按近因排序（证明 boost
  确实生效、测试非空转）；`final_score` 直断言同样只放大已准入证据。
- 边界（不在本任务修复，归 B4）：
  a) `final_score` 对 `relevance=0 && in_current_repo` 的输入会输出
     `+CURRENT_REPO_SCORE_BOOST`（`ranking.rs:67-79`）；生产检索今天不可能
     传入 0 分候选，但新增候选源（ANN 平局、外部 rerank）前必须复核。
  b) `query_semantic_filtered` 的 top-k 无相似度下限，0 相似度向量可占
     top-k 名额；hybrid 的 RRF 按名次给正分（`hybrid.rs:19-73`）。这不属于
     "recency/repo boost 升格"，但若 B4 对外声称"语义证据"需设阈值。

### 3）失败固化 —— 通过
- 批量提交是"先全量校验（重复 id/identity/placement/edge/claim）→ 再单
  事务激活"：失败发生在任何写入之前；成功路径才在事务内写 `source_scans`
  （水位/指纹）。
- 新测试先提交 last-good（指纹 `fp-v1`、generation=1），再注入"更新载荷 +
  新指纹 `fp-v2` + 重复实体 id"的失败批次：断言 generation、`source_scans`
  指纹、catalog payload、FTS 检索结果全部保持 last-good（新词不可检索、旧词
  仍可检索）。CLI 截断尾 Retain（解析失败）由既有 e2e 覆盖：不重 parse、
  不推进 generation、不 tombstone。

### 4）裁剪视图进缓存 —— 通过
- 持久化层只接受**完整批次原子激活**：durable outbox 的 `building` intent 在
  下次写打开被标 `aborted`（`interrupted_before_activation`），不改变
  generation/catalog/FTS/指纹——"半成品/裁剪视图"不会变成"新鲜全量"。
- 展示/预算裁剪只发生在响应装配（`clamp_items` 字节闸）：新测试用 4096 字节
  预算触发 `max_response_bytes` 截断，断言返回页被裁剪且 `next_cursor` 仍可
  达余下命中；水位、`source_scans` 指纹与 generation 不变；重开库后 3 条命中
  全部可检索、payload 完整。
- CLI 侧（支撑证据）：`sync --discover` 仅在 provider 扫描完整时才合成
  tombstone，部分扫描的批次 `relation_complete=false`（不推导删除）；截断尾
  已索引源 Retain 旧索引。AGF-03 的"max_sessions 裁剪进缓存"形态在 ASG 没有
  对应写路径。

### 5）resume 全参数化 —— 通过
- `ResumeDescriptor { provider_binary, args: Vec<String>, working_directory,
  permission_mode }` 是 typed intent；执行层
  `Command::new(binary).args(&args).current_dir(dir)` 直接 spawn，无 shell。
- 新测试 A（纯函数）：含 `$`/`"` 的 provider session id 保持为 args 中的
  独立元素、cwd 独立字段；dry-run 展示串（另一条路径）被整体拒绝（null）。
- 新测试 B（真实 CLI + 测试专用 fake provider）：fixture `sessionId =
  "ccdd&echo>asg-resume-pwned.txt"`（无空白，可通过 identity 校验；`&`/`>`
  在 POSIX shell 与 cmd.exe 都会断句）；首次 `--yes` 强制预览、第二次真实
  执行后断言 argv 逐字为 `--resume ccdd&echo>asg-resume-pwned.txt`、cwd 为
  原会话目录、且 cwd 内**没有**出现 `asg-resume-pwned.txt` 痕迹文件——任何
  shell 字符串拼接都会留下该文件。

### 6）投影截断共病 —— 通过（文档措辞建议见下）
- ASG 只有 FTS 投影有界（单条消息 16,000 字符）；catalog 保留 provider 原文
  全文，`get`/`show`（及 context/handoff 装配）从 catalog 读取；产品当前无独立导出命令——**没有**索引/展示/导出共用截断
  投影。写入路径（CLI 用 `bounded_index_text` 构造三元组）与 rebuild 路径
  （从 catalog payload 重投影）产出同一有界结果，current 判定两侧不分叉。
- 新测试：16k 之后的词不可检索、16k 之内的词可检索；`rebuild_index` 后结论
  不变；`get`（App 层）仍返回含超界词的全文。行为符合"有界投影"的诚实语义。
- 文档边界：README 的 "full-text search" 未带 16k 限定（release 记录已披露
  16,000 chars/message）。建议 B0/B4 在用户可见文案补一句"单条消息索引上限
  16,000 字符、catalog 保留全文"，不涉及行为修改。

## 4. 未决/边界清单（供主会话建后续任务）

| 边界 | 归属 | 建议 |
|------|------|------|
| `final_score` 对 0 relevance + repo boost 输出正分（今日不可达） | B4 | 为将来候选源保留证据门；或在 `final_score` 首行 `if relevance <= 0.0 { return 0.0 }`（需产品拍板） |
| 语义 top-k 无相似度下限，hybrid RRF 可给 0 相似度候选正分 | B4 | 引入相似度下限或显式标注"按名次融合"的语义 |
| README "full-text" 措辞未带 16k 限定 | B0（文档）/B4 | 补限定说明，不改行为 |
| CLI 级"部分 root scan 不 tombstone/不改新鲜度"无新增 e2e | B2/B7 | 若要加，用合成 provider root + 权限失败注入补测 |

