# 实施与验证：Hermes SQLite 变体

## Ordered steps

1. 读取 ports/sqlite spec 与归档 provider 探针/上游证据，确认现有 Hermes crate 结构与变体分派点。
2. 实现只读有界快照读取 + schema 探测（歧义拒绝）。
3. 实现 canonical 映射（排序、REAL 秒、profile 命名空间、两种 tool-call、NULL 语义）。
4. 合成 fixtures + PROVENANCE；probe/parse/search golden 与源不可变、并发提交、超限、坏行测试。
5. 跑 workspace 受影响范围 + 全量 provider golden。
6. 更新 provider 矩阵文档（仅在证据齐备时提交晋级建议；否则记录缺口）。
7. 提交（`feat(provider-hermes): add bounded sqlite state variant`）并合并 main。

## Validation commands

```text
cargo fmt --all --check
cargo clippy -p agent-session-grep-provider-hermes --all-targets --offline -- -D warnings
cargo test -p agent-session-grep-provider-hermes --offline
cargo test -p agent-session-grep-cli --offline   # provider 选择/搜索集成
cargo test --workspace --offline --no-fail-fast
git diff --check
```

## Rollback

新增变体独立于既有 JSON 变体；回退即 revert 该提交，不影响已登记 provider 行为。
