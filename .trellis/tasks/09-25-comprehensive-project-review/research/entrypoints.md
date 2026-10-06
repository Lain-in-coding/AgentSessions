# Entrypoint Workstream Review（截至 2026-09-27）

## Covered surfaces

CLI、Robot protocol、MCP 2025-06-18、loopback Web、TUI、hooks、resume/handoff，以及安装脚本、schema 和 CI/release workflows。

## Findings and disposition

| 编号 | 表面 | 当前状态 |
|---|---|---|
| P1-4 | MCP semantic/hybrid 生成 embedding 后走 `NoSemanticIndex`，必然 lexical fallback | repair task 已接入共享 semantic application path；当前 semantic-candle 与 application tests 本地通过，MCP/CLI contract tests存在 |
| P1-5 | provider filter canonicalization 分叉 | 已集中到 capability/registry，并覆盖入口 parity；仍需 roadmap 的完整 capability evidence |
| P3-2 | hook budget bytes/chars 不一致 | 已统一估算和截断边界；hooks tests 本地通过 |
| P3-3 | Web search 是 subset，未暴露全部 facets/max_bytes | 当前 Web 仍是明确受限 subset；代码修复增加已有参数和 stale/out-of-order response tests，但 full Web parity 仍是独立 roadmap 门，不应称为完成 |
| TUI | semantic/hybrid/provider/time/repo/group 等能力面 | 当前合同按 lexical/read-only subset 诚实表达；完整 parity 未承诺，也未满足开源路线图的全入口发布门 |
| CI/release | hosted quality gate | PR #12 的 installer smoke、core-beta-evidence、cargo-deny、security-audit 通过；通用 `ci` 三平台 Clippy 失败，PR `UNSTABLE`、未 merge |

## Cross-entry verification

本地验证：

- application semantic feature：287 passed。
- workspace debug tests：通过。
- Node Web tests、Robot-related tests、Python scripts/release/evidence suites：本地通过。
- `cargo fmt` 和本地 Clippy：通过。

远端验证：

- `core-beta-evidence`、installer smoke、cargo-deny、security-audit：通过。
- generic `ci` 在 `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8304` 触发 Clippy 1.98 lint，后续 generic workspace test/Web/semantic/Robot/Python steps 未执行；本地通过不能替代 hosted green gate。

## Release-level gaps

- `08-15-open-source-product-roadmap` 仍 planning。
- benchmark/install gate 与 final integration rehearsal 两个 P0 子任务仍 planning，验收项未完成。
- ADR-0009、owner Go/No-Go、三平台 clean-environment rehearsal、五入口 canonical comparison 和 release evidence manifest 尚未形成完整闭环。
- PR #12 未 merge；不得把 open PR 的本地验证写成已发布能力。

## Verdict

入口代码的主要审计缺陷已有修复证据，但跨入口完整发布验收和 hosted CI 尚未闭合；当前应标记为“实现大体完成、发布未完成”。
