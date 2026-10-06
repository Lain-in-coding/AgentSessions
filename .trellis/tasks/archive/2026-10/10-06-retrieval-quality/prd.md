# 检索质量：证据阈值/正名/分桶评测（B4）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准（D2 CLI-first；D3 门已过）。
> 依赖：B2（journal-retention）落定后再启动（同一 adapters-sqlite 文件写域）；输入：review-report P1-04、D3 matrix 3 处边界、父 design.md 第 6 节。

## Goal

把“检索名词”变成“检索质量”：为混合检索补上证据阈值门，为默认向量正名，并用分桶 holdout 给出真实数字；无增益就保持 optional/experimental，不靠改阈值自证。

## Requirements

1. 证据阈值门（D3 边界 2）：semantic/hybrid 候选中相似度低于下限者不得仅凭 RRF 名次分入选；阈值可配置（保守默认），lexical 路径不受影响；阈值选择必须由 holdout 数据支撑并在报告中给出依据。
2. 防御零证据 boost（D3 边界 1）：final_score 对零证据候选不得仅因 repo/recency 得正分（当前不可达也要加防御与测试，防未来新候选源引入时爆雷）。
3. 正名：bigram-hash 在 help/model status/结果元数据中标注为 fuzzy lexical vector（additive，不破坏 retrieval_mode 枚举）；README 的 full-text 声明补 16k 索引截断限定（口径）。
4. 评测：保留既有 100-query 合成集为回归；新增独立 holdout，按 zh/en/code/长文/近重复/无结果分桶；输出 Recall@k、MRR/nDCG、误命中、无结果解释、延迟/存储；真实 E5 若环境无权重则明确跳过（不许假跑）。

## Non-goals

- 不上 ANN、不做云/模型下载；不改 source span/证据契约；不为跑分改金标。
- 不破坏既有 retrieval_mode 枚举与 robot schema（新增字段 additive）。

## Acceptance Criteria

- [ ] 新增测试：0 相似度语义候选在 hybrid 中被证据门拒绝；零证据 boost 防御测试；既有 lexical 结果集不变。
- [ ] holdout 评测报告（分桶数字）+ 阈值选择依据；若无稳定增益则记录“保持 optional/experimental”的结论。
- [ ] 正名与 README 口径变更（文档 diff 可证）。
- [ ] cargo fmt/clippy/test（workspace，isolated target dir）全绿。
