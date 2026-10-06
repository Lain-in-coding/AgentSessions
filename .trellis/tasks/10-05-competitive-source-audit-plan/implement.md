# 执行与整改建议 Plan（Draft）

## 状态

当前只进行审计与规划。以下包含两条不能互相替代的工作线：

- **A：继续完成用户要求的全量审查。**写完竞品矩阵不代表这条线结束，不因并发限流删项目。
- **B：对已证实问题的整改建议。**可以在独立任务获批准后执行；本文件不是实施授权。

D1 journal 保留合同和 D2 发布次序未决，完整审读未完成，禁止 `task.py start` / 自动修改产品 / 自动提交。

**执行态（2026-10-06）**：T2 机械扫描层完成——14 类探针对全部非二进制文件逐文件扫描（9,421 个已扫描，命中计数在 `research/sweep-*.json`，中央账本 `coverage-index.json` 已写回逐文件 state）。T1 全文回执流水线：agf、sessiongrep 已完成并入；claude-historian-mcp、cass-A（pack/资产）、fast-resume、hstry、memex、Recall 进行中；cass-B（检索管线）、Wake 已排队（并发上限 6）。T3 排除项（二进制/超大非源文件）已记录原因与哈希。

## 实施进行态（2026-10-06，D1/D2/D3 已批准）

**第一波（已完成、已提交）**：
- 10-06-fact-table-truth-repair（B0）→ 校验 PASS，commit 77ffbb7
- 10-06-hotpath-git-probe（B1）→ 校验 PASS，commit 4a1aa28（get/show 探测 2→0；search 4→2）
- 10-06-six-invariant-selfchecks（D3）→ 校验 PASS（6/6 通过 + 3 处边界归 B4），commit f899794

**第二波（运行中）**：
- 10-06-first-run-closure（B3，CLI）
- 10-06-journal-retention（B2，adapters-sqlite 适配层 API，preview-first；不动 CLI）

**后续波次（依赖满足后创建，不预支顺序）**：
- B3 首用闭环 ← 依赖 B1 落地后的装配形态
- B2 journal 治理 ← D1 已批准（有约束 compact；未决记录不可删）
- B4 检索质量 / B5 provider 稳定性 ← 依赖 D3 六项结论（失败项先修复）
- B6 发布闭环 ← 依赖 B0/B1/B3
- B7 维护性 ← 最后

**纪律**：子任务实现者不 commit；主会话复核后统一提交（仅本任务树+对应产品文件）。

## A. 剩余审查工作线（不能静默砍掉）

### A0 — 固化覆盖账本

- [x] 清点全部 15 项目、本地 commit/版本点、tracked 工作树状态、候选文件与大文件。
- [x] 分组关键代码链路取证，主项目隔离测试与代表性实验。
- [x] 三层覆盖账本落地：`coverage-index.json` 逐文件 state（T1 回执区间 / T2 探针命中数 / T3 排除+理由+哈希）；脚本 `research/sweep-probes.mjs`、`research/merge-coverage.ps1`、`research/merge-sweep-states.mjs` 可复跑。剩余：为 13 个项目补 T1 全文回执。
- [ ] 对 CodeGraph 未索引的巨型入口文件按符号/区间补读；“查不到索引”不等于没有功能。
- [ ] 为无 Git 的 cc-sessions-viewer 记录内容快照指纹，不冒称有 commit。

### A1 — 逐项目补齐

所有项目仍在范围中：

**A1-status（2026-10-06 覆盖收口）**：T2 扫描 15/15 完成；T1 回执 **15/15 项目并入**（多分片：cass A-D、agentsview A/B、agent-sessions A/B、cc-switch A/B、cc-sessions-viewer A/B、ctx A/B）。账本合计：282 文件全文 + 66 文件区间回执（≈25.1 万行）、9,421 文件探针扫描记录（348 个已升级为 T1 回执）、6 文件缺区间未计入阅读。

**A1 逐项目收口清单**：

- [x] cass：分片 A/B/C/D 回执并入（49,629 行有区间）；残余边界：lib.rs 97,420 行未读段、frankensearch 外部依赖内部语义、connector 细节。
- [x] agent-sessions：分片 A/B 回执并入（19,714 行）；残余边界：UnifiedSessionIndexer/ClaudeSessionIndexer 未读段、AgentSessions/Resume/* launcher、PresenceEngine、Usage/Status 模块。
- [x] agentsview：分片 A/B 回执并入（24,704 行）；残余边界：engine.go 5,067 行未读段、watcher 细节、53-case provider 注册表逐项保真。
- [x] ctx：分片 A/B 回执并入（20,250 行）；残余边界：daemon 命名管道/refresh job、net.rs、MCP e2e。
- [x] hstry：回执并入（15,316 行）；残余边界：service.rs gRPC 区、其余 14 家 adapter 逐行（依赖 T2 探针）。
- [x] AgentRecall：回执并入（10,114 行）；残余边界：database.ts/session-store.ts/indexer.ts/renderer。
- [x] Recall：回执并入（13,244 行）；残余边界：app.rs/popups.rs 未读段、9 家 adapter 逐行。
- [x] memex：回执并入（7,133 行）；残余边界：api/compact/llm/rag/web。
- [x] claude-historian-mcp：回执并入（11,921 行）；残余：package-lock 8,658 行、4 个二进制内容（排除记录）。
- [x] agf：33/34 全文 + TUI 2686/2686（11,313 行）；demo.gif 排除；测试只读未运行。
- [x] fast-resume：12 full/2 partial（5,158 行）；残余边界：opencode legacy 段、9 家 adapter 逐行。
- [x] sessiongrep：25/26（7,333 行）；demo.gif 排除。
- [x] Wake：9 full/4 partial（11,388 行）；残余边界：db.rs 2,330 行未读段、19 家 adapter。
- [x] cc-switch：分片 A/B 回执并入（22,762 行）；残余边界：proxy 未读段（46.5%）、凭据静态加密未验证。
- [x] cc-sessions-viewer：分片 A/B 回执并入（20,690 行）；残余边界：4 家 parser partial、util 测试区。
- [ ] ASG：ASG 侧逐文件补审与 §三·B“对 ASG 的直接含义”自查项（cap 先于过滤/零证据升格/失败固化/裁剪进缓存/shell 参数化/投影截断共用）——**本轮未做**，待用户决定是否作为实施前置任务。

**A1 残余口径**：以上“残余边界”均已写入对应 receipt 的 missing_ranges/limitations，不冒充已读；若要升级为全文覆盖，按 P2 排期，不得由代理自行宣布完成。

### A2 — 动态与跨层补证

- [ ] 原生 Windows/Linux/macOS 的相关 build/安装/升级/恢复证据；没有环境就明确未验证，不生成假认证。
- [ ] 合法且可运行的竞品共用 Claude/Codex 合成语料对跑；先对齐 query/结果单位/过滤/冷暖缓存语义，不比较不同工作量的数字。
- [ ] 真实 E5 bundle 的离线模型质量与长文测试，未经授权不下载或上传私有数据。
- [ ] 复审所有高严重度结论，寻找已有防线和反证，修正误报。
- [ ] 更新报告和 PRD 完成度；只有上述覆盖真正完成才能宣称全量审读结束。

## B. 整改工作线（建议，不是已批准待办）

### B0 — 事实账与指标语义（P0，先行）

**范围**：既有竞品表/路线/benchmark/发布文档；不动产品架构。

- [ ] 修正至少六处技术栈/形态错误、cc-switch 检索能力、Lite/重服务混写。
- [ ] 每条差异化加版本/源码或实验引用，删除不能支持的“无人/最强/已登顶”。
- [ ] 将 resume preview、实际 native resume、pack validity、后续任务成功分开；保留现有原始数据与历史语义。
- [ ] 核验当前而非历史的 release/CI/owner blocker，不能用旧 billing 记录充当当前事实。

**验收**：15 行可追溯；不把未验证/受限/计划能力计入胜负；文案与实际 capability/benchmark 语义一致。
**回滚**：仅文档，保留原始报告；不篡改历史数据。

### B1 — 重复 Git 探测（P1，最小高收益修复）

**依赖**：B0 可并行；用户批准该子任务。与 B3 的 CLI 修改串行以免同文件冲突。
**主要范围**：CLI lib.rs 的 App factory/clock/search/get/show；repo_identity 的测试注入；相关测试。

- [ ] 为纯读取与需要 repo 排序的用例分清环境依赖。
- [ ] 移除“为了取时钟先造一个 App”；一次检索复用一次 repo 解析。
- [ ] get/show 等非排序读取不触发 Git。
- [ ] 加 resolver 调用计数测试、固定时钟/返回 data 一致性测试。
- [ ] 对 CLI/MCP/TUI/Web 的组合路径做交叉检查，不引入长期缓存失效问题。

**验收**：get/show 0 次 Git；search 仅一轮解析；原 ranking/cursor/filter/错误契约不变；重跑配对实验和 Core full。
**性能候选目标**：当前固定本机 get/show P95 先争取 <=20ms、4000-message search P95 先争取 <=80ms；这是待多轮复测的本机目标，不冒充跨平台 SLA。
**重要限制**：search 的 97→10ms 对照关闭了所有 repo 信号；保留 repo 排序后仍有一轮合法成本，不能承诺把全部 87ms 差额无条件拿掉。
**回滚**：小补丁可直接回退；无 schema/data 迁移。

### B2 — Journal 明细治理（P1，D1 未决时不实施）

**依赖**：D1、完整消费者/恢复合同分析；与向量/SQLite 重构修改串行。

- [ ] 列出 building/activated/aborted 各状态的读写消费者、幂等校验与诊断需求。
- [ ] 选择永久无损紧凑表示，或有界明细保留 + 长期必要摘要；记录具体损失边界。
- [ ] 设计旧 schema/备份/rollback，先 dry-run 报告候选量和预计影响。
- [ ] 明确未决操作保护，不默认删整个历史表，不篡改 generation/digest。
- [ ] 加 append/rewrite/branch soak、compact 中断、并发读、重复请求、relocation 回归。

**验收**：固定当前数据的长期体积有解释和预算；清理后重复操作与恢复语义不变；失败不破坏 catalog/FTS；所有潜在数据损失必须被用户合同接受。
**回滚**：保留迁移前备份与兼容窗口；不能用旧二进制打开不兼容格式碰运气。

### B3 — 首次使用与主流程（P1）

**依赖**：B1 完成后再碰共享 CLI 文件；D2 确认优先级。

- [ ] 显式 --db 优先，缺省使用已有 OS 默认路径；不增加无需求的多层 fallback。
- [ ] 缺库读请求仍无写入，准确提示一次显式 sync，不伪装零结果。
- [ ] 帮助/Quickstart 按真实顺序；修掉“search 可能不是合法命令”之类错误引导。
- [ ] 人类结果展示可识别会话/provider/project/time/snippet/动作；复用现有投影，避免另一套 parser。
- [ ] discovery 不支持、无来源、未索引、索引陈旧和真正零命中分清楚。
- [ ] 在新 profile/空 HOME 合成环境测试第一条成功路径，绝不扫用户真实目录。

**验收**：不理解 wire-id 也能完成 search→context→preview/handoff；显式 DB/Robot/只读/脱敏合同不破坏。
**回滚**：命令/路径行为 feature 和兼容说明明确；不自动迁移或合并已有数据库。

### B4 — 检索质量与真实语义（P1/P2）

**依赖**：B1 避免外部进程噪声污染延迟；真实模型与许可就绪；SQLite 大改不得与 B2 并行。

- [ ] 保持现有 frozen 100-query 集作回归，不针对它改答案/金标。
- [ ] 新增独立 holdout，按 zh/en/code/path/error/长文/近重复/无结果分桶。
- [ ] 输出明确 model-kind/effective mode；兼容既有 retrieval_mode 枚举。
- [ ] 用真实 E5 运行，评估前 512 token 之外的内容及多向量 chunk 需求；先有指标再定方案。
- [ ] 同时测 Recall@k、MRR/nDCG/任务完成、错误命中、citation correctness、bytes/token、latency、store。
- [ ] 1万/10万/100万消息与过滤选择性阶梯测试；预算受控逐级跑，不将 4000 消息结论外推。
- [ ] 只有 CPU/I/O profile 证明 exact scan 是瓶颈才选 ANN；保留 prefilter/delete/cursor 正确性。

**验收**：真实语义增益稳定且不牺牲 code/CJK 基线才考虑晋级；不达标则保持 optional，不靠调标签自我认证。
**回滚**：模型 id/version 不混用、旧向量保持隔离；新 chunk/index 的切换原子、可恢复。

### B5 — Core provider 稳定性（P1）

**依赖**：B0 的支持口径、已完成剩余源/版本审读。

- [ ] Claude/Codex 的 provider-version/variant/OS/fixture revision 明确。
- [ ] golden/property/真实授权回归分别记录；append/shrink/同长改写/分叉/移动/SQLite WAL 必测。
- [ ] resume 命令只对验证过的 target 宣称可用；未执行始终标 preview/derived。
- [ ] 长尾 12 家保持已实现功能与 honest maturity，不为“收敛”粗暴删掉。
- [ ] 新来源证据先核验，不直接照搬竞品 parser 后升级认证。

**验收**：能力矩阵、parser 实测、用户文档三者一致；不出现失败扫描删除已有资料；无字节区间时不伪造 offset。

### B6 — 发布闭环（P0 发布门 / P1 工程）

**依赖**：B0/B1/B3 和选定的稳定性/质量门；D2 与 owner 发布批准。

- [ ] 绑定当前 commit/lock/toolchain/feature/target，三 OS build/test/install 有可追溯成功产物。
- [ ] 验证双命令名、干净安装、PATH 指引、升级/卸载、数据库兼容；签名/hash/SBOM/NOTICE 按既有规则。
- [ ] 文档列真实限制，不把配置文件存在当作认证成功。
- [ ] owner 决定公开、签署、历史清洗与账号事项；不擅自推送或发布。

**验收**：新用户拿到具体产物可以独立完成核心闭环；每条发布承诺能指到成功证据。
**回滚**：旧产物/兼容库备份和停用入口说明，schema 不兼容时 fail-closed。

### B7 — 维护性整理与延后清单（P2）

- [ ] 按已证实职责切分 SQLite/CLI 大文件；先测回归再提取模块，不借机改行为。
- [ ] 集中 flag / request / output metadata，减少多处同步点；遵守现有禁 clap 规范，若真要变更另立决策。
- [ ] 统一能力与 schema 投影守护；Trellis 生成文件不是产品重构对象。
- [ ] 暂缓未经收益验证的新 GUI、云/团队、图谱、自动总结、更多模型/适配器。

**验收**：减少重复解析/重复 I/O/契约同步点，可量化而不是只把文件切小；现有功能不退化。

## 依赖与并行纪律

`B0` 可与获准的 `B1` 并行；`B1 → B3 → B6`。`D1 → B2`。`B1 + 模型/评测证据 → B4`。`B0 + provider 证据 → B5 → B6`。`B7` 在具体行为修复稳定后进行。

未来分派 worker 必须采用不重叠写集：CLI 性能与 CLI 首用不能同时改 lib.rs；journal 与 SQLite semantic 不同时改同一 adapter 主文件。主线程保留决策/整合/反证，不靠多个 reviewer 重复扫同一段代替覆盖。

## 通用验证命令（实施前按实际子任务收敛）

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --target-dir <isolated-target> -- -D warnings
cargo test --workspace --locked --target-dir <isolated-target>
cargo test -p agent-session-grep-application --features semantic-candle --locked --target-dir <isolated-target>
cargo check -p agent-session-grep-cli --all-targets --features semantic-candle --locked --target-dir <isolated-target>
node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs
python -m unittest discover -s scripts -p "test_*.py"
python -m unittest discover -s scripts/release -p "test_*.py"
python -m unittest discover -s scripts/evidence -p "test_*.py"
```

本地已有依赖时可加 `--offline`；新 clone 不伪装离线安装成功。性能/增长/模型报告都要通过对应 validator 或保留可复现原始样本。日志和合成数据库放 ignored runtime，不提交个人路径和缓存。

## 当前阶段的退出条件

- [ ] A 工作线覆盖真正完成，或用户明确接受剩余证据限制；不得由代理自行缩水。
- [ ] D1/D2 收敛、最终报告/PRD/设计/计划一致。
- [ ] 用户审阅最终摘要；另有明确实施批准后才创建/启动对应整改子任务。

**当前没有满足退出条件，不宣称完成，不开始产品修改。**
