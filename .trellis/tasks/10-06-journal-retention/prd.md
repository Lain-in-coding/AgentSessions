# journal 治理：terminal 明细有约束 compact（B2）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准 D1（有约束治理）。
> 依赖：D3 第 3/4 项证据（已并入）；上游：review-report P1-03、父 design.md 第 4 节。

## Goal

在“可靠性优先、未决记录不可删”的前提下，让 terminal 批次的完整明细可被有约束地聚合/压缩，遏制 21 次单条改写 → store 1.37MB→6.07MB 的无界增长。

## Requirements

1. 定义保留合同：永久保留 = building/未决操作、generation、digest、恢复/幂等/冲突检测必需字段与身份；可聚合 = terminal 批次的重复明细（保留可验证摘要与可解释的审计损失边界）。
2. 提供显式 compact 维护操作（默认不自动执行）：先 preview（显示将聚合的内容、预计体积收益、受影响记录数），确认后才执行。
3. 格式向前兼容：新布局带版本标记；旧格式可读；未知格式 fail-closed；中断可重入、并发读安全。
4. 不改可观察语义：恢复/重放/冲突检测/relocation/generation 水位全部保持；catalog/FTS/关系原子一致。
5. 禁止：为省空间删除整个 outbox；静默丢日志；把失败记录当 terminal 处理。

## Acceptance Criteria

- [ ] retention contract 文档化（哪些字段保留/聚合/删除边界），与实现常量/结构一致。
- [ ] 复刻父实验（200 消息、21 次单条改写）：compact 前/后 store 与 manifest 增长数字；聚合后恢复/重放/冲突路径全过。
- [ ] compact 中断注入测试 + 重入测试 + 并发读测试 + 旧格式兼容测试 + 未知格式 fail-closed 测试。
- [ ] preview 输出含体积收益与受影响记录数（测试断言）。
- [ ] cargo fmt/clippy/test（workspace，isolated target dir）全绿。

## Non-goals

- 不做自动周期 GC；不改变 sync 对外语义；不动 FTS/catalog 结构。
