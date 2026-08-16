# Codex 交接报告 — agent-session-grep 开源就绪状态（最终更新）

- **更新日期**: 2026-08-17
- **当前集成基线**: `5292f0f`（本报告为其后的文档收尾提交）
- **工作区状态**: 代码与测试已闭合；ToolActivity 已合入 main；本报告更新后工作区应保持 clean
- **结论**: `NOT_READY_EXTERNAL_BLOCKERS`

## 0. 一句话结论

所有当前可由代码仓库本地闭合的 P0/P1 功能已经合入 main，并通过 fmt、clippy、workspace tests、release verification、五入口一致性、privacy scan 和 smoke 验证。剩余阻断主要是 GitHub Actions billing、provider maturity 门禁、SBOM/NOTICE/REUSE 审批、仓库公开与签名发布等 owner/平台事项。

## 1. 本轮已完成

### ToolActivity search facets

以下 3 个 commit 已从旧基线手工迁移并合入当前 main：

- `c3263bb` — domain/ports：`ToolActivityKind`、`ToolActivityActor`、`ToolActivityStatus`、`SearchFacets`、sidechain/tool facet 契约。
- `f826cb5` — application/SQLite：ToolActivity 生命周期、去重、tombstone、facet 查询；schema v12。
- `83fe0ac` — CLI/MCP/providers：Claude/Codex 工具调用配对提取、CLI facet flags、MCP 参数与回归测试。

SQLite migration chain 已确认：

```text
v1..v9 → v9→v10 message_vec → v10→v11 session_fts → v11→v12 tool_activities
```

main 原有的 `session_fts` v10→v11 migration 未被覆盖；ToolActivity 只在其上追加 v11→v12。

已明确 deferred：context enrichment、TUI facet controls、Robot capability UI、retention/cleanup policy，以及 source capture/verify 的更深层流式化。

### 其他已闭合功能

- bounded ingestion：`ReadOnlySource`、`BoundedLineReader`、per-format source/record limits、oversize fail-closed。
- privacy scan：tracked text 个人路径扫描；当前 HEAD 0 findings；合成 golden 占位符使用精确 allowlist。
- session metadata search：SQLite `session_fts`，不索引 source path。
- deterministic handoff pack：稳定 `created_at`/`pack_id`、token/byte budget、redaction、MCP `generate_handoff`。
- resume contract：first-run preview marker、provider binary preflight、dry-run 默认、capability drift test。
- global `--offline`：未来网络能力 fail-closed 为 `capability_not_supported`；zero-egress 静态审计。
- serve hardening：worker pool、request bounds、Host/Origin/CSRF/forwarding-header guards、loopback only。
- 12-provider golden evidence：synthetic fixture、PROVENANCE、pinned output、span/checksum regression。
- release rehearsal：TUI `--snapshot-json`、Web projection、五入口 harness 去除 skip-as-pass、`verify-release.py` 10 checks。
- core-beta source-shrink：已修复 benchmark synthetic fixture 的跨文件 parent 链 bug；store layer 原有 edge-integrity guard 保持 fail-closed。

## 2. 最终验证证据

在 ToolActivity 合入后的当前 main 上：

- `cargo fmt --all --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：通过，0 failed。
- release build：通过。
- `scripts/verify-release.py`：10/10 checks passed。
- `scripts/rehearsal/compare_entrypoints.py`：`overall_verdict=consistent`；CLI/MCP/Robot/Web/TUI 全部直接比较，`skipped`、`aliases`、`unimplemented` 均为空。
- `scripts/install/smoke.ps1`：全部断言通过；MCP 9 tools handshake 通过。
- `scripts/evidence/privacy_scan.py`：0 findings。
- `open_source_gate_benchmark.py`：4/4 gated metrics pass；semantic/hybrid 仅 informational。

## 3. 仍然开放的本地 deferred

这些不再是当前首次开源的本地 P0 blocker，但需要在后续版本明确排期：

1. ToolActivity 出现在 context enrichment 视图。
2. TUI facet controls 与 Robot capability UI。
3. ToolActivity retention/cleanup policy。
4. source capture/verify 更深层的持续流式化与预算策略。
5. Provider maturity：当前仍为 0 Beta，未满足“至少 5 个 Beta + Claude/Codex certified”的产品门禁。

## 4. 外部/owner 阻断

1. GitHub Actions billing/spending limit：当前 workflow 仍出现 0-step failure，无法产生 named successful run。
2. release artifact 的 SBOM/NOTICE/REUSE 审计需要 owner/approver 完成签署；机器可读 dependency inventory 已存在，但不等同于法律审批。
3. 仓库仍需 owner 决定 `PRIVATE → public`、tag、GitHub Release 与 changelog 版本策略。
4. Authenticode、Apple notarization、cosign/OIDC、artifact attestation 等签名与供应链治理未完成。
5. ADR、Go/No-Go、REUSE 审计的 owner/approver 签署未完成。
6. macOS clean-environment rehearsal 受 CI billing blocker 影响尚未执行。

## 5. Codex 接手顺序

1. 先核对当前 `origin/main` 是否包含 `c3263bb`、`f826cb5`、`83fe0ac` 以及本报告收尾提交。
2. billing 恢复后，对 exact SHA 执行四目标 release workflow，保存 named successful run 与 artifact evidence。
3. 完成 SBOM/NOTICE/REUSE 与 provider maturity 的 owner 审批；不要把配置存在误报为发布证据。
4. 执行 Windows/Linux/macOS clean-environment rehearsal。
5. 最后由 owner 决定公开仓库、tag、release、签名与治理签署。

## 6. 诚实宣传边界

当前 `semantic/hybrid` 使用 `bigram-hash-v1` fuzzy-lexical vectorizer，不是 semantic model。任何 README、release note 或对外宣传不得把当前实现描述为已支持真实语义模型检索。

---

本报告只记录事实、验证结果和未决外部事项，不替代 owner 的公开发布决定。
