# 初始索引吞吐与峰值内存专项

## Goal

把 1M 消息的初始索引时间与峰值 RSS 各降低至少一半（对照 `2b8f895` 实测：482.6s / 4.58GB），且 search 与空转 sync 不回退。优化必须建立在阶段级 profile 证据上，逐项验证；不允许用 schema 迁移换性能。

## Requirements

- 基线冻结：初始 sync 10k/100k/1M = 2.42s / 35.0s / 482.6s；峰值 RSS = 154MB / 1.44GB / 4.58GB；库 = 51MB / 512MB / 4.34GB；空转 p50/p95 = 22/24ms、59/79ms、646/2920ms。合并 PR #12 后先在当前 main 复测确认基线可复现（同机、无并发负载、不清系统缓存，不作为冷盘结论）。
- 硬目标：1M 初始 sync ≤ 241s 且峰值 RSS ≤ 2.29GB；10k/100k 不得回退超过噪声；search p50/p95 与空转 sync 不得回退（±10% 护栏）。
- 先证据后优化：复制研究 harness（归档路径）到本任务并把 commit pin 参数化；先产出阶段级 trace（源读取/指纹、解析 staging、catalog 写入、FTS 投影、身份 sidecar、placement/membership/关系、outbox/CAS/generation、finalize）与候选排序。
- 每个优化单独提交，A/B 实测；中位收益 <5% 或造成任何正确性/性能回归即单独回退。保留单次 sync 的原子提交语义，不接受拆分事务换速度。
- 候选（按证据排序，允许 profile 后调整顺序）：批量语句/预编译复用；整目录状态读取范围（历史上有“非瓶颈”回退记录，必须重新验证）；`serde/id_json` 复用与避免重复 payload 解析/克隆；事务内 PRAGMA 调优（不得牺牲持久性契约）；空转双读/双哈希消除。
- 禁止：schema 迁移、新依赖、公共 API/wire 变更、语句缓存落地、默认 trigram/短词 LIKE。
- 若减半目标在不改 schema 前提下不可达：停止并提交归因证据与选项（投影改造/分片/迁移）。

## Acceptance Criteria

- [ ] 当前 main 复测确认基线（含语料哈希与二进制哈希）。
- [ ] profile 报告给出阶段归因与候选排序（原始 trace JSON）。
- [ ] 1M A/B：初始 sync 与峰值 RSS 各 ≥50% 改善；10k/100k 无回退；search/noop 在 ±10% 内。
- [ ] 空转双读/双哈希消除单独报告前后差值。
- [ ] workspace fmt/clippy/test 与 outbox/CAS/generation/关系完整性/身份测试全绿。
- [ ] 每个优化独立提交且可单独回退；证据 JSON 经校验器重算且拒绝篡改。
- [ ] 未发生 schema/依赖/公共契约变更。

## Notes

- 研究测量限制：Windows 计时曾出现 15.6ms 量化，harness 已改用高精度时钟；1M 初始 sync 是分批总计。
- 历史性能线（`perf/commit-write-path`、`perf/scope-commit-reads` 等）基线落后 main 246 提交，只作为线索；其中 commit 占 sync 98.2%、一次 commit-read scoping 被实测回退，必须在当前 main 重新验证。
