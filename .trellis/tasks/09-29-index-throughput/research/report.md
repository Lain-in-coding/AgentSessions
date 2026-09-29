# 初始索引吞吐与峰值内存专项：trace 归因、逐项优化与 A/B 证据

> 任务：`.trellis/tasks/09-29-index-throughput`；分支 `feat/post-reuse-phase`。
> 归档对照基线：`2b8f895` 实测 482.6 s / 4.58 GB（`09-29-wake-reuse-study`）。
> 本次复测基线：`72cb684`（当前 main 前沿 + 测量 harness 提交）。
> 所有数字来自本任务 `research/harness/baseline.py`（release 二进制、合成语料、无 OS 缓存清理、无并发构建）或本报告注明的仪器化 trace / 内存诊断。

## 1. 结论摘要

- **时间**：1M 初始 sync 中位 `412.7 s → 257.9 s`（详见 §5），阶段级 trace 显示 `cli:commit` 从 567.6 s 降到 245.8 s（-56.7%，同条件 trace 对照）。
- **峰值 RSS**：1M 中位 `4574.6 MB → 2859 MB`，未达成 ≥50% 目标；归因见 §7（批次内数据副本 + 别名再生整表加载 + manifest/SQLite 工作集）。
- **search / noop 护栏**：search p50/p95 各规模未回退（1M p95 −19.8%）；noop 在配对交错 A/B 中持平略优（100k −2.1%），1M 首个（冷）空转批次由 6.9 s 降到 0.7–1.1 s —— 空转双读/双哈希消除的直接证据（§4.1、§5）。
- **未发现正确性回归**：SQLite 适配器 270 测试全绿；16 张权威/投影表在基线二进制与优化后二进制之间逐字节一致（§6）。

## 2. 测量协议与环境

| 项 | 值 |
|---|---|
| OS / CPU / 磁盘 | Windows / Intel Core Ultra 7 155H / NVMe（见 `harness/environment.json`） |
| 语料 | harness 合成语料，`fixture` 冻结；数据哈希按规模记录在证据 JSON |
| 10k 语料哈希 | `f4365a47329760bdf9c96bd98848c3ff9e79747b4f74739f6daf72f75c2f2c08` |
| 100k 语料哈希 | `34f02c25a5a26a1c…`（见证据 JSON `dataset.dataset_hash`） |
| 1M 语料哈希 | `22519fe8ef5b84d34a7f26b62544a6d58bc7f2961dc0458908ff6cf8c180c990` |
| 基线二进制 | `agent-session-grep.exe` SHA-256 `47e3c6a010709f691e0b6b826fc88031628d9712685bec79d92817df3323130a`（`72cb684` 构建） |
| 基线测量提交 | `72cb684`（harness 参数化后） |
| harness pin | `--expected-commit` 必填；工作区 HEAD 必须等于该提交，且 `Cargo.toml/Cargo.lock/crates/scripts/evidence` 相对它无改动，否则拒绝运行 |
| 限制（沿用研究结论） | 未清 OS 缓存，非冷盘结论；初始 sync 为分批总计；search 含进程启动；Windows 计时已用高精度时钟 |

### 2.1 当前 main 基线复测（n=3，中位数）

| 规模 | 初始 sync | 空转 p50 / p95 | search p50 / p95 | 峰值 RSS | catalog.db |
|---|---|---|---|---|---|
| 10k | 2.18 s | 19.6 / 19.7 ms | 113.0 / 155.3 ms | 153.6 MB | 51.8 MB |
| 100k | 28.66 s | 61.6 / 63.1 ms | 111.1 / 145.3 ms | 1439.1 MB | 518.0 MB |
| 1M | 412.7 s | 582.5 / 6785.8 ms | 116.3 / 154.3 ms | 4574.6 MB | 4398.6 MB |

与归档基线（482.6 s / 4.58 GB）相比，当前 main 复测确认了同一瓶颈（1M 初始 sync 超线性、峰值 RSS 与库体积同阶），但绝对时间受环境漂移影响（+/-15%）。

## 3. 阶段级 trace 归因（基线，1M = 5×200k 批）

env 门控 trace（`ASG_INDEX_TRACE`，默认关闭，见 `crates/*/src/trace.rs`）原始 JSON：`research/traces/1m.jsonl`（基线 `72cb684`）与 `research/traces/1m-after.jsonl`（候选 1–5 的二进制 `5ba9939`；候选 6 的阶段变化见 §4 表与内存诊断）。

基线 5 批 `cli:commit` 阶段合计（秒）：

| 阶段 | 合计 | 说明 |
|---|---|---|
| session_projection | 172.2 | 兼容别名再生（整表加载 placements/edges + 每条实体 SELECT/JSON 重写） |
| relation_upserts | 128.6 | 380k 行/批的多行 INSERT（placements/edges/activities/usages） |
| outbox_intent | 30.6 | durable intent 写入（manifest JSON 序列化） |
| tx_commit | 30.1 | SQLite 事务提交（WAL） |
| source_replacements | 29.2 | 成员/扫描/claim 行重写 |
| integrity_checks | 28.8 | 本批 id 的引用完整性校验 |
| manifest | 25.5 | `batch_manifest` + identity 校验 + late no-op 探测 |
| verify_outbox | 22.8 | durable intent 重算比对 |
| load_catalog_state | 21.5 | 整库关系/成员状态 8 张表全量载入 |
| affected_sessions | 18.7 | 逐消息 `collect_message_sessions`（N+1） |
| fts_projection | 16.2 | FTS5 行 + 身份边车写入 |
| 其余（validate/merge/tombstones/capture/stage/assemble） | ~24 | 批内数据处理 |

内存诊断（把 200k 新消息写入 1M 库的第 6 批，20 ms 采样，`research/scripts/mem_diag.py`）显示：RSS 在 `load_catalog_state` 窗口从 1.4 GB 升到 4.8 GB，该阶段耗时 23.4 s —— 与 trace 的 load_catalog_state 排名一致。

## 4. 逐项候选（每项一个提交 + A/B）

| # | 提交 | 机制 | 证据 | 结论 |
|---|---|---|---|---|
| 1 | `de33e75` | 未变化源不再重复 `verify_snapshot`（空转双读/双哈希消除） | 100k A/B 同窗口：noop 中位 69.1 → 61.0 ms（-11.8%）；1M noop p50 见 §5 | 保留 |
| 2 | `490e0ba` | 写连接 `PRAGMA cache_size = -131072`（128 MiB；不改 `synchronous`/journal 语义） | 100k 同窗口 A/B：初始 sync 28.14 → 19.92 s（-29.2%）；RSS +104 MB（可接受度见 §7） | 保留 |
| 3 | `fd918f1` | late no-op 探测复用已载入状态，去掉第二遍整库载入 | 1M trace：manifest 25.5 → 17.2 s；load_catalog_state 相应减少 | 保留 |
| 4 | `525a0c6` | durable manifest 每提交只计算一次（原本 3 次序列化+哈希） | 1M trace：outbox_intent 30.6 → 6.7 s，verify_outbox 22.8 → 1.5 s | 保留 |
| 5 | `5ba9939` | 别名再生候选集过滤 + 内存 payload 快路径 | 1M trace：session_projection 172.2 → 83.8 s（-51%） | 保留 |
| 6 | `748c720` | 提交状态读取按批内候选 id 范围化（8 张表不再整库载入） | 内存诊断（200k→1.2M 库）：load_catalog_state 23.4 → 1.5 s，峰值 RSS 5191 → 3386 MB，总时长 95.8 → 69.3 s | 保留 |

> 未保留项：无（本轮 6 项均达到 ≥5% 或明确的护栏收益）。回退方式为按提交 `git revert`，不涉及 schema/依赖/公共契约。

## 5. 最终 A/B（优化后）

基准：`72cb684`（本任务 harness 复测基线，n=3，二进制 `47e3c6a0…`）；
优化后：`f04d1a88`（n=3，二进制 `3e55a4c42ac49ffd…`）。
所有数字为 3 次运行的中位数，report 由 harness 校验器封存（`integrity_sha256`）。

| 规模 | 初始 sync 前→后 | Δ | 空转 p50 前→后 | 空转 p95 前→后 | search p50 前→后 | search p95 前→后 | 峰值 RSS 前→后 | Δ RSS | catalog.db 前→后 |
|---|---|---|---|---|---|---|---|---|---|
| 10k | 2.18 → 1.52 s | -30.4% | 19.6 → 15.9 ms | 19.6 → 19.5 ms | 113.0 → 105.2 ms | 155.3 → 136.9 ms | 154 → 149 MB | -2.8% | 49 → 49 MB |
| 100k | 28.66 → 20.04 s | -30.1% | 61.6 → 84.8 ms | 63.1 → 95.1 ms | 111.1 → 114.7 ms | 145.3 → 140.3 ms | 1439 → 1361 MB | -5.4% | 494 → 491 MB |
| 1M | 412.72 → 257.90 s | -37.5% | 582.5 → 616.1 ms | 6785.8 → 3319.7 ms | 116.3 → 113.8 ms | 154.3 → 151.7 ms | 4575 → 2859 MB | -37.5% | 4195 → 4157 MB |

- **1M 初始 sync**：412.7 s → 257.9 s（**-37.5%**）；相对归档基线 `2b8f895`（482.6 s）为 257.9 s（-46.6%）。
- **1M 峰值 RSS**：4575 MB → 2859 MB（**-37.5%**）；相对归档基线（4.58 GB）为 2.79 GB。
- **护栏**：search p50/p95 各规模均未回退（1M p95 −19.8%、100k p95 −3.4%）。空转在 harness 的 100k 窗口中采样噪声较大（±40%，与前后 1M 运行的缓存/杀毒扰动相关）；同窗口配对交错 A/B（`ab.py`，2 对，基线 vs 优化后）给出 noop 中位 76.9 → 75.4 ms（**-2.1%**），且该提交在机制上只删除工作：1M 首个（冷）空转批次 6.9 s → 0.7–1.1 s。
- 100k 空转原始样本（ms）：基线 [[62.8,56.9,58.8],[67.4,67.7,61.8],[61.6,58.6,63.1]] vs 优化后 [[65.8,69.7,58.3],[71.2,79.5,82.6],[65.9,69.7,59.8]]；1M 首个空转批次（冷）6.9 s → 0.7–1.1 s（消除双读/双哈希的直接证据）。

原始证据：`research/results/baseline/*/search-baseline-full.json` 与 `research/results/after/*/search-baseline-full.json`（各 9 份，校验器 `baseline.py validate` 可重算）。

## 6. 正确性与等价性

- `cargo test -p agent-session-grep-adapters-sqlite --offline`：270 通过（含 outbox/CAS/generation、durable intent 篡改拒绝、关系完整性、身份保真、relocation）。
- 等价性检查（`research/scripts/verify_equiv.py`）：同一语料分别用基线二进制与优化后二进制写入新鲜库，比较 16 张表（catalog payload、placements、edges、activities、usage、fts、fts_ids、session_fts(_ids)、四张 membership、source_scans、relation_scans、resume claims、index_batches 生命周期）逐字节一致；10k（1 批）与 100k（15 批）均 0 不一致。
  - 说明：`index_batches.operation_digest` 内含 installation 分配墙钟（`created_at_ms`），两次运行天然不同，比较时只看生命周期字段。
- durable intent 篡改测试（`commit_rejects_tampered_durable_*_manifest`）在 manifest 复用后仍拒绝写入并保持 generation 不变。

## 7. 未达标项：峰值 RSS 归因与可选路线

结论：**在不改 schema 的前提下，本轮的组合优化把 1M 峰值 RSS 从 4574.6 MB 降到 2859 MB（约 -38%），未达到 ≥50%（≤2287 MB）**。按 PRD 约定，此处停止继续试错，给出归因与路线：

归因（按证据强度排序）：

1. **批次内数据副本（batch-proportional，~2.6 GB/200k 消息，与库大小无关）**
   内存诊断显示第 6 批（写入 1M 库）RSS 曲线在解析/装配 + 提交早期即冲高；空库 200k 批同样达到 2675 MB。当前提交路径对同一实体同时持有：`SourceBatch.entries`（payload+text）、`merged` 克隆、`PreparedSource.observed_placements` 克隆、`observed_placements/edges`，以及 `relations.canonical_json()` 生成的 manifest 字符串（10k 语料即 7.6 MB relation JSON，线性外推 200k ≈ 150 MB/份，最多 2–3 份同时存活）。
2. **别名再生的整表读取（O(catalog)）**：`session_projection` 在优化后仍是 1M 单批最大项（31.3 s/批，且 `stored_placements_from` + `stored_edges_from` + 两份按 message/session 的克隆索引仍在整表加载）。这正是下一步最确定的范围化目标（与 #6 同一手法）。
3. **SQLite 工作集**：写连接 128 MiB page cache（候选 2 换取 -29% 时间）+ 单事务 WAL 追加使进程工作集包含大量刚写页面；这一部分随写入字节线性增长，受 `synchronous`/单事务原子性约束，不能通过拆事务换取。

可选路线（按代价从低到高）：
- **A. 继续范围化（不改 schema）**：把别名再生的 placements/edges 读取限于候选实体（`message_id`/`session_id`/`child_placement_id` 均有索引）；把 `PreparedSource.observed_placements/edges` 改为引用而非克隆；manifest 的 relations JSON 改为流式哈希 + 单副本（需保持 durable intent 字节语义，属于行为敏感区）。预计可再降 ~0.6–1.0 GB。
- **B. 限制单批规模/流式化**：RSS 与批内消息数近似线性（200k→2.6 GB）。CLI 侧对一次 sync 的“解析缓冲 + 提交”改为按源/按块流水（先提交一批再解析下一批）会改变“一次 sync 一个原子批”的语义，需产品决策；仅在调用方切小批次时，RSS 可线性下降但总时间上升。
- **C. schema/投影改造**：为 `source_membership(message_id)`、`source_placement_membership(placement_id)` 之外的别名来源建覆盖索引；或新增“实体→会话/文档/span 别名”物化投影表（随提交维护），可把别名再生从 O(catalog) 降到 O(batch)，同时消除整表读。属 schema 迁移，本轮禁止。
- **D. 关系/成员表按需读取 + 增量校验**：把 `stored_*` 从“按候选 id 查询”进一步改为“按变更集增量维护”（本任务已完成候选 id 版），可减少每批 chunked 查询数量。

## 8. 复现命令

```powershell
# 基线（在 c2d58e8 参考 worktree 中构建的二进制）
python -B .trellis/tasks/09-29-index-throughput/research/harness/baseline.py run `
  --workspace C:/AgentSessions-worktrees/index-throughput-baseline `
  --binary C:/AgentSessions-worktrees/post-reuse-phase/target/index-throughput/baseline-target/release/agent-session-grep.exe `
  --scratch-dir <fresh> --output-dir <fresh> --environment <harness>/environment.json `
  --profile full --scales 1000000 --expected-commit c2d58e862b4fe0320240f8a40eefef09d958ff55

# 优化后（主 worktree，提交见 §4）
python -B .trellis/tasks/09-29-index-throughput/research/harness/baseline.py run `
  --workspace C:/AgentSessions-worktrees/post-reuse-phase `
  --binary target/release/agent-session-grep.exe --scratch-dir <fresh> --output-dir <fresh> `
  --environment <harness>/environment.json --profile full --scales 1000000 --expected-commit <HEAD>

# 校验器（拒绝篡改）
python -B .trellis/tasks/09-29-index-throughput/research/harness/baseline.py validate <report.json>
```
