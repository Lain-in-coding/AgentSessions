import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/implement.md';
let s = fs.readFileSync(p, 'utf8');
const old = `**已创建并启动的子任务**：
- 10-06-fact-table-truth-repair（B0，P0，docs-only）→ 运行中
- 10-06-hotpath-git-probe（B1，P1，CLI 热路径）→ 运行中
- 10-06-six-invariant-selfchecks（D3，P0，测试/研究）→ 运行中`;
if (!s.includes(old)) throw new Error('status block missing');
const neu = `**第一波（已完成、已提交）**：
- 10-06-fact-table-truth-repair（B0）→ 校验 PASS，commit 77ffbb7
- 10-06-hotpath-git-probe（B1）→ 校验 PASS，commit 4a1aa28（get/show 探测 2→0；search 4→2）
- 10-06-six-invariant-selfchecks（D3）→ 校验 PASS（6/6 通过 + 3 处边界归 B4），commit f899794

**第二波（运行中）**：
- 10-06-first-run-closure（B3，CLI）
- 10-06-journal-retention（B2，adapters-sqlite 适配层 API，preview-first；不动 CLI）`;
s = s.replace(old, neu);
fs.writeFileSync(p, s, 'utf8');
console.log('parent status updated');
