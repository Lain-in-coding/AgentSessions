import fs from 'node:fs';
const task = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan';

// ---- implement.md A1-status refresh ----
{
  const p = task + '/implement.md';
  let s = fs.readFileSync(p, 'utf8');
  const i1 = s.indexOf('**A1-status');
  const i2 = s.indexOf('- [ ] cass：', i1);
  if (i1 < 0 || i2 < 0) throw new Error('implement anchors missing');
  const block = `**A1-status（2026-10-06 滚动更新）**：T2 扫描 15/15 完成；T1 回执（含并入状态）——

| 项目 | T2 | T1 回执 / 进度 |
|---|---|---|
| agf | ✅ | ✅ 33/34 + TUI 2686/2686（并入） |
| sessiongrep | ✅ | ✅ 25/26（并入） |
| fast-resume | ✅ | ✅ 12 full / 2 partial（并入） |
| cass | ✅ | ✅ 分片 A/B 并入；C（storage/indexer）、D（lib/sources）运行中 |
| claude-historian-mcp | ✅ | ✅ 41 full / 1 partial / 4 excluded（并入） |
| memex | ✅ | ✅ 9 full（并入） |
| hstry | ✅ | ✅ 29 full / 5 partial（并入） |
| Recall | ✅ | ✅ 35 full / 2 partial（并入） |
| ctx | ✅ | ✅ 分片 A（16 full / 9 partial）并入；B（semantic/MCP）待排 |
| AgentRecall | ✅ | ✅ 9 full（并入） |
| cc-switch | ✅ | ✅ 分片 A（17 full / 3 partial）并入；B（proxy/transform）运行中 |
| agentsview | ✅ | ✅ 分片 A 并入；B（sync/parser）运行中 |
| Wake | ✅ | ✅ 9 full / 4 partial（并入） |
| agent-sessions | ✅ | 🔄 分片 A 运行中；B（Views/恢复链）待排 |
| cc-sessions-viewer | ✅ | 🔄 分片 A 运行中；B（7 家 parser）待排 |

`;
  s = s.slice(0, i1) + block + s.slice(i2);
  fs.writeFileSync(p, s, 'utf8');
  console.log('implement status refreshed');
}

// ---- review-report: AgentRecall section ----
{
  const p = task + '/review-report.md';
  let s = fs.readFileSync(p, 'utf8');
  const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
  if (!s.includes(anchor)) throw new Error('report anchor missing');
  const sec = `### AgentRecall · 分片 A（9/9 full、10,114 行；receipt: \`research/coverage-AgentRecall-core.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AR-01 | P2 | MCP 与桌面检索**确为两套实现、两套排序**：MCP 独立 raw SQL \`ORDER BY rank\`（自认 reimplemented）；桌面为 FTS 闸门+JS smartScore；过滤/title 优先级/source 映射均不同 | \`bin/agent-recall-mcp.mjs:96-107,178-181\`；\`sessions.ts:1058-1093,1729-1737,1422-1425,1440-1447,1661\` |
| AR-02 | P2 | 桌面 query 路径全量 hydrate + JS 全量排序后截断（候选无 LIMIT） | \`sessions.ts:1407-1409,1079-1086\` |
| AR-03 | P2 | 元数据链路：标题经 IPC+FTS 刷新；收藏/置顶/隐藏纯 UPDATE；摘要以 \`file_mtime_ms\` 版本化 stale；**无“下一步”字段**（=resume 记 \`last_resumed_at\`） | \`sessions.ts:461-477,890-917,1698,668-670\` |
| AR-04 | 信息 | 存储真实实现：FTS5 **trigram** 表（unicode61→trigram 重建）、索引含全量消息+摘要 → 内容级全文检索；session_fts 非 contentless（正文双份存储） | \`schema.ts:184-191,304-322\`；\`sessions.ts:1132-1150\` |
| AR-05 | 信息 | 远程边界：远程与本地同库（environment_id），未 hydrate 时桌面直读 SSH；**MCP 无远程逻辑** → 未 hydrate 会话在 MCP 只见 0 条本地消息（推演，未运行验证） | \`main/index.ts:463-486,1206-1218\` |
| 形态修正 | B0 | “Python CLI”错误 → TypeScript/Electron/React 桌面 + node:sqlite（Node ≥22.13）+ 独立 Node stdio MCP server；全仓 0 个 .py（Python 仅出现在可选运行时助手） | \`package.json\`；\`main/index.ts:1559-1562\`；\`session-loader.ts:24-25\` |

`;
  s = s.replace(anchor, sec + anchor);
  fs.writeFileSync(p, s, 'utf8');
  console.log('AgentRecall section added; length', s.length);
}
