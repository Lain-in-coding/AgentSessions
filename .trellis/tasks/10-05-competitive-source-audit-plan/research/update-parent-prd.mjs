import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/prd.md';
let s = fs.readFileSync(p, 'utf8');
const marker = '## User-owned Decisions Still Open';
const i = s.indexOf(marker);
if (i < 0) throw new Error('decisions section missing');
const j = s.indexOf('## Status');
const block = `## User-owned Decisions（2026-10-06 已拍板）

- **D1（journal 保留边界）＝批准（有约束治理）**：允许对已完成操作（terminal）的完整明细作限期/限量 compact，只保留必要摘要、generation/digest 与未决操作；building/恢复幂等所需记录不可删；实施前必须有可解释 preview 与回滚；对应子任务 B2。
- **D2（整改次序）＝批准（CLI-first）**：先兑现 CLI 核心闭环与稳定性，冻结新增 GUI/云/图谱/更多 provider；不等于删除已有入口，也不缩减本审计范围。
- **D3（六项不变量自查）＝批准（实施前置）**：cap 先于过滤 / 零证据升格 / 失败固化 / 裁剪进缓存 / resume 全参数化 / 投影截断共病，作为 B4/B5 晋级前的门；对应子任务 six-invariant-selfchecks。

## Child Tasks（2026-10-06 创建）

- 10-06-fact-table-truth-repair（B0 事实账修复，P0，docs-only）
- 10-06-hotpath-git-probe（B1 热路径 Git 探测，P1，CLI）
- 10-06-six-invariant-selfchecks（D3 六项自查，P0，测试/研究）

后续子任务（B2 journal、B3 首用、B4 检索质量、B5 provider 稳定性、B6 发布闭环、B7 维护性）在依赖满足后创建；依赖关系见 implement.md 与 design.md。

`;
s = s.slice(0, i) + block + s.slice(j);
fs.writeFileSync(p, s, 'utf8');
console.log('parent prd updated');
