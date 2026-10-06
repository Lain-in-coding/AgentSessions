# 实施计划：final-perf-regression

## Steps

1. 重建 release 二进制；复跑 measure.ps1 同口径实验（get/show/search × 20）。
2. 跑 Core benchmark（复用父任务工具链与 validator），记录 P95 与初始 sync/noop。
3. 读 repo_identity/current_repo_slug 当前实现，评估单子进程替代（如 config 文件读取）的契约风险；做/不做均给结论。
4. 维护性映射（wc -l 头部文件 + 职责/边界），必要时做一个最小提取并跑回归。
5. 文档同步 + 证据归档。

## Validation commands

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b7 -- -D warnings
    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b7
