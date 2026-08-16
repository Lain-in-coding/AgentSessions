# Beta 晋级审计：claude-code 与 codex（只读核查）

- **Query**: 核查 claude-code 与 codex 是否满足 RFC-0002 §6 的 Beta 晋级门槛
- **Scope**: internal（代码 + 文档证据核查，只读）
- **Date**: 2026-08-16
- **审计基线**: main 分支 @ `a4309bd`（Windows，本机 checkout）。并行 provider checkout 的 `capability.rs`/matrix 文档与 main 有偏差（见 §0）；一切证据以 main 为准。
- **权威门槛来源**: `docs/architecture/RFC-0002-provider-adapter-contract.md:90-94`（§6）

## 0. 审计环境警示（必须先说明）

| 发现 | 证据 |
|---|---|
| 当前审计 checkout 不是 main | `git worktree list`；目标证据以 main 为准 |
| 并行 checkout 的 `capability.rs`/`lib.rs`/matrix 文档与 main **分歧**（少了 164/38 行，矩阵文档差 69 行） | `git diff --stat main <parallel-checkout> -- crates/...` |
| **审计目标（Beta 晋级证据）在 main 上**，故全部证据行号引自仓库相对路径 | `git log main -3` = a4309bd |

`docs/product/PROVIDER-MATURITY-MATRIX.md`（main:6）自记 "最后更新 2026-08-16"。

## 1. RFC-0002 §6 门槛的权威分解（读原文后逐条）

§6 原文（main `RFC-0002-provider-adapter-contract.md:90-94`）：

> 字段能力：`native | derived | partial | unsupported | unknown`。Provider 整体成熟度：`Certified/GA | Beta | Experimental | Unsupported`，独立于字段能力，晋级必须有证据（**fixture + 共享 contract + 跨 target + 回滚策略**），不由代码存在自动推断。
> `AdapterManifest` 必须声明：`provider_id`、支持的版本/variant 范围、maturity、能力矩阵、**fixture revision**、**最后认证 target**、已知限制。

`capability.rs:15` 的 Beta 注释复述为「主路径 fixture、golden、contract、只读、增量、source span 均通过」。

把两条权威来源合并，Beta 需要逐项核查的清单（比用户列的 6 项多出的项在 §7 标注）：

1. 主路径 fixture
2. golden（pinned canonical 输出）
3. contract（probe 歧义拒绝 / parse 流式 / 只读约束 / staging 原子性）
4. 只读（绝不改 provider 源）
5. 增量（同文件重 ingest 幂等 / tombstone 推导）
6. source span（span 回切源字节）
7. **跨 target（晋级证据要求之一）** — §7 展开
8. **回滚策略（晋级证据要求之一）** — §7 展开
9. **AdapterManifest / fixture_revision / 最后认证 target 声明** — §7 展开

---

## 2. claude-code 逐项核查

| # | 门槛 | 判定 | 证据（文件:行 + 测试名） |
|---|------|------|--------------------------|
| 2.1 | 主路径 fixture | **满足** | `tests/golden/basic.jsonl`（8 行，2223 B，纯合成，PROVENANCE.md:6-28 逐行覆盖声明）；`.gitattributes:4` `-text` 钉字节 |
| 2.2 | golden（pinned canonical JSON 比对） | **满足** | `tests/golden.rs:114` `golden_canonical_output_is_pinned` — 构造 canonical JSON（含 `fixture_blake3`/`session_native_id`/`committed`/`skipped`/每消息 `span`），与 `basic.expected.json` 做 `serde_json::Value` 结构化相等断言；`basic.expected.json:1` pin 了 fixture 的 BLAKE3（`353c76...`）。**已在本机 `cargo test` 跑绿** |
| 2.3 | contract — probe 歧义拒绝 | **满足** | `src/lib.rs:436,477` 两处 `ProviderError::AmbiguousVariant`；测试 `src/lib.rs:779,787,803,856`（`probe_rejects_non_jsonl`/`probe_rejects_empty`/`probe_distinguishes_shape_mismatch_from_bad_json`/`probe_rejects_broken_lines_beyond_tolerance_with_line_numbers`）。共享 orchestration 层 `select_and_stage` 拒绝 ambiguous 见 `agent-session-grep-application/src/lib.rs:438-441` + 测试 `application/src/lib.rs:5243` `stage_rejects_ambiguous_variant` |
| 2.4 | contract — parse 流式产出 | **满足（流式 sink 契约）/ 部分满足（真 bounded）** | trait `ports/src/lib.rs:809-817` 要求 parse 推入 `CanonicalEventSink` 流式产出；claude parse 逐行 `split_inclusive` 遍历（`src/lib.rs:551`），probe 用 `SAMPLE_LINE_LIMIT=16` 采样（`src/lib.rs:18,425-433`）。**但**: parse 入口收的是**整文件字节**（`ingest_file` 先 `read_to_end`，`adapters-sqlite/src/source_fs.rs:38`），StagingSink 又把每条消息 buffer 进 `Vec`（`application/src/lib.rs:532-549`）。RFC-0002 §7「禁止整体加载大型 transcript，使用 bounded buffer」在**生产 ingest 路径**上并未真正落地 bounded buffer——这是 contract 层的真实缺口，非文档虚标但属隐性违背 |
| 2.5 | 只读约束 | **部分满足** | adapter 实现自身**无任何 `std::fs`/`File`/`OpenOptions`/`std::env` 引用**（grep 0 命中），parse 只收 `&[u8]`——adapter 层结构上无法写源。**但是**: ① testkit 的 `assert_read_only`（`agent-session-grep-testkit/src/lib.rs:511`）**从未被 claude/codex 测试调用**（grep 全仓唯一调用点是 testkit 自测 `:809`）；② claude crate 的 dev-dependencies 只有 `blake3`，**根本不依赖 testkit**（`Cargo.toml:15-16`）；③ RFC-0002 §7 要求的「扫描前后 checksum 断言守护」**没有对应测试**（e2e 里 grep 不到任何 sync 前后读 fixture 字节/mtime 比对的断言）。真实回归 `scripts/evidence/real_data_regression.py:373` 注释自称 "read-only regression"，但其六条不变量（`:46-52`）**不含任何源文件 checksum 前后比对**——"read-only" 只是注释声明，不是机器断言 |
| 2.6 | 增量 — 幂等 / tombstone | **满足（e2e 覆盖，但全是 claude fixture）** | `cli/tests/e2e.rs:1302` `sync_commits_then_reports_unchanged_on_resync`（重 sync → `committed:0`/`unchanged:2`/generation 不推进）；`:1417` `sync_tombstones_message_removed_from_source`（源收缩 → 消息被 tombstone 且不可检索）；`:1485` `sync_empty_source_tombstones_all_messages`；`:1532` `sync_truncated_tail_retains_previous_index_without_churn`；`:4057` `sync_discover_tombstones_removed_source_on_complete_scan`。机制：CLI 指纹缓存快路径（`main.rs:2875-2899`）+ `commit_source_batches_if_changed`（`main.rs:2706`）+ `verify_snapshot` 提交前复核（`source_fs.rs:56-87`） |
| 2.7 | source span round-trip | **满足** | 三层证据：① golden `tests/golden.rs:142` `golden_spans_slice_back_to_exact_source_lines`（span.start 落在某行首、`end-start==行字节长`、切片逐字节等于源行、且源行 `uuid==native_id`）；② property `tests/properties.rs:410` `prop_span_roundtrips_to_source_record`（64 种子 × Unicode/CRLF/大字段，span 切片==ground-truth 行）；③ 单元 `src/lib.rs:1216,1232` `parse_reports_span_roundtripping_to_source_line`/`_on_crlf_lines`；④ e2e `cli/tests/e2e.rs:567` `ingest_persists_session_and_document_entities_with_spans`（show 返回 span 切回真实源文件字节==原始行） |

**claude-code 结论：6 项硬门槛里 5 项满足、1 项（只读的运行时 checksum 断言）部分满足。**
**阻碍不是代码缺失，而是 §7 的晋级流程证据。**

---

## 3. codex 逐项核查

| # | 门槛 | 判定 | 证据（文件:行 + 测试名） |
|---|------|------|--------------------------|
| 3.1 | 主路径 fixture | **满足** | `tests/golden/basic.jsonl`（9 行，1853 B，纯合成，PROVENANCE.md:24-44 逐行覆盖，含 session_meta/turn_context/event_msg 镜像/response_item/截断行）；`.gitattributes:4` `-text` |
| 3.2 | golden（pinned canonical JSON 比对） | **满足** | `tests/golden.rs:93` `golden_basic_matches_pinned_canonical_output` — 先验 BLAKE3 指纹（`basic.expected.json:2` pin `f0a53...`），再构造同形 canonical JSON 做结构化相等断言。**已在本机 `cargo test` 跑绿** |
| 3.3 | contract — probe 歧义拒绝 | **满足** | `src/lib.rs:317,362,378` 三处 `AmbiguousVariant`；测试 `src/lib.rs:664,672,678,715`（`probe_rejects_non_jsonl`/`probe_rejects_empty`/`probe_rejects_claude_code_jsonl`/`probe_rejects_broken_lines_beyond_tolerance_with_line_numbers`）。`probe_rejects_claude_code_jsonl`（`:678`）证明不越界解析 claude 格式 |
| 3.4 | contract — parse 流式产出 | **满足（流式 sink 契约）/ 部分满足（真 bounded）** | 同 claude：trait 流式 sink 满足（`ports/src/lib.rs:809-817`），parse 逐行遍历（`src/lib.rs:450`）。但同样收整文件字节（`ingest_file`），真实大型 rollout 无 bounded buffer。镜像去重是 codex 关键：`src/lib.rs:527-535` 只认 `response_item`+`message`，跳过 `event_msg`；测试 `src/lib.rs:810` `parse_takes_authoritative_message_ignores_event_mirror` |
| 3.5 | 只读约束 | **部分满足** | 与 claude 完全同构：adapter 无 fs/env 引用（grep 0 命中）；`assert_read_only` 未被调用；codex crate dev-dependencies 只有 `blake3`（`Cargo.toml:15-16`）；无扫描前后 checksum 断言测试 |
| 3.6 | 增量 — 幂等 / tombstone | **部分满足（机制通用但 e2e 直接证据只用 claude fixture）** | 增量逻辑在 CLI/adapter 通用层（指纹缓存 + `commit_source_batches_if_changed` + `verify_snapshot`），与 provider 无关，理论上对 codex 同样成立。codex 专属 e2e 只有 `cli/tests/e2e.rs:664` `ingest_auto_selects_codex_and_ignores_event_mirror`（probe-select + 镜像去重 + show/search），**该测试不含 resync/tombstone**。所有幂等/tombstone e2e（`:1302,1417,1485,1532,4057`）写的都是 claude 形态 fixture。**capability.rs:159 标 `incremental: Native`，但 codex 侧没有一条直接的重 ingest/tombstone 测试** —— 属「机制可信但直接证据缺失」 |
| 3.7 | source span round-trip | **满足** | 三层证据：① golden `tests/golden.rs:132` `golden_basic_spans_slice_back_to_source_envelope_lines`（span 落在行首、切片==源行、且该行 `type=="response_item"`、`payload.type=="message"`、`payload.id==native_id`——证明指向权威封套而非 event_msg 镜像）；② property `tests/properties.rs:400-407` 性质 2（64 种子 span 回切==ground-truth 封套行）；③ 单元 `src/lib.rs:979,996` `parse_reports_span_roundtripping_to_source_line`/`_on_crlf_lines` |

**codex 结论：6 项硬门槛里 5 项满足、1 项（只读运行时 checksum 断言）部分满足；增量项为部分满足（通用机制可信但 codex 直接 e2e 证据缺失）。**

---

## 4. 「文档虚标」专项核查

| 文档声称 | 代码实况 | 判定 |
|---|---|---|
| `PROVIDER-MATURITY-MATRIX.md:27-28` 两行证据栏写「golden + 确定性 property 套件 + span round-trip」 | golden/property/span 测试**真实存在且跑绿**（§2.2/2.7、§3.2/3.7） | **不虚标**，属实 |
| `PROVIDER-MATURITY-MATRIX.md:45-47` 「关系化 Message/Placement/Edge 的合成与 e2e 证据已入库……剩余 blocker 仍为授权真实数据全量绿色回归与跨 target CI 认证」 | 真实数据回归 `docs/evidence/integration-beta/real-data-regression.md:29-114` 记录**三次全量授权运行六不变量全绿**（08-09/08-12/08-13），即「授权真实数据全量绿色回归」**其实已闭合**；文档自己在 `:99-118` 缺口清单第 4 条也划掉了它。**这句「剩余 blocker 仍含授权真实数据回归」与自身第 4 条矛盾，属过时/虚标表述** | **部分虚标（表述自相矛盾）** |
| `capability.rs:159,174` codex/claude `incremental: Native` | claude 有直接 e2e；**codex 无直接 resync/tombstone e2e**（§3.6） | codex 的 `Native` 标注证据不足，**属能力标注略超前于直接证据** |
| 真实回归自称 "read-only regression"（`real_data_regression.py:373`；`real-data-regression.md:18`「Sources remained read-only」） | 六条不变量**无一条**对源文件做 checksum 前后比对（`real_data_regression.py:46-52`）；`sha256_of` 只对 binary 本身哈希（`:158-163`） | **「read-only」是无机器断言支撑的声明，属隐性虚标** |

---

## 5. RFC-0002 §6 里用户未列到的 Beta 要求（逐项核查）

§6 晋级证据原文是「fixture + 共享 contract + 跨 target + 回滚策略」+ manifest 声明。用户列了 fixture/golden/contract/只读/增量/span，**没列以下三项**：

### 5.1 跨 target（晋级硬性证据要求之一）

- **现状**：`core-beta-evidence.yml`（`.github/workflows/core-beta-evidence.yml:34-65`）配置了 4 个 runner/target（windows-2022、ubuntu-22.04、macos-15-intel、macos-15），但 **step 只跑 `-p agent-session-grep-adapters-sqlite` 测试与 storage spikes**（`:119-169`），**不含 claude/codex provider 的 golden/property**。`ci.yml:11-46` 的 `test` job 是三平台 `cargo test --workspace`（含 provider 测试），但：
  - `docs/operations/core-beta-evidence-matrix.md:38` 把 `IB-CI-INSTALLER-001` 记为 **`ci_configured_only`**（"has not yet produced a named successful run"）；
  - 没有任何一行把「claude/codex golden/property 在三平台跑绿」记为 `ci_verified`。
- **判定：不满足（证据缺口）**。这正是 `PROVIDER-MATURITY-MATRIX.md:119-122` 缺口清单第 5 条自认的 blocker，文档此处**不虚标、属实**。

### 5.2 回滚策略（晋级硬性证据要求之一）

- **现状**：grep 全 docs 找不到针对 provider 的「回滚策略」文档（`回滚策略`/`rollback strategy`/`demote` 无命中）。`capability.rs` 只声明 maturity，无降级/回滚机制；CLI 入口（`main.rs`/`mcp.rs:846-851`）渲染 provider 时**只输出 `id`，不读 capability.rs、不输出 maturity**——即使把某 provider 降级，入口层也无从体现。
- **判定：不满足（文档与机制双缺）**。

### 5.3 AdapterManifest / fixture_revision / 最后认证 target 声明

- **现状**：RFC-0002 §6 要求 `AdapterManifest` 声明 `provider_id`/variant 范围/maturity/能力矩阵/**fixture revision**/**最后认证 target**/已知限制。代码里 `ProviderAdapter` trait（`ports/src/lib.rs:799-818`）**只有 `provider_id`/`probe`/`parse` 三个方法，没有 `manifest()`**；`AdapterManifest` 类型**不存在**（grep 唯一命中是 RFC 文档 `:28` 与无关的 EmbeddingManifest）。fixture_revision 只存在于 PROVENANCE.md 文本（claude `:6`=1、codex `:3`=1），**不进任何结构化 manifest**。
- **判定：不满足（RFC 要求的最小 trait 字段未落地——但这是全 provider 共性的 contract 简化，ports trait 注释 `:798` 自述「首个切片从简」）**。

---

## 6. 总结论

| 维度 | claude-code | codex |
|---|---|---|
| 主路径 fixture | 满足 | 满足 |
| golden | 满足 | 满足 |
| contract（probe 拒绝/parse 流式/staging 原子） | 满足（bounded-buffer 除外，见 2.4） | 满足（bounded-buffer 除外，见 3.4） |
| 只读（运行时 checksum 断言） | **部分满足** | **部分满足** |
| 增量（幂等/tombstone 直接证据） | 满足 | **部分满足** |
| source span | 满足 | 满足 |
| 跨 target CI | **不满足（`ci_configured_only`）** | **不满足（`ci_configured_only`）** |
| 回滚策略 | **不满足（无文档无机制）** | **不满足（无文档无机制）** |
| AdapterManifest/fixture_revision/认证 target 声明 | **不满足（manifest 未落地）** | **不满足（manifest 未落地）** |

**能否晋级 Beta？—— 不能，且当前保持 Experimental 是正确决定。**

缺的不是单一代码测试，而是**晋级流程证据**，按 RFC-0002 §6 原文需补（按优先级）：

1. **跨 target CI 认证**（硬缺口）：把 claude/codex 的 golden+property 纳入三平台 CI 矩阵，跑出 named successful run，把 `IB-CI-INSTALLER-001` 从 `ci_configured_only` 提升到 `ci_verified`。这是 `PROVIDER-MATURITY-MATRIX.md:119-122` 唯一未划掉的 blocker，**文档此处属实**。
2. **回滚策略**：补一份 provider 级降级/回滚决策文档（docs/adr/ 或 RFC 增补），并让入口层能把 maturity 渲染出来（当前 CLI/MCP 根本不读 capability.rs）。
3. **AdapterManifest 结构化声明**：在 `ProviderAdapter` trait 补 `manifest()`（含 fixture_revision、最后认证 target、已知限制），或至少把 PROVENANCE.md 的 fixture_revision 提升为结构化字段。
4. **只读的运行时 checksum 断言**（把「部分满足」补满）：给 sync/ingest 增加「扫描前后对源文件做指纹比对」的 e2e 断言（testkit `assert_read_only` 已存在但闲置，接入 claude/codex 测试即可）；真实回归 harness 的六不变量里加一条「源文件 checksum 前后一致」。
5. **codex 增量的直接 e2e**：补一条 codex fixture 的 resync/tombstone 测试，把 `incremental: Native` 的标注坐实。
6. （隐性、非文档虚标）parse 真实大型 transcript 时的 bounded-buffer 落地（RFC-0002 §7 硬约束，当前生产路径整文件入内存）。

**对 `PROVIDER-MATURITY-MATRIX.md` 的修正建议（非代码改动）**：`:45-47` 的「剩余 blocker 仍为授权真实数据全量绿色回归」与自身 `:99-118` 第 4 条（已划掉、三次全绿）矛盾，应删去前者中的「授权真实数据全量绿色回归」，只保留「跨 target CI 认证」。
