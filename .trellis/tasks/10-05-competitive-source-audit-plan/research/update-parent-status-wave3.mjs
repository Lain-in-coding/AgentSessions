import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/implement.md';
let s = fs.readFileSync(p, 'utf8');
const old = `**第二波（运行中）**：
- 10-06-first-run-closure（B3，CLI）
- 10-06-journal-retention（B2，adapters-sqlite 适配层 API，preview-first；不动 CLI）`;
if (!s.includes(old)) throw new Error('wave2 block missing');
const neu = `**第二波（已完成、已提交）**：
- 10-06-first-run-closure（B3）→ 校验 PASS（含 doctor 缺库路径修复），commit d5b2d54
- 10-06-journal-retention（B2）→ 校验 PASS（校验方修复 fail-closed 顺序缺陷 +4 测试；数字纠偏），commit e552a3f

**第三波（运行中）**：
- 10-06-retrieval-quality（B4，证据阈值/正名/分桶评测）
- 10-06-provider-stability（B5，Claude/Codex 生命周期证据与矩阵同步）
- 10-06-release-closure（B6，三 OS workflow + Windows 安装/升级/卸载 smoke；不发布）

**待办**：B7 维护性复盘（含是否还有 >10ms 可测热路径浪费、巨型文件职责拆分评估）。`;
s = s.replace(old, neu);
fs.writeFileSync(p, s, 'utf8');
console.log('parent status updated (wave3)');
