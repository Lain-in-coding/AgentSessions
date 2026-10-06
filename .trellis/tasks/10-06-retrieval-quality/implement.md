# 实施计划：retrieval-quality

## Steps

1. 读 D3 matrix 边界锚点与现有 semantic/hybrid 取数链（adapters-sqlite/src/lib.rs:8201-8230 一带）+ application ranking.rs:44,67-79。
2. 实现 evidence gate（adapter 取数窗口内）+ final_score 防御，先写失败测试。
3. 正名与 README 口径（小改）。
4. holdout 构造与评测脚本；跑回归集 + holdout；阈值扫描。
5. 汇总结论：晋级或保持 optional/experimental（诚实）。

## Validation commands

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b4 -- -D warnings
    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b4
