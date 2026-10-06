# B4 检索质量：证据门/正名/分桶评测 — 结果报告

> 任务：`.trellis/tasks/10-06-retrieval-quality`（父任务 10-05 · B4）。
> 本文件 + `threshold-scan.json` + `raw-runs.jsonl` + `frozen-regression/` 是结论的原始证据。
> 口径：全部为合成数据、本地执行；**没有**发布/签名动作，也不宣称语义质量晋级。

## 1. 结论（先给答案）

| 项 | 结论 |
|---|---|
| 语义证据门默认下限 | **`SEMANTIC_SIMILARITY_FLOOR_DEFAULT = 0.2`**（adapter 常量，`sqlite` 取数窗口内、top-k 堆插入前过滤；测试/评测可用 `ASG_SEMANTIC_SIMILARITY_FLOOR` 显式覆盖） |
| 选择依据 | holdout 六桶「零召回损失」区间 = 0.00–0.45；frozen 100-query 回归「零损失」区间 = 0.00–0.25（0.30 起 semantic@10 下滑）。取两区间交集上界 0.25，再退一格余量 → **0.20** |
| 未采用的激进阈值 | 0.45 可让「无结果」查询真正返回空并砍掉 ~46% 误命中，但 frozen 回归 semantic@10 从 0.475 掉到 0.340（-13.5pt）、hybrid@10 0.755→0.750；按 PRD「伤召回大于收益则不设阈」的口径**拒绝**，并如实记录 |
| 零证据 boost 防御 | `final_score` 首行 `if !(relevance > 0.0) { return 0.0 }`（覆盖 0/负/NaN）；今天不可达，测试锁死 |
| 正名 | `bigram-hash` → **fuzzy lexical vector**（`model_kind=fuzzy_lexical_hash` / `model_label`），additive 于 `providers`、`model status`、`index embeddings` 结果元数据与 help；`retrieval_mode` 枚举与 `search` schema 未动 |
| README | full-text 声明补 16,000 字符/消息索引截断限定（catalog 保留全文），并说明证据门 |
| 真实 E5 | **跳过**：本机无已验证 bundle（`model_kind=fuzzy_lexical_hash`、`skip_real_e5=true`），不假跑；0.2 只对 fuzzy-lexical 尺度标定 |
| 晋级结论 | 维持 lexical 默认、semantic/hybrid 实验性；`promotion_claim=none`（与既有诚实口径一致） |

## 2. 证据门实现位置

- `crates/agent-session-grep-adapters-sqlite/src/lib.rs`：新增 `SEMANTIC_SIMILARITY_FLOOR_DEFAULT`、`SqliteStore::set_semantic_similarity_floor` / `semantic_similarity_floor`；
  `query_semantic_filtered` 在**候选进入 top-k 堆之前** 丢弃 `cosine_similarity < floor` 的候选（filter-before-topk 不变量对证据门同样成立；RRF 因此看不到零证据候选）。
- 过滤发生在谓词（provider/time/repo/facets）之后、堆插入之前，同一取数窗口；分页哨兵语义与既有实现一致。
- `floor=0.0` 时仍排除负相似度（防御路径）；默认值由 `semantic_evidence_floor_default_is_the_holdout_choice` 测试锚定到本报告选择。
- 覆盖通道：CLI `ASG_SEMANTIC_SIMILARITY_FLOOR`（显式注入，非法值 → `invalid_request`，绝不静默换值；e2e 锚定）。

## 3. holdout 设计与数字

- 语料：12 会话 × 10 消息 = **120 条**（Claude JSONL，合成、无真实 transcript）；每查询有唯一 marker，gold 只用生成器记账且逐条 `get` 验证可达。
- 查询：**54 条**（zh 10 / en 10 / code 10 / long_body 4 / long_tail 4 / near_dup 8 / no_result 8）。
- 构造契约：query 词元是 gold 文本的连续子串（"remembered phrase"，即默认 lexical-first 引擎的实际使用点）；`long_tail` 的 marker 位于 16k 索引上限之外，按契约**必须**不可检索（运行时断言）。
- 纯 paraphrase 召回不在本 holdout 口径内（bigram 模糊词法本就不做语义改写），由 frozen 100-query 回归覆盖。

### 3.1 分桶结果（hybrid @ 默认 0.2，shipped-default 运行）

| 桶 | 查询数 | Recall@10 | MRR@10 | 误命中 | 说明 |
|---|---:|---:|---:|---:|---|
| zh | 10 | 1.0000 | 1.0000 | 90 | |
| en | 10 | 1.0000 | 1.0000 | 90 | |
| code | 10 | 1.0000 | 1.0000 | 90 | |
| long_body（marker≈4.6k 字符） | 4 | 1.0000 | 1.0000 | 36 | semantic 单路 0.0（向量稀释），hybrid 靠 lexical 找回 |
| long_tail（marker≈20k 字符） | 4 | 0.0000 | 0.0000 | 40 | 16k 索引截断：lexical 不可达；bigram 向量同样被稀释 |
| near_dup（4 对副本，gold=两副本） | 8 | 1.0000 | 1.0000 | 64 | 两副本都返回、无去重丢失 |
| no_result（OOV 词） | 8 | —（无 gold） | — | 80 | 空结果率 0.0：见 §5 的「背景相似度 ≈0.40」 |

lexical 基线（floor 无关）：Recall@10 **0.9130**、MRR@10 0.9130、**误命中 0**、no-result 空结果率 **1.0**、session 级元数据命中 0。
（0.9130 = 46 条有 gold 查询里仅 long_tail 4 条在 16k 上限外。）

### 3.2 阈值扫描（hybrid / semantic，摘录；完整表见 `threshold-scan.json`）

| floor | hybrid Recall@10 | hybrid 误命中 | no-result 空率 | semantic Recall@10 |
|---:|---:|---:|---:|---:|
| 0.00 | 0.9130 | 410 | 0.0 | 0.8261 |
| 0.20（默认） | 0.9130 | 410 | 0.0 | 0.8261 |
| 0.35 | 0.9130 | 385 | 0.0 | 0.8261 |
| 0.40 | 0.9130 | 274 | 0.0 | 0.8261 |
| 0.45 | 0.9130 | 222 | **1.0** | 0.8261 |
| 0.50 | 0.9130 | 168 | 1.0 | 0.7609（zh -30pt） |
| 0.60 | 0.9130 | 43 | 1.0 | 0.5543 |

holdout 零召回损失区间 = **0.00–0.45**；0.50 起 semantic zh 掉召回。CLI 单次调用 p50 ≈ **49.0–57.9 ms**（`threshold-scan.json` 各 mode/floor 的进程级采样，含进程启动，仅供量级参考）。
注：本表「hybrid 误命中」与 `threshold-scan.json` 的 `false_hits_total` 同口径，只统计有 gold 的六桶（0.20 时 hybrid 410 = 90+90+90+36+40+64；semantic 414 = 90+90+90+40+40+64）；`no_result` 桶的 80 条命中另计，见 §3.1 与 `no_result_false_hits`。

## 4. frozen 100-query 回归交叉校验（`frozen-regression/`）

既有冻结集原数字（改动前，见 review-report P1-04）：lexical 0.750 / semantic 0.475 / hybrid 0.755（Recall@10）。

| floor | sem@5 | sem@10 | sem@20 | hyb@5 | hyb@10 | hyb@20 | lex@10 |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 0.00（防御基线） | 0.375 | 0.475 | 0.595 | 0.710 | 0.755 | 0.755 | 0.750 |
| 0.20（**默认**） | 0.375 | 0.475 | 0.595 | 0.710 | 0.755 | 0.755 | 0.750 |
| 0.25 | 0.375 | 0.475 | 0.595 | 0.710 | 0.755 | 0.755 | 0.750 |
| 0.30 | 0.370 | 0.465 | 0.575 | 0.710 | 0.755 | 0.755 | 0.750 |
| 0.35 | 0.315 | 0.400 | 0.505 | 0.710 | 0.755 | 0.755 | 0.750 |
| 0.40 | 0.280 | 0.360 | 0.465 | 0.710 | 0.750 | 0.755 | 0.750 |
| 0.45 | 0.265 | 0.340 | 0.410 | 0.705 | 0.750 | 0.755 | 0.750 |

- 默认 0.20 与原行为（0.00）在**所有**报告指标上逐值相等 → 回归零损失。
- 0.30 起 semantic 单路开始掉召回；0.45 的 -13.5pt（sem@10）即被拒绝的代价。
- 0.45 相对 0.00：54/100 条查询的 semantic top-10 命中集变化、27 条丢召回（`frozen-regression/` 逐查询明细可核；合计 13.5 条等价召回 = 0.475−0.340）；frozen 语料植入 40 对 paraphrase（`corpus.planted.paraphrase_pairs`），与「阈值一高就丢弱词法匹配」的形态一致（frozen 报告不落盘逐查询相似度，不做相似度区间断言）。

### 选择规则（写进 `threshold-scan.json.selection`）

1. holdout：保留「semantic 与 hybrid 的每个分桶 Recall@10 相对 floor=0.00 均无损」的 floor 集合；
2. frozen 回归：保留「semantic@10 与 hybrid@10 相对 floor=0.00 均无损」的集合（若提供报告目录）；
3. 取交集上界 0.25，退一格 0.05 余量 → **0.20**；若交集为空则 `floor=0.0`（仅防御路径）。
4. shipped-default 运行（不设环境变量）与 `floor=0.20` 扫描行**逐查询 hit 集完全一致**（脚本断言），常量另有 Rust 测试锚定。

## 5. 未决/残余风险（如实记录）

1. **fuzzy-lexical 的背景相似度 ≈0.40**：不相关短英文文本因字符 bigram 重叠仍得到 ~0.4 余弦；因此默认 0.20 **拦不住**这类「非零但无语义」的误命中（no-result 桶空率 0.0）。要压掉它们需要 0.45，但那会伤 frozen semantic 召回 → 本轮数据判定「不设激进阈值」。真正的修复是真实 embedding 模型 + 重新标定。
2. **long 消息向量稀释**：9k 字符消息的 marker 在 semantic 单路排名低于噪声 → semantic 长文桶 Recall 0；hybrid 靠 lexical 补齐。分块/聚合实验仍属后续任务（父 design §6 已列）。
3. **16k 索引上限**：long_tail 两路皆不可达，README 已补口径；不属缺陷。
4. **真实 E5 未标定**：无已验证 bundle；0.2 对 E5 尺度可能只起防御作用（不伤召回，也不保证质量门）。不得据此宣称语义晋级。
5. **holdout 是 remembered-phrase 口径**，不是 paraphrase 基准；paraphrase 质量由 frozen 回归（semantic@10 0.475）自证其弱。
6. README 只更新了「full-text 16k 限定 + 证据门」两处；语义/ranking 章节原有「hybrid 不重排」表述仍成立（证据门是准入过滤，不是重排）。
7. `search` 结果未新增 schema 键：v1.1 `searchData` 是 `additionalProperties:false` 且 `schemas/` 不在本任务写域；正名放在 `providers`/`model status`/`index embeddings`（`data:true`，additive）。若要进 search 元数据需走 schema minor 流程。

## 6. 证据文件

| 文件 | 内容 |
|---|---|
| `retrieval_holdout.py` | holdout 生成器 + 阈值扫描 + 选择规则（可重复运行；`--shipped-default` 与 `--frozen-regression-dir`） |
| `holdout-dataset.json` / `holdout-corpus/` | 六桶合成语料与查询 manifest（含 gold 记账与可达性契约） |
| `threshold-scan.json` | 全量扫描：每 floor 每 mode 的汇总 + 逐查询 hit 集 + 选择规则与结论 |
| `raw-runs.jsonl` | 全部检索调用的原始结果（floor/mode/query/hit 集/延迟/警告）；gold 可达性 `get` 校验在脚本内先于度量断言（结果不落盘） |
| `holdout-eval-final.log` | 最终运行的控制台摘要（最终 release 二进制 sha256 `70de42679a2b662a…`，`threshold-scan.json` 内为完整哈希） |
| `frozen-regression/summary.json` | frozen 100-query 回归在 0.00/0.20(as-shipped)/0.25/0.30/0.35/0.40/0.45 的指标汇总表 |
| `frozen-regression/semantic-benchmark-*.json` | 各 floor 的完整原始报告（含逐查询明细）；0.45 另有控制台日志 |
| `clippy.log` / `workspace-test-final.log` | 最终代码的 clippy(-D warnings) 与 workspace 测试原始日志 |

## 7. 测试锚点（新增/更新）

- application：`ranking::tests::final_score_zero_evidence_never_gains_boost`。
- adapters-sqlite：`semantic_evidence_gate_filters_below_floor_before_top_k`、`hybrid_rrf_cannot_admit_a_zero_similarity_semantic_candidate`、`semantic_evidence_floor_default_is_the_holdout_choice`；更新 `semantic_index_round_trips_and_ranks_by_cosine`（默认门生效断言）。
- CLI：e2e `semantic_evidence_floor_env_override_is_explicit_and_validated`（1.0→空、-1.0→恢复、非法值→invalid_request/exit 2）；更新 `providers.semantic` 键集断言（5 键）与 e2e 同源断言。
- 验证命令：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked --target-dir …target-b4 -- -D warnings`、`cargo test --workspace --locked --target-dir …target-b4`、`node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs`。