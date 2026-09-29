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

- [ ] 四个子任务全部按各自验收完成并合并到 main。
- [ ] 阶段集成报告：跨入口一致性（CLI/MCP/Robot/Web/TUI 同源摘要）、现有 16 provider golden 无回归、workspace fmt/clippy/test 全绿。
- [ ] 证据一致性：吞吐 A/B 原始 JSON + 校验器通过；摘要用例矩阵通过；provider golden 与 PROVENANCE 齐备。
- [ ] Hermes 晋级建议（或如实保持 experimental 的缺口说明）由 owner 评审后记录。
- [ ] 无 schema/依赖/发布门变更；研究基线数字与新测量的口径一致。

## Notes

- 研究实测基线（1M）：初始 sync 482.6s、峰值 RSS 4.58GB、库 4.34GB、空转 p95 2.92s；进程级 search p50 119–150ms（含 11ms 启动）。
- 历史未合入的性能线（`perf/commit-write-path` 等，基线落后 main 246 提交）只作为线索引用，必须按当前 main 重新验证，不得盲目 cherry-pick。
