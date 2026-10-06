import fs from 'node:fs';
const root = 'C:/AgentSessions/.trellis/tasks/10-06-retrieval-quality';

const prd = [
'# 检索质量：证据阈值/正名/分桶评测（B4）',
'',
'> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准（D2 CLI-first；D3 门已过）。',
'> 依赖：B2（journal-retention）落定后再启动（同一 adapters-sqlite 文件写域）；输入：review-report P1-04、D3 matrix 3 处边界、父 design.md 第 6 节。',
'',
'## Goal',
'',
'把“检索名词”变成“检索质量”：为混合检索补上证据阈值门，为默认向量正名，并用分桶 holdout 给出真实数字；无增益就保持 optional/experimental，不靠改阈值自证。',
'',
'## Requirements',
'',
'1. 证据阈值门（D3 边界 2）：semantic/hybrid 候选中相似度低于下限者不得仅凭 RRF 名次分入选；阈值可配置（保守默认），lexical 路径不受影响；阈值选择必须由 holdout 数据支撑并在报告中给出依据。',
'2. 防御零证据 boost（D3 边界 1）：final_score 对零证据候选不得仅因 repo/recency 得正分（当前不可达也要加防御与测试，防未来新候选源引入时爆雷）。',
'3. 正名：bigram-hash 在 help/model status/结果元数据中标注为 fuzzy lexical vector（additive，不破坏 retrieval_mode 枚举）；README 的 full-text 声明补 16k 索引截断限定（口径）。',
'4. 评测：保留既有 100-query 合成集为回归；新增独立 holdout，按 zh/en/code/长文/近重复/无结果分桶；输出 Recall@k、MRR/nDCG、误命中、无结果解释、延迟/存储；真实 E5 若环境无权重则明确跳过（不许假跑）。',
'',
'## Non-goals',
'',
'- 不上 ANN、不做云/模型下载；不改 source span/证据契约；不为跑分改金标。',
'- 不破坏既有 retrieval_mode 枚举与 robot schema（新增字段 additive）。',
'',
'## Acceptance Criteria',
'',
'- [ ] 新增测试：0 相似度语义候选在 hybrid 中被证据门拒绝；零证据 boost 防御测试；既有 lexical 结果集不变。',
'- [ ] holdout 评测报告（分桶数字）+ 阈值选择依据；若无稳定增益则记录“保持 optional/experimental”的结论。',
'- [ ] 正名与 README 口径变更（文档 diff 可证）。',
'- [ ] cargo fmt/clippy/test（workspace，isolated target dir）全绿。',
''].join('\n');
fs.writeFileSync(root + '/prd.md', prd, 'utf8');

const design = [
'# 设计：retrieval-quality',
'',
'## 证据门位置',
'',
'- semantic 候选进入融合（RRF）前：similarity < floor 直接淘汰（adapter 层取数窗口内过滤，保持 filter-before-topk 不变量）。',
'- lexical 候选保持“MATCH 即证据”；recency/repo boost 只作用于已准入命中（application final_score 域约束为 >0 或显式 evidence flag）。',
'- floor 采用常量 + 可配置覆盖（配置文件/环境仅测试用），默认值来自 holdout 扫描结果（例如 0.20-0.35 区间取不伤召回的最小值）。',
'',
'## 正名',
'',
'- model_labeling/additive 元数据：model_kind=fuzzy_lexical_hash vs real_embedding；retrieval_mode 枚举不变。',
'',
'## 评测',
'',
'- 回归集=现有 frozen 100-query；holdout=新合成集+人工改写（zh/en/code/长文/近重复/无结果五桶）。',
'- 指标脚本与原始数据入库（research/），阈值扫描表同时给出 precision/recall 权衡。',
'',
'## 风险',
'',
'- 若 holdout 显示 floor 伤害召回 > 收益，则默认 floor=0（仅保留防御路径）并如实报告——不得为守承诺强行设阈值。',
''].join('\n');
fs.writeFileSync(root + '/design.md', design, 'utf8');

const impl = [
'# 实施计划：retrieval-quality',
'',
'## Steps',
'',
'1. 读 D3 matrix 边界锚点与现有 semantic/hybrid 取数链（adapters-sqlite/src/lib.rs:8201-8230 一带）+ application ranking.rs:44,67-79。',
'2. 实现 evidence gate（adapter 取数窗口内）+ final_score 防御，先写失败测试。',
'3. 正名与 README 口径（小改）。',
'4. holdout 构造与评测脚本；跑回归集 + holdout；阈值扫描。',
'5. 汇总结论：晋级或保持 optional/experimental（诚实）。',
'',
'## Validation commands',
'',
'    cargo fmt --all --check',
'    cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b4 -- -D warnings',
'    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b4',
''].join('\n');
fs.writeFileSync(root + '/implement.md', impl, 'utf8');
console.log('B4 artifacts written');
