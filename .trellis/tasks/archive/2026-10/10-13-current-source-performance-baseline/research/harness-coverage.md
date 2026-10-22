# Research: 当前基准 harness 的覆盖、计时边界与最小诊断补足

- Query: 当前 canonical Core/Beta 与相关 harness 实际测量哪些 startup/index/sync/search/context 工作负载；怎样保留历史契约并补足可复现基线和操作级诊断？
- Scope: internal；规划研究，不运行测量、不修改产品代码。
- Date: 2026-10-14
- Source: 调度指定的 `.trellis/.runtime/attribution-closeout-20261012/canonical-repaired`，调度已观察的 clean main 为 `8e05eb495160d5ff5979cdbef6f63856d5a7de81`。本研究没有执行 Git 命令重新认证该状态。
- Path convention: 下文产品路径及行号相对此 canonical checkout；`.trellis/spec/`、task 与 workflow 路径相对协调根。未把协调根旧产品内容用作证据。

## Findings

### 1. 结论与文件索引

Core/Beta full 满足它自己的样本数契约，能作为明确标注限制的 CLI 进程级基线；它不等于真实冷启动、完整会话上下文基线或大规模工作负载。最重要的覆盖差异是：**生成 4,000 条后先 shrink，再在保留的 2,000 条上查询；`show/get` 只取单实体，真正的 `context` 没有被测量。**

| 文件 | 当前职责及关键证据 |
| --- | --- |
| `scripts/evidence/core_beta_benchmark.py:30` | 两个固定 profile、合成 corpus、九项 metric、JSON/Markdown 输出、v1 验证器。 |
| `scripts/evidence/test_core_beta_benchmark.py:12` | 四个测试：nearest-rank、单样本离散度、摘要篡改、生成器确定性。 |
| `docs/product/SLI-AND-BENCHMARK-FORMAT.md:17` | SLI 定义与采样方法；`:56` 为 Core/Beta v1 报告契约。 |
| `scripts/evidence/open_source_gate_benchmark.py:427` | 有标签 corpus 的六指标 gate，另附非门禁延迟；与 Core/Beta schema 不同。 |
| `scripts/evidence/semantic_benchmark.py:1452` | 固定 200 Session / 2,000 Message、100 个 qrels 查询，三种检索模式。 |
| `scripts/benchmark.py:40` | 旧 discovery/search/目录体积工具；不适合作为本任务的直接替代入口。 |
| `crates/agent-session-grep-cli/src/lib.rs:2300` | 当前 search 参数、默认页大小与检索模式；`:2614` 单实体读取；`:2787` 真正 context。 |
| `crates/agent-session-grep-application/src/budget.rs:9` | 响应预算默认值及下限。 |
| `crates/agent-session-grep-cli/tests/e2e.rs:1960` | sync/no-op、`:2330` shrink/tombstone、`:4312` context 链与证据、`:4662` partial/exit 10。 |
| `schemas/robot/v1.1/envelope.schema.json:135` | Robot 成功 frame 的 command/outcome/page/meta 等结构；不是 benchmark report schema。 |
| `crates/agent-session-grep-adapters-sqlite/tests/process_evidence.rs:86` | 中断 intent 的恢复正确性；未给出生产恢复延迟基线。 |
| `.github/workflows/core-beta-evidence.yml:101` | 运行 Core/Beta smoke 和验证器，不运行 full。 |

### 2. 精确 profile、工作负载与九项 legacy 指标

`PROFILES` 是固定常量，CLI 没有 files/messages/repetitions 覆盖参数（`core_beta_benchmark.py:30`、`:714`）。

| Profile | 生成 corpus | shrink 后保留 | startup 每种状态 | 每种 sync | search / show / get 各自 |
| --- | ---: | ---: | ---: | ---: | ---: |
| smoke | 2 文件 × 12 = 24 消息 | 2 × 6 = 12 | 3 | 1 | 5 |
| full | 20 文件 × 200 = 4,000 消息 | 20 × 100 = 2,000 | 20 | 3 | 100 |

每个 sync trial 都重新复制 master，并依次执行 **fresh initial sync → 一次不变源 sync → 所有文件保留前半后 sync**。仅最后一个 trial 的 shrink 后 catalog 用于所有查询（`:398`、`:407`、`:410`、`:416`）。不存在“在同一库上连续做三次 no-op”的现行 full 协议。

| legacy metric 名称 | full 样本数及实际动作 | 可以声称的范围 / 主要差异 |
| --- | --- | --- |
| `cli_startup_cold_latency_ms` | 20；各自复制一次二进制，再执行 `--version` | `cold_best_effort`；复制不在计时内，未清 OS cache。 |
| `cli_startup_warm_latency_ms` | 3 次 `--version` 预热后，同一 binary 路径调用 20 次 | 进程启动至版本命令结束；没有 readiness 信号或数据库打开。 |
| `initial_sync_latency_ms` | 3；每次 fresh DB，对 20 个显式文件执行 `sync` | 整个 ingest/store/FTS/退出成本；不是独立 engine index 构建时间。 |
| `noop_sync_latency_ms` | 3；每个独立 trial 的 initial 后执行一次 | 显式文件、不变源；不包含目录 discovery。 |
| `shrink_sync_latency_ms` | 3；所有文件改为前 100 条，再 sync | 覆盖源替换/删除路径；不覆盖追加增长。 |
| `search_latency_ms` | 100；五个固定查询循环，各 20 次 | pooled 分布；最后一个 shrink DB、默认 lexical、默认首屏 20 条。 |
| `show_latency_ms` | 100；retained ID 数组的前 100 个 | full 中全部来自第一个文件，每个 ID 一次；结构化单实体。 |
| `get_latency_ms` | 100；同一前 100 ID，在所有 show 完成后执行 | 原始 payload 单实体；不能与 show 合称会话链读取。 |
| `initial_index_throughput_mb_s` | 3；master bytes / 各次 initial sync 总耗时 | 实际单位 `MiB/s`，不是 SI MB/s；派生值，不是第四种独立同步计时。 |

锚点：startup `:339`；sync `:398`；retained ID 顺序 `:420`；metric 定义 `:432`；吞吐 `:427`。当前 search 默认 lexical、页大小 20、无额外 facet/时间/provider 过滤（CLI `lib.rs:2311`、`:2378`）；默认通用预算为 4 MiB、1,000 items、2,000 snippet chars（`budget.rs:9`）。harness 未把这些默认值显式写入报告，因此源码默认变更会改变工作负载。

### 3. 时钟、warmup、cache 和内存的真实含义

- `run_process` 在 `Popen` 前记录 `perf_counter_ns`，到 `communicate()`、停止采样、`sampler.join(timeout=1.0)` 后结束；包括新进程、数据库打开、Rust 执行/输出和 Python 监测收尾，**不含** Python 的 stdout UTF-8 解码、Robot JSON 解析及后续正确性检查（`:141`、`:162`、`:165`、`:190`）。没有子进程 timeout。
- 只有 startup 有三次明确的排除样本预热。warm startup 先于 copied-binary cold；initial corpus 刚写/复制过；sync/search/show/get 均启动新进程、没有持久连接复用或独立 query warmup。`warm` 只表示运行次序下的已填充 store/cache 状态，不是已控制的暖缓存实验（`:339`、`:383`、`:398`、`:416`、`:475`）。
- Windows 读 `PeakWorkingSetSize`；Linux 读当前 `/proc/<pid>/status` 的 `VmRSS`；macOS 调 `ps rss`；循环间隔 100 ms，按采到的最大值保存（`:89`、`:151`）。Linux/macOS 的短峰可能漏掉；Windows 的最后一次峰值观察也取决于采样时机。测的是 CLI 进程，不是 Python harness 或所有子进程 RSS 总和。
- `metric()` 丢弃不可得的 RSS 项；其 raw 数组没有与 duration 一一对应的 null、采样时刻或后端标记。aggregate 是各操作可用 RSS 样本串接后的分布，不是同一时刻所有进程内存总和（`:245`、`:447`、`:480`）。数值以 1024² 换算，实际为 MiB。
- 可低成本新增记录已有 Robot `meta.duration_ms`：它在 `run()` 开头计时，包含解析/打开 store/dispatch/drop store/写后 wake，结束于 redaction 和最终序列化输出之前；整数毫秒，不能命名为纯 SQL 或纯 application 时间（CLI `lib.rs:498`、`:675`、`:686`、`:841`；schema `:93`）。它与同次外部 wall time 的差额也不能直接归因于单一内部原因。

### 4. Corpus、正确性和原始样本完整性

生成器固定 Claude Code JSONL，UTF-8/LF、确定性 UUID、user/assistant 交替；每 10 条重置 parent，`index % 17 == 0` 标记 sidechain，日期循环 28 天。所有消息都有 `benchmarktoken`，其余在 alpha / 配置 / src/main.rs / EVIDENCE42 四类间轮换；没有显式 `sessionId`、工具 activity、长正文或错误栈分布（`:198`、`:217`）。五查询全部是正命中、高重复模板；没有 qrels、零结果/稀有词/复杂过滤组合。生成器的文件数不能直接当成经观测确认的 canonical Session 数。

master 哈希是排序后的相对 POSIX 路径、NUL、完整 bytes、NUL 的 SHA-256；哈希发生于 shrink 前，报告没有 query 阶段的 corpus hash/message count（`:49`、`:386`、`:455`）。按代码期望 full query corpus 为 2,000 条，但现行运行没有断言全部 ingest 或全部 tombstone 成功，不能把“生成数量”充作“已核验在库数量”。

现行正确性保护是非零退出失败、首行必须是 `ok: true` 的 JSON object、每个 search 至少一个 hit（`:166`、`:179`、`:417`）。没有断言 sync 的 sources/emitted/committed/skipped/unchanged/generation，未核验 shrink 删除后不可检索，未核验 show/get 返回的具体 ID/正文；也未按完整 Robot schema 验证 command/outcome/page。合法 `partial` 的 exit 10 会被当前 helper 拒绝，不能为新 context 工作负载直接放宽所有旧操作的成功条件。

原始 duration 以 6 位小数毫秒保存。统计使用 `ceil(p/100*n)` 的 nearest-rank，mean 与样本标准差 n−1，单样本标准差为 0（`:60`、`:66`、`:81`）。因此 n=20 时 P50 是第 10 个顺序统计量，不是两中项平均；n=3 时 P95/P99 都等于最大值。五类 search pooled P99 不表示每类 P99；full 每类只有 20 次。不要把这些尾部摘要当稳定的 SLO 阈值。

v1 验证器已经重算每项 duration、RSS 与 aggregate RSS 摘要，要求九个 exact metric key、profile 对应 required count、raw 数量至少达到要求，验证 digest 格式、部分环境和 size 一致性（`:536`、`:561`、`:574`、`:599`、`:616`）。这是真正已有的保护，但不等于 provenance 或 correctness 证明：

- 不记录每样本 query/ID/trial/phase/argv/outcome/response bytes；RSS 对齐信息丢失；失败直接抛出，报告不能重构运行次序和失败位置。
- 没有重算吞吐与 initial duration/source bytes 的关系、`index_size_ratio` 与 size 的关系、最后 initial size 与原始 size 样本的关系。
- 不严格校验内层 name/unit/state/methodology/query_set、每一环境值内容或全部容器形状；数值守卫是 `isinstance(int,float) and >=0`，未显式排除 bool 或统一执行 finite 校验（`:582`、`:586`）。不要把这里写成“NaN 一定可通过”。
- full 只对指定的五个环境字段拒绝两个 placeholder；source HEAD 检查不是 dirty-tree 检查，binary hash/provenance 不是独立 source-to-binary 证明（`:353`、`:363`、`:649`）。这部分由 provenance 研究进一步规划。
- 单元测试目前只有四个，未直接覆盖完整 `validate_report` 的正负 fixture、计时边界或 sync 正确性；CI smoke 才实际贯通 run→report→validate（test `:12`；workflow `:101`）。

### 5. 与 SLI 文档逐项对齐

| SLI 要求 | 当前可用证据与必须披露的差异 |
| --- | --- |
| 冷/热 startup 各 20、启动到可接受命令 | full 数量符合；当前是 `--version` 完成代理值、best-effort cold，不是 readiness 或 cache-flushed cold。 |
| search / show 固定集 100 次 | full 符合 v1 数量；搜索为五查询混合，show 为单实体，不覆盖会话 context。 |
| 初次索引吞吐、无变化扫描连续 3 次 | initial sync 吞吐已有；no-op 是三个独立 trial，且不是 discovery scan。 |
| 每 100 ms RSS | 当前实施了平台相关方法；短进程、不可得样本和量纲必须披露。 |
| checkpoint 后 index/source ratio | 当前统计 `catalog.db*` 存在文件、命令退出后的字节数；未显式 checkpoint，且包含整个 catalog。主 ratio 使用最后 initial trial 的 size；另存各 trial size raw 和最后 shrink 后 footprint（`:404`、`:429`、`:484`）。 |
| recall@10、source parse coverage | Core 没有；gate/semantic/qrels 和 provider golden 测试属于独立正确性证据，不能由“非空命中”替代。 |
| recovery_time_ms | Core 诚实报告 `not_implemented` / null，doctor 时间仅 observation（`:495`）；现有恢复正确性测试不提供此生产计时。 |
| robot_response_bytes | Core 未记录；stdout 已被捕获，可在新增诊断区记录同次响应 UTF-8 字节数，并固定是否计入末尾 LF。 |

文档锚点：SLI `:19`、`:33`、`:63`。文档仍为 Draft；标准 100k Session / 10M Message / 50 GB 属专项压测，不由本任务自动纳入（`:6`、`:90`）。历史 full 报告明确标记 2026-07-25、commit `88d86f44a0f8d4d0261aaddddb99524514e57d7b`，不可重新标为当前 main 实测（历史 JSON `:2`）。

### 6. 其他现有 harness 的可复用部分与边界

| Harness | 实际工作负载 / 正确性 | 不能替代 full 的原因 |
| --- | --- | --- |
| open-source gate | 3 个 fixture 文件、Claude+Codex、6 个英文带标签 query；每 query 每模式一次，show/get 默认各 10 次且只取第一个 label ID；sync 默认 1 次，多次时混合 initial 与 no-op。显式 ingest 与隔离 HOME discovery 分开；resume 只预览，handoff 检查 evidence。 | 六指标是四个 threshold ratio + 两个 informational recall；延迟仅 p50/p95/count，没有 raw 保存、warmup 或完整 SLI 环境。`--profile` 只是文件名。不能将 gate pass 解释为性能 full。 |
| semantic | 固定 seed 20260815，200×10 消息，40 近重复对+40 改写对、100 个 zh/en/code gold 查询；top-20 返回算 recall@5/10/20；`--repeat` 默认 1，每 query 连续重复，按 lexical→semantic→hybrid 顺序。首次同步要求 emitted=2000/skipped=0；校验 corpus hash，建 embeddings，记录 fallback/model 身份。 | 延迟仅 pooled p50/p95/count，raw 在内存中测得但未落盘；correctness recall 取首个 repetition；没有暖场/缓存协议或完整环境。默认 bigram-hash 被明确标记非真实 semantic 模型；不用模型下载扩展本任务。 |
| `scripts/benchmark.py` | 默认 12 个 query，各一次，`--max-items 20`；一次 `sync --discover`；遍历 data_root 统计体积。 | **`--corpus` 只解析未使用，discovery 未隔离 HOME，可能读真实源**。丢失败样本和原始延迟；p50 用 n//2、p95 用 int(.95*n) 的索引，与 Core nearest-rank 不同。缺环境/hash/validator；不建议修复此旧工具来创建另一条基线管线。 |
| search-backend spike | 默认 20k 合成文档，直接调用 FTS5/Tantivy 后端比较。 | 是引擎 spike，不是当前 CLI/App/sync 全链路；只保留为历史锚点。 |

锚点：gate `:117`、`:174`、`:284`、`:427`、`:480`、`:624`、`:793`，`fixtures/gate/labels.json:3`；semantic `:87`、`:1452`、`:1515`、`:1647`、`:1812`；旧 benchmark `:23`、`:72`、`:149`、`:174`；spike `src/main.rs:42`。

可复用测试是 Core 的统计/生成器测试，gate 的标签、parse loss 与 gate validator 回归，semantic 的 byte determinism/gold bookkeeping/AND 可达性/隐私/报告计数测试（对应 test 文件 `:12`、`:22`、`:212`、`:41`、`:99`、`:124`、`:297`）。这些测试的存在不代表本轮执行过，也不证明 Core 测量结果正确。

### 7. 最小兼容补足：保留 legacy，新增可选诊断区

建议保留 full 的 corpus 尺寸、九项名字、采样次序、warmup 与统计定义。当前 validator 对 metric key 和 profile corpus 尺寸精确比较（Core `:572`、`:623`），不能直接增加第十个 metric 或暗改 full 尺寸。主计划现已选定同一 process lifecycle 记录双时钟：旧 helper/九项保留 observer cleanup，新扩展单独保存 cleanup 前停止的 corrected observation，详见第 10 节。

可选顶层扩展是主计划可采用的最小方向，例如 `diagnostics` 内带独立 schema/protocol version；**这是设计建议，当前代码不存在**。扩展缺失时旧 v1 报告继续验证；出现时执行独立、条件严格的身份/数量/finite 数值/统计/正确性校验。不得往旧 raw 中混入新 workload 或把旧报告缺失字段伪造为已经测得。具体字段名称及版本由设计审查冻结。

| 新诊断记录（不加入 legacy 九项集合） | 建议 workload/边界 | 原始证据及验证 |
| --- | --- | --- |
| 操作 wall time | 按 workload、phase、trial、query/anchor/policy 分组 | 每次调用序号、实际参数、返回码、运行状态、精确计时定义；从原始值重算统计。 |
| 同次 `cli_reported_duration_ms` | sync/search/context 等有 Robot meta 的调用 | 保留整数粒度和 run→emit 前范围，不冒称独立内部阶段。 |
| 分 query 的 search 摘要 | 首先复用原五查询，无需新增 100 次请求 | 每类 full 20 次；原 pooled 指标不变；记录命中 ID/count、mode、generation、预算。 |
| `context` mainline/raw 与 full/raw wall time | 固定、可核验的 Session 集及显式消息/字节预算 | 链/集合/顺序/evidence/branch leaf/partial 规则见下一节；重复次数由主计划统一决定。 |
| 响应字节、逐次 nullable RSS | 同一调用直接采集 | 字节定义；RSS 方法、间隔、null 原因，保持与 duration 一一对齐。 |
| 各 phase corpus/store/counter 快照 | initial→noop→shrink；可选连续 noop | 生成/保留消息数、源 bytes/hash、observed sources/placements/generation，size 文件清单及派生值校验。 |
| 有限规模/追加/discovery 诊断 | 仅在用户审查矩阵后启用 | 与 legacy full 分离，独立规模/阶段标识；纯 synthetic；固定文件数与单文件消息数，不能只给总消息数。 |

最低 correctness 补足：fresh ingest 校验预期计数/skipped=0；no-op 校验 committed=0 与 generation 不变；shrink 校验保留项仍可搜、删除项不可搜、placements/源状态符合预期；show/get 对照生成器期望正文/role。这些是现有语义的 harness 断言，依据 `e2e.rs:1960`、`:2000`、`:2330`、`:2398`，不需要 Rust 行为修改。

应为新诊断协议增加 child-process timeout/失败记录及可验证的总预算，避免当前无 timeout 的无限等待；不把失败改成零值或删除样本。source/harness/binary、环境、完整参数、工作目录语境和运行顺序必须足以判断比较资格；外部 wall、内部 meta 与 trace 不能混成一个计时系列。trace 的具体启用条件与字段由主研究补充。

必要回归：旧 checked-in full 和 smoke 继续通过；扩展缺失/有效/无效三类；缺失、重复、错序标签与样本数量；非法数值；derived throughput/ratio 对不上；counter/generation/删除语义失败；预期 partial 与真正失败分流；完整 JSON frame 解析；保留当前正常执行路径的计时定义。不以修复所有旧 helper 的历史问题为本任务前提。

### 8. 真正 context 的现有命令与正确性契约

当前合法调用（尖括号为计划待填值，不是已经执行的命令）：

```text
<binary> --db <synthetic-db> --output json context <observed-session-wire-id> --policy mainline --level raw --max-messages <N> --max-bytes <B>
<binary> --db <synthetic-db> --output json context <observed-session-wire-id> --policy full --level raw --max-messages <N> --max-bytes <B>
```

- `--db`、`--output` 是前缀全局参数；沿用 Core 的 JSON 单响应方式。`context` 接收 Session wire ID，不能把 Message ID 换前缀伪造。可在不计时的设置阶段从已核验的 search hit `session_id` 收集真实 canonical ID（schema `:16`）；测试里的 `session_wire_for_message` 实际通过 SQLite placements 查找（`e2e.rs:261`），不是现有 `get-message-contexts` CLI 命令。
- 支持 `--policy mainline|full`，默认 mainline；`--level raw|talks|sessions`，默认 raw；消息上限使用 `--max-messages`，不是 `--max-items`/`--limit`；另有 `--max-bytes`（CLI `lib.rs:2787`）。默认 N=500、B=4,194,304；有效预算下限 N≥1、B≥4,096（`budget.rs:9`、`:20`、`:69`）。本任务新增 raw 两策略即可，talks/sessions 不自动纳入。
- `show/get` 在 Application 都只调用 `catalog.get`；前者把 payload 展开为 `data.entity`，后者为 `data.payload` 字符串，均不装配消息链（Application `lib.rs:2115`；CLI `lib.rs:5402`、`:5425`）。应在计划和报告中明确叫单实体读取。
- raw context 完整成功应核验 `command=context`、`ok=true`、`outcome=success`、`data.session_id` 正确，输出 message/placement ID 与独立生成的关系预期相符。每条 `messages[].id == message_id`；证据 occurrence/message 与 placement/message 对齐，指纹与 source-document 身份一致，byte span 确实切回合成源记录；branch leaf 和 leaf placement 与主链对应（`e2e.rs:4312`）。结果还携带 requested/effective level、truncation、generation（CLI `lib.rs:5521`）。
- mainline 排除 sidechain 并沿 parent 链；full 包含 sidechain，但 full 顺序不可直接当成原文件行序或真实 chronology。已有 fixture 对 mainline 和 full 的确切 ID 顺序有断言（`e2e.rs:4327`、`:4386`）；新增 corpus 的 oracle 应由生成关系/既定排序独立得出，不从被测结果复制成“期望”。
- 预算截断是有效 partial：例如 `--max-messages 2`，已有测试要求 exit 10、`outcome=partial`、2 条 messages、`truncation.truncated=true`、reason=`max_messages`（`e2e.rs:4662`）。若矩阵只测完整返回，应选择足够预算并明确拒绝意外 partial；若专门测预算路径，应在新扩展单独接收并严格验证 0/10，保留旧 `run_process` 非零失败语义。缺 Session 是 exit 4 / not_found，不能当性能成功样本（`:4681`）。

### 9. 需要用户/主计划决定的范围

本节记录首次研究提出的范围问题；2026-10-14 最终规划已在 `prd.md` / `design.md` 固定本轮提案，仍待用户审阅。以最终规划为准，不将以下问题理解为另行扩展授权。

1. 原 full 锚点之外，是否加入有限 scale；主 `research/methodology.md` 提案为 10/50/250 文件 × 200 消息，在完整 2k/10k/50k 上查询，再 shrink 到 1k/5k/25k。它们只能记入新诊断区，不能重新称作 legacy full。规模、整轮重复数、暖场次数、超时/RSS/磁盘上限由主计划冻结，本研究没有选定或运行。
2. 本轮 context 的 mainline/full Session 集和 N/B；是否还包含预算截断。mainline/full 的现有能力不需要 Rust 新功能。
3. 是否加入连续无变化扫描、append、合成目录 discovery、少量零命中/稀有查询。它们有诊断价值，但不是当前九项测量的隐含覆盖，不能自动扩大矩阵。
4. schema 选择建议为同报告可选版本化顶层扩展；历史 v1 校验兼容及新字段严格校验同时保留。不要为扩展而改造成另一条旧 `scripts/benchmark.py` 管线。

### 10. 对主方法提案的兼容性检查

已读同任务 `research/methodology.md`（2026-10-14 提案），三完整运行、每规模三 sync trial、查询类/操作各 3 warmup + 100 样本的方向不需要 Rust 更改，但不能只改 `PROFILES` 达成：

- 现有 `final_db` 已 shrink。新尺度查询必须在最后 trial 的 intact DB、noop 后且 shrink 前发生，或使用独立 intact snapshot；前两 trial 可以按原 initial→noop→shrink 完成。旧 full 自身顺序仍保持。
- 五类各 100 = 500 个 measured search 请求，是新扩展数据；不能写回旧 `search_latency_ms` 的 pooled 100。三次 warmup 也只属于扩展 query 协议。Session/ID 采样应覆盖已批准的固定集合，避免旧 show/get 只读首文件的隐含偏差。
- 双时钟选择已解决（最终 `design.md:26`、`:41`；`methodology.md:65`）：共享生命周期在同一 invocation 内记录 process/output 完成、observer cleanup 前的时间，同时保留 cleanup 后的旧 duration。`run_process` 的旧返回语义、Core 九项、gate/semantic 默认行为不变；扩展包含 anchor 与各 scale 的 corrected observation，分别标识 `legacy-wall-with-observer/v1` 与 `process-wall-before-observer/v1`。不创建两个重复 runner，旧报告缺少扩展仍有效。
- `meta.duration_ms` 命名也已解决：最终设计称 CLI pre-render observation，覆盖参数解析、store open/close、dispatch 与写后 wake，不声称仅 application dispatch。
- 250 文件会全部作为单次 sync 的显式绝对路径参数（Core `:403`）；实现应预检真实 argv 可启动性及 temporary-root 布局，避免把启动失败处理成缺失的性能样本。
- 本轮没有估算三整轮是否能在 60 分钟内完成。应遵守方法中的不完整报告/非成功返回约定；不能为满足预算悄悄减少样本、缩小规模或绕过 correctness。

### 11. 最终规划技术复核（2026-10-14）

- Query: 复核当前 `prd.md`、`design.md`、`implement.md`、`research/methodology.md` 的技术矛盾、可复现性和验收缺口。
- Scope: 只读规划与已研究源锚点；无 benchmark、task start、产品编辑或测试执行。

未发现双时钟兼容、完整查询库安排或 context 500-message / 4-MiB 预算存在本身不可实现的矛盾。首次复核的三项意见，经主会话修订后于 **2026-10-14 限定复核关闭**；本次只核对相应修订，没有扩展审查范围。

| 原 Severity | 状态 | 修订后规划锚点 | 核验结果 |
| --- | --- | --- | --- |
| P2 | 已解决 | `design.md:65`、`:71` | Opt-in child 明确从复制的 launcher environment 中移除 `ASG_INDEX_TRACE`、`ASG_CURRENT_REPO`、`ASG_CLOCK_MS`，保留正常 bootstrap；cwd 固定为冻结 source root，记录有效策略、UTC interval 和安全 repo identity；三报告验证策略/repo 一致。独立 trace trial 仅加入其自有 trace target；不改变父环境或非 opt-in helper 行为。 |
| P2 | 已解决 | `research/methodology.md:93`、`:98` | 明确 `M=Q50(x)`、`MAD=Q50(abs(x_i-M))`，使用已存六位小数原始值、无中间 rounding；spread=`100*(max(m)-min(m))/Q50(m)`，仅严格大于 15% 警告。分母/range 都为零时结果零；分母零而 range 为正时记录 null、警告和 `zero_median_with_positive_range`，不输出 infinity。 |
| P3 | 已解决 | `design.md:30` | 已改为 `run_process default/dict contract`，明确保留 `duration_ms`、`peak_rss_mb`、`stdout` 三个旧 key，与现有返回契约一致。 |

附带的目标选择建议也已落实（`design.md:105`、`:110`）：context 通过固定 generator file/record identity 选取，show/get 的 generator-ID 序列跨轮固定，并在测量前映射到各轮实际 wire ID；不依赖临时路径或 canonical-wire-ID 排序。限定复核范围内没有未解决项；这是规划意见关闭，不表示产品实现或性能实验已通过。

## External references / versions

本轮按调度范围仅核对本地当前文件与相关测试，未调用外部文档，也未借外部性能数字推断本机结果。内部契约版本为 Core/Beta `agent-session-grep.core-beta-benchmark/v1`、gate `agent-session-grep.open-source-gate/v1`、semantic report/manifest 各自 v1、Robot 1.1。未复用历史 SQLite/runtime 版本作为当前测量值。

## Related specs

- `.trellis/workflow.md:353`：研究结果必须落盘；本轮保持 planning。
- `.trellis/spec/agentsessions-cli/backend/index.md`：composition root、read/write 打开方式、Robot 输出和真实应用边界；该文件为已有 dirty spec，只读未修改。
- `.trellis/spec/agentsessions-cli/backend/quality-guidelines.md` 与 `.trellis/spec/agentsessions-adapters-sqlite/backend/quality-guidelines.md` 目前是通用占位模板，不能替代产品可执行证据。
- `.trellis/spec/guides/cross-layer-thinking-guide.md:262`、`:310`：身份/CI 证据边界，观察失败与产品失败、deadline 和业务阶段的区分；只读未修改。
- canonical `AGENTS.md`、`CLAUDE.md`、`CONTRIBUTING.md` 的 synthetic/privacy、最小改动和真实验证要求均适用；没有执行 commit/push/merge。

## Caveats / Not Found

- 没有执行 full、smoke、生成器、单元测试、恢复实验或性能采样；没有当前性能结论、瓶颈排序或速度承诺。已有测试只作为当前契约来源。
- 根据研究角色隔离要求未读取 `implement.jsonl` / `check.jsonl`；任务要求来自调度指定路径的 `prd.md` 与本轮明确范围。
- 未发现独立 Core/Beta JSON Schema 文件；当前产品文档加 Python `validate_report` 是该报告的契约与校验入口。Robot schema 不能替代它。
- 尚缺当前源身份/二进制关联、实际 store SQLite 版本、固定环境和重复运行证据；这些由同任务其他研究与最终执行计划处理。
- source tracing 的阶段细节交由主研究；任何内部归因必须另有证据，不能只由总耗时、RSS 或两个中位数相减得出。
