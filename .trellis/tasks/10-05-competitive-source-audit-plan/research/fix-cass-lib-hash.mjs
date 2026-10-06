import fs from 'node:fs';
import crypto from 'node:crypto';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research/coverage-cass-lib.json';
const r = JSON.parse(fs.readFileSync(p, 'utf8'));
const f = r.files.find(x => x.path === 'src/sources/config.rs');
const actual = crypto.createHash('sha256').update(fs.readFileSync('C:/AgentSessions/Github_src/coding_agent_session_search/src/sources/config.rs')).digest('hex');
if (f.sha256_initial === actual) { console.log('already correct'); process.exit(0); }
f.hash_correction = { date: '2026-10-06', by: 'main-session', note: `receipt recorded a 63-hex-char transcription typo (${f.sha256_initial}); corrected to the on-disk 64-hex SHA256. Content-unchanged claim retained because the worker session verified initial==final and main session re-verified on disk.`, previous_value: f.sha256_initial, corrected_value: actual };
f.sha256_initial = actual; f.sha256_final = actual;
fs.writeFileSync(p, JSON.stringify(r, null, 1), 'utf8');
console.log('corrected to', actual);
