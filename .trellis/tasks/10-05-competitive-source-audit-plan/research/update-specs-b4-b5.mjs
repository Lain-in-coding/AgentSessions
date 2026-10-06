import fs from 'node:fs';
const patchFile = (path, edits) => {
  let raw = fs.readFileSync(path, 'utf8');
  const eol = raw.includes('\r\n') ? '\r\n' : '\n';
  let s = raw.replace(/\r\n/g, '\n');
  for (const [find, replace] of edits) {
    if (!s.includes(find)) throw new Error('anchor missing in ' + path + ': ' + find.slice(0, 70));
    s = s.replace(find, replace);
  }
  fs.writeFileSync(path, s.replace(/\n/g, eol), 'utf8');
  console.log('patched:', path);
};

// 1) sqlite spec: semantic evidence gate
const sqlite = 'C:/AgentSessions/.trellis/spec/agentsessions-adapters-sqlite/backend/index.md';
const gate = [
'## Semantic search evidence gate (B4)',
'',
'Contract: when assembling semantic or hybrid candidates, a candidate whose',
'cosine similarity is below the evidence floor is discarded in the same query',
'pass that applies provider/time/repo/facet predicates, and before the bounded',
'top-k heap insert; filter-before-top-k therefore holds for the evidence gate',
'as well. The shipped default floor is 0.20, selected from the holdout and',
'frozen regression scans (zero-recall-loss interval intersection, one step of',
'margin). The floor is overridable through ASG_SEMANTIC_SIMILARITY_FLOOR;',
'non-numeric or non-finite values must fail as invalid_request rather than',
'silently falling back. A floor of 0.0 still excludes negative similarity.',
'The lexical FTS path is untouched by the floor. Known limitation, disclosed in',
'README and the retrieval report: the fuzzy-lexical hash model has a background',
'similarity near 0.40 on unrelated short text, so the default gate rejects zero',
'and negative evidence but cannot remove non-zero, non-semantic false hits;',
'that requires real embedding weights plus recalibration.',
'',
''].join('\n');
patchFile(sqlite, [[ '## Quality Check\n', gate + '## Quality Check\n' ]]);

// 2) application spec: final_score zero-evidence defense
const app = 'C:/AgentSessions/.trellis/spec/agentsessions-application/backend/index.md';
const appBullet = [
'- `final_score(relevance, age_ms, is_sidechain, in_current_repo)` — ranking',
'  entry point. `relevance <= 0.0` or NaN returns exactly 0.0 before any',
'  recency/repo/sidechain preference is applied: preference signals may only',
'  reorder admitted, positively-evidenced hits and must never resurrect a',
'  zero-evidence candidate. Lexical hits always carry a positive FTS score;',
'  this defense exists so future candidate sources cannot regress that.',
''].join('\n');
const budgetAnchor = '  `max_evidence_spans`).';
patchFile(app, [[budgetAnchor, budgetAnchor + '\n' + appBullet]]);

// 3) provider specs: lifecycle contracts
const claude = 'C:/AgentSessions/.trellis/spec/agentsessions-provider-claude/backend/index.md';
const claudeLifecycle = [
'## Lifecycle evidence contracts (B5)',
'',
'- Append: parsing a strictly longer file that extends the previous bytes keeps',
'  every previously parsed message and span byte-identical; only the new',
'  message and committed count grow (property across the 64-seed corpus).',
'- Shrink / torn tail: truncation at a line boundary equals a prefix snapshot;',
'  a torn half line is skipped honestly (committed/skipped/diagnostics report',
'  it) and never fails the whole file or eats a valid prefix.',
'- Same-length rewrite: rebuilt bytes win (no stale cache); spans stay tied to',
'  the current bytes with unchanged ranges.',
'- Fork: sibling messages may share a parent edge; dangling parent edges are',
'  preserved as-is; an empty parentUuid maps to None.',
'- Parse purity: ProviderAdapter::parse receives bytes only — it has no path',
'  input and no previous-result input, so file moves and WAL handling are',
'  store-layer concerns (see the relocation and source snapshot scenarios in',
'  the sqlite adapter spec).',
'',
''].join('\n');
patchFile(claude, [[ '## Quality Check\n', claudeLifecycle + '## Quality Check\n' ]]);

const codex = 'C:/AgentSessions/.trellis/spec/agentsessions-provider-codex/backend/index.md';
const codexLifecycle = [
'## Lifecycle evidence contracts (B5)',
'',
'- Append: extending the rollout keeps previously parsed items and spans',
'  byte-identical; only new items and committed count grow (property across',
'  the 64-seed corpus).',
'- Shrink / torn tail: line-boundary truncation equals a prefix snapshot; a',
'  torn half line is skipped with explicit diagnostics instead of failing the',
'  file or swallowing a valid prefix.',
'- Same-length rewrite: rebuilt bytes win; stale copies are never returned.',
'- Parent edges: the codex rollout format carries no parent pointer',
'  (parent_native_id is None). Copied prefixes and event_msg mirrors must not',
'  fabricate edges or double-count; this is pinned by reverse tests.',
'- Parse purity: parse receives bytes only (no path, no previous result);',
'  moves and WAL are store-layer concerns.',
'',
''].join('\n');
patchFile(codex, [[ '## Quality Check\n', codexLifecycle + '## Quality Check\n' ]]);
