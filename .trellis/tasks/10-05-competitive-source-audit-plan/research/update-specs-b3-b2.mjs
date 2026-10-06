import fs from 'node:fs';

const patchFile = (path, edits) => {
  let raw = fs.readFileSync(path, 'utf8');
  const eol = raw.includes('\r\n') ? '\r\n' : '\n';
  let s = raw.replace(/\r\n/g, '\n');
  for (const [find, replace] of edits) {
    if (!s.includes(find)) throw new Error('anchor missing in ' + path + ': ' + find.slice(0, 60));
    s = s.replace(find, replace);
  }
  fs.writeFileSync(path, s.replace(/\n/g, eol), 'utf8');
  console.log('patched:', path, 'eol=', eol === '\r\n' ? 'CRLF' : 'LF');
};

// ---- CLI spec ----
const cliPath = 'C:/AgentSessions/.trellis/spec/agentsessions-cli/backend/index.md';
const oldBullet = [
'- [ ] Error envelopes and human diagnostics must preserve canonical category',
'      while keeping messages bounded and path/native-ID-free. Never relay raw',
'      provider, SQLite, or conflict payloads to stdout or stderr.'].join('\n');
const newBullet = [
'- [ ] Error envelopes and human diagnostics must preserve canonical category',
'      while keeping messages bounded. Never relay provider/transcript/SQLite-derived',
'      paths, native IDs, raw payloads, or conflict internals to stdout or stderr.',
'      Caller-owned catalog paths (explicit --db, or the platform default that',
'      config paths already reports) are allowed in bounded operator guidance and',
'      still pass the cross-boundary secret redactor.'].join('\n');
const cliScenario = [
'## Scenario: Default store path and missing-catalog contract',
'',
'### 1. Scope / Trigger',
'Any data subcommand invoked without --db, and any read command pointed at a',
'catalog that does not exist yet.',
'',
'### 2. Signatures',
'resolve_store_path(explicit: Option<PathBuf>) -> CliResult<PathBuf> is the only',
'resolution path; DEFAULT_DB_FILE_NAME = "asg.db" pins the file name inside the',
'platform data directory reported by config paths. No environment-variable or',
'multi-level fallback chains are allowed.',
'',
'### 3. Contracts',
'Explicit --db always wins. Without it, known data subcommands resolve the single',
'platform default; unresolvable defaults return invalid_request with guidance to',
'pass --db explicitly. Read commands against a missing catalog fail closed with',
'catalog_error (exit 6), create no file or parent directory, and include one',
'runnable sync instruction (agent-session-grep --db <path> sync --discover).',
'No-command and unknown-command invocations stay usage errors (exit 2) so typos',
'are never masked by a missing-catalog message. The hook entry stays',
'non-blocking; doctor must surface the same missing-catalog guidance instead of',
'a masked internal error. relocation keeps its opaque, path-free error contract.',
'',
'### 4. Validation & Error Matrix',
'Explicit path -> used verbatim. Default resolvable -> <data>/asg.db. Default',
'unresolvable -> invalid_request + explicit --db guidance. Missing catalog on a',
'read command -> catalog_error + sync --discover instruction, zero filesystem',
'writes. Unknown command -> exit 2. Hook with missing catalog -> exit 1, empty',
'stdout, no writes.',
'',
'### 5. Good/Base/Bad Cases',
'Good: fresh machine runs version -> config paths -> sync --discover -> search.',
'Base: an explicitly supplied --db path keeps winning over the default. Bad: a',
'read command fabricating an empty success, or doctor dying with a masked error',
'while telling the operator to run doctor.',
'',
'### 6. Tests Required',
'first_run_closure.rs must pin: missing catalog writes nothing and names',
'sync --discover; explicit beats default; unresolvable default names --db;',
'discover with zero sources asks for an explicit source; human search projection',
'regression. The five-step experiment is reproduced under an isolated HOME.',
'',
'### 7. Wrong vs Correct',
'Wrong: constructing the full App (or probing git) just to learn the store path,',
'or letting a read command create the database.',
'Correct: resolve the path once, open read-only, and fail closed with an',
'actionable, secret-redacted instruction when the catalog is absent.',
'',
''].join('\n');
patchFile(cliPath, [
  [oldBullet, newBullet],
  ['## Quality Check\n\nBefore proposing a commit for this crate:', cliScenario + '## Quality Check\n\nBefore proposing a commit for this crate:'],
]);

// ---- sqlite spec ----
const sqlitePath = 'C:/AgentSessions/.trellis/spec/agentsessions-adapters-sqlite/backend/index.md';
const sqliteScenario = [
'## Scenario: Journal retention compaction (schema v19)',
'',
'### 1. Scope / Trigger',
'Explicit maintenance on a long-lived catalog whose terminal outbox batches keep',
'full detail manifests for every historical rewrite. Preview, staged plan, apply',
'and recovery are the only surfaces; ordinary open paths never compact.',
'',
'### 2. Signatures',
'preview_journal_compaction() -> JournalCompactionPreview is read-only.',
'stage_journal_compaction(preview) -> JournalCompactionStage persists the plan with',
'a CAS plan digest. apply_journal_compaction(id) -> JournalCompactionOutcome runs',
'the single-transaction rewrite. recover_journal_compactions() converges staged',
'plans explicitly; journal_compaction_event(id) reads the audit row.',
'',
'### 3. Contracts',
'Permanent bytes: operation_id, base/target_generation, state, operation_digest,',
'durable_point, created/committed_at_ms, error_code, relocation_json,',
'detail_format. Unresolved rows (building / search_built / cleanup_pending) keep',
'full detail forever and never enter a plan. Terminal rows (activated / aborted /',
'superseded) may aggregate the five detail columns into [] plus a',
'detail_summary_json carrying counts, byte sizes and a detail_digest; a',
'no-benefit row (detail smaller than the summary) stays full byte-identical.',
'Unknown states or detail formats fail closed with SchemaIncompatible in read,',
'preview and apply; validation happens before any skip logic. Apply is one',
'transaction with a four-column CAS (state, operation_digest,',
'target_generation, detail_digest); drift rolls back whole and recovery',
'abandons the plan with a reason. The v18 -> v19 migration is additive, rolls',
'back on interruption, keeps old rows readable as full, and older binaries',
'refuse the newer user_version instead of misreading summaries. The',
'journal_compactions audit table grows linearly with explicit maintenance and',
'that growth must be disclosed next to any compaction numbers.',
'',
'### 4. Validation & Error Matrix',
'Unknown format on pending or aggregated rows -> SchemaIncompatible (read,',
'preview, apply). Plan drift -> whole-plan rollback; recover marks abandoned,',
'detail untouched. Re-apply -> idempotent, zero rewrites. Interrupted stage ->',
'recover converges without touching details. Concurrent readers -> WAL snapshot',
'isolation across the rewrite.',
'',
'### 5. Good/Base/Bad Cases',
'Good: 21 single-message rewrites aggregate to 0.381 percent of detail bytes',
'(0.876 percent including the audit row) with operation digests unchanged.',
'Base: a tiny terminal row stays full because aggregation would not save bytes.',
'Bad: deleting unresolved detail, silently skipping an unknown format, or',
'compacting automatically inside open_for_write.',
'',
'### 6. Tests Required',
'journal_retention.rs pins preview numbers, the soak, killed-stage recovery,',
'drift abandonment, concurrent reads, v18 migration and rollback, unknown',
'format/state fail-closed on all three surfaces, no-benefit rows staying full,',
'unresolved-row protection, CAS idempotence, and byte-identical relocation',
'manifests.',
'',
'### 7. Wrong vs Correct',
'Wrong: explain-or-skip unknown values, aggregate pending rows, or report a',
'compaction ratio without stating whether the audit row is included.',
'Correct: validate first, aggregate only terminal rows that benefit, keep the',
'CAS transaction atomic, and publish both ratio scopes.',
'',
''].join('\n');
patchFile(sqlitePath, [
  ['## Quality Check\n\n- \`catalog\` remains authoritative;', sqliteScenario + '## Quality Check\n\n- \`catalog\` remains authoritative;'],
]);
