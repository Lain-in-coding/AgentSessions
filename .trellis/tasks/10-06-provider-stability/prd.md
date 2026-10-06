# Provider 稳定性：Claude/Codex 生命周期证据与矩阵同步（B5）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准。
> 依赖：无（与 B4 并行需避免同文件：本任务主写 provider 两个 crate + provider 矩阵文档；adapters-sqlite 只允许读）。上游：review-report P1-06、父 design.md 第 7 节。

## Goal

把 Claude/Codex 的支持声明变成可追溯证据：版本/变体/OS 边界明确、生命周期（append/shrink/同长改写/分叉/移动/SQLite WAL）有 golden/property 测试、矩阵与实测一致；无 span 不伪造 offset；长尾保持 experimental，不为“收敛”删能力。

## Requirements

1. 生命周期测试补强（provider-claude / provider-codex）：append、shrink、同长改写、分叉（parent/父子边）、文件移动、SQLite WAL 读取；使用合成/授权 fixture，不碰真实用户数据。
2. 证据与声明一致：docs/product/PROVIDER-MATURITY-MATRIX.md 与 PROVIDER-BETA-READINESS.md 的每条声明要么指到测试/证据锚点，要么标注 experimental/未验证；不新增无法支撑的“native/resume 成功”口径。
3. 无 span 的源（SQLite/整文件 JSON）继续 precision=unknown，不伪造 byte offset（对照 crates/agent-session-grep-ports/src/capability.rs 的 source_span 约定）。
4. 失败语义：不完整解析不得推进成功水位/覆盖 last-good（与 D3 #3 一致，回归锁已在 adapters-sqlite）。

## Non-goals

- 不新增 provider；不改长尾认证等级；不执行真实 native resume（环境限制，如实记录）。

## Acceptance Criteria

- [ ] 六类生命周期场景（append/shrink/rewrite/fork/move/WAL）各有测试或明确 N/A 理由（含 file:line 锚点）。
- [ ] 矩阵文档同步：涉及行有锚点/标注，无无证据的 native/完整度声明。
- [ ] offset 伪造专项测试（evidence precision=unknown 路径）通过。
- [ ] cargo fmt/clippy/test（workspace，isolated target dir）全绿。
