import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research/python-evidence-tests.log';
let s = fs.readFileSync(p, 'utf8');
const before = (s.match(/C:\\Users\\[^\\\s"']+/g) || []).length;
s = s.replace(/C:\\Users\\[^\\\s"']+/g, '<HOME>');
fs.writeFileSync(p, s, 'utf8');
console.log('sanitized user-path occurrences:', before);
