# Codex 交接报告 — agent-session-grep 开源就绪状态（2026-08-16）

- **Date**: 2026-08-16
- **Main HEAD / origin/main**: `9014546f73376d2c371c3fc08be6e227536654c8`
- **工作区**: clean（本报告前已完成全部可本地闭合的合并与推送）
- **Audit basis**: 10 路并行只读审计（trellis-tree / quality-gate / retrieval / entrypoints / privacy-security / release-ci / providers / resume-handoff / activity-hooks-serve / docs-consistency / git-debris）+ 本轮额外闭合的 P0 修复。

---

## 0. 一句话结论

**项目当前 NOT_READY_LOCAL_BLOCKERS：核心产品面与门禁证据大体齐备，但仍有本地可闭合的 P0/P1 缺口，且 GitHub Actions 因账户欠费无法运行。** 交接给 Codex 的首要任务是闭合剩余本地 P0、跑出三平台成功证据，然后把公开决定留给 owner。

---

## 1. 已完成并合入 main（交接前已推送）

| Commit | 内容 |
|---|---|
| `97e1324` | 结构化 `AdapterManifest`（14 provider，Claude/Codex fixture_revision=1，其余 null，last_certified_targets 空） |
| `f4787c9` | Provider maturity CLI/Robot/human 投影（`providers` 子命令，数据来自 capability.rs） |
| `c5e7292` | ADR-0010 Provider maturity rollback（status Proposed） |
| `989c034` | 真实数据回归 `INV-SOURCES-UNCHANGED` 源 checksum |
| `243d7d5` | Claude/Codex probe/parse 只读字节断言 |
| `432744f` | Codex incremental resync/tombstone e2e |
| `4086f55` | tag-gated release workflow（4 target unsigned archive/SHA256SUMS/inventory） |
| `ccb89a1` | 发布证据文档 |
| `8236f59` | Provider Adapter Contributor Guide + issue/PR templates |
| `7849a3c` | Dependabot + weekly security-audit workflow |
| `e506288` | 版本/宣传口径修正（CHANGELOG 改为 Unreleased、删除 v0.1.0 已发布断言、COMPETITOR-COMPARISON 去掉 `--offline`/semantic 虚标、go-no-go 模板 v0.3.0→v0.1.0、README/SECURITY 0.1.x） |
| `919b48e` | **asg 别名三平台安装器落地**（Windows 双 exe；Unix symlink/wrapper；upgrade/uninstall 原子且拒绝无关 alias；CI 三平台验证两个命令）+ backend spec/README/CHANGELOG/INSTALL-AND-UPGRADE 同步 |
| `9014546` | 修正 main.rs/human.rs 硬编码 0.3.0→0.1.0 版本字符串 + OPEN-SOURCE-ROADMAP License 行 + CJK/offline 主张弱化 |

### 已全链路人工验证（Windows 本机）
- `install.ps1 -Prefix <tmp> -SkipBuild` → 安装 `agent-session-grep.exe` 与 `asg.exe`（版本一致）
- `smoke.ps1 -Binary … -AliasBinary …` → 33 项断言全过（doctor/sync/search/get/context/status/exit-code/MCP 8-tool handshake）
- `uninstall.ps1` → 删除两文件；第二次报告 "not installed" 退出 0
- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` 全绿（opencode 测试为 temp-file 竞态 flake，单独/复跑均通过）

---

## 2. 仍开放：本地可闭合 P0（Codex 优先）

| # | 缺口 | 证据 | 建议修复 |
|---|---|---|---|
| P0-1 | 公开树/历史含个人绝对路径（tracked 文件多处含本机用户目录与工程根路径，含 `provider-codebuddy/src/lib.rs:14` 与大量 trellis 文档） | audit-privacy-security / docs-consistency | 当前树替换为 `<repo>`/相对路径；历史由 owner 用 filter-repo/新公开仓库决策（见 `docs/operations/PUBLIC-HISTORY-SCRUB.md`，本报告发布前已由 privacy 清扫 commit 清理当前树） |
| P0-4 | release pipeline 已存在但从未有 named successful run；无 SBOM/NOTICE/third-party attribution；`ci_configured_only` | audit-release-ci / quality-gate | 修 billing 后跑 exact SHA 的 4-target run；补 NOTICE/REUSE audit 收尾 |
| P0-5 | 五入口一致性 harness 仍允许 Web/TUI skip-as-pass（`compare_entrypoints.py:30-32,446-475`、`e2e_consistency.rs:57-71`）；`verify-release.py` docstring 声称全流程但实际只 5 项 | audit-entrypoints | 让 Web 真实启动 loopback serve 比对；TUI 用 App projection 测试；runbook 更新到真实命令；产出 Go/No-Go 草案 |
| P0-6 | Provider ingest 仍整文件 `read_to_end`（`source_fs.rs:38,74`）、`ProviderAdapter::parse(&[u8])`（`ports/lib.rs:820`），违反 RFC-0002 §7 release-blocking bounded-buffer | audit-quality-gate | 引入 bounded reader/chunked stream；保留 fingerprint/source_changed 原子性；大文件内存上限回归 |

## 3. 仍开放：本地 P1（功能补强，不阻断首次公开但属宣传缺口）

1. **handoff-pack 未满足 PRD deterministic/budget/redaction**：`created_at` 用实时时钟（非字节确定性）、`max_bytes` 不参与裁剪、pack 内部 redaction 恒 default、source_document_id 直接用 hit.id、schema 路径 `schemas/handoff/v1/pack.schema.json` 与 PRD 的 draft 路径漂移；MCP/Robot 无 handoff 工具（仅 CLI）。
2. **resume 执行层未核验**：无 first-run 强制预览状态；Claude/Codex 无真实 provider spawn smoke；capability.rs 与 resume builder 不一致（opencode 标 Derived 但 builder 不支持、grok-build Unknown 但有模板）；`permission_mode` 恒 None。
3. **结构化 ToolActivity 未进生产**：`application/src/activity.rs` 是孤立 extractor；`agent-ae5601a055cc14728` worktree 有完整未提交实现（R1+R2 search facets：`SearchFacets/query_faceted`、SQLite v12、CLI flags），基于旧树需手工移植；session-metadata-search 类似（`session-metadata-search-08-15` worktree，SQLite v11）。
4. **serve hardening 未合入**：main serve 单线程、无 Origin 校验、无请求体积限制、Host 解析不支持 `::1`；`agent-a52175662fdf83038` worktree 有未提交 worker-pool/Origin/CSRF/limits 实现，基于旧树需移植 3 个文件 hunk。
5. **`--offline` 不存在**：仅 hooks.rs:14 注释与文档宣称；README/CONTEXT/runbook 需统一（或实现全局 flag）。
6. **Hook 高级 filter 未实现**：PRD 的 provider/time/decay/config 加载未落地（当前仅 `--enable`/`--max-tokens`）。

## 4. Provider 域结论

- 16 行矩阵事实准确（14 Experimental + 2 deferred）；但**仅 Claude/Codex 有外置 golden/property/span round-trip**；其余 12 个 `AdapterManifest.known_limitations=[]` 与公开限制不一致；`capability.rs` 注释仍称 "2 个 provider"（陈旧）。
- PRD 要求 ≥5 个 Beta + Claude/Codex certified，当前 0 个 Beta。跨 target provider evidence 已配置但无 named success。
- Cline 的 `source_span: derived` 实为 array-index pseudo-span（非 byte span），需修正或降级。

## 5. External（只能 owner/平台）

- GitHub Actions billing/spending-limit（最近 100 次 run 全 0-step 失败，annotation 证实非代码问题）
- 仓库 `PRIVATE`→public 切换、创建 tag、GitHub Release
- Windows Authenticode / Apple notarization / cosign/OIDC / artifact attestation
- branch protection（private 计划下 API 403）
- ADR-0009 / ADR-0010 / REUSE audit / Go-No-Go 的 owner/approver 签署（`accepted_at`）

## 6. 交接时的重要工作区/分支

- **已完成并吸收**：`a03b5a`(release)、`a6c097`(codex e2e)、`aa940`(read-only)、`afa53`(manifest)、`a8a4c4`(security)、`adc518`(contributor)、`a3a648`(docs+installer dirty) — 均已 cherry-pick 或人工合并进 main。
- **有独有价值但基于旧树的未提交/未合并实现（Codex 手工移植）**：
  - `agent-ae5601a055cc14728` — ToolActivity search facets（13 files，+3001/-29）
  - `session-metadata-search-08-15` — SQLite v11 session search（8 tracked）
  - `agent-a52175662fdf83038` — serve hardening（3 files，+1158/-451）
  - `agent-a4584f60772294ec2` — release rehearsal 一致性/验证（6 tracked）
- **陈旧/clean**：`08-16-semantic-candle` 空占位分支（无语义实现，可清理）；大量 f4175a0/8ef6a7c 旧 worktree 的 dirty 残留为早期 spike，审阅前勿批量删除。

## 7. 语义检索诚实口径（重要，避免宣传越界）

- 当前 "semantic/hybrid" = `bigram-hash-v1` fuzzy-lexical vectorizer（`embedding.rs` 文件头明言 NOT semantic）；gate 的 semantic/hybrid 指标为 informational（threshold/pass 均 null），manifest 强制该校验。
- 真实模型仅停留在研究文档（`.trellis/tasks/08-15-open-source-product-roadmap/research/2026-08-16-embedding-model-options.md`：Candle CPU + pinned multilingual-e5-small，`semantic-candle` optional feature，默认关闭）。**任何宣传不得称当前已支持语义检索。**

## 8. 建议 Codex 下一步（按序）

1. P0-1 隐私清扫 + `docs/operations/PUBLIC-HISTORY-SCRUB.md` + 复跑 privacy scan（若已由隐私 agent 完成则核对合入）。
2. P0-6 bounded ingestion（RFC-0002 §7 release-blocking）。
3. P0-5 五入口 harness 去 skip-as-pass + verify-release.py 补 context/resume/handoff/embeddings/hook + 生成 Go/No-Go 草案。
4. 移植 `agent-ae5601a055cc14728`/`session-metadata-search-08-15`/`agent-a52175662fdf83038` 三个未合并实现到 current main（逐个审阅+测试）。
5. handoff determinism/budget/redaction/schema 与 resume first-run/真实 smoke 补强。
6. Provider：为其余 12 个补外置 golden/PROVENANCE/property，`known_limitations` 接真实限制，修 Cline pseudo-span，capability.rs 陈旧注释。
7. 修 billing 后对 exact SHA 跑 4-target evidence + release dry-run，记录 named successful run 才可升级 `ci_configured_only`。
8. 由 owner 决定 public + tag + release + 治理签署。

---

_生成：本会话 10 路并行只读审计 + 主会话合并修复后。仅供 Codex 接手参考。_
