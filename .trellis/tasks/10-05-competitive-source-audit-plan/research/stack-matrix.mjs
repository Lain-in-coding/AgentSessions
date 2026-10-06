import fs from 'node:fs';
import path from 'node:path';
const repoRoot = 'C:/AgentSessions/Github_src';
const out = {};
for (const d of fs.readdirSync(repoRoot, { withFileTypes: true })) {
  if (!d.isDirectory()) continue;
  const root = path.join(repoRoot, d.name);
  const marks = [];
  const has = (p) => fs.existsSync(path.join(root, p));
  if (has('Cargo.toml')) marks.push('Rust(cargo)');
  if (has('go.mod')) marks.push('Go(go.mod)');
  if (has('pyproject.toml') || has('setup.py')) marks.push('Python');
  let pkg = null;
  if (has('package.json')) { try { pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')); } catch {} }
  if (pkg) {
    marks.push('Node/TS(package.json)');
    const deps = { ...(pkg.dependencies || {}), ...(pkg.devDependencies || {}) };
    if (deps.electron) marks.push('Electron');
    if (deps.svelte || deps['@sveltejs/kit'] || deps['vite-plugin-svelte']) marks.push('Svelte');
    if (deps.vue) marks.push('Vue');
    if (deps.react || deps['react-dom']) marks.push('React');
    if (deps['@tauri-apps/api'] || has('src-tauri')) marks.push('Tauri');
    if (deps.flexsearch) marks.push('FlexSearch');
    if (deps.tantivy) marks.push('Tantivy(js)');
  }
  // swift scan (top 3 levels)
  let swift = 0;
  const walk = (dir, depth) => { if (depth > 3) return; let ents = []; try { ents = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const e of ents) { if (e.name === '.git' || e.name === 'node_modules' || e.name === 'target') continue; const p = path.join(dir, e.name); if (e.isDirectory()) walk(p, depth + 1); else if (e.name.endsWith('.swift')) swift++; } };
  walk(root, 0);
  if (swift > 0) marks.push(`Swift(${swift} files)`);
  out[d.name] = marks;
}
console.log(JSON.stringify(out, null, 1));
fs.writeFileSync('C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research/stack-matrix.json', JSON.stringify({ date: '2026-10-06', method: 'mechanical marker detection (Cargo.toml/go.mod/package.json deps/*.swift/src-tauri) on local clones', projects: out }, null, 1), 'utf8');
