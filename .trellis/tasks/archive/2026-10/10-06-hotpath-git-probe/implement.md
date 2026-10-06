# 实施计划：hotpath-git-probe

## Steps

1. 定位并绘制当前调用链：lib.rs 中 read clock App / search App / get-show 路径 + repo_identity.rs 的解析函数与注入点（ASG_CURRENT_REPO）。
2. 设计最小改动（见 design.md）：新增“无 repo 的只读装配”路径；把时钟获取改为不依赖完整 App；search 构造一次上下文并复用。
3. 写回归测试：统计 git 子进程调用次数（优先用现有注入点或 cfg(test) 钩子，避免为测试扩产品 API）。
4. 跑配对实验复现改善（原始数据保留到本任务 research/）。
5. 全量校验（见 PRD），并对同数据 json/jsonl 输出逐字段 diff。

## Validation commands

    cargo fmt --all --check
    cargo clippy -p agent-session-grep-cli --all-targets --locked --target-dir .trellis/.runtime/target-hotpath -- -D warnings
    cargo test -p agent-session-grep-cli --locked --target-dir .trellis/.runtime/target-hotpath
    cargo test --workspace --locked --target-dir .trellis/.runtime/target-hotpath
    node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs

## Review gate

- 契约不变是硬条件；任何 hits/score/cursor 变化视为失败而不是“优化”。
- 主会话复核 diff 与实测报告后才进入 check/收尾。
