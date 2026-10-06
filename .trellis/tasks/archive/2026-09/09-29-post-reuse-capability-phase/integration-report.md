# 阶段集成报告：复用后能力增强（4 子任务）

> 父任务：`.trellis/tasks/09-29-post-reuse-capability-phase`；实现分支 `feat/post-reuse-phase`。
> 研究依据：Wake 复用调研 `71aeca67ec80f8645d1f9d5199290c2c732036ce`（MIT），研究报告 §10。

## 1. 子任务结果

| # | 子任务 | 交付 | 提交（关键） | 独立核查 | 结果 |
|---|---|---|---|---|---|
| 1 | `match-centered-snippet` | `SearchHit.text` 改为命中窗口语义（有字面证据→2右:1左居中窗口，否则前缀）；CLI human 预览按 `why_matched` 最早词元重居中（≤120 字符，40 字符左上下文） | `d556216` | PASS（残留 3 项非阻断边界已记录） | ✅ 达成 |
| 2 | `index-throughput` | 6 项提交写路径优化（空转双读消除、写连接 page cache、late no-op 状态复用、durable manifest 单次计算、别名再生范围化、提交状态按批范围化） | `de33e75`…`748c720` + 证据 `4ea3a2c`/`1e1cef6`/`4c57d96`/`392b098` | PASS-with-findings（11 条全部非阻断，F1–F8 已修正） | ⚠️ 硬目标未达成（见 §3） |
| 3 | `provider-hermes-sqlite` | 新变体 `hermes/sqlite-state-v1`（只读有界 SQLite 快照、两种 tool-call 形态、profile 命名空间、fail-closed 工具关联） | `c8a2f56`/`e9f01f6`/`ecb9783` | PASS-with-findings（5 条非阻断，F1/F2/F3/F5 已修正） | ✅ 达成（维持 experimental） |
| 4 | `provider-cursor-diskkv` | 新变体 `cursor/disk-kv-v1`（`cursorDiskKV`：header 顺序、显式坏行状态、工具输入选择顺序、native id 保真为观测） | `1dc2572`/`989fc5d`/`f93bfcb` | PASS-with-findings（3 条非阻断，F1/F2 已修正） | ✅ 达成（维持 experimental） |

## 2. 跨入口一致性与回归

- **摘要跨入口同源**：窗口逻辑落在 Application 层共享投影（`crates/agent-session-grep-application/src/snippet.rs` 由 `assemble_search_hit` 调用），CLI/MCP/Robot/Web/TUI 与 wire 字段由同一投影产出，无需逐入口改动；CLI 仅额外做人类可读预览重居中。
- **16 provider golden 无回归**：`provider_matrix` 35 passed、`provider_probe_isolation` 3 passed。相关 SQLite golden 文件共 5 个：新增 Hermes `state.db`、Cursor `disk-kv.db` 与 `disk-kv-shuffled.db`，保留已有 OpenCode `basic.db` 与 Cursor `basic.db`；3 passed 是隔离测试数量，不是 fixture 文件数量。
- **阶段级最终门禁**（在归档完成后的 HEAD 上由协调者实跑）：
  - `cargo fmt --all --check` → exit 0
  - `cargo clippy --workspace --all-targets --offline -- -D warnings` → exit 0
  - `cargo test --workspace --offline --no-fail-fast` → **84 suites / 1833 passed / 0 failed**（日志 `target/final-workspace-test.log`，gitignored）

## 3. 未达标项：索引吞吐硬目标（如实记录）

| 指标 | 配对基线（`72cb684`，n=3） | 优化后（n=3） | 相对配对基线变化 | 原 PRD 绝对门槛 | 达标 |
|---|---|---|---|---|---|
| 1M 初始 sync | 412.7 s | 257.9 s | −37.5% | ≤241 s | ❌ |
| 1M 峰值 RSS | 4574.6 MB | 2858.8 MB | −37.5% | ≤2.29 GB（子报告按 ≤2287 MB 判定） | ❌ |
| 10k / 100k sync | 2.18 s / 28.66 s | 1.52 s / 20.04 s | −30% 级 | 不回退 | ✅ |
| search p50/p95（1M） | 116.3 / 154.3 ms | 113.8 / 151.7 ms | −2.1% / −1.7% | ±10% 内 | ✅ |
| noop（护栏） | — | — | 封存 A/B +37.7% 噪声、配对交错 A/B −2.1% | ±10% 内 | ✅（证据偏薄，已如实披露） |

口径区分：原研究基线 `2b8f895` 为 482.6 s / 4.58 GB，PRD 据此写入绝对门槛 ≤241 s / ≤2.29 GB；本轮固定 `72cb684` 做配对 A/B，时间与 RSS 各改善约 37.5%。相对原研究基线，时间改善约 46.6%。**241 s 不是配对基线 412.7 s 的一半**（后者为 206.35 s），不能混用这两种比较口径；无论原 PRD 绝对门槛还是配对 A/B 的 ≥50% 目标，均未达成。

按 PRD「不改 schema 不可达则停止并归因」条款停止试错，归因与路线 A–D 见归档任务 `research/report.md` §7：① 批内数据副本（约 2.8 GB/200k 批，`results/auxiliary/mem-diag-fresh-200k-*.json`）；② 别名再生仍整表读取（最终 trace 为 10.4–24.1 s/批，5 批合计 83.82 s）；③ SQLite 工作集随写入线性增长（受单事务/持久性约束）。路线 A 继续范围化（不改 schema，预计再降 ~0.6–1.0 GB，属粗估）；B 小批次/流式（改原子批语义，需产品决策）；C schema/物化投影（本轮禁止）；D 关系表增量维护。

## 4. 证据一致性

- 吞吐：封存 benchmark JSON 共 21 份（`baseline` 9 份、`after` 9 份，均为 3 规模 × 3 次；`cand1-noop-verify` 仅 100k 规模 × 3 次，共 3 份；不含 auxiliary 诊断文件）。既有验收记录为 harness 校验器 21/21 深度重算通过，本轮只核对汇总与文件清单，不复跑基准或校验器；改数未重封条即拒绝；语料哈希 10k `f4365a47…`、100k `34f02c25…`、1M `22519fe8…`；二进制哈希基线 `47e3c6a0…`（已不在磁盘，需重建 `72cb684`）、优化后 `3e55a4c4…`。
- 等价性：基础脚本 15 表 + `active_generation`（10k/100k）；扩展脚本覆盖 no-op/缩小替换/墓碑/二次 no-op 且含全部 4 张 membership，**25 张非影子表 + active_generation 0 不一致**（`results/auxiliary/equiv-extended-10k.json`）。
- 摘要：application 283 passed（含 11 个 snippet 单测）、cli 590 passed（含 8 个预览单测）；契约、ports 注释、application spec、CLI/Robot/MCP 契约草案同步。
- Provider：Hermes 合成 golden（BLAKE3 `2a21db8b…`）+ PROVENANCE；Cursor 合成 golden（`62f26642…` / 洗序版 `182e1430…`）+ PROVENANCE；两者的固定上游/Wake 引用逐行核对一致。

## 5. Provider 成熟度

- **Hermes SQLite 变体：维持 experimental，不提交 beta 晋级建议**（证据门未全绿）。缺口：无 discovery root；无 seeded property 语料；消息 rowid 未升为 canonical native id；生产有效整源上限 32 MiB（adapter 内部 128 MiB 不可达）；capability 行仍只登记 JSON 变体。已记录于 `docs/product/PROVIDER-BETA-READINESS.md`。
- **Cursor disk-kv 变体：维持 experimental**（Cursor 私有存储无官方版本化契约；未读任何真实 `state.vscdb`）。
- 需 owner 决策的 follow-up：Hermes 整源上限是否放宽；计数型上限复用 ports 的 "bytes" 文案（用户可见诊断失真）；>32 MiB 时选择层把尺寸拒绝掩盖为其他 adapter 的 probe 失败；Cursor 双面库（ItemTable + cursorDiskKV 同时存在）是否引入显式变体选择。

## 6. 约束与偏离记录

- **无 schema 迁移、无新依赖、无公共 wire/契约字段变更、无发布门/SLO 变更**（逐子任务核查确认；唯一 Cargo 变更是 hermes/cursor 复用工作区已锁定的 `rusqlite 0.40.2`）。
- 偏离：父任务原计划「每个子任务独立分支/worktree、独立合并 main」，实际四条线在**同一分支 `feat/post-reuse-phase`** 上按序完成，改为单次 PR 统一合并；子任务仍各自独立提交、独立实现与独立核查、可单独 revert。
- 研究归档目录保持只读；辅助基线 worktree（detached `c2d58e8`）保留作本地参照，位置不纳入共享任务状态；本次未清理任何 worktree。
- 本任务外既存缺陷（建议单开任务）：已记录的空源再次 sync 报 `invalid_request: relocation request is invalid … source installation provenance is unresolved`（`relocation.rs:442-445`，非本次引入）。


## 7. 交付与合并后门禁状态（2026-09-30）

### 已确认的交付证据

- PR #14 于 `2026-09-29T11:31:46Z` 合并到 `main`，合并提交为 `b1324b475f1036718799c5c76ec8c0bfd33ddb24`；采用 merge 保留子任务各自提交，没有压平或改写历史。
- PR head `f238c10ba663794608cfb02aedafb8cc96abf39e` 的 **12 项 CI 全通过**。`git diff --quiet f238c10 b1324b4` 返回 0：合并提交的完整文件树与该实现版本一致。
- 复核既有 `target/final-workspace-test.log` 得到 **84 suites / 1833 passed / 0 failed**，并重新执行 `cargo fmt --all --check` 成功。这是既有测试日志复核与新的格式检查，**不是重新执行全部 workspace 测试**；§2 的 clippy/test 结果也不冒充新的合并后 CI 结果。

### 合并后 CI：attempt 1/2 启动受阻；attempt 3 已完成并成功（2026-09-30）

两条运行均对应合并提交 `b1324b475f1036718799c5c76ec8c0bfd33ddb24`。下表为主协调者于 2026-09-30 通过 GitHub API 核验的 attempt 3 最终结果：两条运行均已完成且为 `SUCCESS`。本轮文档检查未查询、触发或重跑工作流。

| Workflow | Run ID | attempt 1/2（历史） | attempt 3（2026-09-30 最终结果） |
|---|---|---|---|
| `ci` | `36562310348` | 账户计费/支出门拒绝启动；7 个 job 均为 0 步骤 | `SUCCESS`；7/7 个 job 成功，6 个 job 各 19 步骤、cargo-deny 6 步骤 |
| `core-beta-evidence` | `36562310272` | 账户计费/支出门拒绝启动；4 个 job 均为 0 步骤 | `SUCCESS`；4/4 个 job 成功，各 17 步骤 |

GitHub 对 attempt 1/2 全部失败 job 给出的注释是：近期付款失败，或需要调整支出上限。该通用注释不足以判断两种原因中的哪一种实际发生；当时没有读取或修改私有账单、提高预算、降低 CI 门槛或绕过检查。这两个历史 attempt 没有执行步骤，因此没有可用于定位 Rust 测试回归的运行日志；该描述不适用于已执行步骤的 attempt 3。

### 收尾文档 PR #15：检查与合并已核验（2026-09-30）

- PR #15 于 `2026-09-30T02:58:30Z` 合并到 `main`；PR head 为 `3cb2b7575feba0acae2651b420fb5107bf4d378a`，合并提交为 `b19b83f2eee13b611c4bd3d693ba20fb36a46d53`。
- 主协调者核验 **8 项 PR 检查全部 `SUCCESS`**：`ci` 运行 `36661764514` 的 7 个 job、`security-audit` 运行 `36661764525` 的 1 个 job 均成功。该证据与上节 PR #14 实现提交的 attempt 3 结果分别记录。
- 本地复核 `git diff --quiet 3cb2b75 b19b83f` 返回 0，PR head 与合并提交完整文件树一致；工作区已快进至该合并提交。此外，主协调者于 2026-09-30 通过 GitHub API 核验该合并提交的 main-push `ci` 运行 `36662288735`（attempt 1）已完成且为 `SUCCESS`，7/7 个 job 均成功；这是独立的 push 运行证据，不是由文件树相同推定。

### Owner 条件验收决定（2026-09-29）

- 验收询问明确披露：本阶段按已记录的停止规则交付，配对 A/B 改善约 37.5%、未达 50% 目标，Hermes 与 Cursor 新变体保持 experimental。
- Owner 对该询问的直接回复为 **`keep going`**。本次将该上下文中的继续指令解释为同意上述条件继续收尾，并记录 PRD 的 owner 验收项；不将此原始回复改写成声称所有性能指标已达标的确认。
- 同意范围仅限本阶段有条件交付：原性能目标和未达标证据不变，不提交 beta 晋级，不自动启动后续优化路线，不授权修改账户账单、预算或 CI 门槛；Cursor 双面库等后续产品取舍仍未决定。
- 五项 PRD 验收记录已齐，PR #14 实现合并后 CI 与 PR #15 收尾检查/合并证据均已核验，阶段验收与收尾 PR 门已通过。生命周期状态与完成日期由 `task.py archive` 写入 `task.json`；本轮操作结束前仍必须恢复并核验 `PRIVATE`。

### 临时公开授权与执行快照（2026-09-30）

- Owner 已明确接受不可逆的临时公开曝光风险，原始回复为：**“接受，接受，继续吧，不要让我们的项目推进受到阻碍”**。这是本次临时公开并继续 CI/收尾的明确授权，与 2026-09-29 的条件验收回复分别记录。
- 主协调者于约 `2026-09-30T02:31Z` 核验仓库为 `PUBLIC`，随后于当日通过 GitHub API 确认上述两条旧运行的 attempt 3 均已实际执行并最终 `SUCCESS`（job/步骤数量见上表）。合并后 CI 门已通过，但不据此宣称账户账单已恢复。
- 主协调者独占 GitHub visibility、CI 与收尾 PR 操作。**无论 CI/收尾成功或失败，均须在结束后恢复并核验 `PRIVATE`，最迟不得超过本地回滚守卫截止 `2026-09-30T03:30:16Z`**；若届时仍未完成，先恢复 `PRIVATE`，未完成事项继续保留，不据此标记父任务完成。恢复时间与回读核验结果由主协调者补录到收尾 PR 的运行记录及本地 journal，避免为记录最终可见性再次触发 CI；本节为执行快照，不代表仓库持续公开。
- 本次临时公开操作未授权或实施账单、支出上限、凭据、产品代码、依赖、发布门或 SLO 变更；≥50% 性能目标仍未达成，两项 SQLite 变体继续保持 experimental，不由本次授权推导 beta 晋级或新的产品取舍。

### 阶段归档与临时公开清理

1. PR #15 检查及合并已满足收尾 PR 门。主协调者核对验收记录后执行 `task.py archive`，由脚本更新生命周期状态/完成日期并归档；归档记录提交、合入及正常 CI 仍按既有流程，不使用跳过 CI 或管理员绕过。本轮文档补录不代为修改归档元数据。
2. 本轮必要检查完成后，主协调者按上述期限，无论成败均恢复 `PRIVATE` 并回读核验；实际恢复时间和最终可见性写入 PR #15 的操作评论及本地 journal，不为最终可见性另造触发 CI 的提交。若截止时仍有未完成事项，优先按时恢复 `PRIVATE`，其余事项如实保留。

阶段验收与收尾 PR 门已有上述证据，归档生命周期以 `task.json` 为准；归档不等于完成临时公开清理，核验 `PRIVATE` 前不宣告本轮整体完成。已确认成功的检查不再列为待办，也不代表 ≥50% 性能目标达成或 provider beta 晋级。
