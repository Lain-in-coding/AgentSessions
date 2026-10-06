import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### cass · 分片 B（检索管线；query.rs partial 6,525/20,986 + 3 文件全文；receipt: \`research/coverage-cass-query.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CQ-1 | P1（有界） | 过滤 vs cap：Tantivy 主路径过滤先于 cap；但 **SQLite 降级 cap 先于过滤**（1024/批、≤30k 补偿），message-scan 更是 \`LIMIT 30000\` 前缀截断再过滤，“过滤后非空但不足 limit”不重扫 → SG-01 同族、被 30k 窗口有界 | \`query.rs:6201-6218\`；\`6605-6628,6710-6747,6945-6962,7442-7478\` |
| CQ-2 | P2 | 零证据升格变体：无 boost（grep 0 命中），但语义 top-k 无相似度门槛 + RRF“名次即正分” → 低语义证据可入混合结果；文字路径必须有 MATCH 证据 | \`query.rs:4082-4096,4243-4401,1810-1867,1995,2018-2025\` |
| CQ-3 | P2 | 模式降级：默认 Hybrid fail-open→lexical（realized mode+reason 进 robot 元数据）；\`--mode semantic\` fail-closed（code 15）；tier 降级仅 debug 日志 | \`lib.rs:22726-22732,25211-25219,22860-22868\`；\`query.rs:4311-4314\` |
| CQ-4 | P2/信息 | 排序=BM25+RRF（联邦 k=60；混合走外部依赖 frankensearch 默认配置）；无 recency/path boost；过滤后才分页且有跨页测试；cursor 编解码未审 | \`query.rs:1598-1610,1861-1867,5591-5606,10337-10412\` |
| CQ-5 | P3 | 文档-行为漂移：\`MatchType/quality_factor\` 声称用于排序、实际不参与打分；wildcard 回退不降权 | \`query.rs:1117-1148,5714-5718\` |
| 正面 | — | 过滤 pushdown；降级分批扫描补偿；去重/过滤后分页+跨页测试；确定性 RRF tie-break；provenance/line_number/content_hash；fail-open 元数据化；manifest 原子发布+保守恢复+cleanup 指纹审批 | \`lexical_generation.rs\` 等 |

> 关键警告：cass 的核心排序/过滤语义（fs_rrf_fuse / fs_cass_build_tantivy_query / fs_candidate_count）在 **外部 git 依赖 frankensearch**（rev f7fa7a02，本机无 checkout）——可验证性打折。契约在内、排序在别人手里，这也定义了它“pack 已完成”说法的边界。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('cass-B section added; length', s.length);
