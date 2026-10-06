# 搜索热路径：repo 探测并发化（契约不变）（B8）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准 D2（CLI-first）。
> 依赖：B1（4a1aa28）、B7（本次收口）。上游证据：B7 `research/residual-hotpath-verdict.md`、`hotpath-comparison.md`。

## Goal

在**不改变** `current_repo_slug` None/Some 契约、cursor digest、排序 boost 的前提下，把 search 每请求的 2 个**串行** git 子进程改为**重叠执行**，拿回 B7 判定中"无法通过单子进程/文件系统替代获得"的那 ~19.5 ms。

## Requirements

1. `GitRepoSlugResolver::resolve` 在 by_directory 缓存未命中时同时派发
   `git -C <cwd> rev-parse --show-toplevel` 与 `git -C <cwd> remote get-url origin`，两者重叠执行。
2. 门禁语义逐字不变：**toplevel 为 None 一律返回 None**（bare repo / `.git` 内 cwd / 非仓库的现有 None 结果必须保持）。
3. 两级缓存语义不变（by_directory 记 toplevel 含 None；by_toplevel 记 slug 含 None）；URL 归一化路径不变。
4. 不新增依赖（只用 std），不改协议/schema/退出码。
5. 若实测收益 < 5 ms，或任一上下文语义无法逐字保持，则回退并如实报告。

## Non-goals

- 不做契约扩张（bare repo / `.git` 内 cwd 的 None 语义不许改）——B7 已判定那属于 owner 决策。
- 不做跨进程 slug 缓存、不自行解析 `.git`（B7 候选 B/C，高风险）。
- 不动 provider/.github；不做无关重构。

## Acceptance Criteria

- [ ] 语义证伪性测试：普通仓（含子目录）、嵌套仓、bare repo、`.git` 内 cwd、非 git 目录五种上下文判定与改动前逐个一致。
- [ ] `crates/agent-session-grep-cli/tests/hotpath_repo_probe.rs` 仍断言 search=2 探测 / get=show=0 探测（禁止放宽）。
- [ ] 同口径实测（measure.ps1，20 次）：search 中位数较 B7 的 48.02 ms 显著下降，git_detect − seam 的单轮成本从 38.94 ms 收敛；原始 JSON 落 research/。
- [ ] 变体输出逐字段一致（仅 `meta.duration_ms` 允许变化）。
- [ ] cargo fmt/clippy/test（workspace，isolated target dir `.trellis/.runtime/target-b8`）全绿。
