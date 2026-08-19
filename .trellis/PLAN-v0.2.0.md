# agent-session-grep v0.2.0 终局任务规划

> **接手的 Agent 从这里开始。** 这份文件是自包含的:读完它你就有全部上下文,
> 不需要回溯任何对话记录。
>
> - 目标:把功能、性能、文档三条线全部拉满,然后把 `qin-devs/agent-session-grep`
>   从 PRIVATE 转为 PUBLIC,发 `v0.2.0`。
> - 当前版本:`v0.1.0` 已打 tag、已发 Release,但仓库**仍是 PRIVATE**
>   (owner 在发现内部过程泄漏后主动转回私有)。
> - 本文件所在目录 `.trellis/` 被 `scripts/release/export_public_tree.py` 排除,
>   因此可以写真实语料路径、CI 计费、私有仓库名等不能进公开树的内容。
>   **不要把本文件的内容复制进 `docs/`。**

## 0. 如何使用这份文件

1. 读 §1 拿到当前事实状态,读 §2 拿到已锁定的决策(**不要重开这些决策**)。
2. 从 §4 里程碑里挑第一个未完成任务(按 ID 顺序,`M1` → `M5`)。
3. 每个任务都写了**验收标准**和**证据落点**。做完后:
   - 在 §7 进度日志追加一行(日期 + 任务 ID + 结论 + commit)
   - 把任务标题前的 `[ ]` 改成 `[x]`
4. 遇到 §5 里的待调研问题:先查,查不出再问 owner,不要自己拍。
5. 质量门(每次 commit 前必须绿):
   ```
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   python -m unittest discover -s scripts -p "test_*.py"
   python -m unittest discover -s scripts/release -p "test_*.py"
   python -m unittest discover -s scripts/evidence -p "test_*.py"
   python scripts/evidence/privacy_scan.py --repo . --profile repo
   ```
6. **Git 纪律**:owner 已授权自主 commit/push/merge,但 Conventional Commits
   必须遵守,不加 AI co-author 脚注,不 push 到公开仓库 main 除非该里程碑已收口。

## 1. 当前事实状态(2026-08-18 核实,全部有出处)

### 1.1 已发布物

| 项 | 状态 | 出处 |
|---|---|---|
| 公开仓库 `qin-devs/agent-session-grep` | 存在,**PRIVATE** | `gh repo view` |
| `v0.1.0` tag | 已推送,指向 `55648d6` | `git ls-remote --tags` |
| GitHub Release `v0.1.0` | 已发布,非 draft,0 assets | `gh release view` |
| 公开仓库 commit 数 | 1(压缩的初始 commit) | `gh api .../commits` |
| 私有开发仓库 `qin-devs/AgentSessions` | PRIVATE,是开发源 | — |
| 公开树文件数 | 482(排除 `.trellis/` `.claude/` `.codex/` `.codebuddy/` `.agents/` `scripts/evidence/out/`) | `export_public_tree.py` |

### 1.2 代码规模与结构

六边形分层,依赖方向 `domain ← ports ← application ← adapters-sqlite ← cli`:

- `agent-session-grep-domain` — 实体与稳定 ID
- `agent-session-grep-ports` — trait 契约 + 能力矩阵 + 有界读取器
- `agent-session-grep-application` — 用例编排(唯一放行为的地方)
- `agent-session-grep-adapters-sqlite` — SQLite/FTS5 存储(单文件约 1.5 万行)
- `agent-session-grep-cli` — 五入口:CLI/Robot JSON/MCP/TUI/serve
- `agent-session-grep-provider-*` — 14 个已实现 provider
- `agent-session-grep-testkit` — 仅 dev-dependency

SQLite schema 版本 `SCHEMA_VERSION = 12`。

### 1.3 性能实测(已提交的证据,不是估算)

> **⚠️ 2026-08-19:10 万条实测已到手,推翻了本节多处基于 4000 条的推断。
> 先读 §1.3.1,那才是当前权威数字;下面的 4000 条表保留作纵向对照。**

来源 `docs/evidence/core-beta/88d86f4/core-beta-benchmark-full.md`,
**语料仅 4000 条合成消息**,Windows x86_64:

| 指标 | P50 | P95 | 说明 |
|---|---:|---:|---|
| `cli_startup_cold_latency_ms` | 14.6 | 20.4 | 冷启动 |
| `cli_startup_warm_latency_ms` | 8.4 | 11.7 | 热启动 |
| `initial_sync_latency_ms` | 1889 | 2511 | 4000 条 |
| `noop_sync_latency_ms` | 1495 | 1810 | **无变化也要 1.5 秒** |
| `shrink_sync_latency_ms` | 4482 | 7104 | 最慢操作 |
| `search_latency_ms` | 17.7 | 22.4 | 语料太小,不可外推 |
| `show_latency_ms` | 19.0 | 24.2 | — |
| `get_latency_ms` | 18.7 | 25.4 | P99 达 113.9 |
| `initial_index_throughput_mb_s` | **0.566** | 0.591 | **核心问题** |

存储放大 **5.11 倍**(源 1,122,164 B → 库 5,738,496 B)。

推算:owner 真实语料 1.18 GB → 首次全量索引约 **35 分钟**;10 万条消息库体积
约 **1.5 GB**。

`recovery` 标记 `not_implemented`(无 fault-injection 入口,不宣称恢复耗时)。

### 1.3.1 十万条实测(2026-08-19,**当前权威**,四条阀值三条失败)

> **⚠️ 本节的判定已被后续优化改写。当前状态见 §1.3.2 —— 四条里三条已通过,
> 只剩吞吐一条。下表保留为优化前的基线。**

语料 = `scripts/evidence/synthetic_corpus.py` 的冻结 10 万条(5,000 session /
6 provider / 48.6 MiB),harness = `scripts/evidence/performance_gate_benchmark.py`。
**我独立重跑复验过,四个数字全部吻合**(括号内是 agent 首次运行值):

| 动作 | 实测 | 阀值 | 判定 |
|---|---:|---:|---|
| 首次全量索引 | **0.707**(0.697)MiB/s | ≥ 3.5 | **FAIL —— 差 5.0 倍** |
| 增量同步(无变化) | **963.19**(963)ms | ≤ 1000 | **PASS**(仅剩 37 ms 余量) |
| 搜索 p95 | **96.03**(95.8)ms,n=100 | ≤ 50 | **FAIL —— 1.9 倍** |
| MCP 单次调用 p95 | **85.23**(86.5)ms,n=100 | ≤ 50 | **FAIL —— 1.7 倍** |

**这组数字推翻了原有的三处判断:**

1. **原以为会失败的那条通过了,原以为宽裕的两条失败了。** §1.7 瓶颈 #4
   (noop sync 1.5 s @4000)推断"10 万条下线性放大" —— **没有线性放大**,
   实测 963 ms 通过。而 §2 把 MCP 那条写成"已实测 21 ms,守住",搜索也被当成
   宽裕项 —— **两条都以约 1.8 倍失败**。
   → **M2-3 / M2-5 / M2-6 的优化目标需要重排**:它们瞄准的正是唯一通过的那条。
   (但 963 ms 只剩 37 ms 余量,轻微回退就会翻盘,所以不能直接放弃。)

2. **§1.4 的 "MCP p50 16.6 / p95 21.0 ms" 在默认构建下不可复现。**
   默认构建 `model status` 报 `feature: null, present: false`,向量后端是
   `bigram-hash-v1` —— **没有编码器可以常驻,阀值的"编码器常驻"前提根本不成立**。
   那组 21 ms 来自 semantic-candle 构建 + **10 条消息**语料(n=10);
   10 万条下常驻进程实测 85 ms。每个 `mcp_*` metric 现在自带
   `encoder_resident`、实际后端与作用域说明,不再让读者跨节交叉引用才发现
   数字的含义比阀值措辞窄。

3. **搜索 p95 的大头是进程启动。** `cli_process_overhead_p50_ms` = **21.7 ms**
   (裸 `status` 调用)。96 ms 里约 22 ms 与查询无关。一次性 CLI 想达
   50 ms p95 只剩约 28 ms 预算 —— 这条阀值放在常驻 MCP 路径上才诚实,
   而它在那里也以 85 ms 失败。

**另两个实测事实**:
- `store_size_ratio` = **9.49 倍**(不含向量索引;含向量索引 13.6 倍)。
  §1.7 记的 5.11 倍是 4000 条下的值,10 万条下放大比本身在变大。
- **"单次 sync"实际是 27 个进程**:5,000 条路径超出命令行上限,harness 分 27 批。
  所以 963 ms 里很大一部分是 27 × 约 36 ms 的进程启动,不是扫描工作。
  (这正是 M2P-17 `--from-file` 要解决的问题 —— 该 flag 已落地,
  harness 尚未改用它。)

**CI 可行性**:全量 372 s 不适合逐 PR,但其中 285 s 是只喂 informational
metric 的 embeddings 构建。`--skip-embeddings` 在**完整 10 万条规模**下门住
四条阀值,耗时 **79 s**(我实测 81.9 s),判定完全相同 —— 这是建议的逐 PR 子集;
全量放 nightly / 发布前。

### 1.3.2 优化后实测(2026-08-19 晚,**当前权威**,四条只剩一条失败)

同一 harness、同一冻结语料,合并三轮优化后我亲自重跑(70.7 s):

| 动作 | 优化前 | **优化后** | 阀值 | 判定 |
|---|---:|---:|---:|---|
| 首次全量索引 | 0.707 | **0.751** MiB/s | ≥ 3.5 | **仍 FAIL(差 4.7 倍)** |
| 增量同步(无变化) | 963 | **658** ms | ≤ 1000 | **PASS**(余量 37 → 342 ms) |
| 搜索 p95 | 96.03 | **31.55** ms | ≤ 50 | **PASS**(原 1.9 倍超标) |
| MCP 单次调用 p95 | 85.23 | **20.55** ms | ≤ 50 | **PASS**(原 1.7 倍超标) |

`gate.pass = false`,`failures = ['initial_index_throughput_mib_s']` —— **唯一
未过的是吞吐**。

**搜索与 MCP 的 3 倍改善来自同一处**:把 lexical top-k 的物化收敛到"只为
存活者付代价",与之前修 semantic 全表扇描是同一形状。踩坑记录在
`bm25_cutoff_probe_reads_no_content_column` 测试的注释里:`id` 是 fts5 content
column,在 `ORDER BY` 里提它会让 SQLite 为每个匹配行读出整条 content 记录
(`id` + 全部 `text`)只为喂 sorter —— 10 万条语料下一个命中 1.6 万行的词
从 7 ms 变成 47 ms。这类"结果正确但慢"的缺陷行为测试抓不到,故用查询计划断言锁住。

**吞吐的真正瓶颈已定位(这是 §1.7 瓶颈 #2 被推翻后的正确答案)**:
分阶段 trace 显示 **commit 占 98.2%**(38,400 ms / 39,131 ms),parse 仅 1.6%。
且 **代价是 catalog 大小的平方,不是批大小的线性**:
```
batch  1  files=167  commit=  833ms
batch 22  files= 17  commit= 1548ms   ← 文件少 10 倍,耗时仍是 1.9 倍
```
根因:`commit_source_batches_if_changed` 每次调用发六条**无 WHERE 的全表扫描**
(`source_entity_membership_state`、`source_placement_membership_state`、
`source_activity_membership_state`、`stored_placements`、`stored_edges`、
`stored_activities`),于是每批都把整个正在增长的 catalog 重新读进 BTreeMap。
代码注释自己承认了 O(whole catalog),并加了 no-op 快路径来规避 ——
但首次索引时每批都是变更,所以每批都全额付费。
**已派 agent 修**:把这六条读收敛到批内 sources。难点在于跨源 claim/tombstone
推导确实需要非局部知识(一条消息可被多源 claim;tombstone 靠"claim 消失"推导),
不能简单加 `WHERE source_id IN (batch)`。



### 1.3.3 alias regeneration 收敛后实测(2026-08-19 深夜,**当前权威**)

合并 alias regeneration 批内收敛(`af9f1c9`)后我连跑两次:

| 动作 | 第一次 | 第二次 | 阀值 | 判定 |
|---|---:|---:|---:|---|
| 首次全量索引 | 0.762 | 0.692 MiB/s | ≥ 3.5 | **仍 FAIL** |
| 增量同步(无变化) | **1058.9** | 863.7 ms | ≤ 1000 | **两次不一致** |
| 搜索 p95 | 49.1 | 35.1 ms | ≤ 50 | PASS |
| MCP p95 | 27.7 | 21.5 ms | ≤ 50 | PASS |
| `cli_process_overhead_p50_ms` | **30.9** | 26.7 ms | (informational) | ← 噪声指示器 |

**⚠️ 这台机器的测量噪声已经大到能翻转判定,必须记下来:**
第一次 noop 1058.9 ms **超阀值**、第二次 863.7 ms 通过 —— 同一个二进制、同一
语料。同期 `cli_process_overhead_p50_ms`(裸 `status` 调用,纯进程启动)
从 21.7 ms 涨到 30.9 ms 再回落到 26.7 ms,**这就是噪声源**:整机负载
(当时有多个 cargo/agent 在跑)。
**教训:单次 gate 运行不足以判定阀值附近的指标。** noop 的余量本来就只有
一两百毫秒,而进程启动开销的抖动就有 ±9 ms × 27 批 ≈ ±240 ms —— 足以单独
翻转它。
→ **给后续 agent 的规则:noop 与搜索这两条贴近阀值的指标,必须连跑两次取
一致结果;不一致就说明当时机器负载不可信,不要据此声称回归或改进。**
(这也是为什么 peer agent 报的 0.999 与 0.881 是同一个二进制 —— 它明确
说了"信比值不信绝对值",那是对的。)

**吞吐的下一步在哪里(peer 的实测结论,可直接采信)**:
六条 commit 读的收敛**已实现并已回退** —— 实测它不是瓶颈(0.732 → 0.678
带索引 / 0.723 不带),且它需要的 `source_membership(message_id)` 索引在写
路径上要付约 6%,而 id-scoped `IN` 分块在批次覆盖大部分 catalog 时比一次顺序
扫描更慢,反而把 noop 拖过阀值。**一个正确但零收益、还拖垮受门指标的改动
不值得留下** —— 这个判断是对的。
真正的平方项在更深一层:`regenerate_compatibility_aliases_in_tx` 每次提交在
**写事务内**发四条无过滤扫描,并对全 catalog 的 placement 重建两个排序索引。
收敛它得到 +35%(配对测量 0.653 → 0.881)。
**剩下的差距不在 alias**:peer 实测**完全跳过** alias regen 也只到 0.96 MiB/s,
所以剩余开销由**写入与 FTS** 主导。3.5 MiB/s 需要的是那一层的改动
(批量写入策略 / FTS 插入路径),不是继续收敛读。

#### 1.3.4 写路径实测 + **3.5 MiB/s 在当前设计下不可达(2026-08-19,已合并)**

先加 `ASG_COMMIT_TRACE=1` 拿到**事务内逐步耗时**(此前全是推断)。
100k 语料 / 29 个 sync 进程 / 67.0 s 基线:

| 步骤 | ms | 备注 |
|---|---:|---|
| `tx.commit_fsync` | 13280 | SQLite 自身提交 |
| `tx.alias_regen` | 11623 | |
| `tx.placement_edge_upsert` | 7743 | |
| `catalog_state_reads` | 5730 | 六次全表读 |
| `tx.fts_upsert` | 4642 | FTS5 |
| `manifest_validate` | 4199 | **结果被丢弃** |
| `tx.source_replacements` | 3131 | |
| `tx.session_fts_rebuild` | 2594 | |
| `begin_intent` | 2610 | 重算同一个 manifest |
| `tx.affected_sessions` | 1706 | 20 万次单 id 查询 |

**这份测量又杀掉两个推断假设**:FTS 插入早已批量、只占 9%;
`source_batches_are_current` 在 ingest 路径只花 **3 ms**(遇到第一个新实体就
短路),所以"六次读发生两次"这个猜想在此路径上不成立。
→ §1.7 的警告再次得到印证:**没测过的瓶颈条目一律当推断看待。**

**三处改动,各自配对测量**:
1. `page_size=16384` / `cache_size=-262144` / `temp_store=MEMORY`
   —— 事务是**缓存受限**而非 I/O 策略受限:2 MiB 缓存装不下
   `message_placements` 的四个索引内部页。**67.0 → 43.3 s**。
   `synchronous` 未动,耐久性中立;已存库保持原 page size(非空库上该
   pragma 是 no-op),实测旧 4 KiB 库仍能打开、过 `integrity_check`、可写、
   可 no-op 重同步。
2. 去掉重复计算:`batch_manifest` 连算两次(第一次结果丢弃)、
   `collect_message_sessions` 被以单元素切片调用 20 万次、alias regen 每轮
   把同一 payload 从 catalog 读两次。**43.3 → 37.5 s**。
3. 去掉五个全表 loader 的多余 `ORDER BY`(按声明的 PRIMARY KEY 排序会让
   SQLite 走 PK 索引再逐行按 rowid 取回,而目的地 `BTreeMap` 本来就会重新
   排序)。**37.5 → 33.5 s**。

**官方 harness,各跑两次(遵守 §1.3.3 的两次一致规则)**:

| | run 1 | run 2 |
|---|---:|---:|
| 改前(`2fbb700`) | 0.73 | 0.61 MiB/s |
| 改后 | **1.36** | **1.26** MiB/s |

≈**1.96x**,仍 FAIL 3.5。顺带改善:`noop_sync_latency_ms` 1069.0/905.1
(第一次**曾超**1000 ms 阀值)→ **909.6/805.5**(两次都过);
`search_latency_p95_ms` 143.9/45.2 → 42.8/37.6;`store_size_ratio` 9.51 → 9.27。

**正确性**:Gate D 7/7;**输出逐字节相同** —— 七张表加 `source_scans`
(减 `scanned_at_ms`)的 SHA-256,两次独立 sync 之间相同,**且与改动前的
二进制全部 8 个摘要相同**;tombstone 数字与基线一致。

**耐久性没有动,而且这是测出来的决定**:实测 `synchronous=NORMAL`
反而**更慢**(36.7 s vs 33.5 s)—— 256 MiB 缓存下提交耗时由**写约 17 MiB
WAL 帧**主导,省掉的那一次 fsync 是舍入误差。开关做了、测了、然后删了,
**不留一个拿保证换零收益的旋钮**。

**⚠️ 结论:3.5 MiB/s 靠调优此设计不可达。**
> **❌ 这句话是错的 —— 已被 §1.3.6 的实测推翻。下面的算术没错,但它算的是
> "509 字节/消息的语料"的天花板,不是"这个设计"的天花板。保留原文不删,
> 因为它正是"没测过就别下结论"这条教训本身的例子。**

只把"真正写入本存储要保存的事实"那些步骤相加
(`catalog_upsert` 0.20 + `fts_upsert` 2.74 + `placement_edge_upsert` 1.80 +
`source_replacements` 0.95 + `commit_fsync` 7.15 = 12.8 s),
再加任何提交都省不掉的开销(parse、verify、29 次进程启动 ≈ 3.9 s)
= **16.7 s ≈ 3.0 MiB/s**。这是把 durable intent、claimer 图、alias 重生成、
session 元数据投影与全部完整性校验**全部压到零**之后的天花板 —— **低于 3.5**。

两处结构性成本(已写进源码注释留给后续):
- **提交前的读是 O(catalog) per commit**,于是全量 ingest 是 O(n²/batch)。
  它们为了让 tombstone 推导安全而载入整张 claimer 图。`IN` 分块作用域
  已实测更慢;正解是**不再需要每批次拿整张图**(例如维护一个 claimer 计数
  列),而不是换个方式去查它。
- **每条消息 payload 每次提交被写两遍** —— `catalog_upsert` 插入后,
  `alias_regen` 又读回来重写以拼入 `session`/`spans`/`parent`/`is_sidechain`。
  这些别名派生自同一批次内存里已有的 placement 与 edge,**在插入前投影**、
  只对被另一个源同时认领的实体回退到 read-modify-write,就能把一次全 catalog
  重写从热路径上摘掉。

→ **D4 的 3.5 MiB/s 阀值需要一个决定**:要么接受它在 0.2 不可达并把阀值
改成有依据的值(实测天花板 3.0,当前 1.36),要么把上面两处结构性改动
排进 0.3。**不能继续把一个已证明不可达的阀值挂在发布门上**(§3 G4)。

#### 1.3.5 ⚠️ 先别动阀值:**受门语料的密度比真实语料低 14 倍**(待验证假设)

在改阀值之前必须先排除一个可能:**我们可能一直在测一个比 D4 实际要求
苛刻得多的东西。**

| 语料 | 文件 | 字节 | 消息 | **字节/消息** |
|---|---:|---:|---:|---:|
| 受门合成语料(100k) | 5,000 | 50,920,199 | 100,000 | **509** |
| 真实冻结语料 | 1,242 | 1,177,479,794 | 164,136 | **7,174** |

**相差 14.1 倍。** 而 §1.3.4 的逐步测量显示提交成本几乎全部是**按条**发生的
(`placement_edge_upsert`、`fts_upsert`、`catalog_upsert`、`affected_sessions`
的 20 万次查询)——**没有一项与消息字节数成正比**。

**假设**:若成本确实按条而非按字节,那么同样的每条成本在真实语料上会得到
约 14 倍的 MiB/s,因为分子(字节)大了 14 倍而分母(条数×每条成本)不变。
1.36 MiB/s × 14 ≈ 19 MiB/s —— **远超 3.5**。

**这不是用来搪塞阀值的说辞,是一个可判定的实验**:
固定消息条数、把消息体长度提到真实语料的密度(约 7 KB/条),重测 MiB/s。
- 若 MiB/s 显著上升 → **阀值没问题,受门语料选错了**。
  D4 说的是"1 GiB 语料 5 分钟",而 1 GiB 的**真实**语料只有约 15 万条消息,
  不是合成语料那样的 210 万条。修法是让受门语料的密度贴合真实分布,
  而不是降低阀值。
- 若 MiB/s 基本不变 → 成本确实按字节,§1.3.4 的天花板论证成立,
  按上一条改阀值。

**在这个实验有结果之前,不要改 `PERFORMANCE_THRESHOLDS`。**
把阀值调到刚好能过是本仓库明令禁止的
(`performance_gate_benchmark.py:84-85`:"a threshold tuned to fit a
measurement gates nothing")。

#### 1.3.6 ✅ 密度实验结果:**机制假设被推翻,但结论成立**(2026-08-19,已合并)

**这一条要连着上一条读:我提的机制是错的,而我提的结论是对的 ——
如果只看结论就以为"猜对了",下次还会用同样的错推理。**

生成 100k 消息 / 5,000 session、`--body-scale 25.66` 的密语料:
**7,002 字节/消息**(真实语料 7,174,差 2.4%),provider 分布、每会话条数、
CJK 比例与冻结语料一致。

| | MiB/s |
|---|---|
| 密语料(4 次,含 2 次与稀疏交错配对) | **6.11 / 6.13 / 6.57 / 7.83** |
| 稀疏对照(同批,4 次) | 0.79 / 0.99 / 1.37 / … |

**最差的一次密语料仍是 3.5 阀值的 1.75 倍。**
(注:测时机器负载偏高 —— `cli_process_overhead_p50_ms` 跑到 22.7–46.4 ms
对文档记录的 21.7–30.9,稀疏基线从 1.36 退到 0.79–0.99。所以密语料这组数字
是**偏保守**的,不是被美化的。)

**❌ 机制:推翻。成本并非"几乎全部按条"。**
messages/s 从 ~2000 掉到 ~950,**腰斩**。用三组独立配对解
`ms_per_message = a + b·bytes`,得 `b` = **76.7 / 77.7 / 92.8 ns/字节**
(三次复现落在 20% 内)。也就是说**真实密度下约一半成本是按字节的**。
→ 我预测的 ~19 MiB/s(1.36 × 14)根本不可能出现:仅按字节那一项就把
本设计压在 **10–12 MiB/s** 附近。**我把"按条"当成了全部,实际它只是一半。**

**✅ 结论:证实。受门语料不具代表性。**

| 密度 | 1 GiB 内消息数 | 投影耗时 | 要求 |
|---|---:|---:|---|
| 真实 7,174 B/条 | 149,671 | **2.13–2.73 min** | ≤ 5 ✅ |
| 密语料 7,002 B/条 | 153,338 | 2.18–2.79 min | ≤ 5 ✅ |
| 受门 509 B/条 | 2,108,676 | 12.5–17.2 min | ≤ 5 ❌ |

**同一个二进制、同一台机器:真实密度的 1 GiB 达标,合成密度的 1 GiB 差 2.5–3.4 倍。**
D4 要求的是"1 GiB 语料 5 分钟",而 1 GiB **真实** transcript 只有约 15 万条消息,
不是合成语料那样的 210 万条 —— **同样叫"1 GiB",工作量差 14 倍**。

**→ 决定:阀值保持 3.5,不动。改的是受门语料的密度。**
`--body-scale` 旋钮已实现并有测试。
**护栏(比结论更重要)**:非冻结密度的语料现在产出 `gate.pass = null`
(新增 `STATE_OFF_CONTRACT`),且校验器**拒绝**任何声称 threshold scale
却带非冻结 `body_scale` 的 manifest —— 这个旋钮**不能**被用来把阀值糊过去。
冻结语料的 `fixture_hash` 未变,仍逐位可复现。

**换语料前要先解决的两件事(尚未做,不要当已完成)**:
1. 语料从 49 MiB 涨到约 668 MiB、首次索引从约 35 s 涨到约 100 s ——
   **这会改变 CI 的可行性**,不是纯收益。
2. 实验只对齐了**均值**字节/消息,没有对齐**长度分布**:均匀放大把长尾类
   拉到 204k 字符的极端值。要对齐形状,需要真实语料的长度分位数,
   **而本仓库目前没有记录这项测量**。

**顺带发现**:`store_size_ratio` 稀疏 **9.267** vs 密 **3.557** ——
§1.3.2 记录的 9.27 倍存储放大**同样是密度假象**,不是真实用户会遇到的比率。
(这也让 M2-5"不做 external-content"那个决定的依据需要在 0.3 复核:
它是基于 9.27 倍做的判断。)




### 1.4 MCP amortized 语义延迟(已实测)

> ⚠️ 见 §1.3.1 第 2 条:这组数字的前提(编码器常驻)在默认构建下不成立,
> 且语料只有 10 条消息。10 万条常驻进程实测:优化前 p95 85 ms,
> 优化后 **20.55 ms**(§1.3.2)。

`scripts/evidence/semantic_mcp_latency.py`:编码器常驻 MCP 进程时
p50 **16.6 ms** / p95 **21.0 ms**(10 次查询),对比单次 CLI 冷启动约 **3 秒**
(每次进程都要加载 470 MB 权重)。结论:长驻入口才适合 semantic。

### 1.5 semantic 召回实测(真实 E5 权重)

冻结语料 200 session / 2000 消息 / 100 条 gold query:

| 模式 | recall@5 | recall@10 | recall@20 |
|---|---:|---:|---:|
| lexical | 0.705 | 0.750 | 0.750 |
| semantic | 0.720 | 0.755 | 0.775 |
| hybrid | 0.735 | 0.755 | 0.775 |

**semantic 只比 lexical 好 2 个百分点**,代价是 470 MB 权重 + 冷启动 3 秒。
`gate.promotion_claim = none`,`lexical_stays_default = true`,`maturity = beta`。
这组数字是"semantic 保持 opt-in"决策的依据。

模型 id:`intfloat-multilingual-e5-small@614241f6-candle-f32-meanpool-l2-qpass-v1`
权重 pin:revision `614241f622f53c4eeff9890bdc4f31cfecc418b3`,
`model.safetensors` SHA-256 `1a55775f53449dac10a2bcbc312469fac40b96d53198c407081a831f81c98477`
(470,641,600 字节)。国内经 `hf-mirror.com` 下载(huggingface.co 直连失败)。

### 1.6 Gate D 七条不变量(**真实语料上已全绿**)

定义在 `scripts/evidence/real_data_regression.py:47-53`:

1. `INV-SYNC-OK` — sync 退出 0 且 `ok: true`
2. `INV-NO-PARSE-LOSS` — provider emitted == persisted,skipped 为 0
3. `INV-SESSION-PRESENT` — 每个源文件都归属到 session
4. `INV-CONTEXT-NONEMPTY` — session context 可组装
5. `INV-SPAN-COVERAGE` — span 有字节精度
6. `INV-REBUILD-STABLE` — rebuild 后 catalog 数量与采样词一致
7. `INV-SOURCES-UNCHANGED` — 源文件校验和未变(只读保证)

真实语料实测(`docs/evidence/integration-beta/real-data-regression.md:40-46`):
1242 源 / 1,177,479,794 字节 / 164,136 emitted / 0 skipped / 150,091 去重
message / 231 session,**七条全 PASS**,harness 退出 0,报告仅含聚合数字。

⚠️ **已知文档矛盾**(M4 要修):`go-no-go.2026-08-16.md:46` 写
"Gate D invariants (all six) — pending",指的是**合成 rehearsal 语料**没跑;
真实语料早已绿。且八处文档还写"六条",实际是七条。

### 1.7 已确认的四个真实瓶颈(读代码得出,非猜测)

| # | 位置 | 问题 | 10 万条下的影响 |
|---|---|---|---|
| 1 | `adapters-sqlite/src/lib.rs:6613-6618` `query_semantic` | **全表扇描**:`SELECT ... FROM message_vec WHERE model_id=?1 AND dimension=?2`,无候选集过滤,全部向量读进 Rust 算余弦 | 每次查询读 10万×384×4 ≈ **150 MB** |
| 2 | 全仓库 | **ingestion 完全单线程**。`grep thread::spawn` 只在 `serve.rs` 命中,解析路径零并行 | ~~0.566 MB/s 的主因~~ —— **这条推断已被实测推翻**:并行化 parse 后吞吐 0.707 → **0.665 MiB/s(反而略降)**,证明 **parse 从来不是瓶颈**。真正的开销在别处(候选:commit 事务、BLAKE3 capture/verify)。见下方说明 |
| 3 | `adapters-sqlite/src/lib.rs:1365` | FTS5 是**普通表非 external-content**:`fts5(id UNINDEXED, text)`,文本在 catalog 和 fts 各存一份 | 存储放大 5.11 倍的直接来源;~~**10 万条实测放大到 9.49 倍**(§1.3.1)~~ —— **9.49/9.27 这个数字本身是密度假象**:同一二进制在真实密度语料上只有 **3.557 倍**(§1.3.6)。真实用户遇到的是 3.6 倍,不是 9.3 倍 |
| 4 | noop sync 1.5 秒 | 无变化重扫仍要 1.5 秒(4000 条) | ~~10 万条下线性放大~~ —— **这条推断已被实测推翻**:10 万条实测 963 ms,**通过** 1 s 阀值(§1.3.1)。它没有线性放大,而且余量只剩 37 ms |

**⚠️ 到目前为止,这张表里被实测推翻的推断已有四条**(#2「parse 是主因」、
#4「会线性放大」、#3「放大 9.3 倍」,以及 §1.3.4 我自己写下的「3.5 不可达」)。
**规则:本表每一行都是读代码得出的推断,在有配对实测之前不得当作事实引用,
也不得据此声称某项优化"应该"有效。** 四次里没有一次推断是全对的。

**⚠️ 优先级已按 §1.3.1 实测重排。** 上表按"读代码推断的严重度"排序,而实测
判定是:瓶颈 #2(吞吐 0.707 MiB/s,差 5 倍)是唯一**既确认失败又差距最大**的;
瓶颈 #4 **通过**;搜索与 MCP 延迟(不在上表内,原被当成宽裕项)**各以约 1.8 倍
失败**,且其中约 22 ms 是进程启动而非查询工作。动手前先读 §1.3.1。

**⚠️ 2026-08-19 补充:瓶颈 #2 的"单线程是主因"也已被实测推翻。**
把 parse 并行化后吞吐 **0.707 → 0.665 MiB/s**(略降,不是提升)——
**parse 不是瓶颈**。这是第三条被实测推翻的读码推断(前两条:#4 的线性放大、
§1.4 的 MCP 21 ms)。**教训:这张表的每一行都是读代码推断出来的,
在动手优化前必须先测,否则会把工作量投在错误的地方。**
剩余候选(尚未定论,需 phase trace 才能定):
`commit_source_batches_if_changed` 的单个 SQLite 事务、BLAKE3 capture/verify
的全量读哈希。已派 agent 加 `ASG_SYNC_TRACE` 门控的分阶段计时来定位。
副产物:该轮改动把 registry 从"逐文件构建"改成"构建一次",
**noop sync 963 → 923 ms**(这条只剩 37 ms 余量,值得单独保留)。

**已完成的优化(别重复做)**:`fts_rowid` 边车让 FTS 删除从全表扫描降到 O(1)
(32 MB 文件首扫 16.5s → 2.79s,非空库重扫 25.3s → 2.87s);unchanged re-sync
的 fingerprint skip 实测 0.089s;`index rebuild` 8990 entities 实测 0.71s;
批量写入复用 prepared statement;IN 查询按 500 分块规避
`SQLITE_MAX_VARIABLE_NUMBER`;batch-scoped 提交消除 N+1。

**结论:不需要换存储架构。** spike 实证 2 万文档下 FTS5 与 Tantivy recall
打平(均 1.000),Tantivy 仅在体积和延迟占优但 FTS5 已是毫秒级;双存储会引入
跨存储一致性问题,10 万条量级下成本大于收益。要动的是并行化、候选集召回、
存储布局。

### 1.8 Provider 现状

`ProviderCapabilityMatrix::current()`(`ports/src/capability.rs`)是唯一真源,
16 行 = 14 已实现(全部 `Experimental`)+ 2 deferred(`deepseek-harness`、
`zcode`,`Unsupported`,无 transcript 证据)。

**0 个 Beta。** `discover` 能力只有 `claude-code` 和 `codex` 声明 `Native`,
但 `main.rs:2972-2985` 的 `provider_data_root` 实际返回 6 个 provider 的根
(claude-code、codex、openclaw、tencent-codebuddy、antigravity、opencode)——
这是一处**声明与实现不一致**,M4 要修。

真实样本验证过的只有 Claude Code 和 Codex(owner 本机语料)。其余 12 个
provider 只有合成 fixture,格式来自同类项目的格式证据,**未经真实数据验证**。

#### 1.8.1 真实样本获取现状(已实测,推翻两个 deferred 前提)

**头号发现:两个"拿不到样本"的 provider 本机就有真实 transcript。**
`deferred-deepseek-zcode.md` 的判定建立在 "`~/.deepseek` 不存在 /
`~/.zcode` 不存在" 上,两个前提都已过时:

- **`~/.zcode/cli/db/db.sqlite` 存在**(ZCode 3.7.7 装在 `C:\apps\ZCode`,
  目录建于 2026-08-16)。1 个真实 session / 7 消息 / 22 parts,
  另有 `~/.zcode/cli/rollout/model-io-sess_<id>.jsonl`。
- **DeepSeek Harness 已装**,包名 `@deepseek-ai/dsh` 0.1.0-rc.6(全局 npm,
  `dsh` 在 PATH 上)。它的 home 是 **`~/.dsh` 而不是 `~/.deepseek`** ——
  这就是之前扫描漏掉的原因。

**⚠️ 已确认的真实 bug:zcode 的 CLI DB 是 opencode schema 的 fork。**
拿 opencode adapter 的三条 SQL(`provider-opencode/src/lib.rs:133,151,171`)
对 zcode 的 `db.sqlite` 临时副本执行,**三条全部成功**(1 session /
6 text parts / 7 messages,角色正确)。列差异:zcode 的 session 多出
`task_type`/`title_source`/`trace_id`/`title_message_id`/`time_title_updated`,
少了 opencode 的 token/cost 列;message/part 只多一个 `sequence`。
→ **`opencode/sqlite-v1` 的 probe 会自信地把 zcode DB 认成 opencode**
(真实 `AmbiguousVariant` 缺陷),同时这也是最便宜的 zcode adapter 起点。

**Tier A — 已在磁盘上,0 分钟(实测计数)**

| Provider | 真实产物 | 路径 |
|---|---|---|
| claude-code | **1,856** 个 `.jsonl`,24 个项目目录 | `~/.claude/projects/<enc>/<sessionId>.jsonl` |
| codex | **148** 个 rollout | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| antigravity | **40** 个 transcript(53 brain 目录) | `~/.gemini/antigravity-cli/brain/<uuid>/.system_generated/logs/transcript.jsonl` |
| pi | **7** 个 `.jsonl` | `~/.pi/agent/sessions/--C--Users-<user>--/<ts>_<uuid>.jsonl` |
| opencode | `opencode.db` 1.7 MB:10 session / 261 msg / 886 part | `~/.local/share/opencode/` |
| **zcode** | `db.sqlite` + `model-io-*.jsonl` | `~/.zcode/cli/db/`、`~/.zcode/cli/rollout/` |

**Tier B — 本周可自建**

| Provider | 工时 | 关键事实 |
|---|---|---|
| **deepseek-harness** | **5 分钟,零成本,不需要有效 key** | 默认产物是 zstd 压缩的 `session.jsonl.zstd`(不可按行读),用 patch overlay 强制 raw JSONL:`- id: session-persistence-jsonl / config: {root, compression: none, packChunks: false}`,然后 `$env:DSH_HOME=...; dsh --profile headless --patch raw.yml "reply ok"`。**认证失败(exit 1)但日志仍完整写出** —— 实测拿到 19 行。权威类型联合已在本机:`dsh-session/lib/types/types.d.ts:223-452`(`SessionEventMap`,每事件带 `seq`/`time`/`data`),header 形状 `dsh-session-persistence-jsonl/lib/types/format.d.ts:24-35`,root 解析 `dsh-home-paths/lib/index.js:73`(configured > `$DSH_HOME` > `~/.dsh`)。**不需要猜格式。** |
| qoder | 15 分钟 | `npm i -g @qoder-ai/qodercli`(1.1.25,2026-08-18),Node ≥20 ✓。落 `~/.qoder/projects/<project>/*.jsonl` + `state.json`;`QODER_CONFIG_DIR` 可覆盖 |
| tencent-codebuddy | 15 分钟 | `npm i -g @tencent-ai/codebuddy-code`(2.137.1,bin `codebuddy`/`cbc`)。落 `~/.codebuddy/projects/**/*.jsonl`。本机 `~/.codebuddy` 只有 skills/plugins/rules,CLI 从未跑过 |
| kimi-code | 15 分钟 | `winget install MoonshotAI.KimiCodeCLI`(0.36.1)或 `npm i -g @moonshot-ai/kimi-code`(0.37.0,MIT),Node ≥22.19 ✓。落 `~/.kimi-code/sessions/wd_<hash>/<id>/agents/main/wire.jsonl` + `session_index.jsonl` + `state.json` |
| aider | 15 分钟 | `uv tool install aider-chat`(0.86.2,需 Python <3.13 ≥3.10,本机 3.10.11 ✓)。**不要用 pip** —— 本机 pip 25.1.1 的 `--dry-run` 会 JSON 解码崩溃。**`.aider.chat.history.md` 写在项目 cwd 而不是 home** —— 这就是之前只扫 home 找不到的原因。用 `--model deepseek/deepseek-chat` 只花几分钱 |
| cline | 30 分钟 | VS Code 1.131.0 已装且 `code` 在 PATH,`saoudrizwan.gallery.vsassets.io` 直连 200 → `code --install-extension saoudrizwan.claude-dev` 可用。落 `%APPDATA%\Code\User\globalStorage\saoudrizwan.claude-dev\tasks\<taskId>\api_conversation_history.json`(+ `ui_messages.json`/`task_metadata.json`/`context_history.json`) |
| openclaw | 30-45 分钟 | `openclaw` 2026.7.1-2 MIT,但 `engines.node` 要 ≥24.15.0,**本机 24.14.1 差一个 patch**。`nvm` 已装但 `nvm list available` 直连 nodejs.org 失败;`cdn.npmmirror.com/binaries/node/index.json` 是 200 → 设 `NVM_NODEJS_ORG_MIRROR` 后装 24.15.x。cc-switch 里已有两个 openclaw provider profile(DeepSeek Chat、MiniMax2.7),BYO key 已解决。落 `~/.openclaw/agents/<agent>/sessions/*.jsonl` |

**Tier C — 现实拿不到(2 个)**

- **grok-build**:`@xai-official/grok` 1.0.5 Apache-2.0 能装,但网络上
  `x.ai` 直连 403、`api.x.ai` 直连不可达 / 经代理 401(说明活着)。
  需要 xAI 账号 + 额度,这是国内的硬门槛。
- **cursor**:下载 CDN 直连和经代理都 403,需付费订阅。**而且声明的
  `cursor/vscdb-chat-v1` variant 针对的是旧 VS Code KV 形状**
  (`workbench.panel.aichat.view.aichat.chatdata`、`aiService.prompts`),
  当前 Cursor CLI 写的是
  `~/.cursor/projects/{workspaceSlug}/agent-transcripts/{sessionId}/{sessionId}.jsonl`。
  建议诚实标注"declared but unverified" + 注明该 variant 是**历史格式** ——
  这是站得住脚且真实的。
  ⚠️ `npm i cursor-agent` **不是 Cursor**,是 `zalab-inc/cursor_agent`
  一个无关的任务序列工具,别装错。

### 1.9 外部阻塞项

| 项 | 状态 | 影响 |
|---|---|---|
| GitHub Actions | **全线阻塞**:所有 job 5 秒内 0 步失败,账户级 "recent account payments have failed **or your spending limit needs to be increased**" | 无法自动防性能回退、无法跨平台测试、release 流水线无成功 named run |
| macOS 干净环境演练 | 未跑(owner 在 Windows 11) | 三平台演练不完整 |
| 代码签名/公证/cosign | 无凭据 | 无签名二进制 |
| 12 个 provider 真实样本 | 未收集 | provider 无法诚实升 Beta |

#### 1.9.1 CI 阻塞的真实根因(已查实,与三个假设都不同)

**不是封号、不是中国网络、不是账号问题 —— 是两个仓库都是 private,
GitHub Free 私有仓库的 2,000 min/月 配额被烧穿了。**

按官方倍率(Linux 1x / Windows 2x / macOS **10x**)加总两个仓库所有 run:

| 仓库 | Linux | Windows(×2) | macOS(×10) | 计费等效 |
|---|---:|---:|---:|---:|
| `agent-session-grep` | 134.6 | 238.9 | **3,134** | 3,508 min |
| `AgentSessions` | 258.9 | 628.4 | **3,001** | 3,889 min |
| 合计 | | | | **≈7,397 min** |

配额 2,000 min/月,**超了 3.7 倍。macOS 占 83%**(10 倍倍率)。
时间线吻合:最后一次成功 run `2026-08-18T06:47:41Z`(core-beta-evidence
四平台全绿),第一次 0-step 失败 `2026-08-18T12:20:19Z`。

**解法:public 仓库的标准 runner 免费且无限。** 官方文档原文:
"Use of the standard GitHub-hosted runners is **free and unlimited on
public repositories**"。2025-12 定价变更公告重申
"Runner usage in public repositories will remain free"。
额外红利:public 仓库拿到 **4-core/16GB** 机器(private 只有 2-core/8GB),
CI 更快。

**即:owner 本来的发布计划本身就解锁了 CI。** 这条与 D1 完全同向。

**并发墙仍在**(Free plan):20 并发 job、**5 并发 macOS**、
256 matrix job per run。当前 `ci.yml` 与 `core-beta-evidence.yml`
各自都开三平台/四目标 matrix,叠加会撞 macOS 上限 —— 见 M2C-3。

## 2. 已锁定决策(owner 在 2026-08-18 拷问中逐条确认 —— 不要重开)

| # | 决策 | owner 原话/选项 | 理由 |
|---|---|---|---|
| D1 | 发布门槛 = 性能 + 功能 + 文档**全部拉满** | "性能文档功能都要拉到最满" | — |
| D2 | 性能按 **10 万条消息 / 5 千 session** 定档并对外公开数字 | "10 万条消息" | 单人多年 + 多 provider 的真实上限,是现实用量的约 10 倍余量 |
| D3 | 性能硬指标覆盖**四个动作全绑 CI**:首次全量索引、增量同步、搜索、MCP 单次调用 | "四个动作全绑" | 首次索引跑十分钟没反馈一样会跑人,不能只管搜索延迟 |
| D4 | **首次索引 5 分钟硬阀**(1 GB 语料),即吞吐从 0.566 → ~3.5 MB/s(6 倍) | "5 分钟硬阀" | 新用户愿意等的上限 |
| D5 | 测试语料**双轨**:合成生成器扩到 10 万条进 CI 作硬门;真实数据一次性验证,**只提交聚合数字** | "双轨" | 合成保证确定性可复现,真实保证不自欺 |
| D6 | 真实失败样本走**隔离 gitignored 目录**;修复必须用**新写的合成 fixture** 复现;只有合成 fixture 进仓库 | "隔离目录" | 真实会话数据永不进 git,也不上传 |
| D7 | 真实语料**冻结一份固定快照**(约 1.2 GB 本地空间) | "冻结快照" | 语料会变,不冻结则性能数字无法纵向对比 |
| D8 | `--db` 改为**默认平台数据目录**,`--db` 降为覆盖项;严格对齐 ripgrep/gh/atuin 的零配置习惯 | "默认路径" + "尽量和 ripgrep/gh/atuin 的习惯保持一致!!!" | 每条命令手敲路径是新用户第一道摩擦 |
| D9 | provider "真实样本验证过"的定义 = **真实 transcript 解析无损**(session/message/父子链/时间戳/角色全对)+ 缺口写进 `known_limitations` + **反推一份合成 golden fixture 进仓库防回退** | "解析无损+合成 golden" | 字节级往返达不成(provider 写入非确定性);"能跑就算"门槛太低 |
| D10 | 平台范围:**Windows + Linux 完整验证**;macOS 保持代码支持但诚实标注"未在干净环境验证" | "Win+Linux 完整" | owner 无 Mac;CI 恢复后用 runner 补 |
| D11 | 解析容错:**宽容 + 计数上报**,跳过损坏行并在结束时报告"跳过 N 行,详情见 --verbose",默认不中断 | "宽容+计数上报" | 真实会话文件常有写到一半的尾行(agent 正在跑或被杀),中断会让工具在真实环境不可用 |
| D12 | 数据生命周期**本轮要做**,支持三个删除维度:**按时间**、**按项目**、**按 provider** | "要数据生命周期" + 三选 | local-first 工具存了全部历史,不能删是隐私真缺口 |
| D13 | dev-dependency 允许加 **insta 快照测试**(正式依赖不动) | "允许 insta 快照" | 把"解析无损"变成可评审的快照文件 |
| D14 | semantic **保持 opt-in**,但**修全表扇描** | "保持 opt-in" + "修" | recall 只 +2% 不值得默认开;但全表扫描是真缺陷 |
| D15 | 功能范围**全部补齐**(审计列出的缺口一律补,不按"十分钟标准"截断) | "全部补齐" | — |
| D16 | 里程碑顺序:**正确性 → 性能 → 功能 → 文档/UX** | "正确性优先" | 解析错的话性能数字没意义;真实样本可能推翻 schema 假设 |
| D17 | 公开仓库 git 历史:**从今往后真实推送**(不洗旧历史,也不再压成孤儿 commit) | "从今往后真实推送" | 洗历史风险大;单 commit 让人怀疑真实性 |
| D18 | 隐私扫描器与公开树导出器**从公开树排除** | "排除发布工具" | 它们的规则字面写着私有仓库名/内部 tracker 名/借鉴目录名,import 一下就能还原 |
| D19 | 下一个公开版本号 **0.2.0** | "0.2.0" | 0.x 允许 breaking change,留出改 Robot/MCP 契约的空间 |
| D20 | 架构改动授权:"我都允许,但是一定要合理" —— 不换存储、不加正式并行依赖(用 `std::thread` + channel) | owner 原话 | 与 §1.7 结论一致 |
| D21 | 真实样本**必须真实**:"必须要有真实样本必须真实案例测试,不能凭空想象!不能凭空捏造!要有理有据!" | owner 原话 | 这是 M1 存在的理由 |

### 2.1 待 owner 决定(不要自己拍)

*(当前为空 —— 原有的两项已按 D27 / D28 自主决策落定,理由见决策表。
若后续出现真正只有 owner 能定的事项,记在这里。)*

### 2.3 搜索作用域决策(D26,调研已闭环,owner 的反问是对的)

owner 的质疑:"如果默认只在本项目搜索,那我有上下文我直接问 AI
我还用得着通过 asg 搜索吗?" —— **这个质疑完全成立,而且被 14 个同类工具
的实际做法证实。**

**14 个工具全部默认全库搜索。零个默认当前项目。**
(atuin、mcfly、zoxide、fzf Ctrl-R、ctx、sessiongrep、fast-resume、
agentsview、agent-sessions、agf、Recall、claude-historian-mcp、memex、
cc-switch、claude-code-history-viewer —— 已逐个读源码/配置默认值核实)

唯一会收窄默认的三处都不是"按项目":(a) sessiongrep 是**排序**加权不是过滤;
(b) ctx 排除的是**当前活跃会话**不是项目;(c) zoxide 排除**你已经在的那个目录**。

**四条最强证据 —— 有人试过项目默认,然后撤回了:**

1. **agf 撤回了两次。** v0.11.0 移除了同项目排序加权,理由(CHANGELOG:123)
   是关键:"那个把 `project_path == $PWD` 的会话推到最上面的二次排序
   **让按时间排序的列表看起来是坏的**……这个加权是隐式的(**没有屏幕上的
   指示器**),所以用户读成'时间排序错了'"。
   v0.11.2 又**回滚了一个已合并的 PR**(那个 PR 把搜索框预填成 `$PWD`):
   "它改变了**每一个** agent 的无参数行为(**在非项目目录里得到空列表**),
   并重新引入了 v0.11.0 刻意移除的 cwd 特殊处理"。
   两次独立撤回,一次针对排序一次针对过滤,**声明的失败原因都是"不可见"
   而不是加权本身**。struct 上的 `pub cwd` 还留着标了
   `#[allow(dead_code)]`,注释说留给"未来 settings-gated 的重新引入"。
2. **claude-historian 从有上限的按项目扫描改成了无上限的跨项目扫描。**
   commit `7a21053e` 删掉了 `limit * 8` 收集、`perProject` 上限、分批
   和 per-file `break`,留下的注释是
   `// Search all projects — no artificial scope limits`。
   同一个 commit **保留并扩展了项目名加权** —— 方向是:召回放宽,由排序收窄。
3. **Recall 把"当前 repo 默认"做出来了然后默认关掉。**
   `default_current_repo_scope: bool` 的 `Default` 推导为 `false`。
   他们自己的 skill 文档解释了为什么路径推导的作用域不可靠:
   "`recall --project` 只按精确会话目录加子路径过滤;它不理解 repo 名、
   remote、符号链接、或 worktree"。
4. **claude-code-history-viewer 事后才给全库结果补上来源标注**
   (issue #420:"全局搜索结果现在显示每条命中属于哪个会话")——
   证明全库默认能活下来的前提是**每条命中都说清来自哪里**。

**反向信号同样重要:atuin 四年没改过默认。** `filter_mode` 从 v12.0.0
到现在都是 `global`。变的全是**增量**:filter 顺序可配了(默认仍以 global 开头)、
`workspaces` 存在但 `set_default("workspaces", false)`、
2025-09 加的 `session-preload` 是"当前会话**加上会话开始之前的全部全局历史**"——
**连它最窄的可用模式都拒绝放弃全局语料**。
而且 atuin 的循环 UI 配着一个**常驻屏幕指示器**(把过滤模式首字母渲染进
提示符:`G>`、`D>`、`W>`)—— 这正是 agf 说自己缺的那个东西。

**对本项目最要命的一条(架构论证):**
你现在唯一的项目信号是 resume claim 上的 `original_working_directory`,
它被 `pair_observed && original_working_directory_state == "resolved"` 门控,
多源冲突时**整个丢弃**(`adapters-sqlite/src/lib.rs:4384-4441`)。
而 `SearchFilters`(`ports/src/lib.rs:246-250`)只有
`providers`/`since`/`until`,**根本没有项目维度**。
让**默认行为**依赖一个对很多会话合法为 null 的字段,意味着默认会
**静默隐藏可召回的历史**;而对同一字段做**加权**则优雅降级 ——
null 只是没有加分。**这是决定性的架构理由。**

还有一条具体的:本仓库现在就跑在一个与主 checkout
**不共享任何路径前缀**的 git worktree 里。
Recall 的 skill 文档正好记录了路径前缀作用域在这类情况下的失败。

**量级参考(sessiongrep,本项目最接近的结构同类:Rust + SQLite/FTS5 +
CLI/TUI/MCP)**:`prefer_current_repo = true` 默认开,同 repo 命中 **+200**,
对比标题精确匹配 **600**、recency **≤180**(`src/db.rs:318-374`)。
claude-historian 叠了同样的思路(cwd 匹配加性 +5,查询词命中项目名时
乘性 ×3)—— **拿不准就用加性**,乘性项目加权会压过文本相关性。

### 2.2 CI 决策(已查实,D22-D25)

| # | 决策 | 依据 |
|---|---|---|
| D22 | **不注册第二个 GitHub 账号。** ToS B.3 明文 "you may not have more than one free Account";Additional Product Terms → Actions 写明滥用可致 "suspension or termination of your GitHub account"。**而且根本解决不了问题** —— 官方:"Minutes usage is charged to **the repository owner**",新账号只有整体搬仓库才有用,那等于放弃已准备公开的身份。**高风险 + 零收益。** | owner 问过,答案是不要 |
| D23 | **不用 linux.do 或任何第三方的 CI 账号/runner。** runner registration token 等价于代码执行权 + 仓库写权限:对方能拿到 `GITHUB_TOKEN`、所有 secrets、以及构建产物的完整控制权 —— 对一个别人会 `cargo install` 的项目是教科书级供应链投毒入口。linux.do 社区规则第一条自己就写"勿外借、买卖账号"。同样零收益。 | owner 问过,答案是强烈反对 |
| D24 | **public 仓库上绝不挂 self-hosted runner。** GitHub secure-use 官方原文:"self-hosted runners should **almost never** be used for public repositories, because any user can open pull requests against the repository and compromise the environment"。Legit Security 扫到 43,803 个公开仓库这么干,Sysdig 记录了 Shai-Hulud 用这条路径植入后门的真实案例。owner 的 Windows 11 主机上有真实 transcript 与 `.claude` 配置,不能暴露。 | 安全红线 |
| D25 | **CI 解法 = 转 public(免费无限)+ 砍 PR 上的 macOS job + Actions budget 设 $5 兜底。** 预期成本 $0,耗时 1 小时内,且与 D1 完全同向。 | 见 §1.9.1 |
| D26 | **搜索默认全库 + 同项目加性加权 + 每条结果显示来源项目。不做 `--all` 逃生舱。** | 14/14 同类工具默认全库;agf 试过项目默认并撤回两次;本项目的项目信号字段对很多会话合法为 null,做默认会静默隐藏历史,做加权则优雅降级。详见 §2.3 |
| D27 | **文档语言方针:新增的用户可见字符串一律英文;不做存量中文的批量翻译。** 存量中文文本原地保留,`docs/zh/` 留给中文治理文档。 | `--help` 91 行里 69 行含中文而 README/CHANGELOG/SECURITY 全英文,非中文读者用不了主帮助面;但一次性全量翻译会与所有并行改动冲突。按"新写英文、旧文本随手改到的顺带改"渐进收敛 |
| D28 | **先把公开树清干净并推送,再转 PUBLIC;转 public 不等待全部 91 个任务完成。** 转 public 立刻解锁免费 CI(D25),而 CI 是 M2 性能阀值防回退的前提 —— 等全做完再转会让整个 M2 期间都没有回退保护。未完成状态由 README 顶部的诚实标注承担。 | 转 public 不可逆,所以顺序是「清干净 → 推送 → 转」;但把它排在 M0 之后而非 M5 之后,是因为 CI 是后续所有性能工作的基础设施 |

**替代平台已全部评估过,没有一个能提供 "免费 + 三平台 + 含 macOS" 的组合**:
Cirrus CI **已停服**(2026-06-01,Cirrus Labs 加入 OpenAI);Travis OSS 免费层
2020 年就没了;GitLab CI Free 只有 400 min/月(macOS 6-12x 倍率,等于没有);
CircleCI 的 OSS 额度**只覆盖 Linux/Arm/Docker,不含 macOS**;
Codeberg CI 官方明确"不跑专有操作系统,不会支持 Windows";
自建 Forgejo/Woodpecker 换来运维成本且 macOS 仍无解。

**macOS 无 Mac 的选项**(万一将来脱离 GitHub):public 仓库 GitHub-hosted
macOS **$0**(`macos-15-intel` + `macos-15` M1 已在 workflow 里配好且历史跑绿过);
Scaleway Mac mini M1 €0.11/hr 但**最低租 24h**(Apple 授权限制)→ 单次最少
≈€2.6,整月 ≈€79;MacStadium $109/月起。偶发使用的经济学很清楚。

⚠️ **自建 runner 免费是"被推迟"不是"被取消"** —— GitHub changelog 明说
control plane 免费 "not sustainable long term",别把长期架构押在上面。

## 3. 发布门(全部为真才把仓库转 PUBLIC 并发 v0.2.0)

⚠️ **v0.1.0 是在自己的发布门没关闭的情况下发出去的。**
功能缺口审计逐条核了 `OPEN-SOURCE-ROADMAP.md:92-97` 的 12 条原始条件:
**3 条满足、7 条部分满足、2 条未满足**。而且其中 3 条(Claude/Codex certified、
≥5 Beta、三平台演练)**在本地根本不可能满足** —— 这就是
`OPEN-SOURCE-ROADMAP.md:28` 记着 `NOT_READY_EXTERNAL_BLOCKERS`
而 `CHANGELOG.md:7` / `README.md:11` 同时宣布"首次公开发布"的原因。
**下面这套门是重写过的:每条都本地可验证,不含无法关闭的条件。**

逐条可验证,不留主观判断:

| 门 | 判据 | 验证命令/证据 |
|---|---|---|
| G1 | Claude Code + Codex 用真实语料按 D9 标准验证通过,并各有一份合成 golden fixture 在仓库里 | `cargo test --workspace` 含 golden;`M1` 的 owner 授权真实运行报告(聚合) |
| G2 | 至少 5 个 provider 达到 D9 标准并升为 Beta;其余诚实标注 | `capability.rs` 矩阵 + 漂移测试;`PROVIDER-MATURITY-MATRIX.md` |
| G3 | 四个动作在 10 万条合成语料上全部达阀,且阀值绑进自动化 | `open_source_gate_benchmark.py` 报告 `all thresholds pass` |
| G4 | 首次全量索引 1 GB ≤ 5 分钟 | 同上,`initial_index_throughput_mib_s ≥ 3.5`。**⚠️ 判定必须说明用的是哪种密度的语料**:真实密度 1 GiB 实测 2.1–2.8 min(达标),当前受门合成语料 12.5–17.2 min(不达标),同一二进制 —— 差异全部来自 509 vs 7,174 字节/消息(§1.3.6)。**G4 问的是真实语料,所以放行依据必须是真实密度的测量,不能拿合成语料的 FAIL 当阻塞、也不能拿它的 PASS 当通过。** |
| G5 | 七条 Gate D 不变量在真实冻结快照上全绿 | `real_data_regression.py` 退出 0 |
| G6 | 五入口一致性 harness `overall_verdict: consistent` | `compare_entrypoints.py` |
| G7 | 零出网静态审计通过;`--offline` fail-closed | `cargo test -p agent-session-grep-cli --test network_egress` |
| G8 | 跨边界脱敏覆盖;合成 secret fixture 不外泄 | 现有脱敏 e2e |
| G9 | 数据生命周期三维度可用且有测试 | `M3` 交付 |
| G10 | 公开树 public-profile 隐私扫描 0 findings | `privacy_scan.py --profile public`(经导出器) |
| G11 | 文档:quickstart 可照抄执行、每 provider 有"记录在哪/解析不了什么"表、故障排查手册、MCP 客户端接入实例、CLI reference 完整 | `M4` 交付 + 人工走一遍 |
| G12 | 产品内 UX:零配置可用、每条错误给下一步命令、首次运行向导、shell 补全 | `M4` 交付 |
| G13 | 无过时/自相矛盾声明(含"六条 vs 七条"、Gate D pending、discover 声明与实现不一致、`aider: tool_activity=Partial` 假声明、MCP 注释说"八个工具"实为九个、ADR-0009 撞号、roadmap 的 `NOT_READY` 判定 vs README 的"已发布") | `M4` 交付 |
| G14 | Windows + Linux 完整验证;macOS 诚实标注 | 本地 + WSL2 执行记录 |
| G15 | 全部质量门绿 | §0 第 5 条的七条命令 |
| G16 | **每个声明的能力都有一条行为断言**(不再让矩阵自我认证) | `M1-13` 交付 |
| G17 | **五个入口都能答"我最近干了什么"**(recency 浏览) | `M3-4` 交付 |
| G18 | **时间过滤要么覆盖全 provider,要么明确告知排除数量** | `M2P-12` 交付 |

**明确不阻塞公开**(诚实标注 + 建 issue 跟踪即可):macOS 干净环境演练、
代码签名/公证/cosign、12 个 provider 全部升 Beta、分支保护、
ADR/REUSE 正式签字、SBOM 认证、session diff、丰富查询语法
(ADR-0003 刻意撤掉的,不推翻)、`ses_v2_` 身份迁移(除了 M3-16 那条
Windows 大小写缺陷)、snapshot API、`recovery_time_ms` 测量。

### 3.1 v0.1.0 发布门核验结果(存档,说明为何要重写门)

| # | 原条件 | 判定 | 关键证据 |
|---|---|---|---|
| 1 | 16 provider 证据齐 | 部分 | 16 行存在、14 有 golden,但字段级为假:`context` 与 `handoff` 对全部 14 个是 `Unsupported`;`AdapterManifest.last_certified_targets` 对每个 provider 硬编码为空(`manifest.rs:117`) |
| 2 | Claude/Codex certified | **未满足** | 两者都是 `Experimental`,且有测试**强制**没有任何行高于 Experimental(`capability.rs:400-414`) |
| 3 | ≥5 主流 beta/GA | **未满足** | 0 Beta |
| 4 | semantic+benchmark | 部分 | 真实 E5 已验证,但阈值未冻结;`verify-release.py:192` 仍断言 `model_id == "bigram-hash-v1"` 是**期望的发布行为** |
| 5 | handoff-pack/v1 | **满足** | schema + 字节确定性 e2e 齐 |
| 6 | Web UI parity | 部分,**比报告承认的更差** | Web 侧**完全没有 facet 控件**(无 sidechain / tool_kind / tool_name),而 CLI、MCP、TUI 都有;无 list/browse、无 get-message;且下拉框 16 选项里 13 个返回 HTTP 400 |
| 7 | 零遥测可验证 | 满足(静态) | 依赖禁令 + socket 断言;干净环境抓包从未跑过(已诚实记录) |
| 8 | 跨边界脱敏 | 代码满足,治理未签 | ADR-0009 状态是 `Proposed` 而非 `Accepted` |
| 9 | Hook 默认关闭 | **满足** | `enabled: false` + verify-release 断言 |
| 10 | 三平台安装 | 部分 | 脚本与 3-OS CI job 存在,但 `ci_configured_only` 无具名成功 run |
| 11 | benchmark/文档/对比表一致 | 部分 | Gate D 复核 pending;而且门自己的 `discovery_coverage ≥ 0.95` **只种了 claude+codex 的 fixture** —— 它测的是 14 个里的 2 个,**永远不会因另外 12 个而失败** |
| 12 | 三平台演练通过 | **未满足** | macOS 未跑 |

## 4. 里程碑与任务

顺序按 D16:**M0 → M1 → M2P → M2C → M2 → M3 → M4 → M5**。
M0 可与 M1 并行(纯清理);M2P 是"首次运行正确性",属正确性范畴,
可与 M1 并行,**必须早于 M2**(优化一个新用户跑不通的流程没有意义)。
M2C 是 CI 解锁,**M2-2 的阀值要绑 CI 就得先有 CI**;但 M2C-2 依赖 M5-5
转 public,而转 public 又要求发布门全绿 —— 所以实际执行顺序是:
先做 M2C-1(budget 兜底)与 M2C-3(砍 macOS matrix),
M2C-2/4/5 在 M5-5 转 public 之后立刻补上。
若 M2C-1 也走不通(加不了支付方式),M2 的阀值先在本地跑并记录为
`locally_verified`,转 public 后再升 `ci_verified`。
每个任务独立成 worktree + 分支,完成后合回 `main`(私有仓库)。

---

### M0 — 收口当前遗留(小而急,先清掉)

- [x] **M0-1 从公开树排除发布工具链(D18)**
  `scripts/release/export_public_tree.py` 的 `EXCLUDED_PREFIXES` 增加
  `scripts/evidence/privacy_scan.py`、`scripts/evidence/test_privacy_scan.py`、
  `scripts/release/export_public_tree.py`、`scripts/release/test_export_public_tree.py`。
  注意:排除机制目前是**前缀**匹配,需要支持精确文件排除。
  同时修 `export_public_tree.py:124` 的自豁免 bug:它 `continue` 跳过了
  `MANIFEST_NAME`,导致它唯一写的那个文件是唯一不被扫的文件 ——
  改为扫描 manifest,或干脆不发布 manifest。
  **验收**:导出后这四个文件不在公开树;public-profile 扫描 0 findings。

- [x] **M0-2 `PUBLIC-TREE-MANIFEST.json` 不进公开树**
  它含 `excluded_prefixes`(泄漏内部目录布局)、`source_commit`(指向公开
  读者无法解析的私有 SHA)、482 行 SHA-256(对读者零价值)。
  **验收**:公开树无此文件;导出仍在本地写 manifest 供校验。

- [x] **M0-3 修 SECURITY.md 支持版本表**
  当前写 "0.1.x | 不支持 / Published releases | None",但 `v0.1.0` 已发。
  改为 `0.1.x | Supported`,删掉 "None" 行。
  **验收**:与 CHANGELOG、README 的版本陈述一致。

- [x] **M0-4 `go-no-go.2026-08-16.md` 与 `go-no-go.template.md` 不进公开树**
  它是内部决策草稿,结论是 **No-Go**,还带 7 条 P0 缺陷清单
  (含 "serve token generator not CSPRNG"、"Web/JSON boundary leaked secrets")
  和未签名的签字块。公开读者下载 v0.1.0 会看到维护者自己判定不该发。
  保留 `rehearsal-runbook.md`(是真有用的流程文档)。
  **验收**:公开树无 go-no-go;README/CHANGELOG 不再引用它。

- [x] **M0-5 `PUBLIC-HISTORY-SCRUB.md` 不进公开树**
  公开仓库只有 1 个 commit,历史重写手册在结构上不适用;而它公布了
  "45 个 commit 带路径痕迹、6 个 commit 含 secret 形状"的度量表和
  `git log -S"<username>" --all -p` 这样的搜索配方。
  **验收**:公开树无此文件。

- [x] **M0-6 `R0-ARCHITECTURE-REVIEW.md` 不进公开树**
  标题写"待签署",正文写"Review status: Pending — not approved"、
  "此前 R0 Gate 整体保持未通过",还有 15 个未勾选决策框,其中一条是
  "对先实现后审批作显式 exception" —— 等于公开承认实现跑在架构审批之前。
  内容已被各 ADR 取代。
  **验收**:公开树无此文件;引用它的地方改指 ADR。

- [x] **M0-7 治理文档的 owner 字段统一**
  18 个文件写 `owner: （待指派）`、`approver: 项目最终验收人`、`approved_at: —`
  (全部 ADR 0001-0009、RFC-0001/0002、CLI 契约、THREAT-MODEL、
  FIXTURE-REDACTION-POLICY、REUSE-LICENSE-AUDIT、SLI-AND-BENCHMARK-FORMAT、
  external-readiness-gate、search-backend SPIKE-CARD)。
  ADR-0009-cross-boundary 和 ADR-0010 已填 `QIN`,同目录内不一致更难看。
  统一填 `QIN`。
  **验收**:`docs/` 内无 `（待指派）`。

- [x] **M0-8 `CONTEXT.md` 决策日志段落处理**
  第 174-346 行是内部决策日志,其中 `:199`/`:284-285` 写"仓库保持 PRIVATE"、
  `:176` 写"完成 0.3 后再决定发布"(已发 0.1.0)、`:259` 有编辑事故
  (标题与正文粘连)、`:319` 有未翻译中文混在英文句中、`:243`/`:245` 有中文
  表头在英文文档里。保留 §Language 术语表(第 1-171 行,是 `AGENTS.md:113`
  指向的东西,质量很好),砍掉决策日志或把耐久部分移入 ADR。
  **验收**:术语表完整保留;无 PRIVATE 陈述;`:259`/`:319` 修好。

- [x] **M0-9 `OPEN-SOURCE-ROADMAP.md` 标题与章节修复**
  标题 `开源登顶路线图`(字面意思"登上开源榜首")与项目其余部分的保守克制
  完全相反,改成中性的"开源交付路线图"。章节编号从 `## 0.1` 直接跳到 `## 2`,
  缺 `## 1`,第 39-53 行是孤立散文。两处 "无人做到" 的无限定断言
  (`:59`、`:63`)加上"在 2026-08-14 调研集合内未观察到"的限定。
  `§0.1` 是带 `NOT_READY_EXTERNAL_BLOCKERS` 判定的过时内部快照,删或重写。
  **验收**:章节编号连续;无绝对化竞品断言。

- [x] **M0-10 `INSTALL-AND-UPGRADE.md:145` 修 uninstall 调用**
  写着 `sh scripts/install/uninstall.sh`,但该脚本是
  `#!/usr/bin/env bash` + `set -euo pipefail`,同一文档 `:61-63` 自己警告过
  Debian/Ubuntu 的 `sh` 是 dash 会失败。改成 `bash`。
  **验收**:与 `rehearsal-runbook.md:368` 的写法一致。**这条会真实影响用户。**

- [x] **M0-11 CLI 输出移除内部编号**
  `scripts/verify-release.py:246` 打印 `agent-session-grep release verification (#10)`,
  `scripts/benchmark.py:2` 是 `"""agent-session-grep benchmark harness (#9)."""`。
  `(#9)`/`(#10)` 是内部任务号,出现在用户可见输出里。
  **验收**:输出无内部编号。

- [x] **M0-12 ISSUE_TEMPLATE 版本占位符修正**
  `bug-report.yml:43` 与 `feature-request.yml:35` 的 `placeholder: v0.3.0 or 0.3.x`
  (0.3 是内部里程碑编号,已发布版本是 0.1.0)。且两者把 40 位 commit SHA 设为
  `required: true`,用发布版二进制的报告者填不出来。
  改 placeholder 为 `v0.2.0 or 0.2.x`,commit 改为可选或注明"仅开发构建"。
  **验收**:模板可被真实用户填完。

- [x] **M0-13 spike 卡片断链修复**
  `spikes/search-backend/SPIKE-CARD.md:100-102` 引用了不存在的
  `report.md`、`selection-gate.md`、`fts5/`、`tantivy/` 目录
  (实际是扁平的 `src/fts5.rs`、`src/tantivy_be.rs`)。
  `spikes/sqlite-source-identity/` 缺 `SPIKE-CARD.md`,但
  `spikes/R0-EVIDENCE-SUMMARY.md:93` 声称"四个原本只有 Evidence 的 Spike
  已补 Spike Card"并列了五个。
  **验收**:所有 backtick 路径解析成功。

- [x] **M0-14 数字一致性修正**
  - 七条不变量 vs 八处文档写"六条"(`CONTEXT.md:160` 是术语表定义,最该修)
  - error catalog 实际 14 码,`CONTRACT-cli-robot-mcp-draft.md:49` 写 13 码,
    `OPEN-SOURCE-ROADMAP.md:65` 写 "13+ 码"
  - `core-beta-evidence-matrix.md:40` 写 "12 tests",实际 14 个 `def test_`,
    运行报告 10 个(1 skip)
  - `COMPETITOR-COMPARISON.md:52-53` 的 12/13 算术不闭合
  **验收**:每个数字与代码/schema 一致,或改为引用 schema 不写死数字。

- [x] **M0-15 semantic 证据自相矛盾修正**
  `SEMANTIC-MODEL-BUNDLE.md:140` 写 "semantic recall@k **tracked at/below
  lexical**",而 `go-no-go.2026-08-16.md:137` 写 "**semantic ≥ lexical**"。
  按 §1.5 实测:semantic 在 @5/@10/@20 全部 ≥ lexical(0.720/0.755/0.775 vs
  0.705/0.750/0.750),所以 `SEMANTIC-MODEL-BUNDLE.md` 那句是错的。
  同时:`:124-126` 的加粗 "Real inference HAS been verified in this
  environment" 去掉喊话语气;`:148` 的 "p50 16.6 ms / p95 21.0 ms" 必须就地
  标注 n=10、语料 10 条合成消息,否则是引人误解的 benchmark 数字。
  **验收**:两处方向一致且与报告 JSON 相符;延迟数字带样本量限定。

- [x] **M0-16 `docs/performance-baseline-0.2.md` 重命名与限定**
  文件名和标题写 0.2,但那时还没有 0.2;`:80-81` 的 "16.5s → 2.79s"、
  "25.3s → 2.87s" 是开发机真实语料的加速比,摘要行没写语料。
  **验收**:文件名与版本对应;每个数字带语料说明。

- [x] **M0-17 `spikes/sqlite-source-identity/EVIDENCE.md` 竞品拆解处理**
  `:131-180` 点名 `jhlee0409/claude-code-history-viewer`(MIT, v1.22.0)并
  判定其方案劣于本项目,依据是"临时下载后 grep,下载物已删除"。
  公开点名评判个人项目会招致审视。技术发现(SQLite provider 的行级身份)
  有价值,保留;删掉"与 CCHV 的对照"和"CCHV provider 存储分档"两节,
  或压成一句中性陈述。
  **验收**:无对第三方项目的优劣判定。

- [x] **M0-18 `spikes/R0-EVIDENCE-SUMMARY.md:90-94` 删杀软逸事**
  记录了开发者的杀软反复静默删除 `EXTERNAL-READINESS-GATE.md`、改成全小写
  文件名后才留存。诚实但读起来像环境不稳 + 轻度绕过 AV。
  保留 `SLI-AND-BENCHMARK-FORMAT.md` 里关于为何记录 `antivirus_state` 的
  那一句实质内容。
  **验收**:附录删除。

- [x] **M0-19 机器与私有语料细节脱敏**
  - `core-beta-benchmark-full.json:14-19` 含 CPU 型号串
    `Intel64 Family 6 Model 170 Stepping 4`、22 逻辑核、31.5 GB RAM、
    `Samsung NVMe SSD`、`antivirus_state`。硬件行作为 benchmark 出处可保留
    (`SLI-AND-BENCHMARK-FORMAT.md:50` 明确要求记录 `antivirus_state`),
    但 CPU 串考虑泛化。
  - `real-data-regression.md` 量化了私有语料:1,340 源 / 1,267,099,050 字节 /
    182,886 消息 / 242 session,且所有被引报告都在 gitignored `evidence-output/`
    下,读者无法核验 → 要么删掉计数,要么加一句说明语料私有不可核验。
  - 把"本机"统一改成"维护者的开发机",避免读成对读者机器的断言
    (`PROVIDER-MATURITY-MATRIX.md:31,37,42,43`、
    `spikes/sqlite-source-identity/EVIDENCE.md:109-112` 还列了开发者装了哪些
    agent 工具)。
  **验收**:无对读者环境的误导;私有语料计数有出处说明或删除。

- [x] **M0-20 CI 证据的不可达引用处理**
  `core-beta-evidence-matrix.md:21` 写 run `30165919066` / PR #1,实测:
  该 run 在公开仓库 404,只在私有仓库存在(分支
  `chore/batches-1-3-governance-and-evidence`);公开仓库 PR #1 是
  dependabot 的 checkout 升级。四行 `ci_verified` 挂在这个不可达 run 上,
  按该文档 `:70` 自己的规则(必须**具名**一次成功 run)在公开语境下不可核验。
  同理 `docs/evidence/core-beta/88d86f4/` 的目录名 pin 在私有 SHA。
  处理:四行降级为 `ci_configured_only`,或加一句"该 run 早于公开仓库,
  不可公开链接";证据目录加一句说明 `88d86f4` 指发布前开发历史。
  **验收**:公开读者不会看到指向自己无法访问之处的"已验证"声明。

---

### M1 — 正确性:真实样本验证(D16 第一优先,D21 是硬要求)

**为什么先做这个**:解析错了的话性能数字没意义 —— 优化一个会丢消息的索引器
是白干。而真实样本可能推翻 schema 假设,越早知道越好。

- [ ] **M1-1 建立隔离样本工作区(D6)**
  在仓库外或 gitignored 路径下建 `evidence-input/real-samples/<provider>/`,
  写一份 `README`(**不进仓库**)说明:样本永不 `git add`、永不上传、
  只用于本地定位。`.gitignore` 加规则确保误 add 会被拦。
  ⚠️ **第三方公开仓库里的真实 transcript 也放这里**,不进仓库:
  它们含提交者的 prompt、文件路径、用户名,偶尔有近似凭据的输出。
  按只读第三方 PII 处理 —— 仅用于验证 probe/parse 行为,
  **永不提交、永不再分发、永不在文档或 commit message 里引用内容**。
  无 license 的仓库不授予任何再分发权;AGPL-3.0 的更会让派生提交物
  变成许可问题。这与现有 `docs/security/FIXTURE-REDACTION-POLICY.md`
  的"合成优先、禁止真实 transcript"一致。
  已知可用的公开真实样本(仅供本地核验):
  `thetom42/stock-portfolio`(~103 个 Cline task,**无 license**)、
  `TimeWarpEngineering/timewarp-simple-icons`(CC0)、
  `unmodeled-tyler/aider-ollama`(MIT)、`MarioPadilla/claude-vault`(无 license)。
  注:`rollout-*.jsonl` 按文件名搜索**零命中**,
  `parentUuid isSidechain sessionId` 也**零命中** ——
  真实 claude/codex transcript 基本没人公开提交。
  **验收**:`git status` 在放入样本后仍干净;`git check-ignore` 确认命中。

- [ ] **M1-2 冻结真实语料快照(D7)**
  把当前 provider 数据根整体拷到 `evidence-input/frozen-corpus-2026-08-<dd>/`,
  记录清单(文件数、总字节、每文件 SHA-256)到**同目录**的清单文件
  (不进仓库)。后续所有真实回归跑这一份。
  **验收**:清单可复算;`real_data_regression.py` 能指向该快照运行。

- [ ] **M1-3 样本采集执行(D21)—— 已有实测路径,见 §1.8.1**
  按 §1.8.1 的 Tier 分级逐个执行。**建议顺序(每步都已实测可行)**:
  1. **今天 15 分钟**:用有效 DeepSeek key 重跑 dsh raw-JSONL 配方,
     拿到含 `assistant/message` / `tool/call` / `tool/result` 的完整一轮 →
     直接写 `deepseek-harness/session-jsonl-v0` adapter
     (类型联合已在本机 `types.d.ts`,**不需要猜**)
  2. **今天 30 分钟**:promote zcode。从 opencode adapter 起步,
     按 zcode 独有列(`session.task_type`/`trace_id`、`message.sequence`)
     判别,**同时收紧 opencode 的 probe 让它拒绝 zcode**(见 M1-9)。
     root:`~/.zcode/cli/db/db.sqlite`
  3. **第 2 天 45 分钟**:kimi-code + qoder + codebuddy 连着做
     (三家国内直连,各一次登录)。**趁 ground truth 在眼前时顺手修
     qoder 路径漂移**(见 M1-10)
  4. **第 3 天**:aider(注意在 **cwd** 不在 home)和 cline。
     锁 variant 前先对照 `deja-vu` 的 cline legacy-vs-modern 两种布局
  5. **第 4 天**:openclaw(先升 node 到 24.15+)
  6. **然后决策**:hermes 是新增 variant 还是撤回声明(见 M1-11);
     grok-build / cursor 记为诚实未验证
  **本周内无论做什么都改变不了 grok-build 和 cursor** —— 那需要
  xAI / Cursor 的付费通道。
  **验收**:每个采集到的 provider 在 §7 记录"已采集 + 落点 + 采集方式"。

- [ ] **M1-4 逐 provider 解析无损验证(D9)**
  对每个有真实样本的 provider:用真实文件跑 sync,验证
  session/message/父子链/时间戳/角色全部正确;记录 emitted vs persisted;
  跳过的行按 D11 计数上报。发现的缺口写进 `capability.rs` 的
  `known_limitations`。
  **验收**:该 provider 的真实运行 emitted == persisted 或差额有明确解释。

- [ ] **M1-5 反推合成 golden fixture(D9)**
  ⚠️ **已知待补(M1-13 发现)**:`provider-codex/tests/golden/basic.jsonl`
  **零个** `custom_tool_call` / `function_call_output` 记录,所以它无法见证
  codex 声明的 tool-activity pairing 能力(提取是真实现了的,
  见 `emit_paired_activity`/`emit_unpaired_activity`)。
  当前由 `capability_behaviour.rs` 的具名补充样本承载证据 ——
  **扩 fixture 后必须删掉那条登记项**(有测试守着,登记项过期会失败)。
  fixture 是 BLAKE3 pin + `expected.json`,改动要同步重算并更新 PROVENANCE。
  同类可查:claude 的 fixture 也没有 `tool_use`/`tool_result` 块。
  按真实样本观察到的**形状**手写一份合成 fixture(虚构项目代号、假 UUID、
  占位路径),配 `PROVENANCE.md` 说明它模仿的真实结构与固定种子/BLAKE3 哈希。
  用 insta 快照(D13)固定归一化结果。
  **格式证据的最佳来源(全部宽松许可,可放心参考形状)**:
  `vshulcz/deja-vu`(MIT,644★)是最全的 registry —— `fixtures/registry/`
  下有 aider/antigravity/claude-code/cline/codex/copilot/cursor/gemini/
  goose/grok/hermes/kimi/openclaw/opencode/pi/qwen/roo/zed,
  且含 **cline 的 legacy 与 modern 两套布局**、grok 的
  `summary.json` + `updates.jsonl`、openclaw 的
  `.jsonl` + `.checkpoint.<uuid>.jsonl` + `sessions.json`、
  hermes/opencode/zed 的 `.sql`。
  另有 `eric-tramel/moraine`(Apache-2.0,110★,含 kimi 子 agent、
  hermes trajectory、cursor `state-vscdb-kv.jsonl`)、
  `letta-ai/trajectory`(Apache-2.0,227★)、`rjx18/codor`(MIT,271★)、
  `GliteTech/glite-english-audit`(Apache-2.0,每场景一份 `opencode.db.sql`)。
  **验收**:golden 测试进 `cargo test --workspace`;真实样本不进仓库。

- [ ] **M1-6 解析容错改造(D11)**
  损坏/截断/未知字段行:跳过并计数,结束时输出
  "skipped N lines (use --verbose for detail)",默认不中断。
  真实会话文件常有写到一半的尾行(agent 正在跑或进程被杀)。
  **验收**:构造截断文件的测试;sync 仍退出 0 并正确报告跳过数。

- [ ] **M1-7 provider 升 Beta(G2)**
  达到 D9 标准的 provider 在 `capability.rs` 升 `Beta`,同步
  `PROVIDER-MATURITY-MATRIX.md`(有漂移测试守着,必须同步)。
  目标 ≥5 个 + Claude/Codex。达不到的诚实留 `Experimental`。
  **验收**:漂移测试绿;矩阵与代码一致。

- [x] **M1-8 修 discover 声明与实现不一致(§1.8)**
  `capability.rs` 只给 claude-code/codex 声明 `discover: Native`,
  但 `main.rs:2972-2985` 实际支持 6 个。要么升那四个的声明,要么
  把矩阵注释改成解释"注册了 root ≠ discovery 能力已认证"。
  `PROVIDER-MATURITY-MATRIX.md:63` 那句自相矛盾(说"仅 claude-code/codex 注册"
  又说"antigravity/opencode 已加入")必须改。
  **验收**:三处(代码、矩阵、SKILL.md)一致。

- [x] **M1-9 修 opencode↔zcode probe 冲突(真实 `AmbiguousVariant` 缺陷)**
  实测:opencode adapter 的三条 SQL 对 zcode 的 `db.sqlite` **全部成功**,
  所以 `opencode/sqlite-v1` 会把 zcode DB 认成 opencode。
  按 zcode 独有列判别:`session.task_type`/`title_source`/`trace_id`/
  `title_message_id`/`time_title_updated`、`message.sequence`;
  zcode 少了 opencode 的 token/cost 列。
  **收紧 opencode 的 probe 让它显式拒绝 zcode**,并新增 zcode adapter。
  **验收**:两个真实 DB 各被正确识别;probe 冲突有回归测试。

- [x] **M1-10 修 qoder 路径三方不一致**
  adapter 与文档写 `~/.qoder/projects/<project>/transcript/*.jsonl`;
  官方文档写 `~/.qoder/projects/<项目>/*.jsonl` + `state.json`
  (**没有 `transcript/` 这一段**);而本机装的 Qoder IDE 1.106.3 实际写
  `~/.qoder/cache/projects/<project>-<hash>/conversation-history/<id>.txt` ——
  实测是**纯文本不是 JSONL**(空行、`--- Request: <uuid> ---`、`user:`、
  `<communication>…</communication>`、`<user_query>…</user_query>`)。
  AgentRecall 声称那个路径是 `*.jsonl`,本机是 `.txt`。**三方分歧。**
  M1-3 第 3 步拿到 ground truth 时一并修。
  **验收**:路径与格式与真实观察一致;CLI 与 IDE 两条路径都被记录。

- [x] **M1-11 hermes 声明的格式已停用(需决策)**
  上游文档明确:`~/.hermes/state.db`(SQLite + FTS5,WAL)是权威格式,
  **"replaces the earlier per-session JSONL file approach"**;
  遗留在 `~/.hermes/sessions/` 的 `.jsonl` **"no longer written or read"**。
  AgentRecall 也指向 `state.db`。而本项目 adapter 解析的是
  `~/.hermes/sessions/session_<id>.json`。
  → **生成一个真实 hermes session 会产出 adapter 读不了的文件。**
  本机 `~/.hermes` 只有 `config.yaml`,无 `state.db`。
  **决策**:新增 `hermes/state-db-v1` variant,还是撤回 hermes 声明。
  **验收**:声明与上游现实一致。

- [x] **M1-12 kimi wire.jsonl 未处理的记录类型**
  `deja-vu`(MIT,644★)的 kimi fixture 覆盖 14 条记录 / 6 种类型
  (`metadata`、`config.update`、`turn.prompt`、`context.append_message`、
  `context.append_loop_event`、`usage.record`),是本项目 golden fixture
  6 行的**严格超集** —— `usage.record`、`turn.prompt`、`metadata`、
  `config.update` 是未处理类型,值得补。
  **验收**:补齐后 golden fixture 覆盖全部 6 种类型。

---

### M2P — 首次运行正确性(**审计新发现,优先级高于 M2,与 M1 并行**)

> 文档/UX 审计用真实二进制在一次性伪 HOME 下逐条验证,发现五个 **P0**:
> 它们不是文档问题,是**新用户第一次用就会被静默坑到**的功能缺陷。
> 每条都有实测复现记录,不是推断。

- [x] **M2P-1 human 模式 search 输出的 Session ID 不可用(最伤信任)**
  `human.rs:712-751` 的"Session ID"列填的是
  `metadata.provider_session_id`(`main.rs:2854` 的 `attach_session_resume_rows`),
  是 **provider 原生 id**;而 `context` / `show` / `get-session-resume` 全部
  只认规范 id。实测:把表格里的 id 复制给 `context` →
  `error [invalid_request]: not a valid session id`,退出 2。
  而 `main.rs:947` 的 help 文本明确承诺
  "search 返回命中消息 → show &lt;msg_id&gt; → context &lt;ses_id&gt;" ——
  **这条被文档化的数据流从 human 输出出发根本走不通**。
  规范 `ses_v1_...` 其实已经在 robot envelope 里(`suggested_next_commands`
  连完整命令都拼好了),只是 human 模式从不显示。
  `CONTEXT.md:228-236` 记录了双 id 规则,human 渲染器静默违反了它。
  **修法**:human 表格渲染规范 `ses_v1_…`(或两列都给),并在表格下方打印
  已算好的 `suggested_next_commands`。
  **验收**:照抄 human 输出的 id 能直接跑通 `context`/`show`。

- [x] **M2P-2 读命令在路径打错时静默新建空库**
  实测 `asg --db &lt;typo&gt;.db search hello` → `no hits`,退出 **0**,
  并创建了一个 233 KB 的新库。search 是所有人第一个敲的命令,
  打错一个字母就得到"干净的无结果",毫无警告。**这是最可能的静默失败。**
  **修法**:读命令不建库;库不存在时报 `not_found`,消息里给出路径本身
  和该跑的 `sync --discover` 完整命令。
  **验收**:对不存在的库跑 search 报错且不留文件。

- [x] **M2P-3 `--db` 无默认值、无环境变量、config.toml 是死路(补强 D8/M3-1)**
  `asg config paths` 打印 `config: .../AgentSessions/config.toml`
  (`main.rs:871,887,911,927`),但**全仓库没有任何代码读这个文件** ——
  grep 只命中这四处路径拼装。也没有 `ASG_DB` 之类环境变量。
  更糟:`config paths` 报告的 data 目录**工具自己不会创建**,实测
  `asg --db &lt;data&gt;\asg.db status` → `[catalog_error] 数据库内部错误`,
  错误还反过来怪你路径写法;`mkdir` 之后同一命令立刻成功。
  **修法**(按价值排序):(a) `--db` 默认 `&lt;data&gt;/asg.db` 且自动创建父目录;
  (b) 支持 `ASG_DB`;(c) config.toml 要么真读要么别打印。
  **验收**:全新环境 `asg search "x"` 无参数可跑;`config paths` 不撒谎。

- [x] **M2P-4 空库死胡同:三条命令都不提 `sync --discover`**
  `search` → `no hits`(`human.rs:344-346`)、`list` → `catalog is empty`
  (`human.rs:391`)、`status` → `entities: 0`。全新库上唯一正确的下一步动作
  **一次都没被说出来**。
  **修法**:`catalog_count == 0` 时打印
  "索引为空 — 运行:asg --db &lt;path&gt; sync --discover"。
  **验收**:三条命令在空库上都给出该提示。

- [x] **M2P-5 一个坏 `.jsonl` 让整个 discover 退出 2**
  实测:放一个非法 jsonl 进 `~/.claude/projects/`,`sync --discover` 报
  `no provider recognized this source; ...not a SQLite database (missing
  magic header)` 并**整轮退出 2,零消息入库**。既泄漏 probe 内部细节,
  又拿 SQLite 报错去怪一个文本文件。注意:**逐行**损坏已经按 D11 宽容处理了,
  缺的是**逐源文件**的宽容。
  **修法**:单个源无法识别时可恢复跳过 + 警告计数,与行级处理对齐。
  **验收**:目录里混入垃圾文件后 discover 仍退出 0 并报告跳过数。

- [x] **M2P-6 `sync --discover` 的 per-provider 报告在 human 模式被丢弃**
  `main.rs:3212-3232` 真的算出了 14 个 adapter 的
  `{id, found, removed, complete}` 和 `discovery.complete`,但
  `human.rs:551-571` 的 `render_sync` 从不读 `data.discovery`。
  用户因此不知道扫了哪些 provider、哪些根不存在、扫描是否不完整。
  更糟:`render_sync:565-567` 按 `emitted` 而非 `committed` 分支,
  导致什么都没提交的重跑仍打印"总结:新增 5 条消息"——
  这个 bug 只在源文件有跳过行时触发,**而真实 transcript 正是常态**。
  **修法**:打印每 provider 一行(found / 根不存在 / discovery 不支持时
  提示手动传什么文件),摘要改按 `committed` 分支。
  **验收**:human 输出能看出扫描覆盖面;no-op 重跑不谎报新增。

- [x] **M2P-7 discover 只认 `.jsonl` 扩展名 → 6 个已注册根里有的永远找不到**
  `discover_provider_sources`(`main.rs:3049-3051`)只收扩展名恰好是
  `jsonl` 的文件。实测:把真实 `opencode.db` 放到它注册的根
  `~/.local/share/opencode/`,得到 `{"complete":true,"found":0,"id":"opencode"}`
  和 `entities: 0` —— **报告成功却永远找不到**,因为 OpenCode 是 SQLite 格式。
  另外 14 个已实现 provider 中 **8 个根本没有 discovery 根**
  (aider、grok-build、pi、qoder、kimi-code、cline、hermes、cursor),
  `capability.rs` 对此是诚实的(`discover: Unsupported`),但
  `README.md:89-106` 的表格把 14 个都列成 Experimental 且**没有 discovery 列**,
  于是 Cursor 用户跑 `sync --discover` 得到退出 0 + 空索引,
  无从得知必须手动传 `state.vscdb`。手动 `sync` 这些格式是能用的(已验证
  `.md`/`.json`/`.db` 显式命名时都能入库)。
  **修法**:discover 按 provider 声明的格式收文件(不只 jsonl);
  README provider 表加 discovery 列(与 M4-2 合并做)。
  **验收**:opencode 的 `.db` 能被 discover 找到;README 表有 discovery 列。

  **2026-08-19 实测基线(在假 HOME 上,给修复前后做对照)**:
  同时放 `.claude/projects/proj/a.jsonl` 与
  `.hermes/sessions/session_h-s1.json`,`sync --discover` 只找到 **1 个**源
  (claude-code),hermes 那个 JSON 完全不在视野内。
  逐 provider 报告(M2P-6 新加的,很有用)把两种零结果**分开**了:
  ```
  claude-code  found=1 complete=true  root_state=scanned
  codex/openclaw/codebuddy/antigravity/opencode  root_state=missing     ← 有根，磁盘上没有
  aider/grok/pi/qoder/kimi/cline/hermes/cursor   root_state=unsupported ← 压根没注册根
  ```
  → **这重新划分了 M2P-7 的范围,它其实是两个不同的缺口**:
  (a) **扩展名过滤**:只影响 `root_state=missing` 那 5 个里的 opencode
      —— 它的根注册了、格式是 SQLite,所以永远 `found=0`。
      这条是纯 bug,按 provider 声明的格式收文件即可。
  (b) **8 个 provider 连根都没有**(`root_state=unsupported`)。
      这**不是 bug**:`capability.rs` 的 `discover: Unsupported` 是诚实的,
      而且这 8 个里有几个的真实路径本来就未核实(见 §1.8.1 的 qoder 三方
      路径不一致、hermes 上游已停用该格式)。**乱填一个根比不填更糟** ——
      会让 `found=0` 从"我不找"变成"我找了但没有",掩盖路径其实是错的。
      这条要等 M1 真实样本落地后逐个补,不能凭猜。
  所以验收拆成两条:(a) opencode 的 `.db` 能被 discover 找到;
  (b) `root_state` 三态(scanned/missing/unsupported)对用户可见且 README
  provider 表有 discovery 列 —— 让"我不找"和"我找了没有"永远分得开。

- [x] **M2P-8 unknown-subcommand 列表过时**
  `main.rs:2347-2366` 的可用命令列表漏了 `handoff`、`resume`、`hook`、
  `serve`、`providers`、`model`、`get-session-resume`。实测
  `asg --db x handof q` 会声称 `handoff` 不是可用命令。
  **修法**:从 `known_subcommand`(`main.rs:1096`)派生这个列表,让它无法漂移;
  加 did-you-mean。
  **验收**:列表与 `known_subcommand` 一致(加测试守护)。

- [x] **M2P-16 四个 provider 的 JSONL 形状互相歧义 → 合法文件被整体拒绝
  (2026-08-19 我在建 10 万条语料时实测撞上;同日闭合)**
  一个最朴素的两行文件:
  ```
  {"type":"session","id":"amb-1","cwd":"/work/placeholder"}
  {"type":"message","message":{"role":"user","content":"..."}}
  ```
  实测 `sync` 直接拒绝、退出 2:
  ```
  invalid_request: ambiguous provider selection: multiple variants matched
  with equal confidence (one candidate was pi/session-jsonl-v1)
  ```
  `pi` / `openclaw` / `qoder` / `tencent-codebuddy` 的 golden fixture 形状
  近乎相同(都是 `{"type":"session"|"session_meta"} + {"type":"message"|role}`),
  probe 无法区分 → 等confidence 歧义。
  **注意这不是 bug 而是设计**:RFC-0002 明确要求歧义一律 `AmbiguousVariant`
  拒绝,不做低置信度猜测 —— 拒绝比猜错好。但**后果是真的**:
  - 这四个 provider 的真实 transcript 若形状落在交集里,用户无法 sync,
    而错误消息只说"多个 variant 等置信度匹配",不告诉用户能做什么。
  - 它同时封死了为这四个 provider 造合成语料的路 —— M2-1 的 agent 正因此
    刻意避开了它们(它预判到了,我没有,我撞上了才知道)。
  **修法方向**(需要先看真实样本再定,不要凭空改 probe):
  (a) 给 `sync` 加 `--provider <id>` 显式消歧(目前 sync **不接受**该 flag,
      见 M2P-15);(b) 让 probe 用各 provider 独有的次级信号提高置信度;
  (c) 至少把错误消息改成可行动的:列出候选 variant 并说明如何指定。
  **依赖**:(a) 与 M1 的真实样本采集互为前提 —— 真实文件到手才知道形状交集
  在实际数据上多大。
  **验收**:这四个 provider 的真实样本能被 sync;或错误消息给出消歧命令。

  ---

  **⚠️ 2026-08-19 深夜复测:比原先记录的严重得多,而且不必等真实样本。**
  逐个把**已提交的 golden fixture** 喂给 `sync`(排除 `expected.json`),
  12 个 provider 里 **3 个连自己的 fixture 都同步不了**:

  | provider | 结果 | 说明 |
  |---|---|---|
  | **codex** | **AMBIGUOUS** | **旗舰 provider,有真实语料回归,`discover: Native`** |
  | **pi** | **AMBIGUOUS** | |
  | **openclaw** | **AMBIGUOUS** | 报的候选是 `pi/session-jsonl-v1` |
  | 其余 9 个 | ok | claude/kimi/qoder/codebuddy/grok/aider/cline/hermes/antigravity |

  **三条修正原记录的判断:**
  1. **codex 在受影响名单里,而原记录没提它。** 原记录说是
     `pi`/`openclaw`/`qoder`/`codebuddy` 四个。实测 qoder 与 codebuddy
     **能过**,而 **codex 不能** —— 一个有真实语料回归、`discover: Native`、
     被当作旗舰的 provider,**其提交在仓库里的 golden fixture 无法被自己的
     工具同步**。这不是"合成 fixture 太简陋"能解释的。
  2. **不必等真实样本。** 原记录写"修法需要先看真实样本"。不需要 ——
     仓库里已有的 fixture 就足以复现,而且这些 fixture 是各 provider
     格式证据的权威载体。真实样本能告诉我们交集在实际数据上多大,
     但**修 probe 的判据现在就有**。
  3. **错误消息本身是第二个缺陷**:它只报**一个**候选
     (`one candidate was ...`),不说和谁撞了。用户拿到这条消息无法自查,
     连"哪两个 provider 需要消歧"都不知道。
     → 这条**独立可修且必须修**:歧义错误必须列出**全部**候选 variant。

  **为什么这条现在是发布阻塞而非可延后**:`sync --discover` 会遇到并跳过
  这些源(M2P-5 让它跳过并计数而非整轮失败),所以用户的 codex 历史会被
  **静默跳过**并只在 warning 里出现一个计数。旗舰 provider 的历史进不了索引,
  而工具报的是"成功"。

  **可立即执行的修法(不依赖 M1)**:
  (a) 歧义错误列出全部候选 —— 纯诊断改进,零风险;
  (b) 给 `sync` 加 `--provider <id>` 显式消歧(`--provider` 已在 M2P-9 打通了
      矩阵校验,复用即可);
  (c) 逐个查 codex/pi/openclaw 的 probe 为何与他人等分,用各自独有的
      **必需**字段提高置信度 —— codex 有 `payload.type`/`session_meta` 封套,
      pi 有 `type:"session"` header,openclaw 是 v3 header;三者本应可区分。

  ---

  **✅ 2026-08-19 已修一半,并查明另一半的性质(重要:不是同类问题)**

  **(a) 已完成** `eba0ab2`:歧义错误现在列出全部并列 variant。这个改动立刻
  自证价值 —— 新消息暴露出真实碰撞对是
  **codex ↔ qoder** 与 **pi ↔ openclaw**,而旧消息只报第一个候选,
  这正是原记录把受影响 provider 记错成四个的原因。

  **codex ↔ qoder:已修**(见下一条 commit)。根因是 qoder 的 probe 只看
  `type == "session_meta"` 就计数,而 codex 的 rollout **也有**
  `{"type":"session_meta"}` —— 区别在 codex 把身份包在 `payload` 对象里,
  qoder 把 `session_id`/`cwd` 放在**顶层**。这是结构性差异,不是启发式:
  qoder 现在要求顶层身份字段且不得有 `payload` 封套。修完 codex 与 qoder
  的 fixture 都能同步。

  **pi ↔ openclaw:性质不同,不能靠 probe 修。**
  两份 fixture 的前四行**逐字节相同**,唯一差异是 pi 多一条 `compaction` 记录。
  而 openclaw adapter 自己的文档就写着(`provider-openclaw/src/lib.rs:15-16`):
  > "The type-based dispatch and content extraction **follow the same v3 JSONL
  > shape as the Pi adapter**"

  **也就是说这两个 provider 在磁盘上的格式确实是同一个形状** ——
  `{type:session,id,cwd,timestamp}` + `{type:message,message:{role,content}}`。
  **没有任何内容 probe 能区分它们,因为没有内容差异可区分。**
  唯一的区分信号是**目录根**:`~/.pi/...` 对
  `~/.openclaw/agents/<agent>/sessions/`(或 `~/.clawdbot`/`OPENCLAW_STATE_DIR`)。

  **关键发现:root 提示已经存在,但用错了地方。**
  `discover` 已经在 `discovered_provider_ids` 里记下了每个文件来自哪个 root
  (`main.rs:3608`),但这个提示只在**归属成功之后**用于打标签
  (`main.rs:4478` 传给 `staged_to_source_with_provider`)——
  **它从不参与 probe**。所以一个歧义文件在提示能帮上忙之前就已经被拒了。
  → **正确修法**:把 root 提示作为**消歧输入**下推到选择逻辑
  (root 已确定 provider 时,同分候选里优先该 provider),而不是等归属完再贴标签。
  这也顺带解决显式 `sync <file>` 的情形:用户手动传单个文件时没有 root 上下文,
  所以仍需 (b) 的 `--provider` 显式消歧。
  **不要试图给 pi/openclaw 造"独有字段"** —— 那会是凭空发明格式差异,
  正是本仓库反复撤回过的那类无证据声明。

  ---

  **✅ 2026-08-19 全部闭合。三处改动,全部有实测证据。**

  1. **codex ↔ qoder**(`a12ef29`):qoder probe 要求顶层身份字段且无 `payload`
     封套。两边 fixture 都能归属。
  2. **provider hint 下推为消歧输入**:`select_and_stage_source` 新增
     `provider_hint` 参数,`choose_probed_candidate` 在同分且多 variant 时
     用它选出胜者。discover 路径传 `discovered_provider_ids`(root 归属),
     此前该值只在归属**之后**用于打标签,从不参与判定。
  3. **`sync --provider <id> <file>...`**:显式 `sync` 没有 root 上下文,
     用户点名即唯一证据。走同一条 hint 通道,所以只在同分时生效 ——
     点错名字不会把一个格式硬塞给另一个 provider(实测:
     `--provider claude-code` 传 pi fixture 仍然拒绝,并说明 claude-code
     不在并列候选里)。校验权威是 `provider_registry` 而非能力矩阵:
     矩阵含两个 deferred provider(无 adapter),点名它们只会换来
     "没人认领"这种误导性错误。

  **实测(release 二进制,伪 HOME)**:
  - `sync --discover` 在 `~/.openclaw/agents/main/sessions/` 下发现 openclaw
    fixture 并入库 3 条消息,归属 `openclaw/session-jsonl-v3`。**此前整份文件
    被 skip-and-count 静默跳过。**
  - `sync --provider pi <pi fixture>` 入库 3 条,归属 `pi/session-jsonl-v1`;
    不带 flag 仍按 RFC-0002 拒绝(证据缺席即拒绝,不猜)。
  - `sync --provider bogus` 列出全部 14 个可选 id。

  **仍然成立的边界**:pi 没有注册 discovery root,所以它只能走显式
  `sync --provider pi <file>`。这不是缺陷 —— pi 的磁盘位置本仓库没有证据,
  凭空注册一个 root 才是无证据声明。

- [x] **M2P-17 `sync` 不接受目录 + Windows 命令行上限 → 大语料无法一次同步
  (2026-08-19 实测)**
  `sync` 只收显式文件列表(`sync <file>...`,help 明写"不接受目录"),
  于是 5000 个源文件的路径展开后**超过 Windows 32 KB 命令行上限**,
  PowerShell 直接报「文件名或扩展名太长」,进程根本没启动。
  `real_data_regression.py` 和新的 `synthetic_corpus.py` 都各自实现了
  24 KB 分批来绕过它 —— **两个 harness 重复实现同一个 workaround,
  说明缺口在工具而不在脚本**。
  `--discover` 能绕过(路径由发现填充),但它只扫 provider 数据根,
  对"我有一堆导出的 transcript 在某个目录"这个场景无解。
  **修法**:加 `--from-file <list>`(每行一个路径,ripgrep/xargs 惯例)
  或让 `sync` 接受目录并递归。前者更小且与现有"不接受目录"的显式设计一致。
  **验收**:5000 个源文件可一条命令同步,不需调用方分批。

- [x] **M2P-9 `--provider` 只接受 16 个中的 3 个**
  `main.rs:2434`(以及 `mcp.rs:888` 的 enum)只认
  `claude|claude-code|codex`,而 `asg providers` 列 16 行。实测
  `search Rust --provider aider` 报 `unknown provider: aider`,
  **即使 aider 的数据就在索引里且可被搜到**。
  根因在类型层:`enum SearchProvider { Claude, Codex }`(`ports/src/lib.rs:200-203`)。
  ⚠️ **Web UI 更糟**:provider 下拉框从全部 16 行矩阵填充
  (`web/index.html:166-176`),但 `/api/search?provider=` 只接受那 3 个
  (`serve.rs:792`)→ **16 个选项里 13 个返回 HTTP 400 `invalid_request`**。
  这是唯一图形界面上前 30 秒就能看见的缺陷。
  **修法**:扩到 `providers` 打印的规范 id 全集(要动 `SearchProvider` 类型);
  或在 help、README 与 Web 下拉框三处同时明写限制。
  **验收**:能按任一已实现 provider 过滤,或限制在三处一致地被文档化。

  **2026-08-19 实测复现 + 修法已定(读代码确认,可直接照做)**:
  ```
  sync 一条 codebuddy 记录 → committed: 1
  search "marker"                        → hits=1   ← 数据在库里
  search "marker" --provider tencent-codebuddy
    → invalid_request: unknown provider: tencent-codebuddy
      (expected claude|claude-code|codex)          ← 按自己的 provider 过滤被拒
  ```
  **关键发现:enum 是多余的抽象,不需要列 16 个变体。** SQL 下推处
  (`adapters-sqlite/src/lib.rs:6276-6282` 与 `push_provider_predicate`)
  只用 `provider.as_str()` 拼 `json_extract(payload,'$.provider') IN (?,?)`
  —— **存储层根本不关心有几个变体,它只要字符串**。全仓库对
  `SearchProvider::Claude|Codex` 的引用只有两处解析点
  (`main.rs:2630-2631`、`mcp.rs` 的 enum)其余全是测试。
  → 正确修法是把 `SearchProvider` 从**封闭 enum** 换成**对
  `ProviderCapabilityMatrix::current()` 校验过的字符串**:合法性由单一真源
  (矩阵)判定,不再由类型系统里一份手抄的副本判定。这同时消掉了"加 provider
  要改 enum"这条隐性维护债。
  建议只允许 `search: Native` 的 provider(14 个已实现全是 Native;两个
  deferred 是 Unknown,应拒绝)—— 让能力矩阵真正成为过滤器的权威。
  **Web/serve 是同一根因不需单独修**:`serve.rs:796,826` 把 `provider` 查询参数
  原样转成 `--provider` 交给同一个解析函数,所以 CLI 修好后 HTTP 400 自动消失。
  下拉框(`web/index.html:166-176`)本来就从 `/api/providers` 拉全部 16 行,
  修好后它才第一次名副其实。
  ⚠️ **落地时机**:`ports/src/lib.rs` 当前被 M2P-5/7 的 agent 持有
  (它在同文件加 `read_source_head` / `SourceFormatFamily`),等其合并后再动,
  否则必冲突。

- [x] **M2P-12 `--since`/`--until` 静默丢弃无时间戳 provider(接近正确性 bug)**
  时间过滤下推是 `asg_instant_sort_key(...) >= ?`,而 **NULL 对任何比较都失败**
  (`adapters-sqlite/src/lib.rs:6395-6405`)。
  **静默给错答案比报错更糟,这违反本项目自己的原则。**

  ⚠️ **2026-08-19 调研推翻了原本的"直接传播时间戳"方案。逐 provider 结论:**

  | provider | 格式里有逐消息时间戳吗 | 结论 |
  |---|---|---|
  | **codex** | 有 envelope `timestamp`,**但它是 per-occurrence 的重放写入时间** | **不要传播**。resume 会把复制的前缀用重放时刻重新盖章(原作于 02:08 的消息在续接文件里盖成 03:03:50)。传播 = 另一个方向的静默错误,**而且会让多文件 ingest 硬失败**:同一 msg id 在两个文件里带不同 timestamp,`merge_message_payloads` 判冲突 → exit 6 → **旗舰 provider 的历史变成不可 ingest**。另有 e2e `compact_relative_duration_uses_application_clock` 断言了当前的 NULL 排除行为(CLI 层测试,provider agent 改不了)。→ 保持 None,在 `known_limitations` 里诚实写明"envelope 时间是重放写入时刻,不作为稳定消息时间传播",并记录它对时间过滤的影响 |
  | **opencode** | 有 `message.time_created`(epoch millis,本机真实库已验证) | **可安全传播**。它用真实 `msg_id` 做 native id,跨文件不碰撞 → 无 merge 冲突。需 epoch→RFC3339 转换(`parse_search_instant` 只认带时区的 RFC3339)。golden fixture 的 `time_created` 是 1/2/3/4 占位值,应重生成为真实量级 epoch |
  | **grok-build** | 有,每行 RFC3339 字符串 | 可传播(取该消息首个 chunk 的时间,是真实创作时刻不是臆造)。**但被下面的碰撞问题阻塞**。另有latent bug:`UpdateRecord.timestamp` 声明为 `Option<String>`,真实数据若是数字会导致整行反序列化失败被跳过 → 应改 `Option<Value>` 兼容 |
  | **kimi-code** | 有,记录级 `time`(epoch millis) | 同上,被碰撞阻塞 |
  | **cline** | 有,记录级 `timestamp`(字符串或 epoch) | 同上。现有代码把数字时间戳转成**空字符串**(`provider-cline/src/lib.rs:181-185`),这是明确的 bug |
  | **aider** | **确实没有**(只有 run 级 header 时间) | 不传播;在 `known_limitations` 补一条"无逐消息时间戳" |

  **依赖**:grok/kimi/cline 必须先做 M2P-14(合成 id 碰撞),否则传播时间戳会把
  一个静默丢数据的 bug 变成整轮 sync 硬失败。

  **进度(2026-08-19)**:
  - [x] **opencode 已完成** `559cfcf`。原本连 `time_created` 列都没 select
    (只出现在 `ORDER BY` 里),所以每条消息都是 `timestamp: None`。现读 epoch-ms
    渲染成 `YYYY-MM-DDTHH:MM:SS.mmmZ`;`<= 0` 或 `> 9999-12-31` 返回 None
    而非编造时刻;列类型异常时丢时间戳而不丢消息。golden 新增
    `golden_timestamps_fall_inside_search_window`,用**生产**的
    `parse_search_instant` 解每条时间戳并断言 `sort_key()` 落在预期窗口内 ——
    这样"能被时间过滤命中"本身成了被测性质,而不是靠肉眼看字符串像不像。
  - [x] **codex 结论已定:不传播**(见上表理由),需在 `known_limitations`
    写明 envelope 时间是重放写入时刻。
  - [ ] **grok-build / kimi-code / cline** —— 阻塞在 M2P-14,等其合并后再做。
  - [ ] **aider** —— 补 `known_limitations` 一条"无逐消息时间戳"。
  - [x] **G18 的另一半已完成**(`31a4350` `1baf011` `c660a7e` `27db18a`):
    时间过滤现在如实报数。它与 provider 覆盖面无关 —— 即使全部 provider 都有
    时间戳,`timestamp IS NULL` 的历史行仍会被排除,所以这条独立成立。
    计数**来自对同一批非时间谓词的查询**,不是全表 NULL 数,否则会虚报。
    实测四个用例:(A) 无时间窗 → 7 命中零 warning,输出字节不变;
    (B) 有时间窗 → 3 命中 + "time filter excluded 4 records with no
    timestamp; re-run without --since/--until to see them";
    (C) 时间窗 + 只命中有时间戳记录的查询词 → 3 命中**零 warning**
    (证明计数尊重查询谓词,没把无关的 NULL 行算进来);
    (D) 时间窗 + 只命中无时间戳记录的查询词 → **0 命中 + 明确告知 4 条被排除**
    —— 这正是原缺陷最恶劣的形态(自信地给出空答案),现在它会说话。
    ⚠️ 该 agent 死于 API 错误,死前只验了 `cargo build` 未验 `--all-targets`,
    留下 4 处测试构造点 + 1 处测试解构缺字段,已由主线补齐(`27db18a`)。

- [x] **M2P-14 合成消息 id 跨文档碰撞 → 静默丢数据(新发现,独立于时间戳)**
  **这是比时间戳更严重的缺陷,而且此刻正在发生。**
  cline / grok-build / kimi-code / aider(可能还有 hermes/pi/qoder/openclaw/
  codebuddy)emit 形如 `cline-msg-{seq}` 的**按序号合成 id**。序号是文档内计数,
  所以**任意两个文件的第 0 条消息都拿到同一个 id**。
  **已实测**:两个互不相关的 cline task 文件各含一条消息,都得到
  `cline-msg-0`;sync 报告"新增 2 条消息",但**第二条被静默丢弃**,
  搜索搜不到。`merge_message_payloads` 在 text 差异时保留较长的那条,
  等长时保留先到的 —— 另一条无声消失。
  **修法**:CLI 已经有正确路径 —— `main.rs:3317` 在 `native_id` 为空时
  按 `[provider, variant, document_id, seq]` 派生**文档作用域**的 id
  (`Stability::Unstable`)。所以这些 provider 应该 **emit 空 native_id**,
  让 CLI 派生,而不是自己造一个会碰撞的。
  副作用:存储 id 从 `msg_v1_cline-msg-0` 变成 `msg_v1_<hex>`;
  同内容重复 sync 仍幂等(document_id 是内容指纹)。
  ⚠️ 落地前先 grep 非 provider crate 是否有测试断言这些合成 id
  (CLI e2e 若断言了,provider agent 改不了那些测试 → 需要单独一轮)。
  **验收**:两个不同文件的同序号消息都能被搜到;有回归测试。

- [x] **M2P-15 `sync` 把任何未识别的 flag 当成文件路径,再拿 `source_io` 怪路径
  (2026-08-19 实测新发现)**
  实测三次,全部复现:
  ```
  asg --db <db> sync --provider claude-code <existing readable file>
  → error [source_io]: 源文件无法读取
     下一步:确认源文件路径存在且可读
  → exit 5
  ```
  同一个文件去掉 `--provider` 立刻成功入库 40 条。换成完全臆造的
  `--bogus-flag` 也是同一条 `source_io`。Robot JSON 里同样是
  `{"code":"source_io","message":"源文件无法读取","details":{}}`。
  **为什么这条比"参数名写错了"严重**:诊断把用户**指向错误的方向**。
  路径明明存在且可读,工具却让你去检查路径 —— 用户会去查权限、查盘符、
  查转义,而真正的原因是那个 flag 不存在。`details` 是空的,连它到底
  试图打开哪个"文件"都不说。这与 M2P-3 的 `catalog_error` 反过来怪路径
  写法是同一个失败模式:**用错误的错误码,把人送去错误的地方。**
  根因:`sync` 的位置参数收集把所有剩余 token 当路径,不校验 `-`/`--` 前缀。
  `--provider` 确实不是 `sync` 的参数(它接受 `<file>...` 与 `--discover`),
  所以拒绝是对的,**错的是拒绝的方式**。
  **修法**:`sync` 的位置参数遇到 `-` 开头的 token 时报 `invalid_request`,
  消息里回显那个 token 并给出 `sync --help` 的合法形态。同时给 `source_io`
  的 `details` 加上它实际尝试打开的路径 —— 空 `details` 让这类错误无法自查。
  ⚠️ 顺带核查其他子命令有没有同一个模式(位置参数不校验前缀)。
  **验收**:未识别 flag 报 `invalid_request` 且消息含该 flag 本身;
  `source_io` 的 details 含被尝试的路径。

- [x] **M2P-13 `aider` 的 `tool_activity: Partial` 是假声明** — 已完成
  `c1356cb`(声明)+ `2fc1d3e`(两份 ledger)。
  `capability.rs` 降为 `Unsupported` 并写明理由;
  `PROVIDER-BETA-READINESS.md:39` 的 `partial → missing`;
  `PROVIDER-MATURITY-MATRIX.md:66` 的 per-capability 汇总行也列着 aider
  在 `partial` 下(第二处漂移,agent 没碰到但下一步就会撞上),同步修正。
  由 M1-13 的行为断言守护,今后同类假声明会在证据层失败而不是在
  "文档与声明一致地假"这层通过。
  `capability.rs:143` 声明 `Partial`,`PROVIDER-BETA-READINESS.md:39` 记 `partial`,
  但 `provider-aider/src/lib.rs` 里**零个 `emit_activity` 调用** ——
  它的 blockquote 工具输出被折进 assistant 文本(`:219-229`)。
  **aider 根本不产出任何 tool activity。**
  之所以能过 CI:16 行漂移测试(`provider_matrix.rs:311-345`)只比对
  矩阵与**文档**,从不比对 adapter 的**实际行为**。
  **修法**:要么真做 aider 的 activity 提取,要么把声明降为 `Unsupported`。
  并加行为断言(见 M1-13)。
  **验收**:声明与代码行为一致。

- [x] **M1-13 加"声明 vs 行为"断言(结构性,防止矩阵自我认证)** — 已完成
  `e4f836f` + `c1356cb` + `2fc1d3e`。落在
  `crates/agent-session-grep-cli/tests/capability_behaviour.rs`,
  用 `include_bytes!` 绑定 14 个 provider 的 committed golden fixture
  (fixture 改名会变成编译错误而不是静默跳过测试)。四条断言:
  `tool_activity` 双向(声明 Unsupported ⟹ 零活动;声明其他 ⟹ 必须真 emit);
  `source_span` 单向(捕过度声明,低报留给证据驱动升级,遵 RFC-0002 §6);
  覆盖面守护(新增 provider 漏加会失败);以及补充样本登记表的自净测试。
  已修 `aider.tool_activity: Partial → Unsupported`(grep 确认零 `emit_activity`)。
  已自证断言会咬:临时把 `openclaw.tool_activity` 与 `cline.source_span`
  改成过度声明,两条都如期失败,已回滚。
  **副产物发现**:codex 的 golden fixture **零个** `custom_tool_call` /
  `function_call_output` 记录,所以它无法见证自己声明的 pairing 能力 ——
  但 codex 的提取是真实现了的(`emit_paired_activity`/`emit_unpaired_activity`
  + 单元测试覆盖),降级会变成假低报。已用具名补充样本承载证据,
  真正的修法是扩 codex golden fixture(见 M1-5)。
  当前两个测试:漂移测试比对 `capability.rs` ↔ markdown 文档,
  manifest 测试比对 manifest ↔ ledger 列。**没有任何测试比对声明 ↔ 实际行为。**
  这就是 `aider: tool_activity = Partial` 能在全绿 CI 下存活的原因 ——
  **矩阵变成了自我认证的文档而不是证据。**
  加至少一条按能力的行为断言,例如:
  "若 `tool_activity != Unsupported`,解析该 provider 的 golden fixture
  必须 emit ≥1 个 activity";
  "若 `source_span != Unsupported`,必须产出带字节精度的 span";
  "若 `resume != Unsupported`,builder 必须产出命令"(这条已有,
  见 `resume.rs:321-345`,是好的先例)。
  **验收**:每个声明的能力都有一条行为断言守着。

- [ ] **M2P-10 真实机器上首次 discover 三次失败(记录,可能需设计决策)**
  在 owner 真实 home 上实测:第一次 35 秒后退出 5
  `source_changed: len 37466709 -> 37478177`;第二次退出 6
  `catalog_error`(路径其实没问题);第三次又是退出 5。
  **零消息入库,三种不同错误,还在目录里留下 `writer.lock`。**
  根因:agent 正在跑时 transcript 一直在长,快照校验必然失败 ——
  这在真实环境里**无法靠重试赢**。
  `rebuild-and-migration-runbook.md:163-179` 正确解释了这两个码,
  但 README 从不链接它,CLI 输出也不指向它。
  **修法方向**(需决策):允许"读到快照点为止"的部分成功
  (记录已处理到的 offset,下次增量续上),而不是整轮失败;
  或提供 `--allow-growing-sources` 显式接受部分读取。
  与 D11 的宽容精神一致。
  **验收**:agent 运行中跑 discover 能成功入库并正确报告部分性;
  不留 `writer.lock`。

  ⚠️ **2026-08-19 实测:"不留 `writer.lock`"这条验收标准是错的,已撤销。**
  成功运行之后 `writer.lock` **同样**留在 data root 里,而紧接着的第二次
  sync 照常成功(`emitted: 0` 幂等)。原因写在 `lease.rs:3`:
  **lease 的权威是 OS 独占文件句柄,不是文件是否存在** —— 进程崩溃时 OS
  自动释放锁,残留的文件不拦任何人。`Drop for WriterLease` 只 `unlock` 不删文件,
  这是刻意的:Windows 上强制锁期间删除/重开同一路径会 error 33
  (`spikes/data-root-locking/EVIDENCE.md`)。
  → **这条不是缺陷,删掉它反而会引入 Windows 上的竞态。**
  M2P-10 真正要解的只剩 `source_changed` 那一半:agent 正在写时 transcript
  一直在长,快照校验必然失败,重试赢不了。
  **修正后的验收**:agent 运行中跑 discover 能入库并如实报告部分性。
  (`writer.lock` 的存在不作为失败判据。)

- [x] **M2P-11 `index <id-fact> <text>` 开发后门出现在用户 help 里**
  `main.rs:958` 在面向用户的 `--help` 里公布了这个直写入口,而且真的能用 ——
  等于邀请用户破坏自己的真实库。
  **修法**:从用户 help 移除(保留命令供测试用,或加 `--force-dev` 门)。
  **验收**:`--help` 不再公布;误用有防护。

---

### M2C — CI 解锁(D25,**M2 的前置**:阀值要绑 CI 就得先有 CI)

> 根因见 §1.9.1:不是封号,是私有仓库 2,000 min/月 配额被 macOS 的 10 倍
> 倍率烧穿。public 仓库标准 runner 免费无限。

- [ ] **M2C-1 Actions budget 从 $0 调到 $5(兜底,5 分钟)**
  $0 预算 + "stop usage" 是硬停机制,这是**立即解锁**开关,也是 public
  方案万一不生效时的兜底。public 仓库用量走 100% 折扣,不会真扣钱。
  前提:账上有可用支付方式。owner 在中国,如果加不了国际卡这条走不通 ——
  那就只能靠 M2C-2 + 走 GitHub Support 清 billing flag
  (community discussion #184661 的当事人联系 support 后 flag 被清掉)。
  **验收**:budget > $0;推一个一步 workflow 验证能跑起来。

- [ ] **M2C-2 转 public 后验证 CI 解锁(依赖 M5-5)**
  ⚠️ **顺序硬约束:必须先完成公开树导出与隐私核验,再转 public。**
  转 public 是**不可逆的暴露**(fork / 缓存 / 第三方镜像),
  且 `AgentSessions` 私有仓库有 344 个 run 的历史。
  转 public 后先推一个一步 workflow 验证解锁,再谈跑完整 matrix。
  **诚实风险**:少数案例(community discussions #184077、#188932、#180456)
  报告 billing lock 在 public 仓库上也没解开且拖了数月 —— 但那些是另一句
  更硬的报错("account is locked due to a billing issue"),
  owner 拿到的是 spending-limit 那句;#183940 里同样报错的用户把 spending
  limit 从 $0 调到 >$0 后立刻恢复。
  **验收**:public 仓库上一个 workflow 真实跑完并绿。

- [x] **M2C-3 砍 PR 上的 macOS job(即便免费了也该做)** —— **已做(`63583ae`)**
  macOS 占历史消耗 83%,且 Free plan 有 **5 并发 macOS job 上限**。
  当前 `ci.yml` 和 `core-beta-evidence.yml` **各自**都开了三平台/四目标
  matrix,两个 workflow 叠加会顶到并发墙。
  改:macOS 只在 tag / push-to-main 上跑,PR 只跑 Linux + Windows。
  **验收**:PR 触发的 job 数下降;matrix 不撞并发上限。

  **✅ 实测(用脚本对两个分支各自求值,不靠肉眼看 YAML)**:
  PR **10 → 6 个 job,macOS 归零**;push-to-main 仍是完整的 3 + 4。

  **⚠️ 踩坑(差点写出一个看起来对、实际永不生效的条件)**:
  第一版把筛选写在 **job 级 `if:`** 上(`if: ... || !matrix.darwin`)。
  查 GitHub 官方 contexts 文档后确认:**`jobs.<id>.if` 拿不到 `matrix` 上下文**
  (该处只有 `github` / `needs` / `vars` / `inputs`)。
  于是 `matrix.darwin` 求值为空 → 条件恒真 → **macOS 照跑,而 diff 看着像修好了**。
  正解是把选择放在 `strategy.matrix` 里用 `fromJSON(github.event_name == ...)`
  ——`github` 在那里是可用的。
  **教训**:YAML 改完能解析 ≠ 语义正确;条件类改动必须对两个分支分别求值验证。

- [ ] **M2C-4 转 public 后开启 fork PR 审批**
  当前 `default_workflow_permissions: read` /
  `can_approve_pull_request_reviews: false` 已是安全默认,**保持住**。
  另外把 "Approval for running fork pull request workflows" 设为需要审批。
  **验收**:fork PR 不会未经审批就跑 workflow。

- [ ] **M2C-5 四个动作的阀值接进 CI(D3,依赖 M2C-2 与 M2-2)**
  M2-2 产出的 harness 接进 workflow,阀值不过就 fail。
  注意 artifact **只留 7 天**,`ci_verified` 的可复核性天然弱于
  `locally_verified` —— 继续保持
  `core-beta-evidence-matrix.md` 里那个状态词区分。
  **验收**:性能回退能被 CI 自动拦住。

---

### M2 — 性能(D2/D3/D4,四个瓶颈全修)

- [x] **M2-1 合成语料生成器扩到 10 万条(D5)**
  扩展现有生成器,产出 10 万消息 / 5 千 session / 6 provider 的确定性语料。
  必须:固定种子、可复算 SHA-256、CRLF 归一化(已有
  `normalized_tree_hash` 先例)、长度分布与 CJK 比例贴近真实观察。
  **验收**:两次生成哈希一致;可进 CI。

- [x] **M2-2 四个动作的阀值 harness(D3)**
  扩展 `open_source_gate_benchmark.py`,把四个动作都变成带阀值的门:
  | 动作 | 阀值 | 依据 |
  |---|---|---|
  | 首次全量索引 | 1 GB ≤ 5 min,即 ≥ 3.5 MB/s | D4 |
  | 增量同步(无变化) | ≤ 1 s @ 10 万条 | 现在 4000 条要 1.5 s,必须改善 |
  | 搜索 | p95 ≤ 50 ms @ 10 万条 | 交互式"瞬时"感 |
  | MCP 单次调用 | p95 ≤ 50 ms(编码器常驻) | 已实测 21 ms,守住 |
  **验收**:harness 输出每项 threshold + pass/fail;报告 schema 有版本号。

- [x] **M2-3 ingestion 并行化(瓶颈 #2,D20)** —— 已做,但**实测证明 parse 不是瓶颈**(占 1.6%);并行化保留是因为它让 noop sync 963→658 ms(见 §1.3.2)。真正的吞吐瓶颈是 commit 的平方开销,另派 agent 修
  用 `std::thread::scope` + channel(不加正式依赖):解析在多线程,
  SQLite 写入保持单写者(SQLite 的正确做法)。按
  `std::thread::available_parallelism()` 定并发度。
  **验收**:吞吐 ≥ 3.5 MB/s;七条不变量仍全绿;确定性输出不变
  (同一输入产出同一 catalog)。

- [x] **M2-4 修 semantic 全表扇描(瓶颈 #1,D14)**
  `query_semantic` 改为分层检索:先用 FTS 召回候选集,只对候选集算余弦。
  这符合现有架构(hybrid 已用 RRF 融合两路),零新依赖。
  **验收**:10 万条下 semantic p95 ≤ 50 ms;recall 不低于当前
  (与 §1.5 基线对比,不得回退)。
  **已完成**(`1c4d633` `8e17e81` `2adf345`):候选上限取 **2000** 而非计划举例的
  500 —— 重排 2000 条只花 ~24 ms 仍在预算内,降到 500 只省个位数毫秒却把召回
  安全边际砍掉 75%;2000 = 100 × recall@20,且恰好等于冻结语料全量,所以在
  recall 基线上分层路径可证穷尽。实测 p50 895.6→24.6 ms、p95 1111.2→26.6 ms,
  recall 与 §1.5 逐位一致。**候选集过小(<500)时回退全量精确打分**(147 ms,
  结果与旧路径逐位相同):实测 100 条 gold query 字面最多命中 8 条(中位 2),
  不回退就会把 paraphrase 召回抹平成 lexical。裸迭代 10 万行 `message_vec`
  本身就要 ~150 ms,所以有界候选是达标的**必要**条件而非优化选项。

- [x] **M2-5 存储放大治理(瓶颈 #3)** —— **实测后决定不做 external-content**
  原计划:评估 FTS5 改 external-content(`content=` 指向 catalog),消除文本双存。
  **验收允许"明确的不做决定 + 理由",本条走这一条。**

  **2026-08-19 实测(10 万条真库,537.8 MB),推翻了"文本双存是主因"的前提:**

  | 项 | 大小 | 说明 |
  |---|---:|---|
  | `catalog.payload` | 71.5 MB | 权威正文 + JSON 封套 |
  | **`fts_content`** | **26.2 MB** | **文本的第二份拷贝 —— external-content 唯一能省的就是这个** |
  | `fts_data` | 21.4 MB | FTS5 倒排索引,**external-content 一分不省** |
  | `source_membership` | 17.6 MB | |
  | `message_placements` | 14.8 MB | |
  | `fts_ids.id_json` | 13.3 MB | |
  | 逻辑数据合计 | 167.5 MB | |
  | **B-tree / 索引开销** | **277.2 MB** | **占整库 52%,是真正的大头** |
  | freelist(已分配未使用) | 80.1 MB | |

  **三条结论:**
  1. **文本双存只占 26.2 MB / 537.8 MB = 4.9%。** 就算 external-content 完美
     работает,整库也只从 537.8 MB 降到约 511 MB —— 换来的是触发器维护、
     与 `fts_rowid` 边车/rebuild/generation 三套机制对齐、以及 FTS5
     external-content 特有的"content 与索引不同步则查询静默出错"风险。
     **收益 4.9%,风险涉及七条不变量里的多条 —— 不做。**
  2. **真正的大头是 B-tree/索引开销(277 MB, 52%)**,根因是 **wire id 是
     43 字节字符串**(`msg_v1_` + 36 字符 UUID),而它作为主键/外键在
     catalog、fts_ids(两个唯一索引)、message_placements(四列 + 三个索引)、
     source_membership、message_edges 里反复出现。真要降体积,方向是
     **给 wire id 配一个整型 rowid 内部键**,外部 API 仍暴露 wire id ——
     但这会动 RFC-0001 的身份契约,属于 0.3 的架构级改动,不是 0.2 的优化。
  3. **免费的 17%:`VACUUM` 实测把 537.8 MB 降到 444.7 MB**(回收 93 MB
     freelist),零 schema 改动、零风险。已登记为 M2-9。

  **`store_size_ratio` 阀值本身没有被 gate 门住**(它是 informational),
  所以本条不阻塞发布。

- [x] **M2-6 增量同步优化(瓶颈 #4)** —— **已达标**:658 ms vs 1000 ms 阀值,余量从 37 ms 扩到 342 ms(并行 BLAKE3 capture 带来的,见 §1.3.2)
  4000 条无变化重扫 1.5 s,10 万条会线性放大。现有 fingerprint skip 实测
  0.089 s 是**单文件**路径,批量路径没走到。找出 1.5 s 花在哪
  (可能是每次都全量读 catalog 或重建游标)。
  **验收**:10 万条无变化重扫 ≤ 1 s。

- [x] **M2-7 首次索引进度反馈**
  即使压到 5 分钟,5 分钟无输出也是坏体验。加实时进度
  (已处理文件数/总数、当前 provider、已入库消息数),支持 Ctrl-C
  中断后可继续(增量语义已有)。
  **验收**:进度可见;中断后重跑不重复入库。

- [x] **M2-8 真实冻结快照回归(G5)**
  在 M1-2 冻结的快照上跑 `real_data_regression.py`,七条不变量全绿,
  记录聚合数字(D5:只提交聚合)。与 §1.6 的历史数字对比确认无回退。
  **验收**:harness 退出 0;`docs/evidence/` 记录聚合结果。

  **⚠️ 2026-08-19 我在 10 万条合成语料上试跑,发现 harness 自己有个缺陷 ——
  它会静默漏掉四分之一的语料,先修它再谈 G5。**
  试跑结果(`--sources evidence-output/synthetic-corpus-100k/corpus`,290 s):
  七条不变量**全 PASS**、harness 退出 0,数字自洽
  (3,675 源 / 73,504 emitted / 0 skipped / 73,504 persisted / 3,675 session /
  42,569 span 全字节精度 / rebuild 80,854→80,854 ids match)。
  **但语料实际是 5,000 个文件 / 10 万条消息。** 按扩展名分:
  `.jsonl` 3,675、`.json` 875、`.md` 450 —— **harness 只收了 `.jsonl`**
  (`collect_sources()` 在 `real_data_regression.py:170-185`,三处硬编码
  `endswith(".jsonl")`),于是 **26.5% 的语料被静默排除,而报告仍宣称全绿**。
  这与 CLI 刚修掉的 M2P-7 是**同一类缺陷**:工具侧已改为按 provider probe
  判定,harness 侧还在按扩展名过滤。危险在于它不报错也不计数 ——
  "七条全绿"读起来像覆盖了全部语料。
  **修法**:`collect_sources` 改为收全部候选文件并交给 probe 判定(或至少
  把跳过的非 `.jsonl` 文件计数上报,与 D11 一致);`--sources` 的 help 与
  错误消息里的 "`.jsonl` file" 措辞同步改掉。
  **修完再跑,并把「源文件数 = 语料文件数」作为新的前置断言** ——
  否则下一次静默漏文件仍然抓不到。
  **G5 本身仍依赖 M1-2**(真实冻结快照尚未建立);上述合成语料试跑
  不能替代它,只是提前暴露了 harness 缺陷。

- [x] **M2-9 回收 freelist(实测免费的 17%,由 M2-5 的测量发现)** —— **已实现** index compact(af30de)。1,500 源库实测 180.1 → 148.1 MB,回收 31.9 MB(17.7%),search 命中数与 generation 前后一致。刻意做成显式维护命令而非 sync 尾步(VACUUM 整库重写 + 排他锁 + 需等量临时空间);报 before/after/reclaimed 三个字节数而不报比例 —— 回收量取决于删除历史,比例会被读成保证
  10 万条真库实测:537.8 MB 里有 **80.1 MB 是 freelist** —— 已向 OS 申请
  但当前未使用的页。`VACUUM` 实测把库从 **537.8 MB 降到 444.7 MB**
  (回收 93 MB,17.3%),**零 schema 改动、零查询语义变化**。
  这是整个 M2 里性价比最高的一项:比 external-content 省的 26.2 MB 多三倍,
  而风险几乎为零。
  **注意几件事,不要无脑加 `VACUUM`**:
  - `VACUUM` 需要约等于库大小的临时空间,且**整库重写**,10 万条上耗时可观 ——
    不能放在每次 sync 之后。
  - 它拿排他锁,与 writer lease 语义要对齐(lease 已保证单写者,但 VACUUM
    期间读也会被阻塞)。
  - `PRAGMA auto_vacuum=INCREMENTAL` + 定期 `incremental_vacuum` 是更平滑的
    替代,但 auto_vacuum **只能在建库时或 VACUUM 时设定**,存量库需要迁移。
  **修法方向**:给 `index` 加一条显式的 `index compact`(或 `vacuum`)维护
  子命令,由用户/定时任务决定何时付这个代价,而不是隐式塞进 sync;
  文档里写明"何时该跑、要多少临时空间"。
  **验收**:有显式命令可回收 freelist;实测回收量与耗时记录在案;
  七条不变量在 compact 后全绿(尤其 `INV-REBUILD-STABLE` 与
  `INV-SOURCES-UNCHANGED`)。

---

### M3 — 功能补齐(D15 全部补齐,D12 数据生命周期)

- [x] **M3-1 `--db` 默认路径(D8)** —— **实测已实现,本条记录已过时(2026-08-19 核实)**
  默认落到平台数据目录(`asg config paths` 已能算出),`--db` 降为覆盖项。
  严格对齐 ripgrep/gh/atuin 的零配置习惯:装完就能用,不需要先读文档。
  旧命令(显式 `--db`)必须全兼容。**与 M2P-3 是同一件事,合并做。**
  **验收**:全新环境 `asg search "x"` 无参数可跑;所有现存测试仍绿。

  **✅ 已满足,无需改动。** `resolve_store_path`(`main.rs:1600-1618`)的
  优先级是 `--db` → `ASG_DB` 环境变量 → 平台数据目录下的默认库,
  三态由 `DbOrigin` 区分(提示里是否带 `--db` 前缀就靠它)。
  **实测(release 二进制,伪 HOME + 伪 LOCALAPPDATA,全程不传 `--db`)**:
  `sync --discover` 入库 3 条、`search "hello"` 正常出结果、
  `config paths` 打印出默认 data/cache/logs 位置。
  → **M4-1 的"依赖 M3-1"前提已不存在**,quickstart 可以直接写零配置三步。
  ⚠️ 注意 `config paths` 的 `config` 一项仍指向真实 `APPDATA`
  (只有 data/cache/logs 跟随 `LOCALAPPDATA`)—— 这是 Windows 目录语义,
  不是缺陷,但写文档时不要声称"所有路径都随 HOME 重定向"。

- [x] **M3-2 数据生命周期三维度(D12)** —— **四个维度已交付,第五个未做(merge `1e357d4`)**
  - [x] `asg prune --before <date>` — 按时间
  - [x] `asg forget --project <path>` — 按项目(离职/交接/敏感客户项目整体抹除)
  - [x] `asg prune --provider <id>` — 按 provider
  - [x] `asg forget <session-id>` — 单会话(粒度最细,删误入的敏感会话)
  - [ ] `asg uninstall --purge` — 连数据一起删 —— **未做**:二进制里没有
        `uninstall` 子命令,卸载由 `scripts/install/uninstall.*` 承担且按设计
        不碰 data root。为复制安装器职责新增一个破坏性子命令是独立任务。
  全部默认 dry-run 打印将删除什么,`--yes` 才执行。删除必须同时清
  catalog、fts、fts_ids 边车、message_vec、tool_activities 等所有表。
  **验收**:每个维度有测试;删除后 `doctor` 报告一致;rebuild 后不复活。

  **⚠️ 上面这句枚举读起来完整,实际漏了两处**(详见 M3-5 的字节级审计):
  漏了 **FTS5 的 segment 残留**(delete marker 不重写 segment,
  `VACUUM` 与 `optimize` **各自单独都不够**)与
  **`index_batches` outbox 日志**(逐字保留了 `forget --project` 想抹掉的路径)。
  → **教训**:"要清哪些表"的清单不能靠读 schema 列举,
  必须**直接数原始文件字节**才知道有没有残留。
  额外发现 `message_vec` 在**既有** tombstone 路径上就没被清理 ——
  不是新命令引入的缺陷,而是这次审计才暴露出来的老问题。


- [ ] **M3-3 搜索作用域:全库默认 + 同项目加权 + 每条显示来源(D26)**
  调研已闭环,结论明确,见 §2.3。落一份 ADR 再实现。
  **具体形状**:
  - 默认作用域:**全库**。不加新的默认过滤,**不做 `--all` 逃生舱**。
  - 新增 `--project <path-or-name>` 显式收窄(`SearchFilters` 加一个维度,
    对解析出的工作目录做前缀/子串匹配,对齐 ctx 的 `--workspace` 与
    sessiongrep 的 `--path`)。同时加 `--exclude-project`(agentsview 有,
    用户会要)。
  - 加**加性**同项目加权:cwd → git toplevel 解析,**量级要明显低于
    精确文本相关性**。做成 config key 默认开(照 sessiongrep 的
    `prefer_current_repo`),工作目录无法解析时自动变成 no-op。
    CLI 与 **MCP 两条路径都要应用** —— agent 问"这个项目"时收益最大。
    ⚠️ **不要用乘性加权** —— 会压过文本相关性。
  - 每条结果在 **human 与 robot 两侧**都显示来源项目,并在 header 里
    写明生效的作用域(例如 `scope: global (this project boosted)`)。
  - **robot/MCP 侧的 project 字段没做好之前,不要上线加权** ——
    否则会精确复刻 agf 的 bug。
  - 暂不做循环过滤模式 UI:atuin 的 ctrl-r 循环之所以成立,是因为它是
    常开的交互式 TUI 且有常驻单字符指示器;一次性 CLI 用
    `--project` + 可见的 scope 行就能拿到同样收益,成本低得多。
  **验收**:ADR 落地;实现与 ADR 一致;human 与 robot 都显示来源项目;
  加权在工作目录未解析时无副作用。

- [x] **M3-4 recency 浏览:五个入口都答不了"我昨天干了什么"(最高优先)** —— **已交付**
  全仓库只有两种排序:`SORT_WIRE_ID_ASC` 与 `SORT_SCORE_DESC`
  (`application/src/lib.rs:37-40`)。`list` 是 `ORDER BY id ASC` 对一个
  **BLAKE3 摘要**排序(`adapters-sqlite/src/lib.rs:2065-2068`)——
  等于随机顺序;而且它把 message/session/document 混在一起
  (`main.rs:2294` 传 `sessions_only: false`,CLI 没有 `--sessions` flag,
  尽管 MCP 的 `list_sessions` 有)。没有 `--sort recency`、没有
  `sessions --recent`、没有"最近 10 个会话"。TUI 开局是空查询框且空输入
  回车是 no-op(`tui/core.rs:304-307`);Web UI 显示
  "Type a query to inspect the catalog"(`web/index.html:112`)。
  → **用户做的第一件事("我昨天/上周干了什么")在五个入口全都做不到。**
  这是"搜索引擎"与"可用的历史工具"之间的差别。
  **修法便宜**:按已存在的时间戳投影排序
  (~~`adapters-sqlite/src/lib.rs:6808`~~ **实际 `:7661`**,
  已有 `latest_activity_ymd_for_sessions`)。
  依赖 M2P-12(时间戳提取)才能覆盖全 provider。
  **任何文档都没承认这个缺口。**
  **验收**:五个入口都能无查询词列出最近会话。

  **✅ 已交付(merge `a334345` + 后续)**:新增 `ListSort::{WireIdAsc, RecencyDesc}`
  与 `SORT_RECENCY_DESC`;摘要序仍是默认且处处可达(`--sort id`)。
  端口新增 `CatalogStore::sessions_by_recency` 只返回 id + 排序键,
  页内 payload 由 application 一次批量 `get_many` 取 —— 无 N+1。
  排序键 `latest_activity` 在所有机器面可见并计入字节预算。

  **五个入口全部实测通过**,且 `compare_entrypoints.py` 新增 `recent_sessions`
  跨 cli/mcp/robot/web/tui 比对:`overall_verdict: consistent`,零分歧。

  **两处原记录不准(已核实)**:
  1. **"修法便宜:按已存在的投影排序"低估了。**
     `latest_activity_ymd_for_sessions` **不能复用**:它需要先给出会话集合
     (而 recency 必须排完全表才知道 top N),且它截断到 `YYYY-MM-DD` ——
     太粗无法用于排序,其文档也明写"仅供 human 展示"。必须另写一个聚合。
  2. **"依赖 M2P-12"对正确性不成立。** 无时间戳的 provider 今天就被诚实处理:
     排在末尾、`latest_activity: null`、human/TUI/Web 显示 `—`。
     M2P-12 只影响**有多少会话能进入有日期那一组**。

  **一个关键实现细节值得留意**:`ORDER BY latest IS NULL ASC` 是刻意的 ——
  裸 `latest DESC` 在 SQLite 里会把 NULL 排**最前**,
  于是**没有时间戳的会话会冒充成最新的**。实测确认无日期会话排末尾。
  并列与整个无日期组都按 wire id 决胜,所以顺序是全序,
  游标 offset 在每一页含义一致。

  **游标绑定已实测**:recency 下发的游标拿到 `--sort id` 重放 →
  exit 2 `cursor_invalid`("cursor was issued for a different sort order");
  同一游标在 `--sort recency` 下正常续读第二页。
  `ListSort::digest()` 进入令牌,与 `(generation, query_digest, result_set)` 并列。

  **⚠️ 诚实的代价(已记入 adapter spec,不要当免费)**:一页 recency 的开销与
  **placement 表规模**成正比而非页大小。11 万实体库实测每进程
  **约 0.49–0.54 s**,而同一页 wire-id 序约 0.018–0.022 s。
  交互可用,但便宜的正解是**物化一个 per-session `latest_activity` 列**,
  不是把查询写得更宽。
  另:会话标题仍是 `—` —— 需要"本会话首条用户消息"投影,现无端口暴露;
  宁可留空也不臆造(工作目录列承担"哪个项目"的信号)。


- [x] **M3-5 删除/保留:目前完全没有删除路径(隐私义务,非选配)** —— **已实现(merge `1e357d4`)**
  唯一的清理命令是 `index purge-activities`,而它**只删孤儿** tool-activity 行
  (`main.rs:1589-1609`)。**没有** `forget <session>`、
  **没有** `prune --older-than`、~~**没有** `vacuum`~~(**记录过时**:
  `index compact` 早在 M2-9 就已实现)、**没有**保留配置。
  catalog 行只能靠 tombstone 消失,而 tombstone 要求源文件消失**并且**
  跑一次完整的 `sync --discover`(`main.rs:3146-3176`)。
  → 索引了一个后来删掉的仓库、或误入了一份私密 transcript,
  **没有任何受支持的方式移除它**。
  `INSTALL-AND-UPGRADE.md:155-156` 让用户手动删整个 data root。

  **✅ 已交付**:`forget <ses-id>` / `forget --project <路径>` /
  `forget --list|--readmit` / `prune --before <日期>` / `prune --provider <id>`,
  全部**默认 dry-run**,`--yes` 才执行。schema 升到 **v13**(新增
  `forgotten_sources` 抑制表)。

  **⚠️ 字节级审计揪出三个真实泄漏(都已修),其中两个不在原记录的枚举里**:
  1. **`message_vec` 从来没在 tombstone 时清理** —— 这是**既有删除路径就有的
     缺陷**,不只是新命令的问题:`query_semantic` 只读 `message_vec` 自己的列,
     所以它仍会给已删除消息打分并返回其 wire id。
  2. **`index compact` 原本不足以让删除内容不可读** —— FTS5 删除只写
     delete marker,不重写 segment。实测:单跑 `VACUUM` 仍留 4 处、
     单跑 FTS5 `optimize` 也仍留 4 处,**只有两者都做**才归零。
     现在 `compact` = `optimize` → `VACUUM` → `wal_checkpoint(TRUNCATE)`。
  3. **outbox 日志 `index_batches.source_replacements_json` 逐字保留了项目路径**
     (ADR-0009 resume claim 里的 `original_working_directory`),
     于是 `forget --project <敏感客户>` 之后那个客户的目录名还在库里。

  **我自己独立复验(release 二进制,伪 HOME,直接数 `.db` 原始字节)**:
  ```
  forget 前:            5 处
  forget 后 / compact 前: 7 处   ← 反而变多(delete marker)
  compact 后:            0 处
  ```
  **这个 5→7→0 正是问题与修法的证据**:光 `forget` 不但没抹掉,计数还上升;
  必须跟一次 `index compact`。所以 dry-run 的警告里明写了这一步 —— 用户不跑
  compact 就以为删干净了,是本条最容易踩的坑。
  另外复验:`search` 归零、源 transcript 逐字保留、
  `sync --discover` **不复活**(报 `forgotten list` 跳过并给出 `--readmit` 出路)。

  **未做(如实记录,不当已完成)**:M3-2 列的 `asg uninstall --purge` **没实现** ——
  二进制里根本没有 `uninstall` 子命令(卸载是 `scripts/install/uninstall.*`,
  且按设计不碰 data root)。为复制安装器职责而新增一个破坏性子命令是另一件事,
  宁可标注也不塞 stub。
  **`forget --project` 的适用面比名字听起来窄**:项目归属只能来自
  `resolved` 状态的 Original Working Directory claim;`missing`/`ambiguous`
  的源没有可信项目,被排除并在 `warnings` 里报数 —— 不猜。

  对一个 local-first、以隐私为卖点的工具,"你索引了不该索引的东西"
  必须有比"手动删掉整个数据目录"更好的答案。
  **这是 D12 的落点,与 M3-2 是同一件事** —— M3-2 写了命令形状,
  这里记录为什么它是 must-fix 而不是 nice-to-have。
  文档只承认了 D9("ToolActivity retention/cleanup"),
  **更大的缺口(catalog 层完全无法删除)哪里都没承认。**
  **验收**:见 M3-2。

- [ ] **M3-6 `config.toml` 被宣传但从不被读取(与 M2P-3 同源)**
  `config paths` 在四个平台都报告 `config.toml` 路径
  (`main.rs:871,887,911,927`),但**依赖图里没有任何 TOML 解析器**
  (`Cargo.lock` 里零个 `toml` 条目),也没有任何代码读那个文件。
  `HookConfig` 被记为"stored in config"(`hooks.rs:26-29`)
  但实际只能靠 flag(`main.rs:2184-2197`)。
  → **本项目没有任何形式的持久化配置**:存不了默认 db、
  存不了默认 provider 过滤、存不了 hook 开关、存不了脱敏偏好。
  **修法**:要么真实现配置读取(需加 TOML 依赖,或用更简单的 KV 格式
  避免加依赖),要么停止打印这个路径。
  **验收**:`config paths` 不撒谎;若实现则至少默认 db 与 hook 开关可持久化。

- [ ] **M3-7 无导出、无非 JSON 输出格式**
  只有 `--output human|json|jsonl`(`main.rs:1014`)。
  没有 `--format markdown`、没有剪贴板、没有输出到文件。
  handoff pack 要么是 JSON(`main.rs:1987`)要么是有损的 human 摘要
  (`human.rs:185-298`);想把一个会话粘给另一个 agent,得自己后处理 JSON。
  `SKILL.md:104` 还明确告诉消费者永远不要解析 human 模式。
  **修法**:加 `--format markdown`(handoff 与 context 最需要)。
  **验收**:handoff 与 context 能直接产出可粘贴的 markdown。

- [ ] **M3-8 搜索缺 role / 路径 / 排除项过滤**
  `safe_fts_query` 给每个 token 加引号(`adapters-sqlite/src/lib.rs:6885-6904`),
  ADR-0003 刻意移除了 phrase/boolean/`NEAR`/prefix 语法(`ADR-0003:15,29-31`)——
  **这个取舍对首发是站得住的,不要推翻**。
  但 ADR 承认的是语法移除,**没有承认**缺失的是:
  `role:` 限定(现在只有二元的 `--include-system`)、文件/路径搜索、
  `-exclude` 排除项。**没有办法只搜 assistant 消息。**
  **修法**:`role` 限定优先(最常用),`--exclude` 次之;
  不要重新引入 ADR-0003 撤掉的语法。
  **验收**:能只搜某个 role;能排除词。

- [ ] **M3-9 命中处无一步到位的上下文窗口**
  `search` 返回有界前缀片段(`application/src/lib.rs:887-889`,
  human 预览截断到 120 字符,`human.rs:18`),**无匹配高亮、无周边行**。
  想看匹配在上下文里的样子要第二条命令(`show`)和第三条
  (`get-message --around` / `context`)。
  `why_matched` 列出哪些词匹配了但不说匹配在哪
  (`application/src/lib.rs:896-902`)。
  **修法**:加匹配高亮 + 可选的周边行(`--context N`)。
  **验收**:一条命令能看到命中在上下文中的位置。

- [ ] **M3-10 无统计:用户答不出"我到底有多少历史、来自哪里"**
  `status` 只有四个计数器(`main.rs:4109-4114`)。
  没有按 provider 计数、没有日期直方图、没有 top 工具、没有会话大小分布。
  `doctor` 只查健康(`main.rs:1305-1318`)。
  **修法**:`status --detail` 或 `stats` 子命令:按 provider / 按月 / 按项目。
  **验收**:一条命令能看到历史的构成。

- [ ] **M3-11 增量更新的人机工程(MCP 侧最痛)**
  `sync` 全手动 —— 无 watcher、无 daemon、无搜索时自动同步、无 `notify`。
  **MCP 是只读的且没有 sync/refresh 工具**(`mcp.rs:343-366`),
  所以 MCP 客户端的索引会一直过期,直到人在另一个终端手动跑 `sync`。
  `sync --discover` 只覆盖 14 个里的 6 个且只认 `.jsonl`;
  显式 `sync <file>` 要求用户知道每个 provider 的磁盘布局,
  而且拒绝目录并提示"在你的 shell 里展开文件列表"
  (`main.rs:3672-3677`)—— **在 Windows 上这相当不友好**。
  写者持排他 lease(`main.rs:430-439`),所以长时间 `sync` 会阻塞其他写入。
  **修法**:MCP 加一个 refresh/sync 工具(最高价值,一行改动级别的收益);
  `sync` 接受目录(Windows 友好);discover 覆盖非 jsonl(见 M2P-7)。
  **验收**:MCP 客户端能自己触发刷新;`sync <dir>` 可用。

- [x] **M3-12 `--offline` 目前是稳定的 no-op** —— **已修文档(`bfe4bc9`)**
  `const NETWORK_REQUIRING_SUBCOMMANDS: &[&str] = &[]`(**实际在 `main.rs:1744`**)——
  注册表是空的,所以 `--offline` 今天什么也不 fail-close。
  这不是缺陷(默认构建本来就零出网),但**文档把它说成一个生效的门**。
  **修法**:要么把真正需要网络的路径注册进去(目前只有 `model import`
  的下载路径,而那个路径不存在),要么把它文档化为
  "未来网络能力的前置门,当前默认构建无网络能力所以无操作"。
  **验收**:`--offline` 的语义与实现一致。

  **✅ 选后者(改文档,不动代码)。** 代码里的注释本来就是诚实的
  (`main.rs:1740-1743` 明写"当前没有任何子命令需要联网,列表为空"),
  **偏差只在对外文档**:
  - `README.md:127-131` 与 `SECURITY.md:30` 都写成
    "refuses / rejects **any** network-requiring capability",单独读就是一个
    生效的门 —— 用户去验证会发现什么都没拦。已改成"standing fail-closed
    gate for any **future** capability;当前不改变任何行为"。
  - `docs/release/rehearsal-runbook.md:300` 让操作员"验证零出网**除显式模型
    下载外**" —— 默认构建里没有任何下载路径,**这条例外恰好会放过抓包想抓的
    那一个发现**。已删除该例外。
  - `CHANGELOG.md:54` 与 runbook 开头本来就写的是 "future network-dependent
    capability",无需改。
  - `docs/reference/cli.md` 与 `skills/.../SKILL.md` 的措辞也已经是诚实的。

  **实测证据**(release 二进制):`doctor --offline` 的 `offline` 字段为 `true`,
  而 `--offline` 下 `sync` 与 `search` 全部正常完成 —— 这正是"当前什么也不拦"
  的证据。`cargo tree -p agent-session-grep-cli -e normal` 无
  reqwest/hyper/ureq/curl/isahc/surf。

- [x] **M3-13 死代码清理:`activity.rs` 整个文件无生产调用者** —— **已删(`1a05104`)**
  `application/src/activity.rs`(14.9 KB)的 `extract_activities`、
  `filter_by_kind`、`filter_by_status`、`file_targets`、`command_targets`
  **零生产调用者**(全仓库搜索确认);实际走的是
  `ports::infer_tool_activity_kind`(**实际在 `ports/src/lib.rs:492`,
  原记录的 440-449 已过时**)。
  **修法**:删除,或接进生产路径(如果它确实比现用的实现更好)。
  **验收**:无死代码;若保留则有调用者。

  **✅ 选择删除,理由是两者在原则上冲突**(不是"没人用所以删"这么简单):
  生产路径 `ports::build_tool_activity` 是**刻意 fail-closed** 的 ——
  未知工具名 → `kind = Unknown` **且** `target = None`,且**从不推断 status**
  (`ports/src/lib.rs:505-529`)。
  而这个死模块**处处在猜**:工具名缺失时默认成 `"unknown"`
  (`:119`)、无法识别的一律归为 `ToolCall`(`:203`)、
  **靠在结果前 200 字符里子串匹配 "error"/"failed"/"exception" 来判定失败**
  (`:225`)。把它接进生产等于把本仓库明确拒绝做的推断给发出去。
  → **删除是唯一与既有原则一致的选项。**
  删掉后 workspace 编译与测试全绿(这正是"确实无人可达"的证据);
  测试数 198 → 187,少掉的 11 条全是该模块自己的测试。

- [x] **M3-14 handoff pack 的两个字段永远是空** —— **已修(merge `worktree-agent-a2f5cfd37e9528719`)**
  `provenance: None` 恒真(`handoff_pack.rs:236`),
  `inference: Vec::new()` 恒真(`:233`)。
  而 handoff-pack/v1 的卖点之一就是 **evidence/inference 分栏**。
  同类:`metadata_provider_permission_hint` 对每个 provider 都返回 `None`
  (`resume.rs:151-153`),CLI 无条件报 `permission_mode_verified: false`
  (`main.rs:2099`);`ContextGraphStore::tool_activities_for_messages`
  的默认实现返回空(`ports/src/lib.rs:190-193`)。
  **修法**:要么填充这些字段,要么从 schema 与文档里移除承诺。
  **验收**:schema 承诺的字段都有真实内容,或承诺被撤回。

  **✅ 两类分别处理,不是一刀切"都填上"**:
  - **`provenance` 与 `MatchedSession.provider_id`:填充。**
    原注释说"搜索型 pack 无单一会话/提供商,honest:不臆造" ——
    **这个理由只在命中多个会话时成立**。恰好命中一个会话时,provider 与
    session 都是已持有的事实,`None` 是**少报**而不是诚实。
    provider 取自 store 的 resume claim(`source_session_resume_claims`);
    没有 claim 时省略 provider、仍报 session —— 报事实,不猜。
    **实测(release 二进制,零配置)**:单会话查询得到
    `{"provider_id":"openclaw","session_id":"ses_v1_..."}`,此前恒为 `null`。
  - **`inference: Vec::new()`:保留,改文档。** `generation_mode` 是
    `Deterministic`(不调用任何模型),所以**确实没有任何可推断内容**,
    空列表是唯一诚实取值,不是"待填充"。缺陷在**承诺的措辞**:
    schema 与类型文档现在明写"Deterministic 下恒为空;非空只可能来自
    尚未提供的 local-llm 生成器"。`InferenceEntry` 作为契约占位保留,
    保证真接上本地模型时 evidence/inference 分栏在类型层面已强制成立。
  - `tool_activity` 同样补了文档:v12 之前建的库或本就无工具调用的会话为空。
  **schema 未升版**:改动全是新增可选字段与文档,`HandoffPack::SCHEMA_VERSION`
  无需移动;pack 的字节可复现性(`derive_pack_id`)未受影响。

- [x] **M3-15 D13 运行时守卫:data root 与 source root 重叠无检查** —— **已实现(`76260de`)**
  `THREAT-MODEL.md:49` 自己写明:"尚未实现运行时代码守卫",
  只靠文档和安装器默认值缓解。
  → 一个配错的 `--db` 落在 `~/.claude/projects` 里,
  **会把本工具自己的写入放进它承诺永不触碰的目录**。
  小、本地、且保护的是整个产品赖以成立的那条不变量
  (provider 源只读)。
  **修法**:open 时检查 data/cache/log 路径与任一已注册 source root 的
  包含关系,重叠则拒绝并给出改法。
  **验收**:重叠时 fail-closed;有测试。

  **✅ 已落在 `resolve_store_path` 之后(`reject_store_inside_source_root`)**,
  所以**每个命令都过这道门**,不是只 sync。
  比危害描述更进一步的一点:那个目录随后**会被 `sync --discover` 扫描**,
  于是工具开始把自己的 SQLite/WAL 当成待索引的源 —— 不只是"写进了不该写的
  目录",而是会自我污染索引。错误消息里明说了这一点。
  **设计选择(已写进代码注释)**:判定是**纯词法**前缀包含,**不碰文件系统** ——
  守卫必须在库还不存在时就生效(写路径随后会 `create_dir_all`),
  所以不能依赖 `canonicalize`。代价:symlink 绕过不在本守卫覆盖内
  (「默认不跟随 symlink」是另一条独立控制项,不重复实现)。
  只在**严格位于** root 内部时拒绝:root 的祖先合法(否则默认库落在 home 下
  会被误拒)、`.claude` 本身合法(root 是 `.claude/projects`)、
  `projects-backup` 不算 `projects` 的内部路径。
  **实测(release 二进制,伪 HOME)**:库指向 `.claude/projects/index.db` →
  `invalid_request` 并点名 provider 与 root,**且被拒的那次运行在受保护目录里
  没留下任何文件**;库指向 home 下其它路径 → 正常同步。
  Windows 反斜杠写法与盘符小写写法都有回归断言。
  → **`THREAT-MODEL.md:49` 那句"尚未实现"现在可以改了**(留给 M4-8 统一清理)。

- [ ] **M3-16 `ses_v2_` 身份迁移里的 Windows 路径大小写缺陷(潜在,Windows 是主目标)**
  `domain/src/ids.rs:624-629` 记录了一个**已知活跃缺陷**:
  生产的 `installation_namespace` 缺少 Windows 路径大小写归一化。
  → 同一个安装的会话身份会**按盘符大小写静默分裂**。
  D10(provider-scoped identity,schema v13)整体是正确 deferred 的,
  但这条缺陷因为 Windows 是主平台,值得单独提前修。
  **修法**:归一化 Windows 路径大小写;加回归测试。
  **验收**:同一路径不同大小写产出同一 namespace。

- [ ] **M3-17 16 provider 的成本在哪里:渐进式披露该用在输出层,不是加载层
  (2026-08-19 owner 提问 + 我实测)**

  **owner 的顾虑**:用户通常只装 1–3 个 agent,全量加载 16 个 provider
  是否白占性能、是否该做渐进式披露?

  **实测结论:加载层不是成本所在,输出层才是。** 顾虑的方向对,落点要改。

  **测量(release 二进制,伪 HOME,同机连测)**:

  | 场景 | 耗时 |
  |---|---:|
  | `status`(不触发 discovery)基线 | 11–17 ms |
  | 一个 provider 都没装,`sync --discover` 扫 16 个空 root | **13–15 ms**(暖态) |
  | 200 个 claude 文件(registry 第 1 位) | 218 ms |
  | 200 个 antigravity 文件(第 12 位,前 11 个 probe 必须先失败) | 271 ms |

  三点解读:
  1. **空 root 的扫描开销约 2 ms**,即"装了 16 个但只用 1 个"几乎不花钱 ——
     未注册 discovery root 的 provider 连 `stat` 都不做,注册了的只 `is_dir()`
     一次就跳过。
  2. **probe 顺序的代价是 53 ms / 200 文件 ≈ 0.27 ms/文件**,即最坏位置的
     provider 每个文件多付 11 次失败 probe。真实语料(1,242 文件)约多 0.33 s ——
     可优化但不是体验瓶颈。
  3. **adapter 是无状态的**:`provider_registry()` 只 `Box::new` 14 个空结构体,
     没有 I/O、没有配置读取、没有索引预热。"加载 16 个 provider"在本设计里
     **几乎不是一个开销**。而且 `search`/`list`/`context`/`handoff`
     **根本不构造 registry**(只有 sync/ingest 路径构造,实测确认),
     所以日常检索完全不受 provider 数量影响。

  **⚠️ 真正的成本在输出**:一个只装了 claude 的用户跑 `sync --discover`,
  会看到 **14 行 provider 状态**,其中 **13 行是"你没装这个"的噪声**;
  `providers` 命令输出 33 行。**这才是 16 provider 真正伤到用户的地方,
  而它是纯呈现问题,不是架构问题。**

  → **渐进式披露应落在这里**(默认摘要,`--all`/`--verbose` 展开全量):
  - `sync --discover` 人类输出:默认只列**有发现的** provider + 一行
    "另有 N 个 provider 未检测到数据(`--all` 查看)"。
    `--robot` JSON **必须保持全量**(机器消费者需要完整矩阵,
    且契约禁止 CLI/机器面分叉)。
  - `providers` 默认只列**本机检测到**的,`--all` 列全部 16 行。
  - **不要**做"按配置只加载部分 adapter":收益约 2 ms,代价是
    (a) 用户换 agent 后静默不索引 —— 正是 M2P-16 刚修掉的那类静默跳过;
    (b) 归属需要全量 probe 才能拒绝歧义(pi/openclaw 同形),
    砍掉候选集会让"宁拒不猜"退化成"猜先加载的那个"。
  - 若要优化 probe 顺序,正确做法是**用 discover 的 root 提示排序候选**
    (该提示已存在,见 M2P-16),让来自 `.gemini/...` 的文件先问 antigravity ——
    这是**排序**优化,不改变候选完整性,因此不损失歧义拒绝能力。
  **验收**:人类输出默认只显示相关 provider;`--robot` 全量不变;
  `--all` 可展开;probe 候选集**不因配置而减少**。

---

### M4 — 文档与 UX(G11/G12/G13)

- [ ] **M4-1 quickstart 可照抄执行**
  按 M3-1 的零配置改造重写:装完 → `asg sync --discover` → `asg search "x"`,
  三步能跑通,无未声明前置条件。每步给真实输出示例。
  **验收**:在全新环境照抄执行成功。

- [ ] **M4-2 每 provider 的"你的记录在哪 / 我们解析不了什么"表**
  逐 provider:transcript 默认路径(三平台)、格式、已验证到什么程度、
  `known_limitations`、resume 是否可用。这是 local-first 工具最该有的表。
  **验收**:表与 `capability.rs` 一致(考虑加漂移测试)。

- [ ] **M4-3 故障排查手册**
  新建 `docs/operations/TROUBLESHOOTING.md` 并从 README 链接。
  **按用户真实看到的字符串编排索引**(实测得来的原文):
  `需要数据库参数 --db`、`数据库内部错误 (exit 6)`、
  `source_changed (exit 5)`(含"停掉正在跑的 agent 或接受部分运行")、
  `no provider recognized this source`、`not a valid session id`、
  `no hits on a fresh store`、`writer_busy`、`schema_incompatible`、
  `ambiguous provider selection`。从它链到
  `rebuild-and-migration-runbook.md`(那里已正确解释了
  `writer_busy`/`source_changed`/游标错误,但 README 从不链接它)。
  当前全仓库 grep `troubleshoot|常见问题|FAQ` **零命中**。
  **验收**:每个错误消息在手册里能找到对应条目。

- [ ] **M4-4 MCP 客户端接入实例**
  仓库里**唯一**一份 MCP config JSON 在
  `skills/agent-session-grep/SKILL.md:126-135` —— 而这个文件既没被 README
  链接也没被 CONTRIBUTING 链接,躺在 MCP 用户永远不会看的 `skills/` 下。
  `docs/` 里 `mcpServers` 零命中。`README.md:7-8` 把 MCP 列为头号入口,
  之后再也不提。
  要给:Claude Code(`claude mcp add` 命令行)、Claude Desktop
  (`claude_desktop_config.json` 的具体路径)、Cursor/Cline/Zed 各自配法;
  9 个工具逐个说明;一对真实的 `tools/call` 请求/响应;
  明确写出 **`--db` 必须已存在**(`asg mcp` 无 `--db` 直接报错退出);
  以及**必须定期重跑 `sync --discover` 保持索引新鲜**(MCP server 是只读的,
  没有 `asg watch`,索引不更新 agent 就在搜过期快照 —— 现在没有任何文档提这件事)。
  从 README 链接 `SKILL.md`(它是仓库里最好的集成文档,12 KB,却无人能找到)。
  **验收**:照抄 config 能接上;每个工具有示例;新鲜度问题被说明。

- [ ] **M4-5 完整 CLI reference**
  每个子命令、每个 flag、退出码表(14 码 error catalog 对齐)、
  Robot JSON schema 指引。
  已知 help 里**完全没出现**的真实 flag:`--mode`(只在 search 子命令 help 里)、
  `--no-color`(`main.rs:1345` 真的接受,任何地方都没文档)、`--dir`、
  `--snapshot-json`、`--lan`、`--decay-days`、`--max-evidence`、
  `--yes`(只在 resume 那行内联提到)。
  退出码表(`main.rs:1027-1028`)只列了 0 和 10 就说"其余见 error catalog",
  而那个文件(`schemas/robot/v1/error-catalog.json`)help 里从没点名、
  README 也没链接 —— 撞上退出 6 或 9 的用户无路可循。
  **验收**:与 `--help` 和 schema 一致;退出码表完整。

- [ ] **M4-6 产品内 UX(G12)**
  - 每条错误消息给下一步可执行命令(现在很多只说"失败了")
  - 首次运行向导:检测到哪些 provider、建议第一条命令
  - `asg doctor` 改成引导式诊断。当前它只查存储侧健康
    (`schema`/`generation`/`interrupted_batches`/孤儿计数,这些是好的),
    但**新用户最常问的它一个都不查**:provider 根有哪些、各根找到几个文件
    (这是"为什么 discover 什么都没找到"的头号问题)、`--db` 父目录是否存在
    可写、entity 数量(才能提示"库是空的,跑 sync --discover")、
    `asg` 与 `agent-session-grep` 版本是否一致、安装目录是否在 PATH。
    另:本地 `semantic_feature: true` 与 `README.md:78-80`("默认关闭")矛盾,
    因为本地二进制是带 `--features semantic-candle` 编的 —— 要说明哪个权威。
  - shell 补全(bash/zsh/fish/PowerShell)。当前**零补全**:无 `clap_complete`、
    无 `completions/` 目录、无 `asg completions <shell>`。本项目手写参数解析,
    补全要从头写,但覆盖 21 个子命令 + 约 30 个 flag 的静态补全文件是半天工作量,
    而且**它直接缓解 `--db` 路径手敲的痛**。
  - man page(当前无 roff、无 `.1`、无生成器)
  - `--help` 里的 usage 行全部写 `agent-session-grep`,**即使以 `asg` 调用**;
    version 行是 `agent-session-grep-cli 0.1.0`(crate 名,带用户不认识的
    `-cli` 后缀)。README 主推 `asg` 这个名字。要按实际调用名显示。
  - `README.md:56` 说跑 `asg config paths`,`INSTALL-AND-UPGRADE.md:44`
    说跑 `agent-session-grep --robot config paths` —— 统一。
  **验收**:人工走一遍新用户路径无卡点。

- [ ] **M4-7 真实录屏演示**
  asciinema 或 GIF,60 秒内展示:装、索引、搜到三周前的东西、resume。
  当前**零视觉资产**(无 `.cast`、无 GIF、无截图),而项目有 TUI 和内嵌
  Web UI 两个纯视觉价值面。
  **验收**:README 顶部可见。

- [ ] **M4-8 全部过时/矛盾声明清理(G13)**
  M0 已修一批,这里做最终一遍:七条不变量、Gate D 状态、
  discover 一致性、版本陈述、semantic 方向、error catalog 码数。
  另需修:**ADR-0009 编号重复**
  (`ADR-0009-cross-boundary-output-redaction.md` 与
  `ADR-0009-session-resume-metadata.md` 撞号)。
  **验收**:交叉检查脚本或人工清单确认零矛盾。

- [ ] **M4-9 `serve` / `tui` / `hook` 三个已发布入口零用户文档**
  五个宣传入口里有三个没有任何面向用户的文档:
  - `serve`(loopback HTTP + Web UI):grep 全仓库只在 ADR/roadmap/go-no-go
    和 rehearsal runbook 里出现过。它打印
    `asg serve: open http://127.0.0.1:<port>/?token=<32-hex>` 和
    `loopback-only; LAN mode is capability_not_supported` ——
    token-in-URL 模型需要解释,值得原文照录。
  - `tui`:键位(`m` 切 sidechain facet、`k` 切 tool-kind、`f` 切 policy,
    见 `tui/core.rs:647-694`)只在 `SKILL.md:88` 和源码里有。
  - `hook`:Claude Code hook 集成只在
    `subcommand_help_text`(`main.rs:1171-1178`)和 `THREAT-MODEL.md:88` 里。
  **验收**:三个入口各有用户文档;TUI 有演示录屏。

- [x] **M4-10 `docs/` 索引与内部治理文档隔离** —— **已做(`072117e`)**
  当前**没有 `docs/README.md` 或 `docs/index.md`**(已核实不存在),
  40 个 `docs/` 文件里 **35 个只能靠浏览目录树发现** —— README 只链了 5 个。
  写一张表(受众 / 状态 / 一句话用途)。
  同时:M0 已排除若干内部治理文档,剩下仍在公开树里的(各 ADR、RFC、
  contracts draft、evidence matrix、REUSE audit、SLI format、
  external-readiness-gate 等)应移到 `docs/internal/` 并加一行说明
  "这些是历史过程记录,不是产品状态"。
  另:`migration-v5-to-v6.md` / `migration-v6-to-v7.md` 自己被
  `INSTALL-AND-UPGRADE.md:132-134` 标为"historical",而当前 schema 是 v12 ——
  要么删要么明确标注。
  **验收**:README → docs 索引 → 任一文档三跳可达;内部文档有隔离标注。

  **✅ 已完成,但**其中一项做法与原记录不同,理由如下**:
  实际文件数是 **46**(不是 40)。`docs/README.md` 全部列出,带受众与一句话用途。
  **没有把治理文档物理移到 `docs/internal/`** —— 改为在索引里做**语义隔离**
  (单独分区 + 顶部说明"这些是历史记录,不是当前状态;与产品文档冲突时以产品
  文档为准")。理由:物理移动会打断代码注释与其它记录里已有的**大量**相对引用
  (ADR/RFC 被 `main.rs`、`lib.rs`、契约文档反复按路径引用),
  为一个"公开树观感"问题制造一批坏链接不划算;语义隔离达到同样的读者效果。
  **若后续仍要物理迁移,必须同批更新全部引用**,不能只 `git mv`。

  **顺带发现两处,只记录不擅自改**:
  1. **两个不同的 ADR 都编号 0009**(`ADR-0009-cross-boundary-output-redaction`
     与 `ADR-0009-session-resume-metadata`)。索引里明写"按文件名引用,不要只
     按编号",并说明为何不单方面重编号:现有代码注释与其它记录按路径引用,
     重编号会一并打断。**这是个真缺陷,重编号需要单独决策记录。**
  2. 两份 migration runbook 已按 `INSTALL-AND-UPGRADE.md` 的既有说法标为
     **Historical**(schema 已是 v12),不删除 —— 与既有文档保持一致而非各说各话。

  **机械验证(不靠肉眼)**:全部链接可解析、46/46 全部在索引中、
  从 README 出发**每个 docs 文件 2 跳内可达**(要求是 3 跳)。

- [ ] **M4-11 落实文档语言方针(D27)**
  方针已定(D27):**新增的用户可见字符串一律英文,不做存量批量翻译。**
  现状分裂:所有根文档与 operations runbook **0% CJK**,而每份治理文档
  13-31% CJK(`ADR-0007` 30.7%、`THREAT-MODEL.md` 26.3%),
  `--help` 91 行里 **69 行含 CJK**,`human.rs` 有 220 行含 CJK 的用户可见文本。
  **一个 README/CHANGELOG/SECURITY/INSTALL 全英文的公开仓库,主帮助界面
  却约 75% 中文 —— 非中文读者无法使用主帮助面。**
  本任务只做三件事(不做批量翻译):
  1. 在 README 与 CONTRIBUTING 写明语言方针
  2. `--help` 的**顶层结构与命令描述**改英文(这是非中文读者的唯一入口);
     术语速记表可保留双语
  3. 错误消息的 `下一步:` 前缀与可执行命令部分改英文(命令本身是英文,
     提示语混中文最伤)
  其余存量中文随后续任务顺带收敛,不专门开工。
  **验收**:方针写进 README/CONTRIBUTING;`--help` 顶层与错误的下一步提示
  为英文;无新增中文用户可见字符串。

---

### M5 — 转公开与发布 v0.2.0

- [ ] **M5-1 发布门逐条核验(§3 G1-G15)**
  每条给出验证命令与结果。任何一条不过就不往下走。
  **验收**:15 条全绿的记录。

- [ ] **M5-2 最终对抗性审计**
  重跑本轮用过的多路审计:真实数据泄漏、内部过程泄漏、许可证归属、
  语义性内部叙述、断链、自相矛盾、过度声明,**外加一次真实二进制的
  新用户首次运行验证**(本轮就是这样发现 M2P 那 11 条的 —— 只读文档
  发现不了它们)。
  **验收**:无 P0/P1 findings。

- [ ] **M5-3 公开树导出与推送(D17)**
  从今往后真实推送历史:不再压成单 commit,把 M0-M4 的 commit 按序推到
  公开仓库(注意:私有仓库的旧历史不推,从本轮起的干净 commit 开始)。
  **验收**:public-profile 扫描 0 findings;公开仓库历史真实且干净。

- [ ] **M5-4 CHANGELOG 0.2.0 段 + tag + Release**
  按 D19 发 `v0.2.0`。Release notes 必须诚实列出仍存的限制
  (macOS 未验证、无签名二进制、未升 Beta 的 provider)。
  **验收**:tag 指向正确 commit;Release 非 draft。

- [ ] **M5-5 仓库转 PUBLIC**
  `gh repo edit qin-devs/agent-session-grep --visibility public --accept-visibility-change-consequences`
  **这是唯一需要 owner 明确点头的动作 —— 不要自主执行。**
  ⚠️ **不可逆**:转 public 后 fork / 缓存 / 第三方镜像会永久留存内容。
  前置:M5-1 发布门全绿 + M5-2 对抗性审计无 P0/P1 + M5-3 公开树导出干净。
  转完立刻做 M2C-2(验证 CI 解锁)与 M2C-4(fork PR 审批)。
  **验收**:owner 确认后执行并验证 `visibility: PUBLIC`。

## 5. 关键坑与约束(踩过的,别再踩)

### 5.1 环境坑

- **Windows Defender 会间歇删除本仓库下新写的文件。** 写文件后必须核查留存。
  已知案例:全大写含 signing/credential/notarization 等词的文件名被反复静默
  删除,改全小写连字符后稳定。写完 `Test-Path` 确认。
- **PowerShell 是主 shell**(pwsh 7+)。Bash 工具默认禁用,仅在必须 POSIX
  行为时用。Unix 命令不存在:用 `Select-Object -First N` 代 `head`、
  `Get-Content -Tail` 代 `tail`。
- **本机 `bash` 是 WSL bash**,解析 `/mnt/c/...` 而非 `C:\...`。
  `test_install_entrypoints.py` 已加能力探测跳过,别改回 `shutil.which("bash")`。
- **CJK 内容用 Edit 工具时 JSON 编码易损坏**。用 PowerShell here-string
  (闭合 `'@` 必须顶格)或基于 Read 的精确编辑。
- **`cargo build` 有个无害 warning**:`main.rs` 同时属于 `agent-session-grep`
  和 `asg` 两个 bin target。不是错误,别去"修"。

### 5.2 Git 与 worktree 坑

- **stash 栈与主 checkout 及其他 worktree 共享**,可能有并发 session。
  永不用裸 `git stash` / `git stash pop`。要暂存改动用临时 WIP commit;
  必须 stash 时用 `git stash push -u -m "<唯一标签>"`,立刻记下 SHA,
  用 `git stash apply <sha>` 恢复(不用 pop),之后按标签找回并 drop。
- **公开仓库已有 10 个 dependabot PR**(依赖升级),转公开后要处理。
- `.public-tree-preview/` 是导出预览目录,已 gitignored,可随时删重建。

### 5.3 隐私与许可红线(不可越)

- **provider 源 transcript 只读**。永不修改、上传、提交任何真实用户会话数据。
- 真实语料计数可以进文档,但必须注明语料私有不可核验;
  路径、指纹、用户名、主机名、文本内容一律不进。
- **`cass` / `coding_agent_session_search` 的源码禁读**(LICENSE 含
  restricted-party rider),仅允许 clean-room 思路。已验证代码中零痕迹,
  保持这个状态。
- 已修的三处许可归属别退回:agf bounded reader(MIT)、
  agf shell quoting(MIT)、ctx WAL PRAGMA(Apache-2.0)必须保留归属注释。
- `NOTICE` 必须与 `REUSE-LICENSE-AUDIT.md` 一致(已修过一次不一致:
  sessiongrep 的五个 helper 函数虚假 direct-copy 声明,代码里根本不存在)。
- 零出网必须保持:`deny.toml` 禁 HTTP client crate,
  `tests/network_egress.rs` 静态断言,`--offline` fail-closed。

### 5.4 诚实性约束(项目的核心气质,别为了好看破掉)

- 默认 vectorizer 是 `bigram-hash-v1`,是**模糊词法不是语义**,
  文档里永不写成 semantic。
- semantic 的 `promotion_claim` 保持 `none`,`lexical_stays_default` 保持
  `true`,阈值保持 pending —— 除非 owner 明确冻结阈值。
- 性能数字必须带语料规模。无语料说明的 "p50 16.6 ms" 是误导。
- provider 成熟度只能被证据推动,不能因为代码存在就升级。
- 不伪造 CI 成功、不伪造签名、不把 checksum 说成 attestation。

## 6. 待补充(调研 agent 回报后填这里)

- [x] **provider 真实样本获取路径表** — 已回报并落地为 §1.8.1 与
  M1-3/M1-9/M1-10/M1-11/M1-12。**头号发现:两个 deferred provider
  本机就有真实 transcript**(`~/.zcode/cli/db/db.sqlite` 存在;
  DeepSeek Harness 装成 `@deepseek-ai/dsh`,home 是 `~/.dsh` 不是
  `~/.deepseek` —— 之前的扫描找错地方了)。
  12 个可一小时内自建,2 个(grok-build、cursor)现实拿不到。
- [x] **CI 方案结论** — 已回报并落地为 D22-D25 与 M2C-1..5。
  根因不是封号也不是中国网络:私有仓库 2,000 min/月 配额被 macOS 的
  10 倍倍率烧穿(两天 7,397 计费分钟,超 3.7 倍)。
  **public 仓库标准 runner 免费无限 —— 转公开本身就是解法。**
- [x] **搜索作用域惯例调研** — 已回报并落地为 D26、§2.3、M3-3。
  **14/14 同类工具默认全库,零个默认当前项目。** agf 试过项目默认并
  **撤回两次**(排序加权 + cwd 预填),声明原因都是"不可见"。
  atuin 四年没改过 `global` 默认。本项目的架构理由更硬:
  项目信号字段对很多会话合法为 null。
- [x] **功能缺口完整清单** — 已回报并落地为 M3-4..16、M2P-12/13、M1-13、
  G16-G18,以及 §3 的门重写与 §3.1 的 v0.1.0 门核验存档。
  **头号发现:五个入口都答不了"我昨天干了什么"** —— 全仓库只有
  按 BLAKE3 摘要排序和按分数排序两种,没有任何 recency 排序。
  **次严重:`--since` 静默丢弃 Codex 全部历史**(NULL 时间戳对比较恒假)。
  **结构性:矩阵是自我认证的文档而不是证据** —— 漂移测试只比对
  声明↔文档,从不比对声明↔行为,所以 `aider: tool_activity=Partial`
  这种假声明能在全绿 CI 下存活。
- [x] **文档/UX 缺口完整清单** — 已回报并落地为 M2P-1..11 与 M4-1..11。
  关键方法论:该审计**用真实二进制在一次性伪 HOME 下逐条实测**,
  这才发现了 11 条只读文档发现不了的功能缺陷。后续审计沿用这个做法。
- [ ] **性能审计完整报告** — 补充 M2(§1.3/1.7 已有主要结论;
  上一个 agent 中途 API 失败,已重启)。

## 7. 进度日志(每完成一个任务追加一行,最新在最上)

| 日期 | 任务 | 结论 | commit |
|---|---|---|---|
| 2026-08-19 | **M2P-12 cline + grok 时间戳传播** | cline 是明确的 bug:epoch-millis 整数被映射成 `""` —— 读到了、丢掉了、换成空串,**比 `None` 更糟,因为空串看起来像数据**;现渲染成 RFC3339 UTC。grok 完全不需要转换,它的 ACP envelope 时间戳本就是 RFC3339,取每条分组消息首个 chunk 的时间直接传播 —— 那是真实创作时刻,与 codex 的 envelope 时间(per-occurrence 重放写入时刻,刻意不传播)性质不同。两个 provider 的 `known_limitations` 不再声称"未抽取时间戳";golden 用**生产** `parse_search_instant` 断言时间戳落在预期窗口内,而不是只断言非 null —— 后者在一个过滤器读不懂的字符串上也会通过。golden numstat 仅 2 行,证明行尾未被改写。**kimi 仍在进行**(它需要 epoch 转换,涉及把 helper 从 opencode 提到 ports) | `673f317` `b5584b4` |
| 2026-08-19 | **M2-8 + 回归 harness 两个缺陷(我自己发现并修的)** | 想在 10 万条语料上跑七条不变量,结果先撞上 harness 自己的缺陷。**缺陷一**:`collect_sources` 只收 `.jsonl`,于是 hermes/cline(整份 JSON)、opencode/cursor(SQLite)、aider(Markdown)的 transcript **全部被静默丢弃** —— 实测 5,000 个文件只收 3,675 个,而 harness 仍报七条全绿,**四分之一的语料被一份读起来像全覆盖的报告排除掉**。这与 CLI 刚修的 M2P-7 是同一类缺陷,但在一个"存在意义就是被信任"的 harness 里更严重。改为显式短排除清单而非 transcript 扩展名白名单,`.md` 刻意不在清单里(aider 的 transcript 就是 `.aider.chat.history.md`)。**缺陷二**:恢复全覆盖后 `INV-SPAN-COVERAGE` 立刻失败 —— 43,894 个 span 里 875 个非字节精度,而这 875 个**正好**是 cline(450)+ hermes(425),两者的 `source_span` 都声明 `Unsupported`,因为整份 JSON 文档没有文件内字节区间。`Precision::Unknown` 是这种情况的**契约规定值**(`application/src/evidence.rs:12-13`),所以是**不变量在要求一件不实的事**。现改为:至少一个 span 字节精度 + 拒绝契约外的 tier,并如实报出 unknown 计数及其原因。修完全覆盖复跑:5,000 源 / 100,000 emitted / 0 skipped / 七条全 PASS / 退出 0 / 440 s。⚠️ **G5 本身仍依赖 M1-2 的真实冻结快照**,合成语料不能替代 —— 已在证据文件里写明 | `fbfa7a9` `af790ec` |
| 2026-08-19 | **M2-2 四个动作的阀值 harness** | 四条性能阀值第一次真正门住东西(此前 `core_beta_benchmark.py` 测了数字但自己声明"不是 SLO 不是发布认证",而正确性 gate 只有 recall/parse-loss/discovery/resume 四条)。**10 万条实测三条失败,且失败的不是计划预测的那条**:吞吐 0.707 MiB/s(差 5.0 倍)FAIL、noop sync 963 ms **PASS**、搜索 p95 96.03 ms(1.9 倍)FAIL、MCP p95 85.23 ms(1.7 倍)FAIL。**我独立重跑复验,四个数字全部吻合**(0.707/963.19/96.03/85.23 对 agent 的 0.697/963/95.8/86.5),耗时 81.9 s 对其 79.1 s。三处推翻已写入 §1.3.1 并回改 §1.7 的瓶颈表:(a) 瓶颈 #4"线性放大"的推断是错的,它通过了,但只剩 37 ms 余量;(b) §1.4 的 21 ms 在默认构建下**不可复现** —— 默认构建 `present: false`、后端是 bigram-hash,"编码器常驻"这个阀值前提根本不成立,那 21 ms 来自 semantic-candle 构建 + **10 条**语料;(c) 搜索 96 ms 里约 22 ms 是进程启动(`cli_process_overhead_p50_ms` 实测 21.7 ms),一次性 CLI 想达 50 ms 只剩 28 ms 预算。放在 sibling 脚本而非扩展正确性 gate,理由过硬:后者的 `validate_manifest` 断言 metric 集合**完全相等**,折进去会让已发出的每份正确性 manifest 失效。`--skip-embeddings` 在完整规模下门住四条阀值仅需 79 s(285 s 的 embeddings 构建只喂 informational metric)—— 建议逐 PR 用它,全量放 nightly | `9a11dc2`..`9d4efa1` 共 5 个 |
| 2026-08-19 | **M2P-17 `sync --from-file`** | 加 `--from-file <清单>`(每行一个路径,空行与 `#` 注释忽略),按 ripgrep/xargs 惯例。**实测对照**:600 个真实 transcript 的路径展开成 **35,291 个字符**,PowerShell 直接拒绝启动进程(「文件名或扩展名太长」)—— 命令行方式**连进程都起不来**;同一批经 `--from-file` 是 600 sources / 600 emitted / 600 committed / 0 skipped / **0.5 秒**。刻意**不做** glob 展开与引号解析:清单存在的意义就是绕开 shell 分词,含空格的路径必须原样保留,否则清单自己又要处理引号,缺口等于没补。清单读不到报 `invalid_request` 并点名该 flag 而非 `source_io` —— "你给错了 `--from-file` 路径"与"transcript 读不到"的下一步动作不同,不能塌成同一个码;空清单明确报错而非静默成功;与 `--discover` 及多余位置参数互斥。**六个用例全部实测通过**。注:这条缺口此前由调用方各自绕过 —— `real_data_regression.py` 与 `synthetic_corpus.py` 都写了自己的 24 KB 分批,同一个 workaround 写两遍,正是"缺的是工具能力而非脚本技巧"的证据 | `d60d582` |
| 2026-08-19 | **M2P-9 `--provider` 改由能力矩阵校验** | 按前一条已定的修法落地:`enum SearchProvider { Claude, Codex }` 换成 newtype over `String`,合法性交给 `ProviderCapabilityMatrix::current()` 且只接受 `search: Native` 的 provider。**enum 本来就是多余的** —— SQL 下推只用 `as_str()` 拼 `IN (?,?)`,存储层不关心有几个 provider;那份 enum 只是类型系统里一份手抄的矩阵副本,没有任何机制强制它同步。usage 提示改为从同一判据枚举,所以提示与校验不可能漂移(实测提示已列出全部 14 个)。落地时两个真实摩擦:(a) `as_str()` 不再是 `&'static str`,SQL 参数需 `.to_string()` 拥有一份;(b) 19 处测试用 `SearchProvider::Claude` 变体,加 `claude_code()`/`codex()` 两个便捷构造子(刻意只给这两个 —— 它们是唯一有真实语料回归的,任意 provider 走被校验的 `parse`)。**我独立端到端复验五条**:按 tencent-codebuddy 过滤命中它的消息(此前被拒);无数据的 provider 返回 0 命中而非报错;`claude` 别名仍解析;deferred 的 zcode 仍被拒(它 `search: Unknown` —— 矩阵在起作用);不存在的 provider 仍被拒。**Web 层如预测般自动继承**:`/api/search?provider=tencent-codebuddy` 从 HTTP 400 变成 200 + 1 命中,零额外改动 | `434f36a` |
| 2026-08-19 | **M2P-5 + M2P-7 discover 宽容化与格式感知** | **M2P-5**:`--discover` 走的是用户没点名的目录,所以一个非 transcript 文件(编辑器备份、写一半的日志、别的工具的输出)就让整轮退出 2、零消息入库。现改为跳过并计数,与 D11 对损坏行的既有处理对齐;显式 `sync <file>` 仍然失败 —— 那是用户自己点的路径。误导性诊断另有根因:`last_probe_error` 报的是**最后一个** probe 的失败,而 registry 以 SQLite 的 Cursor adapter 结尾,于是每个不认识的文本文件都被怪成"缺 SQLite magic header";现按源自身的 format family 过滤 probe 失败。**M2P-7**:候选文件按 `.jsonl` 扩展名收,于是 opencode(根已注册、格式是 SQLite)永远 `found=0` 却报 `complete=true` —— 而 complete 正是 tombstone 差分的依据。现读 64 KiB 头按签名分类,只把源交给同族 adapter;实测 300 个垃圾文件 + 3 个真源 = 105 ms。**歧义的处置已明确决策**:`AmbiguousVariant` 也是跳过并计数而非致命,措辞单列 —— 拒绝猜测的原则不变,变的只是波及面;把用户真实的 pi/openclaw transcript 说成"不是可识别的 transcript"会把人送去查错误的问题。**我独立端到端复验四条**:1 好 + 2 垃圾 → 退出 0、好的那条可搜;歧义源 → 退出 0、`unrecognized=1`、warning 说"匹配了多个格式,跳过而非猜测,显式 sync 这个文件可看到哪些格式冲突";显式 sync 垃圾文件仍退出 2;错误消息里再无 SQLite 字样 | `cf7b48a` `e70426f` `6a1d83f` |
| 2026-08-19 | **M2P-11 `index` 直写后门加门** | 先实测确认它**真的能用**:往库里写任意文本、`search` 立刻能搜到,而它就登在用户 `--help` 里 —— 等于邀请用户破坏自己的真实历史(写进去的东西无源文件、无 provenance,`index rebuild` 会忠实重投影)。现在需显式 `--force-dev`,拒绝时指向 `sync --discover`;三处 help(顶层 COMMANDS、`index --help`、模块头)全部撤下。15 个 e2e 测试用它造数据 —— 它们不是编码了错误行为(后门本就该留给测试),所以走一个新的 `seed()` 辅助函数集中补 flag,而不是把 flag 抄 15 遍。四个用例实测:无门拒绝且指向 sync / 顶层 help 无痕 / 子命令 help 无痕 / 带门仍可写且可搜 | `5a1d52e` |
| 2026-08-19 | **M2-1 十万条确定性合成语料** | 发布门的每条性能阀值都写在 10 万条上,而树里最大语料只有 2000 条 —— 阀值 harness、增量同步、存储放大三项都无从测量。**语料不入库**(约 49 MB,可由生成器 + 种子完全重建),入库的是生成器 + 26 个常驻测试 + 冻结清单(CRLF 归一化树哈希 + 逐 provider 计数)。确定性来自逐 session 独立播种,所以一个 session 的字节不依赖语料规模或写入顺序。**我独立复算复验**:重新生成得 `65cc08e9…`,与冻结清单逐位一致;摄入 100,000 emitted / 0 skipped / 100,000 placements、catalog 110,000(10 万消息 + 5 千 session + 5 千 document,零去重塌陷)—— 这同时证明 M2P-14 的修复在 10 万条规模上成立。我自己也写了一版生成器,发现 agent 版本更完备(它刻意避开了 pi/qoder/codebuddy 的歧义陷阱,我撞上了才知道)后弃用自己的 | `4c46802`..`2892783` 共 5 个 |
| 2026-08-19 | **M2P 首次运行:M2P-1/2/3/4/6/8 + M2P-15** | 用户头一分钟就会撞到的六条已修。**M2P-2 最重要**:读命令过去在 `--db` 打错时会**静默建一个 233 KB 的空库并报"无结果"** —— 打错一个字母得到"干净的无结果",毫无警告;现在报 `not_found` 且不留文件(实测确认)。M2P-3 存储路径改为 `--db` > `$ASG_DB` > `<data>/asg.db`,零配置可用。M2P-1 human 表格改渲染规范 `ses_v1_` id,并有测试**像用户那样**从表格里抓 id 再喂给 `context`。M2P-15(我自己发现并自己修的)`sync` 把未识别 flag 当路径,再拿 `source_io`「确认源文件路径存在且可读」怪路径 —— 路径明明存在可读;现在报 `invalid_request` 并回显该 flag,`-`(stdin 惯例)仍合法,真正缺文件仍报 `source_io`。**计划里"给 `source_io` 的 details 加路径"那条建议是错的,已否决**:`protocol.rs:253` 明确写了 SourceIo 细节被扣留正因为它可能含绝对 transcript 路径,加上去会违反代码自己在执行的隐私约束。顺带修了 `render_human_error` 无条件追加通用指引的问题 —— 库不存在时它会在正确的下一步命令后面再补一句"确认实体 ID 是否正确(运行 list 可浏览)",既答错问题又推荐一条同样需要库的命令 | `8d0d43b` `e801a59` `3d8cf91` `89a058c` `0dfaf19` `568472c` `4243f98` |
| 2026-08-19 | **M2P-14 合成 id 碰撞(11 个 provider,比预估多 7 个)** | 缺陷面比登记时大:除 cline/grok/kimi/aider,还有 hermes/pi/qoder/openclaw/codebuddy/cursor/antigravity。**antigravity 最严重** —— 它把 `step_index` 当持久 id 直传,于是**每一份** transcript 的第一步都声明 id `"0"`,连 provider 前缀都没有。cursor 的代码注释还写着 id 是按序号派生的"与 cline/codebuddy/qoder adapter 相同" —— 缺陷在树里被当成可复制的范式记录着。11 个 provider 现改为 emit 空 `native_id`,由 CLI 的 `[provider, variant, document_id, seq]` 派生文档作用域 id。**fixture 源字节零改动**(BLAKE3 pin 全部仍匹配),只有 `expected.json` 里的 `native_id` 值变了 —— 这正是 golden 应有的行为。副产物:去掉 antigravity 的 id 用法后 `StepRecord.step_index` 成死代码被 clippy 抓到(probe 判别改读原始 JSON 值),已删字段并注明为何刻意缺席。**我独立端到端复验**:两份互不相关的 pi transcript 各含一条消息 0,sync 报 2 条,两条的标记词各搜各中(修复前第二条静默消失),派生 id 是两个不同的 `msg_v1_<hex>` | `d01f47e`..`8387387` 共 11 个 |
| 2026-08-19 | **M0 全 20 项闭合** | 剩余两条清掉:14 处发布文件把治理状态挂在**不发布**的架构审查文档上(读者被指向打不开的记录),改为指向各文件自己已记录的 `approver`,只有一处真属 ADR-0001 才引 ADR;裸「本机」在发布文档里会被读成对**读者**机器的断言,provenance 站点统一改「维护者开发机」。刻意保留四处(ADR-0004/0009 的 local-first 隐私声明、RFC-0001、探针脚本自己的输出)—— 那里「本机」确实指读者的机器,改了会把隐私声明反转 | `c1e2a88` `33f1194` `9f738a9` |
| 2026-08-19 | **M2-4 分层语义检索 + 生产接线** | 语义打分改为「有界候选集 + 精确重排」:候选集经与 lexical 完全同一套 FTS 路径召回至多 2000 条,只对候选读向量算余弦。10 万条 × 384 维实测 p50 895.6→24.6 ms、p95 1111.2→26.6 ms(达标 ≤50 ms);候选集为空/过小(<500)时**回退全量精确打分**,因为实测 100 条 gold query 字面最多只命中 8 条(中位 2),不回退会正好抹掉 paraphrase 召回——语义检索存在的唯一理由。踩坑:SQLite 选了非选择性的 `message_vec_model` 索引全表扫,分层白做(674 ms),两个谓词加一元 `+` 强制走主键(674→24 ms),已用查询计划断言锁住(选错计划是"结果正确但慢",行为测试抓不到)。recall 与 §1.5 基线逐位一致。**接线**:`query_semantic` 端口只收 `(embedding, limit)` 无查询原文,故分层路径在生产里是死的(仍走 147 ms 回退);已在 `main.rs`/`mcp.rs` 两处 search 入口逐请求设置候选原文(不能只设一次,否则上一次的原文会筛这一次的候选集),索引构建路径无查询不设。**我独立端到端复验**:查询词完全不在语料里时仍返回 5 条命中 —— 若候选为空时没回退,结果会是 0 命中(即静默退化成 lexical) | `1c4d633` `8e17e81` `2adf345` |
| 2026-08-19 | **M2P-12 opencode 消息时间戳** | opencode `ORDER BY time_created` 但从未 select 该列,每条消息的 `timestamp` 都是 None → 落在所有时间窗过滤之外。现读取 epoch-ms 并渲染 RFC3339 UTC(`parse_search_instant` 拒绝无时区后缀的裸本地时间,所以必须渲染而非直传);缺失或超范围(≤0 / >9999-12-31)保持 None 而非编造时刻。golden 测试用**生产解析器**解每条时间戳并断言落在预期窗口内 | `559cfcf` |
| 2026-08-19 | **M0 公开树收口(20 条中 18 条)** | 发布工具链与内部草稿出公开树;`export_public_tree.py` 的自豁免 bug 修法是把 manifest 写到目标目录**之外**——它唯一写的那个文件曾是唯一不被扫的文件,这正是 `PUBLIC-TREE-MANIFEST.json:3` 的泄漏能在一轮报"0 findings"里存活的原因。实测导出 475 文件、public profile 0 findings。文档数字与出处对齐(七条不变量、14 码 error catalog、semantic ≥ lexical 方向纠正、延迟数字补 n=10 限定)。M0-6/M0-19(c) 有残留悬挂引用,已派单独 agent | `2ba7022` 及其 8 个来源 commit |
| 2026-08-19 | **M1-13 + M2P-13(首个实施任务)** | 行为断言落地(`capability_behaviour.rs`,4 条断言含覆盖面守护),**矩阵不再能自我认证**;修 aider 假声明 `Partial→Unsupported` 及两份 ledger 的同步漂移。已自证断言会咬(临时把 openclaw/cline 改成过度声明,两条如期失败)。副产物:codex golden fixture 缺 tool-call 记录,无法见证自己声明的能力(提取是真实现的)→ 已登记进 M1-5 | `e4f836f` `c1356cb` `2fc1d3e` |
| 2026-08-18 | 功能缺口审计 | v0.1.0 发布门 12 条:3 满足 / 7 部分 / 2 未满足,且 3 条本地不可能满足 → 门已重写(§3)。最大缺口:五个入口都答不了"我昨天干了什么"(无任何 recency 排序);`--since` 静默丢弃 Codex 全部历史;catalog 层完全无删除路径;矩阵只比对声明↔文档不比对声明↔行为,所以 `aider: tool_activity=Partial` 假声明能过全绿 CI | — |
| 2026-08-18 | 搜索作用域调研 | 14/14 同类工具默认全库,零个默认当前项目;agf 试过项目默认并撤回两次(原因是"不可见"非加权本身),claude-historian 从按项目扫描改为无上限跨项目,Recall 把 current-repo 默认做出来后关掉,atuin 四年未改 global 默认。决策 D26 落定:全库默认 + 加性同项目加权 + 双侧显示来源 | `d706e46` |
| 2026-08-18 | provider 样本调研 | 推翻两个 deferred 前提:zcode 与 deepseek-harness 本机都有真实 transcript(dsh 的 home 是 `~/.dsh` 不是 `~/.deepseek`)。发现 opencode probe 会误认 zcode DB(真实 AmbiguousVariant)、qoder 路径三方不一致、hermes 声明的格式已被上游停用。12 个可一小时内自建,grok-build/cursor 拿不到 | `42b5936` |
| 2026-08-18 | CI 调研 | 根因查实:不是封号/中国网络,是私有仓库 2000 min/月 配额被 macOS 10x 倍率烧穿(两天 7397 计费分钟)。public 仓库 runner 免费无限 → 转公开本身即解法。多账号与借用第三方 CI 均否决(ToS + 供应链风险 + 零收益) | `f4df823` |
| 2026-08-18 | 文档/UX 审计 | 用真实二进制实测发现 11 条首次运行 P0/P1 功能缺陷(落为 M2P)+ 11 条文档缺口(落为 M4);最严重:human search 输出的 session id 喂给 context 会被拒、读命令打错路径静默建空库 | `eeea337` |
| 2026-08-18 | 规划 | 本文件建立;owner 拷问确认 D1-D21 共 21 条决策 | `af26bb6` |
| 2026-08-18 | 发布后审计 | 发现内部过程泄漏(5 CRITICAL + 11 HIGH)与 3 处许可违规,已修;仓库转回 PRIVATE | `ff0838a` |
| 2026-08-18 | v0.1.0 | tag + Release 发布(公开仓库,随后转回 PRIVATE) | `55648d6` |







