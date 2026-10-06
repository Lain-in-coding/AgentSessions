# B8 第三轮（最终）独立复核报告（trellis-check round 3）

- 任务：.trellis/tasks/10-07-repo-probe-overlap；HEAD 283b9480e166e209d6f69747cb2e943686d3d765（未提交工作树）
- 源码：crates/agent-session-grep-cli/src/repo_identity.rs sha256 bff74be2cc4598bd21feef933f531007cbd38dcbfe1aecbe88a5a9b441e333bf
- 二进制：.trellis/.runtime/target-b8/release/agent-session-grep.exe sha256 68eb7f6d47854eb5e10ee5f2fb8c188bc4d91460f36a001ad8968b7024da6c3b（02:15:15 重建；cargo build --release --locked 复跑 0.17s 无重编译 → 与源码同步）
- 第三轮守卫实现核对：cwd_is_inside_toplevel(cwd, toplevel) = Path::new(cwd).starts_with(toplevel)（Rust 组件级比较、Windows 下不折叠大小写）；采纳条件 = toplevel_is_rediscoverable 与 cwd_is_inside_toplevel 同时成立，否则 git_origin_url(&toplevel) 串行补齐；env 五变量命中仍整段串行。与主会话描述一致。判否方向恒为回退旧串行语义，只会损失性能、不改变结果。
- 直接原因：H1/H2 的共同特征是 cwd（gitdir）不在门禁返回的 toplevel 之下 → 该守卫必然把这两类推回串行路径。

## ① 42 上下文结论（check3-semantics.ps1 / .log / .json）

- 总计 42/42 EQUAL、0 DIFF、0 lit=FAIL（模型级 A/B：旧串行 vs 三轮守卫模型；脚本把 Path::starts_with 近似为组件级、大小写敏感比较）。
- 重点行：
  - H1（toplevel/.git 属另一仓）：第二轮 DIFF(value) → 本轮 EQUAL（新路径 overlap+serial-reprobe，两测同返 h1-worktree-repo）。
  - H2（toplevel/.git 为无效空目录）：第二轮 DIFF(old=None) → 本轮 EQUAL(None)。
  - R1（仅 GIT_WORK_TREE）：EQUAL(None)、路径 serial(env)（保持第二轮修复）。
  - R2（相对 GIT_DIR + GIT_WORK_TREE，嵌套仓）：EQUAL(Some(envrepo))、serial(env)。
  - R3（core.worktree 外指，无环境变量）：EQUAL(None)、overlap+serial-reprobe；R3c（cwd=外部 worktree）EQUAL(None)。
- 其余 37 个上下文（五上下文、worktree、submodule 含其 gitdir、无 origin、大写 ORIGIN、本地路径、insteadOf、pushurl、worktree 级 config、GIT_DIR 绝对/天花板、H3 指针文件等）全部 EQUAL 且字面期望通过。

## ② E2E 真值表 + 探测次数（真实二进制 + check2-gitshim.rs 逐次 spawn 记录 + cursor query_digest 反推 slug；check3-e2e.log / .json）

| 上下文 | 期望（改动前语义） | 实测 68eb7f6d | 判定 | spawn 数 |
|---|---|---|---|---|
| plain root / subdir | Some(plain) | Some(plain) | MATCH | 2 |
| bare root | None | None | MATCH | 2 |
| .git cwd | None | None | MATCH | 2 |
| 非 git 目录 | None | None | MATCH | 2 |
| 无 origin | None | None | MATCH | 2 |
| linked worktree | Some(wt-host) | Some(wt-host) | MATCH | 2 |
| submodule | Some(submodule) | Some(submodule) | MATCH | 2 |
| R1 GIT_WORK_TREE | None | None | MATCH | 2（串行：gate + 从 W 探测） |
| R2 plain / R2b nested | None / Some(envrepo) | None / Some(envrepo) | MATCH | 1 / 2 |
| R3 core.worktree 外指 | None | None | MATCH | 3 |
| H1 toplevel/.git=别的仓 | Some(h1-worktree-repo) | Some(h1-worktree-repo) | MATCH | 3 |
| H2 toplevel/.git 无效 | None | None | MATCH | 3 |

**14/14 MATCH，0 DRIFT。** 探测次数：主路径（普通仓 root/subdir）仍 2（两条都 -C cwd，守卫放行重叠）；bare/.git/非 git/无 origin = 2（门禁丢弃 URL）；worktree/submodule = 2；R1 = 2、R2 plain/nested = 1/2；R3/H1/H2 = 3（重叠 2 + 串行重探 1，仅非常规布局付费）。逐条 spawn argv 见 check3-e2e.log。

## ③ 干净测量（check3-measure.ps1，20 次/变体、3 次探测计数、无并行构建；check3-after-measurements.json）

| 命令 | 中位 ms | 探测 | min / max |
|---|---:|---|---|
| get | 8.19 | 0,0,0 | 7.29 / 12.58 |
| show | 8.08 | 0,0,0 | 7.14 / 10.26 |
| search | 31.97 | 2,2,2 | 30.16 / 39.60 |
| seam/search | 8.74 | 0,0,0 | 8.18 / 9.38 |

- search 中位 31.97 ms：较 B7 基线 48.02 ms 下降 16.05 ms，与第一轮 32.01 / 第二轮 31.53 同量级 → 第三轮守卫未误伤主路径；单轮解析成本 38.94 → 23.23 ms。
- 对账：主会话 02:15 的 after-measurements.json（守卫版同一二进制）受并行构建干扰（get 中位 11.35 ms、search 41.32 ms、seam/get 11.04 ms），不采用；口径数据为本轮干净运行。
- 变体输出：check3-output-diff.py → RESULT: PASS（仅 meta.duration_ms）；与 after-output-*.json（守卫版重跑）交叉比对非易变字段 0 差异。

## ④ 是否仍有 DIFF 或理论漏洞

- 无已知可复现 DIFF：三轮反例（R1/R2/R3、H1/H2）全部收敛；42 上下文模型级 + 14 上下文真实二进制端到端 + 单测矩阵三路一致。
- 剩余仅为"保守回退"理论面（方向安全，只影响性能、不影响结果）：Path::starts_with 在大小写不一致路径、含 . 或 .. 的路径、相对路径、符号链接路径等情形会判否 → 回退串行（结果仍与改动前一致）。这些路径在真实调用中罕见；组件级比较避免了 /a/bc 与 /a/b 的字符串前缀误判。
- 已排除项（前轮结论，本轮适用）：GIT_CONFIG_GLOBAL / GIT_CONFIG_COUNT 注入 core.worktree 不改变 worktree 落点（git 2.55 实测）；GIT_OBJECT_DIRECTORY / GIT_NAMESPACE / GIT_INDEX_FILE / GIT_ALTERNATE_OBJECT_DIRECTORIES 不影响发现落点；H3（.git 指针文件指回同一 gitdir）等价。
- hotpath_repo_probe.rs 未修改（git diff 为空），断言未放宽（get/show/status 0 探测、search 2 探测）。

## ⑤ 文档对账（已完成）

research/hotpath-overlap-comparison.md 已重写为与最终守卫版一致：
- 顶部明确「最终实现 = 守卫版」并描述三条规则；最终二进制 sha256 68eb7f6d...（表 1-4 口径 = check3-after-measurements.json）；收窄前 1ab64b09... 作为历史行保留并注明来源 after-measurements-unguarded.json；
- 表 1/2 增加"收窄前"与"最终"两列 + 最终 Δ vs B7；表 3 更新为守卫版 min/max；表 4 增加守卫版复测（check3-output-diff.*）；第 5 节加入三轮守卫矩阵与 E2E 结论；第 6 节门禁补 check3-* 日志；第 7 节命令补 check3-measure.ps1 与 check3-output-diff.py；
- 明确标注 02:15 的 after-measurements.json 受并行构建干扰、不作口径（文件保留未删）；未删除任何证据文件。

## ⑥ 门禁（isolated target dir .trellis/.runtime/target-b8）

| 门禁 | 结果 | 日志 |
|---|---|---|
| cargo fmt --all --check | exit 0 | check3-fmt.log |
| cargo clippy -p agent-session-grep-cli --all-targets --locked -D warnings | exit 0 | check3-clippy-cli.log |
| cargo clippy --workspace --all-targets --locked -D warnings | exit 0 | check3-clippy-workspace.log |
| cargo test -p agent-session-grep-cli --locked | 590 passed / 0 failed（含五上下文矩阵测试与 hotpath probe） | check3-test-cli.log |
| cargo test --workspace --locked | 1812 passed / 0 failed | check3-test-workspace.log |
| cargo build --release -p agent-session-grep-cli --locked | 无重编译（二进制与源码同步） | check3-release-freshness.log |

## 最终结论

可以收口提交。第三轮守卫把 H1/H2 收敛且未影响主路径收益（search 31.97 ms 远低于 48.02 ms、探测 2/2/2）；语义在模型级、真实二进制端到端、真实 resolver 单测三路全部一致；门禁全绿；交付文档已与最终代码/二进制对账。本轮未改动产品代码（repo_identity.rs 归主会话）、未 commit、未覆盖任何历史证据文件。

（附注：本轮执行中曾有一次命令误落 bash，导致本报告文本被部分当作 shell 行解析；已核查仓库状态、源码/二进制哈希与证据文件均未受影响，报告随后以 PowerShell 重新写入。）
