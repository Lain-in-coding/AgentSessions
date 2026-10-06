import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research/merge-receipt.mjs';
let s = fs.readFileSync(p, 'utf8');
// 1) accept `ranges`
const oldNorm = "  } else if (Array.isArray(rf.reviewed_ranges)) {";
if (!s.includes(oldNorm)) throw new Error('norm anchor missing');
s = s.replace(oldNorm, "  } else if (Array.isArray(rf.ranges)) { for (const r of rf.ranges) { if (Array.isArray(r)) rs.push([Number(r[0]), Number(r[1])]); else rs.push([Number(r.start_line ?? r.start), Number(r.end_line ?? r.end)]); } }\n  else if (Array.isArray(rf.reviewed_ranges)) {");
// 2) hash semantics: null when no expected hash provided
const oldHash = "  ok = actual ? expected.includes(actual) : false;\n  if (!ok) hashFail.push(f.path);";
if (!s.includes(oldHash)) throw new Error('hash anchor missing');
s = s.replace(oldHash, "  if (expected.length === 0) { ok = null; }\n  else { ok = actual ? expected.includes(actual) : false; }\n  if (ok === false) hashFail.push(f.path);");
fs.writeFileSync(p, s, 'utf8');
console.log('merge script fixed');
