# B8 第二轮独立复核报告（trellis-check round 2，收窄实现）

- 任务：`.trellis/tasks/10-07-repo-probe-overlap`；HEAD `283b9480e166e209d6f69747cb2e943686d3d765`（未提交工作树）
- 被校验源码：`crates/agent-session-grep-cli/src/repo_identity.rs` sha256 `14b288381518805f8669d6c6b752243a6b54e28294a24fec7dd70f63a743c551`
- 被校验二进制：`.trellis/.runtime/target-b8/release/agent-session-grep.exe` sha256 `66b6b090511f6904281afddfdfc7327689cd4ef9e5f32e1356590e197940a0db`（02:01:37 重建；`cargo build --release -p agent-session-grep-cli --locked` 复跑 0.17s 无重编译 → 与当前源码同步）
- 收窄逻辑核对（代码阅读）：`discovery_env_overrides_present()`（GIT_DIR / GIT_WORK_TREE / GIT_COMMON_DIR / GIT_CEILING_DIRECTORIES / GIT_DISCOVERY_ACROSS_FILESYSTEM，`var_os().is_some()` → 空值也算存在）→ 命中即整段串行；未命中走重叠，`toplevel_is_rediscoverable()`（`<toplevel>/.git` 存在，文件/目录皆可）不成立时丢弃重叠 URL 并用 `git_origin_url(&toplevel)` 串行补齐。与主会话描述一致。
- 第一轮产物 `check-*` 与交付 `after-*`/`output-diff.txt`/`test-b8.log` 未被覆盖（mtime ≤ 01:36）。

## ① 语义 harness（28+ 上下文，含 R1/R2/R3 与新增反例）

方法：`research/check2-semantics.ps1`（模型级 A/B：旧串行 vs 收窄模型，40 个上下文）+ `research/check2-semantics-outside.ps1`（clean 位置复算 detached 家族）。结果写入 `check2-semantics.log|json`、`check2-semantics-outside.log|json`。

- **R1（仅 GIT_WORK_TREE）**：`EQUAL(None)`，路径 `serial(env)` —— 第一轮 DIVERGENT 已消除。
- **R2（相对 GIT_DIR + GIT_WORK_TREE，嵌套仓）**：`EQUAL(Some(envrepo))`，路径 `serial(env)` —— 已消除。
- **R3（core.worktree 外指，无环境变量；clean 位置）**：R3a/R3b `EQUAL(None) lit=ok`，路径 `overlap+serial-reprobe`；R3c（cwd=外部 worktree）`EQUAL(None)` —— 已消除。
- 其余 35 个上下文（五上下文、worktree、submodule、无 origin、大写 ORIGIN、本地路径、insteadOf、pushurl、worktree 级 config、GIT_DIR 绝对/天花板等）全部 EQUAL，字面期望全部 `lit=ok`。
- **新增反例（残余）**：H1 `DIFF(value)`：旧 `h1-worktree-repo` → 新 `h1-gitdir`；H2 `DIFF(value)`：clean 位置旧 `None` → 新 `h2-gitdir`（本轮 in-repo scratch 下旧值显示为外层仓 URL，方向相反，见 `check2-semantics-outside.log` 的 `DIFF(old=None)`）。H3（`<toplevel>/.git` 指针文件指回同一 gitdir）EQUAL。

## ② 真实二进制端到端（探测次数 + slug 判定）

方法：`research/check2-e2e.ps1` + `check2-gitshim.rs`（PATH 前置的 `git.exe` shim：逐次记录 spawn argv 并透传真 git 的 stdio/退出码）+ cursor 的 `query_digest`（`search_query_digest` 含 `current_repo`，用 `ASG_CURRENT_REPO` 注入值建立 digest→slug 参照表；`--max-items 1` 强制出 cursor）。原始数据 `check2-e2e.log|json`。

| 上下文 | 期望（改动前语义） | 实测（真实二进制） | 判定 | spawn 数 |
|---|---|---|---|---|
| plain root / subdir | Some(plain) | Some(plain) | MATCH | 2 |
| bare root | None | None | MATCH | 2 |
| `.git` cwd | None | None | MATCH | 2 |
| 非 git 目录 | None | None | MATCH | 2 |
| 无 origin | None | None | MATCH | 2 |
| linked worktree | Some(wt-host) | Some(wt-host) | MATCH | 2 |
| submodule | Some(submodule) | Some(submodule) | MATCH | 2 |
| R1 GIT_WORK_TREE | None | None | MATCH | 2（串行：gate + 从 W 探测） |
| R2 相对 GIT_DIR+WT（plain/nested） | None / Some(envrepo) | None / Some(envrepo) | MATCH | 1 / 2 |
| R3 core.worktree 外指 | None | None | MATCH | **3**（重叠 2 + 串行补齐 1） |
| H1 toplevel/.git=别的仓 | Some(h1-worktree-repo) | **Some(h1-gitdir)** | DRIFT | 2 |
| H2 toplevel/.git 无效 | None | **Some(h2-gitdir)** | DRIFT | 2 |

- 主路径（普通仓 root/subdir）仍为 **2 次 spawn**（两条都 `-C cwd`，重叠派发未被守卫误伤）；bare/`.git`/非 git 仍是 2 次（门禁丢弃 URL，与第一轮一致；相对改动前的 1 次属 PRD 要求 1 的既定代价）。
- `crates/agent-session-grep-cli/tests/hotpath_repo_probe.rs` 未修改（`git diff` 空），其断言（get/show/status=0、search=2）在 `check2-test-cli.log` / `check2-test-workspace.log` 中通过。

## ③ 收益（check2-measure.ps1，20 次/变体，3 次探测计数）

| 命令 | B7 基线 | 第一轮 | 本轮（收窄后） | 探测 |
|---|---:|---:|---:|---|
| get | 7.96 | 7.94 | 8.47 | 0,0,0 |
| show | 7.91 | 7.81 | 8.13 | 0,0,0 |
| **search** | **48.02** | 32.01 | **31.53** | **2,2,2** |
| seam/search | 9.08 | 9.33 | 8.65 | 0,0,0 |

- 单轮解析成本 38.94 ms（B7）→ 22.89 ms；较基线 **−16.49 ms**，较第一轮 −0.48 ms（无回退，守卫未误伤主路径）。
- `check2-output-diff.py`：变体间仅 `meta.duration_ms` 变化（RESULT: PASS）；与交付 `after-output-*.json` 交叉比对非易变字段 0 差异。

## ④ 残余 DIVERGENT / 理论漏洞

1. **H1（实测 DRIFT）**：`core.bare=false` 的 gitdir 经本地 `core.worktree` 指向**另一个仓库的工作树根**（该根自带 `.git`）。门禁返回该 W，守卫见 `W/.git` 存在 → 采纳 cwd 探测（gitdir 自己的 origin），而改动前会从 W 探测到另一个仓的 origin。→ slug 值变化（排序 boost + cursor digest 输入变化）。
2. **H2（实测 DRIFT，None→Some）**：同布局但 `W/.git` 是**无效**（空目录等）条目：改动前从 W 探测失败 → None；新路径采纳 cwd 探测 → Some(gitdir)。干净位置实测 `DIFF(old=None)`。
3. **H1 的次生效应（代码推导，未单独执行）**：`by_toplevel` 以 toplevel 字符串为键，H1 中 W 同时是"gd 的 worktree"和"另一个仓的根"，同一进程（如 sync 的 `session_repo_slug` 多 cwd 解析）先解析 gd、再解析 W 内 cwd 时，后者会命中被 H1 污染的 `by_toplevel[W]`，把 gitdir 的 slug 带给 W 仓库；顺序相反时无此效应。
4. 已排查并**排除**的理论面：`GIT_CONFIG_GLOBAL` / `GIT_CONFIG_COUNT` 注入的 `core.worktree` 在 git 2.55 下不参与 worktree 落点（实测 gate 仍返回仓库根），故不构成额外漏洞；`GIT_OBJECT_DIRECTORY`/`GIT_NAMESPACE`/`GIT_INDEX_FILE`/`GIT_ALTERNATE_OBJECT_DIRECTORIES` 不影响发现落点；`<toplevel>/.git` 为 worktree/submodule/separate-git-dir 指针文件（H3）实测等价。
   H1/H2 需要"detached gitdir 的 core.worktree 指向自带（有效或无效）`.git` 的目录"这一极不常见布局；五上下文与 worktree/submodule 等现实配置全部一致。按纪律未改产品代码。

## ⑤ 门禁

| 门禁 | 结果 | 证据 |
|---|---|---|
| `cargo fmt --all --check` | exit 0 | `check2-fmt.log` |
| `cargo clippy -p agent-session-grep-cli --all-targets --locked -D warnings` | exit 0 | `check2-clippy-cli.log` |
| `cargo clippy --workspace --all-targets --locked -D warnings` | exit 0 | `check2-clippy-workspace.log` |
| `cargo test -p agent-session-grep-cli --locked` | 590 passed / 0 failed（含五上下文矩阵测试） | `check2-test-cli.log` |
| `cargo test --workspace --locked` | 1812 passed / 0 failed | `check2-test-workspace.log` |
| `cargo build --release -p agent-session-grep-cli --locked` | 无重编译（二进制与源码同步） | `check2-release-freshness.log` |

## 结论

- 收窄实现把第一轮的 R1/R2/R3 三个反例全部收敛为逐字一致（模型级 + 真实二进制 + 真实 resolver 三路证据），主路径收益保持（search 31.53 ms，探测 2/2/2），门禁全绿：**可以收口提交**。
- 唯一遗留是 H1/H2 两个"detached gitdir 的 core.worktree 指向另一仓库根/含无效 `.git` 的目录"反例（外加 H1 的进程内缓存次生效应）。它们比第一轮三个反例更罕见，但仍是可复现的契约漂移；若要严格逐字不变，唯一精确修法是放弃该场景的重叠（或对 W 再做一次"同一仓库"校验，等于多一次探测）。建议主会话/owner 决策：按 (a) 接受并在 B8 证据/设计说明中记录；否则需回到串行探测。
