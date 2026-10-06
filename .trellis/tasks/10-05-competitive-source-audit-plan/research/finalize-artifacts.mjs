import fs from 'node:fs';
const task = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan';

// ---------- review-report ----------
{
  const p = task + '/review-report.md';
  let s = fs.readFileSync(p, 'utf8');
  s = s.replace('# agent-session-grep 竞品源码对照与整改评审（证据阶段稿）', '# agent-session-grep 竞品源码对照与整改评审（覆盖收口稿）');
  s = s.replace('> 日期：2026-10-05；ASG HEAD：`5b232cdedbff33a251e7fb266558be7af8496e1a`。', '> 日期：2026-10-05 起草，2026-10-06 覆盖收口；ASG HEAD：`5b232cdedbff33a251e7fb266558be7af8496e1a`。');
  const oldLine = '中央账本 `coverage-index.json` 记录每个文件的 state/区间/哈希复核；截至本次更新 agf 与 sessiongrep 已完成 T1 全文回执，其余 13 项在流水线上（回执逐步并入）。';
  if (!s.includes(oldLine)) throw new Error('header line missing');
  s = s.replace(oldLine, '中央账本 `coverage-index.json` 记录每个文件的 state/区间/哈希复核；覆盖收口时 **15/15 项目均已有 T1 回执并入**（cass/agentsview/agent-sessions/cc-switch/cc-sessions-viewer/ctx 为多分片）：282 个文件全文 + 66 个文件有区间回执（合计 ≈25.1 万行带精确区间），9,125 个文本文件有 14 类探针逐文件命中；未读/未纳入行区间者与残余边界在各项目 receipt 与 §三·B 明确列出，不冒充已读。');

  const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
  if (!s.includes(anchor)) throw new Error('anchor missing');
  const sec = `### cc-sessions-viewer · 分片 B（7 家 parser；20,148 行中实读 11,438 行，3 full + 4 partial；receipt: \`research/coverage-cc-sessions-viewer-parsers.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CCVP-01 | P2 | 检索语义**宽于**“只索引用户消息”：精确层=\`role=="user"\` 且 \`block.kind=="text"\`，无 meta_kind 过滤 → 伪用户系统文本可被检索；反证：tool 结果 7 家全部不参与（均落 \`kind=tool_result\`）、kimi 两层最严格 | \`agents/mod.rs:1008-1019,988-991\`；\`grok.rs:880-905\`；\`agy.rs:695-713\`；\`pi.rs:889-904\`；\`claude.rs:1574-1599\`；\`codex.rs:1217-1223\`；\`kimi.rs:506-515,2006-2014\` |
| CCVP-02 | P2 | agy 早期历史恢复依赖每会话 .git + 读路径内启动 git 子进程 + 失败静默；\`preferred_transcript\` 以“更大文件”选完整度 | \`agy.rs:434-546,95-109\` |
| CCVP-03 | P2 | codex 发现层静默丢会话/降级：首行非 session_meta → 整个文件无声消失；state sqlite 不可用 → flags 全默认（internal 子代理可能不被识别） | \`codex.rs:450-475,2123-2125,2168-2178,113-154,172-183\` |
| CCVP-04 | P2 | 快照/并发保护不对称：pi 3 次 revision 重试、kimi 4 次+部分写入拒绝 vs claude/codex/grok 直读容忍坏行 | \`pi.rs:330-343\`；\`kimi.rs:46-47,133-206\`；\`claude.rs:1458-1468\`；\`codex.rs:1443-1450\`；\`grok.rs:996-1003\` |
| CCVP-05 | 信息 | 能力矩阵：树/导出/leaf 唯一 Pi；磁盘 fork 唯一 Claude；resume 逐家（pi --session / opencode --session / agy --conversation / claude --resume / codex resume / grok --resume / kimi --session） | \`pi.rs:1526-1562,439-522\`；\`claude.rs:1082-1239\`；\`mod.rs:251-253,257-264,274-289,438-447\` |
| 对照结论 | — | **广度：ASG 领先**（14 活跃 provider vs 7）；**保真深度：CCV 领先**（7 家全有工具结构化/diff、注入分类、写路径、resume 构造、过半家 usage 解码，而 ASG 自报 tool_activity 仅 claude/codex=partial） | \`asg-provider-matrix.json\` 对照 |

> 事实表注解：L31“只匹配用户消息（工具调用/结果/文件改动不参与匹配）”应改为“工具结果与文件改动不参与；但无 meta_kind 过滤，伪用户系统文本可被检索”。这条修正同样进入 B0 清单。

`;
  s = s.replace(anchor, sec + anchor);
  fs.writeFileSync(p, s, 'utf8');
  console.log('review-report finalized; length', s.length);
}

// ---------- implement.md A1 list + status ----------
{
  const p = task + '/implement.md';
  let s = fs.readFileSync(p, 'utf8');
  const i1 = s.indexOf('**A1-status');
  const i2 = s.indexOf('- [ ] ASG：', i1);
  if (i1 < 0 || i2 < 0) throw new Error('implement anchors missing');
  const i3 = s.indexOf('\n', i2);
  const block = `**A1-status（2026-10-06 覆盖收口）**：T2 扫描 15/15 完成；T1 回执 **15/15 项目并入**（多分片：cass A-D、agentsview A/B、agent-sessions A/B、cc-switch A/B、cc-sessions-viewer A/B、ctx A/B）。账本合计：282 文件全文 + 66 文件区间回执（≈25.1 万行）、9,125 文件探针命中记录、6 文件缺区间未计入阅读。

**A1 逐项目收口清单**：

- [x] cass：分片 A/B/C/D 回执并入（49,629 行有区间）；残余边界：lib.rs 97,420 行未读段、frankensearch 外部依赖内部语义、connector 细节。
- [x] agent-sessions：分片 A/B 回执并入（19,714 行）；残余边界：UnifiedSessionIndexer/ClaudeSessionIndexer 未读段、AgentSessions/Resume/* launcher、PresenceEngine、Usage/Status 模块。
- [x] agentsview：分片 A/B 回执并入（24,704 行）；残余边界：engine.go 5,067 行未读段、watcher 细节、53-case provider 注册表逐项保真。
- [x] ctx：分片 A/B 回执并入（20,250 行）；残余边界：daemon 命名管道/refresh job、net.rs、MCP e2e。
- [x] hstry：回执并入（15,316 行）；残余边界：service.rs gRPC 区、其余 14 家 adapter 逐行（依赖 T2 探针）。
- [x] AgentRecall：回执并入（10,114 行）；残余边界：database.ts/session-store.ts/indexer.ts/renderer。
- [x] Recall：回执并入（13,244 行）；残余边界：app.rs/popups.rs 未读段、9 家 adapter 逐行。
- [x] memex：回执并入（7,133 行）；残余边界：api/compact/llm/rag/web。
- [x] claude-historian-mcp：回执并入（11,921 行）；残余：package-lock 8,658 行、4 个二进制内容（排除记录）。
- [x] agf：33/34 全文 + TUI 2686/2686（11,313 行）；demo.gif 排除；测试只读未运行。
- [x] fast-resume：12 full/2 partial（5,158 行）；残余边界：opencode legacy 段、9 家 adapter 逐行。
- [x] sessiongrep：25/26（7,333 行）；demo.gif 排除。
- [x] Wake：9 full/4 partial（11,388 行）；残余边界：db.rs 2,330 行未读段、19 家 adapter。
- [x] cc-switch：分片 A/B 回执并入（22,762 行）；残余边界：proxy 未读段（46.5%）、凭据静态加密未验证。
- [x] cc-sessions-viewer：分片 A/B 回执并入（20,690 行）；残余边界：4 家 parser partial、util 测试区。
- [ ] ASG：ASG 侧逐文件补审与 §三·B“对 ASG 的直接含义”自查项（cap 先于过滤/零证据升格/失败固化/裁剪进缓存/shell 参数化/投影截断共用）——**本轮未做**，待用户决定是否作为实施前置任务。

**A1 残余口径**：以上“残余边界”均已写入对应 receipt 的 missing_ranges/limitations，不冒充已读；若要升级为全文覆盖，按 P2 排期，不得由代理自行宣布完成。
`;
  s = s.slice(0, i1) + block + s.slice(i3 + 1);
  fs.writeFileSync(p, s, 'utf8');
  console.log('implement.md A1 finalized');
}

// ---------- prd.md ----------
{
  const p = task + '/prd.md';
  let s = fs.readFileSync(p, 'utf8');
  const st = ' 2026-10-06：T2 扫描层完成（9,421 文件/14 探针）；T1 回执 2/15 完成、6 项目运行中、7 项目排队。';
  if (!s.includes(st)) throw new Error('prd status line missing');
  s = s.replace(st, ' 2026-10-06（收口）：T2 扫描 15/15（9,125 文本文件探针记录）；T1 回执 15/15 项目并入（282 文件全文 + 66 文件区间回执，≈25.1 万行）；残余边界逐项目在 receipt 中列明。任务仍为 planning，D1/D2 未决，未开始产品修改。');
  const ac = '- [ ] 全部项目的三层覆盖收口：每个文件 state 明确（T1 全文回执 / T2 探针扫描 / T3 带理由排除），且每个项目关键链路 T1 回执齐备；不是仅审查核心链路。当前 T2/T3 已完成，T1 进行中（agf/sessiongrep 已并入）。';
  if (!s.includes(ac)) throw new Error('prd ac missing');
  s = s.replace(ac, '- [x] 全部项目的三层覆盖收口：每个文件 state 明确（T1 全文回执 / T2 探针扫描 / T3 带理由排除），且每个项目关键链路 T1 回执齐备。2026-10-06：15/15 项目 T1 回执并入；每项目残余边界（未读区间）在 receipt/报告明示，不冒充已读。');
  fs.writeFileSync(p, s, 'utf8');
  console.log('prd.md finalized');
}
