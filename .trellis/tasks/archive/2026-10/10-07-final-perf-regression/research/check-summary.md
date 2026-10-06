# B7 独立校验报告（trellis-check）

被校验交付：`.trellis/tasks/10-07-final-perf-regression/research/`（纯证据型，无产品代码改动）
校验时 HEAD：`4bea67f4d113976d439e91a5bac825bb10a7106d`；release 二进制
`.trellis/.runtime/target-b7/release/agent-session-grep.exe` sha256 `5924c24d…`
（与本交付口径一致）。

## 1. 校验项与结论

| # | 校验项 | 结论 | 复核证据 |
|---|---|---|---|
| 1 | `after-measurements.json` 支持 hotpath/b7 中位数与探测次数 | PASS | `check-numbers.log`（20 样本逐项重算 min/median/max、median 与表内 2 位小数逐项一致；探测 0/0/2 与 0/0/0 逐项一致；git_detect−seam 差值 +0.05/−0.25/+38.94 复算一致） |
| 2 | `core/core-beta-benchmark-full.json` 支持 P50/P95 表 | PASS | `check-numbers.log`：nearest-rank 重算全部 9 个指标 × P50/P95 与表内 3 位小数一致；样本数、dataset hash `8ebb21d29ce5…`、提交/二进制/生成时间、sizes 全部逐字段一致 |
| 3 | 独立复现热路径测量 | PASS | `check-measure.ps1`（仅改输出名/夹具目录）+ `check-after-measurements.json` + `check-rerun.log`：探测次数逐一相同（0/0/2、0/0/0），中位数漂移 +19%/+15%/+11%/−2%/−7%/−6%（±30% 内），exit 0；wire id 与交付一致 |
| 4 | 反证“不实施”判定（bare repo / `.git` 内 cwd 的 None→Some） | PASS（论据成立） | `check-residual-probe.ps1` + `check-residual-repo-probe-evidence.log`：独立重建上下文后 `rev-parse --show-toplevel` exit 128 而 `remote get-url origin` exit 0；真实二进制在这两种 cwd 只派发 1 个 git 进程（失败的门禁步），证明当前 = None |
| 5 | `current_repo` 进入 ranking boost 与 cursor digest | PASS | 代码复核：`application/src/lib.rs:939`（digest 字段 `current_repo`）、`:1953-1986`（in_current_repo → `ranking::apply_lexical_signals`）、`ranking.rs:44/82-84`（`CURRENT_REPO_SCORE_BOOST = 0.5*RRF`）；`repo_identity.rs:47-57/145-180`、`cli/src/lib.rs:3200-3212` 与 verdict 引用一致 |
| 6 | 写域检查（产品代码零改动） | PASS | `git status --short -- crates scripts docs .github .trellis/spec` 为空；B7 交付仅新增于任务 `research/`（任务目录整体 untracked） |
| 7 | 文档同步 N/A 声明 | PASS | 独立检索 README.md + docs/**/*.md（排除 docs/evidence/**）39 个文件：仅 2 处子串命中，均为 rustc 版本号 `1.97.1` 内的 `97.1`（`docs/operations/INSTALL-AND-UPGRADE.md:15`、`docs/release/OWNER-RELEASE-CHECKLIST.md:48`），非性能数字 |
| 8 | 门禁真实性（fmt/clippy/test） | PASS | `workspace-tests.log` 逐 target 求和 = 89 targets / 1811 passed / 0 failed / 20 ignored，末行 exit=0；`core-benchmark-run.log` 含 2 行 `valid …/v1 report`；复核者独立重跑 `check-workspace-tests.log`（1811/0/20，exit 0）、`check-fmt.log`（exit 0）、`check-clippy.log`（exit 0）、`check-clippy-fresh-target.log`（**全新 target dir** 68 个 check 单元、0 warning，exit 0） |

补充复核（非派发要求）：变体输出逐字段 diff（`check-output-diff.txt`）PASS——
deliverable 内 git_detect vs seam 仅 `meta.duration_ms` 不同；复核重跑输出与
deliverable 输出逐字段相同（仅 duration_ms）；B1 v18 夹具在当前二进制确为
exit 9 `schema_incompatible`（与 hotpath-comparison.md §5 一致）；三份夹具 wire id
逐字节一致。

## 2. 发现的问题

- trivial（已修）`b7-summary.md` §5：原写“唯一命中在历史证据 manifest/checklist…
  旧 artifact size 6,929,408 bytes”，与实际可复现的搜索结果不符（2 处命中，均为
  `1.97.1` 内的 `97.1` 子串，且 6,929,408 不匹配检索式）。已改写为准确表述，
  结论（N/A）不变。
- trivial（已修）`maintainability-map.md` 方法句：原写“以首个 `#[cfg(test)]` 行切分”，
  但 adapters-sqlite 首个 `#[cfg(test)]` 在 4,794 行（表内切分点是 9,503 行）。
  已改为“以表内标注的测试模块 `#[cfg(test)]` 行切分（…4,794 行另有门控辅助方法，
  按本口径计入生产段）”；行数 20,504/7,644/7,437/3,502/2,765/2,580/460 经
  `(Get-Content).Count` 逐项复核无误。
- substantive：无。

## 3. 证据缺口评估（clippy.log）

交付的 `clippy.log` 是幂等复核 run（0.54s，无 lint 输出），本身不构成“全量 lint
曾执行”的直接证据——属**轻微证据缺口**。判断：可接受。(a) 复核者以同一 target dir
重跑仍 exit 0；(b) cargo 指纹按内容散列，同一 target dir 的 “Finished” 意味着当前
工作树内容对应的 clippy 单元是最新的；(c) 复核者另用全新 target dir 跑出
`check-clippy-fresh-target.log`（38 Compiling + 68 Checking、0 warning、exit 0），
该缺口已由复核证据补齐。

## 4. 收口结论

可以收口提交。无产品代码改动、无 substantive 问题；8 项校验全部 PASS；
本轮仅 2 处 trivial 文档措辞修正（见 §2）。
