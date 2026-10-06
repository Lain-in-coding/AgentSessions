// merge-receipt.mjs --project <name> --receipt research/coverage-X.json [--dry]
// Generalizes receipt merging: hash-verify + range union + state recompute. Idempotent.
import fs from 'node:fs';
import path from 'node:path';
const args = process.argv.slice(2);
const getArg = (n) => { const i = args.indexOf(n); return i >= 0 ? args[i + 1] : null; };
const project = getArg('--project');
const receiptRel = getArg('--receipt');
const dry = args.includes('--dry');
if (!project || !receiptRel) { console.error('usage: --project <name> --receipt research/coverage-<x>.json [--dry]'); process.exit(2); }
const research = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research';
const repoRoot = 'C:/AgentSessions/Github_src';
const idxPath = path.join(research, 'coverage-index.json');
const idx = JSON.parse(fs.readFileSync(idxPath, 'utf8').replace(/^\uFEFF/, ''));
const proj = idx.projects.find(p => p.project === project);
if (!proj) { console.error('project not found:', project); process.exit(2); }
const recPath = path.join(research, receiptRel.replace(/^research[\\/]/, ''));
const receipt = JSON.parse(fs.readFileSync(recPath, 'utf8').replace(/^\uFEFF/, ''));
const norm = (rf, f) => {
  let rs = [];
  if (Array.isArray(rf.read_ranges)) {
    for (const r of rf.read_ranges) {
      if (Array.isArray(r)) rs.push([Number(r[0]), Number(r[1])]);
      else if (r && typeof r === 'object') rs.push([Number(r.start_line ?? r.start), Number(r.end_line ?? r.end)]);
    }
  } else if (Array.isArray(rf.ranges)) { for (const r of rf.ranges) { if (Array.isArray(r)) rs.push([Number(r[0]), Number(r[1])]); else rs.push([Number(r.start_line ?? r.start), Number(r.end_line ?? r.end)]); } }
  else if (Array.isArray(rf.reviewed_ranges)) { for (const r of rf.reviewed_ranges) rs.push([Number(r.start_line ?? r.start), Number(r.end_line ?? r.end)]); }
  else if (Array.isArray(rf.read_intervals)) { for (const r of rf.read_intervals) rs.push([Number(r.start_line ?? r.start), Number(r.end_line ?? r.end)]); }
  return rs.filter(([a, b]) => Number.isFinite(a) && Number.isFinite(b) && b >= a);
};
const coalesce = (rs) => { rs.sort((a, b) => a[0] - b[0]); const out = []; for (const r of rs) { if (!out.length || r[0] > out[out.length - 1][1] + 1) out.push([...r]); else out[out.length - 1][1] = Math.max(out[out.length - 1][1], r[1]); } return out; };
let merged = 0, hashFail = [], notFound = 0;
const allEntries = (receipt.files || []).concat(receipt.supplementary_reads || []);
for (const rf of allEntries) {
  const f = proj.files.find(x => x.path === rf.path);
  if (!f) { notFound++; continue; }
  const ranges = norm(rf, f);
  const abs = path.join(repoRoot, project, f.path);
  let ok = false, actual = null;
  if (fs.existsSync(abs)) { actual = fs.statSync(abs).size > 0 || true ? (await import('node:crypto')).createHash('sha256').update(fs.readFileSync(abs)).digest('hex') : null; }
  const expected = [rf.sha256_final, rf.sha256_initial, rf.sha256, rf.current_file_sha256].filter(Boolean).map(s => String(s).toLowerCase());
  if (expected.length === 0) { ok = null; }
  else { ok = actual ? expected.includes(actual) : false; }
  if (ok === false) hashFail.push(f.path);
  const prev = Array.isArray(f.read_ranges) ? f.read_ranges.map(r => [Number(r[0]), Number(r[1])]) : [];
  const union = coalesce(prev.concat(ranges));
  const covered = union.reduce((s, [a, b]) => s + (b - a + 1), 0);
  const total = Number(f.line_count) || 0;
  f.read_ranges = union;
  if (rf.status === 'excluded') { f.state = 'excluded'; if (rf.reason || rf.exclusion_reason) f.exclusion_reason = rf.reason || rf.exclusion_reason; }
  else if (total > 0 && covered >= total) f.state = 'read_full';
  else if (covered > 0) f.state = 'read_partial';
  else f.state = 'unread';
  f.receipts = Array.isArray(f.receipts) ? Array.from(new Set(f.receipts.concat([`research/${path.basename(recPath)}`]))) : [`research/${path.basename(recPath)}`];
  f.hash_verified = ok;
  f.verified_at = '2026-10-06';
  merged++;
}
const counts = {};
for (const f of proj.files) counts[f.state] = (counts[f.state] || 0) + 1;
proj.coverage_merge = Object.assign({}, proj.coverage_merge, { date: '2026-10-06', receipts: Array.from(new Set((proj.coverage_merge?.receipts || []).concat([`research/${path.basename(recPath)}`]))), counts, hashes_verified: hashFail.length === 0, hash_failures: hashFail });
if (!dry) { idx.updated = new Date().toISOString(); fs.writeFileSync(idxPath, JSON.stringify(idx), 'utf8'); }
console.log(JSON.stringify({ project, receipt: path.basename(recPath), files_merged: merged, not_in_index: notFound, hash_failures: hashFail.length, states: counts, dry }, null, 1));
