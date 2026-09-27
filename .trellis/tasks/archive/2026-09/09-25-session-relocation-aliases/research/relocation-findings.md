# Relocation Evidence (2026-09-27)

Baseline: `1d24c07` (product repair commit `a20e8ab`). Planning only; no product
source or real provider data was changed/read for this investigation.

## Identity facts

- `crates/agent-session-grep-cli/src/lib.rs:3108` owns
  `installation_namespace(path, provider_id)`. Claude/Codex use the path through
  their `.claude`/`.codex` marker; other providers use the source's parent.
  This is a legacy compatibility function, not a relocation-safe registry.
- `crates/agent-session-grep-cli/src/lib.rs:3605` passes that value to
  `StableId::native_session_scoped` for report-level and message-level native
  Session observations.
- `crates/agent-session-grep-domain/src/ids.rs:235` hashes provider, namespace
  and native Session ID. Preserving the namespace input preserves the old
  `ses_v1` exactly; the Domain algorithm need not be replaced.
- The no-native fallback in the same staging function depends on the content
  document and, for explicit source-local sessions, the provider/variant/key.
  It does not depend on the source path. Preserve this distinction: a missing
  native observation must not become a fabricated native resume identity.
- Message/Document/placement identities are not to be rekeyed by relocation.
  Existing Session IDs remain direct canonical keys, including after any
  location-alias compatibility window ends.

## Persistence and write boundaries

Current schema is v17. Seven live tables store source locators:

| Table | Source-locator role |
| --- | --- |
| `source_scans` | Successful scan, fingerprint, length, provider and parser version |
| `source_membership` | Entity/document claims |
| `source_placement_membership` | Placement claims |
| `source_relation_scans` | Relation completeness |
| `source_session_resume_claims` | Per-Session native ID/CWD observations |
| `tool_activity_membership` | Activity claims |
| `usage_event_membership` | Usage claims |

A rename must cover all seven and the new registry binding in one committed
operation. Immutable historical outbox manifests must not be rewritten.
Resume observations retain their recorded values: moving a transcript directory
does not prove that its original working directory changed.

`SqliteStore::open` is read-only. Writer lease, durable intent validation,
generation CAS and transactional activation already exist. `index rebuild`
is the existing explicit write/upgrade command; there is no current
`maintenance` CLI command group. The new command should therefore be a
single top-level `relocate` command, not an invented nested maintenance API.

## Governing constraints

RFC-0001 sections 5/5.1 require persistent installation identity, unchanged
Session/Message identity on relocation, explicit path semantics and honest
registry-loss behavior. Section 7 asks for a defined alias compatibility
window. The existing task additionally requires explicit old/new mappings,
conflict refusal, writer-leased atomic updates and synthetic tests.

The first release can preserve canonical IDs through flat installation-location
aliases. A general Session-ID rewrite/merge table is unnecessary when canonical
IDs do not change. Occupied or ambiguous destination installations must fail
closed; automatic reconciliation of already separately indexed copies is a
different operation and is not implied by a directory mapping.
