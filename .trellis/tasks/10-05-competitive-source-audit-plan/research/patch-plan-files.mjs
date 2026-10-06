import fs from 'node:fs';
const task = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan';
const assert = (s, needle) => { if (!s.includes(needle)) throw new Error('missing anchor: ' + needle.slice(0, 50)); };

// ---- implement.md ----
{
  const p = task + '/implement.md';
  let s = fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, '');
  const a1 = 'D1 journal 保留合同和 D2 发布次序未决，完整审读未完成，禁止 `task.py start` / 自动修改产品 / 自动提交。';
  assert(s, a1);
  s = s.replace(a1, a1 + '\n\n**执行态（2026-10-06）**：T2 机械扫描层完成——14 类探针对全部非二进制文件逐文件扫描（9,421 个已扫描，命中计数在 `research/sweep-*.json`，中央账本 `coverage-index.json` 已写回逐文件 state）。T1 全文回执流水线：agf、sessiongrep 已完成并入；claude-historian-mcp、cass-A（pack/资产）、fast-resume、hstry、memex、Recall 进行中；cass-B（检索管线）、Wake 已排队（并发上限 6）。T3 排除项（二进制/超大非源文件）已记录原因与哈希。');

  const a0 = '- [ ] 为每个第一方源码、测试、文档、安装/发布配置记录：完整读过 / 已读范围 / 尚未读 / 非产品生成物及理由。';
  assert(s, a0);
  s = s.replace(a0, '- [x] 三层覆盖账本落地：`coverage-index.json` 逐文件 state（T1 回执区间 / T2 探针命中数 / T3 排除+理由+哈希）；脚本 `research/sweep-probes.mjs`、`research/merge-coverage.ps1`、`research/merge-sweep-states.mjs` 可复跑。剩余：为 13 个项目补 T1 全文回执。');

  const hdr = '所有项目仍在范围中：';
  assert(s, hdr);
  const table = hdr + `

**A1-status（2026-10-06 执行态）**：T2 扫描 15/15 完成；T1 回执如下——

| 项目 | T2 探针 | T1 回执 / 进度 |
|---|---|---|
| agf | ✅ | ✅ 33/34 全文 + TUI 2686/2686；回执已并入账本 |
| sessiongrep | ✅ | ✅ 25/26 全文（demo.gif 排除；Cargo.lock 全读） |
| claude-historian-mcp | ✅ | 🔄 worker 运行中 |
| cass | ✅ | 🔄 A: pack/asset_state（运行中）；B: query/lexical/two-tier/policy（排队）；C: storage/indexer；D: lib 脊柱 + sources/UI |
| fast-resume | ✅ | 🔄 worker 运行中 |
| hstry | ✅ | 🔄 worker 运行中 |
| memex | ✅ | 🔄 worker 运行中 |
| Recall | ✅ | 🔄 worker 运行中 |
| Wake | ✅ | ⏳ 排队 |
| cc-switch | ✅ | ⏳ 待排（拆 2 片：proxy/transform + session/数据库） |
| agentsview | ✅ | ⏳ 待排（Go core：parser/db/sync/server 分片） |
| agent-sessions | ✅ | ⏳ 待排（Swift 核心/索引/搜索分片） |
| ctx | ✅ | ⏳ 待排（capture/store/search 分片） |
| AgentRecall | ✅ | ⏳ 待排 |
| cc-sessions-viewer | ✅ | ⏳ 待排 |

`;
  s = s.replace(hdr, table);

  const agfItem = '- [ ] agf：其余 8 scanner、fuzzy/cache/TUI、shell/命令构造、安装及全部测试。';
  assert(s, agfItem);
  s = s.replace(agfItem, '- [x] agf：其余 8 scanner、fuzzy/cache/TUI、shell/命令构造、安装及全部测试。→ 静态全文回执完成（33/34 + TUI 2686/2686，demo.gif 排除）；测试只读未运行；AGF-01~07 见回执报告。');
  const sgItem = '- [ ] sessiongrep：5 家 parser、DB schema/transaction/candidate 过滤、TUI/MCP/config/安装与测试。';
  assert(s, sgItem);
  s = s.replace(sgItem, '- [x] sessiongrep：5 家 parser、DB schema/transaction/candidate 过滤、TUI/MCP/config/安装与测试。→ 静态全文回执完成（25/26，demo.gif 排除，Cargo.lock 全读）；SG-01~10 见回执报告。');
  fs.writeFileSync(p, s, 'utf8');
  console.log('implement.md patched');
}

// ---- prd.md ----
{
  const p = task + '/prd.md';
  let s = fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, '');
  const bg = '- 竞品不作虚构性能排名；未试装、未运行真实模型/native resume、未执行它们的完整测试。';
  assert(s, bg);
  s = s.replace(bg, bg + '\n- 覆盖模型（2026-10-06 更新）：T1=逐段全文精读（精确行区间+SHA256 回执）；T2=14 类探针对 9,421 个非二进制文件的逐文件机械扫描（`research/sweep-*.json` 含逐文件命中计数）；T3=二进制/超大非源文件排除（原因+哈希）。agf 与 sessiongrep 已完成 T1；其余 13 项 T1 流水线运行中（并发上限 6，排队不砍项）。');
  const ac = '- [ ] 完成全部项目剩余的逐文件审查，并对排除项给出理由；不是仅审查核心链路。';
  assert(s, ac);
  s = s.replace(ac, '- [ ] 全部项目的三层覆盖收口：每个文件 state 明确（T1 全文回执 / T2 探针扫描 / T3 带理由排除），且每个项目关键链路 T1 回执齐备；不是仅审查核心链路。当前 T2/T3 已完成，T1 进行中（agf/sessiongrep 已并入）。');
  const st = '`planning`。当前 `review-report.md` 和设计/执行文件是阶段稿/建议稿，不是“完整审计已完成”或“可以开始修改产品”的声明。任务创建授权、keep going 和本轮已有授权均不替代后续对最终实施摘要的批准。';
  assert(s, st);
  s = s.replace(st, st + ' 2026-10-06：T2 扫描层完成（9,421 文件/14 探针）；T1 回执 2/15 完成、6 项目运行中、7 项目排队。');
  fs.writeFileSync(p, s, 'utf8');
  console.log('prd.md patched');
}
