import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### cass · 分片 C（storage/indexer；sqlite.rs 6,610/30,047 + indexer/mod.rs 4,762/52,111 + semantic.rs 全 6,229；receipt: \`research/coverage-cass-storage.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CS-1 | P2 | “内容指纹”实为 \`content-v1:{会话数}:{max会话ID}:{max消息ID}\`（基数+ID）→ 保量改写不使 checkpoint 失效，可能跳过重建/续传错位 | \`indexer/mod.rs:8695-8747,2801-2808,14093-14128\`；\`semantic.rs:3156-3161,4920-4983\` |
| CS-2 | P2 | begin-concurrent 写者 \`foreign_keys=OFF\`（失败仅 debug）+ 孤儿清理 preflight 默认跳过（代码注释自认断连留孤儿） | \`indexer/mod.rs:26128-26135,25978-25984,13459-13501\`；\`sqlite.rs:4639-4656,5923-6222\` |
| CS-3 | P2 | in-DB FTS 影子写入 best-effort（错误吞掉仅 warn）；routine preflight 校验默认关闭 | \`sqlite.rs:14102-14236,11127-11167\`；\`indexer/mod.rs:13342-13419,14842-14911\` |
| CS-4 | P3 | 批量消息插入用 \`last_rowid-(n-1)\` 反推 message id，snippets/FTS 依赖推断 id；rowid 非连续会错绑 | \`sqlite.rs:13577-13586,13713-13722,10031-10050\` |
| CS-5 | P3 | \`forget_conversations_by_source_glob\` 拼 SQL（无绑定无上限）；\`delete_source\` 的 cascade 参数被忽略 | \`sqlite.rs:7723-7764,9833-9844\` |
| 正面 | — | staged swap+校验后才发布；“无法证明可恢复就拒绝原地重建”（FTS Unqueryable/Excess/Divergent fail-closed）；全局水位只在扫描成功且无排除时推进；OOM 二分删除+staging 残骸回收；重试白名单+串行 fallback；full rebuild 不 eager 删除 | 见回执 |

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('cass-C section added; length', s.length);
