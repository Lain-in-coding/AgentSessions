import fs from 'node:fs';
const research = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research';
const p = research + '/coverage-index.json';
const idx = JSON.parse(fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, ''));
const done = new Set(['agf', 'sessiongrep']);
let touched = 0;
for (const proj of idx.projects) {
  if (done.has(proj.project)) continue;
  const sweep = JSON.parse(fs.readFileSync(`${research}/sweep-${proj.project}.json`, 'utf8'));
  const map = new Map(sweep.files.map(f => [f.p, f]));
  let swept = 0, binary = 0, oversize = 0, readErr = 0;
  for (const f of proj.files) {
    const s = map.get(f.path);
    const receipt = `research/sweep-${proj.project}.json`;
    if (!s) { f.state = 'excluded_binary'; f.sweep_receipt = receipt; f.skip_reason = 'binary_inventory_exclusion'; binary++; continue; }
    if (s.swept) { f.state = 'swept_probes'; f.sweep_receipt = receipt; f.probes_hit = Object.keys(s.hits || {}).length; swept++; continue; }
    if (s.skipped === 'oversize') { f.state = 'skipped_oversize'; f.sweep_receipt = receipt; f.skip_reason = 'oversize_gt_4MiB'; oversize++; continue; }
    if (s.skipped === 'read_error') { f.state = 'sweep_read_error'; f.sweep_receipt = receipt; f.skip_reason = 'utf8_read_error'; readErr++; continue; }
    f.state = 'not_in_sweep';
  }
  proj.coverage_merge = { date: '2026-10-06', layer: 'T2_sweep_only', receipts: [`research/sweep-${proj.project}.json`], sweep_scanned: swept, binary_excluded: binary, oversize_skipped: oversize, read_errors: readErr, full: 0, partial: 0, unread_text: swept, notes: 'T1 full-read receipts pending for this project; T2 probe sweep complete over all inventoried non-binary files.' };
  touched++;
}
if (!idx.merge_log) idx.merge_log = [];
idx.merge_log.push({ date: '2026-10-06', merged_by: 'main-session', projects: touched, method: 'T2 sweep states written back to per-file ledger (swept_probes/excluded_binary/skipped_oversize)' });
idx.updated = '2026-10-06T22:05:00+08:00';
fs.writeFileSync(p, JSON.stringify(idx), 'utf8');
console.log('updated projects:', touched);
