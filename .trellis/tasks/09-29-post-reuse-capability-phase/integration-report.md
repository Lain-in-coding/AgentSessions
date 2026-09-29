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


## 7. 交付与合并后门禁状态（2026-09-29）

### 已确认的交付证据

- PR #14 于 `2026-09-29T11:31:46Z` 合并到 `main`，合并提交为 `b1324b475f1036718799c5c76ec8c0bfd33ddb24`；采用 merge 保留子任务各自提交，没有压平或改写历史。
- PR head `f238c10ba663794608cfb02aedafb8cc96abf39e` 的 **12 项 CI 全通过**。`git diff --quiet f238c10 b1324b4` 返回 0：合并提交的完整文件树与该实现版本一致。
- 复核既有 `target/final-workspace-test.log` 得到 **84 suites / 1833 passed / 0 failed**，并重新执行 `cargo fmt --all --check` 成功。这是既有测试日志复核与新的格式检查，**不是重新执行全部 workspace 测试**；§2 的 clippy/test 结果也不冒充新的合并后 CI 结果。

### 合并后 CI：启动受阻，不是测试失败或通过

| Workflow | Run ID | 最近 attempt | 结果 | 已执行步骤 |
|---|---|---|---|---|
| `ci` | `36562310348` | 2（首次运行后仅重试一次） | 账户计费/支出门拒绝启动 | 7 个 job 均为 0 |
| `core-beta-evidence` | `36562310272` | 2（首次运行后仅重试一次） | 账户计费/支出门拒绝启动 | 4 个 job 均为 0 |

GitHub 对全部失败 job 给出的注释是：近期付款失败，或需要调整支出上限。该通用注释不足以判断两种原因中的哪一种实际发生；本次没有读取或修改私有账单、提高预算、降低 CI 门槛或绕过检查。没有执行步骤，因此也没有可用于定位 Rust 测试回归的运行日志。

### 剩余验收与恢复步骤

1. 由账户 owner 检查 GitHub 的 **Billing & plans**，确认 Actions 恢复可用；条件不变时不再反复重试。
2. 条件解除后重试以下失败运行，检查全部 job 实际执行并成功；若出现真正的构建/测试失败，再按对应日志处理，不预先归因于既有 flaky 测试。

   ```text
   gh run rerun 36562310348 --repo qin-devs/AgentSessions --failed
   gh run rerun 36562310272 --repo qin-devs/AgentSessions --failed
   ```

3. 由 owner 评审 Hermes experimental 缺口说明，并决定是否接受性能未达标项的停止/后续路线记录；Cursor 双面库等产品取舍仍不在本轮擅自决定。
4. 收尾记录位于 `docs/post-reuse-closeout` 分支，仅修改本父任务文档与上下文。账户条件解除后，再为该分支创建并验证收尾 PR；当前不新建 PR 来重复触发已知受阻的检查。
5. 核对父任务所有验收项后再归档。当前 `in_progress` 表示交付后的验收收尾，不表示四个子任务需要重新实现，也不表示 ≥50% 性能目标已达成。
