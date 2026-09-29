# Research: v6 re-ingest identity proof and Windows source lookup

- Query: Explain the Ubuntu failure of `migrated_v6_catalog_stays_readable_until_complete_reingest_enables_context`, trace fixture identity through resume claims and namespace registration, and identify the minimum contract-preserving correction.
- Scope: internal; read-only analysis of the relocation-ci-compatibility worktree.
- Date: 2026-09-28

## Findings

### 1. The failing positive fixture asks for a forbidden identity replacement

The fixture creates `legacy_session_wire = format!("ses_v1_{session_native}")` at `crates/agent-session-grep-cli/tests/e2e.rs:299`. `create_v6_catalog` stores that exact ID in catalog payloads and `source_membership` (`:178`, `:208`, `:219`). Its `fts_ids` table exists but receives no identity sidecar rows (`:158`, `:208`).

The source JSON does contain the correct native `sessionId` (`e2e.rs:304`). Re-ingest does not lose the native ID or omit resume claims:

1. `ingest_file` normalizes the input and stages the provider (`crates/agent-session-grep-cli/src/lib.rs:4372`, `:4387`).
2. It asks `resolve_or_allocate_installation_namespace` for the historical namespace before building entities (`:4395`).
3. `staged_to_source_with_provider_namespace` generates a Native canonical Session using provider, persisted namespace, and native session ID (`:3943`, `:3948`, `:3994`).
4. `SourceResumeClaim::from_observation` associates the resolved observation with that same canonical Session (`:4022`, `:4029`).
5. `StableId::native_session_scoped` hashes those three facts; it never produces a raw native UUID with a prefix (`crates/agent-session-grep-domain/src/ids.rs:216`, `:235`).

On the Unix path used in this fixture, namespace lookup recognizes the already-scanned source (`crates/agent-session-grep-adapters-sqlite/src/relocation.rs:309`). `legacy_assignment` receives the staged provider, reconstructs the old path-derived seed, and sees no stored Native sidecar that could contradict it (`:176`, `:205`, `:227`). The no-sidecar fallback is specifically **Unstable**, not Reconstructed (`crates/agent-session-grep-adapters-sqlite/src/lib.rs:3041`; `crates/agent-session-grep-domain/src/ids.rs:278`).

Commit validates the provisional legacy assignment before any source activation (`crates/agent-session-grep-adapters-sqlite/src/lib.rs:3345`; `relocation.rs:513`). In `validate_legacy_source_proof`:

- `existing_sessions = { ses_v1_<raw native UUID> }` from persisted membership (`relocation.rs:555`).
- `native_sessions = { ses_v1_<scoped digest> }` from staged Native entities (`:565`).
- `claimed_sessions = { ses_v1_<same scoped digest> }` from resolved claims (`:573`).
- Native coverage succeeds, but `existing_sessions != claimed_sessions` fails (`:582`). This yields the exact CI error at `:586`.

The test then explicitly expects `assert_ne!(session_wire, legacy_session_wire)` (`e2e.rs:385`). That expectation conflicts with the current relocation contract: a re-scan must not silently replace an existing canonical Session. This is a stale positive-fixture expectation, not missing `resume_claims`. Do not weaken the equality check or reverse-engineer a native ID from a `ses_v1` suffix.

### 2. Why Windows passed: unbound old locators can evade the proof lookup

This is an independent production-path concern, not evidence that the fixture is valid on Windows.

- The fixture stores `PathBuf::to_string_lossy()` unchanged in both `source_scans` and `source_membership` (`e2e.rs:310`, `:219`, `:227`). Windows produces backslashes and can retain an uppercase drive.
- `source_input_path` immediately returns `source_path_identity(path)` for absolute inputs (`crates/agent-session-grep-cli/src/lib.rs:3493`). On Windows this rewrites backslashes and drive case (`:3476`). The existing-source protection below that return is not consulted.
- `resolve_or_allocate_installation_namespace` tests `source_scans.source_path = ?1` using exact text (`relocation.rs:309`). The normalized locator therefore misses an unbound raw Windows locator.
- A migrated v6 source has NULL provider metadata until a new scan (`crates/agent-session-grep-adapters-sqlite/src/lib.rs:2028`). With no resume observations, `unbound_legacy_root` does not select it (`relocation.rs:1705`). The v18 migration also cannot bind provider-less provenance (`:184`, `:195`).
- The code can consequently select a new `allocated-v1` namespace for the same file (`relocation.rs:413`, `:425`) and skip the `legacy-v1` proof guard (`:513`). `canonicalize_source_batches` cannot recover the old locator: it consults only `source_installations`, which is empty for this unbound source (`:1671`).

This static trace explains a Windows pass with a changed Session ID and a retained old source record. It also identifies a likely data-integrity defect for real unbound pre-v18 Windows catalogs. It was not reproduced by running a binary in this research task. The parent should explicitly validate this boundary; normalizing only the fixture makes CI honest but does not repair old on-disk locators.

### 3. Minimum correction to the positive migration test

For the scenario that claims successful identity-preserving re-ingest:

1. Produce the same portable source locator that the CLI will use, including Windows separator and drive normalization, before seeding the database.
2. Compute the legacy seed with `agent_session_grep_application::relocation::legacy_installation_namespace(source_path, "claude-code")` and the session wire ID with `StableId::native_session_scoped` using that seed and `session_native`.
3. Keep the v6 tables and absence of relation completeness/resume claims. Those are the migration behavior the test is meant to exercise.
4. Replace the final Session inequality with equality. Document identity may still change after the complete scan; canonical Session identity must remain stable.
5. Keep a separate negative case for a legacy ID that cannot be reproduced from current native proof: re-ingest must reject while old catalog reads and identities remain unchanged. This protects existing raw-ID catalogs honestly rather than pretending every v6 shape can be automatically upgraded.

For the Windows unbound-locator issue, a product correction must resolve or reject an existing lexical-equivalent source before allocating a namespace, preserve its exact frozen namespace seed, and keep ambiguous matches fail-closed. Simply lowercasing historical seeds or rewriting every old source row is unsafe. That change needs its own implementation scope and regression evidence.

## Files found

- `crates/agent-session-grep-cli/tests/e2e.rs` — hand-built v6 catalog, source fixture, and obsolete Session inequality.
- `crates/agent-session-grep-cli/src/lib.rs` — platform input normalization, provider staging, namespaced Session and resume-claim creation.
- `crates/agent-session-grep-adapters-sqlite/src/relocation.rs` — legacy provenance matching, namespace allocation, membership proof, and canonical source lookup.
- `crates/agent-session-grep-adapters-sqlite/src/lib.rs` — NULL legacy provider migration, identity-sidecar lookup, and pre-commit validation.
- `crates/agent-session-grep-domain/src/ids.rs` — namespaced canonical Session derivation and honest Unstable wire parsing.
- `crates/agent-session-grep-application/src/relocation.rs` — legacy seed and host-independent lexical path keys.
- `crates/agent-session-grep-adapters-sqlite/src/relocation/tests.rs:1938` — regression proving incomplete or wrong native-session claims fail without table/intent mutation.

## Related specs

- `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md:102` — migration does not fabricate relations; context remains incompatible until complete re-ingest.
- `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md:130` — wire parsing does not recover stable identity metadata.
- `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md:223` — exact frozen namespace, no historical rekey, no registration side effects on failed source commits.
- `.trellis/spec/agentsessions-cli/backend/index.md` — complete provider parse reports and source-staging identity rules.

## External references

None required. The conclusion follows from local project code, tests, and contracts; no private paths, source code, or identifiers were submitted to public search.

## Caveats / Not Found

- No Cargo commands, test binaries, Git operations, source edits, spec edits, or task metadata edits were performed. Runtime confirmation belongs to the parent/implement agent.
- CodeGraph was consulted first at the indexed root, but its root code is not the isolated worktree. All decisive anchors above were read from the requested worktree; source line numbers may move as the implement agent edits it.
- The initial native research hook pointed at the older root review task. The parent corrected the session pointer to the active worktree task and moved this newly generated CI audit into its research directory; existing root review artifacts remain untouched. This audit concerns only the current CI failure and its directly related identity-proof boundary.
- Role isolation forbids loading `implement.jsonl`/`check.jsonl`; relevant task PRD/design/implementation notes, workflow, specs, and code were read directly instead.
## Follow-up: minimal locator-resolution layer and API

- Query: Correctly resolve equivalent Windows absolute locators for unbound pre-v18 sources, including normalized input and ambiguous legacy rows, while preserving the registered fast path.
- Scope: internal, read-only API/design research; no implementation or validation runs.
- Date: 2026-09-28

### Guard ordering alone is insufficient

Moving the `source_fingerprints` check above the absolute-path return in `source_input_path` catches only an input that exactly spells the stored locator (`cli/src/lib.rs:3491`). `source_fingerprints` uses `WHERE source_path = ?1` (`adapters-sqlite/src/lib.rs:2914`); a user who supplies an already-normalized locator still misses the raw historical row. Returning an error on every known source is also an unnecessary regression: the safe operation is to retain the original locator and its historical seed, then perform the existing identity proof.

Do not fix this by broadening `source_fingerprints` itself. Its output keys are caller-requested paths, and other callers also key counts, relation-recovery sets, and source batches by locator. Returning an aliased fingerprint without returning the original locator leaves later membership/proof lookups inconsistent.

### Recommended smallest coherent boundary

Add one concrete SQLite-adapter lookup API, for example `existing_source_locator(&self, input: &str) -> PortResult<Option<String>>`, backed by a shared private helper in `relocation.rs`. Its result is the **exact stored locator**, not a normalized display path, namespace, or inferred provider. A batch form can share the same rules. No new domain/ports trait is needed: the CLI already composes `SqliteStore` and uses its concrete source metadata APIs.

Rules:

1. Parse only a valid lexical absolute input with the existing application `normalize_absolute_path`. Do not resolve symlinks, guess a working directory for stored relative paths, or infer provider identity from a path. `AbsolutePath::key` already handles Windows ASCII case/separators and leaves POSIX case significant (`application/src/relocation.rs:389`).
2. Preserve the indexed registered path: look up `source_installations.source_key` and return its authoritative stored `source_path` for the current binding. That key is unique (`adapters-sqlite/src/relocation.rs:101`); mirror `assignment_for_source`'s current-location semantics (`:276`). Registered normal calls should not enumerate `source_scans`.
3. Without a registered binding, examine **unbound** `source_scans` locators and compare their lexical keys with the requested key. Include NULL `provider_id`, NULL fingerprints, and rows that still own membership. Do not use `source_paths_for_provider`: it deliberately excludes precisely these unknown-provider rows (`adapters-sqlite/src/lib.rs:2958`). Do not ignore a known scan merely because its file is presently empty.
4. Zero matching valid absolute locators means genuinely unknown at this boundary. Exactly one returns its spelling unchanged. Two or more distinct stored locators with the same key must return a bounded `InvalidRequest`; do not prefer an exact-text row, first row, provider hint, or matching native ID. Exact text is not proof that an equivalent second legacy row has the same identity.
5. Unknown/relative legacy locators remain readable but cannot be matched by filesystem or suffix guessing. Keep the existing exact legacy-relative rejection behavior in the CLI before canonicalizing a genuinely new relative input.

The helper does not register anything. With its returned old spelling, the existing resolver derives the **old case-sensitive legacy seed**, stages current claims, and checks old membership. The lexical key is for matching only; never substitute it for `legacy_installation_namespace(stored_locator, provider)`. Lowercasing the old drive or path components before seed derivation can rekey a historical Session even when the lookup itself is correct.

### Wire the lookup before identity creation, not only at commit

The CLI must use the returned original locator before capture/cache/probe/namespace/staging, and must not pass that returned value through `source_path_identity` again. Only genuinely new inputs use the fresh-source normalization behavior.

Cover the actual ingress paths:

- `ingest_file` uses `source_input_path` (`cli/src/lib.rs:4372`).
- Explicit `sync_files` calls it once per supplied path (`:4524`).
- `sync_discover` constructs source paths and calls the inner sync path directly; it does **not** call `source_input_path` (`:3652`, `:3707`). Resolve discovered locators before building the keyed provider/diff/recovery maps, rather than changing batch paths after those maps exist.

The namespace allocator is also the storage safety boundary. It must not treat an equivalent known unbound locator as a new source simply because a caller bypassed the CLI resolver. Reuse the same matcher before the `scanned`/allocation decision in `resolve_or_allocate_installation_namespace` (`adapters-sqlite/src/relocation.rs:309`, `:413`). Two compatible implementations are possible:

- Require an unbound caller to supply the returned stored locator; reject an equivalent but differently spelled locator before making a pending namespace reservation. This preserves the current `PortResult<String>` namespace API and is the smallest change once CLI callers resolve first.
- Return locator and namespace together and require staging to use both. This is a broader signature change; it should be chosen only if preserving transparent alias handling for direct adapter callers is necessary.

Do not silently choose the historical namespace while leaving `SourceBatch.source_path` on the input alias. That can make `validate_legacy_source_proof` read an empty old membership set. Likewise, extending only `canonicalize_source_batches` (`:1671`) is too late: an opaque namespace may already have been allocated and used to derive Session IDs. Locator resolution and the allocation guard must agree before ID derivation.

### Bounded work without schema changes

There is no normalized-key index on unbound `source_scans`; its primary key is the raw locator. A complete cold equivalence check therefore requires inspecting candidate legacy rows unless a request-local index has already been built. SQL `lower(replace(source_path, ...))` does not turn that existing raw-path primary key into a normalized-key index and does not reproduce the full Windows/UNC parsing contract.

For the smallest correctness patch, keep the registered point-lookup fast path and use a streamed, bounded-memory legacy slow path only for unregistered inputs. Stop with an error as soon as a second matching locator is proven; never stop at a fixed row limit and return `None`, because unseen aliases could then be registered as new identities. An anti-join restricts returned candidates to unbound rows, but the database may still inspect the whole source table; do not claim this is an indexed O(1) lookup.

For explicit/discovered bulk sync, batch the unresolved input keys and traverse candidate legacy rows once for the request, retaining only up to two matches per requested key. Reuse that resolved set during namespace checks; merely batching the CLI pre-pass while independently rescanning inside every allocator call does not remove repeated work. A short-lived lookup context scoped to the writer-leased sync operation is sufficient; avoid a permanent cache with hidden invalidation rules. Namespace checks must still consult current registered bindings/pending assignments before using request-local legacy information.

This gives an indexed registered hot path and at most one legacy traversal per batch. If the immediate patch keeps scalar cold lookups for smaller scope, record the cold-batch cost explicitly rather than describing streaming as a time-complexity fix.

### Focused verification requirements for the implement/check agents

- Store a Windows legacy source with raw backslashes and uppercase drive, request it both verbatim and with normalized separators/drive/component case, and confirm lookup returns the exact old locator and seed.
- For reproducible historical Session IDs, both spellings should reach the same proof and preserve IDs/membership. For non-reproducible IDs, both should fail before registry/intent/generation mutation.
- Two unbound absolute rows with one lexical key must fail even when one row exactly matches the user's input; preserve both rows and their existing IDs.
- Include NULL provider metadata and raw Windows locators in adapter tests that run on every host. Filesystem CLI reproduction can remain Windows-specific.
- Preserve the registered lookup behavior and retired-root rejection; new unrelated absolute inputs remain eligible for a new namespace.
- Keep POSIX case-sensitive paths distinct, and do not infer equivalence for legacy relative paths or symlink spellings.
- Check explicit multi-file sync and discovery, not only `ingest`, because discovery bypasses the current helper.

### Follow-up caveats

The API names above are proposed, not existing methods. All anchors refer to the isolated worktree read during this audit and may move with concurrent edits. No implementation, Cargo command, Git operation, or source mutation was performed. The parent corrected the active task and moved this report; this follow-up appends only to that existing active-task report.