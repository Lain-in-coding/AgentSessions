# 实施计划：journal-retention

## Steps

1. 读 adapters-sqlite 现有 outbox/journal 结构（父实验锚点 adapters-sqlite/src/lib.rs:969-1061,6972-7027），列出 terminal batch 明细字段的消费者（恢复/幂等/诊断）→ 形成保留合同表。
2. 设计 aggregating compactor：版本化摘要 + 不可变字段保留；先 preview 后执行（分离两个入口）。
3. 实现 + 迁移：旧读路径保持；新布局读路径；fail-closed。
4. 测试：父实验复刻 soak、中断/重入、并发读、旧格式、未知格式、preview 断言。
5. 复核可观察语义不回退（恢复/重放/冲突/relocation/generation）。

## Validation commands

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b2 -- -D warnings
    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b2
