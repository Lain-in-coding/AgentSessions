# 收口复盘：热路径复测与维护性映射（B7）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准（D2 CLI-first）。
> 依赖：B1/B3/B4 已落地（HEAD 含 d864207/c41cbca/006746b）。上游：review-report P2-07、父 design.md 第 8/9 节。

## Goal

用当前 HEAD 重新测量热路径与核心 benchmark，确认前几波的收益与残余；只在证据充分且低风险时才做进一步优化；输出维护性映射（巨型文件/重复同步点）与建议，不搞无收益重构。

## Requirements

1. 复测（与父任务同口径）：get/show/search 探测次数与中位数；Core P95（search/show/get/initial sync/noop sync）；对比 audit 基线（53.67/52.44/97.11ms；P95 search 162.9ms、noop 16.22ms）输出前后表。
2. 残余热路径判定：search 仍保留一轮 repo 解析（2 个子进程）。评估是否存在**低风险**单子进程/文件系统替代（保持 current_repo_slug 契约与 cursor digest 不变）；若有且测试可证，实施；若风险高，仅给方案与数据，不实施。
3. 维护性映射：对仓库内最大的 3-5 个文件（adapters-sqlite/lib.rs、application/ranking 等）输出规模/职责/可提取边界/风险/收益表；只做“可量化收益”的最小提取，或明确标注推迟理由。
4. 文档：若 README/benchmark 文档数字因本轮改动过时，同步（不改历史记录，加时间戳新行）。

## Non-goals

- 不做大规模重构、不换框架、不新增功能；不为“看起来整洁”而拆文件。
- 不改协议/schema/退出码；不动 provider/.github。

## Acceptance Criteria

- [ ] 前后对比表（探测次数/中位数/P95）有原始日志与可复现命令。
- [ ] 残余优化：实施+测试，或给出不实施的理由与数据（二选一，均算通过）。
- [ ] 维护性映射表（含文件:行、边界、风险、是否建议执行）。
- [ ] 文档数字同步（如有过时）。
- [ ] cargo fmt/clippy/test（workspace，isolated target dir）全绿。
