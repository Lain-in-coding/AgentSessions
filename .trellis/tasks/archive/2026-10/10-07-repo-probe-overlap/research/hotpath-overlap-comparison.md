# B8 热路径对比：repo 探测重叠派发（契约不变）

口径与 B7 逐字一致：`research/measure.ps1`（B7 harness 的最小改动副本，仅 `$expDir`/
`$outDir` 指向本任务）、release 二进制、20 次/变体、3 次 GIT_TRACE 探测计数、
`--robot --request-id hotpath-diff`、`ASG_CLOCK_MS=1787616000000`、cwd=`C:\AgentSessions`
（带 origin 的真实 git 仓库）、单条确定性合成消息；`seam` = `ASG_CURRENT_REPO=''`
既有测试注入（零探测基线）。

> **最终实现 = 守卫版（收窄后）**：`GIT_DIR` / `GIT_WORK_TREE` / `GIT_COMMON_DIR` /
> `GIT_CEILING_DIRECTORIES` / `GIT_DISCOVERY_ACROSS_FILESYSTEM` 任一存在 → 整段回退改动前的
> 串行两级探测；未命中且 `toplevel_is_rediscoverable()`（`<toplevel>/.git` 存在）与
> `cwd_is_inside_toplevel()`（`cwd` 组件级前缀于 `toplevel`）同时成立时才采纳重叠结果，
> 否则丢弃重叠 URL、从 toplevel 串行重探。**本文件表 1–4 以守卫版的干净运行为结论口径**；
> 收窄前数值仅作历史行保留。

- 二进制（最终，守卫版）：`.trellis/.runtime/target-b8/release/agent-session-grep.exe`
  sha256 `68eb7f6d47854eb5e10ee5f2fb8c188bc4d91460f36a001ad8968b7024da6c3b`（工作树含本任务
  全部守卫改动，commit `283b948`；`cargo build --release --locked` 复跑无重编译）
- 二进制（历史，收窄前重叠版）：sha256
  `1ab64b094fe11d64ec9eca65ead6f7f7d8bec8830e667be11001d9231b74389f`
- 夹具：`.trellis/.runtime/hotpath-experiment-b8/fixture.db`，wire id 与 B1/B7 逐字节一致
  `msg_v1_cb6837d0712fbb4e022f869652978b74`
- 原始数据（结论口径）：`check3-after-measurements.json`（守卫版 68eb7f6d 的干净 20 次运行，
  无并行构建）
  - 历史来源：`after-measurements-unguarded.json`（收窄前 1ab64b09 的原始 20 次）、
    `check-after-measurements.json`（第一轮独立复核）、`check2-after-measurements.json`
    （第二轮独立复核）；`after-measurements.json` 是 2026-10-07 02:15 主会话用守卫版重跑的
    一次运行，受并行构建干扰（get 中位 11.35 ms、search 中位 41.32 ms），**不作为口径**，
    仅保留为存在性证据
- 变体输出：`after-output-*.json`（守卫版重跑）+ `check3-after-output-*.json`（本轮独立复核）；
  diff：`output-diff.txt`（收窄前 canonical 输出）、`check3-output-diff.txt`（守卫版）

## 1. git_detect（真实 repo 发现）中位数与探测次数

| 命令 | B7 中位 ms（探测） | 收窄前 1ab64b09（探测） | **最终 68eb7f6d（探测）** | 最终 Δ vs B7 |
|---|---:|---:|---:|---:|
| get | 7.96（0） | 8.60（0） | **8.19（0）** | +0.23 |
| show | 7.91（0） | 8.76（0） | **8.08（0）** | +0.17 |
| **search** | **48.02（2）** | **34.66（2）** | **31.97（2）** | **−16.05** |

- 探测计数 3 次逐次一致：get/show = 0,0,0；search = 2,2,2（全部 exit 0）。
  探测次数按设计**不变**（by_directory 未命中仍各派发 1 个 `rev-parse` + 1 个
  `remote get-url`，只是重叠执行）；普通仓 cwd 在 toplevel 之下且 `<toplevel>/.git`
  存在，两个守卫都放行，故主路径仍走重叠。
- 收窄前数值来自 `after-measurements-unguarded.json`，最终数值来自
  `check3-after-measurements.json`；02:15 的 `after-measurements.json` 因并行构建整体偏高，
  未引用。

## 2. seam（零探测参照，噪声地板）与单轮解析成本

| 命令 | B7 中位 ms | 收窄前 1ab64b09 | 最终 68eb7f6d | 最终 git_detect − seam |
|---|---:|---:|---:|---:|
| get | 7.91 | 8.73 | 8.14 | +0.05 |
| show | 8.16 | 8.62 | 8.09 | −0.01 |
| search | 9.08 | 9.88 | 8.74 | **+23.23** |

- 单轮解析成本：B7 **38.94 ms**（2 个子进程串行 ≈ 19.5 ms/进程）→ 收窄前 **24.77 ms**
  → 最终 **23.23 ms**（2 个子进程重叠 ≈ 11.6 ms/进程等效墙钟，含 Windows 进程启动争用）。
- 收益未达满额 −19.5 ms 的原因：两进程抢占同一磁盘/CPU 管道，重叠效率约 60%；仍显著超过
  PRD 的 5 ms 回退阈值（守卫未误伤主路径；守卫只影响 `core.worktree` 外指/环境变量等非常规上下文）。

## 3. min/max（守卫版 20 次原始样本，`check3-after-measurements.json`）

| 变体/命令 | 中位 | min | max |
|---|---:|---:|---:|
| git_detect/get | 8.188 | 7.292 | 12.584 |
| git_detect/show | 8.078 | 7.144 | 10.260 |
| git_detect/search | 31.966 | 30.164 | 39.603 |
| seam/get | 8.136 | 7.386 | 16.328 |
| seam/show | 8.085 | 7.298 | 9.659 |
| seam/search | 8.741 | 8.179 | 9.381 |

（收窄前 20 次样本的 min/max 见 `after-measurements-unguarded.json`。）

## 4. 变体输出逐字段一致（仅 meta.duration_ms 允许变化）

- 守卫版复测：`check3-output-diff.py` 对 `check3-after-output-*.json` 做逐字段 walk
  （叶字段数：get 17 / show 18 / search 25）——get/show 0 差异；search 仅
  `$.meta.duration_ms`；RESULT: PASS（`check3-output-diff.txt`）；与 `after-output-*.json`
  （守卫版重跑）交叉比对非易变字段 0 差异。
- 历史：`output-diff.txt`（收窄前 canonical 输出：get 0 / show 0 / search 仅 `duration_ms`）。
- 注：B7 文档里「get/show 25 个叶子字段」是当时摘要的近似口径；本脚本按实际输出结构计数。
  另 `git -C <cwd> remote get-url origin` 与 B7 的 `git -C <toplevel> ...` 在本机
  git 2.55.0.windows.2 上对普通仓子目录/嵌套仓输出一致（单测矩阵同时锚定字面值）。

## 5. 语义证伪性测试与守卫矩阵

`crates/agent-session-grep-cli/src/repo_identity.rs` 的
`resolve_context_matrix_matches_pre_change_semantics`：五种上下文（普通仓根/子目录、嵌套仓、
bare repo、`.git` 内 cwd、非 git 目录）同时对照旧串行组合（`derive_repo_slug`：
`git_toplevel` + `git_origin_url`）与生产解析器路径，并断言字面期望值；bare 与 `.git` 内 cwd
的 URL 探测**本身会成功**，测试确保门禁仍丢弃该结果（改动前语义）。

独立复核矩阵（全部 EQUAL/MATCH，证据文件见括号）：
- 第三轮 `check3-semantics.*`：42 个上下文（含 R1 仅 `GIT_WORK_TREE`、R2 相对 `GIT_DIR` +
  `GIT_WORK_TREE`、R3 `core.worktree` 外指、H1 `<toplevel>/.git` 属另一仓、H2 `<toplevel>/.git`
  无效）全部 `EQUAL`、字面期望 0 失败——`cwd_is_inside_toplevel` 守卫把 H1/H2 收敛回串行路径。
- 第三轮 `check3-e2e.*`（真实二进制 + git shim 逐次 spawn 计数 + cursor `query_digest` 反推
  slug）：14/14 MATCH；典型探测次数：普通仓 root/subdir = 2、bare/`.git`/非 git/无 origin = 2、
  linked worktree/submodule = 2、R1 = 2、R2(plain/nested) = 1/2、R3/H1/H2 = 3
  （重叠 2 + 串行重探 1）。
- 历史轮次：`check-semantics.*`（第一轮 28 上下文）、`check2-semantics.*` /
  `check2-semantics-outside.*`（第二轮 42 + 6 上下文）、`check2-e2e.*`。

`crates/agent-session-grep-cli/tests/hotpath_repo_probe.rs` 断言未放宽：get/show/status
= 0 探测、search = 2 探测（一次解析）；该文件未被修改（`git diff` 为空）。

## 6. 门禁（isolated target dir `.trellis/.runtime/target-b8`）

| 门禁 | 结果 | 日志 |
|---|---|---|
| `cargo fmt --all --check` | exit 0 | `fmt-check.log`、`check3-fmt.log` |
| `cargo clippy -p agent-session-grep-cli --all-targets --locked -- -D warnings` | exit 0 | `check3-clippy-cli.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0（0 warning） | `check3-clippy-workspace.log` |
| `cargo test -p agent-session-grep-cli --locked` | exit 0（590 passed / 0 failed） | `check3-test-cli.log` |
| `cargo test --workspace --locked` | exit 0（1812 passed / 0 failed） | `test-b8.log`、`check3-test-workspace.log` |
| `cargo build --release -p agent-session-grep-cli --locked` | 无重编译（二进制与源码同步） | `check3-release-freshness.log` |

## 7. 命令

```powershell
cargo build --release -p agent-session-grep-cli --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b8
pwsh -NoProfile -File .trellis/tasks/10-07-repo-probe-overlap/research/measure.ps1 `
  -Binary C:\AgentSessions\.trellis\.runtime\target-b8\release\agent-session-grep.exe `
  -Phase after -Runs 20 -ProbeRuns 3          # 注意：会覆盖 after-measurements.json
pwsh -NoProfile -File .trellis/tasks/10-07-repo-probe-overlap/research/check3-measure.ps1 `
  -Binary C:\AgentSessions\.trellis\.runtime\target-b8\release\agent-session-grep.exe `
  -Phase after -Runs 20 -ProbeRuns 3          # 独立复核：只写 check3-after-*.json
python .trellis/tasks/10-07-repo-probe-overlap/research/output_diff.py         # 会写 output-diff.txt
python .trellis/tasks/10-07-repo-probe-overlap/research/check3-output-diff.py  # 只写 check3-output-diff.txt
```
