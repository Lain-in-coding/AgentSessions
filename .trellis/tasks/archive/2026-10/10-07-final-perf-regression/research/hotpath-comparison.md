# B7 热路径复测对比（audit 基线 / B1 after / B7）

口径与 B1 完全一致：`research/measure.ps1`（B1 harness 的最小改动副本，仅 `$expDir`/`$outDir`
指向本任务），release 二进制、20 次/变体、3 次 GIT_TRACE 探测计数、`--robot
--request-id hotpath-diff`、`ASG_CLOCK_MS=1787616000000`、cwd=`C:\AgentSessions`
（带 origin 的真实 git 仓库）、单条确定性合成消息；`seam` = `ASG_CURRENT_REPO=''`
既有测试注入（零探测基线）。

## 1. git_detect（真实 repo 发现）中位数与探测次数

| 命令 | audit 基线 5b232cd 中位 ms（探测） | B1 after 912f45c8 中位 ms（探测） | B7 5924c24d 中位 ms（探测） | Δ vs 基线 |
|---|---:|---:|---:|---:|
| get | 53.665（2） | 9.263（0） | **7.96**（0） | −45.70 |
| show | 52.438（2） | 9.326（0） | **7.91**（0） | −44.53 |
| search | 97.112（4） | 55.728（2） | **48.02**（2） | −49.09 |

- 基线行的探测次数取自 B1 `before-measurements.json`（同一 commit 5b232cd 的本地构建
  6d7ebe73，同一 harness；audit JSON 本身只存时长）。
- B7 行原始数据：`after-measurements.json`（binary sha256 `5924c24d…`，commit `4bea67f`），
  三次探测计数逐次一致（0,0,0 / 0,0,0 / 2,2,2），全部 exit code 0。

## 2. seam（零探测参照，噪声地板）

| 命令 | audit 基线 中位 ms | B1 after 中位 ms | B7 中位 ms |
|---|---:|---:|---:|
| get | 9.152 | 9.086 | 7.91 |
| show | 8.969 | 8.957 | 8.16 |
| search | 9.982 | 9.848 | 9.08 |

## 3. 残余单轮解析成本 = git_detect − seam（同次测量内配对）

| 命令 | audit 基线 | B1 after | B7 |
|---|---:|---:|---:|
| get | +44.513 | +0.177 | +0.05 |
| show | +43.469 | +0.369 | −0.25 |
| search | +87.129 | +45.880 | **+38.94**（2 个 git 子进程 ≈ 19.5 ms/进程） |

结论：B1 的修复在 B7 HEAD 上保持成立——get/show 已是 0 探测（与 seam 无差别）；
search 仍保留一轮 repo 解析（rev-parse + remote get-url，2 子进程），是本机热路径上
唯一剩余的 git 成本。

## 4. min/max（20 次原始样本）

| 变体/命令 | B7 中位 | B7 min | B7 max |
|---|---:|---:|---:|
| git_detect/get | 7.96 | 7.37 | 9.86 |
| git_detect/show | 7.91 | 7.54 | 10.89 |
| git_detect/search | 48.02 | 45.65 | 50.58 |
| seam/get | 7.91 | 7.46 | 12.16 |
| seam/show | 8.16 | 7.45 | 8.65 |
| seam/search | 9.08 | 8.26 | 9.87 |

## 5. 与 B1 夹具的可比性说明

- B1 的 `.trellis/.runtime/hotpath-experiment/fixture.db` 是 schema v18；当前 HEAD 支持
  schema v19（B2/B3 迁移），直接复用时所有命令 fail-closed（`schema_incompatible`，
  exit 9，0 探测）。因此 B7 用同一 seed 命令、同一 clock/request-id 在同一 harness 下
  新建 v19 夹具 `.trellis/.runtime/hotpath-experiment-b7/fixture.db`。
- 新建夹具的 wire id 与 B1 **逐字节一致**：`msg_v1_cb6837d0712fbb4e022f869652978b74`
  （`hotpath-experiment/wire-id.txt` == `hotpath-experiment-b7/wire-id.txt`），即数据语义
  完全同源，差异仅是 schema 版本。

## 6. 变体输出一致性（补充完整性检查）

`output_diff.py`（B1 脚本的变体对比改版）对读 6 份 canonical 输出做逐字段 walk：
get/show 25 个叶子字段 0 差异；search 仅 `meta.duration_ms` 不同（42 → 1 ms），
hits/score/page/cursor/schema/outcome 全部一致 → PASS（`output-diff.txt`）。

## 7. 命令

```powershell
cargo build --release -p agent-session-grep-cli --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b7
pwsh -NoProfile -File .trellis/tasks/10-07-final-perf-regression/research/measure.ps1 `
  -Binary C:\AgentSessions\.trellis\.runtime\target-b7\release\agent-session-grep.exe `
  -Phase after -Runs 20 -ProbeRuns 3
python .trellis/tasks/10-07-final-perf-regression/research/output_diff.py
```