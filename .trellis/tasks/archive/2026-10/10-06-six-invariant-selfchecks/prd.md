# 六项不变量专项自查（D3）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan（第三节·B“对 ASG 的直接含义”）。用户 2026-10-06 批准为实施前置。
> 依赖：与 B1 可并行（写集=测试/研究，不动生产逻辑）。

## Goal

把竞品踩过的六类坑变成 ASG 的可执行自查证据。本任务只验证并记录；失败项输出精确锚点+修复建议，由后续子任务修复，不在本任务内改生产行为。

1. cap 先于过滤（含 semantic/hybrid/ANN 路径）：域外高排名 > cap、域内仍有合法命中时不得漏。
2. 零证据升格：文字/语义证据为零的结果不得仅凭 recency/repo boost 入选。
3. 失败固化：解析/同步失败不得推进成功水位、不得覆盖 last-good、不得标记 current。
4. 裁剪视图进缓存：任何展示/预算裁剪不得被持久化为“新鲜全量”（对比 agf AGF-03）。
5. resume 全参数化：恢复命令必须 typed intent + argv/cwd 分离；无 shell 字符串拼接（对比 agf AGF-02）。
6. 投影截断共病：索引/导出/展示共用截断投影时不得声称全文可检索（对比 hstry H-01）。

## Acceptance Criteria

- [x] 六项各自有可执行证据：新增测试（优先）或可复现 fixture 脚本 + 原始输出日志。
      2026-10-06：新增 4 个测试文件 / 9 个测试（`invariant_search_gates.rs`、
      `invariant_failure_semantics.rs`、`invariant_projection_consistency.rs`、
      `invariant_resume_args.rs`），原始日志见 `research/focused-invariant-tests.log`
      与 `research/workspace-tests-isolated.log`。
- [x] research/invariant-matrix.md：每项一行 = 结论(通过/失败/不适用)+证据命令+锚点+若失败的最小修复建议。
- [x] 新增测试在 cargo test --workspace 与目标 package 测试中可跑（不 flaky、无网络、无真实用户数据）。
      2026-10-06：`cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-invariants`
      = 85 套件 / 1768 passed / 0 failed（exit 0）。
- [x] 不修改任何生产行为（允许 cfg(test) 辅助与新增测试文件）。
- [x] 失败项必须给出 file:line 锚点与建议归属（B4 检索 / B5 provider / B2 journal 等）。
      2026-10-06：六项结论均为通过；2 处边界（final_score 零证据代数、语义 top-k
      无阈值）与 1 处文档措辞建议已带锚点归入 B4/B0，见矩阵第 2/6 行与第 4 节。

## Non-goals

- 不做性能调优、不改 schema、不改协议；不重跑竞品代码。

