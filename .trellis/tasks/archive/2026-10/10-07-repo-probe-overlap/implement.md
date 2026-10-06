# 实施计划：repo-probe-overlap

## Steps

1. 读 `crates/agent-session-grep-cli/src/repo_identity.rs`（`git_output` / `git_toplevel` / `git_origin_url` / `GitRepoSlugResolver::resolve`）与 `crates/agent-session-grep-cli/tests/hotpath_repo_probe.rs`。
2. 并发派发：spawn 两个子进程（stdout piped），各自取首行（语义与 `git_output` 一致：spawn 失败/非零退出/非 UTF-8/空 → None）；先用 rev-parse 结果做门禁，门禁通过时用已就绪的 URL 结果归一化 slug。
3. 保持两级缓存与调用点不变；`ASG_CURRENT_REPO` 注入旁路与 None 降级语义不变。
4. 测试：`repo_identity.rs` 单测补 bare repo / `.git` 内 cwd 的 None 断言；`hotpath_repo_probe.rs` 断言不放松。
5. 测量：`.trellis/.runtime/target-b8` release 构建 → 复制 B7 `research/measure.ps1` 改 `$expDir`/`$outDir` → 20 次中位 + 探测计数 + 变体输出 diff。
6. 门禁 + 证据归档到本任务 `research/`。

## Validation commands

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b8 -- -D warnings
    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b8
