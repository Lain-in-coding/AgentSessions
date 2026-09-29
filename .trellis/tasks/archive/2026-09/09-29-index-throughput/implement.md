# 实施与验证：索引吞吐

## Ordered steps

1. 合并 PR #12 后的 main 上复测基线（复制的 harness，参数化 commit pin），记录语料/二进制哈希。
2. 加阶段 trace（临时门控）并在 10 万/100 万跑一次归因，产出 profile 报告与候选排序。
3. 逐项实施候选：每项独立提交 + 100k A/B；达标项再进 1M 复测；不达标即 revert。
4. 空转双读/双哈希消除单独提交与单独前后证据。
5. 全量 A/B（1M × ≥3、10k/100k × 3）与 workspace 正确性门。
6. 更新 `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md` 记录测量/预算边界（如有新约束）。
7. 提交并按子任务合并 main；证据 JSON + 校验器随任务提交。

## Validation commands

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test -p agent-session-grep-adapters-sqlite --offline
cargo test --workspace --offline --no-fail-fast
(测量) python -B research/harness/baseline.py run --workspace <repo> --binary <release>   --scratch-dir <fresh> --output-dir <fresh> --environment research/harness/environment.json   --profile full --expected-commit <main-sha> --scale-timeout-seconds 1800
python -B research/harness/<validator> <report>
git diff --check
```

## Rollback

按提交粒度 revert；不涉及 schema/迁移；测试与证据保留在任务目录。
