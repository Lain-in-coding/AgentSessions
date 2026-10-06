# cass 检索管线与排序审计（分片 B · T1 回执）

- 对象：`C:\AgentSessions\Github_src\coding_agent_session_search`（cass），commit `aa92a45311e10a3ac9c40c3730664a7366af2fe8`（`git log -1` 核对；工作树干净，仅未跟踪 `.codegraph/` 本地索引，未做任何 git 写操作）。
- 范围：`src/search/query.rs`（partial，9 个指定子系统全覆盖）、`src/search/two_tier_search.rs`（全文）、`src/search/policy.rs`（全文）、`src/indexer/lexical_generation.rs`（全文）；辅助有界读 `src/lib.rs` 三段（模式降级/robot 元数据，见覆盖 JSON）；T2 机械扫描按需过滤 `sweep-coding_agent_session_search.json`。
- 口径：全部为静态源码推导（本轮禁止 build/test/run），行号均为 1-based；结论标注可达性假设。覆盖统计与逐文件 read_ranges/missing_ranges 见 `research/coverage-cass-query.json`。
- 覆盖概览：query.rs 6,525/20,986 行（其余为测试主体与未读 helper，见 missing_ranges）；two_tier_search.rs 1,364/1,364；policy.rs 1,598/1,598；lexical_generation.rs 4,066/4,066。四文件 SHA-256 读前后一致。

## 一、结论速览（Top-5）

| ID | 级别 | 一句话结论 | 关键锚点 |
|---|---|---|---|
| CQ-1 | P1（有界可达） | 过滤先于候选 cap 在主 Tantivy 路径成立；但 SQLite 降级路径是"先 cap 后过滤 + 30k 扫描补偿"，message-scan 兜底更是"先取前 30k 行再过滤"，存在 SG-01 同族的域内命中被挤出/少返的可达路径 | `src/search/query.rs:6605-6628,6710-6747,7171-7184,7442-7478,6945-6962` |
| CQ-2 | P2 | 没有 recency/path boost；但语义候选无相似度门槛、混合 RRF 按名次给所有进入候选的命中正分，弱语义近邻可被升格为"相关"（与 SG-02 机制不同、方向相同） | `src/search/query.rs:4082-4096,4243-4401,1810-1867,1999,2018-2025` |
| CQ-3 | P2 | 默认 Hybrid 对语义不可用 fail-open 到 lexical，且 realized mode + fallback reason 进 robot 元数据（可感知）；`--mode semantic` fail-closed（code 15 + hint）。但 tier 级降级（two-tier→exact）只写 debug 日志，不进结果元数据 | `src/lib.rs:22726-22732,22851-22890,23085-23095,25211-25219,25444-25447`；`src/search/query.rs:4311-4314` |
| CQ-4 | P2/信息 | 排序=BM25（词法）+ RRF 名次融合（k=60），无权重/无近因/无路径 boost；分页"fetch from 0 → 全局去重/过滤 → skip/take"，并有窗口补偿与测试；cursor 字符串编解码在 lib.rs（未审计） | `src/search/query.rs:1598-1610,1861-1867,1995,2018-2025,3561-3642,5537-5567,5591-5606,10337-10412,10785-10830` |
| CQ-5 | P3 | `match_type/quality_factor` 注释声称用于排序，实际只用于展示性 dominant match_type；wildcard/implicit-wildcard 命中不因质量因子降权——文档-行为漂移 | `src/search/query.rs:1117-1148,3123-3140,5714-5718` |

## 二、逐项发现（证据 / 可达 / 反证 / ASG 教训）

### CQ-1 · P1（有界可达）· 候选截断与过滤次序：主路径正确，降级路径有 SG-01 同族缺口

**证据（次序链条）**
1. 主 Tantivy 路径：`agents/workspaces/created_from/created_to/source_filter` 被翻译为 `FsCassQueryFilters` 并送入 `fs_cass_build_tantivy_query(raw_query, &fs_filters, fields)`（`query.rs:6201-6218`），即过滤进入检索式、发生在 `TopDocs` cap 之前；`session_paths` 因 `source_path` 只 stored 不 indexed 而明确走后过滤（注释 `6216-6217`），实际实现在 `postprocess_hits_page`（`5599-5606`）。
2. 为抵消 session_paths 后过滤，`search()` 采用 "fetch from 0 + 自适应放大 + 饱和重试"：`fallback_fetch_limit` 在 session_paths 激活时放大到 `total_docs.min(no_limit_result_cap())`（`3572-3580`）；仅在 `initial_hit_count == initial_fetch_limit`（翻页饱和）且去重后不足目标时重试（`3612-3642`；联邦版 `3689-3724`）。
3. SQLite FTS5 降级路径：排名 SQL 不携带任何过滤（函数签名即 `_filters`，`6605-6628`），过滤在行 hydrate 之后（`sqlite_fts5_filters_need_post_hydration` `6710-6717`；`sqlite_fts5_hit_matches_filters` `6719-6767`）。补偿手段是分批循环：`target_hits=offset+limit`、每批 `clamp(1,1024)`（`7177-7184`），逐批累计命中直到 `>=target_hits`、批次不满或 `scanned_rows>=30000`（`7442-7461`；常量 `118-122`：batch 1024 / scan 30000）。
4. message-scan 兜底（FTS 缺失/不可用/完整性失败，或 FTS5 过滤后为 0）：SQL 为 `ORDER BY m.id LIMIT ?`、参数恒为 30000（`6945-6946,6960`），行读入后在 Rust 里评分（`6881-6908`）并逐行做同一套过滤（`7072-7074`），再 `skip/take`（`7084-7088`）。

**可达路径（静态推导）**
- 仅 SQLite 后端（无 Tantivy 索引）时使用过滤（provider/agent/workspace/since/source/session_paths）且库大于 30k 行：若排名前 30k 行内"域外"命中占满窗口，域内命中可不足（返回少于 limit 的页）或为 0（转 message-scan 再取"最早 30k 条 message"，仍可能为 0）。
- 注意两个缺口点：(a) 循环在 `hits.len() >= target_hits` 之外的出口包括 `scanned_rows >= 30000`（`7453-7461`）；(b) 过滤后**非空但不足** limit 时不触发 message-scan 重扫（`7463-7478` 只在 `hits.is_empty()` 时兜底）。因此 SG-01 式"过滤后返回 0 而域内仍有命中"在 sqlite 降级路径下有界可达（窗口 30k），"页不满"更容易复现。
**反证/界限**
- Tantivy 存在时零命中不回退 SQLite（`3657-3661`，明示 authoritative），主路径的 cap 之前已含全部行内过滤（除 session_paths，且有饱和重试与全量上限补偿）；SQLite 的 30k 窗口远大于 sessiongrep 的 limit×5；FTS5 完整性/列探测失败会走 message-scan 而不是静默空（`7136-7168`）；session_paths 的语义侧同样有"越过初始候选继续找"的测试（`18257-18321`）。
- 未做运行时复现：需要构造 >30k 匹配行且排名次序不利的库；本轮仅静态可达性判断。

**ASG 教训**：过滤必须在 cap 之前完成（或提供"域外高排名 > cap、域内仍有命中"的 fixture 验收）；若保留降级后端，其"固定扫描上限 + 后过滤"必须在契约里显式声明为有损，并禁止把"页不满/为空"当作无更多结果。

### CQ-2 · P2 · 零文字/零语义证据能否被"升格"：没有 boost，但有"名次即证据"的升格面

**证据**
- 无权重 boost：全文 grep `boost` 在 query.rs 0 命中；`MatchType::quality_factor()` 只在 `dominant_match_type`（`3123-3140`）与测试中使用，不进入任何分数运算（也见 CQ-5）。
- 语义候选无相关性阈值：`collapse_semantic_results` 只做"按 message 合并取最高分 + 排序 + 截断"（`4082-4096`）；exact/ANN/two-tier 三条路径（`4150-4241,4243-4401`）都返回 top-k，没有最低相似度、没有"空文本证据"检查（内容在 hydrate 阶段才取，`4895` 起）。
- 混合融合按名次给分：`rrf_fuse_hits` 把词法/语义两路的每个命中转换为 `FsScoredResult/FsVectorHit` 后交给 `fs_rrf_fuse(..., FsRrfConfig::default())`（`1861-1867`），同 key 命中累加 RRF 分（`1995`），最终按分数 + `SearchHitKey` 稳定排序（`2018-2025`）。任何进入候选的命中都得到 `1/(k+rank)` 量级的正分——**弱语义近邻即使文本证据为零（message 存在但内容空）也能以正分出现在混合结果**。
- message-scan 的分数是词频计数（`6881-6908`），命中判定基于 normalize 后子串匹配、haystack 含 title/agent/workspace/source_path（`7003-7021`）：一个宽泛词可给大量行正分。
**反证/界限**
- 纯词法路径的命中必来自 Tantivy/FTS `MATCH`（含 wildcard fallback 的 `*term*`），不是无条件引入；`hit_is_noise` 会先剔除工具调用噪声（`3187-3202,3253-3256`），且"零文本"在投影场景被显式放行而不是升格（`3198-3200` 的注释：字段裁剪 ≠ 空行）；RRF 合并同 key 只是把"两路都被检索到"的证据相加，仍需先被检索到。
- 与 sessiongrep SG-02（recency/repo 无条件加分）不同：cass 没有时间/路径加分；被质疑的只是"语义 top-k 无下限 + RRF 名次分"这一更窄的升格面。
**ASG 教训**：文字/语义证据决定"能否入选"，recency/path 只影响合格结果排序；若做语义，给相似度下限或至少在结果里显式暴露相似度/证据类型。

### CQ-3 · P2 · 模式降级/模型缺失：默认 fail-open 且元数据可感知；tier 级降级静默

**证据**
- 契约层：`policy.rs:11-34`（Lexical 永远可用；语义机会性增强）、`56-103`（HybridPreferred 默认 / LexicalOnly / StrictSemantic 三模式，`requires_semantic` 仅 Strict=true）。
- CLI 层：Hybrid 是"preference 而非硬依赖"，语义资产不可用时 `mode_meta.fall_back_to_lexical(...)`（`lib.rs:22726-22732,22857-22859,22880-22881,23085-23095`）；`fall_back_to_lexical` 设置 `realized=Lexical + fallback_tier + fallback_reason`（`25211-25219`），并写入 robot 元数据 `fallback_tier/fallback_reason` 与 typed reason（`25444-25447`）——**用户可感知**；Tantivy 缺失另有 stderr 警告（`22717-22724`）。显式 `--mode semantic` 无资产 → fail-closed，CliError code 15 + 安装 hint（`22860-22868`；HNSW 缺失 → 专门 hint，`23024-23035`）。
- tier 级：two-tier 索引不可用 → "falling back to exact single-tier search" 仅 `tracing::debug`（`query.rs:4311-4314,4317-4323`），且失败被按 tier 缓存（`2110-2135,3984-4032`）避免反复尝试；ANN 近似不可用 → 硬错误 + 修复 hint（`4049-4053,4334-4336`）；progressive 上下文缺失 → `progressive two-tier context unavailable` 硬错误（`5240-5242`）；progressive 细化失败以 `RefinementFailed{latency,error}` 事件上报（`5377-5382`）。
**反证/界限**：tier 降级不改变"结果仍然正确（exact 单层）"这一语义，只是质量层级下降；CLI 入口对 tier 标志仅在有语义模式时才生效并另有 stderr 提示（`22734-22738`）。真正静默的是"结果质量层级"而非"检索失败"。
**ASG 教训**：fail-open 可以，但 realized/fallback 必须进结构化输出（cass 做到了）；质量层级降级（fast→exact）也应进同一处元数据，而不是只在 debug 日志。

### CQ-4 · P2/信息 · 排序公式、分页与 cursor 一致性

**排序公式（cass 自持部分）**
- 主排序：Tantivy BM25（`6221,6228-6229`，打分在 `TopDocs::order_by_score`）；SQLite FTS5 用 `bm25(fts_messages)` 升序 + `rowid` 次序（`6614-6625`），入库前取负转为"越大越好"（`7427`）。
- 联邦分片：RRF `1/(60+rank+1)`（`1598-1610`），合并排序 `fused_score` ↓ → `shard_rank` → `SearchHitKey` → `shard_index`（`1612-1627`）。
- 混合：词法+语义交 `fs_rrf_fuse`（默认配置），随后 cass 侧全局去重（同 key 合并、`score += rrf`）再排序分页（`1810-2026`）。**无 recency/path boost、无权重向量**；`quality_weight=0.7` 只属于 two-tier 语义内部的 fast/quality 混合（`two_tier_search.rs:51-93,735-767`；`policy.rs:165`），不作用于最终混合排序。
- 一致性问题（观测）：`fs_rrf_fuse/FsRrfConfig::default()/fs_candidate_count` 由外部 `frankensearch`（git rev `f7fa7a02`，`Cargo.toml:91`）实现，本仓库无法验证其权重/k/双列表加成；cass 侧只保证"调用默认配置 + 自己去重/切片"。
**分页一致性**
- 统一从 0 取数（`3598,3783`），去重/噪声/`session_paths` 过滤后再 `skip(offset).take(limit)`（`5591-5606`）；语义侧同样先过滤后 offset（`5513-5516`），并带"窗口可能漏掉竞争者"检测与重试（`4098-4126,5537-5567`）。
- 证据测试：跨页去重（`10337-10412`，两页返回不同 source_path）、offset 跳过（`10785-10830`）、语义 offset 在过滤之后（`18290-18321`）。
- cursor：SearchClient 只认 offset；CLI 的 cursor 分页靠"多取 1 条"（`search_limit=limit+1`）与 token-budget 页大小在 lib.rs 组装（`lib.rs:22921-22948`，T1 边界外，仅定位）；cursor 串编解码未审计。
**残余**：`browse_by_date` 所有命中 `score=0.0`、`MatchType::Exact`（`7639,7646`），按 `created_at` 排序——属显式设计（注释 `7481-7486`），不参与 BM25 比较。

### CQ-5 · P3 · `match_type/quality_factor` 文档-行为漂移，wildcard 回退不降权

- `MatchType` 注释写"Used for ranking: exact matches rank higher than wildcard matches"（`1117-1148`），`quality_factor` 提供 0.6~1.0 六档；但实际上：函数只在 `dominant_match_type` 里被调用来选"最差的匹配类型"用于展示（`3123-3140`），评分链路（tantivy/sqlite/RRF）从未乘 quality factor。
- wildcard 自动回退命中被标记 `MatchType::ImplicitWildcard`（`5714-5718`），但排序仍按 `*term*` 命中的 BM25 与其他页混排——不降权意味着宽泛回退可能压过精确命中（没有显式 demotion 证据）。
- 反证：`search_with_fallback` 只在"回退命中数严格多于原结果"时才采用回退（`5714-5724`），且 `offset!=0` 时禁用回退（`3321-3323`）；影响面限于首屏、稀疏场景。
- ASG 教训：要么把质量因子真正施加到排序（并在测试中固化），要么把注释改成"展示用途"，不要让文档承诺排序语义。

### CQ-6 · P3 · 遗留 two-tier 模块的吞错与"全等分数=1.0"归一化

- `two_tier_search.rs`（仍在 `src/search/`，与 query.rs 的 frankensearch 路径并存）：`search_fast/search_quality` 把引擎错误吞成空 Vec 仅 warn（`407-412,459-463`）；`quality_scores_for_indices` 失败时按位置补 0.0（`489-498`）→ 细化打分为零、排序静默退化为 fast 排序。
- `normalize_scores`：全等分数（含全 0）归一化为全 1.0（`824-830`，测试 `1112-1120`）——若参与 blend 且质量分为 0 集合，会把 fast 分整体抬到 1.0；在 `quality_only` 路径 0 分向量经 `search_quality` 排序仍全部返回。
- 正面：细化失败以 `SearchPhase::RefinementFailed` 上报（`668-671,693-699,706-712,778-780`），且单测覆盖"fast embed 失败/daemon 不可用/quality-only 无 daemon"（`1252-1322`）。
- 边界：query.rs 的生产混合路径使用 `frankensearch` 的 `FsTwoTierSearcher`（`4264-4269`）而非本模块的 `TwoTierSearcher`；本模块是否仍被产品路径引用未在本轮确认（残余）。

### CQ-7 · P3 · 文档/静默细节三则

- 死引用：`query.rs:6216-6217` 注释指向 `apply_session_paths_filter()`，全仓库 grep 只有该注释（无此函数）；实际过滤在 `postprocess_hits_page`（`5600-5602`）。
- progressive 词法适配器对"无法解析消息身份"的命中静默丢弃（`2469-2473` 的 `let Some(...) else { continue }`），只把可解析命中写入缓存（`2474-2499`）；失败以整批 `SubsystemError` 冒泡（`2454-2465`），单命中解析失败无声。
- Tantivy 内容 hydrate 在 SQLite guard 打不开时返回空 map（`6034-6040`），命中退化为 preview 内容（`6393-6406`）——对"证据完整性"是软失败（snippet/content 可能只有 preview），无结果级标记。

### 正面证据（必须保留）

- 过滤 pushdown（Tantivy）与饱和重试（`6201-6218,3572-3580,3612-3642`）；去重后分页 + 跨页测试（`5591-5606,10337-10412`）；RRF 确定性 tie-break（`1598-1627,2018-2022`）；空结果建议且零命中时不懒开 SQLite（`5900-5925`，测试 `15283+`）；`no_limit_result_cap` 对 0-limit 的动态上限（`445-587`）。
- 索引侧生成契约：manifest 原子写入（tmp+rename+fsync，`lexical_generation.rs:1696-1745`）、拒绝未来版本（`1796-1802`）、保守恢复（隔离/失败不得进入服务面，`1304-1398`）、cleanup dry-run + 操作者批准 + 指纹校验（`571-659,899-997`）、8 种处置的 protected/reclaimable 划分有 golden 测试（`3485-3602`）。
- 语义策略契约：默认可重建、词法永远可用、StrictSemantic 才 fail-closed（`policy.rs:11-34,95-103`）；失效判定区分软重建/硬重建/驱逐（`791-842`）；预算与逐出顺序显式（`877-909,963-991`）。

## 三、特别追查问答（直接回答）

1. **过滤 vs 候选 cap**：主 Tantivy 路径=过滤先于 cap（行内过滤 pushdown；session_paths 是受控例外，带饱和重试与全量上限补偿）。SQLite FTS5 降级路径=cap 先于过滤 + 1024/批、≤30k ranked-row 的循环补偿；message-scan 兜底=先 `ORDER BY m.id LIMIT 30000` 再过滤。**"域外高排名挤掉域内命中"在 sqlite-only + 过滤 + 大库条件下可达且被 30k 窗口限制**；"过滤后为空但域内仍有命中"也在此条件下可达（窗口外），页不满更易复现。对照 SG-01：结构同族，但 cass 有分批扫描补偿与 Tantivy 主路径差异，不能等同描述为"全局截断先于过滤"。
2. **零文字/零语义证据能否被 boost 升格**：没有 boost（无 recency/path/权重加分，grep 实证）。但语义 top-k 无相似度门槛 + 混合 RRF 的"名次即正分"，使弱相关（含零文字内容）的语义近邻可入选混合结果；文字路径必须有 MATCH 证据。结论：**"零语义证据"不存在（入选即相似度排序），但"低语义证据"可以被升格，且没有任何阈值/标注阻止**；与 SG-02 机制不同、风险面更窄。
3. **模式降级/模型缺失**：默认 Hybrid fail-open → lexical，realized mode/fallback reason 进 robot 元数据与 typed reason（可感知）；`--mode semantic` fail-closed（code 15 + install/HNSW hint）；tier 降级（two-tier→exact）与 in-memory 索引不可用是静默（debug 日志+按 tier 缓存），不进结果元数据；progressive 细化失败有事件上报。
4. **排序公式**：BM25（词法）与语义相似度各自排序 → RRF（k=60 联邦；混合用 frankensearch 默认配置）→ cass 侧全局去重（同 key 合并累加）→ 分数+稳定 key 排序 → skip/take。无权重向量、无 recency/path boost（`quality_weight=0.7` 仅在 two-tier 内部 fast/quality 混合）。分页一致性：所有引擎 fetch 从 0 开始，offset 在去重/过滤之后应用，并有跨页/语义过滤 offset 的测试；cursor 字符串与 CLI overfetch 在 lib.rs 范围外。
5. **与 ASG 检索对照**：
   - **强于 sessiongrep 之处（该学）**：过滤 pushdown（而非 limit×5 后过滤）；降级路径有分批扫描补偿（不是一次性 cap）；分页在去重/过滤之后且有专项测试；确定性 RRF tie-break；结果带 provenance/line_number/content_hash、空结果有建议与 hint；fail-open 元数据化（realized/fallback reason）。
   - **弱于/不可照抄之处（不该学）**：SQLite 降级的 30k 扫描窗口 + message-scan 的 30k 前缀先截断；语义无相关性门槛 + RRF 名次分升格；tier 降级静默；quality_factor 文档-行为漂移；核心排序/过滤语义藏在外部依赖（frankensearch/frankensqlite）导致审计与复现边界不清。
   - **对 ASG 的具体动作**：(a) 把"过滤在 cap 前"写成可测契约并用"域外高排名 > cap、域内仍有命中"fixture 验收；(b) 若保留降级后端，扫描上限与"页不满=可能有更多"必须进输出元数据；(c) 语义结果显式暴露相似度/证据类型或设下限；(d) recency/repo 只排序不入选（ASG 已有 filter-before-topk 与 snapshot/generation 机制，本轮未做 ASG 侧验证——沿用 review-report §三·B 口径）。

## 四、残余未决（Open Items）

1. **外部依赖不可读**：`fs_rrf_fuse/FsRrfConfig::default()`、`fs_candidate_count`、`fs_cass_build_tantivy_query` 的过滤子句、BM25 字段权重、two-tier 融合细节、snippet generator 均在 git 依赖 `frankensearch`（rev `f7fa7a02`），本机无 checkout（`Cargo.toml:91`）。CQ-2/CQ-4 中涉及这些函数内部语义的部分只能确认"调用点与调用参数"，不能确认其内部行为。
2. **query.rs 未读区间**（missing_ranges 已逐条记录）：含 `open/open_with_options`（3363-3459）、semantic context setters、`resolve_semantic_doc_ids_for_hits`/`hydrate_semantic_hits_with_ids` 主体（4680-5113）、QueryCache/缓存内部（2040-2093,2528-3122,7935-8263,8384-10336）与测试主体（10905-18256,18327-20986）。任何结论若依赖这些区间均已标注为未验证。
3. **lib.rs 边界**：只审计了三段辅助区间；cursor 串编解码、TUI/MCP 调用侧、`--robot` 输出组装在范围外。
4. **无运行时证据**：全部结论为静态推导；CQ-1 的"需 >30k 匹配行 + 不利排名"可达路径未运行复现（禁止 build/test）。
5. **two_tier_search.rs 的引用面未确认**：本模块 `TwoTierSearcher` 与 query.rs 使用的 frankensearch searcher 并存，产品路径实际引用未见（仅类型/常量可能被使用）；未做全仓库调用图确认。
6. **lexical_generation 契约 vs 实际重建接线**：本轮只审计契约与测试；「哪个 generation 真正被 search 读取」的接线在 `src/indexer/*` 其他文件与 lib.rs，未审计。

## 五、覆盖与验证说明

- 读取方式：`codegraph node --file <path> --offset <start> --limit <=120`（1-based 行号），单次 ≤120 行；两处因批量输出上限被中段截断的范围（query.rs 589-599/645-679）已单独重读补齐；`src/lib.rs` 因超出 codegraph 索引规模用 PowerShell 行号读（≤120 行窗口）。
- 校验：读前/读后两次 SHA-256 一致（query.rs `fc10f18b…`、two_tier_search.rs `9026aa66…`、policy.rs `49b6452c…`、lexical_generation.rs `18fe6f73…`）；query.rs 读+缺=20,986 行闭合校验通过（脚本内断言）。
- 机械扫描：T2 sweep 四文件 probe hits 已并入 `coverage-cass-query.json` 的 `sweep_probe_hits`；本轮未重跑扫描、未改任何既有研究文件。
- 本文件与 `coverage-cass-query.json` 为本 worker 唯一创建物；`coverage-cass-pack.json`（分片 A）仅读取参考。