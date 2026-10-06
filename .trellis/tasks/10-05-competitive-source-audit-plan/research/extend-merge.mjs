import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research/merge-receipt.mjs';
let s = fs.readFileSync(p, 'utf8');
const old = 'for (const rf of receipt.files || []) {';
if (!s.includes(old)) throw new Error('anchor missing');
s = s.replace(old, 'const allEntries = (receipt.files || []).concat(receipt.supplementary_reads || []);\nfor (const rf of allEntries) {');
fs.writeFileSync(p, s, 'utf8');
console.log('merge script extended');
