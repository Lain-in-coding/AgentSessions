# agent-session-grep 终极交接文档（2026-08-17）

> 交接对象：**完全不了解本项目的 Agent**。请按顺序阅读：§0 先读 → §1–§7 建立全景 → §8 确认已完成（避免重做）→ §9 是你要继续推进的工作 → §10 协作规则 → §11 第一步行动 → §12 附录速查。
>
> 交接时点：2026-08-17；权威 HEAD：`dc704df`（origin/main 已同步）；产品版本 0.1.0（未发布，仓库仍 PRIVATE）。
>
> 一次更新的交接入口：`.trellis/tasks/08-15-final-integration-release-rehearsal/research/2026-08-17-ultimate-handoff.md`（本文件）。上一份交接：同目录 `2026-08-16-codex-handoff-report.md`；更早的全量审计：`2026-08-16-final-open-source-audit.md`。

---

## 0. TL;DR（两分钟版）

- **这是什么**：`agent-session-grep`（CLI 别名 `asg`）是一个 Rust 写的本地优先 AI coding-agent 会话历史搜索引擎。16 个主流 AI 编程工具（Claude Code、Codex、Grok Build、Pi、Hermes、Cline、Aider 等）把会话写在不同目录、不同格式；本工具把它们归一成 canonical 模型，提供全文检索、resume（继续会话）与 handoff（交接包），从 CLI、MCP、Robot、TUI、Web 五个入口一致暴露能力。
- **当前状态一句话**：2026-08-17 发布收口 wave 已全部完成并推送到 origin/main（`dc704df`），本地全部质量门绿（fmt/clippy/workspace 测试、Python 套件、verify-release 10/10、五入口一致性 harness、安装 gate、cargo deny、隐私扫描 0 findings）；**发表公开不是"代码缺陷"而是 owner 与外部资源决定**（GitHub Actions billing、PRIVATE→public、签名/审计签署等，见 §9.2）。
- **还没做的最大一块（本地可做）**：真实本地语义模型（`semantic-candle` + pinned `multilingual-e5-small`），当前 `bigram-hash-v1` 只是诚实标注的 fuzzy-lexical vectorizer（§9.1-1）。做它之前不得在任何宣传口径里称"语义搜索"。
- **新 Agent 第一步**：见 §11——先验证环境与全门，再决定从 §9.1 里选任务。

---

## 1. 项目是什么

### 1.1 定位

散落在 16 个 AI coding agent 本地目录里的会话历史，变成一套**可验证、可恢复、可交接**的本地基础设施：

> 给一个模糊问题，不仅找到相关历史，还给可验证证据（source span）、关键上下文，以及可直接 resume 的会话或可交给当前 agent 的 handoff pack。

- 第一用户：个人 AI coding-agent 重度用户；第二入口：agent/MCP 消费者。
- 首发范围明确排除：团队同步、云端、知识图谱、原生 GUI（见 roadmap 决策）。
- 发布形态：一次性完整开源；仓库保持 PRIVATE 直到发布门全绿，公开是 owner 最终裁量。
- License：MIT OR Apache-2.0 + Provider Adapter Protocol contribution policy。
- 竞品基线：`docs/product/COMPETITOR-COMPARISON.md`（13 个可自由引用外部项目 + cass clean-room-only）。

### 1.2 差异化主张（对外宣传锚点，改口径前先查这些文档）

| 差异化 | 说明 |
|---|---|
| Evidence-first | 每条结果带 source span 可回溯原文；handoff pack 原文/推断分栏 |
| 身份与一致性 | StableId 三级 + durable outbox + CAS generation + 失败不删除 |
| CJK 一等公民 | bigram 索引（ADR-0007/0008）；语义模型落地前不得宣称中文语义优势 |
| 诚实能力矩阵 | certified/GA/beta/experimental/unsupported 分级、证据晋级（capability.rs 单源） |
| 零遥测可验证 | 代码级禁止 + CI 静态检查 + 全局 `--offline` + network_egress 测试 + deny.toml bans |
| 跨边界默认脱敏 | Web/Handoff/MCP/Robot 默认脱敏，CLI/TUI 本地不脱敏（ADR-0009） |
| Robot 契约 | 13 码 error catalog + cursor 防篡改 + retrieval_mode |

### 1.3 版本与产物

- workspace 版本 `0.1.0`（`Cargo.toml [workspace.package]`）；`--version` 输出 `agent-session-grep 0.1.0`。
- 二进制名 `agent-session-grep` + 别名 `asg`（同一 main.rs 双 bin target；Cargo 的 "file present in multiple build targets" warning 是**有意设计**，不是错误）。
- Rust edition 2024，`rust-version = "1.90"`；release profile：`opt-level=3, lto=thin, codegen-units=1, panic=unwind, strip=debuginfo`（理由写在 Cargo.toml 注释）。

---

## 2. 环境与仓库地图

### 2.1 路径（Windows，用户 QIN / 小Q）

| 东西 | 路径 |
|---|---|
| 主仓库 checkout（main 分支被它占用） | `C:\AgentSessions` |
| 本波发布的隔离工作树 | `C:\AgentSessions\.claude\worktrees\public-release-audit`（分支 `worktree-public-release-audit`） |
| 其余历史工作树 | `C:\AgentSessions\.claude\worktrees\08-15-*`、`08-16-*`、`agent-*` 等（多为陈旧任务残留，见 §2.3 警告） |
| 参考源码（可借鉴/可抄） | `C:\AgentHub\project\Github_src`（13 可引用 + cass 仅 clean-room） |
| 我的持久 memory | `C:\Users\小Q\.claude\projects\C--AgentSessions\memory\`（索引 `MEMORY.md`） |
| 用户全局规则 | `C:\Users\小Q\.claude\rules\*`（powershell.md、code-search.md、search_core.md、backup-before-edit.md） |

### 2.2 Shell 与工具纪律（重要）

- **默认只用 PowerShell 工具**（pwsh 7）；Bash 工具仅限例外清单（仓库 .sh 脚本、git hooks、CI 复现等，见用户规则 powershell.md）。
- 文件操作用专用工具：列文件 Glob、读文件 Read、搜内容 Grep（即 rg，**不要**在 shell 里调 rg）、编辑 Edit/Write。
- **编辑含大量中文的文件时，Write/Edit 偶发 JSON 编码损坏**（本波已踩坑多次）。稳妥做法：Read 原样取字符串 → 小段 Edit；大段中文改写优先用 PowerShell 单引号 here-string `@'...'@` + `Set-Content`（注意 here-string 闭合 `'@` 必须顶格）。
- 写文件默认 UTF-8 无 BOM（pwsh 7 默认）；不要手动加 BOM。
- 路径一律正斜杠 `C:/...`。

### 2.3 Git 与 worktree 纪律（防止事故）

- **`main` 分支被主 checkout `C:\AgentSessions` 占用**，无法在此 worktree 内 `git branch -f main`；同步靠 `git push origin HEAD:main` 与 `git fetch origin`。
- **git stash 是多 worktree 共享的**：绝不要裸 `git stash pop`（可能弹出别的会话的改动）。要暂存先 `git stash push -u -m "<唯一标签>"` 并记下 SHA，用 `git stash apply <sha>` 恢复。
- **陈旧 worktree `08-15-provider-opencode` 有未提交改动且已过时**（capability.rs 里 opencode 的 `resume: Derived` 是**错误旧值**，main 已修正为 `Unknown`）。**不要合并它**；main 是最新权威。
- 提交纪律见 §10；用户在本波任务中已授权**自主 commit/push/merge**（无需逐次征求同意），但合并 PR 仍是 owner 决定、main 直接 push 请保持 Conventional Commits 与质量门绿。
- 远端推送偶发 `schannel: failed to receive handshake`（TLS/网络抖动）：`git config http.version HTTP/1.1` 后再推即可恢复。

### 2.4 仓库目录地图

```
crates/
  agent-session-grep-domain/       # canonical 模型：Message/Placement/Edge/StableId/ToolActivity/EvidenceSpan
  agent-session-grep-ports/        # 端口契约：CatalogStore/SearchIndex/SemanticIndex/ReadOnlySource/
                                   # capability.rs(16行矩阵单源)/manifest.rs(AdapterManifest)/redact.rs/
                                   # robot 相关 PortError/CanonicalCode/RetrievalMode
  agent-session-grep-application/  # App ADT + 用例：Search/Context/List/Get/Show/Status/Sync/Resume/
                                   # Handoff/embedding.rs(bigram-hash)/hybrid.rs(RRF)/cjk.rs/
                                   # guidance.rs/resume.rs
  agent-session-grep-adapters-sqlite/  # SqliteStore：catalog+FTS5+message_vec+claims+tool_activities；
                                       # schema 迁移 v1..v12；source_fs.rs(只读流式捕获)/lease.rs/WAL/CAS
  agent-session-grep-cli/          # 五入口：main.rs(CLI/Robot)、mcp.rs、tui/、serve.rs、web/index.html、
                                   # protocol.rs、redaction.rs、hooks.rs、human.rs
  agent-session-grep-testkit/      # 测试工具（只读断言等）
  agent-session-grep-provider-*/   # 14 个 provider adapter（每仓有 golden fixtures）
docs/
  product/     PROVIDER-MATURITY-MATRIX.md / OPEN-SOURCE-ROADMAP.md / COMPETITOR-COMPARISON.md
  release/     go-no-go.2026-08-16.md / rehearsal-runbook.md
  operations/  PUBLIC-HISTORY-SCRUB.md / INSTALL-AND-UPGRADE.md / REUSE-LICENSE-AUDIT.md /
               REAL-DATA-REGRESSION.md / external-readiness-gate.md
  security/    THREAT-MODEL.md / FIXTURE-REDACTION-POLICY.md
  architecture/ RFC-0001-canonical-model-and-stable-id.md / RFC-0002-provider-adapter-contract.md
  adr/         ADR-0001..ADR-0010（0009 跨边界脱敏、0010 provider 晋级回滚）
  contracts/   CONTRACT-cli-robot-mcp-draft.md
  evidence/    integration-beta/（真实数据回归证据、数据仅聚合）
schemas/
  robot/v1/ error-catalog.json；robot/v1.1/ envelope.schema.json
  handoff/v1/ pack.schema.json
scripts/
  verify-release.py              # 10 项发布验证（§7.1）
  test_verify_release.py / test_benchmark.py / test_compare_entrypoints.py / test_install_entrypoints.py
  benchmark.py                   # 已修 --max-items；孤儿脚本，勿继续扩展
  install/  install/uninstall/smoke/gate_smoke.{sh,ps1}   # 权威安装器（根目录 scripts/install.sh 是转发器）
  evidence/ open_source_gate_benchmark.py / core_beta_benchmark.py / real_data_regression.py /
            privacy_scan.py + test_*.py；out/README.md（生成物不得入库；*.json 已 gitignore）
  rehearsal/ compare_entrypoints.py（五入口一致性）
  release/   build-manifest.py / export_public_tree.py + test_*
.github/workflows/  ci.yml / release.yml / core-beta-evidence.yml / security-audit.yml
.trellis/tasks/     任务树（08-13/08-14/08-15/archive）；被公开树导出器排除（§7.2）
spikes/             研究 spike；三个被 CI 引用的已有 [workspace] 标记
skills/agent-session-grep/SKILL.md   # 面向用户/agent 的能力文档（与 9 工具、16 行矩阵一致）
```

---

## 3. 架构全景（hexagonal）

### 3.1 分层与依赖方向

```
∥ CLI (五入口：human CLI / Robot JSON / MCP stdio / TUI / loopback Web)
∥   ↓ 同一 Application ADT
∥ application (App) ← use case 编排、检索降级、简历/handoff 装配、预算
∥   ↓ ports（trait 契约）
∥ adapters-sqlite (SqliteStore) ← catalog/索引/向量/claims 实际存储
∥ provider-* (14) → canonical events → adapter 读源（只读、bounded）
∥ domain：canonical 模型（Message/Placement/Edge/StableId/...）
```

- **应用层不依赖具体后端**：`App<C: CatalogStore+ContextGraphStore, S: SearchIndex, R: ResumeClaimsStore=NoResumeClaims, M: SemanticIndex=NoSemanticIndex>`；生产用同一个 `SqliteStore` 填充多个槽（`&T` blanket impl 支持共享引用）。
- **检索模式**（`RetrievalMode`）：`lexical`（默认）/ `semantic` / `hybrid`；索引未就绪时显式降级 `lexical_fallback` + warning，**禁止静默切换**（Q54 原则落地在 `resolve_retrieval`）。
- **Cursor**：带签名的 page token，绑定 generation + 查询摘要（非 lexical 模式绑定 mode）；过期/代际变化返回 `cursor_expired/expired` 类错误，客户端重建查询。TTL 15 分钟。
- **预算**：`max_items/max_bytes/max_messages/max_evidence/max_tokens` 击中即 `outcome=partial` + `data.truncation` + exit 10，绝不静默截断。
- **身份**：`StableId` 三级（wire id: `msg_v1_`/`ses_v1_`/`doc_v1_`…）+ durable outbox + CAS generation + 失败不删除（RFC-0001）。

### 3.2 存储与迁移

- 单 SQLite 文件（bundled rusqlite 0.40.1）：catalog 表 + FTS5（bm25）+ `message_vec`（embedding 投影，按 model_id 隔离）+ resume claims + `tool_activities`/`tool_activity_membership`。
- `SCHEMA_VERSION = 12`（`crates/agent-session-grep-adapters-sqlite/src/lib.rs`）。迁移链（当前阶段）：v7→v8 resume-claims → v9 source-scan provider_id → v10 semantic vector sidecar → v11 session 元数据检索投影 → v12 tool-activity 投影；v5→v6/v6→v7 为历史文档（migration-v5-to-v6.md / migration-v6-to-v7.md）。迁移幂等、`PRAGMA user_version` 门控、单事务；schema 不匹配 → `schema_incompatible`（exit 9）。
- 写入纪律：WriterLease（OS 独占句柄）+ CAS activation + journal（outbox）；推导投影（FTS/向量）可从 catalog 重建（`index rebuild`）。
- 只读源：`ReadOnlySource`/`FileSource`/`SnapshotFs`——捕获时记录 `(len, mtime_ms, fingerprint)`，提交前复核；SQLite 类 provider 源一律 `SQLITE_OPEN_READONLY + busy_timeout`，绝不写 provider 库。
- Bounded ingestion：流式单行上限 8 MiB、整文档类上限 32 MiB、SQLite 类 128 MiB（RFC-0002 §7）。

### 3.3 检索链

- lexical：FTS5 bm25 + CJK bigram transform（`application/src/cjk.rs`，ADR-0007/0008）。
- semantic/hybrid（当前为占位）：`bigram-hash-v1`（384 维、BLAKE3 哈希 + bigram 累积、L2 归一、`query:`/`passage:` 前缀对齐 E5 约定），**明确 NOT semantic**（`embedding.rs` 头注释 + 测试 + verify-release 均守护这一诚实标签）；RRF k=60 融合（`hybrid.rs`，参考 ctx idea-only）。
- 排序：score desc + id tiebreak 钉住全序；group_by_session 时按 session 折叠并带 `occurrences`。

---

## 4. Provider 体系（16 行矩阵）

### 4.1 事实

- **26 行能力矩阵单源** = `crates/agent-session-grep-ports/src/capability.rs` 的 `ProviderCapabilityMatrix::current()`。任何入口（CLI providers、MCP list_providers、Web /api/providers、docs 矩阵文档）都投影它；禁止入口硬编码。
- 现有 16 行：14 个 implemented（全 `Experimental`）+ 2 个 deferred（`deepseek-harness`、`zcode`，无 transcript 证据，`Unsupported`，不宣传）。
- 14 个已实现 provider_id：`claude-code, codex, grok-build, antigravity, opencode, pi, hermes, cursor, kimi-code, openclaw, qoder, tencent-codebuddy, cline, aider`（各自 crate：`agent-session-grep-provider-*`，每仓有 golden fixture + PROVENANCE.md + `AdapterManifest`）。
- 关键字段现状（capability.rs 为准，矩阵文档与其一致并被 16 行漂移测试守护）：
  - resume：`derived`（claude-code/codex/pi/grok）；`unknown`（opencode 等）；`unsupported`（aider/cline/openclaw）。
  - tool_activity：`partial`（claude-code/codex/aider；schema v12 落库）；其余 implemented `unsupported`。
  - discover：仅 claude-code/codex `native`；context/handoff/incremental 基本 `unsupported/unknown`（后续全能力链任务）。
- **晋级门（RFC-0002 §6 + ADR-0010）**：Beta 需主路径 fixture/golden/contract/只读/增量/source span 全过 + 跨 target named CI run + owner 决策；禁止只凭代码存在就晋级。当前 0 个 Beta——这是**公开宣传硬门槛**，不是仓库缺陷。

### 4.2 Golden 与 evidence

- 每个 implemented provider 有 `tests/golden/`（BLAKE3 锁定字节 + 期望输出比对）+ `tests/properties.rs`（固定种子确定性）+ e2e。
- 只读断言：`testkit::assert_read_only` 守护 probe/parse（含 SQLite 只读打开路径）；真实回归 `INV-SOURCES-UNCHANGED`。
- 真实数据回归（授权沙箱内）：`scripts/evidence/real_data_regression.py`，六条不变量（sync 计数、no-parse-loss、逐会话 context、byte 精度、rebuild 稳定、sources-unchanged）；最近全绿证据时间戳与数字见 `PROVIDER-MATURITY-MATRIX.md`「晋级到 Beta 的缺口」第 4 条。**该 harness 只在授权数据 root 上跑，数据不外传、报告只含聚合**。

---

## 5. 五个入口与对外契约

### 5.1 CLI / Robot

- 命令集（SKILL.md 有完整表）：`search <query>`、`handoff <query>`、`get-session-resume <id>`、`get-message <id>`、`list [limit]`、`context <id>`、`get <id>`、`show <id>`、`status`、`providers`、`sync --discover`、`doctor`、`mcp`、`hook <event>`、`index rebuild`；全局：`--db <path>`、`--robot`、`--output human|json|jsonl`、`--request-id`、`--offline`、`--no-color`。
- search 过滤/分面：`--provider`（接受 `claude|claude-code`、`codex`，Web/MCP 已统一 canonical alias）、`--since/--until`、`--mode lexical|semantic|hybrid`、`--max-items`、`--cursor`、`--include-system`、`--group-by-session`、分面 `--main-only`/`--subagent-only`/`--include-sidechain`/`--tool-kind file|command|web|query|unknown`/`--tool-name <name>`。
- **Robot envelope v1.1**（`schemas/robot/v1.1/envelope.schema.json`，`protocol::SCHEMA_VERSION="1.1"`）：success/error/progress/diagnostic 四帧；success 必带 `schema_version/frame_type/command/request_id/ok/outcome/data/retrieval_mode/redaction/warnings/page/meta`；search 帧 `data=$searchData`（hits/generation/truncation + **可选 facets echo**——2026-08-17 新增并已被 protocol 测试守护）；`--robot` 是确定性主路径。
- error catalog：`schemas/robot/v1/error-catalog.json`（权威 13 码 + internal；protocol 测试断言 schema/runtime 集一致）；常见 exit：0 成功、2 usage、4 not_found、6 catalog_error、7 capability_not_supported、9 schema_incompatible、10 partial/截断；help/version 恒 exit 0 且走 envelope（ADR-0006）。
- `providers` 命令输出 16 行矩阵（含 `maturity_target`）；`config paths` 输出平台 data root。

### 5.2 MCP（stdio JSON-RPC 2.0）

- 9 个工具：`search_sessions, get_session_context, get_session_resume, get_message, list_sessions, generate_handoff, list_providers, get_status, doctor`（`mcp.rs` tool_catalog，冒烟断言 9）。
- 协议版本协商：支持 `2025-06-18 / 2025-03-26 / 2024-11-05`；initialize gate；`-32602` 校验错误带 `data.canonical_code=invalid_request`；业务失败走 `isError:true` + `structuredContent.error.{canonical_code,message,retryable,details}`。
- `search_sessions` providers 参数枚举 `["claude","claude-code","codex"]`；分面参数 `sidechain/tool_kind/tool_name` 有回显（非默认 facets → `data.facets`，与 CLI/Robot 一致）。
- `list_providers`：**已升级为完整 16 行**，每行 `{id, variant, maturity, maturity_target, ingestible}`（14 行为 `ingestible:true`，2 个 deferred 为 `false`；单测断言 16 行）。
- 成功载荷 `{outcome,data,warnings,page}` 与 Robot 同形状；默认跨边界脱敏（ADR-0009）；错误帧 message/details 也已脱敏（2026-08-17 加固）。

### 5.3 TUI

- ratatui 0.29 + crossterm；五入口之一，facet 控件**有意 deferred**（`SearchFacets::default()` 注释说明）。
- 无头投影：`crates/agent-session-grep-cli/src/tui/mod.rs::snapshot_search(store, query)` 输出 `{outcome, data.hits[].id, page.has_more/next_cursor, warnings}`，`--snapshot-json` 标志（已被 is_known_flag_name 与测试覆盖）。

### 5.4 Web（loopback serve）

- `asg serve`：std::net::TcpListener（无 http crate）+ 32 位 CSPRNG token + Host/Origin fail-closed + GET-only（POST 变更 501）+ 常量时间 token 比较 + `frame-ancestors 'none'` CSP + bounded 请求头/体/worker 池（4 workers）——2026-08-17 安全加固已完成。
- 路由：`/`（内嵌 UI）、`/health`、`/api/status /search /projection/search /context /handoff /providers /show /resume`；除 `/api/providers` 已是统一 envelope（2026-08-17 修复：`{command,outcome,data,page,warnings}`）外全部经同一 dispatch。
- LAN 模式：`--lan` → `capability_not_supported`（exit 7），不做危险的静默降级。
- stderr 打印 `asg serve: open http://{addr}/?token={token}`（compare_entrypoints.py 解析这行）。

### 5.5 一致性 harness（发布前必跑）

`python scripts/rehearsal/compare_entrypoints.py --binary target/release/agent-session-grep.exe`
- 五入口（cli/mcp/robot/web/tui）全部**直接对比**同一组 canonical operation 的输出；`aliases=[], skipped=[], unimplemented=[]` 恒空；`overall_verdict=consistent`。当前实测绿。

---

## 6. 安全与隐私

### 6.1 零遥测（可验证）

- `crates/agent-session-grep-cli/tests/network_egress.rs`：禁止 HTTP-client crate 出现在 Cargo.lock；断言 crates 里唯一 socket 是 serve 的 loopback `TcpListener`。
- `deny.toml [bans]` 2026-08-17 新增同名单：reqwest/hyper/ureq/isahc/surf/attohttpc/curl/quinn/quiche/tiny_http/rouille/warp/axum/tokio/async-std（`cargo deny check licenses bans sources` 实测绿）。
- `.github/workflows/security-audit.yml`（weekly + **PR 触发**，2026-08-17 补）：零出站静态审计镜像 + cargo audit。
- 全局 `--offline`：需要网络的操作为 fail-closed（exit 7），不是降级。

### 6.2 跨边界脱敏（ADR-0009）

- 引擎：`ports/src/redact.rs`（文本）+ `cli/src/redaction.rs`（JSON 树 walker）。规则：AWS access key/secret key（含嵌入 40 字符 base64 混合大小写 span）、GitHub PAT 家族含 `github_pat_`（2026-08-17 补）、sk-/sk-ant-/xai-、Bearer、PEM private key；每值最多 16 span；短串(<8)保守不碰。
- 边界：Robot/MCP/Web/Handoff/Hook 默认脱敏；Human CLI/TUI 不脱敏（本地边界）；MCP error 帧与 hook header 2026-08-17 也已接入（`business_error_result`、`error_frame`、`format_context_header`）。
- `scripts/evidence/privacy_scan.py`：3 条规则（user-home / machine-root / agent-coordinate），精确 ALLOWLIST（只放行确证合成的 fixture），`--repo .` 扫描全部 tracked 文本（2026-08-17 起不再跳过任何前缀），当前 **0 findings**。

### 6.3 公开发布历史决策（owner 决定）

- `docs/operations/PUBLIC-HISTORY-SCRUB.md`：当前树已清洗，但 **git 历史仍可能含旧路径**；发布前 owner 必须选 Option A（新公开仓）或 Option B（filter-repo 重写），本仓库不做未经批准的破坏性重写。
- **2026-08-17 新增 Option A 机械例行**：`scripts/release/export_public_tree.py --repo . --commit <sha> --destination <空目录>`——导出 tracked 文件、排除内部记录前缀（`.trellis/ .codex/ .codebuddy/ .agents/ .claude/ scripts/evidence/out/`）、写 `PUBLIC-TREE-MANIFEST.json`（source SHA + 每文件 SHA-256）、对导出目录跑同一隐私规则；当前 SHA 导出 274 文件全绿。
- 威胁模型：`docs/security/THREAT-MODEL.md`（状态 Draft，待 owner 签署；2026-08-17 已补 serve-LAN / hook / 模型下载 / 外部 Embedding API 四攻击面，并修正"Data 与 Source Root 重叠拒绝启动"为**未实现**的如实标注）。

---

## 7. 质量门与发布管线（全部命令）

### 7.1 本地质量门（每次改动后跑、发布前全跑）

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                      # ~1280 tests
python -m unittest discover -s scripts -p "test_*.py" -v
python -m unittest discover -s scripts/release -p "test_*.py" -v
python -m unittest discover -s scripts/evidence -p "test_*.py" -v
python scripts/evidence/privacy_scan.py --repo .
cargo deny check licenses bans sources
```

发布回演（release 二进制）：

```powershell
cargo build --release --locked -p agent-session-grep-cli
python scripts/verify-release.py --asg target/release/agent-session-grep.exe   # 10/10
python scripts/rehearsal/compare_entrypoints.py --binary target/release/agent-session-grep.exe  # consistent
pwsh -NoProfile -File scripts/install/smoke.ps1 -Binary ... -AliasBinary ...    # 或 smoke.sh
pwsh -NoProfile -File scripts/install/gate_smoke.ps1 -Prefix <tmp> -OutputDir <tmp>  # 或 gate_smoke.sh
python scripts/evidence/open_source_gate_benchmark.py run --binary ... --output-dir scripts/evidence/out  # 生成物不入库
python scripts/release/export_public_tree.py --repo . --commit HEAD --destination <空目录>   # Option A 例行
```

- verify-release 的 10 项：build/version、sync（0 skip）、lexical search、get、context、resume metadata+dry-run、handoff schema "1.0" 确定性、semantic/hybrid 有效模式=bigram-hash-v1、hook 默认关、providers=16。
- gate benchmark 阈值门（`open_source_gate_benchmark.py`）：lexical_recall@10 ≥0.95、parse_loss ≤0.05、discovery_coverage ≥0.95、resume_handoff_success =1.0；semantic/hybrid 指标存在但**无阈值**（诚实标注 bigram-hash、非语义）。
- 注意：`cargo test -p agent-session-grep-cli` 没有 lib target，单元测试要走 `--bin agent-session-grep`；`scripts/evidence/out/*.json` 已 gitignore（README 明令生成物不入库，已有提交删除过违规跟踪）。

### 7.2 CI 与 release 管线（当前受外部 billing 阻塞，非代码缺陷）

- `ci.yml`：`test`（3 OS：fmt/clippy/test + 三个 Python 套件，2026-08-17 接线）、`installer`（3 OS：install 到临时 prefix + 双命令 + smoke + 卸载幂等）、`deny`（cargo-deny）；**可复用**（`workflow_call` 已加）。
- `release.yml`：`quality`（调用 ci.yml 全量，2026-08-17 修正——此前跨 workflow `needs:[test,deny,installer]` 是**无效引用**的 bug）→ `prepare`（tag/version/lock/metadata 校验+依赖清单）→ `build`（4 target：windows-2022 msvc zip、ubuntu-22.04 gnu tar.gz、macos-15-intel x86_64、macos-15 aarch64；`--locked --release` + verify-release + build-manifest package）→ `assemble`（四归档+四 manifest+SHA256SUMS）→ `publish`（`gh release create --verify-tag`，contents:write 最小权限）。
- `scripts/release/build-manifest.py`：`validate`（tag↔version↔Cargo.lock↔cargo metadata）→ `inventory`（THIRD-PARTY-DEPENDENCIES.json/.csv，`spdx_document:false` 诚实标注未核验）→ `package`（确定性归档+member manifest，`cryptographic_attestation:false`）→ `checksums`。
- 所有 actions 已 pin SHA（2026-08-17）；docs/operations/external-readiness-gate.md 记录的外部项见 §9.2。
- **现状**：GitHub account billing/spending-limit 使所有 workflow run 在首步前 0-step 失败，因此没有 named successful run——这是外部阻塞，任何"伪造绿 CI"的代码改动都是禁止的；billing 恢复后重跑同一 SHA 并记录 run id 即可填 `last_certified_targets`。

---

## 8. 2026-08-17 发布收口 wave（已完成，别重做）

origin/main `8de7312` → `dc704df` 共 26 个提交，按组列出（每组含代表提交与守护测试）：

1. **Robot 契约**：`schemas/robot/v1.1/envelope.schema.json` searchData 新增可选 `facets`（`sidechain/tool_kind/tool_name`）；`protocol.rs` `published_envelope_schema_contains_runtime_contract` 断言 facets 形状；`parse_output_mode` 跳过列表补 `--tool-kind/--tool-name` + 跨扫描器 parity 测试（`value_flag_skip_lists_cover_tool_facets_across_all_scanners`）。
2. **Web/MCP provider 归一**：`canonical_search_provider` 接受 `claude-code`（+ 历史别名 `claude`）；MCP `search_sessions` schema 枚举 `["claude","claude-code","codex"]`；Web `/api/search?provider=claude-code` 端到端可用。
3. **MCP 一致性**：`tool_search` 非默认 facets 回显 `data.facets`（与 Robot 同契约）；`list_providers` 16 行全矩阵投影（`ingestible` 字段；单测 `list_providers_enumerates_the_registry` 断言 16 行/2 个 unsupported）；Web `/api/providers` 统一 envelope（原有对单路由特判的测试已改为断言标准形状）。
4. **安装器**：根目录 `scripts/install.sh/.ps1` 改为转发权威 `scripts/install/install.*`（+ `scripts/test_install_entrypoints.py`）；`gate_smoke.ps1` 修 `-Prefix/-SkipBuild` 与自拷贝；`gate_smoke.sh` 同样 self-copy 守卫；CI installer job 的三段流程（install→双命令→smoke→卸载幂等）。
5. **安全加固**：`redact.rs` 补 `github_pat_`（standalone+embedded）与嵌入 AWS secret key（40 字符 base64 边界锚定+大小写混合，防 40 字符 hex SHA 误伤——有专门负例测试）；MCP `business_error_result`/`error_frame` 与 hook `format_context_header` 过脱敏；`serve.rs` `check_token` 常量时间比较；HTML 与 serve CSP 加 `frame-ancestors 'none'`；`deny.toml` bans HTTP-client 清单。
6. **CI 管线**：`ci.yml` 加三个 Python 套件 step + `workflow_call`；`release.yml` 加 `quality` job 调用 ci.yml、`build.needs=[prepare, quality]`（修复跨 workflow 无效 needs）；`security-audit.yml` 加 `pull_request` 触发；ci/core-beta-evidence/release 三个 workflow 的 actions 全部 pin SHA；blog **间接修复**：`scripts/benchmark.py` `--limit`→`--max-items`（`scripts/test_benchmark.py`）。
7. **矩阵与文档**：`provider_matrix.rs` 新增 16 行全量漂移测试 `matrix_rows_match_capability_matrix_all_sixteen`（文档↔capability.rs 两侧同守）；矩阵文档 resume 行修正（grok=Derived、opencode=Unknown）；INSTALL-AND-UPGRADE/rebuild runbook 迁移说明更新到 v11→v12；CONTRACT 文档 MCP 工具 9 个；`OPEN-SOURCE-ROADMAP.md` §0.1 快照刷新为 `NOT_READY_EXTERNAL_BLOCKERS`；`THREAT-MODEL.md` 四攻击面；`go-no-go.2026-08-16.md` 记录 wave 闭合、移除过时的"benchmark 缺陷"条目；`COMPETITOR-COMPARISON.md` 与 `SKILL.md` 同步 MCP 16 行/五入口一致/3 别名口径。
8. **Evidence/privacy/工程**：`scripts/evidence/out/gate-manifest-gate.json` 解除跟踪 + gitignore（违反 out/README 契约的遗留）；`privacy_scan.py` 删除硬编码操作者用户名与 EXCLUDED_PREFIXES（现在扫全部 tracked 文本，新增两个测试）；三个 spike crate 补 `[workspace]`（嵌套 worktree 下 `cargo metadata --locked` 失败修复）；公开树导出器（§6.3）。

**验证记住**：全套门 §7.1 在 `dc704df` 实测全绿后才推送；origin/main 与 HEAD 一致。

---

## 9. 尚未完成——继续推进的清单（诚实版）

### 9.1 仓库本地可做（按优先级）

1. **真实本地语义模型（最大缺口，登顶差异化的核心）**——任务 `08-15-semantic-hybrid-local-retrieval`（prd.md + design.md 完整规划）与调研 `08-15-open-source-product-roadmap/research/2026-08-16-embedding-model-options.md`（已锁定模型/feature/分发策略）。要点：
   - 实现栈：`candle-core/candle-nn/candle-transformers 0.11` + `tokenizers` + safetensors；模型 `intfloat/multilingual-e5-small@614241f6...`（384 维、mean pooling、L2、`query:`/`passage:` 前缀）；本机 Cargo 缓存已有 candle-core 0.10.2，参考实现 `C:\AgentHub\project\Github_src\Recall\src\embedding.rs`（MIT，可借鉴）。
   - Cargo feature 名 `semantic-candle`，**默认 off**；`cargo install` 默认保持纯 lexical。
   - 模型获取两入口且网络隔离：`model import --dir <bundle>`（全离线、校验 pinned manifest/SHA-256/license）与 `model install multilingual-e5-small`（**唯一允许联网的命令**、先打印 repo/revision/size/license、staging+hash+atomic rename）；search/sync/index/model load 永不联网；`--offline` 下不可用 → 显式 `lexical_fallback`。
   - `EmbeddingModel`/`SemanticIndex`/`EmbeddingManifest`/`message_vec`（model_id 隔离）/RRF 融合/`lexical_fallback` 全部已就位；新实现用**新 model_id**（如 `intfloat-multilingual-e5-small@614241f6-candle-f32-meanpool-l2-qpass-v1`），旧 bigram rows 自动 inert。
   - 门槛：frozen benchmark（CJK/英文/代码 recall + p50/p95 + 体积）达标前 maturity 保持 beta、lexical 保持默认；不得只凭"能跑"升级宣传。相应更新 `PROVIDER-MATURITY-MATRIX`、`COMPETITOR-COMPARISON`、`verify-release.py` 的 semantic 检查。
2. **Provider 晋级证据推进**：14 个 Experimental→Beta 需跨 target named CI run（billing 解锁后）+ owner 决策；仓库内可先行的是把每个 provider 的"Beta 本地缺口 vs 外部缺口"结构化到 `AdapterManifest`/矩阵文档（避免"机制存在=证据存在"的误判），并补齐 context/handoff/incremental 覆盖（对 partial 的 claude-code/codex/aider 尤其）。
3. **context/handoff 工具活动富化**：schema v12 已有 tool_activities，但 `handoff_pack.rs` 仍 `tool_activity: Vec::new()`、context 视图不投影 sidechain/tool activity（任务 `08-15-evidence-handoff-pack` 要求 pack 带 tool activity；schemas/handoff/v1 已预留字段）。需加批量读取、预算/脱敏、golden/契约测试。
4. **TUI facet 控件 + Robot capability UI**：CLI/MCP 已有 facets，TUI `SearchFacets::default()` 是显式 deferred；加控件后更新五入口 harness。
5. **ToolActivity retention/cleanup 策略**：表/索引已有，生命周期策略 deferred（WriterLease/CAS 下安全修剪 + 审计 + rebuild 不变量测试）。
6. **provider-scoped session identity 迁移**：`08-15-unified-release-contract/design.md` §Deferred 的 `ses_v2` wire ids + `installation_namespaces` registry + `id_alias` TTL + backfill（RFC-0001 §5.1 open debt）——最大的剩余本地 feature。
7. **语义模型相关附属**：frozen corpus/query manifest、benchmark SLI 报告、`model install/import` 测试矩阵。

### 9.2 外部/owner blockers（仓库内无法"修复"，如实记录；每个都有明确解锁者）

| # | 阻塞项 | 谁解锁 | 需要什么证据/动作 |
|---|---|---|---|
| 1 | GitHub Actions billing/spending-limit | owner（GitHub account） | 恢复 billing 后重跑同一 SHA，记录 named successful run id → 填 `last_certified_targets`、晋 Beta 的前置 |
| 2 | PRIVATE→public 切换 | owner | 选定 Option A（公开树导出器已就绪）或 Option B（filter-repo 重写，runbook 完整）；公开前跑导出器 + privacy scan 0 findings |
| 3 | 历史清洗执行 | owner | 按 PUBLIC-HISTORY-SCRUB.md 在 disposable fresh clone 里执行；本仓库不做破坏性重写 |
| 4 | tag + GitHub Release + 签名 | owner + 凭据（Authenticode/notarization/cosign） | release.yml 已配置（`--verify-tag`、四 target、SHA256SUMS）；signing/attestation 字段诚实标 false 直到有凭据 |
| 5 | SBOM / NOTICE / REUSE 审核 | owner + approver | `REUSE-LICENSE-AUDIT.md` Draft→Accepted；`THIRD-PARTY-DEPENDENCIES.json` 诚实非 SPDX；不凭空生成 NOTICE/版权头 |
| 6 | Provider maturity 晋级 | owner + 独立审查 | 0 Beta 现状；≥5 Beta（roadmap PRD）需 evidence + 跨 target run + ADR-0010 记录 `accepted_at` |
| 7 | ADR-0009/0010、THREAT-MODEL 签署 | owner/approver | 文件头部结尾补 `accepted_at`/签名；签署前不得划掉 Proposed/Draft |
| 8 | macOS 全新环境演练 | billing 解锁后 CI | rehearsal-runbook §10.2/§11 已填实（2026-08-17），跑 Gate D 性能终检并记录 run id |
| 9 | 竞品对比表发布基线 | owner | 13 可引用 + cass clean-room 边界；发布 benchmark 用固定 clone commit 复现（口径说明已写） |

**禁止事项**：不得以"修 CI"名义改代码伪造跨平台认证；不得把 `bigram-hash-v1` 宣传为语义模型；不得代 owner 签署 ADR/威胁模型；不得把 deferred 项划掉；不得对真实用户会话数据做修改/上传/提交。

---

## 10. 治理与协作规则

- 仓库 `CLAUDE.md` 是硬约束：**未经用户明确请求不 commit/push**（例外：本次发布驱动任务中用户已显式授予自主 commit/push/merge 权限，继续沿用但保持 Conventional Commits 纪律：`type(scope): subject`、一提交一逻辑、不 `git add .` 盲加、不加 AI co-author/footer、不 amend/squash 已推送提交、破坏性命令禁）。
- 质量门（§7.1）绿是 commit 前提；提交前自己跑。
- 对外中文交流（用户语言），工具/commit 消息英文；PR 目标 main，合并是 owner 决定。
- 隐私：真实路径/hostname/身份不得进代码/测试/fixture/docs/commit；不得提交 secret；provider 原始 transcript 只读、绝不外传。
- 参考源码边界：`Github_src` 13 个可自由引用（MIT/Apache 按 REUSE 矩阵保留 attribution），cass 仅 clean-room 思路禁读源码直接复制。
- memory 使用：写前先 Read 目标 memory 文件；`MEMORY.md` 只放一行索引。
- 大型搜索（>3 步）委派子代理做，主对话收结论；子代理结论只取结论不搬原文。

---

## 11. 给新 Agent 的第一步（30 分钟确认环境）

1. `git -C C:\AgentSessions\.claude\worktrees\public-release-audit status --short --branch`（应显示 `worktree-public-release-audit` 分支，HEAD=`dc704df`，工作树干净；若在别的目录工作，先建自己的 worktree）。
2. `git fetch origin` 确认 origin/main=`dc704df`。
3. 跑 §7.1 全门一次，确认你接手的环境与文档一致（预期：fmt/clippy/全部 Rust+Python 测试绿、privacy 0 findings、deny ok）。
4. 读四个文件建立心智模型：`CLAUDE.md`、`docs/product/OPEN-SOURCE-ROADMAP.md`、`docs/product/PROVIDER-MATURITY-MATRIX.md`、`crates/agent-session-grep-ports/src/capability.rs`（矩阵单源）。
5. 读 `crates/agent-session-grep-cli/src/main.rs` 的 dispatch 与 `crates/agent-session-grep-application/src/lib.rs` 的 App 入口，理解五个入口共享 ADT。
6. 用 `cargo run -q -p agent-session-grep-cli -- --db <临时> search test`（或 `--robot`）各跑一条，感受 envelope 形状。
7. 再从 §9.1 选一个任务开工（推荐 #1 语义模型 或 #3 context 富化），按 §10 纪律提交。

## 12. 附录：速查

- 关键测试名：`provider_matrix`（16 行漂移）、`published_envelope_schema_contains_runtime_contract`、`value_flag_skip_lists_*`、`list_providers_enumerates_the_registry`、`search_echoes_non_default_facets_like_the_cli_robot_surface`、`sync_indexes_tool_activities_and_search_facets_filter_them`（e2e）、`network_egress`、`privacy`（Python）、`real_data_regression`（E2E，需授权 root）。
- 环境坑：release 构建 `os error 5`（旧 MCP 进程锁 exe）→ `Stop-Process` 后重试；push Schannel 失败 → `git config http.version HTTP/1.1`；CLI crate 无 lib target → 单测用 `--bin agent-session-grep`；`scripts/evidence/out/*.json` 勿入库。
- 常见常量：`SCHEMA_VERSION=12`、`BIGRAM_HASH_MODEL_ID="bigram-hash-v1"`、`BIGRAM_HASH_DIMENSION=384`、Robot `schema_version="1.1"`、handoff `schema_version="1.0"`、9 MCP tools、16 矩阵行、exit 0/2/4/6/7/9/10。
- 本文档由 2026-08-17 release 收口 wave 结束时编写；此后任何改动应同步更新 §8/§9 与 HEAD 引用，保持交接文档永远可交付。