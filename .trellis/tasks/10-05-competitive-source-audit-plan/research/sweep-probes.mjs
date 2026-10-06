// sweep-probes.mjs — mechanical probe sweep over every inventoried text file (T2 coverage layer).
// Input: coverage-index.json (central ledger). Output: sweep-<project>.json per project + sweep-index.json summary.
import fs from 'node:fs';
import path from 'node:path';

const research = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research';
const repoRoot = 'C:/AgentSessions/Github_src';
const idxRaw = fs.readFileSync(path.join(research, 'coverage-index.json'), 'utf8').replace(/^\uFEFF/, '');
const idx = JSON.parse(idxRaw);
const skipProjects = new Set(['agf', 'sessiongrep']);

const B = '\\b';
const probes = {
  shell_exec: /child_process|execSync|spawnSync|execFile|std::process::Command|os\/exec|exec\.Command|subprocess|CreateProcess|Process\.Start|\/bin\/(sh|bash)|\bcmd\.exe\b|shell\(\s*true/gi,
  sql_dynamic: /format!\([^)\n]{0,200}(SELECT|INSERT|UPDATE|DELETE)|f"(SELECT|INSERT|UPDATE|DELETE)|f'(SELECT|INSERT|UPDATE|DELETE)|`(SELECT|INSERT|UPDATE|DELETE)|"(SELECT|INSERT|UPDATE|DELETE)[^"]{0,400}"\s*(\+|\.format|\$\{)/gi,
  provider_paths: /\.claude[^a-z]|\.codex[^a-z]|\.gemini[^a-z]|\.cursor[^a-z]|\.opencode[^a-z]|\.aider[^a-z]|\.continue[^a-z]|\.windsurf[^a-z]|\.zed[^a-z]|\.goose[^a-z]|\.pi[^a-z]|\.kiro[^a-z]|\.hermes[^a-z]|Antigravity|roo-?code/gi,
  fs_watch: /chokidar|fs\.watch|notify::|RecommendedWatcher|FileSystemWatcher|watchman/gi,
  catch_empty: /catch\s*(\([^)]*\))?\s*\{\s*\}|except[^:\n]*:\s*\n\s*(pass|\.\.\.|#)|recover\(\)\s*\{\s*\}/g,
  unwrap_or: /unwrap_or(_default|_else)?\(/g,
  ignored_result: /let\s+_\s*=|\.ok\(\);|\.ignore\(\)/g,
  truncation_limit: /max_sessions|MAX_SESSIONS|\.truncate\(|\.take\(\d|\.limit\(|LIMIT\s+\d|\.slice\(0,\s*\d|head\s+-n/gi,
  concurrency: /tokio::spawn|thread::spawn|std::thread|rayon|go func|async move|Promise\.all|worker_threads|new Worker\(|ThreadPool|spawn_blocking/g,
  index_storage: /CREATE VIRTUAL TABLE|fts5|tantivy|duckdb|hnsw|usearch|bm25|embedding|cosine_similar|CREATE INDEX|vectorize|vector_search/gi,
  resume_launch: /--resume|portable-pty|ConPTY|\btmux\b|\bscreen\b|--continue|\bresume\b|\battach\b/gi,
  secret_redaction: /redact|sanitiz|\bmask_\b|api[_-]?key|authorization|bearer|credential|\btoken\b/gi,
  inline_tests: /#\[cfg\(test\)\]|#\[tokio::test\]|#\[test\]|func Test|func Benchmark|describe\(|it\(|test\(/g,
  surface_mcp: /McpServer|rmcp|#\[tool|mcp|app\.route|Router::new|axum|express\(|fastify|actix|get\("|post\("/gi,
};
const probeNames = Object.keys(probes);
const maxBytes = 4 * 1024 * 1024;

const indexSummary = { schema: 'competitive-audit.sweep/v1', date: '2026-10-06', method: 'per-file regex hit counts over every inventoried non-binary file; files that were full-read separately are also swept for uniformity', probes: Object.fromEntries(probeNames.map(n => [n, probes[n].source])), projects: {} };

for (const proj of idx.projects) {
  if (skipProjects.has(proj.project)) continue;
  const root = path.join(repoRoot, proj.project);
  const filesOut = [];
  const stats = { total: proj.files.length, scanned: 0, excluded_binary: 0, skipped_oversize: 0, skipped_read_error: 0, files_with_any_hit: 0 };
  const probeFileCounts = Object.fromEntries(probeNames.map(n => [n, 0]));
  const probeHitTotals = Object.fromEntries(probeNames.map(n => [n, 0]));

  for (const f of proj.files) {
    if (f.category === 'binary') { stats.excluded_binary++; continue; }
    const abs = path.join(root, f.path);
    let text;
    try {
      const st = fs.statSync(abs);
      if (st.size > maxBytes) { stats.skipped_oversize++; filesOut.push({ p: f.path, skipped: 'oversize', bytes: f.bytes }); continue; }
      text = fs.readFileSync(abs, 'utf8');
    } catch (e) { stats.skipped_read_error++; filesOut.push({ p: f.path, skipped: 'read_error', bytes: f.bytes }); continue; }
    stats.scanned++;
    const hits = {};
    let any = 0;
    for (const name of probeNames) {
      const re = new RegExp(probes[name].source, probes[name].flags.includes('g') ? probes[name].flags : probes[name].flags + 'g');
      const m = text.match(re);
      if (m && m.length) { hits[name] = m.length; any++; probeFileCounts[name]++; probeHitTotals[name] += m.length; }
    }
    if (any) stats.files_with_any_hit++;
    filesOut.push({ p: f.path, bytes: f.bytes, swept: true, hits });
  }

  const out = { schema: 'competitive-audit.sweep-project/v1', date: '2026-10-06', project: proj.project, commit: proj.commit, repo_path: `Github_src/${proj.project}`, probe_definitions_in: 'sweep-index.json', stats, probe_file_counts: probeFileCounts, probe_hit_totals: probeHitTotals, files: filesOut };
  const outName = `sweep-${proj.project}.json`;
  fs.writeFileSync(path.join(research, outName), JSON.stringify(out), 'utf8');
  indexSummary.projects[proj.project] = { receipt: `research/${outName}`, stats, probe_file_counts: probeFileCounts };
  console.log(`${proj.project}: scanned=${stats.scanned} binary_excl=${stats.excluded_binary} oversize=${stats.skipped_oversize} read_err=${stats.skipped_read_error} with_hits=${stats.files_with_any_hit}`);
}
fs.writeFileSync(path.join(research, 'sweep-index.json'), JSON.stringify(indexSummary, null, 1), 'utf8');
console.log('sweep-index.json written');

