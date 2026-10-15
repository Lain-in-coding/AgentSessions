# Research: 两个 checkout 的关系 —— 优化与复测应当落在哪里

- Query: 基线证据/源界 harness 在哪个 checkout？当前工作区与"被测源码"的关系是什么？实现与复测应基于哪棵树？
- Scope: internal
- Date: 2026-10-15

## Findings

### 1. 两个 checkout

| | 协调/工作区 checkout | canonical 产品 checkout |
| --- | --- | --- |
| 路径（相对协调根） | `.`；本地别名指向同一协调树 | `.trellis/.runtime/attribution-closeout-20261012/canonical-repaired` |
| remote | `qin-devs/AgentSessions.git` | `Lain-in-coding/agent-session-grep.git` |
| 当前 HEAD | `221fc4f`（`feat/journal-online-maintenance`；工作树另有 3 个未提交的 spec/任务文件改动） | `de2c95d`（`main`，干净，且 `origin/main == HEAD`） |
| 含 baseline（b0fc44d）谱系 | 否（`git cat-file` 找不到 `b0fc44d/4f34287/de2c95d/b333b56`） | 是（`b0fc44d` → `4f34287` → `de2c95d`） |
| 含 `docs/evidence/core-beta/b0fc44d75690/` | 否（工作区只有 `88d86f4/`） | 是（manifest + 三跑报告 + `evidence-output/` 原始产物） |
| harness `--baseline-run` / `validate-baseline` / 三规模矩阵 | 否（`core_beta_benchmark.py` 无 `baseline_*`/`SCALES`） | 是（`SCALES`、`run_trace_trial`、`validate-baseline` 均在） |
| Trellis 任务目录 `.trellis/` | 是（本任务所在） | 否（该树没有 `.trellis/`） |

两棵树的 `crates/agent-session-grep-adapters-sqlite/src/lib.rs` 差异经逐行比对为
注释级（7 删 11 增，均为 semantic-floor/PRAGMA 出处注释，无功能差异；
`git hash-object`：workspace `b43b930c…`、canonical `e624d0cc…`）。

### 2. 结论性事实（都可以复核）

- 被测源 S 是 canonical 树的 `b0fc44d75690`；基线三跑报告、冻结二进制
  `baseline-cli.exe`、trace 原始记录只在 canonical 的
  `docs/evidence/core-beta/b0fc44d75690/` 与
  `evidence-output/current-source-20261014-b0fc44d75690/`。
- 当前工作区 checkout 无法原地执行 R3 协议（缺 `--baseline-run` 与
  三规模矩阵），也无法引用 PRD 给出的证据路径。
- 上一个任务（archive/2026-10/10-13-current-source-performance-baseline）
  的 PRD/implement 计划明文写过：**"Product checkout:
  .trellis/.runtime/attribution-closeout-20261012/canonical-repaired …
  all product commands below run there, not in the older coordination
  repository"**，且用 feature branch 在 canonical 仓库提交。

### 3. 对本任务的直接含义（供主会话定夺，不在研究角色内动手）

- 若沿用上一任务的先例：实现提交与三跑复测都在 canonical checkout
  （新 feature branch，基于 `main`/`de2c95d`；`--expected-commit` 用该分支
  提交的完整 SHA），产品 diff 再按治理流程回流（canonical 的 PR 链是
  `Lain-in-coding/agent-session-grep`；工作区的任务/研究文件留在本树）。
- 反向做法（在本工作区改代码）先前置把 baseline 界 harness 与证据引入本树，
  这既违反"不修改测量口径/同源"的最小改动原则，也会使"同 harness"落空；
  仅在主会话确认工作区才是发布权威时可行。
- 由于 adapter 子树功能等价，diff 在两树之间可干净移植；**风险只在
  "哪棵树被测量、报告绑定哪个 commit"**，不在代码内容。
- 复测前 canonical 树必须保持干净（`git status` 干净，`main == origin/main`，
  构建/输出目录在 `.gitignore` 的 `evidence-output/` 下）；run1 会在
  provenance 阶段强制校验 `--expected-commit` 与工作区一致。

## Caveats / Not Found

- 本文件不构成"允许在 canonical 树实施"的授权结论；它只记录事实与先例，
  最终实现位置需要主会话/用户确认（涉及跨仓库写与提交治理）。
- 工作区当前有 3 个未提交的 `.trellis/spec` 与任务文件改动（既有用户改动，
  与本任务无关，不得回退）。
- canonical 树上的 `evidence-output/` 是本地忽略目录；其原始 trace 属
  PRD 所说"私有 runtime 产物"，**不得进入产品提交**，引用数值时按
  `baseline-protocol-and-gate.md`/`commit-path-hotspot-map.md` 的口径转述。
