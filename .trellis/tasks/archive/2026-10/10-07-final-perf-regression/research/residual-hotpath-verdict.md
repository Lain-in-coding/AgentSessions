# 残余热路径判定：search 的单轮 repo 解析（结论：不实施）

任务：`.trellis/tasks/10-07-final-perf-regression`（PRD 要求 2）。
硬约束：保持 `current_repo_slug` 契约与 cursor digest 不变；证据不足或风险高就只给方案+数据。

## 1. 现状与成本（B7 实测）

- search 每请求解析一轮当前仓库 = 2 个 git 子进程：
  `git rev-parse --show-toplevel`（工作树门禁）+ `git remote get-url origin`（URL 派生）。
  代码：`crates/agent-session-grep-cli/src/repo_identity.rs:47-57`（两条命令）、
  `:145-180`（`GitRepoSlugResolver::resolve` 两级缓存）；调用点
  `src/lib.rs:3200-3212`（`current_repo_slug`，每次 CLI 进程新建 resolver）。
- B7 配对实测（`hotpath-comparison.md`）：search git_detect 48.02 ms vs seam 9.08 ms，
  单轮成本 **38.94 ms ≈ 19.5 ms/子进程**；get/show 已 0 探测。
- core benchmark 同步复测：`search_latency_ms` P50 63.600 ms、P95 72.133 ms（audit 基线
  114.505/162.896）。

## 2. 候选替代与证伪数据

原始证据：`residual-repo-probe-evidence.log`（真实 git 4.x，Windows；可复跑
`residual-probe-evidence.ps1`）。

### 候选 A（单子进程）：只跑 `git -C <cwd> remote get-url origin`

机制：git 自己完成发现并返回 origin URL（含 `insteadOf` 展开），省掉 `rev-parse`
门禁进程。预估收益：search 2→1 子进程，本机 **−19.5 ms**（48.02 → ≈28.5 ms；
core benchmark P50 63.6 → ≈44 ms）。

证伪（None → Some 的可观测漂移）：

| 上下文 | `rev-parse --show-toplevel`（现实现第 1 步） | `remote get-url origin`（单步候选） |
|---|---|---|
| 普通仓库子目录 | exit 0，返回工作树根 | exit 0，返回同一 URL（等价） |
| **bare repo（带 origin）** | **exit 128**：fatal: this operation must be run in a work tree | **exit 0**，返回 URL |
| **cwd 在 `repo/.git/hooks`（`.git` 内）** | **exit 128**，同上 | **exit 0**，返回 URL |
| GIT_DIR env 指向仓库、cwd 在外部 | exit 0（返回 cwd） | exit 0（返回 URL，等价） |

影响面（为什么不能静默做）：`current_repo` 既是 search 排序信号
（`agent-session-grep-application/src/lib.rs:1953-1990`，`ranking::CURRENT_REPO_SCORE_BOOST`），
又进入 cursor 摘要（`application/src/lib.rs:906-940`，`search_query_digest` 的
`"current_repo"` 字段）。None→Some 会同时改变排序与 cursor 绑定输入——违反本任务
“契约与 cursor digest 不变”的硬约束。要实施必须先改 spec/design 并由 owner 批准
（把 bare repo / `.git` 内 cwd 的语义从 None 明确改为 Some），然后更新
`crates/agent-session-grep-cli/tests/hotpath_repo_probe.rs`（断言 2→1）并补边界测试。

### 候选 B（文件系统替代）：手工 walk `.git` + 自行读 config

要等价重现 git 的 discovery/worktree 判定，需处理：`.git` 文件（worktree/submodule）
指针与 `commondir`、`include`/system/global 配置分层、`insteadOf` 重写、
`GIT_DIR`/`GIT_WORK_TREE`/`GIT_CEILING_DIRECTORIES`/跨卷边界等环境语义。
这与 `repo_identity.rs:5-13` 的既定决策（不自行解析 `.git`，交给 git 原生处理）直接冲突，
且任何近似实现都会重新引入 None↔Some 漂移面。**高风险，不采纳**。

### 候选 C（持久缓存 cwd→slug）：0 探测但需新状态与失效语义

跨进程缓存可把重复调用降为 0 探测，但引入新持久状态（缓存文件/DB 行）、失效语义
（origin 变更、目录移动、共享库多用户）与“缓存值 vs 实时派生”不一致风险，
同样可能改变 digest 绑定的实时语义。超出“低风险”，**不采纳（留作独立设计）**。

### 候选 D（空表短路）：session_repo_slugs 为空则跳过解析

被证伪：`current_repo` 参与 cursor digest；`sync` 后表会由空变非空，cursor 绑定状态
会静默漂移。**不采纳**。

## 3. 判定

**不实施代码改动。** search 保留一轮解析（2 子进程 ≈ 38.9 ms 本机）是当前契约下的
合法残余成本；get/show 已是 0 探测。理由不是“收益小”，而是唯一能省下这 ~19.5 ms 的
低改造路径会扩张 `current_repo_slug` 契约并改变排序/cursor 行为，属于需要 owner 决策的
契约变更而非低风险优化。

后续若要拿这部分收益（供 owner 决策，三选一）：

1. **契约扩张 + 单步解析**（最小 diff）：批准“bare repo / `.git` 内 cwd 解析为 slug”
   语义，改 `resolve()` 为单次 `remote get-url origin`，`hotpath_repo_probe.rs` 断言 2→1，
   新增 bare/`.git` 边界用例。收益：search ≈ −19.5 ms（本机），sync 每条新 cwd 也少 1 进程。
2. **跨进程缓存**：需要新的失效语义设计（收益更大，风险自担）。
3. **维持现状**：把 38.9 ms 记为已接受的残余成本（本任务选择项）。