# 设计：retrieval-quality

## 证据门位置

- semantic 候选进入融合（RRF）前：similarity < floor 直接淘汰（adapter 层取数窗口内过滤，保持 filter-before-topk 不变量）。
- lexical 候选保持“MATCH 即证据”；recency/repo boost 只作用于已准入命中（application final_score 域约束为 >0 或显式 evidence flag）。
- floor 采用常量 + 可配置覆盖（配置文件/环境仅测试用），默认值来自 holdout 扫描结果（例如 0.20-0.35 区间取不伤召回的最小值）。

## 正名

- model_labeling/additive 元数据：model_kind=fuzzy_lexical_hash vs real_embedding；retrieval_mode 枚举不变。

## 评测

- 回归集=现有 frozen 100-query；holdout=新合成集+人工改写（zh/en/code/长文/近重复/无结果五桶）。
- 指标脚本与原始数据入库（research/），阈值扫描表同时给出 precision/recall 权衡。

## 风险

- 若 holdout 显示 floor 伤害召回 > 收益，则默认 floor=0（仅保留防御路径）并如实报告——不得为守承诺强行设阈值。
