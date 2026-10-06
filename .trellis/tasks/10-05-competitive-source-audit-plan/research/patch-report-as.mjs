import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### agent-sessions · 分片 A（T1 9,721/9,721 行 = 100%；receipt: \`research/coverage-agent-sessions-core.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AF-01 | P2 | 解析失败=成功且推进水位：Codex 读失败产出 1 个 error 事件仍算成功并可覆盖旧事件；Claude nil 被静默丢弃但水位照常前进 → 失败不再重试、无错误面（SG-03 教训的活例） | \`SessionIndexer.swift:2056-2060,910-947,1308-1324\`；\`ClaudeSessionParser.swift:115-120\`；\`SessionIndexingEngine.swift:143-165\`；\`ClaudeSessionIndexer.swift:412\` |
| AF-02 | P2 | 重索引窗口该 source 搜索/列表静默归零（guardrail 注释自认 silent zero；实测 5GB/3363 会话 ~149s）；保语料原语仍清 meta；两个历史标记整体清 FTS 语料 | \`DB.swift:388-396,1334-1383,406-427\` |
| AF-03 | P3 | sideChatsOnly 自由文本可被 FTS 截断饿死（限额提升只覆盖 repo/archived；FTS 先全库 bm25 序 LIMIT 2000） | \`SearchCoordinator.swift:735-742\`；\`DB.swift:1901-1909\` |
| AF-04 | P3 | FTS 降级无健康面（DDL 失败空 catch；查询 \`try? … ?? []\` 静默走 legacy） | \`DB.swift:325-327\`；\`SearchCoordinator.swift:319-329\` |
| AF-05~07 | P3 | 结果序无统一契约（bm25→toolIO→后台扫描→legacy 分段拼接）；删除/重算无事务；ID 冲突键不对称 | \`SearchCoordinator.swift:333-374,483-527,793-973\`；见回执 |
| 正面 | — | meta-only 水合→先发布→增量+gap 差集重扫→≥8MB tail-first 预绘；mtime+size+format_version 三元组“当前性”资格；stale 绕体积门槛；COALESCE 保留；rollups 最大余数法守恒+meta_mtime 增量 | \`SessionIndexer.swift:743-848,1326-1334,446-501\`；\`DB.swift:1791-1848,1633-1649,2318-2387\` |

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('agent-sessions section added; length', s.length);
