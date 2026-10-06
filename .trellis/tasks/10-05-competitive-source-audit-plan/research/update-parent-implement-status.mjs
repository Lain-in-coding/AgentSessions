import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/implement.md';
let s = fs.readFileSync(p, 'utf8');
const marker = '## A. 剩余审查工作线（不能静默砍掉）';
if (!s.includes(marker)) throw new Error('marker missing');
const block = `## 实施进行态（2026-10-06，D1/D2/D3 已批准）

**已创建并启动的子任务**：
- 10-06-fact-table-truth-repair（B0，P0，docs-only）→ 运行中
- 10-06-hotpath-git-probe（B1，P1，CLI 热路径）→ 运行中
- 10-06-six-invariant-selfchecks（D3，P0，测试/研究）→ 运行中

**后续波次（依赖满足后创建，不预支顺序）**：
- B3 首用闭环 ← 依赖 B1 落地后的装配形态
- B2 journal 治理 ← D1 已批准（有约束 compact；未决记录不可删）
- B4 检索质量 / B5 provider 稳定性 ← 依赖 D3 六项结论（失败项先修复）
- B6 发布闭环 ← 依赖 B0/B1/B3
- B7 维护性 ← 最后

**纪律**：子任务实现者不 commit；主会话复核后统一提交（仅本任务树+对应产品文件）。

`;
s = s.replace(marker, block + marker);
fs.writeFileSync(p, s, 'utf8');
console.log('parent implement.md updated');
