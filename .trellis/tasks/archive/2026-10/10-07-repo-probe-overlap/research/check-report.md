# B8 独立校验报告（trellis-check）

- 任务：`.trellis/tasks/10-07-repo-probe-overlap`（B8）
- HEAD：`283b9480e166e209d6f69747cb2e943686d3d765`（工作树含待提交改动）
- 被校验代码：`crates/agent-session-grep-cli/src/repo_identity.rs`（sha256 `dabcba4c5b93da0b63f3072ef47816de8476fedf8ace5ac15c5eca86cd263a1e`，`git diff --stat` = 143 insertions / 13 deletions）
- 被校验二进制：`.trellis/.runtime/target-b8/release/agent-session-grep.exe`
  sha256 `1ab64b094fe11d64ec9eca65ead6f7f7d8bec8830e667be11001d9231b74389f`（与 `after-measurements.json` 记录一致）
- 校验方：Codex trellis-check（独立复算 + 反例构造；不 commit）

## 1. 语义等价（最高优先级）——PASS（PRD 五上下文）/ 发现 3 个边缘反例（substantive，未改）

方法：进程级 A/B 对照（`check-semantics.ps1`，28 个上下文：旧串行组合 = `rev-parse -C cwd` 门禁 +
`remote get-url -C toplevel`；新路径 = 同一门禁 + `remote get-url -C cwd`），并用真实 resolver 复算
（临时诊断测试，运行后按 sha256 还原源文件，`check-rust-diag.log`）。

PRD 五上下文（与改动前逐项一致，全部 EQUAL）：
- 普通仓根/子目录/尾斜杠：`github.com/synthetic-owner/plain` 一致
- 嵌套仓（内层自成一仓）：内层 slug 一致；外层不受影响
- bare repo（带 origin，根 + `refs/heads` 子目录）：门禁失败 → None（URL 探测成功也被丢弃）
- `.git` / `.git/hooks` 内 cwd：None
- 非 git 目录、不存在的目录：None

额外覆盖且 EQUAL：linked worktree（根/子目录/宿主）、submodule（根/深层/宿主/子模块 gitdir）、
无 origin、大写 `ORIGIN`、本地路径 origin、`insteadOf` 重写、pushurl≠fetch url、
`extensions.worktreeConfig` worktree 级 origin、GIT_DIR 绝对路径（cwd 在外/在内）、
GIT_CEILING_DIRECTORIES、`core.worktree=仓库根` 的 `.git` cwd。

真实 resolver 复算的 3 个 DIVERGENT 反例（旧 → 新）：
1. **仅设 `GIT_WORK_TREE`（无 GIT_DIR），cwd 在仓内子目录**：旧 None → 新 `Some(github.com/synthetic-owner/wr)`。
   门禁 `rev-parse` 返回 `GIT_WORK_TREE` 指向的外部目录，旧代码从该目录再探 URL 失败（None）；
   新代码从 cwd 探测命中宿主仓。
2. **相对 `GIT_DIR=.git` + `GIT_WORK_TREE=<仓根>`，cwd 在嵌套仓内**：旧 `Some(envrepo)` → 新 `Some(nested)`。
   相对 GIT_DIR 在旧代码里按 toplevel 解析、在新代码里按 cwd 解析，命中不同仓库（slug/digest/排序输入变化）。
3. **gitdir 的 `core.worktree` 指向 gitdir 树之外（core.bare=false），cwd = gitdir 或 gitdir/sub**（无需任何环境变量）：
   旧 None → 新 `Some(github.com/synthetic-owner/detached)`。门禁因 core.worktree 存在而成功（返回外部 worktree），
   旧代码从该 worktree 再探失败（该目录不可发现为仓库），新代码从 cwd 探测命中该 gitdir。
   （cwd = 外部 worktree 本身时两侧仍同为 None。）

可达性：三者都要求非常规配置；第 1/2 需要调用方环境带 `GIT_WORK_TREE`（第 2 还要求相对 GIT_DIR），
第 3 需要某个解析 cwd 落在「gitdir 树内 + core.worktree 外指」的布局。两个调用面（`current_repo_slug()`
用进程 cwd；sync 的 `session_repo_slug()` 用会话记录里的任意 cwd）都可能遇到，但现实概率极低；
五上下文与 worktree/submodule 等现实配置全部一致。结论：属于需要 owner 决策的边界语义，
本次按“substantive 只报告”处理，未改代码。

## 2. 缓存语义——PASS

- by_directory 命中（含缓存 None）：`cached?` 直接返回，0 探测；by_toplevel 命中：0 探测。
  未命中路径把 toplevel/结果成对写入两级缓存，故 `by_toplevel` 必有对应条目（代码内不变量）。
- GIT_TRACE 实测（`check-probe-count-cache.log`，真实 resolver）：非 git 目录首次 1 条 rev-parse 痕迹、
  第二次 0；同仓 `sub` 与 `repo` 各 2 探测（by_toplevel 命中时第二探已并发派出、被丢弃）；重复 `sub` 0。
  合计 7 条（= 新实现口径；旧实现为 6）。
- 已知差异（设计使然）：失败上下文（bare/.git/非仓库）未命中时由 1 探测变 2；
  by_toplevel 命中但 by_directory 未命中时由 1 探测变 2——皆为 PRD 要求 1「未命中即重叠派发」的直接结果，
  不影响 `hotpath_repo_probe.rs` 的 2/0/0 断言。

## 3. 独立测量——PASS

`pwsh -NoProfile -File research/check-measure.ps1 -Binary <交付 exe> -Phase after -Runs 20 -ProbeRuns 3`
（仅改输出前缀 check-，其余与 B7 口径逐字一致；写 `check-after-measurements.json`）：

| 命令 | B7 基线中位 | 本校验中位 | 探测 |
|---|---:|---:|---|
| get | 7.96 | 7.94 | 0,0,0 |
| show | 7.91 | 7.81 | 0,0,0 |
| **search** | **48.02** | **32.01** | **2,2,2** |
| seam/search | 9.08 | 9.33 | 0,0,0 |

- 单轮解析成本 38.94 ms → 22.68 ms（−16.3 ms），远高于 PRD 的 5 ms 回退阈值。
- `check-output-diff.py`：变体间仅 `meta.duration_ms` 变化，其余 0 差异（PASS）；
  与交付 `after-output-*.json` 交叉比对：非易变字段 0 差异。

## 4. 门禁真实性——PASS（一处证据缺口，trivial）

| 门禁 | 结果 | 证据 |
|---|---|---|
| `cargo fmt --all --check` | exit 0 | `check-fmt.log`（我重跑） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0，0 warning | `check-clippy-workspace.log`（我重跑） |
| `cargo clippy -p agent-session-grep-cli --all-targets ...` | exit 0 | `check-clippy-cli.log`（我重跑） |
| `cargo test --workspace --locked` | exit 0，1812 passed / 0 failed | `check-test-workspace.log`（我重跑；与 `test-b8.log` 数字一致） |
| `cargo test -p agent-session-grep-cli --locked` | exit 0，590 passed / 0 failed | `check-test-cli.log`（含新矩阵测试） |

- 缺口（trivial，已就地修复）：`hotpath-overlap-comparison.md` 第 6 节引用 `fmt-check.log`，该文件原本不存在；
  本次校验重跑 `cargo fmt --all --check`（exit 0）生成 `fmt-check.log` 并在文件内标注来源，另保留 `check-fmt.log`。
  `clippy-b8.log` 仅 73 B（缓存重跑输出）、缺命令行与明细；未覆盖交付文件，改以 `check-clippy-cli.log` / `check-clippy-workspace.log` 补证。
- 二进制溯源：以交付源码重建（`check-release-rebuild.log`）得 `1be72da7…`，与交付 `1ab64b09…`
  逐字节仅差 24 字节（PE `TimeDateStamp` 等非确定字段，`Rich` 头后偏移 0x108 起），
  其余字节完全一致 → 交付二进制确由交付源码构建；行为差异检查亦未发现。

## 5. 写域——PASS

- `git status --short -- crates/` 仅 ` M crates/agent-session-grep-cli/src/repo_identity.rs`。
- `.trellis/tasks/10-05-*`、`09-*`、`10-06-*` 删除（已归档）、`archive/**`、`.github/`、`scripts/`、其他 crate 均未被 B8 触碰：
  其 mtime（≤2026-10-07 01:20 或更早）早于本任务源码改动（01:30:53），内容为前序波次/主会话台账。
- `hotpath_repo_probe.rs` 未修改（`git diff --stat` 为空），断言仍为 get/show/status=0、search=2。

## 6. 测试证伪能力——PASS

`resolve_context_matrix_matches_pre_change_semantics` 同时断言「旧串行组合（`derive_repo_slug`，gate+toplevel 探测）」
与「生产重叠路径」对五个上下文逐项等于字面期望值；bare/.git 场景 URL 探测本身会成功，
若门禁被绕过（URL 结果被无条件采纳），该用例会立即失败。新增/修改测试确实能抓住该类回归。

## 7. 结论

- 验收标准（PRD 5 上下文、探测计数、测量增益、变体输出一致、三项门禁）全部满足：**可以收口提交**。
- 但 PRD Goal 的「None/Some 契约逐字不变」在上述 3 个边缘上下文（R1 GIT_WORK_TREE、R2 相对 GIT_DIR+GIT_WORK_TREE、
  R3 core.worktree 外指）存在可复现漂移，属 substantive，需主会话/owner 决策：
  1) 推荐：接受并在 B8 证据/设计说明中显式记录（现实可达性极低，五上下文与常见布局全部一致）；
  2) 若要严格逐字不变：R1/R2 可按环境变量存在性回退到旧的串行探测；R3 无环境信号，
     只能整体放弃重叠（或额外加一次探测做一致性校验）——即放弃本次收益，需 owner 取舍。

