# CI 第一轮失败与修复（2026-10-07）

PR #22 head `8755928` 首轮 CI：security-audit 绿；ci 与 release-verify 红；core-beta-evidence 进行中。

## F1（阻塞）Windows：search 探测数 3 而不是 2

- 现象：hotpath_repo_probe.rs:134 `left: 3, right: 2`（ci 与 release-verify 各一次，均为 windows-latest；ubuntu、installer smoke、cargo-deny 通过）。
- 根因（本地已复现）：`git rev-parse --show-toplevel` 会解析路径别名——Windows junction 下 cwd 报链接路径、toplevel 报真实路径（实测 `...\alias-probe\link\repo` 变 `.../alias-probe/real/repo`）。B8 收窄实现里的守卫 `cwd_is_inside_toplevel` 用字面 `Path::starts_with` 比较，在 runner 的别名/短名临时目录上判否，于是走"丢弃重叠结果 + 串行重探"分支，多出第 3 个子进程。
- 修复：守卫改为字面比较不成立时按 `canonicalize` 后的真实路径再比一次，两侧都解析失败才判否（保守回退）。语义不变：core.worktree 外指、bare repo、`.git` 内 cwd 等仍走串行（H1/H2/R1/R2/R3 结论不受影响）。
- 回归测试：repo_identity.rs 新增 `cwd_inside_toplevel_resolves_symlink_alias`（unix）与 `cwd_inside_toplevel_accepts_case_alias`（windows）；并在 junction 仓库上端到端验证，GIT_TRACE 计数回到 2。

## F2（阻塞）macOS：D3 测试的 cwd 字面比较

- 现象：invariant_resume_args.rs:239 `spawned cwd "/private/var/..." != expected "/var/..."`。
- 根因：该测试（D3 提交 f899794 新增，main 中不存在）的 `same_cwd` 只做字面比较（Windows 例外仅大小写）。macOS 的 TMPDIR 是 /var 符号链接，子进程 getcwd 报真实路径。
- 修复：新增 `same_path` 帮助函数（字面 + Windows 大小写），`same_cwd` 在其失败时按 canonicalize 后的真实路径再比一次。
- 同类排查：D3 另 3 个测试文件（adapters-sqlite 两个、application 一个）无路径别名敏感比较。

## 复跑证据

- `cargo test --workspace --locked --no-fail-fast`（target-merge）：1913 passed / 0 failed（日志 merge-gate-tests-round2.log）。
- `cargo fmt --all --check` exit 0。
