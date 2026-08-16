# Codex 交接报告 — agent-session-grep 开源就绪状态（2026-08-17 更新）

- **Date**: 2026-08-17（本报告由 2026-08-16 版本更新；主会话已闭合全部本地 P0/P1 功能波）
- **Main HEAD / origin/main**: `2fdbb093cac1308d0f259f2ee5d6bb668e581988`
- **工作区**: clean（P0-5 已合入；ToolActivity 移植由集成 agent 在隔离 worktree 进行中）
- **Audit basis**: 10 路并行只读审计（2026-08-16）+ 本轮闭合的 P0/P1 功能波

---

## 0. 一句话结论

**项目当前 NOT_READY_EXTERNAL_BLOCKERS：所有本地可闭合的 P0/P1 已落地并通过质量门与发布验证；剩余阻断全部是外部/owner 侧（GitHub Actions 欠费、provider maturity 门禁、公开/签名/治理签署）。** 交接给 Codex 的首要任务是闭合 P0-4（billing 修复后跑 named successful run + SBOM/NOTICE/REUSE），然后跑出三平台干净环境证据，把公开决定留给 owner。

---

## 1. 已合入 main（本轮之前 + 本轮闭合）

### 本轮之前已合入（2026-08-16 交接前）
`97e1324`（AdapterManifest）、`f4787c9`（providers 子命令）、`c5e7292`（ADR-0010）、`989c034`（源 checksum）、`243d7d5`（Claude/Codex 只读断言）、`432744f`（Codex resync e2e）、`4086f55`（release workflow）、`ccb89a1`（发布证据文档）、`8236f59`（Contributor Guide）、`7849a3c`（Dependabot + security-audit）、`e506288`（宣传口径修正）、`919b48e`（asg 别名三平台安装器）、`9014546`（版本字符串修正）。

### 本轮闭合的本地 P0/P1 功能波
| Commit/波 | 内容 |
|---|---|
| `bfb5f21` + `86f30b0` | **P0-6 bounded ingestion**：`ReadOnlySource`/`BoundedLineReader`，per-format 上限（JSONL 8MiB/record、JSON 族 32MiB、SQLite 128MiB），`SourceTooLarge`/`RecordTooLarge` fail-closed |
| `cd59ad1`（privacy sweep） | **P0-1 隐私清扫**：`privacy_scan.py` 扫描 tracked 文本个人/机器绝对路径（当前 HEAD 0 findings）；`PUBLIC-HISTORY-SCRUB.md` 记录历史改写决策 |
| `1748a63` | **session-metadata search（SQLite v11）**：`session_fts` 投影只索引 resolved provider session id / pair-observed cwd / 首条 user request；`group_by_session` 去重 |
| `ea983cb` 等 | **handoff determinism 契约**：时钟无关 `created_at`/`pack_id`、真实 token/byte 预算三级截断、共享 `redact.rs`、权威 source locator、MCP `generate_handoff`（9 工具） |
| `123d837` | **`--offline` 全局 flag + hook provider/time filters**：fail-closed `capability_not_supported`（exit 7）、零出口静态审计（`network_egress.rs`）、`--provider`/`--decay-days` |
| resume 波 | **resume 执行契约**：first-run 强制预览 marker、provider binary preflight、dry-run 默认、capability 矩阵↔builder drift 测试 |
| 12-provider golden | 12 个 provider 合成 fixture + PROVENANCE.md + pinned canonical output + span round-trip + 只读 checksum（grok/antigravity/pi/kimi/openclaw/qoder/codebuddy/hermes/hermes/opencode/cursor/aider/cline） |
| serve hardening | loopback worker pool + Host/Origin/CSRF/体积限制 + `/api/projection/search` 别名 |
| `89d5081` | **P0-5 代码件**：`tui --snapshot-json <query>` headless 投影 + serve projection 别名 |
| `f4c1da1` | **P0-5 harness**：五入口一致性 harness 去 skip-as-pass（Web 真实 loopback serve + TUI 快照），`verify-release.py` 扩到 10/10 |
| `2fdbb09` | **P0-5 docs**：runbook 更新到 landed 功能 + `go-no-go.2026-08-16.md` No-Go 草案 |

### 已全链路人工验证（Windows 本机，release build）
- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` 全绿（含去 skip 后的五入口 harness e2e）
- `verify-release.py` → **10/10 checks passed**
- `compare_entrypoints.py` → **overall_verdict=consistent**，5 入口全部直接比对，skipped/aliases/unimplemented 全空
- `open_source_gate_benchmark.py` → 4/4 gated 指标 measured+pass（lexical_recall 1.0、parse_loss 0.0、discovery_coverage 1.0、resume_handoff_success 1.0），semantic/hybrid informational
- `smoke.ps1`（release）→ 33 项断言全过；MCP 9 工具 handshake
- Python 单测 `test_compare_entrypoints.py` + `test_verify_release.py` → 10 项全过

---

## 2. 仍开放：本地 P0（Codex 优先）

| # | 缺口 | 证据 | 建议修复 |
|---|---|---|---|
| P0-4 | release pipeline 已配置但从未有 named successful run；无 SBOM/NOTICE/third-party attribution；`ci_configured_only` | audit-release-ci | **修 billing 后**跑 exact SHA 的 4-target run，记录 named successful run 才升级 `ci_configured_only`；补 NOTICE/REUSE audit 收尾 |

（P0-1 隐私清扫、P0-5 release rehearsal、P0-6 bounded ingestion 均已在当前 main 闭合。）

---

## 3. 仍开放：本地 P1

1. **ToolActivity 移植合入 main**：集成 agent 正在隔离 worktree 把 `agent-a79f8db9b001ffc53`（baseline 058b434 → tip ed761cd，3 commits：`ba3a35d` domain+ports / `fe48b0a` sqlite+application schema v12 / `ed761cd` cli+providers+tests）cherry-pick 到 current main，冲突解决优先级：main 的 v10→v11 session_fts 迁移保持不变、在其上新增 v11→v12 tool_activities；保留 bounded ingestion/offline/serve-hardening/12-provider golden。合入后需复跑质量门。**已知 deferred**（与参考实现一致）：context enrichment（上下文视图携带活动）、TUI facet 控件、Robot capability 界面、retention/清理策略、source_fs capture/verify 流式化。
2. **core-beta source-shrink 的 store-layer edge-integrity 缺陷**（既有）：`cannot delete a catalog entity still referenced by message edges` —— 基准测试 source-shrink 步骤触发，待修。
3. **Cline `source_span: derived`** 实为 array-index pseudo-span（非 byte span），需修正或降级。

---

## 4. Provider 域结论

- 16 行矩阵事实准确（14 Experimental + 2 deferred）；12 个 provider 现已有外置 golden/PROVENANCE/property/span round-trip（本轮闭合），`capability.rs` 陈旧 "2 providers" 注释已修。
- PRD 要求 ≥5 个 Beta + Claude/Codex certified，当前 **0 个 Beta**（仍是宣传/门禁缺口）。
- `tool_activity: Partial` 在 capability.rs/manifest.rs 诚实声明（claude-code/codex/aider）。

---

## 5. External（只能 owner/平台）

- GitHub Actions billing/spending-limit（最近 100 次 run 全 0-step 失败，annotation 证实非代码问题）
- 仓库 `PRIVATE`→public 切换、创建 tag、GitHub Release
- Windows Authenticode / Apple notarization / cosign/OIDC / artifact attestation
- branch protection（private 计划下 API 403）
- ADR-0009 / ADR-0010 / REUSE audit / Go-No-Go 的 owner/approver 签署（`accepted_at`）
- macOS 干净环境 rehearsal（受 CI billing 外部阻塞）

---

## 6. 语义检索诚实口径（重要，避免宣传越界）

- 当前 "semantic/hybrid" = `bigram-hash-v1` fuzzy-lexical vectorizer（`embedding.rs` 文件头明言 NOT semantic）；gate 的 semantic/hybrid 指标为 informational（threshold/pass 均 null），manifest 强制该校验。
- 真实模型仅停留在研究文档（`.trellis/tasks/08-15-open-source-product-roadmap/research/2026-08-16-embedding-model-options.md`：Candle CPU + pinned multilingual-e5-small，`semantic-candle` optional feature，默认关闭）。**任何宣传不得称当前已支持语义检索。**

---

## 7. 建议 Codex 下一步（按序）

1. 接收 ToolActivity 集成 agent 结果：合入 3 commit 到 main，复跑 fmt/clippy/test + verify-release + 五入口 harness（确认 MCP 仍 9 工具）。
2. P0-4：修 billing 后对 exact SHA 跑 4-target evidence + release dry-run，记录 named successful run；补 NOTICE/SBOM/REUSE audit。
3. 修 core-beta source-shrink 的 store-layer edge-integrity 缺陷。
4. 三平台干净环境演练（Windows 本机已过；macOS 等 billing 恢复）。
5. 由 owner 决定 public + tag + release + 治理签署（ADR-0009/0010、Go/No-Go）。

---

_生成：主会话合并修复 + 发布验证后。仅供 Codex 接手参考。_
