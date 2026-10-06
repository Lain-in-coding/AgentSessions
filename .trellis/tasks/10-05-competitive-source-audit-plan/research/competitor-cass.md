# Research: cass 本地源码竞争力审计

- Query: 独立审计 coding_agent_session_search 的实际能力、优势、缺陷、可借鉴机制与可复用验收；不审计 agent-session-grep。
- Scope: internal；只读 `C:/AgentHub/project/Github_src/coding_agent_session_search`；唯一输出为本文件。
- Date: 2026-10-05（Asia/Shanghai）
- 状态：研究进行中，尚未完成；以下是已核实的清点信息，不是最终结论。

## Findings

### 快照与研究约束

- `.git/HEAD` 指向 `refs/heads/main`，直接读取 loose ref 得到 `aa92a45311e10a3ac9c40c3730664a7366af2fe8`；未运行任何 git 命令。HEAD 身份不等于工作树干净证明。
- `Cargo.toml:1-10`：包名 `coding-agent-search`，版本 `0.6.22`，Rust edition 2024，默认二进制 `cass`，仓库 `https://github.com/Dicklesworthstone/coding_agent_session_search`。
- 本轮保持 Trellis planning；不运行 task start、不改产品、不下载依赖/更新仓库、不启动真实会话、不执行来源发现或默认索引、不发送私人内容。
- 已读取任务 PRD、workflow 的 planning/research 规则、共享 cross-layer 思考指南、竞品根 AGENTS.md 的适用约束。根下未发现嵌套 AGENTS.md/CLAUDE.md/GEMINI.md。
- 遵循 CodeGraph-first；已返回片段不再用文件读取重复加载。目录清点和语法大纲不等于逐行审计。旧 deep-read 报告不作为证据。

### 第一方清点（不把大小/测试数量当质量结论）

| 区域 | 清点结果 | 分类边界 |
|---|---:|---|
| `src/` | 262 文件，约 18.4 MB；234 个 Rust 文件 | 包含主产品、内联测试、网页 JS/CSS/HTML 和少量 fixture |
| `tests/` | 约 630 文件；254 个 Rust 文件 | 集成/契约/CLI/TUI/恢复测试、fixture、浏览器资产；不是全部已运行 |
| `benches/` | 11 个 Rust 文件 | index/search/runtime/db/cache/crypto/export、端到端与集成回归 |
| `fuzz/` | 2,333 文件；13 个 fuzz target | 大部分是 corpus，不是 2,333 个独立测试 |
| `docs/` | 288 文件 | 224 个 artifacts、29 个 planning、6 个 reference、6 个 assets、1 个 perf 文档及根文档 |
| `.github/` | 13 文件 | 12 个工作流与 UBS 版本文件 |
| `scripts/` | 63 文件 | 测试/性能/历史恢复/发布辅助；未执行 |
| 根配置 | Cargo.toml/Cargo.lock/build.rs、toolchain、deny/audit、安装器、LICENSE、packaging | 依赖、发布、安装、安全政策需要分别验证，不能由 README 推导 |

模块清点覆盖：`connectors`（23 个具名 provider 模块 + mod）、`model`、`indexer`、`storage`、`search`、`daemon`、`ui`、`sources`、`html_export`、`pages`/`pages_assets`、`analytics`，以及根模块中的 doctor/recovery/evidence/privacy/context_pack/robot 等。最大源文件是 `src/lib.rs`（约 4.05 MB）、`src/indexer/mod.rs`（约 2.16 MB）、`src/ui/app.rs`（约 1.92 MB）、`src/storage/sqlite.rs`（约 1.18 MB）、`src/search/query.rs`（约 0.79 MB）；其中包含内联测试，不能把这些大小全算成生产实现。

## Caveats / Not Found

- 尚未运行编译、测试或基准。当前写权限仅限本 Markdown；不通过构建、模型下载或运行 cass 产生其他文件。
- provider 核心实现部分来自 Git 固定版本依赖 `franken-agent-detection`，其依赖源码不在本轮允许的竞品根目录内；将区分本地 wrapper/调用/测试证据与未读的依赖实现。
- 后续结论将标记「静态发现 / 已运行验证 / 未验证推测」，并给出 8–12 组文件:行号证据及诚实阅读边界。

## 主线程后续源码结论

研究员再次限流后，主线程接管关键链路。以下不是整仓逐行审读完成声明；尤其大文件 `src/lib.rs` 未被 CodeGraph 索引，已改用限定源码区间验证，不能从“索引里没查到”推断功能不存在。

1. **产品形态明确是 Rust CLI/TUI**：根 `Cargo.toml:1-18` 的 coding-agent-search 0.6.22、cass 默认 binary；ASG 旧竞品表的 Python 分类错误。
2. **pack 是接到实际 CLI 的，不仅是计划模块**：`src/lib.rs:23592-23609` 给出 cass pack 命令错误提示及导入；`23794-23863` 从 search hits 构造 PackCandidate、调用 plan_answer_pack，并输出 effective mode/readiness/redaction 等。
3. **预算与遗漏理由完整**：`src/search/pack_planner.rs:58-85` 限制 token/session/evidence/context/excerpt；`372-469` 记录 evidence 选择分数、duplicate/stale/unavailable/budget 等遗漏原因、输出格式与脱敏策略。ASG 的“无人做到 pack 级证据契约”没有事实基础；不过它们的 schema 和证明强度不完全相同，不能宣布等价。
4. **来源/语义并非简陋**：`src/search/tantivy.rs:11-25,29-53` 有 conversation/evidence packet 与 provenance、frankensearch lexical API；AST 还识别 exact semantic、ANN、two-tier 搜索路径。本轮未完整读外部依赖实现或实测模型，不用函数名证明效果。
5. **Provider 核心迁入独立依赖**：`src/connectors/mod.rs:1-44` 明说大部分 connector 在 franken_agent_detection；`58-103` 保留 Codex scan-root identity 的 enrichment/preflight。应把本仓库 wrapper 与外部依赖覆盖分开，不能沿用“40+ 全部读过”的口径。
6. **source-aware 功能有代价**：根 Cargo.toml 有多项版本/commit 固定的生态依赖，巨大主文件和复杂运行模式也会带来可维护性与打包成本。ASG 不该照着它做功能清单堆叠。
7. **测试/benchmark 资产是资产，不是结果**：pack planner 的 golden/边界测试、search_perf bench、robot_perf tests 已定位，但本轮没有跑 cass 的测试或 benchmark，因此不引用“更快多少”等比较数字。
8. **许可是硬边界**：LICENSE:1-39 为带 OpenAI/Anthropic rider 的许可，条款明确包含 benchmarking/testing/analyzing/indexing 等。它不是可按普通 MIT 处理的代码。没有运行、复制到 ASG 或导入对比 benchmark；不提供授权法律结论，不把 clean-room 当作自动绕过附加条款的许可。

### ASG 应借鉴与不应照抄

- 借鉴：task-oriented `pack`、明确为何选中/遗漏、来源状态、请求预算、default data-dir 友好入口。
- 保留自己的优势：稳定身份和 Placement、明确的字节/逻辑证据边界、只读源、跨入口统一协议与默认脱敏。差异需要同一问题/语料下验证，不能靠“无人有此功能”。
- 不照抄：全部发布/导出/网页/remote/语义 tier/团队协作功能，或者受限制代码；不因对方复杂就替 ASG 的无关复杂度辩护。
- 验收：pack 中每个 excerpt 可定位、budget 超额有原因、无证据时不虚构、坏 source 状态不混为“没有结果”、原文和推断严格区分。

### 未完成范围

未逐行完成 cass 全部第一方 source/tests/fuzz/scripts，未读完被依赖承载的 connector/search 实现，未做安装、模型与 release 实跑。本任务仍需保留该全量阅读缺口，不能宣布 cass 审计收官。
