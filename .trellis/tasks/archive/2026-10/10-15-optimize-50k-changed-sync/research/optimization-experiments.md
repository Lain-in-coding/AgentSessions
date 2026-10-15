# Research: 优化实验聚合记录（E1/E4/E3 + 等价 oracle）

- Query: G0 的两个实验（E1 提交内 I/O 归因、E4 变量上限）与第一组优化后的
  spot 测量（E3）结论是什么？G0/G1 决策门如何判定？
- Scope: internal（私有 runtime 实验；本文件只保留复核过的聚合，原始日志、
  scratch DB、私有脚本留在 `.\.trellis\.runtime\optimize-50k-changed-sync-20261015\`）
- Date: 2026-10-15
- 环境：与基线相同的本机（Windows 10.0.22631 / NVMe / Defender 实时防护开启），
  rustc 1.97.1（E3 的新二进制），bundled SQLite 3.53.2（E4 运行时确认）。

## 1. E4 — bundled SQLite 变量上限（O4 目标分块的依据）

- 在真实 v19 库上打开连接（与 store 相同的 PRAGMA：`journal_mode=WAL`、
  `cache_size=-131072`），运行时读取：
  - `sqlite3_limit(SQLITE_LIMIT_VARIABLE_NUMBER)` = **32766**；
  - `PRAGMA compile_options` = `MAX_VARIABLE_NUMBER=32766`（bundled 未额外覆盖）。
- 结论：现常量 `BULK_INSERT_ROWS_PER_CHUNK = 100`（及其 `8*ROWS ≤ 950` 断言）
  反映的是旧版 999 上限，当前实际上限是其 ~33 倍。
- O4 目标分块：**512 行/块**（最宽语句 8 列 → 4096 变量，为上限的 12.5%，
  留足余量）；断言建议更新为 `8 * ROWS ≤ 32766`（或显式常量上限），并保留
  保守回退常量。
- 实现注意（实验中发现）：SQLite 的 *表达式深度* 上限（`SQLITE_MAX_EXPR_DEPTH`，
  默认 1000）与变量数无关——动态 SQL 不要构造长 `?+?+…` 链（≥1000 项即报
  "Expression tree is too large"）；多行 `VALUES (?,…),(?,…)` 是行列表而非
  深表达式树，不受此限。O4 落地后应以真实 512 行语句做一次 prepare/execute
  冒烟（O4 步骤内完成）。

## 2. E1 — 大明细整行重写的提交内成本（O5 假设验证）

实验设置（私有，见 runtime `run_e1_probe.py` + `e1/e1_results.json`）：

1. 用 canonical harness 生成 250×200 语料（源字节 14,027,058，与基线诊断一致），
   冻结二进制 B 跑 initial → 48.6 MB journal 行、shrink → 33.8 MB 行。
2. 独立探针对真实库执行与 `generation_activate` 同形的
   `UPDATE index_batches SET state='activated', durable_point='activated',
   committed_at_ms=? WHERE operation_id=?`（与 `store_metadata` CAS 同一事务），
   以 tiny-row 对照相减，分别测 warm / cold（测量前重开连接）与
   `wal_autocheckpoint=1000`（默认，与 store 相同）/ `0`（实验差分）。

实测（聚合；毫秒）：

| 场景 | 行明细 | 整行重写边际（语句+提交；默认 autocheckpoint） | 其中提交侧（WAL 写+fsync+checkpoint 回拷） | 关闭 autocheckpoint 的分解 |
| --- | ---: | ---: | ---: | --- |
| initial | 48.57 MB | warm 353 / cold 397 | ≈ 315 / 303 | 提交 192+checkpoint 135（warm）、304+130（cold） |
| shrink | 33.82 MB | warm 301 / cold 347 | ≈ 275 / 277 | 提交 206+checkpoint 279（warm，checkpoint 偏噪）、195+151（cold） |

- 第二次整行写本身造成的 WAL 增量：**+48,970,352 B（initial）/
  +34,109,512 B（shrink）**；tiny-row 对照仅 8,272 B。与诊断文件
  （freelist ≈ 旧行体积、`outbox_intent` INSERT + activation UPDATE 各写一次）
  相互印证。
- 当天在真实提交路径内的插桩对照（单样本、插桩，不入主延迟池）：`generation_activate`
  initial = 162.4 ms、shrink = 81.2 ms（baseline 二进制）；2026-10-14 基线
  run-03 的对应值为 652.6 ms / 104.9 ms——语句成本随当日缓存/Defender 状态浮动，
  但两者都远大于 tiny-row 对照（≈0.02 ms）。

**G0 门判定（O5 假设成立）**：

- initial：语句成本 162 ms（今日在途插桩）～653 ms（run-03 环境）+ 提交侧
  ~275–315 ms ≈ **0.44–0.97 s** ≥ 0.4 s ✅
- shrink：语句 81–105 ms + 提交侧 ~275 ms ≈ **0.36–0.38 s** ≥ 0.3 s ✅
- 结论：整行重写（第二次写）本身就是 O5 可消除的目标，收益假设成立；
  量级与 `candidate-optimizations-and-risks.md` 的下沿一致（initial 0.4–1.0 s、
  shrink 0.3–0.4 s），而非上限。O5 仍需按 G3（E2 原型）复核 end-to-end 收益。
- 附注（不影响结论）：关闭 autocheckpoint 只是把回拷搬出 `tx_commit`
  （checkpoint 成本在关闭后单独出现），总量不减——与既有结论一致，不采用该
  调参作为收益手段。

## 3. 第一组优化提交（O1=C2、O2=C3）

- `d60091a`：`perf(sqlite): batch affected-session lookups (C2)` —
  affected_sessions 收集批量化。
- `0fcfbbe`：`perf(sqlite): drop duplicate identity check (C3)`
  + 新增 tamper fail-closed 回归测试。
（三条实现提交的消息于 2026-10-15 由主会话按治理重写为 50/72 合规格式；
内容与树哈希不变，重写前历史保留在 `safety/pre-msgfix`。）
- 质量门（两提交后）：`cargo fmt --all --check`、`cargo clippy --workspace
  --all-targets -- -D warnings`、`cargo test --workspace` 全绿
  （adapter 299、CLI e2e 153、journal_retention 13、mcp_e2e 45 等；日志在私有 runtime）。

## 4. E3 spot 测量（O1+O2 后，G1 决策）

设置：canonical harness 生成器（字节一致）250×200 语料；两轮交替顺序
（B、新、新、B），各自 fresh 语料目录 + fresh DB；`sync initial` 后
`shrink_dataset` 再 `sync shrink`；wall 计时（不含插桩）。另跑一组插桩对照
（诊断用，不入 wall 样本）。

### 4.1 wall 样本（ms）

| 系列 | baseline B | optimized（d60091a+0fcfbbe） | 门槛（R4） | 相对 B 改善 |
| --- | --- | --- | ---: | --- |
| 50k initial | 10,749.6 / 9,300.2（中位 10,024.9） | 8,665.1 / 8,270.6（中位 8,467.8） | < 8,604.5 | ≈ 15.5%（同日基线口径） |
| 50k shrink | 7,946.9 / 12,801.2（中位 10,374.0） | 7,907.2 / 7,995.1（中位 7,951.1） | < 8,451.7 | ≥ 5.9% 余量（对照噪声大） |

- 同日基线 shrink 跨轮 7.95–12.80 s（spread 61%）再次验证 shrink 系列噪声；
  optimized 两轮 7.91/8.00 s 稳定。
- 正确性 sanity（同次运行）：两二进制 census 完全一致（catalog 50,500 /
  fts 50,000 / placements 50,000 / edges 45,000 / membership 50,500+50,000，
  generation 1→2，user_version 19），帧计数一致。

### 4.2 插桩对照（单样本，诊断；不入主延迟池）

| 阶段 | initial B → 新 | shrink B → 新 |
| --- | --- | --- |
| `affected_sessions` | 499.3 → **108.7** | 726.0 → **276.9** |
| `manifest` | 740.2 → **631.0** | 545.8 → **431.0** |
| `apply_commit`（父） | 6,136.9 → 5,872.7 | 5,910.3 → 4,935.9 |
| `tx_commit` | 1,513.4 → 1,508.2 | 1,513.6 → 1,121.7 |
| `generation_activate` | 162.4 → 162.5 | 81.2 → 104.5 |

- O1 直接命中 `affected_sessions`（3.5–4.6×）；O2 命中 `manifest`
  （−109/−115 ms）；`tx_commit`/`generation_activate` 未被触及（符合预期）。
- 插桩组 wall：initial 9,355 → 8,719；shrink 8,791 → 7,673（同日、同语料）。

### 4.3 G1 门判定

- 门槛余量：optimized 中位 initial 8,467.8（距 8,604.5 仅 1.6%）、
  shrink 7,951.1（距 8,451.7 约 5.9%）。两者都**未达到 ≥10% 余量**。
- **G1 = 继续**：按设计进入第二组（O3+O4）；按估算 O1+O2 单独（无 O5）大概率
  不满足 R4，符合 design §2 的组合预期。

## 5. 等价 oracle（B vs 新二进制，私有脚本）

设置：同一语料目录（两侧路径一致）、fresh DB；phases initial → noop →
shrink；全用户表排序 dump 哈希 + `active_generation` + `user_version` + 帧计数。
时间列（`*_ms`）与 `operation_id`/`operation_digest` 归一化；库内随机分配的
installation namespace（`ins_v1_*`/`installation-v1:*`，`randomblob(32)`
按设计不可复现）及其内嵌分配时间戳一并归一化；其余字节级比较。

- 语料 250×200 与 1×20 两种规模：**全部通过（verdict=match，0 findings）**——
  25+ 张表（catalog/fts/fts_ids/message_placements/message_edges/
  source_* / session_* / installation_* / index_batches 等）在每个 phase 的
  归一化 dump 均逐表相等，`active_generation`（1→2）相等。
- 该 oracle 提供归一化状态相等的补充证据，不能单独证明全部字节或语义不变。
  2026-10-15 恢复审查发现：`*_ms` 全部归零、所有 namespace 替换为同一标记、
  operation ID/digest 不参与比较，以及没有显式单侧 noop 精确快照断言，均可能
  隐藏差异。原始结果仍保留；冻结 S2 前须收紧这些规则并补做验证。

## 6. 对后续工作块的输入

- O4：上限 32766 → 分块 512 行（4096 变量），更新断言；落地后补 512 行真实语句冒烟。
- O5：G0 假设成立；进入 G2/G3 时以 E2 原型复核 `tx_commit + generation_activate`
  end-to-end 收益（门槛 ≥0.4 s）与恢复/retention 测试面。
- G2 预期：O3+O4 后两场景需继续留有 ≥5% 余量才能评估跳过 O5；以本文件的
  同日基线口径（initial 中位 ≈10.0 s、shrink 噪声大）做 spot 对比，正式判定
  仍以 G4 三跑为准。

## 7. 第二工作块：O3 否决 + O4 落地 + G2 判定（2026-10-15）

### 7.1 O4=C5 提交（`557833c`，canonical `perf/50k-changed-sync`）

`perf(sqlite): raise bulk insert chunk to 512`：

- `BULK_INSERT_ROWS_PER_CHUNK` 100 → **512**（最宽语句 = message_placements 8 列
  → 4096 变量）；新增具名上限常量 `SQLITE_MAX_VARIABLES = 32766`（E4 运行时实测，
  见 §1）与保守回退常量 `BULK_INSERT_ROWS_PER_CHUNK_CONSERVATIVE = 100`
  （旧版 999 变量口径，断言 `8 × 100 ≤ 950`）。8 处编译期断言全部改为对具名上限断言。
- 新增两个测试：满块 `512 行 × 8 列` 真实语句 prepare/execute 冒烟
  （`bulk_insert_statement_at_full_chunk_prepares_and_executes`，同时核对
  `?` 占位符计数与落库行数）与跨 `512/1` 块边界的批量提交逐表行数核对
  （`bulk_commit_spanning_chunk_boundary_lands_every_row`）；
  `batched_commit_rows_match_sequential_commit_rows` 的分块说明同步更新
  （batched-vs-sequential 等价性断言不变，边界覆盖移交给上述两测试）。

### 7.2 O3=C4 原型与否决（私有，未入产品提交）

原型 diff 存档：私有 runtime `o3-prototype-rejected.patch`（批作用域
`stored_placements_for_entities` / `stored_edges_for_placements` + 调用点替换 +
行投影抽取）。

**查询级预检**（真实 25k placement 库、只读探针，`EXPLAIN QUERY PLAN`）：

- 整表装载走 `SCAN message_placements USING COVERING INDEX`（无排序代价）；
  批作用域 `message_id IN (…) OR session_id IN (…)` 走 MULTI-INDEX OR（两条索引）。
- 本次矩阵的真实形态是"250 源一次提交 → fully-complete 实体 ≈ 全部实体"，
  于是作用域查询要按 500 id/块发 ~51 条语句，且**同一 placement 会被 message 侧与
  session 侧各取回一次**（跨语句不去重，map 内去重）：实测整表 42.6/52.9 ms
  vs 作用域 261.3/309.6 ms（同一库、重复两次）。

**真实二进制同轮次 A/B**（spot2；交替轮次、每个 run 都是 fresh 250×200 语料；
wall ms，2 轮中位数）：

| 系列 | baseline B | O1+O2+O4 | O1+O2+O3+O4 | O3 增量 |
| --- | ---: | ---: | ---: | ---: |
| initial | 9,076.8 | 7,782.4 | 8,793.3 | **+1,010.9 ms（+13.0%）** |
| shrink | 8,395.2 | 7,609.0 | 7,879.0 | **+270.0 ms（+3.5%）** |

原始样本：B initial 9,060.7/9,092.9、B shrink 8,320.9/8,469.5；O4 initial
7,678.9/7,886.0、O4 shrink 7,575.3/7,642.7；O3+O4 initial 8,523.1/9,063.5、
O3+O4 shrink 7,736.6/8,021.4。

插桩归因（单样本、诊断，不入 wall 池）与机制一致：`session_projection` initial
1,543.2 → 1,889.3 ms（+346 ms，alias 装载"逐块点查 + 双份取回"反超整表扫描）。
另注：原型会让 `stored_placements_from`/`stored_edges_from` 在非测试构建里成为
dead code（`cargo build --release` 报 1 warning），保留时必须再以 `#[cfg(test)]` 收口。

**结论：否决本次 O3 原型，不进入产品提交（G2 范围内记录决定与依据）。** 本任务矩阵
每次 sync 都是 250 源整批提交，候选集 ≈ catalog；本轮测量中，整表 covering-index
扫描优于该原型的分块点查。此结果不否定所有批作用域方案，也未测量单源/小批增量
场景。其他设计或场景是否有利须另行验证；原型与原始数据保留在私有 runtime。

### 7.3 O4 独立 A/B（spot3：512 vs 100 行/块，其余源码相同）

同轮次交替（B / chunk512 / chunk100），fresh 250×200 语料；两个优化侧二进制除
`BULK_INSERT_ROWS_PER_CHUNK` 外逐字节同源（同一工作树两次构建）。wall ms，2 轮中位数：

| 系列 | baseline B | chunk100（O1+O2） | chunk512（O1+O2+O4，shipped） | O4 增量 |
| --- | ---: | ---: | ---: | ---: |
| initial | 8,650.0 | 8,487.1 | 8,075.0 | **−412.1 ms（−4.9%）** |
| shrink | 8,241.3 | 8,186.6 | 7,918.6 | **−268.0 ms（−3.3%）** |

原始样本：B initial 8,356.6/8,943.4、B shrink 8,139.1/8,343.4；chunk512 initial
8,002.8/8,147.1、shrink 7,732.1/8,105.0；chunk100 initial 8,334.9/8,639.3、
shrink 7,855.0/8,518.1。

插桩诊断（spot2 单样本）：`relation_upserts` initial 1,219.4 → 982.2 ms
（95k relation upsert 的语句数 950 → 186；catalog/fts 各 505 → 99 条），与"语句数
下降"的机制一致。实测收益高于 C5 原估（0.1–0.3 s），量级仍属同一档。

### 7.4 G2 门判定

shipped 组合 = O1+O2+O4（`557833c`）。两次同日 spot 的中位数与 R4 门槛：

| spot | 系列 | shipped 中位数 | R4 门槛 | 余量 |
| --- | --- | ---: | ---: | ---: |
| spot2 | initial | 7,782.4 | < 8,604.5 | 9.55% |
| spot2 | shrink | 7,609.0 | < 8,451.7 | 9.97% |
| spot3 | initial | 8,075.0 | < 8,604.5 | 6.15% |
| spot3 | shrink | 7,918.6 | < 8,451.7 | 6.31% |

按 G2 规则"两场景均低于门槛且余量 ≥5% → 可评估跳过 O5"，字面成立。但要如实记下
两点：（a）波动不小——同一 shipped 二进制两次 spot 相差 ≈3.8%/4.1%，同日 B 的
跨轮 spread 最高 7%，此前会话更有 61% 的 shrink 样本；（b）当日冻结 B 比已发布
基线中位数快 10–17%（initial 8,650–9,077 vs 10,122.9；shrink 8,241–8,395 vs 9,943.2），
说明"对门槛的绝对余量"随机器状态漂移，6–10% 并不稳健。

**G2 结论与主会话决策（2026-10-15）：** 字面进入"评估跳过 O5"分支；但基于（a）(b)
与 O5 的 G0 证据（整行重写可消除 0.44–0.97 s（initial）/ 0.36–0.38 s（shrink）），
主会话采纳"仍按计划实施 O5"的评估结论——先跑 E2 原型 + G3 门槛复核，把 R4 的安全
边际做实，不跳过 O5；O6 仍仅在 G3 判定需要时考虑。若后续选择先跑 G4 三跑以验证
O1–O4 是否已达标，本文件的 spot 数据可作为其预期区间的输入。

### 7.5 等价 oracle #2（O1+O2+O4 vs 冻结 B）

同 `run_oracle.py` 口径、新根目录（私有 `oracle2/`）：250×200 与 1×20 两种语料，
initial → noop → shrink 三相位；全表（含 catalog/fts/fts_ids/placements/edges/
source_*/session_*/index_batches 等 25+ 张）排序 dump 哈希 + `active_generation`(1→2)
+ `user_version`(19) + Robot 帧计数：**两语料 verdict=match，0 findings**。

### 7.6 质量门（`557833c`）

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿；
- `cargo test --workspace` 全绿：**2009 passed / 0 failed**（含 adapter 301、
  CLI e2e 153、journal_retention 13、journal_maintenance 31、mcp_e2e 45、
  provider_matrix 44、application 359/299 等）；
- 原始日志与 spot/oracle 产物（不入产品提交）：私有 runtime
  `optimize-50k-changed-sync-20261015/`（`logs/spot2.log`、`spot2/`、
  `logs/spot3.log`、`spot3/`、`logs/oracle2.log`、`oracle2/`、
  `logs/workspace-tests-o4.log`、`o3-prototype-rejected.patch`）。

### 7.7 治理发现与修复（2026-10-15，主会话）

canonical `scripts/governance/check_commit.py` 按 `CONTRIBUTING.md`（header ≤ 50 字符、
body 行 ≤ 72）校验提交语法：本分支最初的 O1/O2 提交消息（`cd97bcd`/`bc0ccf6`）被该
工具拒绝（header 66 字符、body 行超宽；作为对照，已发布的基线提交通过同一 range
校验）。主会话已在本地重写三条实现提交的消息为合规格式，内容不变（树哈希与重写前
一致）：`d60091a`（O1/C2）、`0fcfbbe`（O2/C3）、`557833c`（O4/C5，原消息本已合规）。
重写后 `check_commit.py --repo . --base de2c95d… --head 557833c…` = **passed**
（introduced-commit-syntax-and-identities，3 commits）；重写前历史保留在分支
`safety/pre-msgfix`（不入发布链）。

## 8. O5 E2 与 G3 否决（2026-10-15）

恢复时发现未提交的父/子表原型，修改仅在 adapter `lib.rs` 和 `maintenance.rs`。
原型拆分实际五字段（包含 `delete_ids_json`），永久 `relocation_json` 留在父行。
实现代理服务未能启动，主会话接手实验；未把代理错误算作产品测试失败。

### 8.1 身份与无效尝试

控制源码固定为 `557833c242572d08006294ad5a49a5dc184ad506`，在独立 detached
worktree 构建。第一次尝试共享 Cargo target，随后 canonical 构建错误复用了
控制产物，两侧哈希相同。主会话检测后停止该尝试；已采的一个 initial 样本、
构建日志和 `invalidated.json` 保留在私有 `o5-e2-01/`，不参与任何决策。

修复方式为独立 target/build 输出与预检。有效尝试 `o5-e2-02/` 使用同一锁定
harness、同工具链、release 配置；控制二进制 SHA-256
`464202b65b96e03b87cc6602719ba3561da8c006ffc71f809ec7319c92a2aa5b`，原型
`29220101d7e030fa0c04b86cedfdac4a1438fb636a2044097d4bd4218faa8b4d`。
计时前断言哈希不同，并以 1×20 合成语料验证控制 schema 19、原型 schema 20；
原型父行五字段合计 10 字节占位，子表行存在，harness census 通过。

### 8.2 完整对照

每侧每相位两个未插桩 wall 样本；顺序 O4/O5/O5/O4，每次 fresh 250×200，
initial 后 shrink。之后另以相同顺序采两个 trace 样本；插桩 wall 不混入主池。
全部 16 个相位的 `sync_check`、`SyntheticOracle.scan` 通过。该检查不替代
迁移、retention、tamper 的完整质量门，原型尚未通过这些门。

| 相位 | O4 wall 样本 ms | O5 wall 样本 ms | O4 阶段和样本 ms | O5 阶段和样本 ms |
| --- | --- | --- | --- | --- |
| initial | 11789.6488 / 9601.9231 | 9962.7743 / 8286.9446 | 1219.222 / 1265.076 | 912.606 / 931.575 |
| shrink | 15100.7619 / 7515.0729 | 8057.9411 / 8115.1028 | 1361.612 / 1037.194 | 1131.847 / 1215.031 |

阶段和仅为同一 `adapter:commit` 的互不重叠 `generation_activate` 与 `tx_commit`。
中位收益 initial **320.0585 ms**，shrink **25.964 ms**，均低于 G3 的 400 ms。
未插桩 wall 中位数下降 1570.9265 / 3221.3955 ms，但控制 shrink 跨样本相差
7585.689 ms，且阶段证据不足，不能用 wall 改善覆盖预先约定的 G3。

**决定：否决本次 O5 原型。** 保存 patch、二进制、完整样本和 trace 后精确反向
应用 patch；canonical 恢复 `557833c` 干净树。schema 20 不进入产品提交，O5
正式迁移/维护加固工作不再执行。G0 的写放大量级仍是有效诊断，但收益假设未在
本轮 G3 得到足够支持；本结论不证明所有 journal 拆分方案都无效。

### 8.3 下一步

评估计划内 O6/C6：manifest 数组逐项序列化与排序键缓存，保持既有校验、
canonical_value、JSON 字节及 digest 域。仅在字节等价和 E3 对照支持后保留。
正式 R4 仍未运行，不作成功声明。

## 9. O6 E3：逐项构造 manifest（2026-10-15）

实现只改 `RelationManifests::canonical_json`：缓存纯函数排序键，复用原有
`canonical_value`，逐项写入紧凑 JSON 数组。避免同时持有关系 upsert、delete、
source replacement 的三个完整 Value 数组；最终 JSON 字符串仍须保留并由
原 `batch_manifest` 哈希。**这不是流式哈希**，不改任何 schema/digest 域。

控制沿用 §8 重新构建且预检通过的 `557833c` 二进制；O6 在 canonical 独占的
target 内重新编译 adapter/CLI，冻结 SHA-256
`f74bffc939d0a0a81f933a09bfdd9d6aa58b3c3c46aa6464df9416d48ad283ef`。
源 patch、构建日志、二进制、样本和 trace 保留于私有 `o6-e3-01/`。
两侧预检均为 schema 19；二进制哈希不同，父表完整明细和 census 正确。

顺序 O4/O6/O6/O4，每侧每相位两个 wall 样本；之后同顺序两个独立 trace，
不混用计时池。16 个相位的 `sync_check` / `SyntheticOracle.scan` 均通过。

| 相位 | O4 wall 样本 ms | O6 wall 样本 ms | O4 manifest 样本 ms | O6 manifest 样本 ms |
| --- | --- | --- | --- | --- |
| initial | 10067.5030 / 8328.6912 | 8355.8727 / 7969.8366 | 633.552 / 747.722 | 463.608 / 530.746 |
| shrink | 8875.7894 / 8754.2560 | 7833.3285 / 7337.2502 | 479.386 / 529.062 | 295.956 / 292.765 |

manifest 中位收益 **193.460 ms / 209.8635 ms**，与 C6 的 CPU/内存方向一致。
wall 中位数分别 **9198.0971→8162.85465 ms / 8815.0227→7585.28935 ms**；
其较大的总差值仍包含机器波动，不能全归因于 manifest，也不是正式 R4 测量。
主会话决定保留 O6，待完整正确性与质量门通过后冻结 S2。

基础 JSON 字节测试已通过；独立 reviewer 补充真实 typed manifest、旧三段序列化
和原始摘要对照，并加强 O4 尾块与 O2 tamper 测试。严格 oracle 与完整质量门仍
在进行，本段不提前宣称它们成功。

## 10. 复核后的验收与等价边界

- 原始报告精确中位数为 initial 10122.8974、shrink 9943.2103 ms，严格门槛为
  **8604.462790 / 8451.728755 ms**。此前三位小数及 8604.5/8451.7 仅作展示。
- 正式比较有 48 个 process-wall 主系列；8 个 legacy latency 另列，不能把同一
  调用的两种计时当作独立观测。anchor 每类 search20次、scale 每类100次；
  startup 只在 anchor，context 只在 scale。详见独立 `formal-protocol-verification.md`。
- `validate-baseline` 只计算新 suite 的内部 spread，不自动判 R4；还须单独核对
  独立 trace 可用与包含 trace/收尾的 suite 总时长。
- O4 可改变内部 FTS rowid 分配，既有 batched/sequential 合约已允许该物理差异。
  强化 oracle 保存原始地址，跨侧验证完整 wire→identity→FTS id/text 双向关联，
  不把“丢弃 rowid”当作证明；新增 200 条全量改写覆盖该边界。同侧 noop 仍检查
  包含原始地址在内的精确快照不变，其他表的领域数据不新增宽泛归一化。

## 11. 最终正确性与质量验证（2026-10-15）

最终候选为 O1/O2/O4/O6，schema19。独立 checker 补齐 typed manifest 的三段旧
序列化/原始 digest 对照（包含 Placement/Edge/Activity/Usage 与 source/resume/
installation）、非法 manifest 拒绝、514 messages/513 edges 尾块与两类 membership
精确集合、合法 noop 和两种 identity tamper 入口的全表原状断言。

私有严格 oracle 的最终结果为三组 match /0 findings：250×200、1×20，以及
200 条同 ID 全量改写；18 个真实 sync 相位、每相位26张用户表、12个独立原始
digest proof、6次同侧 raw noop 精确相等。10张 FTS5 影子表不进入跨侧语义比较，
但进入同侧 raw 快照。200条改写产生200个物理FTS rowid差异；完整身份/正文
双向关联、200/0新旧词 postings 与4个 show/get 均一致。

只归一化明确记账时间和双射 namespace/operation 关联；先验证原始摘要。
对 shrink，先用上一代原始 payload 与已验证 fresh projection 重建提交前 union，
而不是错误地拿提交后 subtractive alias 当摘要输入。Robot 调用 ID/耗时与 opaque
cursor 仅在指定路径映射；cursor 原始摘要、query/sort/offset/generation、15分钟TTL
和调用时间窗均独立验证。24个state/proof控制与15个frame/cursor控制通过。
未执行 cursor 续页；工具活动、用量、搬迁表在合成 corpus 中为空，这些类型由 typed
manifest 回归及既有 workspace 测试补充覆盖。

- 脚本：私有 `strict-oracle-01/strict_oracle.py`，SHA-256
  `74019400d458a7d8bd97ba13b1a39fae33504b8b6b25d845016fc240c3fcf9e6`。
- 最终收据：`recompare-01/result.json`，SHA-256
  `4b9a3cf97959e822178605248c9c359dc8f1a87da2580a818d9f8ff9c676973d`。
- `prepare-01` 链接失败、`run-01` 摘要输入前提失败、`run-02` 原始响应时钟/游标
  比较失败均保留；`reprove-01` 与 `recompare-01` 只读重证，不覆盖原始样本。

最终质量门：fmt/clippy exit0；adapter340 pass/0fail/0ignore；完整 workspace
2012 pass/0fail/23个既有显式ignored（20个手动fixture输出、2个provider微基准、
1个扩大维护实验，与base逐路径/内容一致）；evidence143与governance93全部通过且
无skip。新增 typed fixture 首次因手写 legacy seed 被正确拒绝，已改为真正派生值，
失败和复跑均保留。代码未弱化任何校验。正式 G4/R4 和源码冻结仍另行执行。
