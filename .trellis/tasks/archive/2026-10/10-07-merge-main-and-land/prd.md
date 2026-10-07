# 合并 main 并落地主干（冲突解决 + PR + CI + merge）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-07 批准：原分支合并、允许 CI 绿后直接合入 main。
> 输入：合并方案（Plan Mode 已批准）。预演：`git merge-tree --write-tree HEAD origin/main` → 2 个文档冲突，其余自动合并。

## Goal

把 `origin/main`（57 提交）合并进 `fix/session-relocation-identity`（18 提交，HEAD `8c232b8`），保留全部既有哈希；解决冲突后开新 PR，CI 全绿后以 merge commit 合入 `main`。

## Requirements

1. `git merge --no-ff origin/main`；冲突**只允许**按下列锁定规则解决（不得新增其它改动）：
   - A `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md`：双方追加章节全保留，顺序 = main 的 `Batch-scoped commit state` → main 的 `Historical empty-placeholder repair` → 我方 `Journal retention compaction (schema v19)` → 我方 `Semantic search evidence gate (B4)`；删除冲突标记；`---` 与 `## Quality Check` 原样。
   - B `docs/product/PROVIDER-BETA-READINESS.md`：头部时间链条三段且最新在前（2026-10-06 我方 Last updated → `Previous pass, 2026-09-29` hermes → `Previous pass, 2026-08-28` kimi-code，2026-08-28 段只出现一次）；章节顺序 = main 的 `Recorded follow-ups from the Hermes state.db variant (2026-09-29)` + 表 → 我方 `Lifecycle evidence (2026-10-06, B5)`，均在 `## Global external blockers` 之前。
2. 硬校验：`git diff origin/main..HEAD --stat` 只含这 18 个提交的文件；两文件冲突标记 grep 0 命中。
3. 契约不得被 main 覆盖：`SCHEMA_VERSION` 保持 19、`RELATION_SCHEMA_VERSION` 保持 7。
4. 推送原分支（追加 merge commit，不 force）；开新 PR 到 `main`。
5. CI（`ci` / `core-beta-evidence` / `security-audit` / `release-verify`）在 PR head 全绿后，以 **merge commit** 合入 main；不做 squash/rebase。
6. 若合并后的自动合并 Rust 无法编译，修复限于冲突/重复定义/类型错误，不得改变 main 的契约语义。

## Non-goals

- 不拆分 PR、不重写哈希、不打 tag/发布、不删除分支。
- 不在本任务升级 `IB-CI-PROVIDER-EVIDENCE-001`（可用本 PR 的 run id 后续单独补）。
- 不改 main 已建立的契约（batch-scoped commit state、empty-placeholder 修复、hermes/cursor 变体）。

## Acceptance Criteria

- [ ] 合并后 `grep -n "^<<<<<<<|^=======$|^>>>>>>>"` 两文件 0 命中，且 `git diff origin/main..HEAD` 只含本分支 18 提交的文件。
- [ ] `SCHEMA_VERSION=19` / `RELATION_SCHEMA_VERSION=7` 在合并结果中保持。
- [ ] 本地门禁全绿：fmt / clippy `-D warnings` / `cargo test --workspace`（≥1820 passed）/ 三套 Python 单测 / web_ui。
- [ ] B8 语义 harness（42 上下文）与 `hotpath_repo_probe`（search=2 探测）在合并结果上仍通过。
- [ ] PR 创建成功且 4 条 workflow 全绿；随后 main 合入成功、`origin/main` 含本 PR、工作树干净。
