# Provider-scoped session identity — implementation plan (schema v13 proposal)

> Owner: `08-15-unified-release-contract` (per `design.md` "Deferred: provider-scoped
> session identity — migration plan (design only)").
> Status: **proposal, not executed.** This slice ships this plan plus domain-only
> tests; it touches no SQLite migration and no production wire format.
> Verified against worktree @ `f7e2a49` (`docs(handoff): add paste-ready Zcode
> startup prompt`). All anchors below were read, not assumed.

Companion artifact (this slice): pure-function tests in
`crates/agent-session-grep-domain/src/ids.rs` (tests module, section
"provider-scoped v2 migration plan mirror") pinning the derivation,
namespace-key normalization, and alias-resolution properties below.

---

## 1. Baseline (what exists today at f7e2a49)

- The derivation side already landed: `SessionIdentityNamespace` +
  `StableId::native_session_scoped` (`crates/agent-session-grep-domain/src/ids.rs:92-97`,
  `ids.rs:204-217`) hash `(provider_id, installation_namespace, native_session_id)`
  but still emit the `ses_v1_` wire prefix (`ids.rs:191`). The composition root
  feeds it the path-derived namespace (`crates/agent-session-grep-cli/src/main.rs:3176-3190`).
- Known debt (named in `design.md:121-126`): no persisted registry, no
  `id_alias`, no Windows path-case normalization, resume claims not keyed by
  namespace id.
- Catalog schema is at **v12** (`crates/agent-session-grep-adapters-sqlite/src/lib.rs:5438`);
  migrations are `PRAGMA user_version`-gated sequential steps
  (`lib.rs:1327-1342`, dispatch chain `lib.rs:1492-1509`). A newer DB is refused
  by older binaries (`lib.rs:1336-1342`).
- Resume claims: `source_session_resume_claims` keyed `(source_path, session_id)`
  with `pair_observed` CHECK (`lib.rs:1682-1696`), written in the source
  replacement transaction (`lib.rs:4789-4814`), read back fail-closed on any
  conflicting claim (`lib.rs:4279-4315`, `crates/agent-session-grep-ports/src/lib.rs:834-856`,
  `ports/src/lib.rs:897-928`). Providers set `multi_session` and fold Resume
  metadata to `Ambiguous` when one source holds several native Session ids
  (`crates/agent-session-grep-provider-claude/src/lib.rs:849-857`,
  `crates/agent-session-grep-provider-codex/src/lib.rs:738-741`).

## 2. Wire format proposal: `ses_v2_*` (scoped canonical Session ids)

New wire prefix `ses_v2_`; digest = truncated BLAKE3 (same 32 hex chars as v1,
`ids.rs:113-117`) over length-prefixed facts `(provider_id, namespace_key,
native_session_id)`, domain-separated by the `ses_v2_` prefix itself. This
mirrors `StableId::derive` framing exactly (`ids.rs:157-183`) with a different
domain separator, so identical facts can never collide with any `ses_v1_`
digest — the prefix bump IS the format version bump that `IdKind` already
documents (`ids.rs:31-33`: "a changed hashing scheme bumps to `_v2_` and both
can coexist during migration").

Rules:

1. `namespace_key` is the registry key, not a raw path: composition-root rule
   (last provider data-root marker `.claude`/`.codex`, `main.rs:2829-2850`)
   with Windows path-case normalization applied (separators folded to `/`,
   ASCII-lowercased whole key on Windows). This reuses and tightens the
   existing `source_path_identity` precedent (`main.rs:2877-2888`). Open
   decision O6: whole-key vs drive-letter-only lowercasing.
2. Only **native** sessions get `ses_v2_`. The `Reconstructed` document-derived
   fallback (`main.rs:3185-3189`) already scopes by `(provider, variant,
   fingerprint)` via the document id (`main.rs:3162-3171`) and stays `ses_v1_`
   forever.
3. The wire contains neither the native id nor the namespace in recoverable
   form (digest-only suffix); the no-reverse-derivation contract at
   `ids.rs:195-199` carries over unchanged.
4. `StableId::from_wire` (`ids.rs:247-266`) and `validate` (`ids.rs:268-280`)
   must become version-aware (prefix set per kind). Today `kind.prefix()`
   returns exactly one string (`ids.rs:46-56`) — see O5.

## 3. Schema v13 DDL proposal (additive, one transaction)

Follows the v9/v11/v12 additive pattern (`lib.rs:1709-1738`, `lib.rs:1770-1782`,
`lib.rs:1793-1814`): create tables + indexes, commit `PRAGMA user_version = 13`
in the same transaction. No `UPDATE`/`DELETE` on any existing table.

```sql
CREATE TABLE IF NOT EXISTS installation_namespaces (
    namespace_key          TEXT PRIMARY KEY,   -- "<provider_id>:<normalized root>"
    provider_id            TEXT NOT NULL,
    installation_path_hash TEXT NOT NULL,      -- BLAKE3 hex of root as ingested (opaque)
    created_at             INTEGER NOT NULL    -- unix_ms() values, per repo convention
);
CREATE TABLE IF NOT EXISTS id_alias (
    old_id         TEXT PRIMARY KEY,           -- legacy ses_v1_* wire id
    new_id         TEXT NOT NULL,              -- ses_v2_* wire id, or old_id (identity marker)
    created_at_ms  INTEGER NOT NULL,
    expires_at_ms  INTEGER NOT NULL,
    CHECK(expires_at_ms > created_at_ms),
    CHECK(old_id LIKE 'ses_v1_%'),
    CHECK(new_id LIKE 'ses_v2_%' OR new_id = old_id)
);
CREATE INDEX IF NOT EXISTS id_alias_new ON id_alias(new_id);
```

Field names follow `design.md:131-134` (target contract #2). `created_at` keeps
the design's name; values are `unix_ms()` to match `index_batches.created_at_ms`
convention (`lib.rs:1388`). The `old_id = new_id` identity row encodes the
design's "sources without a resolvable root keep `ses_v1_*` and an alias row
with a short TTL" (`design.md:146-147`): a pending marker that resolution
treats as "not rewritten yet" (see §6) so the next complete re-scan retries it.

v13 does **not** re-key `source_session_resume_claims` (design target #4's
"claims keyed by `(namespace_registry_id, session_id)`"): that is a primary-key
change requiring a table rebuild, not an additive step — open decision O1.

## 4. Migration invariants

**I1 — Existing `ses_v1_*` ids are never rewritten by the v13 step.**
The migration step is DDL-only. All `ses_v1_*` rows (native-adopted legacy,
v1 scoped digests, reconstructed) stay byte-identical. Rewrites happen only
inside a later, CAS-guarded, scan-driven backfill (see §5), and only for
sources that pass the gating rules of §6.

**I2 — Catalog rebuild invariant.**
`rebuild_index` treats `catalog` as the authoritative entity set and
re-projects FTS/session FTS from it (`lib.rs:5259-5260`, `lib.rs:5305-5308`).
Consequences the executing task must honor:

- Backfill must write the new session row into `catalog` in the same
  transaction as every other rewrite; a FTS-only or claims-only rewrite would
  be silently reverted by the next `rebuild_index`.
- The rebuild scan `SELECT id FROM catalog WHERE id LIKE 'ses_v1_%'`
  (`lib.rs:4385`) and `list_filtered`'s `kind.prefix()` LIKE (`lib.rs:1958-1962`)
  must become prefix-set scans once `ses_v2_` rows can exist; same for the
  application fake `starts_with("ses_v1_")` (`crates/agent-session-grep-application/src/lib.rs:2988`),
  the handoff-pack `^ses_v1_` schema patterns (`schemas/handoff/v1/pack.schema.json:129,154,173,196`),
  and the `GetSessionResume` kind-check wording (`application/src/lib.rs:1940-1947`).
- `fts_ids` identity fidelity (`lib.rs:5251-5254`) carries over: the sidecar
  stores the full id JSON; v2 rows store v2 wires. Session entities have no
  message-FTS rows (`lib.rs:1523-1524`); their projection is
  `session_fts`/`session_fts_ids` (v11, `lib.rs:1770-1782`), rebuilt per
  session in the same transaction.

**I3 — Version guard.**
A v12 binary opening a v13 DB refuses cleanly (`lib.rs:1336-1342`); it never
misreads. Downgrade = drop the two additive tables (or stay on v13 binary);
no data corruption path exists (§8).

## 5. Backfill order (scan-driven, per source, single transaction)

Trigger: **next complete re-scan** of a source (complete = `relation_complete`
gating as in `lib.rs:4773-4788`; incomplete scans skip backfill, mirroring
tombstone gating). No open-driven data migration — same rationale as v9's
"old rows stay NULL until rescanned" (`lib.rs:1710-1714`) and v11's
"rebuild or next affected source commit populates" (`lib.rs:1768-1769`).

Per re-scanned source, inside the existing source replacement transaction
(`lib.rs:4740-4814`) which already runs under the WriterLease (single writer,
`crates/agent-session-grep-adapters-sqlite/src/cas.rs:1-4`):

1. Derive `namespace_key` (composition-root rule + Windows normalization,
   §2.1); `INSERT OR IGNORE INTO installation_namespaces` — first writer
   wins; an existing key is never mutated (registry stability, RFC-0001
   `docs/architecture/RFC-0001-canonical-model-and-stable-id.md:206-210`).
2. Gate (§6): if the source is multi-session (claim `ambiguous` /
   `multi_session`) or the conflict set has ≥2 distinct targets → **skip**.
   Keep `ses_v1_` rows as-is, write no alias, emit a diagnostic. Disclose
   nothing about the conflicting values (the existing fail-closed posture,
   `ports/src/lib.rs:905-916`, `lib.rs:4279-4315`).
3. Compute the candidate `ses_v2_` id from the re-scan's
   `ParseReport.session_native_id` (`main.rs:3177-3184`) + `namespace_key`.
   Never recover the native id from a digest wire suffix (`ids.rs:195-199`);
   adopted-form legacy suffixes are the native id by construction
   (`ids.rs:143-155`) but backfill does not need them — the re-scan is
   authoritative.
4. Rewrite, all in the same transaction:
   - `catalog`: insert `(ses_v2_, payload)`; delete the old `ses_v1_` row
     only when no placements/claims/membership reference it anymore
     (else keep — coexistence, design target #1). Open decision O4.
   - `message_placements`: `session_id` → new, and re-derive `placement_id`
     via `PlacementId::derive` from `(new session, document, message,
     ordinal)` (`ids.rs:300-325`, table columns `lib.rs:1588-1602`). **This
     cascade is mandatory**: placement identity hashes the session wire, so a
     session rewrite changes every placement id of that session.
   - `message_edges.child_placement_id` → re-derived placement ids
     (`lib.rs:1603-1608`); `parent_message_id` (a message id) is untouched.
   - `source_placement_membership.placement_id` → re-derived ids
     (`lib.rs:1609-1613`).
   - `source_session_resume_claims.session_id` → new (column update; re-keying
     is O1, not here).
   - `session_fts` + `session_fts_ids`: rebuild the row for the new id,
     delete the old (`lib.rs:4347-4377`); the per-claim conflict fail-closed
     in `session_search_text` (`lib.rs:4279-4315`) applies unchanged.
   - `id_alias`: `INSERT OR REPLACE (old, new, created, expires)` — the
     CAS-guarded rewrite the design demands (`design.md:138-141`).
   - `advance_generation_in_tx` (`lib.rs:5502`) so search/list cursors bound
     to the old generation invalidate — the existing CAS contract.
5. Durability: the rewrite rides the existing durable outbox / index-batch
   two-phase machinery (`lib.rs:5245-5249`, `lib.rs:5295-5330`); a crash
   leaves an intent that `recover_interrupted` aborts idempotently
   (`lib.rs:5333-5348`) — "no destructive rewrite" (design target #3).

Conflict detection under step 2 is per-transaction but race-free: writers are
serialized by the WriterLease, and the conflict set is read from
`id_alias` + the current derivation inside the same transaction.

## 6. Multi-ID split rules (fail closed)

For a legacy `ses_v1_` id, the live target set = distinct
`id_alias.new_id` rows with `expires_at_ms > now`, ∪ the current derivation
(identity rows `new_id = old_id` are excluded from the set and mean "not yet
rewritten"). Resolution is the pure function pinned by the domain tests:

- empty set → keep legacy id (coexistence: legacy ids stay valid/searchable,
  design target #1);
- exactly one distinct target → rewrite to it;
- **≥2 distinct targets → Conflict**: rewrite nothing, write no alias, keep
  `ses_v1_` rows, record a diagnostic, disclose nothing about the conflicting
  values. Never pick by path, source order, scan order, or lexicographic
  order — the multi-Session-source fail-closed rule generalized to identity
  resolution (`design.md:148-151`, `ports/src/lib.rs:843-845`,
  `claude/src/lib.rs:849-857`).

Path-case variants of one root collapse to one key by normalization, so the
historical "same installation, two case spellings" false-split disappears by
construction; genuine splits (two installations that really shared a native id
under the old unscoped scheme) surface as Conflicts and stay `ses_v1_`.

Expiry convention: `expired ⇔ now >= expires_at_ms` (half-open). Expired rows
are filtered before conflict detection; an expired-only alias resolves to the
legacy id.

## 7. TTL policy

`id_alias` TTL gives RFC-0001's open question (alias retention upper bound,
`RFC-0001:220`) an explicit answer:

- rewrite aliases: `ttl = 90 days` (default; constant, documented, not
  configurable in v13);
- identity markers for unresolvable roots: `ttl = 7 days` (short, per
  `design.md:146-147`) so the next re-scan retries soon.

## 8. Rollback path

- v13 is two additive tables; nothing else references them. Rollback =
  revert to the v12 binary and optionally `DROP TABLE installation_namespaces;
  DROP TABLE id_alias;` — side-effect-free (design target #6).
- A v12 binary opening a v13 DB refuses via the version guard
  (`lib.rs:1336-1342`): clean refusal, never silent misread. So the rollout
  order is: ship v13 first, backfill second; before backfill has run, the
  v13 DB is byte-identical to v12 except the two empty tables.
- An interrupted backfill leaves only aborted outbox intents
  (`lib.rs:5333-5348`); no half-written rewrite can survive a crash.
- After a rewrite has committed, there is no downgrade that erases it —
  that is exactly why rewrites are CAS-guarded, alias-recorded, and
  TTL-bounded rather than destructive (design target #3, #6).

## 9. Open decisions for the executing task

- **O1** — resume-claims re-keying `(source_path, session_id)` →
  `(namespace_registry_id, session_id)` (`design.md:144-145`): PK change,
  needs a table rebuild step; propose a separate v14 or same-task-later step,
  never inside the additive v13 DDL.
- **O2** — TTL values (90d/7d proposal) vs the RFC-0001 §7.2 adjudication;
  record the decision in `docs/adr/`.
- **O3** — `installation_path_hash` is BLAKE3 hex (not reversible); confirm
  the registry key itself (which embeds a path prefix) stays local-DB-only
  and is excluded from handoff-pack export (privacy).
- **O4** — when to delete the old `ses_v1_` catalog row: immediately when
  orphaned (proposal) vs keep-until-TTL-expiry. Both keep legacy lookup valid;
  only the listing-visibility differs.
- **O5** — prefix-set abstraction: `IdKind::prefix()` single-string
  (`ids.rs:46-56`) is baked into `list_filtered` (`lib.rs:1958-1962`),
  rebuild scan (`lib.rs:4385`), application fake (`application/src/lib.rs:2988`),
  handoff-pack schema patterns, and MCP/help strings. The executing task must
  introduce a version-aware prefix set and update every surface; the domain
  tests in this slice pin only the pure derivation/alias semantics.
- **O6** — Windows normalization scope: whole-key ASCII lowercase (proposal,
  matches NTFS case-insensitivity) vs the narrower drive-letter-only
  precedent in `source_path_identity` (`main.rs:2877-2888`).

## 10. Anchor index (verified file:line at f7e2a49)

| Anchor | File:line |
|---|---|
| `_v1_` infix = format version; `_v2_` coexistence | `crates/agent-session-grep-domain/src/ids.rs:31-33` |
| `prefix()` constants, Session → `ses_v1_` | `ids.rs:46-56` |
| `DIGEST_HEX_LEN = 32`, native clamp 256 | `ids.rs:113-127` |
| `native()` adopt-sanitize | `ids.rs:143-155` |
| `derive()` framing + domain separation | `ids.rs:157-183` |
| `native_session_scoped` v1 (facts, `ses_v1_` wire, no reverse) | `ids.rs:186-217` |
| `from_wire` / `validate` prefix logic | `ids.rs:247-280` |
| `PlacementId::derive` hashes session wire | `ids.rs:300-325` |
| SCHEMA_VERSION = 12 + version history | `crates/agent-session-grep-adapters-sqlite/src/lib.rs:5416-5438` |
| migrate gate + refuse-newer | `lib.rs:1327-1342` |
| migration dispatch v7..v12 | `lib.rs:1492-1509` |
| v7 DDL: placements/membership/edges | `lib.rs:1588-1633` |
| v8: resume-claims table + pair_observed CHECK | `lib.rs:1667-1707` |
| v9: additive provider_id pattern | `lib.rs:1709-1738` |
| v11: session_fts projection | `lib.rs:1770-1782` |
| v12: tool activity projection | `lib.rs:1793-1814` |
| `list_filtered` kind.prefix() LIKE | `lib.rs:1950-1964` |
| provider_id backfill precedent | `lib.rs:2061-2075` |
| provider_id in current/replacement determination | `lib.rs:2966-2991`, `lib.rs:3901-3919` |
| claims conflict → fail closed indexing | `lib.rs:4279-4315` |
| session search row rebuild | `lib.rs:4347-4377` |
| rebuild scan `LIKE 'ses_v1_%'` | `lib.rs:4379-4389` |
| source replacement tx (claims replace) | `lib.rs:4740-4814` |
| rebuild_index: catalog authoritative, fts_ids fidelity | `lib.rs:5251-5260`, `lib.rs:5298-5330` |
| recover_interrupted | `lib.rs:5333-5348` |
| `advance_generation_in_tx` on put | `lib.rs:5495-5504` |
| CAS semantics | `crates/agent-session-grep-adapters-sqlite/src/cas.rs:1-4,50-74` |
| MetadataResolution / ProviderSessionObservation | `crates/agent-session-grep-ports/src/lib.rs:834-856` |
| `SourceResumeClaim::from_observation` ambiguity fold | `ports/src/lib.rs:897-928` |
| `list_sessions` ses_v1_ doc | `ports/src/lib.rs:104` |
| GetSessionResume kind check | `crates/agent-session-grep-application/src/lib.rs:1940-1947` |
| application fake ses_v1_ filter | `application/src/lib.rs:2984-2994` |
| handoff-pack ses_v1_ asserts | `application/src/handoff_pack.rs:324,919-920` |
| installation_namespace derivation | `crates/agent-session-grep-cli/src/main.rs:2829-2850` |
| provider_data_root markers | `main.rs:2861-2875` |
| source_path_identity Windows normalization | `main.rs:2877-2888` |
| composition root session derivation | `main.rs:3162-3190` |
| multi_session fail-closed (claude/codex) | `crates/agent-session-grep-provider-claude/src/lib.rs:849-857`, `crates/agent-session-grep-provider-codex/src/lib.rs:738-741` |
| handoff-pack ses_v1_ schema patterns | `schemas/handoff/v1/pack.schema.json:129,154,173,196` |
| RFC-0001 §5 Stable ID constraints | `docs/architecture/RFC-0001-canonical-model-and-stable-id.md:171-204` |
| RFC-0001 §5.1 InstallationNamespaceId | `RFC-0001:206-210` |
| RFC-0001 §7.2 id_alias retention bound | `RFC-0001:220` |
| design.md Deferred target contract | `.trellis/tasks/08-15-unified-release-contract/design.md:108-162` |
