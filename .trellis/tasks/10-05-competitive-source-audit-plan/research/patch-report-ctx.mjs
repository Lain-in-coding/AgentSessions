import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### ctx · 分片 A（capture/store/search packet；25 文件 11,333 行，7 个 T1 必读全 full；receipt: \`research/coverage-ctx-core.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CTX-01 | P2 | pagination 只写不读且三形状分裂：packet 生成 \`offset:{n}\`，但 query/main/MCP 均无 cursor/offset 输入；契约 fixture \`{limit}\` 与 JVM(limit/offset/nextCursor/hasMore) 对不上；>200 命中无取回路径 | \`packet.rs:114-123\`；\`query.rs:24-30\`；\`main.rs:317-434\`；\`search.results.json:85-87\`；\`SearchPagination.java:18-55\` |
| CTX-02 | P2 | 全量索引重建无事务快照：裸 DELETE+逐表重建、无外层事务；自愈只在投影计数为 0 时触发 → 半重建可持久、缺表时搜索静默空 | \`projections.rs:580-660,757-799,134-136\`；\`import.rs:830\`；\`import/native.rs:66,91\` |
| CTX-03 | P2 | citation 仅事件粒度：有 id/seq/cursor/raw path/exists，但无 span/内容哈希；snippet 来自索引期裁到 2048 字符的预览；完整 payload 不参与检索 | \`dtos.rs:742-760\`；\`projections.rs:1639-1666\`；\`results.rs:284\`；\`events.rs:526\` |
| CTX-04 | P2/P3 | withheld 三层不一致：枚举+过滤谓词存在，但 DDL CHECK 不含 withheld → 写不进；packet visibility 恒为 LocalOnly | \`sync.rs:9-18\`；\`projections.rs:1600-1622,1005-1020\`；\`ddl.rs:371-373\`；\`results.rs:195,304\` |
| CTX-05 | P3 | 批量半写：64 单位/8MiB 轮转提交，错误只回滚当前批；无“已提交到哪”的回执 | \`batches.rs:341-387,199-202,180-188,235-245\` |
| 正面 | — | citation 字段骨架（事件/会话双引用+外部 id+cursor+源存在性）；truncation reason 机器可读；indexed/last_imported 双水位与 pending SQL；错误分类；显式 retention 元数据 | 见回执 |

> 对 ASG handoff-pack 的底线（来自 ctx 教训）：证据必须带 span+强哈希并从权威 payload 渲染；cursor 必须有消费者和“翻页无重无漏”验收；有损归一化必须落 omission 元数据。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('ctx section added; length', s.length);
