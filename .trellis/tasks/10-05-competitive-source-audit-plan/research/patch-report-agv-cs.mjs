import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### agentsview · 分片 A（5 文件 12,138/12,544 行=96.8%；receipt: \`research/coverage-agentsview-core.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AGV-01 | P2 | semantic/hybrid 候选 k 先于元数据过滤（取回后才按 allowed-session/Scope 过滤；hybrid FTS 腿 4×k 批上限自认 under-fill）→ SG-01 有界变体；词法模式过滤在 LIMIT 前（正面对照） | \`search_content.go:716-720,801-821,1043-1067\` |
| AGV-02 | P2 | Recall 每腿 500 截断先于 Rank；FTS→LIKE 静默回退，API 无 engine/degraded 标志 | \`recall.go:995-1007,612-636,855-866,1373-1385\` |
| AGV-03 | P2 | 侧栏索引无分页路径整表物化（仅 Limit>0/Cursor/Starred 才分页） | \`sessions.go:711-795\` |
| AGV-04 | P2 | 旧 Search OFFSET 深分页（每页重跑 FTS+name UNION）；会话列表已是 HMAC keyset | \`search.go:342-546\`；对照 \`sessions.go:401-464\` |
| AGV-05 | P3 | FTS 缺失失败语义三态不一致（静默容忍/显式 errFTSUnavailable/静默 LIKE）；判定靠错误串匹配 | \`db.go:3342-3347\`；\`search_content.go:617-634\`；\`recall.go:1380-1385\` |
| AGV-06..08 | P3 | sync_marker 坏 created_at 盲窗；MAX(id)+1 依赖单写者；跳过缓存全表重写 | \`db.go:2260-2270\`；\`messages.go:762-771\`；\`skipped.go:32-72\` |
| 正面 | — | user_version+行级 data_version“过期不盖章”；删除后写排除+幽灵防护；keyset+HMAC 游标与 sort 误配拒绝；FTS 触发器批量删重协议；删除日志 tombstone；失败分类学 | \`sessions.go:2341-2536,1221-1268,401-464\`；\`messages.go:1277-1313\` |

### cc-switch · 分片 A（会话管理/检索/用量；20 文件 11,675 行，T1 主文件 10/10 full；receipt: \`research/coverage-cc-switch-sessions.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CS-1 | P1（事实表） | **“无检索”被推翻**：存在 FlexSearch 元数据检索（字段 sessionId/title/summary/projectDir/sourcePath，tokenize=full，内存态不含正文） | \`src/hooks/useSessionSearch.ts:27-48\`；\`SessionManagerPage.tsx:229-237,997-1017\`；\`SessionItem.tsx:88-90\` |
| CS-2 | P2 | 检索是“会话说”不是证据级：结果只能打开会话；正文命中词需打开后高亮、不自动跳转命中；TOC 只跳 user 消息 | \`SessionMessageItem.tsx:39-48,89-93\`；\`SessionManagerPage.tsx:366-391\` |
| CS-3 | P2 | resume：5/7 家可构造（codex/claude/opencode/gemini/grok），实际拉起仅 macOS；其余复制命令；cwd 有单引号转义 | \`codex.rs:414\`；\`claude.rs:251\`；\`opencode.rs:476\`；\`gemini.rs:172\`；\`grok.rs:192\`；\`SessionManagerPage.tsx:418-427\`；\`terminal/mod.rs:328-338\` |
| CS-4 | P2 | 命令字符串直通 shell 已在源码记录为接受风险 | \`commands/session_manager.rs:27-60\` |
| CS-5 | P2/P3 | 静默截断/一致性：Hermes LIMIT 500、JSONL 浅扫、messages 固定列、JSONL 删除不校验 ID；Codex 递归无深度上限 | \`hermes.rs:84,291-307,488-496\`；\`codex.rs:504-522\` |
| CS-6 | P2 | 无持久索引、无 watcher，list_sessions 每次 7 线程全量重扫；UI 30s staleTime。加分：用量子系统游标+去重+重放防护+批事务 | \`mod.rs:58-94\`；\`commands/session_manager.rs:6-11\`；\`queries.ts:307-324\`；\`session_usage_codex.rs:1093-1125,1295-1346\` |

> 事实表修正（B0 直接输入）：L30/L49 的“无检索/不可比”应改为“元数据检索（FlexSearch 内存索引：无正文/无持久化/无 source span）+ 7 家会话管理与 resume 命令生成”；“不可比”限定为“证据级检索不可比”。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('agentsview+cc-switch sections added; length', s.length);
