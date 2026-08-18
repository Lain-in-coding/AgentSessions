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

### 1.4 MCP amortized 语义延迟(已实测)

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
| 2 | 全仓库 | **ingestion 完全单线程**。`grep thread::spawn` 只在 `serve.rs` 命中,解析路径零并行 | 0.566 MB/s 的主因;多核未利用 |
| 3 | `adapters-sqlite/src/lib.rs:1365` | FTS5 是**普通表非 external-content**:`fts5(id UNINDEXED, text)`,文本在 catalog 和 fts 各存一份 | 存储放大 5.11 倍的直接来源 |
| 4 | noop sync 1.5 秒 | 无变化重扫仍要 1.5 秒(4000 条) | 10 万条下线性放大,交互式使用不可接受 |

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

### 1.9 外部阻塞项

| 项 | 状态 | 影响 |
|---|---|---|
| GitHub Actions | **全线阻塞**:所有 job 5 秒内 0 步失败,账户级 "recent account payments have failed" | 无法自动防性能回退、无法跨平台测试、release 流水线无成功 named run |
| macOS 干净环境演练 | 未跑(owner 在 Windows 11) | 三平台演练不完整 |
| 代码签名/公证/cosign | 无凭据 | 无签名二进制 |
| 12 个 provider 真实样本 | 未收集 | provider 无法诚实升 Beta |

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

- **Q16 搜索默认作用域** — owner 反问得很关键:"如果默认只在本项目搜索,
  那我有上下文我直接问 AI 我还用得着通过 asg 搜索吗?" 这个质疑成立,
  说明**默认只搜当前项目是错的**。倾向方案:**默认全库 + 同项目排序加权 +
  每条结果显示来源项目**,收窄用 `--project`。正在调研 13 个同类项目与
  atuin 的实际做法(atuin 有 session/directory/host/global 循环过滤模式,
  且默认档位改过 —— 那个改动方向是最强信号)。调研回来后确认再落 ADR。
- **Q9 CI 方案** — owner 问了三件事:能否注册多个 GitHub 账号(需查 ToS 与
  封号风险)、CI 是否必须绑一个账号(**关键:公开仓库的 Actions 分钟数是否免费
  无限 —— 若是,则转公开本身就解锁了 CI**)、能否从 linux.do 论坛买 CI
  (需评估凭据泄露与供应链风险)。调研中。
- **provider 真实样本获取路径** — 16 个 provider 逐个给"自己生成/花钱买/
  拿不到"的判定与操作步骤。调研中。

## 3. 发布门(全部为真才把仓库转 PUBLIC 并发 v0.2.0)

逐条可验证,不留主观判断:

| 门 | 判据 | 验证命令/证据 |
|---|---|---|
| G1 | Claude Code + Codex 用真实语料按 D9 标准验证通过,并各有一份合成 golden fixture 在仓库里 | `cargo test --workspace` 含 golden;`M1` 的 owner 授权真实运行报告(聚合) |
| G2 | 至少 5 个 provider 达到 D9 标准并升为 Beta;其余诚实标注 | `capability.rs` 矩阵 + 漂移测试;`PROVIDER-MATURITY-MATRIX.md` |
| G3 | 四个动作在 10 万条合成语料上全部达阀,且阀值绑进自动化 | `open_source_gate_benchmark.py` 报告 `all thresholds pass` |
| G4 | 首次全量索引 1 GB ≤ 5 分钟 | 同上,`initial_index_throughput_mb_s ≥ 3.5` |
| G5 | 七条 Gate D 不变量在真实冻结快照上全绿 | `real_data_regression.py` 退出 0 |
| G6 | 五入口一致性 harness `overall_verdict: consistent` | `compare_entrypoints.py` |
| G7 | 零出网静态审计通过;`--offline` fail-closed | `cargo test -p agent-session-grep-cli --test network_egress` |
| G8 | 跨边界脱敏覆盖;合成 secret fixture 不外泄 | 现有脱敏 e2e |
| G9 | 数据生命周期三维度可用且有测试 | `M3` 交付 |
| G10 | 公开树 public-profile 隐私扫描 0 findings | `privacy_scan.py --profile public`(经导出器) |
| G11 | 文档:quickstart 可照抄执行、每 provider 有"记录在哪/解析不了什么"表、故障排查手册、MCP 客户端接入实例、CLI reference 完整 | `M4` 交付 + 人工走一遍 |
| G12 | 产品内 UX:零配置可用、每条错误给下一步命令、首次运行向导、shell 补全 | `M4` 交付 |
| G13 | 无过时/自相矛盾声明(含"六条 vs 七条"、Gate D pending、discover 声明与实现不一致) | `M4` 交付 |
| G14 | Windows + Linux 完整验证;macOS 诚实标注 | 本地 + WSL2 执行记录 |
| G15 | 全部质量门绿 | §0 第 5 条的七条命令 |

**明确不阻塞公开**(诚实标注 + 建 issue 跟踪即可):macOS 干净环境演练、
代码签名/公证/cosign、12 个 provider 全部升 Beta、分支保护、
ADR/REUSE 正式签字、SBOM 认证。

## 4. 里程碑与任务

顺序按 D16:**M0 → M1 → M2 → M3 → M4 → M5**。M0 可与 M1 并行(纯清理)。
每个任务独立成 worktree + 分支,完成后合回 `main`(私有仓库)。

---

### M0 — 收口当前遗留(小而急,先清掉)

- [ ] **M0-1 从公开树排除发布工具链(D18)**
  `scripts/release/export_public_tree.py` 的 `EXCLUDED_PREFIXES` 增加
  `scripts/evidence/privacy_scan.py`、`scripts/evidence/test_privacy_scan.py`、
  `scripts/release/export_public_tree.py`、`scripts/release/test_export_public_tree.py`。
  注意:排除机制目前是**前缀**匹配,需要支持精确文件排除。
  同时修 `export_public_tree.py:124` 的自豁免 bug:它 `continue` 跳过了
  `MANIFEST_NAME`,导致它唯一写的那个文件是唯一不被扫的文件 ——
  改为扫描 manifest,或干脆不发布 manifest。
  **验收**:导出后这四个文件不在公开树;public-profile 扫描 0 findings。

- [ ] **M0-2 `PUBLIC-TREE-MANIFEST.json` 不进公开树**
  它含 `excluded_prefixes`(泄漏内部目录布局)、`source_commit`(指向公开
  读者无法解析的私有 SHA)、482 行 SHA-256(对读者零价值)。
  **验收**:公开树无此文件;导出仍在本地写 manifest 供校验。

- [ ] **M0-3 修 SECURITY.md 支持版本表**
  当前写 "0.1.x | 不支持 / Published releases | None",但 `v0.1.0` 已发。
  改为 `0.1.x | Supported`,删掉 "None" 行。
  **验收**:与 CHANGELOG、README 的版本陈述一致。

- [ ] **M0-4 `go-no-go.2026-08-16.md` 与 `go-no-go.template.md` 不进公开树**
  它是内部决策草稿,结论是 **No-Go**,还带 7 条 P0 缺陷清单
  (含 "serve token generator not CSPRNG"、"Web/JSON boundary leaked secrets")
  和未签名的签字块。公开读者下载 v0.1.0 会看到维护者自己判定不该发。
  保留 `rehearsal-runbook.md`(是真有用的流程文档)。
  **验收**:公开树无 go-no-go;README/CHANGELOG 不再引用它。

- [ ] **M0-5 `PUBLIC-HISTORY-SCRUB.md` 不进公开树**
  公开仓库只有 1 个 commit,历史重写手册在结构上不适用;而它公布了
  "45 个 commit 带路径痕迹、6 个 commit 含 secret 形状"的度量表和
  `git log -S"<username>" --all -p` 这样的搜索配方。
  **验收**:公开树无此文件。

- [ ] **M0-6 `R0-ARCHITECTURE-REVIEW.md` 不进公开树**
  标题写"待签署",正文写"Review status: Pending — not approved"、
  "此前 R0 Gate 整体保持未通过",还有 15 个未勾选决策框,其中一条是
  "对先实现后审批作显式 exception" —— 等于公开承认实现跑在架构审批之前。
  内容已被各 ADR 取代。
  **验收**:公开树无此文件;引用它的地方改指 ADR。

- [ ] **M0-7 治理文档的 owner 字段统一**
  18 个文件写 `owner: （待指派）`、`approver: 项目最终验收人`、`approved_at: —`
  (全部 ADR 0001-0009、RFC-0001/0002、CLI 契约、THREAT-MODEL、
  FIXTURE-REDACTION-POLICY、REUSE-LICENSE-AUDIT、SLI-AND-BENCHMARK-FORMAT、
  external-readiness-gate、search-backend SPIKE-CARD)。
  ADR-0009-cross-boundary 和 ADR-0010 已填 `QIN`,同目录内不一致更难看。
  统一填 `QIN`。
  **验收**:`docs/` 内无 `（待指派）`。

- [ ] **M0-8 `CONTEXT.md` 决策日志段落处理**
  第 174-346 行是内部决策日志,其中 `:199`/`:284-285` 写"仓库保持 PRIVATE"、
  `:176` 写"完成 0.3 后再决定发布"(已发 0.1.0)、`:259` 有编辑事故
  (标题与正文粘连)、`:319` 有未翻译中文混在英文句中、`:243`/`:245` 有中文
  表头在英文文档里。保留 §Language 术语表(第 1-171 行,是 `AGENTS.md:113`
  指向的东西,质量很好),砍掉决策日志或把耐久部分移入 ADR。
  **验收**:术语表完整保留;无 PRIVATE 陈述;`:259`/`:319` 修好。

- [ ] **M0-9 `OPEN-SOURCE-ROADMAP.md` 标题与章节修复**
  标题 `开源登顶路线图`(字面意思"登上开源榜首")与项目其余部分的保守克制
  完全相反,改成中性的"开源交付路线图"。章节编号从 `## 0.1` 直接跳到 `## 2`,
  缺 `## 1`,第 39-53 行是孤立散文。两处 "无人做到" 的无限定断言
  (`:59`、`:63`)加上"在 2026-08-14 调研集合内未观察到"的限定。
  `§0.1` 是带 `NOT_READY_EXTERNAL_BLOCKERS` 判定的过时内部快照,删或重写。
  **验收**:章节编号连续;无绝对化竞品断言。

- [ ] **M0-10 `INSTALL-AND-UPGRADE.md:145` 修 uninstall 调用**
  写着 `sh scripts/install/uninstall.sh`,但该脚本是
  `#!/usr/bin/env bash` + `set -euo pipefail`,同一文档 `:61-63` 自己警告过
  Debian/Ubuntu 的 `sh` 是 dash 会失败。改成 `bash`。
  **验收**:与 `rehearsal-runbook.md:368` 的写法一致。**这条会真实影响用户。**

- [ ] **M0-11 CLI 输出移除内部编号**
  `scripts/verify-release.py:246` 打印 `agent-session-grep release verification (#10)`,
  `scripts/benchmark.py:2` 是 `"""agent-session-grep benchmark harness (#9)."""`。
  `(#9)`/`(#10)` 是内部任务号,出现在用户可见输出里。
  **验收**:输出无内部编号。

- [ ] **M0-12 ISSUE_TEMPLATE 版本占位符修正**
  `bug-report.yml:43` 与 `feature-request.yml:35` 的 `placeholder: v0.3.0 or 0.3.x`
  (0.3 是内部里程碑编号,已发布版本是 0.1.0)。且两者把 40 位 commit SHA 设为
  `required: true`,用发布版二进制的报告者填不出来。
  改 placeholder 为 `v0.2.0 or 0.2.x`,commit 改为可选或注明"仅开发构建"。
  **验收**:模板可被真实用户填完。

- [ ] **M0-13 spike 卡片断链修复**
  `spikes/search-backend/SPIKE-CARD.md:100-102` 引用了不存在的
  `report.md`、`selection-gate.md`、`fts5/`、`tantivy/` 目录
  (实际是扁平的 `src/fts5.rs`、`src/tantivy_be.rs`)。
  `spikes/sqlite-source-identity/` 缺 `SPIKE-CARD.md`,但
  `spikes/R0-EVIDENCE-SUMMARY.md:93` 声称"四个原本只有 Evidence 的 Spike
  已补 Spike Card"并列了五个。
  **验收**:所有 backtick 路径解析成功。

- [ ] **M0-14 数字一致性修正**
  - 七条不变量 vs 八处文档写"六条"(`CONTEXT.md:160` 是术语表定义,最该修)
  - error catalog 实际 14 码,`CONTRACT-cli-robot-mcp-draft.md:49` 写 13 码,
    `OPEN-SOURCE-ROADMAP.md:65` 写 "13+ 码"
  - `core-beta-evidence-matrix.md:40` 写 "12 tests",实际 14 个 `def test_`,
    运行报告 10 个(1 skip)
  - `COMPETITOR-COMPARISON.md:52-53` 的 12/13 算术不闭合
  **验收**:每个数字与代码/schema 一致,或改为引用 schema 不写死数字。

- [ ] **M0-15 semantic 证据自相矛盾修正**
  `SEMANTIC-MODEL-BUNDLE.md:140` 写 "semantic recall@k **tracked at/below
  lexical**",而 `go-no-go.2026-08-16.md:137` 写 "**semantic ≥ lexical**"。
  按 §1.5 实测:semantic 在 @5/@10/@20 全部 ≥ lexical(0.720/0.755/0.775 vs
  0.705/0.750/0.750),所以 `SEMANTIC-MODEL-BUNDLE.md` 那句是错的。
  同时:`:124-126` 的加粗 "Real inference HAS been verified in this
  environment" 去掉喊话语气;`:148` 的 "p50 16.6 ms / p95 21.0 ms" 必须就地
  标注 n=10、语料 10 条合成消息,否则是引人误解的 benchmark 数字。
  **验收**:两处方向一致且与报告 JSON 相符;延迟数字带样本量限定。

- [ ] **M0-16 `docs/performance-baseline-0.2.md` 重命名与限定**
  文件名和标题写 0.2,但那时还没有 0.2;`:80-81` 的 "16.5s → 2.79s"、
  "25.3s → 2.87s" 是开发机真实语料的加速比,摘要行没写语料。
  **验收**:文件名与版本对应;每个数字带语料说明。

- [ ] **M0-17 `spikes/sqlite-source-identity/EVIDENCE.md` 竞品拆解处理**
  `:131-180` 点名 `jhlee0409/claude-code-history-viewer`(MIT, v1.22.0)并
  判定其方案劣于本项目,依据是"临时下载后 grep,下载物已删除"。
  公开点名评判个人项目会招致审视。技术发现(SQLite provider 的行级身份)
  有价值,保留;删掉"与 CCHV 的对照"和"CCHV provider 存储分档"两节,
  或压成一句中性陈述。
  **验收**:无对第三方项目的优劣判定。

- [ ] **M0-18 `spikes/R0-EVIDENCE-SUMMARY.md:90-94` 删杀软逸事**
  记录了开发者的杀软反复静默删除 `EXTERNAL-READINESS-GATE.md`、改成全小写
  文件名后才留存。诚实但读起来像环境不稳 + 轻度绕过 AV。
  保留 `SLI-AND-BENCHMARK-FORMAT.md` 里关于为何记录 `antivirus_state` 的
  那一句实质内容。
  **验收**:附录删除。

- [ ] **M0-19 机器与私有语料细节脱敏**
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

- [ ] **M0-20 CI 证据的不可达引用处理**
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
  **验收**:`git status` 在放入样本后仍干净;`git check-ignore` 确认命中。

- [ ] **M1-2 冻结真实语料快照(D7)**
  把当前 provider 数据根整体拷到 `evidence-input/frozen-corpus-2026-08-<dd>/`,
  记录清单(文件数、总字节、每文件 SHA-256)到**同目录**的清单文件
  (不进仓库)。后续所有真实回归跑这一份。
  **验收**:清单可复算;`real_data_regression.py` 能指向该快照运行。

- [ ] **M1-3 样本采集执行(D21)**
  按调研产出的获取路径,逐个 provider 拿真实 transcript。优先级:
  1. Claude Code、Codex — 已有(owner 本机)
  2. 国内可及且易得的:kimi-code(Moonshot)、qoder(阿里)、
     tencent-codebuddy(腾讯)—— 这三家国内注册付费最顺
  3. 开源 / BYO key 可自建的:aider、cline、opencode
  4. 其余按调研判定
  每个 provider 只需**一次约 5 分钟的真实会话**即可产出格式样本 ——
  模型质量无关,只要磁盘格式真实。可用 Ollama / DeepSeek API / Moonshot API
  等便宜后端。
  **验收**:每个采集到的 provider 在 §7 记录"已采集 + 落点 + 采集方式"。

- [ ] **M1-4 逐 provider 解析无损验证(D9)**
  对每个有真实样本的 provider:用真实文件跑 sync,验证
  session/message/父子链/时间戳/角色全部正确;记录 emitted vs persisted;
  跳过的行按 D11 计数上报。发现的缺口写进 `capability.rs` 的
  `known_limitations`。
  **验收**:该 provider 的真实运行 emitted == persisted 或差额有明确解释。

- [ ] **M1-5 反推合成 golden fixture(D9)**
  按真实样本观察到的**形状**手写一份合成 fixture(虚构项目代号、假 UUID、
  占位路径),配 `PROVENANCE.md` 说明它模仿的真实结构与固定种子/BLAKE3 哈希。
  用 insta 快照(D13)固定归一化结果。
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

- [ ] **M1-8 修 discover 声明与实现不一致(§1.8)**
  `capability.rs` 只给 claude-code/codex 声明 `discover: Native`,
  但 `main.rs:2972-2985` 实际支持 6 个。要么升那四个的声明,要么
  把矩阵注释改成解释"注册了 root ≠ discovery 能力已认证"。
  `PROVIDER-MATURITY-MATRIX.md:63` 那句自相矛盾(说"仅 claude-code/codex 注册"
  又说"antigravity/opencode 已加入")必须改。
  **验收**:三处(代码、矩阵、SKILL.md)一致。

---

### M2 — 性能(D2/D3/D4,四个瓶颈全修)

- [ ] **M2-1 合成语料生成器扩到 10 万条(D5)**
  扩展现有生成器,产出 10 万消息 / 5 千 session / 6 provider 的确定性语料。
  必须:固定种子、可复算 SHA-256、CRLF 归一化(已有
  `normalized_tree_hash` 先例)、长度分布与 CJK 比例贴近真实观察。
  **验收**:两次生成哈希一致;可进 CI。

- [ ] **M2-2 四个动作的阀值 harness(D3)**
  扩展 `open_source_gate_benchmark.py`,把四个动作都变成带阀值的门:
  | 动作 | 阀值 | 依据 |
  |---|---|---|
  | 首次全量索引 | 1 GB ≤ 5 min,即 ≥ 3.5 MB/s | D4 |
  | 增量同步(无变化) | ≤ 1 s @ 10 万条 | 现在 4000 条要 1.5 s,必须改善 |
  | 搜索 | p95 ≤ 50 ms @ 10 万条 | 交互式"瞬时"感 |
  | MCP 单次调用 | p95 ≤ 50 ms(编码器常驻) | 已实测 21 ms,守住 |
  **验收**:harness 输出每项 threshold + pass/fail;报告 schema 有版本号。

- [ ] **M2-3 ingestion 并行化(瓶颈 #2,D20)**
  用 `std::thread::scope` + channel(不加正式依赖):解析在多线程,
  SQLite 写入保持单写者(SQLite 的正确做法)。按
  `std::thread::available_parallelism()` 定并发度。
  **验收**:吞吐 ≥ 3.5 MB/s;七条不变量仍全绿;确定性输出不变
  (同一输入产出同一 catalog)。

- [ ] **M2-4 修 semantic 全表扇描(瓶颈 #1,D14)**
  `query_semantic` 改为分层检索:先用 FTS 召回候选集(比如 top 500),
  只对候选集算余弦。这符合现有架构(hybrid 已用 RRF 融合两路),零新依赖。
  **验收**:10 万条下 semantic p95 ≤ 50 ms;recall 不低于当前
  (与 §1.5 基线对比,不得回退)。

- [ ] **M2-5 存储放大治理(瓶颈 #3)**
  评估 FTS5 改 external-content(`content=` 指向 catalog),消除文本双存。
  这是 5.11 倍放大的直接来源。注意:external-content 需要维护触发器,
  且与现有 `fts_rowid` 边车、rebuild 路径、generation 机制都要对齐 ——
  **改动面大,先写 spike 验证再动生产代码**。若评估后风险大于收益,
  记录理由并只做增量优化(比如只索引不存原文的字段)。
  **验收**:放大比下降且七条不变量全绿;或有明确的不做决定 + 理由。

- [ ] **M2-6 增量同步优化(瓶颈 #4)**
  4000 条无变化重扫 1.5 s,10 万条会线性放大。现有 fingerprint skip 实测
  0.089 s 是**单文件**路径,批量路径没走到。找出 1.5 s 花在哪
  (可能是每次都全量读 catalog 或重建游标)。
  **验收**:10 万条无变化重扫 ≤ 1 s。

- [ ] **M2-7 首次索引进度反馈**
  即使压到 5 分钟,5 分钟无输出也是坏体验。加实时进度
  (已处理文件数/总数、当前 provider、已入库消息数),支持 Ctrl-C
  中断后可继续(增量语义已有)。
  **验收**:进度可见;中断后重跑不重复入库。

- [ ] **M2-8 真实冻结快照回归(G5)**
  在 M1-2 冻结的快照上跑 `real_data_regression.py`,七条不变量全绿,
  记录聚合数字(D5:只提交聚合)。与 §1.6 的历史数字对比确认无回退。
  **验收**:harness 退出 0;`docs/evidence/` 记录聚合结果。

---

### M3 — 功能补齐(D15 全部补齐,D12 数据生命周期)

- [ ] **M3-1 `--db` 默认路径(D8)**
  默认落到平台数据目录(`asg config paths` 已能算出),`--db` 降为覆盖项。
  严格对齐 ripgrep/gh/atuin 的零配置习惯:装完就能用,不需要先读文档。
  旧命令(显式 `--db`)必须全兼容。
  **验收**:`asg search "x"` 在全新环境无参数可跑;所有现存测试仍绿。

- [ ] **M3-2 数据生命周期三维度(D12)**
  - `asg prune --before <date>` — 按时间
  - `asg forget --project <path>` — 按项目(离职/交接/敏感客户项目整体抹除)
  - `asg prune --provider <id>` — 按 provider
  - `asg forget <session-id>` — 单会话(粒度最细,删误入的敏感会话)
  - `asg uninstall --purge` — 连数据一起删
  全部默认 dry-run 打印将删除什么,`--yes` 才执行。删除必须同时清
  catalog、fts、fts_ids 边车、message_vec、tool_activities 等所有表。
  **验收**:每个维度有测试;删除后 `doctor` 报告一致;rebuild 后不复活。

- [ ] **M3-3 搜索作用域(待 owner 确认,见 §2.1 Q16)**
  倾向方案:默认全库 + 同项目排序加权 + 每条结果显示来源项目,
  `--project` 收窄。**等调研结论与 owner 确认后落 ADR 再实现。**
  **验收**:ADR 落地;实现与 ADR 一致;结果显示来源项目。

- [ ] **M3-4 审计列出的其余功能缺口**
  待功能缺口审计 agent 回报后填充。已知方向:过滤维度、输出格式、
  多项目处理、导出、session diff。按 D15 全部补齐。
  **验收**:逐项有测试。

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
  覆盖:找不到 provider、库为空、transcript 解析失败、路径错、
  权限问题、DB 锁、索引中断后如何继续。每条给具体命令。
  **验收**:每个错误消息在手册里能找到对应条目。

- [ ] **M4-4 MCP 客户端接入实例**
  给具体客户端(Claude Code、Claude Desktop、其他 MCP 客户端)的真实
  config JSON、9 个工具的说明与参数、可照抄的调用示例。
  这可能是本工具最有价值的入口。
  **验收**:照抄 config 能接上;每个工具有示例。

- [ ] **M4-5 完整 CLI reference**
  每个子命令、每个 flag、退出码表(14 码 error catalog 对齐)、
  Robot JSON schema 指引。
  **验收**:与 `--help` 和 schema 一致。

- [ ] **M4-6 产品内 UX(G12)**
  - 每条错误消息给下一步可执行命令(现在很多只说"失败了")
  - 首次运行向导:检测到哪些 provider、建议第一条命令
  - `asg doctor` 改成引导式诊断,不只是打印状态
  - shell 补全(bash/zsh/fish/PowerShell)
  - `--help` 可跳读、术语与文档一致
  **验收**:人工走一遍新用户路径无卡点。

- [ ] **M4-7 真实录屏演示**
  asciinema 或 GIF,60 秒内展示:装、索引、搜到三周前的东西、resume。
  **验收**:README 顶部可见。

- [ ] **M4-8 全部过时/矛盾声明清理(G13)**
  M0 已修一批,这里做最终一遍:七条不变量、Gate D 状态、
  discover 一致性、版本陈述、semantic 方向、error catalog 码数。
  **验收**:交叉检查脚本或人工清单确认零矛盾。

---

### M5 — 转公开与发布 v0.2.0

- [ ] **M5-1 发布门逐条核验(§3 G1-G15)**
  每条给出验证命令与结果。任何一条不过就不往下走。
  **验收**:15 条全绿的记录。

- [ ] **M5-2 最终对抗性审计**
  重跑本轮用过的多路审计:真实数据泄漏、内部过程泄漏、许可证归属、
  语义性内部叙述、断链、自相矛盾、过度声明。
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

- [ ] **provider 真实样本获取路径表** — 16 个 provider 的
  "自己生成/花钱买/拿不到"判定 + 逐个操作步骤 + 工时估算。
  填入后 M1-3 才能执行。
- [ ] **CI 方案结论** — 多账号 ToS 风险、公开仓库是否免费无限分钟数
  (若是则转公开本身解锁 CI)、自建 runner 的公开仓库安全风险、
  linux.do 买 CI 的风险评估、macOS CI 的现价选项。
  填入后 M2-2 的"绑进 CI"才有落点。
- [ ] **搜索作用域惯例调研** — 13 个同类项目 + atuin/mcfly/zoxide 的默认
  作用域与演变。填入后 M3-3 才能落 ADR。
- [ ] **功能缺口完整清单** — 填入 M3-4。
- [ ] **文档/UX 缺口完整清单** — 补充 M4。
- [ ] **性能审计完整报告** — 补充 M2(§1.3/1.7 已有主要结论)。

## 7. 进度日志(每完成一个任务追加一行,最新在最上)

| 日期 | 任务 | 结论 | commit |
|---|---|---|---|
| 2026-08-18 | 规划 | 本文件建立;owner 拷问确认 D1-D21 共 21 条决策 | — |
| 2026-08-18 | 发布后审计 | 发现内部过程泄漏(5 CRITICAL + 11 HIGH)与 3 处许可违规,已修;仓库转回 PRIVATE | `ff0838a` |
| 2026-08-18 | v0.1.0 | tag + Release 发布(公开仓库,随后转回 PRIVATE) | `55648d6` |







