import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### Wake（13 文件 11,388 行；核心 6 件全 full + db.rs 选区 31.6%；receipt: \`research/coverage-Wake.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| WK-01 | P2 | 任意 <3 码点词项使整条查询退化为全表 LIKE（含常见 CJK 双字）；degraded 路径丢 bm25 | \`db.rs:3259-3262,2618-2688,2250-2258,1121-1122\`；反证 \`mcp/tools.rs:658-681\` |
| WK-02 | P2 | ≥3 码点 trigram 零命中无 LIKE/提示兜底；fts_reindex 旗一轮后无条件清 → 重索引失败窗口静默空结果 | \`db.rs:2565-2571,2618-2653\`；\`scanner.rs:328-333,625-630\` |
| WK-03 | P2 | 新鲜度仅 mtime+size，无内容哈希（sidecar 有独立通道） | \`scanner.rs:461-464,485-488,447-451\` |
| WK-04 | P2 | 解析/写库失败仅 eprintln，MCP/CLI 无健康度面；index_note 只报新鲜度 | \`scanner.rs:558,562,594-597,1153\`；\`mcp/tools.rs:471-480\` |
| WK-05 | P3 | 消息身份只有 seq、索引文本 32KiB 截断；无原生消息 id/父边/字节锚 | \`models.rs:324-340,372-380,712-720\` |
| 正面 | — | 过滤在 FTS SQL 内 LIMIT 前完成；\`None≠Some(空)\` 冻结纪律；写事务内副本裁决+last-good；删除三段式（无链接检查→revalidate→sha256 journal→trash→内容证据恢复+墓碑）；watcher 溢出 rescan；CLI↔MCP 双射契约测试 | \`db.rs:2583-2598\`；\`scanner.rs:389-423,546-605,642-755\`；\`cleanup.rs:291-545,810-1028\`；\`watcher.rs:114-200\`；\`cli.rs:959-1012\` |

> Wake 是目前竞品中“防错纪律”最接近 ASG 方向的一家（过滤前置、None 语义、last-good、删除内容证据）；弱点在短词全表 LIKE 与 seq-only 身份。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('Wake section added; length', s.length);
