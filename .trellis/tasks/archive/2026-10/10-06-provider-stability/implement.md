# 实施计划：provider-stability

## Steps

1. 读 provider-claude/provider-codex 现有测试与 fixture 布局，列出六类场景的缺口表（research/scenario-matrix.md）。
2. 补测试：优先复用既有 fixture 生成器；新增合成变体（shrink/rewrite/fork/move/WAL）。
3. 矩阵同步：docs/product/PROVIDER-MATURITY-MATRIX.md 与 PROVIDER-BETA-READINESS.md 逐条核对（不改等级，只补锚点/限定）。
4. offset 伪造专项测试（无 span 源 → precision unknown）。
5. 全量门禁 + 证据归档。

## Validation commands

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b5 -- -D warnings
    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b5
