# 独立校验报告：合并提交 `161b83e` + 证据提交 `8755928`（trellis-check）

- 校验者：trellis-check 子代理（独立复核，不 commit / 不 push / 不改产品代码）
- 被校验对象：`161b83e`（origin/main `2edf2dc` 合入 `fa22246`）+ `8755928`（证据文档）
- PR：LainHappy/AgentSessions #22（base main，head `8755928`）
- 本地重跑目标目录：`.trellis/.runtime/target-merge`（沿用既有构建产物）
- 结论：**仓库/本地维度 PASS；CI 维度 FAIL（ci、release-verify 红）→ 当前不可收口合并。**

## 0. 工作树观察（重要）

- 检查开始（04:05）时工作树干净（`git status --porcelain` 为空）。
- 04:15:34 起 `crates/agent-session-grep-cli/src/repo_identity.rs` 出现未提交修改（+19/-1，
  为 `cwd_is_inside_toplevel` 增加 canonicalize 兜底），04:15:51 仍在被继续编辑。
  **这不是本次 check 所为**，是主会话的并行修复工作；其正确性需另行验证、另行跑 CI。
- 本报告全部本地重跑均在干净树（`8755928`）上完成，时间早于该修改。

## 1. 逐条 PASS/FAIL

### 1.1 冲突 A：`.trellis/spec/agentsessions-adapters-sqlite/backend/index.md` — PASS

- 场景齐全且顺序正确（行号）：`Batch-scoped commit state`(362) → `Historical empty-placeholder repair`(438)
  → `Journal retention compaction (schema v19)`(517) → `Semantic search evidence gate (B4)`(576)，
  均在 `## Quality Check`(593) 之前。
- `git diff 2edf2dc -- <file>`：**76 插入 / 0 删除**（相对 main 是纯追加，证明 main 的
  WAL/源字节表述、Tests Required 改动一字未回退）。
- `git diff fa22246 -- <file>`：仅把分支旧句替换为 main 新句（`-or write the provider source...` 等）。

### 1.2 冲突 B：`docs/product/PROVIDER-BETA-READINESS.md` — PASS

- 头部时间链：L10 `Last updated: 2026-10-06 (lifecycle wave (B5)...)` → L12 `Previous pass, 2026-09-29 (provider coverage wave: hermes ...)` → L19 `Previous pass, 2026-08-28 (local-gap closure pass: kimi-code ...)`；
  `2026-08-28` 全文件计数 = 1，带 `Previous pass, ` 前缀。
- 章节顺序：L64 `## Recorded follow-ups from the Hermes state.db variant (2026-09-29)` + 表 →
  L76 `## Lifecycle evidence (2026-10-06, B5)` → L104 `## Global external blockers`。
- `git diff 2edf2dc -- <file>`：31 插入 / 1 删除，2 hunks（仅表头两行改写）；hermes/cursor 表格行未被触碰。

### 1.3 冲突标记全仓库 — PASS

`git grep -n -e '^<<<<<<< ' -e '^>>>>>>> ' -e '^=======$'` → exit 1，0 命中（tracked 文件全量）。

### 1.4 main-only 文件回退检查 — PASS

- 复算 "仅 main 改过"：`git diff --name-only 2b8f895 2edf2dc` 减去 `git diff --name-only 2b8f895 fa22246`
  → **208 个**；与 `git diff origin/main..HEAD --name-only`（488 个）交集 = **0**。

### 1.5 契约保持 — PASS

- `SCHEMA_VERSION`：main=18、分支=19、**合并=19**（`lib.rs:8829`）；`RELATION_SCHEMA_VERSION=7`（`lib.rs:8710`）。
- `migrate_v18_to_v19`（`lib.rs:2600`，`PRAGMA user_version = 19`）与迁移阶梯 `if current < 19` 均在。
- 同一 `lib.rs` 中 main 的 batch-scoped 提交路径符号（`source_entity_membership_state_for_sources` 等 6 处）
  与分支的 journal compaction 符号（11 处）同时存在，双向都未被覆盖。

### 1.6 写域（人工解决仅限 2 文档）— PASS

用 `git merge-tree --write-tree fa22246 2edf2dc` 重放自动合并（exit 1 = 预期两处冲突），
其结果树与 `161b83e` 逐文件对比：**只有 2 个文档文件不同**，其余 485 个文件逐字节相同
→ 人工解决严格限于 PRD 锁定的两个文档，没有夹带其它改动（较 `git show --stat` 更强的证据）。

### 1.7 门禁真实性 — PASS（本地）

- `.trellis/.runtime/merge-gate-tests.log`：93 条 `test result:` 行求和 = **1912 passed / 0 failed / 22 ignored**，
  与证据声明一致。
- 独立重跑（同一 `target-merge`）：
  - `cargo test -p agent-session-grep-adapters-sqlite --locked` → exit 0
  - `cargo test -p agent-session-grep-cli --locked` → exit 0
  - 二者合计 915 passed / 0 failed；`journal_retention.rs` 13 项、`hotpath_repo_probe`（get/show=0、search=2 探测）均通过。
  - 日志：`research/check-2026-10-07-cargo-reruns.log`。

### 1.8 42 上下文语义 harness — PASS

重跑 `check3-semantics.ps1`：42 行上下文全部 EQUAL，VERDICT `ALL CONTEXTS EQUIVALENT`；
唯一报错是向已归档路径写入 json（与证据注记一致）。日志：`check-2026-10-07-semantics-rerun.log`。
（注：持久化日志文件 mtime 为 02:17，属合并前产物；我本次在合并工作树上重跑得到同一 VERDICT。）

### 1.9 性能声明反证 — PASS（"重叠仍生效"成立）

- 同 harness（`measure.ps1` 同款）就地重跑，同一二进制 sha256 `792776c1...`：
  - git_detect/search 中位 **38.01 ms**（探测 2,2,2）；seam/search 中位 **12.48 ms** → 单轮解析成本 **25.53 ms**。
  - 当刻 `git -C C:\AgentSessions rev-parse --show-toplevel` ×20：min 27.10 / median **30.88** / max 38.00 ms
    （×10 中位 29.06 ms）。
- 判定：单进程成本**没有**回落到 ~19ms（实测 ~29-31ms），单轮成本 ≈ 0.83-0.88 × 单进程，
  远低于串行预测（2 × 29-31 ≈ 58-62ms）→ 证据结论"重叠派发在合并结果上仍生效"成立，
  "更像串行"的否证不成立。
- 披露：本拟隔离重跑，但脚本路径替换未生效，实际在原 `.trellis/.runtime/merge-perf/` 就地重跑，
  覆盖了同名 check3-after-* 运行时文件（均在被 .gitignore 排除的 .runtime 下，非受控文件）。

### 1.10 CI / PR 状态（head `8755928`，查询时点 ~04:14，详见 check-2026-10-07-ci-status.txt）— FAIL

- `ci`：**FAIL**（test (macos-latest) / test (windows-latest) 失败；test (ubuntu-latest)、3 个 installer smoke、cargo-deny 通过）
- `release-verify`：**FAIL**（required CI quality gates 的 test (macos-latest) / test (windows-latest) 失败；ubuntu 通过）
- `security-audit`：SUCCESS
- `core-beta-evidence`：IN PROGRESS（Ubuntu 22.04 GNU、Windows MSVC、macOS ARM64 已通过；macOS Intel pending）
- 失败 1（macOS）：`crates/agent-session-grep-cli/tests/invariant_resume_args.rs:239` —
  `spawned cwd "/private/var/..." != expected "/var/..."`（macOS `/var`→`/private/var` 符号链接；`same_cwd` 只做字面比较）。
- 失败 2（Windows CI，ci 与 release-verify 两个 workflow 各一次）：
  `crates/agent-session-grep-cli/tests/hotpath_repo_probe.rs:134` — `left: 3, right: 2`
  （search 实际派发 3 个 git 内建进程）。对照：macOS 同一测试 **通过**；本地 Windows（git 2.55.0.windows.2）
  通过，手工 GIT_TRACE 复现 = 2 行（rev-parse + remote get-url）；CI Windows 为 git 2.55.0.windows.5。
  候选根因：`repo_identity.rs` 的 `cwd_is_inside_toplevel` 用字面 `Path::starts_with` 比较 cwd 与 git 返回的
  toplevel，在 runner 路径别名/大小写/短名下失配 → 触发文档化的"回退串行重探"（第 3 个 git 进程）。

## 2. 问题清单

- 实质（未修复，属主会话职责，需新提交 + 新 CI run 才能收口）：
  1) macOS CI：`invariant_resume_args::same_cwd` 字面比较失败（`/private/var`）。
  2) Windows CI：`hotpath_repo_probe` 断言 3 != 2。
- 建议修复方向：
  1) macOS：`same_cwd` 两侧先 canonicalize 再比较（`std::fs::canonicalize`），或对 macOS 接受 `/private` 前缀。
  2) Windows：先定位 runner 上三个门（env override / `<toplevel>/.git` 存在 / cwd⊆toplevel）哪个失效；
     若是路径别名失配，用 canonicalize 兜底（工作树中已有人在做的方向）；若确认走的是文档化串行回退，
     断言可放宽为 ≤3（旧实现的 4 仍会被抓住），但需在测试注释中写明。
- 非阻塞观察：证据文档把 42 上下文断言的持久化日志留在合并前产物上（mtime 02:17），本轮已由我重跑复核。

## 3. 收口结论

- **本地维度：可以**（合并解决、契约、门禁、语义、性能均复核通过）。
- **CI 维度：未绿**（ci、release-verify 红；core-beta-evidence 仍在跑）→ **当前不可合并进 main**。
- 在两条 CI 失败修掉并以新 head 重跑 4 条 workflow 全绿之前，PRD 验收条件（"4 条 workflow 全绿"）不满足。
