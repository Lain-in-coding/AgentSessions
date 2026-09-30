# 复用后能力增强阶段：索引规模、命中体验与 Provider 覆盖

> 父任务：只维护任务地图、跨子任务验收与最终集成；不直接写产品代码。
> 研究依据：Wake 复用调研（`71aeca67ec80f8645d1f9d5199290c2c732036ce`，MIT；实测基线 `2b8f895`）与研究报告 §10。

## Goal

把复用调研的结论落地为四条可独立验收的产品增强线：命中窗口摘要、初始索引吞吐、Hermes SQLite 变体、Cursor IDE cursorDiskKV 变体；不引入新依赖、不改数据库 schema、不改发布承诺与 SLO。

## Requirements

- 子任务顺序：`match-centered-snippet` → `index-throughput` → `provider-hermes-sqlite` → `provider-cursor-diskkv`；每个子任务独立分支/worktree、独立实现与检查、独立合并到 main。
- 基线：PR #12 合并后的 main；研究证据与 harness 通过 PR #13 进入主线，归档研究目录保持只读。
- 索引吞吐硬目标：1M 初始 sync 与峰值 RSS 各降 ≥50%，search/noop 不回退（±10% 噪声护栏）；若必须改 schema 才可达标则停止并报告选项。
- 摘要契约：`SearchHit.text` 改为“有字面命中→命中窗口，否则前缀”；同字段同类型、计入 `max_response_bytes`；排名/游标/`why_matched`/证据字段不变；契约文档与 spec 同步。
- Provider：新增变体一律 additive；Hermes 目标 beta 仅在证据门全绿时提交晋级建议（owner 评审决定）；Cursor 目标 experimental；均不新增 resume 命令、不伪造原生身份/stability。
- 禁止：新依赖、schema 迁移、默认 trigram/短词 LIKE、语句缓存落地、后台 watcher、DSH/ZCode 适配、GPUI/远程同步/团队功能、真实 transcript fixtures。

## Acceptance Criteria

- [x] 四个子任务全部按各自验收完成并合并到 main。
- [x] 阶段集成报告：跨入口一致性（CLI/MCP/Robot/Web/TUI 同源摘要）、现有 16 provider golden 无回归、workspace fmt/clippy/test 全绿。
- [x] 证据一致性：吞吐 A/B 原始 JSON + 校验器通过；摘要用例矩阵通过；provider golden 与 PROVENANCE 齐备。
- [x] Hermes 晋级建议（或如实保持 experimental 的缺口说明）由 owner 评审后记录。
- [x] 无 schema/依赖/发布门变更；研究基线数字与新测量的口径一致。

> 技术验收项按子任务既有验收、已通过的 PR 检查与本轮文档核查记录；owner 项依据下述验收询问后的直接回复记录：吞吐子任务采用明确的停止并报告路径，不表示 ≥50% 目标达成；单次 PR 合并的流程偏离见集成报告 §6；既有验证不替代合并后 CI 的最终通过结果。

## 交付状态（2026-09-30）

- **实现已合并，阶段交付有条件验收**：PR #14 已合并到 `main`，合并提交 `b1324b475f1036718799c5c76ec8c0bfd33ddb24`；四个子任务均已归档。子任务生命周期完成不等于性能硬目标达标。
- **保留未达标项**：配对 A/B 的 1M 初始 sync 与峰值 RSS 均改善约 37.5%，未达到 ≥50%；不改写原目标，也不自动启动后续优化路线。
- **区分验证来源**：PR 实现版本的 12 项 CI 全通过，本地最终测试记录为 84 suites / 1833 passed / 0 failed；合并后 `ci`（`36562310348`）与 `core-beta-evidence`（`36562310272`）的 attempt 1/2 均遭账户计费/支出门拒绝启动、0 步骤。主协调者于 2026-09-30 通过 GitHub API 核验 attempt 3 均已完成、结论为 `SUCCESS`：`ci` 7/7 个 job 成功（6 个 job 各 19 步骤，cargo-deny 6 步骤），`core-beta-evidence` 4/4 个 job 成功（各 17 步骤）；两条运行均对应上述合并提交。
- **收尾文档已合并**：PR #15 于 `2026-09-30T02:58:30Z` 合并到 `main`，head 为 `3cb2b7575feba0acae2651b420fb5107bf4d378a`，合并提交为 `b19b83f2eee13b611c4bd3d693ba20fb36a46d53`；8 项 PR 检查全部 `SUCCESS`（`ci` 运行 `36661764514`：7 个 job；`security-audit` 运行 `36661764525`：1 个 job），head 与合并提交的文件树一致。同一合并提交的 main-push `ci` 运行 `36662288735` 也已由主协调者通过 GitHub API 核验为 `SUCCESS`、7/7 个 job 成功；PR 与 push 证据分别记录于集成报告 §7。
- **owner 条件验收已记录**：2026-09-29，owner 对明确披露“性能改善约 37.5%、未达 50%、Hermes/Cursor 保持 experimental”的验收询问直接回复 `keep going`；本次按同意上述条件继续收尾记录。原始回复、解释范围和限制见集成报告 §7；不将其解释为 beta 晋级、修改预算或降低 CI 门槛的授权。
- **临时公开已获明确授权**：2026-09-30 owner 已接受不可逆的临时公开曝光风险并授权继续；主协调者约 `2026-09-30T02:31Z` 确认仓库为 `PUBLIC`，临时公开成为已授权的 CI/收尾 PR 路径。无论成败，主协调者均须在 CI/收尾后恢复并核验 `PRIVATE`，不得超过本地回滚守卫截止 `2026-09-30T03:30:16Z`；本次操作未授权或实施账单、支出上限、凭据、产品代码、依赖、发布门或 SLO 变更。授权原文见集成报告 §7。
- **阶段验收与收尾 PR 门已通过**：五项验收记录、实现合并后 CI 及 PR #15 检查/合并证据已齐。归档生命周期以 `task.json` 为准，由主协调者通过 `task.py archive` 更新；归档提交继续接受正常 CI。阶段归档不代表临时公开操作完成，无论结果如何仍须按上述期限恢复并核验 `PRIVATE`。交付证据与收尾顺序见 `integration-report.md` §7。

## Notes

- 研究实测基线（1M）：初始 sync 482.6s、峰值 RSS 4.58GB、库 4.34GB、空转 p95 2.92s；进程级 search p50 119–150ms（含 11ms 启动）。
- 历史未合入的性能线（`perf/commit-write-path` 等，基线落后 main 246 提交）只作为线索引用，必须按当前 main 重新验证，不得盲目 cherry-pick。
