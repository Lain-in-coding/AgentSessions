import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### agentsview · 分片 B（sync/parser；T1 9,804 行，codex/claude full + engine partial；receipt: \`research/coverage-agentsview-sync.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AGV-B1 | P1 正面 | 全量扫描不会因“源文件未发现”删档：主循环只处理发现集、无 presence 清扫；presence 漂移只在只读 parse-diff 报告；重扫用 orphan copy 保档 | \`engine.go:2465-2757,213-235,1753-1782\`；\`parsediff.go:917-967\` |
| AGV-B2 | P2 | 显式删除例外族：容器/成员消失 → SkipNoSession+ForceReplace → DeleteParserExcludedSessions 真删；同路径 ID 换代有 stale-row cleanup；护栏=可达性检查/cwd 冻结/resurrection guard | \`engine.go:4461-4469,4685-4701,4016-4028,4879-4982,2997-3015,4935-4969\`；\`cwd_filter.go:74-96\` |
| AGV-B3 | P2 | 失败分类=noCacheSkip（瞬时错误不落负缓存）+ 按结果 data_version 降版重试；弱点：无退避/重试计数/告警面，重试依赖下一次同步 | \`engine.go:4227-4232,4660-4682,4722-4728,6847-6858\` |
| AGV-B4 | P2 | resync 换库守卫充分（取消/空发现/synced=0/failed>ok 拒绝 swap），但 rename 成功后 reopen 失败不回滚（降级非丢数据） | \`engine.go:1195-1229,1618-1641,1906-1949\` |
| AGV-B5 | P3 | parser 保真边界：codex fork gate fail-open；claude DAG 分支阈值启发式；半解析处置到位（截断识别、完整行才推进 offset）；provider 注册表 53 case+2 import-only | \`codex.go:97-145,1769-1851,1951-1963\`；\`claude.go:997-1103,269-276\`；\`provider.go:402-521\` |

> agentsview 的“非破坏重扫 + 显式删除契约”是竞品中最接近 ASG 纪律的实现之一；它与 sessiongrep/Recall 形成对照：同族产品里“扫描删数据”“失败固化为 current”各有踩坑者。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('agentsview-B section added; length', s.length);
