# ASG 主线程证据笔记

## 口径

- 日期：2026-10-05；主项目 HEAD `5b232cdedbff33a251e7fb266558be7af8496e1a`。
- 本轮没有改产品源码。开始时已有的其他任务修改保持不动。
- 静态审读、实际执行、历史报告三类证据分开。文件清点和 CodeGraph 命中不等于全文读完。
- Trellis / 平台集成资产不是 ASG 产品功能，不用其数量攻击产品体积。

## 已执行验证

| 验证 | 实际结果 | 边界 |
|---|---|---|
| `cargo test --workspace --locked --offline --target-dir target-competitive-audit-20261005`，关闭增量编译 | 1758 passed / 0 failed / 20 ignored；80 个 suite summary | Windows 本机、默认 feature；忽略项主要为 fixture 重生成和显式微基准 |
| `cargo clippy --workspace --all-targets --locked --offline ... -- -D warnings` | exit 0 | 默认 feature；不是跨平台认证 |
| `cargo fmt --all --check` | exit 0 | 无格式修改 |
| `node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs` | 11 passed / 0 failed | DOM 交互 harness，不是浏览器视觉/无障碍实测 |
| `cargo test -p agent-session-grep-application --features semantic-candle --locked --offline ...` | 287 passed / 0 failed | feature 代码测试，不等于真实 E5 模型检索质量已经达标 |
| `python -m unittest discover -s scripts -p test_*.py` | 18 tests，1 skipped，suite OK | 纯测试桩中故意出现失败投影，不代表整个 suite 失败 |
| 独立构建的 `asg search needle` | exit 2，要求 --db，并输出“search 可能不是有效命令” | 确认首用摩擦；没有访问真实会话 |

### 不成立的最初判断

首次复用默认 `target/` 跑 workspace test 时出现 E0061，声称 `is_ready` 需要 usize 参数。当前源代码 `ports/src/lib.rs:780-785` 明确无该参数，Git 未显示产品修改；独立 target + `CARGO_INCREMENTAL=0` 全部通过。因此不能把这次缓存/构建目录异常记为 HEAD 编译缺陷。具体缓存异常根因没有完全定位，不擅自归因到某个外部并发进程。

## 已证实问题与建议入口

### ASG-01：对外竞品事实表不可靠（高优先级）

- `docs/product/COMPETITOR-COMPARISON.md:20-33` 把 cass 写成 Python CLI、agentsview 写成 TS CLI、AgentRecall 写成 Python CLI、agent-sessions/agf 写成 Go CLI、claude-historian-mcp 写成 Python MCP。
- 本地实际证据：cass 根 Cargo.toml:1-10；agf src/main.rs:86；AgentRecall package.json 的 electron-vite/TypeScript 与 src/core/session-store.ts；agent-sessions AgentSessions/Search/SearchCoordinator.swift；agentsview internal/db/search.go 与 frontend Svelte；historian src/search.ts 与 package.json。
- `docs/product/OPEN-SOURCE-ROADMAP.md:88-94` 还有“无人做到 pack 级证据契约”“无人做到可验证”“竞品基本无中文分词处理”等绝对化竞争判断，不能仅由本仓库测试证明。
- cass 已存在接入 CLI 的 pack：`src/lib.rs:23592-23609,23794-23863`，调用 plan_answer_pack；预算/去重/遗漏原因/来源/脱敏字段见 `src/search/pack_planner.rs:58-85,372-469`。ctx 的 `crates/ctx-history-search/src/packet.rs:14-74` 有 citation、visibility、分页和截断。Wake `crates/wake-core/src/db.rs:2542-2659` 明确 CJK/substr trigram 与短词 LIKE 路径。
- 结论：不是“ASG 没价值”，而是原有差异化论证不够可信，必须重新逐条取证。各项目 pack 语义不完全相同，不能反过来宣称完全等价。

### ASG-02：从源码构建 + 每次指定 DB，把内部概念推给首用用户

- README:28-61 要从 checkout 构建，手动 PATH，查询必须 --db。
- CLI `src/lib.rs:1680-1718` 缺 DB 的真实错误包含“已知合法 search 也可能不是有效命令”。
- 当前 `--help` 的“新手从这里开始”优先展示 search/show/context，未先完成 sync/建立库；`list` 列的是 catalog 实体而非以会话为中心的产品列表。
- 推荐默认路径解析与显式 override，首次使用提供明确的“发现→确认→同步→首条结果”路径；不得为了方便静默扫描所有私人目录。机器协议及现有 --db 行为要有兼容测试。

### ASG-03：交付状态没有兑现产品路线的目标

- README:11-12 明确无 release tag/公开产物；Cargo workspace 0.1.0。
- README:83-107：14 implemented + 2 deferred，全部 experimental；不能因此断言 parser 不工作，但也不能把“14 支持”当作 14 家同等可靠。
- `docs/product/OPEN-SOURCE-ROADMAP.md:27-37,60-64` 的跨平台 hosted CI、发布/签署、provider 晋级仍未闭合。它是历史记录，不代表现在 GitHub 账单仍然是同一状态；本轮未在线核验账户状态。
- `.github/workflows/ci.yml:19,42-83` 实际配有三 OS、semantic feature、Web 和 Python 测试。问题不是没有流水线，而是“有配置”和“有可追溯成功 release 证据”不是一回事。

### ASG-04：语义能力的默认语义与用户直觉仍有鸿沟

- README:73-78 清楚承认默认是 bigram-hash fuzzy lexical，真实 Candle E5 feature 默认 off、离线导入。
- 默认二进制仍提供 `index embeddings` 和 semantic/hybrid 模式；用户容易把向量路径等同真实语义。当前诚实标签是优点，不应删除；建议在 CLI/help/model status/结果头让 effective capability 一眼可见，而不是只在长文档里解释。
- `scripts/evidence/semantic_benchmark.py --help` 明确约 2000 消息/200 会话/100 查询、合成近重复与改写金标，threshold_pending。不能说项目“没有语义 benchmark”；也不能说已有脚手架就证明语义质量。

### ASG-05：向量检索仍是过滤后精确全扫，需用规模实验决定后续方案

- SQLite adapter `src/lib.rs:8172-8200` SQL 先按模型、维度、metadata/facet/visibility 筛选；`8201-8229` 对每条候选解码并计算 cosine，BinaryHeap 只保留 top-k。
- 优点：不是先取 top-k 后过滤，内存不保留全部 hits，坏向量 fail-closed，已删除 catalog 行通过 JOIN 排除。
- 风险：候选数 × 向量维度的 CPU/读取成本仍线性；不是看到向量就该上 ANN，而应先测 1万/10万/100万消息和过滤选择性。
- 计划必须保留过滤/删除/分页正确性，不可用预截断候选换漂亮延迟。

### ASG-06：大文件与手写参数元数据重复有真实维护成本

- SQLite `src/lib.rs` 19287 行，测试开始约 8520 行，不能把所有 1.9 万行都叫生产代码；但前 8 千余行仍承担 schema/迁移、写入、关系投影、查询、向量等多职责。
- CLI lib.rs 约 5 千行生产路径后接内联测试；run/dispatch/scan/ingest/render 聚集。
- `.trellis/spec/agentsessions-cli/backend/index.md:29-35` 自己要求每个新 flag 注册到多个 prefix scanner；这是需要回归保护的同步点。该规范禁止直接引入 clap，整改不能偷偷违背已有契约。
- 建议按稳定职责提取模块，保留单一 Application ADT 和端口边界；先做参数元数据单源/一致性测试，再考虑解析器选型。禁止“换框架等于重构完成”。

### ASG-07：性能/可靠性证据需要明确规模与时效，不能说“已登顶”

- 历史 `docs/evidence/core-beta/88d86f4/core-beta-benchmark-full.md:5-12` 只有该提交、Windows、4000 合成消息的本地锚点，不是当前 HEAD，也不是正式 SLO。
- 历史 noop P95 约 1809.8ms、initial throughput P50 0.566 MB/s、store/source 5.11；本轮正在复测，不能直接拿旧数字判当前产品。
- `docs/product/SLI-AND-BENCHMARK-FORMAT.md:5-13,78-95` 仍 Draft，标准规模 100k sessions/10m events/50GB 是专项压测目标，不是已经跑过。
- 需要检索质量/任务完成率、端到端首用和版本漂移三类证据，不能只统计单元测试数量。

## 不应为了锐评而砍掉的已实现优势

- 源读取：`source_fs.rs:125-145,161-218` 捕获范围、metadata + fingerprint 前后验证，SQLite 源有独立快照路径；不把源字节随意修改。
- 语义过滤、删除可见性和坏向量处理已在生产查询实现与 workspace tests 中体现。
- 稳定 Message / contextual Placement、显式 relocation、cursor generation/时间约束和脱敏分界有实际代码与测试，不是只有 RFC。
- 五个界面共享 Application 契约是可保留的架构资产。不能因为五个界面就建议重写五套后端。

## 仍待补齐

当前文件是进行中证据笔记。15 项完整对标报告、当前 release benchmark、其他 provider 逐契约审读、复现/覆盖清单和最终计划仍须汇总，不宣称全量逐行审读已经完成。

## 新增实测：当前 HEAD 的完整 Core benchmark 与热路径根因

当前 release 二进制在独立 target、`--locked --offline` 下构建成功，耗时 1m19s。Core full harness 在本机验证通过，4000 消息、20 文件、1,122,164 源字节；dataset SHA-256 与 88d86f4 历史基线完全相同。CPU、RAM、Rust 版本、Windows target、NTFS 一致；存储/Defender 描述文字不同但同为 Samsung NVMe + 实时保护开启。跨日期非完全受控，不直接宣称严格因果回归。

| 当前指标 | P50 | P95 |
|---|---:|---:|
| warm startup | 7.04ms | 8.01ms |
| initial sync | 917.10ms | 1059.32ms |
| noop sync | 15.49ms | 16.22ms |
| shrink sync | 728.60ms | 742.80ms |
| search | 114.50ms | 162.90ms |
| show | 53.96ms | 59.78ms |
| get | 54.43ms | 62.90ms |

Store/source 为 17.83，约 20.01 MB 存储 / 1.12 MB 合成源；release artifact 6.92 MB。这不是 50GB 数据集结果，不能外推无限线性。SQLite 3.53.2 由同 HEAD 的既有 WAL spike 实测，两个 lock 的 rusqlite 0.40.1 / libsqlite3-sys 0.38.1 版本和 checksum 一致；没有假装通过被测 CLI 的 SQL 接口查询过版本。

### ASG-08：App 组合构造触发重复且无关的 Git 子进程（已实测，P1）

- `cli/src/lib.rs:3058-3099`：`resume_app`/`resume_semantic_app` 每次构造都注入 `current_repo_slug()`；每次临时新建 GitRepoSlugResolver，缓存不跨 App 实例保留。
- `cli/src/repo_identity.rs:31-54,146-167`：有 origin 的 Git 仓库需要 `git rev-parse` + `git remote get-url` 两次外部进程。
- `cli/src/lib.rs:2082-2089` 为取当前时钟构造一次 App，`2145-2161` 真正 search 再构造一次。因此通常一次搜索重复探测两轮（四次 Git）。
- `cli/src/lib.rs:2365-2390` 的 get/show 也调用该构造器，尽管按 ID 读内容不需要当前仓库排序信号。
- 配对实验：同一个合成 catalog 条目、相同二进制、固定时钟，真实 Git 探测 vs 已有 `ASG_CURRENT_REPO=''` 测试注入；各 20 次，返回 data 全部相同。中位数 get 53.665→9.152ms，show 52.438→8.969ms，search 97.112→9.982ms。原始样本见 `repo-probe-overhead.json`。
- 结论：额外进程开销是实际热路径浪费；不是 SQLite 查询本身的责任。测试注入仅用于隔离变量，不建议用户永久关闭 repo 排序。
- 整改：按请求构造一次上下文；先直接读 clock 再建 App；只在需要 repo-aware 检索的用例解析该信号；共享一次解析结果并保持无仓库/无 origin 语义。验收要测试 Git 调用次数与排序数据一致，而不只测试耗时。
- 已排除的假设：`SqliteStore::open`（1517-1539）是 read-only open + user_version + scalar function 注册，未发现每次 open 全库审计，不能把成本归咎于不存在的全库检查。

### ASG-09：能力计数不能等价用户闭环计数

独立构建的 `asg --robot providers` 实测已保存到 `asg-provider-matrix.json`。14 已实现 provider 的 resume 为 8 derived / 3 unknown / 3 unsupported；Aider、Cursor discover 为 unsupported，需要显式输入源；tool_activity 仅 Claude/Codex partial，其余已实现 provider 为 unsupported。

`context` 字段在 `ports/src/capability.rs:84-85` 指的是上下文图能力，不能把 unsupported 偷换成“完全不能展示该 provider 的平面会话”。矩阵是一项诚实资产，应改进用户可理解的分层支持说明，不是篡改等级让数字更好看。

## 新增实测：默认检索质量与 durable journal 体积

### ASG-10：默认“semantic”向量路线没有形成质量优势（限定语料）

已执行并通过 semantic benchmark validator：当前 release、2000 合成消息 / 200 会话 / 100 query、固定 manifest hash `411cba9fa287542b30c3dd64555199d0546bc22889175b3ec3444e4a6f0332dc`。Recall@10：lexical 0.750，bigram-hash 的 semantic 0.475，hybrid 0.755。向量 projection 增量约 2.14 MB。报告明确 `is_real_embedding_model=false`、`threshold_pending=true`、`promotion_claim=none`。

这只能证明默认 hash 路线在这组冻结合成查询上不具备语义优势；不能据此否定真实 Candle E5 模型，后者本轮没有加载权重。核心建议是把 fuzzy lexical 与真实 semantic 的产品命名、可用性、文档/结果投影和验收分清；不能仅因为“跑通向量/混合”就认为用户自然语言问题已解决。

### ASG-11：普通同步保留大体积 activated manifest，当前状态不变也会累积（P1）

- 4000 消息样本的只读逻辑字段统计：`index_batches` 一行的 TEXT/BLOB 总计约 3,886,680 bytes，`catalog` 4040 行约 2,491,624 bytes。这里只是逻辑字段长度，不是物理页占比。
- Python SQLite 3.40.1 未启用 DBSTAT，不能假装测得物理分表体积；已核对官方 `https://www.sqlite.org/dbstat.html` 的编译开关条件。没有安装扩展或修改 SQLite 构建。
- 更直接的重放实验：单个 200-message 合成文件，56,320 bytes；每轮仅改最后一条消息的固定长度 revision，连续 sync 21 次。catalog 一直 202 行；activated batch 从 1→21；journal manifest 字符数从 194,315→4,739,455；store（含存在的 sidecars）从 1,368,064→6,066,176 bytes。原始逐轮记录见 `journal-growth.json`。
- 这不是“删了数据 SQLite 没缩文件”的猜测：journal 内仍有具体、持续增长的 activated 记录。也不能把 21 次实验外推成任意规模的精确增长曲线。
- 源码：SQLite `lib.rs:969-1044` 序列化 upsert/delete ID、relations/source replacements 等 manifest；`1048-1061` 是 durable outbox 行；`6972-6987` 的 recovery 只把 building 改 aborted，`7007-7027` 读取完整历史 manifest。
- 不能为了瘦身删除整个 outbox。先定义恢复/idempotency/诊断必需的信息和 terminal batch 的保留策略，再讨论 compact/GC 或减少重复 relation manifests。必须保留 building 操作、generation/digest 核验和 crash-consistency 测试。
- 建议加入固定当前数据集的反复编辑、仅追加、分支修订三类 soak test，观察“当前 catalog 体积”与“历史 journal 体积”，而不是只测首次导入。

### ASG-12：恢复成功指标必须保留语义限定

`scripts/evidence/open_source_gate_benchmark.py:284-299` 明确 resume_handoff_success 只测 dry-run preview 生成命令及 handoff 有 evidence，不启动真实 provider；`328-335` 用 `available && command && !executed` 计成功。脚本注释是诚实的，不能在路线宣传里把这个比率转述成“真实恢复执行成功率”。应拆分 preview、native resume smoke、handoff pack validity、接收 agent 任务完成率四个指标。
