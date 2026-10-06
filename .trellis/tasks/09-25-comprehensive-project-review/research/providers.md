# Provider Workstream Review（截至 2026-09-27）

## Coverage

工作区 provider crates 覆盖 Claude、Codex、DeepSeek Harness、Grok Build、Antigravity、OpenCode、Pi、Aider、Cline、Cursor、OpenClaw、Hermes、Kimi Code、ZCode、Qoder、Tencent CodeBuddy，以及 provider matrix、testkit 和 shared staging。

## Confirmed baseline findings and disposition

| 编号 | 问题 | 当前状态 |
|---|---|---|
| P1-2 | OpenCode/Cursor 一个 SQLite 源的多 session 边界丢失 | `a20e8ab` 的 multi-session staging、placement、resume/usage 回归已覆盖；当前本地 provider/workspace tests 通过 |
| P1-5 | provider filter 实现与 capability registry 漂移 | 当前 repair task 已将 canonical IDs/aliases/implemented 状态集中到 registry，并有 CLI/MCP/Web parity tests |
| P2-7 | OpenCode SQL prepare/row/schema 错误静默丢弃 | 当前 repair task 记录 fail-closed regression；不再把 schema drift 当成空成功 |
| P3-1 | Grok provider-controlled `u64` rewind 直接 cast `usize` | 当前 provider tests 包含 oversized rewind；改为 checked conversion，避免 32-bit 静默截断 |
| P3-2 | hook Unicode budget 与截断单位不一致 | 当前 hook contract 使用统一 conservative estimator；ASCII/CJK/emoji/JSON 合法性有回归 |

## Current release evidence

- 本地 workspace provider tests、golden fixtures 和 property tests通过；`cargo test --workspace --offline --no-fail-fast` 已完成。
- core-beta-evidence 在 Windows x64/MSVC、Ubuntu 22.04/GNU x64、macOS Intel/x64、macOS Apple Silicon/ARM64 通过，证明了该 workflow 的 targeted provider/evidence slices。
- 这些证据不等同于 16 个 provider 已达到 roadmap 要求的 certified/beta release maturity。`08-15-open-source-product-roadmap` 仍为 planning，其父验收项（16 provider 证据、Claude/Codex certified、至少 5 个完整 beta）尚未全部勾选。

## Open risks

- 没有真实 provider 安装目录和历史格式全量验证；只使用 synthetic/redacted fixtures。
- 没有 32-bit runtime proof，也没有把所有 provider 的跨版本/崩溃恢复/正式 target 证据汇总成发布 manifest。
- semantic optional stack 仍含 `paste` advisory path；对应 `09-25-semantic-dependency-advisories` 仍 planning，不能宣称依赖门已闭合。
- hosted generic CI 仍因 Clippy failure 不能证明其后续 provider/workspace test steps 在当前 PR head 上远端通过。

## Verdict

provider 层的已知代码缺陷大部分有修复和本地回归证据，但“所有 provider 和发布证据都完成”不成立；这属于发布路线图未完成，而不是把单个 provider test 通过误写成 GA。
