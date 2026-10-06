import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### Recall（35/37 full + 2 partial、13,244 行；receipt: \`research/coverage-Recall.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| R-01 | P1 | handoff 把全量 transcript 直塞 argv、无预算：无 token/byte 上限、无证据标记；超长会话触 OS 参数上限 | \`handoff.rs:20-25\`；\`transcript.rs:3-28\`；\`session.rs:651-660\`；\`session_action.rs:40-54\` |
| R-02 | P2 | sqlite-vec 的 k 截断先于过滤（CLI k=300 全局 KNN 后 JOIN 过滤；FTS 反而是前置过滤）→ 强过滤下向量召回漏失且不报错 | \`search.rs:158-175\` vs \`:133\` |
| R-03 | P2 | CJK 无分词：unicode61 使连续中文成为整串 token，查询按空白切词 OR 连接，无 bigram/trigram/fuzzy | \`schema.rs:80-85\`；\`search.rs:295-308\` |
| R-04 | P2 | 无孤儿对账：prune 默认空 → 源文件删除后索引永久陈旧 | \`sync.rs:196-246\`；\`adapters/mod.rs:40-42\` |
| R-05 | P2 | 嵌入失败无重试/崩溃恢复；processing 中途崩溃无接管；TUI 单次失败即进程内永久降级 | \`semantic_store.rs:73-95\`；\`semantic.rs:65-82\`；\`search_worker.rs:121-136\` |
| R-06 | P2 | RRF 等分无 tiebreak + offset 分页 | \`search.rs:291\`；\`session.rs:353-359\` |
| 正面 | — | 11 家 provider；任务导向动作+确认流；后台嵌入队列状态机；内置评测 harness（Hit@5/10+MRR）；单一 SearchEngine；imported 禁 resume/open 但允许 handoff 的边界 | \`bench.rs:305-360\`；\`session.rs:608-610\`；\`app.rs:3372-3390\` |

`;
s = s.replace(anchor, sec + anchor);
const tail = 'ASG 的投影/索引/导出是否共用同一截断版本需要自查。';
if (!s.includes(tail)) throw new Error('tail missing');
s = s.replace(tail, tail + 'Recall R-02 又把 SG-01 的“cap 先于过滤”扩展到了向量路径——ASG 若做 ANN/向量，必须同时证明「先过滤后 top-k」与过滤选择性阶梯。');
fs.writeFileSync(p, s, 'utf8');
console.log('Recall section added; length', s.length);
