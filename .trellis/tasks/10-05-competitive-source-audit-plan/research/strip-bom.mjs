import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research/coverage-index.json';
let s = fs.readFileSync(p, 'utf8');
if (s.charCodeAt(0) === 0xFEFF) { s = s.slice(1); JSON.parse(s); fs.writeFileSync(p, s, 'utf8'); console.log('BOM stripped'); } else { console.log('no BOM'); }
