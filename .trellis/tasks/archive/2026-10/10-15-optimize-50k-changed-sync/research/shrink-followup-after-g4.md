# Research: G4 首次失败后的 shrink 成本与关系批删候选

- Query: 独立验证 S2 正式三跑的 R4 失败；提取 shrink 每 trial、对应观测和 trace；核对当前逐条关系删除与连续同型批删的证据和可行边界。
- Scope: internal / source-only；只读原始 JSON、trace、当前 adapter 源码与锁定 rusqlite 源码；未运行新 benchmark、build、test 或 Git；唯一新写入为本文件。
- Date: 2026-10-15
- Active task: `.trellis/tasks/archive/2026-10/10-15-optimize-50k-changed-sync`
- 产品相对根：`./.trellis/.runtime/attribution-closeout-20261012/canonical-repaired`。

## Findings

### 1. 可行动结论

**G4/R4 失败属实，唯一不通过项为 `scale-50000/sync/shrink`。** 当前三跑中位数 8717.9847 ms，需要在本次中位数基础上再减少 **严格超过 266.255945 ms（3.054099705%）** 才跨越精确门槛 8451.728755 ms；实际候选应留下对已观察噪声的余量。

**连续同型关系删除批量化是值得有界原型验证的 adapter 提交路径候选。** 当前 shrink 95,000 个 relation delete 每个都会调用一次普通 `tx.execute`，锁定的 rusqlite 0.40.2 `Connection::execute` 又每次执行 `self.prepare(sql)`，没有 prepared-cache 复用。可以减少的重复工作有明确源码证据；其耗时没有被独立量出，不能承诺足够省下 266 ms。

主要归因限制：trace 中名为 `relation_upserts` 的 1371.035 ms 同时包含关系删除和 upsert；`catalog_deletes=0` 的 stopwatch 还在真正删除前就结束，不能理解为 catalog/FTS 删除没有成本。不要据这两项的名称作过细归因。

### 2. 独立 R4 复核

- S2：`e8e4975817f71647c158f6d802a7b98cee2f1b15`；freeze receipt tree：`f1a99f12e093fd669958102a23e470c37a2ad784`。
- Suite：`evidence-output/opt50k-e8e4975817f7-20261015T121711/`。
- 冻结 B2：`03c168a0d146ec9d0e7cfc3d49602468457e9e3643bd17f443e0407ee70bd6cd`，已重新对本地冻结文件计算 SHA-256 并吻合。
- harness：`aeb23f909de805f507f45c1c61a20cf940a111ee962a0c6d6cd4ae37a49f39f8`，当前文件重算一致。
- 本次读到的 adapter `src/lib.rs` SHA-256：`2ec45e89d3c10e58690d60a1816b62779bd4263624893b4388530fb39f9c97a1`；未运行 Git，源码归属以父会话提供的 S2/freeze 记录为准。
- 新 run1/2/3 JSON SHA-256：
  - `bdc50482b07db8200df539516609daf7bc0f71040409a6843bfedcc4fbb23e27`
  - `692a8302d11e87f7ce9c9f93e8571211c6ffe8120fa8e00cbce24f72f1047a2c`
  - `6111da3b7bc50ad22bf126c30824639a72c6ff639bc919c394e245d02b513a4d`
- 用 Decimal 从原基线及新三跑各自 **9,228 个主 measured observations** 重算48主系列P50，逐项与summary完全相等；8个legacy也从自己的raw_samples重算。未依赖主会话的派生R4文件。

| 50k 操作 | 原基线三跑中位数 ms | 新 run1 / run2 / run3 P50 ms | 新中位数 ms | 相对原基线变化 | R4 |
| --- | ---: | --- | ---: | ---: | --- |
| initial | 10122.8974 | 7819.6887 / 8661.5382 / 6953.7125 | 7819.6887 | −22.752465% | 通过，<8604.462790 |
| shrink | 9943.2103 | 7146.9575 / 10717.6615 / 8717.9847 | 8717.9847 | −12.322234% | **失败**，应<8451.728755 |

- **48主系列：47通过/1失败**（47包含initial；其余46主系列均在1.15倍上限内）。**8 legacy：全部通过**。这不是56个独立主系列。
- 非两项改善门的54行里最大相对回归为 `anchor/sync/noop`：+12.205028%，仍在15%以内。
- 新 initial 跨run spread **21.840073%**；新 shrink **40.957906%**。基线分别11.228326% /42.418474%。spread公式仍为100×range/三跑中位数，不能当回归豁免。
- 已读取 `suite-end-check.json`：`through_trace_and_driver_return_ms=1717156.0`，即28分37.156秒；trace available、3 phases。父会话已跑正式validators；本次未重新执行。

### 3. 全部9个 shrink trial 与关联观测

下列initial/noop来自相同run、相同fresh trial，shrink是原始`process_wall_ms`；CLI整数毫秒与RSS只作相邻证据，不与主时钟混算。

| run / trial | 关联 initial ms | 关联 noop ms | shrink wall ms | shrink CLI ms | shrink peak RSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 / 1 | 7847.2709 | 65.0935 | 7146.9575 | 7131 | 511.589844 |
| 1 / 2 | 7819.6887 | 76.2069 | 7296.0393 | 7264 | 511.636719 |
| 1 / 3 | 7409.2954 | 65.9515 | 7119.7261 | 7106 | 511.722656 |
| 2 / 1 | 13648.1540 | 140.2968 | 12234.1512 | 12216 | 509.648438 |
| 2 / 2 | 8661.5382 | 86.3056 | 8091.2939 | 8067 | 510.664062 |
| 2 / 3 | 8257.1628 | 73.8554 | 10717.6615 | 10703 | 510.707031 |
| 3 / 1 | 7847.7471 | 105.2195 | 9162.2293 | 9147 | 509.808594 |
| 3 / 2 | 6953.7125 | 80.5066 | 8717.9847 | 8702 | 510.023438 |
| 3 / 3 | 6799.3557 | 64.8660 | 7904.1227 | 7890 | 509.363281 |

- 第三trial查询固定插在noop后、shrink前；第一/二trial没有该批查询。run1三个shrink都低于门槛，run2有显著偏慢值，run3第二trial的8717.9847正是最终三跑中位数。不能移除run2或挑选最快trial来改变结论。
- run2第一trial的initial/noop也同时上升（13648.1540/140.2968 ms）；这支持“该时间窗口存在更广泛变化”的关联，但报告没有CPU占用、磁盘延迟、Defender活动或温控数据，**不能据此确定外部噪声根因**。
- 九次shrink census一致：250 sources，25,000 Messages、25,000 Placements、22,500 Edges、250 Sessions/SourceDocuments，generation=2；每次source bytes=7,013,530，post-child store bytes=238,444,544。不是输入规模、输出census或磁盘体积改变导致不同trial。
- shrink RSS约509.36–511.73 MiB；原始记录中的CLI duration大体同步wall变化，observer-cleanup增量只是约0.08–0.18 ms，不能解释几秒级起伏。
- 每trial都是fresh store；这些绝对值不能用减去另一轮startup、混池或事后校正来重写R4。

### 4. 单独 shrink trace：成本仍主要在提交

`run-03/index-trace-shrink.jsonl` SHA-256：`bfe7ec6c1e19903e671cdf106ef327d29394181657a71eedb28ccca420d24120`。
独立trace shrink process wall=**8667.7765 ms**，CLI reported=8655 ms，peak RSS=510.367188 MiB；ordinal3172、verification角色，不进入主延迟池。

| label / stage | 单次 shrink trace ms | 范围解释 |
| --- | ---: | --- |
| `cli:sync/commit` | 8256.101 | 最外层CLI提交范围 |
| `adapter:catalog/apply_commit` | 6119.483 | 事务apply，包住下述commit各段 |
| `adapter:commit/relation_upserts` | 1371.035 | 实际含95k逐条关系删除 +47.5k关系upsert |
| `adapter:commit/tx_commit` | 1250.909 | 事务提交 |
| `adapter:commit/session_projection` | 1005.167 | 会话投影 |
| `adapter:commit/fts_projection` | 540.982 | 含adapter:fts子段 |
| `adapter:commit/integrity_checks` | 539.355 | 提交完整性校验 |
| `adapter:catalog/load_catalog_state` | 524.463 | 加载先前catalog/关系状态 |
| `adapter:commit/source_replacements` | 507.871 | 250个source replacement |
| `adapter:commit/affected_sessions` | 425.968 | 受影响会话集合 |
| `adapter:catalog/merge_stored` | 386.458 | 既有记录合并 |
| `adapter:catalog/manifest` | 302.397 | O6后的manifest构造 |
| `adapter:catalog/outbox_intent` | 272.592 | durable intent写入 |
| `adapter:commit/generation_activate` | 98.208 | generation与journal状态更新 |
| `adapter:commit/catalog_deletes` | 0.000 | 计时位置错误，不能解释为真实删除为0 |

各label存在嵌套，表中外层与内层不能相加；这些是一次有instrumentation的观测，不是三跑主中位数。`adapter:fts/fts_projection_rows=444.956`、`fts_ids_insert=93.160` 已包含于 `adapter:commit/fts_projection`，也不能再加一次。

原trace聚合计数（经白名单字段提取）：

| phase | entity upserts | entity deletes | relation upserts | relation deletes | source replacements |
| --- | ---: | ---: | ---: | ---: | ---: |
| initial | 50,500 | 0 | 95,000 | 0 | 250 |
| shrink | 25,500 | 25,500 | 47,500 | **95,000** | 250 |

- `relation_upserts` timer在 `lib.rs:7029` 开始，`:7030` 先完成逐条delete，之后批量Placement/Edge upsert与Activity/Usage upsert，最后`:7222`记入同一个stage。因此1371.035 ms不是纯插入，更不是95k删除的独立耗时。
- `catalog_deletes`在`:7001`开始、`:7002`立即结束；真正catalog/FTS/fts_ids删除位于`:7003`之后。其0.000 ms属于现有诊断计时位置问题，真实删除成本仍落在外层apply_commit/CLI commit内。这次研究只记录问题，不修改正式harness或产品源码。
- `tx_commit=1250.909`和`session_projection=1005.167`仍是较大单段；批量关系删除不会自动消除其WAL、索引维护或投影工作。

### 5. 当前删除路径与连续同型分块可行性

#### 当前路径有95k次prepare+execute的源码证据

- `commit_index_batch_with_relations`内 `lib.rs:7030` 按原 `relation_deletes` 向量逐个match：
  - Edge → `DELETE FROM message_edges WHERE child_placement_id = ?1`（`:7034`）
  - Placement → `DELETE FROM message_placements WHERE placement_id = ?1`（`:7041`）
  - Activity → `DELETE FROM tool_activities WHERE activity_id = ?1`（`:7048`）
  - Usage → `DELETE FROM usage_events WHERE usage_id = ?1`（`:7054`）
- `Cargo.lock:2090`锁定rusqlite **0.40.2**。读取该锁定版本本地crate source：`rusqlite-0.40.2/src/transaction.rs:233`使Transaction deref到Connection；`src/lib.rs:620`的execute实现是 `self.prepare(sql).and_then(|mut stmt| stmt.execute(params))`。所以这里不是“已prepare一次后只execute95k次”，也没有使用prepare_cached。
- 当前source-batch构造在 `lib.rs:4418`起按Edge →Placement →Activity →Usage连接各BTreeSet，生成长同型连续段。这个合成corpus没有Activity/Usage，初始为50k Placement+45k Edge；结合95k relation deletes与构造代码，可以推导该批次为45k Edge删除段及50k Placement删除段。该分类是代码/合成census推导，不是新增单独计数测量。

#### 建议原型边界（研究候选，不是已批准设计或收益结论）

- 只把连续同variant的删除段切成既有 `BULK_INSERT_ROWS_PER_CHUNK=512` 小块，每块按固定enum→table/key映射执行参数化 `DELETE ... WHERE key IN (...)`。复用 `in_placeholders`（`:161`）与现有 `params_from_iter`/catalog批删模式（`:7003`）；每块仅512个变量，小于既有32766上限。
- 保持**输入向量的跨variant顺序**，不把任意混排调用者全局重新分组；不更改relation manifest本身、其canonical bytes/digest、source replacement、outbox、generation CAS、事务边界或最终integrity检查。
- 对当前推导的两个长段，SQL调用数结构上为 `ceil(45000/512)+ceil(50000/512)=88+98=186`，原为95,000。**这是调用次数算术，不是速度测量**。IN集合构造、索引删除、WAL页面和提交成本仍在。
- 四张表目前以各ID作单列PRIMARY KEY（schema代码`:2164`、`:2179`、`:2372`、`:2496`）；没有生产DELETE trigger/声明式FK参与这四段的schema定义，关系完整性由同事务末尾`:7454`校验。相同表不同主键删除在现有schema下提供集合等价基础；仍需验证分块和错误回滚语义。
- `RelationManifests::validate`已在`:850`拒绝重复delete键，在`:861`拒绝同relation同时upsert/delete。不要移走或弱化这些前置校验来批删。
- 合成initial的relation_deletes=0，故直接优化命中的主要是shrink；空集必须保持廉价，其他规模/读取不能因此引入新的扫描或分配。

#### 下一步最有判别力的验证

1. 在私有原型测量中把**删除**与**upsert**分段，并正确包住catalog-delete loop；保留同一S2/B2作对照，不改变正式harness。若先做“每连续段prepare一次、仍逐行execute”的诊断对照，可区分prepare成本与VM逐行执行成本；不要把这种对照当作已证明最终方案。
2. 同机交替A/B观察50k shrink端到端与对应删除段，多次观测并保留全部结果。判断是否有稳定的>266.255945 ms端到端余量，再决定是否值得下一次正式suite；不能拿1371.035 ms总relation块直接承诺可回收多少。
3. 最小正确性面：空/单条/512/513条边界，四种variant及交错连续段，missing-id既有语义，manifest重复拒绝，后续块失败时事务整体rollback与building intent保留。现有错误回滚例在 `lib.rs:14935`（relation_apply_failure_rolls_back_everything_except_building_intent）。
4. 延续已有严格oracle：冻结旧B与candidate在两种语料initial→immediate noop→shrink比较，保留journal bytes/digest、generation与全表约束；必要的真实adapter/CLI检查由实现/检查角色运行。本研究没有执行这些测试。

该候选限于已讨论的adapter changed-sync提交路径，不增加public功能、schema或新依赖；研究结论不代替主会话的设计/实现决定，也不承诺R4一定通过。

### 6. 文件与规范

| 文件 | 用途 |
| --- | --- |
| `.trellis/tasks/archive/2026-10/10-15-optimize-50k-changed-sync/prd.md`、`design.md` | R3/R4、adapter提交范围、失败不得宣称成功 |
| 同任务`research/formal-protocol-verification.md` | 48主+8legacy区分、精确原基线阈值与时钟口径 |
| `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md:375` | durable manifest、原子性、正确性不能为性能放宽 |
| `.trellis/spec/agentsessions-cli/backend/index.md:303` | evidence真实性/私有产物边界 |
| 产品`docs/evidence/core-beta/b0fc44d75690/run-0{1,2,3}/core-beta-benchmark-full.json` | 原始基线全部raw样本 |
| 产品`evidence-output/opt50k-e8e4975817f7-20261015T121711/run-0{1,2,3}/core-beta-benchmark-full.json` | S2正式三跑全部raw样本 |
| 同新suite `source-freeze.json`、`suite-end-check.json`、`baseline-cli.exe` | 来源、含trace时长、冻结B2核对 |
| 同新suite `run-03/index-trace-observation.json`、`index-trace-shrink.jsonl` | 独立诊断聚合/计数；不进入主分布 |
| 产品`crates/agent-session-grep-adapters-sqlite/src/lib.rs` | 当前逐条删除、批量upsert、范围计时、校验与schema |
| 产品`Cargo.lock:2090`；本地 `<CARGO_HOME>/registry/src/index.crates.io-1949cf8c6b5b557f/rusqlite-0.40.2/src/lib.rs:620` 与 `transaction.rs:233` | 锁定依赖实际prepare/execute行为 |

External references：没有依赖联网摘要；rusqlite行为直接以本地锁定0.40.2版本原源码核对。没有将最新版文档、旧spike或训练记忆代替当前调用链。

## Caveats / Not Found

- 这轮R4失败应保留，不能通过删除样本或立即无改动重跑来洗掉；本研究没有触发新suite。
- 没有纯relation-delete耗时、磁盘/CPU/Defender证据；不能确认批删收益大小或把run2偏慢归因给任何具体系统活动。
- trace既有stage名称/位置存在上述两个限制；未修改harness或源码来补它们，后续只能通过明确的私有归因实验细分。
- 本轮未重新执行正式validators、build、test或Git；只计算原始样本统计、哈希并核对源码。没有宣称新候选已实现/通过/获设计批准。

## Decision note: 已采纳 O7 范围与块大小（2026-10-15）

主会话已在 `design.md` §9 与 `implement.md`「第五组：O7」批准有界原型。
**实际采纳值为既有 `BATCH_IN_CHUNK=500`，不是本文早先候选估算的512。**
仅批量相邻同类型的 Edge/Placement 段，Activity/Usage 保持原单行路径，跨类型顺序不变。
该合成批次的语句数算术相应为 `ceil(45000/500)+ceil(50000/500)=90+100=190`。
上文512/186和四种variant的讨论保留为历史研究；执行以已批准设计为准。这是决策更新，不是新增性能实测或收益承诺。
