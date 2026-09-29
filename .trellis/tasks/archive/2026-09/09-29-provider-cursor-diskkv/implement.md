# 实施与验证：Cursor cursorDiskKV 变体

## Ordered steps

1. 读取 ports/sqlite spec 与归档 provider 探针，确认现有 Cursor crate 结构与变体分派点。
2. 实现 cursorDiskKV probe + 有界只读读取（复用既有 SQLite 快照实现）。
3. 实现 header 顺序解析、坏行状态、工具输入编码与 native id 保真。
4. 合成 fixtures + PROVENANCE；顺序/坏行/编码/上限/源不可变测试。
5. 跑 workspace 受影响范围 + 全量 provider golden。
6. 更新 provider 矩阵文档（维持 experimental，记录证据边界）。
7. 提交（`feat(provider-cursor): add bounded cursorDiskKV variant`）并合并 main。

## Validation commands

```text
cargo fmt --all --check
cargo clippy -p agent-session-grep-provider-cursor --all-targets --offline -- -D warnings
cargo test -p agent-session-grep-provider-cursor --offline
cargo test -p agent-session-grep-cli --offline
cargo test --workspace --offline --no-fail-fast
git diff --check
```

## Rollback

新增变体独立；回退即 revert 该提交，不影响既有 ItemTable 变体。
