# 阶段集成报告：复用后能力增强（4 子任务）

> 父任务：`.trellis/tasks/09-29-post-reuse-capability-phase`；分支 `feat/post-reuse-phase`。
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
- **16 provider golden 无回归**：`provider_matrix` 35 passed、`provider_probe_isolation` 3 passed（登记 3 个 SQLite fixture：opencode/cursor 既有 + `hermes/state.db` + 2 个 cursor disk-kv）。
- **阶段级最终门禁**（在归档完成后的 HEAD 上由协调者实跑）：
  - `cargo fmt --all --check` → exit 0
  - `cargo clippy --workspace --all-targets --offline -- -D warnings` → exit 0
  - `cargo test --workspace --offline --no-fail-fast` → **84 suites / 1833 passed / 0 failed**（日志 `target/final-workspace-test.log`，gitignored）

## 3. 未达标项：索引吞吐硬目标（如实记录）

| 指标 | 基线（当前 main 复测） | 优化后 | 变化 | PRD 硬目标 | 达标 |
|---|---|---|---|---|---|
| 1M 初始 sync | 412.7 s | 257.9 s | −37.5% | ≤241 s | ❌ |
| 1M 峰值 RSS | 4574.6 MB | 2858.8 MB | −37.5% | ≤2287 MB | ❌ |
| 10k / 100k sync | 2.18 s / 28.66 s | 1.52 s / 20.04 s | −30% 级 | 不回退 | ✅ |
| search p50/p95（1M） | 116.3 / 154.3 ms | 113.8 / 151.7 ms | −2.1% / −1.7% | ±10% 内 | ✅ |
| noop（护栏） | — | — | 封存 A/B +37.7% 噪声、配对交错 A/B −2.1% | ±10% 内 | ✅（证据偏薄，已如实披露） |

按 PRD「不改 schema 不可达则停止并归因」条款停止试错，归因与路线 A–D 见归档任务 `research/report.md` §7：① 批内数据副本（约 2.8 GB/200k 批，`results/auxiliary/mem-diag-fresh-200k-*.json`）；② 别名再生仍整表读取（31.3 s/批）；③ SQLite 工作集随写入线性增长（受单事务/持久性约束）。路线 A 继续范围化（不改 schema，预计再降 ~0.6–1.0 GB，属粗估）；B 小批次/流式（改原子批语义，需产品决策）；C schema/物化投影（本轮禁止）；D 关系表增量维护。

## 4. 证据一致性

- 吞吐：封存 JSON 21 份（`baseline`/`after`/`cand1-noop-verify` 各 3 规模 × 3 次）经 harness 校验器深度重算通过，改数未重封条即拒绝；语料哈希 10k `f4365a47…`、100k `34f02c25…`、1M `22519fe8…`；二进制哈希基线 `47e3c6a0…`（已不在磁盘，需重建 `72cb684`）、优化后 `3e55a4c4…`。
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
- 研究归档目录保持只读；辅助 worktree `C:/AgentSessions-worktrees/index-throughput-baseline`（detached `c2d58e8`）保留作基线参照，可用 `git worktree remove` 清理。
- 本任务外既存缺陷（建议单开任务）：已记录的空源再次 sync 报 `invalid_request: relocation request is invalid … source installation provenance is unresolved`（`relocation.rs:442-445`，非本次引入）。
