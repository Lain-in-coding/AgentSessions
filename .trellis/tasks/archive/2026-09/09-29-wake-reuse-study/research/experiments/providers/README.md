# Experiment C: synthetic provider structure probes

**Isolated synthetic structure probes — not product adapters, provider
certification, resume support, canonical identity proof or a benchmark.**
Stdlib Python only; every database is generated in a temp directory, read
through one pinned read-only snapshot, and never copied or mutated.

## Run from the repository root (PowerShell)

```powershell
$dir = '.trellis/tasks/09-29-wake-reuse-study/research/experiments/providers'
$out = '.trellis/tasks/09-29-wake-reuse-study/research/results/providers.json'

python -B -m unittest discover -s $dir -p 'test_*.py' -v
python -B "$dir/report.py" report $out
python -B "$dir/report.py" validate-report $out
```

No home scan, provider launch, resume command, network call or Wake execution.
The report replays the frozen suite; a tampered or stale report fails
validation. Failed cases are reported with `status: failed`, never relabeled.

## What is probed

- **Cursor IDE (`cursorDiskKV`)**: `composerData:<id>` metadata and
  `bubbleId:<composerId>:<bubbleId>` bodies; message order follows
  `fullConversationHeadersOnly`, never KV key or insertion order. Missing,
  NULL, malformed-JSON, wrong-shape and invalid-UTF-8 bubbles keep an explicit
  slot and state instead of silently disappearing. Tool inputs prefer
  `rawArgs`, then `params`, accept object or JSON-string forms, and never
  execute input.
- **Hermes (`state.db` + `profiles/<name>/state.db`)**: sessions/messages
  columns with REAL unix-second timestamps ordered by `timestamp, id`; profile
  namespaces separate identical native session IDs; compact
  `{name,arguments}` and OpenAI `{id,function:{...}}` tool-call shapes both
  decode; text is never trimmed and NULL stays NULL.

## Bounded and honest by construction

`Limits` caps file bytes, logical bytes per cell/table, rows, columns, tool
calls and SQLite progress steps before anything is materialized. Every probe
runs inside ONE pinned read transaction; live-writer tests assert a concurrent
commit neither changes the observed snapshot nor mutates source DB/WAL/SHM
bytes or metadata. Ambiguous or unmatched tool associations stay
`authoritative: false` with candidate lists.

## Interpretation limits

- Python `sqlite3` version is probe metadata, not product runtime evidence.
- Hermes column layout has an independent upstream snapshot (schema version
  30, see `../../upstream-evidence.md`); Cursor's private storage has no
  official version contract located, so only the pinned Wake implementation
  and these synthetic probes support it.
- Passing probes do not certify compatibility with any shipped provider
  version, do not prove StableId/native-byte identity, and do not authorize
  resume support.
