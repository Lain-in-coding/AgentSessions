# 优化 50k changed-sync 提交路径设计

> 状态：in_progress（2026-10-15 已批准实施）。父任务：`08-15-open-source-product-roadmap`。
> 目标仓库：产品仓库 `Lain-in-coding/agent-session-grep`；实现与复测落点 =
> canonical checkout `./.trellis/.runtime/attribution-closeout-20261012/canonical-repaired`
> （main = `de2c95d2c93f`，含基线 harness 与证据）。协调根仓库只保存任务/研究/日志。

## 1. 边界、证据与数值门槛

- 优化对象：`crates/agent-session-grep-adapters-sqlite` 的 changed-sync 提交路径
  （`commit_source_batches_if_changed` → `commit_index_batch_with_relations`），
  不改测量协议、不改 CLI 行为契约、不动 noop 快路径。
- 证据链：基线 bundle `docs/evidence/core-beta/b0fc44d75690/`（S=`b0fc44d75690`）
  的三跑报告 + 独立 trace 试验 + 两份研究：
  `research/commit-path-hotspot-map.md`（阶段 → 代码 → 候选）、
  `research/candidate-optimizations-and-risks.md`（候选排序/风险/实验）、
  `research/50k-journal-row-diagnostic.md`（journal 行 48.6 MB 实测）、
  `research/correctness-contracts-and-verification.md`（不变量/测试面）、
  `research/baseline-protocol-and-thresholds.md`（协议与门槛算术）。
- R4 硬门槛（三跑 P50 中位数，相对 S）：
  - 50k `sync/initial`：10122.8974 ms → **必须 < 8604.462790 ms**；
  - 50k `sync/shrink`：9943.2103 ms → **必须 < 8451.728755 ms**；
  - 其余系列（noop/search 五类/show/get/context 两种/startup/2k/10k/anchor）
    不得回归超过各自 15%（数值清单见 thresholds 研究 §4）。
- 本机 control：研究已用冻结二进制 B 复现 initial 9,990 ms / shrink 10,227 ms，
  落在基线带宽内；若预算允许，在正式复测前再跑一次未优化源码的三跑作为
  同日 control，否则以冻结二进制 sanity + 公开基线对比并如实披露。

## 2. 候选与统一编号

本设计统一使用下表编号；与两份研究文件的对应关系：
（hotspot 文件用 C1–C10，candidates 文件用 C1–C6。）

| ID | 对应研究候选 | 改动点（代码位置以 canonical checkout 为准） | 预期收益（initial/shrink） | 风险 |
| --- | --- | --- | --- | --- |
| O1 | candidates C2；hotspot C1 | `affected_sessions` 收集批量化：循环内只收集 message id，循环外一次分块 IN 查询（`lib.rs` 约 6871-6912；helper 6461-6484 已分块） | 0.5-0.9 s / 0.6-0.9 s | 低 |
| O2 | candidates C3 | 去掉 `manifest` 阶段对 upserts 的重复身份校验（`validate_incoming` 已逐源校验同一 id 集；`lib.rs` 约 4428 与 3671-3716） | 0.2-0.25 s / 0.1-0.2 s | 低 |
| O3 | candidates C4；hotspot H2b | alias 重建批作用域化：`stored_placements_from`/`stored_edges_from` 全表装载改为按本批 fully-complete 实体候选集（`lib.rs` 约 5417/5549/5734-5762） | 0.2-0.4 s / 0.2-0.4 s | 中低 |
| O4 | candidates C5；hotspot C4 | 批量行数提升：先运行时确认 bundled SQLite 变量上限，再把 `BULK_INSERT_ROWS_PER_CHUNK` 提升到 256-512（含全部断言更新） | 0.1-0.3 s / 0.1-0.3 s | 低 |
| O5 | candidates C1；hotspot C7/C5 | journal 明细拆分（schema v20）：大明细列移入 `index_batch_details` 子表，父行变小；Phase B 只 UPDATE 小行，消除 ~48.6 MB（initial）/33.8 MB（shrink）整行重写 | 0.7-1.2 s / 0.4-0.8 s | 中高 |
| O6 | candidates C6；hotspot C8 | `manifest` 阶段逐项序列化 canonical JSON 数组、缓存排序键（保持最终 JSON 字符串、`operation_digest` 与存库字节不变；不做流式哈希） | 0.2-0.4 s | 中 |
| O7 | 首轮 G4 后的 relation-delete 走读 | 相邻同类型 Edge/Placement 删除按既有 `BATCH_IN_CHUNK=500` 分块，保留类型顺序和 Activity/Usage 单行路径 | 待真实 A/B 测量 | 中低 |

当前选择：O1/O2/O4/O6 已进入未合并实验 S2；O3、O5、O7 原型均经实测否决。
下文 §4 的 O5 仅保留设计历史，不授权执行 schema v20 迁移。首轮 G4 的
shrink 未达标，§9 的 O7 有界原型也未通过预设探索门；schema 继续为 19。
按 PRD 的“如实报告 + 保留现状”路径收尾，不发布性能成功结论、不合并 S2。

明确不做（研究已否决）：降低/关闭 `synchronous`、改 `journal_mode`、拆事务、
`wal_autocheckpoint` 调参、"过滤未变化 relation 写入"（shrink 身份重派生，
事实不成立）、跳过 alias/integrity 校验。

## 3. 决策门（先实验，后提交）

所有收益数字都是估算；进入任何提交的优化必须有私有实验（不入产品提交）的
before/after 支撑，且与候选 ID、trace 阶段、代码位置一一对应（AC1）。

- G0（E1/E4）：用私有 scratch 复现 50k initial/shrink，采 WAL 字节与
  `generation_activate`/`tx_commit` 分段；确认 bundled SQLite 的
  `SQLITE_LIMIT_VARIABLE_NUMBER` 实际值。产出决定 O4 的分块大小与 O5 的
  收益假设是否成立。
- G1（O1+O2 后）：私有 spot 测量（50k initial/shrink 各 1 次，同机同语料）。
  若单轮已显著低于门槛并留 ≥10% 余量，可直接进入 O3 收尾；否则继续。
- G2（O3+O4 后）：spot 测量。若两项场景均已低于门槛且余量 ≥5%，可评估
  跳过 O5；否则 O5 必做。
- G3（O5 原型 E2）：最小子表实现 + 读侧重组的 spot 测量；`tx_commit +
  generation_activate` 合计收益 <0.4 s 或崩溃恢复测试面无法收敛时，回退
  O5（保留 O1-O4），按 PRD 约束走"如实报告 + 后续建议"或补 O6。
- G4（正式复测）：三跑中位数满足 R4 才发布成功结论；未满足则不得宣称成功。

## 4. O5（schema v20）详细设计

- 结构：新增 `index_batch_details(operation_id TEXT PRIMARY KEY,
  detail_format TEXT NOT NULL, upsert_ids_json TEXT NOT NULL, delete_ids_json TEXT NOT NULL,
  relation_upserts_json TEXT NOT NULL, relation_deletes_json TEXT NOT NULL,
  source_replacements_json TEXT NOT NULL)`。
  `index_batches` 保留小列（state/durable_point/时间戳/error/summary 等）并
  作为状态权威；明细权威 = 子表（存在子表行时），否则回退读父行旧列
  （v19 历史行兼容，双格式读）。五个可聚合字段与既有
  `JOURNAL_COMPACTION_FIELDS` 一致；`delete_ids_json` 必须包含在内。
  `relocation_json` 与 `detail_summary_json` 继续由父行持有，保持永久保留契约。
- 写路径：Phase A（intent INSERT）同时写父行（小）与子表（大，一次）；
  Phase B 的 activation/错误更新只 UPDATE 父行；不再触碰明细。
- 读路径：`verify_pending_in_tx` 从子表（或旧父行列）读回明细，与调用方
  manifest 逐字段比较，语义与字节口径保持；`recover_interrupted` 逻辑不变。
- 维护面：journal retention（B2）compaction 对新旧格式都要正确（`full`
  明细在子表时对子表做替换/聚合，`aggregated_v1` 行不聚合的规则不变，
  未决行永不聚合，CAS apply 保持）；维护接口读明细时经统一读取层。
- 迁移：`SCHEMA_VERSION 19 → 20`，additive `migrate_v19_to_v20`：建子表 +
  将既有行的 5 个明细列复制进子表（`detail_format` 沿用父行值），父行旧列
  保留为 legacy（只读、不再写新值、写入侧用默认值满足 NOT NULL）；同一事务，
  最后写 `PRAGMA user_version`；旧二进制拒绝 v20（既有策略）。
- 回滚：任务内回滚 = revert 代码；不提供降级写。已生成的 v20 库由新二进制
  继续读写；回滚后如需访问该库，用保留的迁移测试 fixture 验证过的升级路径
  或重建，不做破坏性降级。

## 5. 正确性契约（不得破坏；来源 correctness-contracts 研究）

- Catalog 权威、FTS/session 投影可重建；两阶段 outbox（durable intent →
  单事务 apply）；generation CAS 与 activation 同事务；durable intent
  逐字段一致（tamper 必须 fail-closed）；relation/tombstone 语义；
  跨源 alias 合并规则；身份保真（`fts_ids.id_json`）；resume claims
  fail-closed；journal retention 全部规则；迁移 additive、按
  `user_version` 门控、不得 drop/recreate；源文件只读；多批/单批等价。
- noop 快路径（`sources_are_current` 短路）与 `sources=0` 行为不得改变。
- CLI/Robot/MCP 输出与退出码语义不变；全部改动限定在 adapter 提交路径与其
  测试；如需触碰 application/cli 层，先在计划评审中扩范围。

## 6. 验证设计

1. 等价 oracle（私有脚本，不入产品提交）：同一合成语料（至少 250×200 与
   1×20 两种）分别由冻结二进制 B（b0fc44d）与新二进制执行
   initial→noop→shrink；对全部用户表做排序 dump 哈希比对。FTS 内部 rowid
   可随分块/重建改变，跨侧按完整 wire-ID→identity→FTS id/text 连接比对，并
   断言无孤儿/重复/缺失；原始 rowid 留在私有证据，同侧 noop 仍须原值不变。
   补 200 条同 ID 全量改写用例覆盖这一物理地址差异。时间列
   仅明确列出的运行时记账时间（如扫描/提交/创建时间）可归一化，
   不得按 `*_ms` 后缀抹去任意领域字段；随机标识采用一一对应映射，
   不得将不同 namespace 或 operation 合并。单侧 immediate noop 前后状态
   必须精确相等，operation digest 应独立核验其对应的原始明细，
   `active_generation` 必须一致；差异即失败。O5 变更额外验证：新旧格式
   journal 行可读、recovery 与 retention 流程通过。
2. 代码质量门：`cargo fmt --all --check`、
   `cargo clippy --workspace --all-targets -- -D warnings`、
   `cargo test --workspace`；adapter 的 `journal_retention`/
   `invariant_failure_semantics`/迁移/outbox-CAS/tamper 测试全绿；
   CLI `e2e`（sync/noop/shrink/tombstone、multi-session）全绿。
3. 正式复测（R3）：在每轮冻结的新源码上按基线协议跑三跑（首次 S2，O7 后 S3）
   `--baseline-run 1..3`（run 1 构建、run 2/3 复用收据；suite ≤3600 s、
   child ≤300 s），`validate-report` ×3 + `validate-baseline` ×1；
   trace 试验在 run 3 后独立进行；环境字段与基线同日一致（磁盘/Defender/
   工具链/SQLite），不一致须披露。
4. 判定：以三跑 P50 中位数对 R4 门槛判定；同时报告全部系列的跨轮 spread
   与原始样本完整性；失败尝试保留并披露。
5. 治理：提交/路径隐私/凭据扫描、PR 元数据与合并信息校验按仓库流程执行。

## 7. 发布与回滚

- 提交序列（产品仓库，基线 S 之上）：
  A) 实现提交（可分多个原子提交：O1/O2、O3/O4、O5 各自独立，便于二分与回滚）；
  B) 达标后的发布提交：`docs/evidence/core-beta/<measured-source-prefix>/`（三份报告 +
  投影 + manifest）与 `docs/performance-baseline-0.2.md` 的带日期跟进条目
  （含新源码身份、对比口径、门槛达标证据、噪声与限制）。
  报告始终绑定该次实际被测量源码，不写未来文档或合并提交哈希。
- 未达标路径：不发布成功结论；在任务与 PR 中如实报告实验数据与原因，
  给出后续建议（如 O5 单独立项或换方向的证据）。
- 回滚点：G1/G2/G3 各自可弃用对应改动（独立提交）；正式复测后如需回滚，
  revert 实现提交（不动证据提交的历史），重新走质量门。

## 8. 主要风险与缓解

| 风险 | 缓解 |
| --- | --- |
| O5 触碰崩溃恢复/审计核心 | 双格式读 + 全量既有测试 + tamper/recovery/retention 专项 + 等价 oracle；G3 原型先行 |
| 性能收益不稳定（机器/噪声，shrink spread 42%） | 三跑中位数判定；同日 control 或冻结二进制 sanity；披露分布，不用单轮结论 |
| 分块提升触发 SQLITE_TOOBIG/内存放大 | 运行时读取上限值；提高后保留保守断言与回退常量；A/B 验证 |
| 改动波及维护接口（B2）未覆盖 | 维护流程测试必跑；读明细统一走读取层；PR 描述披露格式版本 |
| 误改输出语义 | 改动限制在 adapter；workspace 全量测试 + e2e；不触碰输出装配层 |

## 9. 首轮 G4 后的有界迭代（O7）

首轮正式源码 S2 = `e8e4975817f71647c158f6d802a7b98cee2f1b15`。
三跑中位数 initial = 7819.6887 ms（改善 22.7525%），shrink =
8717.9847 ms（改善 12.3222%）；shrink 比严格门槛多 266.255945 ms。
48 个主系列仅该项失败；其余 46 个非目标系列和 8 个 legacy guard 均通过。
这些证据完整保留；不重跑同一源码挑样本，也不更改 R4。

证据：S2 `lib.rs` 的 relation-delete 循环对 shrink 的 45,000 条 Edge 与
50,000 条 Placement 各执行一次 `tx.execute`，在当前 rusqlite 实现中
每次重新 prepare。独立 trace 的 `relation_upserts` = 1371.035 ms，计时
范围实际包含全部关系删除及随后 47,500 条关系写入；不能把它解释为纯删除
成本或可节省的时长。`catalog_deletes` 计时结束点位于删除循环之前，零值
不能说明 catalog/FTS 删除没有成本。本轮不以修计时点来改变正式 harness。

最小实现限定：

- 仅处理 relation-delete 列表中相邻同种的 Edge 或 Placement 连续段。
  不排序、不跨类型合并；每段按既有 500 参数上限执行主键 `DELETE ... IN`。
- Activity/Usage 原单行路径保留。所有删除仍在原 phase-2 事务中，且在
  `verify_pending_in_tx` 之后；intent 字节、键集合、CAS、投影与失败语义不变。
- 检查实际 schema 的外键与触发器影响；同类型内部顺序不构成现有产品契约，
  但不同类型间顺序必须保留。任何执行错误仍回滚整个 phase-2 事务。
- 增加超过 500 的边界、精确键集与无关行、交错类型顺序、后续块 ABORT
  后整事务回滚及 building intent、durable 删除明细篡改先拒绝的测试。
- 私有 A/B 以 S2 正式冻结 B（SHA256 `03c168a0d146ec9d0e7cfc3d49602468457e9e3643bd17f443e0407ee70bd6cd`）
  为唯一对照，新候选使用独立 Cargo target，先核二进制身份再测量。
  交替至少两对未插桩 initial/shrink，另取独立 trace；完整保留样本。
- 没有明确实测收益或有正确性风险则否决 O7。有收益时，重跑新候选的严格
  oracle、完整质量门、独立检查后冻结 S3，再用新 suite 做正式三跑。
  S2 的正确性收据不能作为新源码的测试结果，S2 的失败必须随结论披露。

## 10. O7 否决与未达标收尾（2026-10-15）

两对未插桩 A/B 的 shrink 成对节省为 -202.3545 / +310.8686 ms；
中位节省 54.25705 ms，约 0.6635%，未达到事先固定的 >266.255945 ms，
也不满足两对均有正收益。initial 中位数回归 13.6854%。独立 trace 的
关系删除及写入阶段 shrink 中位下降 264.638 ms，仅作为局部诊断。
O7 不进入产品提交，保留私有源码、patch、二进制、全部样本及回退收据。

S2 正式 R4 继续失败，不生成 S3、不原样重跑以挑选样本。S2 的五个原子
提交保留于实验分支，由未合并草稿 PR 说明动机、正确性证据、完整失败
数据及限制；不生成成功性能 bundle，不修改已发布基线或关闭父发布门。
任务收尾表示实验已结束，AC3/R4 未满足，不能解读为优化验收通过。

GitHub 的 GH007 邮箱隐私保护拒绝了首次推送。原始五个被测提交保留在
私有 ref；公开副本只改为已认证同一账号的 noreply 邮箱，逐提交 tree、
author/committer 姓名与时间、完整消息保持一致，公开 head 为
`c9af8bbc89e14e676471cd7be8eb55a4312273dd`。PR #22 明确区分两个身份；
报告继续绑定原始 S2，不把公开 commit hash 替换进已完成的测量收据。

后续若继续，应先改善独立诊断对 catalog/FTS 删除阶段的归因，再依据新的
证据提出不同候选；不把本次嵌套 trace 或同机噪声视为性能收益证明。
