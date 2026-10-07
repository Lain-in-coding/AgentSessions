# 实施计划：merge-main-and-land

## Steps

1. 记录前置：`git rev-parse HEAD origin/main`、确认工作树干净。
2. `git merge --no-ff origin/main`（预期仅 A/B 两文件冲突）。
3. 按 PRD 规则解决 A/B；`git add` 两文件；完成 merge commit（消息写明两处解决规则）。
4. 硬校验：冲突标记 grep、`git diff origin/main..HEAD --stat`、schema 常量比对。
5. 本地门禁（isolated target dir `.trellis/.runtime/target-merge`）：fmt / clippy / workspace tests；Python 三套；web_ui。
6. B8 回归：42 上下文语义 harness + `hotpath_repo_probe`。
7. 推送分支；`gh pr create --base main`；记录 PR 号。
8. 盯 4 条 workflow（`gh pr checks` / `gh run list`），全绿后 `gh pr merge --merge`。
9. 合后核验：`git fetch origin main`、确认 main 含本 PR、工作树干净；归档任务。

## Validation commands

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-merge -- -D warnings
    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-merge
    python -m unittest discover -s scripts -p "test_*.py"
    python -m unittest discover -s scripts/release -p "test_*.py"
    python -m unittest discover -s scripts/evidence -p "test_*.py"
    node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs
