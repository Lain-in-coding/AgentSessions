# 热路径修复：消除重复 Git 探测（B1）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准（D2：CLI-first，先兑现核心闭环与稳定性）。
> 依赖：无（与 B0 文档任务并行安全）。上游结论：父任务 review-report.md 第四节 P1-02。

## Problem（已实测）

普通命令在热路径上重复构造 App 并重复解析 repo：
- crates/agent-session-grep-cli/src/lib.rs:2082-2089 为读时钟建 App；:2145-2161 检索又建一次；:3058-3099 每个 App 临时解析当前 repo；repo_identity.rs:31-54 通常两次 git 子进程；get/show 也承担不需要的探测。
- 配对 20 次实测（同一 release、同数据、返回一致）：get 53.67→9.15ms、show 52.44→8.97ms、search 97.11→9.98ms。

## Goal

按请求消除重复/无关 Git 探测：
1. 取时钟不构造带环境副作用的 App。
2. 仅 Search/Handoff 等确实需要 repo-aware 排序的用例解析 repo；每次请求最多一轮解析并复用。
3. get/show/status 等按 ID 纯读取零 Git 探测。
4. 契约不变：同 repo boost、无 origin 时 None、cursor 固定评分时钟、显式过滤、机器输出枚举、hits/score/page/cursor 语义全部不变。

## Non-goals

- 不引入跨请求永久缓存 origin（MCP/Web 长生命周期有失效语义风险）。
- 不重写 CLI 框架、不换参数解析器、不改协议/退出码。
- 不让用户设置 ASG_CURRENT_REPO 才能快——该变量仅测试注入用。

## Acceptance Criteria

- [ ] get/show：0 次 Git 子进程（用既有探测注入/包装统计）。
- [ ] 普通 search：≤1 轮 repo 解析（有 origin 时最多 2 个子进程），结果与修复前逐字段一致。
- [ ] 配对实验复跑（20 次/变体）：get/show 中位数回到 ~10ms 量级；search 保留 repo-aware 排序的合法成本，报告实测中位数。
- [ ] cargo fmt --all --check、cargo clippy -p agent-session-grep-cli --all-targets --locked -- -D warnings、cargo test -p agent-session-grep-cli --locked、cargo test --workspace --locked 全绿（isolated target dir）。
- [ ] 现有 CLI/MCP/Web 测试不回归；新增针对“get/show 零探测、search 单轮解析”的回归测试。

## Rollback

保留旧 factory 路径可切换（或 revert 单 commit）；不改 schema/协议，无数据迁移。
