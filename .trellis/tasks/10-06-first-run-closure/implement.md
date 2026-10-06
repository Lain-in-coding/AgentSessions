# 实施计划：first-run-closure

## Steps

1. 绘制现状：db flag 解析/默认值路径、缺库读命令行为、README quickstart 现状（记录文件:行）。
2. 最小改动：提取单一默认路径解析函数；缺库读命令的 fail-closed 错误路径与 sync 指引；必要的结果投影微调。
3. 隔离实验：temp HOME + temp DB，跑 Quickstart 五步并留日志。
4. 文档同步：README（或对应 quickstart 文档）与 --help 对齐。
5. 校验与 diff 审查（只动必要文件）。

## Validation commands

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b3 -- -D warnings
    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b3
    node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs
