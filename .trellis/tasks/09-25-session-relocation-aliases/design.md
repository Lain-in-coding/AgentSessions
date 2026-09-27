# Explicit Installation Relocation Design

Status: approved and in implementation, 2026-09-27. Baseline `1d24c07`.

## 1. Identity model

An installation has a persisted opaque registry identity and an immutable
namespace input used by `StableId::native_session_scoped`. Existing source
registrations retain the exact string produced by the legacy
`installation_namespace` function, including historical casing. New
installations receive an opaque allocated namespace that is persisted with
the source commit, not a freshly derived physical path on each scan.

Physical current/retired roots are location records. They point directly to an
installation; they never point to another alias. Relocation replaces a current
root while retaining the immutable namespace. No canonical ID is rewritten,
so old canonical lookups stay direct and permanent. A general `id_alias`
rewrite table is not needed for this first release.

A selected root may contain several legacy namespace groups, particularly for
providers whose old grouping used each source's parent directory. Preserve all
of those groups and their relative subpaths; never combine them merely because
they share the selected provider or a native Session ID.

No-native Session derivation remains content/document based. Missing or
source-local observations do not become native IDs during registry bootstrap.

## 2. Schema v17 to v18

Add registry structures without renaming or deleting existing canonical rows:

- `installation_namespaces`: registry ID, provider, immutable namespace input,
  allocation origin (`legacy-v1` or `allocated-v1`) and creation metadata.
- `installation_locations`: provider, normalized root key, private root locator,
  namespace reference, current/retired state and nullable retirement expiry.
  Unique current ownership prevents an occupied root from being reassigned.
- `source_installations`: source locator to namespace binding.
- `installation_relocations`: committed operation digest, generations and
  private mapping/aggregate receipt needed for idempotency and audit.

Extend the existing durable intent manifest for namespace/location assignments
and relocation operations. Existing intent content remains immutable; old
interrupted intents continue to be aborted through writer recovery.

Bootstrap v17 registrations from recorded provider/source provenance and exact
legacy derivation. Do not guess missing provider facts or collapse conflicting
legacy namespaces. Unverifiable sources remain readable as catalog history but
are ineligible for relocation until provenance is resolved by an explicit
normal scan. Migration and all bootstrap writes occur under the writer lease.
Current read-open rules remain intact; `index rebuild` is the existing explicit
write path for upgrading an older catalog before using a v18 preview.

## 3. Proposed command contract

```text
asg --db <catalog> relocate --provider <provider> --from <old-root> --to <new-root>
asg --db <catalog> relocate --provider <provider> --from <old-root> --to <new-root> --apply --plan <opaque-plan> --backup <new-backup-file> [--alias-ttl-days 90]
```

- Preview is the default. Match the old absolute root against persisted locator syntax; it need not
  still exist. Require an absolute host-local destination, the shared provider
  registry/alias vocabulary, and well-formed scalar arguments. The command does
  not create a missing catalog and is not a provider-file move command.
- Applying requires the same mapping/retention inputs as preview, a current
  matching plan and a new backup destination. `--alias-ttl-days` is also accepted
  on preview; default 90, range 1..365. Never overwrite a backup, catalog or
  provider source through this option.
- The plan binds schema, catalog generation, normalized mapping, namespace
  ownership, source fingerprints and retention policy. Use a versioned,
  domain-separated integrity digest and a 15-minute injected-clock lifetime.
  Opaque claims contain digests/versions/counts, not reversible private roots.
  The plan is a freshness check, not an authentication boundary.
- Results use the existing Robot envelope and a bounded shape: status
  (`planned`, `applied`, `unchanged`), plan when relevant, source/Session/
  installation counts, and generation information. Do not echo roots, native
  IDs, backup paths or source content in diagnostics/protocol output.
- CLI only. MCP and Web remain read-only; new CLI command/help and capability
  descriptions must not accidentally advertise a remote mutation tool.

## 4. End-to-end flow

1. Preview opens the catalog read-only, resolves the provider and selected
   registered groups, and constructs a component-aware old/new source mapping.
2. Read mapped destination files using existing bounded read-only source
   snapshots. Require complete, verifiable source provenance and matching
   opaque fingerprints, including logical SQLite WAL fingerprints. Preview
   fails for missing/changed/unverifiable files or target ownership conflicts.
3. Apply obtains the writer lease, verifies generation/plan lifetime, and
   revalidates the mapping and captured snapshots. A stale preview cannot
   authorize a different source set.
4. Create and verify a consistent SQLite backup using the Backup API. Backup
   failure leaves the live catalog untouched. Do not copy only the main file
   of a live WAL catalog or checkpoint a provider source.
5. Begin the relocation through the existing durable intent mechanism. Its
   phase-2 transaction verifies the manifest/generation, moves all live source
   locator keys, changes registry locations/aliases, records the operation,
   advances generation once and activates the intent atomically.
6. Recheck source snapshot validity before activation. A source change or SQL
   failure rolls back live state; a durable building intent may remain for
   normal writer recovery to abort, as with other current write operations.
7. Subsequent ingest/sync resolves persisted namespaces before staging native
   Session IDs. Source commits persist any new registration in the same
   authoritative write. Explicit discovery includes current registered roots
   and does not silently reactivate retired locations.

The seven existing live locator tables are `source_scans`, `source_membership`,
`source_placement_membership`, `source_relation_scans`,
`source_session_resume_claims`, `tool_activity_membership` and
`usage_event_membership`. Include `source_installations` and registry records
in the same transaction. Add a regression inventory so a future live
`source_path` table cannot be omitted silently. Do not mutate historical outbox
paths or observed original CWD/native-ID values. Keep derived Session search
projections consistent with unchanged authoritative claims.

## 5. Boundaries and reuse

- Domain keeps its existing stable-ID derivation. Installation registry facts
  and relocation data use backend-independent port DTOs.
- Application owns plan validation, matching, lifetime/idempotency policy and
  canonical error classification through ports. Filesystem snapshot and SQL
  details stay in their current adapters/composition helpers.
- SQLite owns registry persistence, schema migration, backup, durable intents
  and atomic key changes. No SQL or copied migration logic in protocol handlers.
- CLI owns argument parsing and concrete wiring. Replace path derivation in
  production staging with resolved namespace injection; retain the old helper
  only for verified legacy bootstrap/tests. Reuse provider canonicalization,
  response/error projection, source snapshots and generation logic.
- Stream source enumeration/digesting in bounded batches; never load transcript
  payloads or all embeddings merely to relocate their source locators.

## 6. Conflict and path rules

- Different installations with equal native Session IDs remain distinct.
  Occupied destination roots or conflicting source/namespace bindings fail;
  this command does not reconcile already separately indexed copies.
- Source/target ancestor overlap and conflicting normalized mappings fail.
  Flat aliases prevent chained lookup cycles. A reverse move is an explicit
  fresh operation, not recursive alias rewriting.
- Keep the recorded path flavor when matching an old root, and use host path
  semantics for the destination. Cover Windows drive/separator/case, POSIX
  components and Unicode roots, but do not normalize the frozen
  namespace seed or native IDs. If normalization would merge existing distinct
  ownership, refuse it rather than choosing one owner.
- Equivalent casing/separators or a repeated already-applied mapping is a no-op
  when ownership agrees; it does not create duplicate source claims or advance
  generation. Changed ownership requires a fresh plan and cannot reuse a receipt.
- Retired location aliases last 90 days by default (1..365 explicitly selectable).
  They are compatibility/ownership records, not permission to scan the old root
  as the moved installation. Expiry never deletes a namespace, canonical entity
  or canonical-ID lookup. No automatic ambiguous identity reconstruction.

## 7. Validation and rollback

| Condition | Outcome |
| --- | --- |
| Unknown provider, invalid roots/TTL, occupied/ambiguous mapping | Bounded invalid request; no live writes |
| Missing/unreadable source or backup failure | Source/backend I/O category; no partial relocation |
| Changed source fingerprint | `source_changed`; no partial relocation |
| Plan generation changed | `generation_mismatch` |
| Invalid/expired/mismatched plan | Explicit invalid request; never silently replan/apply |
| Writer contention | `writer_busy` |
| Older schema on preview | `schema_incompatible`; explicit leased upgrade required |
| Injected phase-2 failure | All live tables/generation unchanged; recoverable intent bookkeeping only |
| Same applied mapping and unchanged ownership | `unchanged`; no generation advance |

Use only synthetic fixtures. Assert complete pre/post table/identity/claim
snapshots, not only row counts. Cover source deletion after relocation,
subsequent incremental sync, multi-session SQLite sources, interrupted writes,
backup failures, case/Unicode roots, alias expiry and no-create preview.

Restore the verified pre-relocation backup with writers stopped to roll back
catalog state. Restore the matching source layout (or keep the restored
catalog read-only until a new explicit mapping is applied); the tool does not
undo filesystem moves. Downgrading to a pre-v18 binary additionally requires a
compatible pre-upgrade backup; no destructive down-migration is proposed.
