# Unified release contract — Design

> Parent: `08-15-open-source-product-roadmap` (P0, Phase 1).
> Status: planning → implementation slice completed 2026-08-15 (see implement.md).
> This document covers the first contract slice delivered by
> `08-15-unified-release-contract` and the **deferred provider-scoped identity
> migration plan** (owned here, executed later).

## Scope of this slice

The full PRD spans: provider-scoped session identity, Robot 1.1
(retrieval_mode + semantic error codes), Application ADT capability surface,
schema registry ownership, boundary-aware projection, capability honesty,
handoff-pack/v1 contract, and the `asg` CLI alias.

This slice (per dispatch) delivers the **additive contract foundation** only:

1. `asg` CLI alias (second `[[bin]]` sharing the composition root).
2. Robot protocol schema 1.0 → 1.1 (additive codes only; no field/code
   renamed or removed).
3. Ports types: capability manifest, OutputBoundary projection types,
   `RedactionMetadata` (exact ADR-0009 field set).
4. `handoff-pack/v1` JSON schema draft registered under `schemas/`.
5. Deferred provider-scoped identity migration **plan** (design only, no code).

Deliberately **not** in this slice (siblings/later tasks own them):
retrieval_mode emission, redaction engine (08-15-offline-privacy-hooks),
handoff implementation (08-15-evidence-handoff-pack), install-script
multi-binary changes (08-15-benchmark-install-open-source-gate), capability
matrix consumption by entry layers.

## Decisions

### D1 — `asg` alias is a second `[[bin]]` in the same crate

`crates/agent-session-grep-cli/Cargo.toml` gains a second `[[bin]]`
(`name = "asg"`, `path = "src/main.rs"`) next to the existing
`agent-session-grep` bin. Both names share the same composition root, so
behavior and protocol output are identical by construction (no wrapper, no
behavior fork). `CARGO_PKG_NAME`-based output (version/status/doctor/MCP
tool payloads) stays `agent-session-grep` regardless of which name was
invoked — that is deliberate: the product identity is a single string.

Consequence for later tasks: install scripts (`scripts/install/*.{ps1,sh}`)
currently copy exactly one binary (`agent-session-grep`); the installer task
must copy/install both names and `uninstall` must remove both.

### D2 — Robot 1.1 is strictly additive

`SCHEMA_VERSION` moves `1.0 → 1.1`; minor bump means 1.0 frames remain
parseable and 1.0-only clients keep working (unknown-major rejection is the
only hard rule). The four new canonical codes:

| Wire code | Layer | Retryable | Exit | Redaction | Meaning |
|---|---|---|---|---|---|
| `capability_not_supported` | application | false | 7 | none | Requested capability not declared by the provider — never silently degrade |
| `model_download_failed` | semantic | true | 6 | none | Semantic model download failure (network transient) |
| `model_corrupted` | semantic | false | 6 | none | Model file corrupt; re-download/reinstall required |
| `vector_index_not_ready` | semantic | true | 6 | none | Vector index still building; retry after build |

Exit-code families: 7 = provider/adapter family (capability absence is a
provider-declared property), 6 = catalog/index/resource family (model and
vector index are retrieval resources). `layer: "semantic"` is a new additive
layer value; the drift test enforces
`error-catalog.json` ⇄ `envelope.schema.json` ⇄ `CanonicalCode` runtime
mapping stay in lockstep (13 → 17 codes).

No existing field or code is renamed or removed. `retrieval_mode` emission
on search responses is a separate, later step (needs the semantic layer);
the envelope `data` object is schema-open, so adding it later stays
additive.

### D3 — Ports types are contract-only (no engine)

All three additions live in `agent-session-grep-ports` and are pure types +
doc comments:

- `ProviderCapability` (wire strings: discover/parse/search/context/resume/
  handoff/tool_activity) + `ProviderCapabilityManifest` + a
  `ProviderAdapter::capabilities()` trait method **with a default**
  (`vec![Parse]`), so existing adapters compile unchanged and every future
  provider declares its real surface. Entry layers must consume the
  manifest, never hardcode (PRD req 6).
- `OutputBoundary` (HumanCli/Tui/Robot/Mcp/Web/Http/Handoff) with
  `is_cross_boundary()` encoding the ADR-0009 split (local human output
  stays ADR-0004-exempt; everything else defaults to redaction).
- `RedactionMetadata` with **exactly** the PRD/ADR-0009 field set:
  `mode` (`default`/`reveal`), `status` (`applied`/`none`/`partial`),
  `ruleset_version`, `redacted_count`, `audit_id: Option<String>`.
  The redaction engine itself is a sibling task
  (08-15-offline-privacy-hooks); these types are the cross-task contract.

No serde derives: serialization projection belongs to the CLI/Web layer,
and the ports crate currently has no serde dependency — adding derives
would be speculative.

### D4 — handoff-pack/v1 registered as a draft

`schemas/handoff-pack/v1/handoff-pack.schema.json` is contract-first
registration, marked `"status": "draft"` and `draft-2026-08-15` in
`schema_version`. It fixes the authoritative field shape
(pack_id/generation/budget/truncation/redaction + evidence/inference split
with per-item spans) so the handoff task implements against a stable
contract. Markdown output is a projection, never authoritative. Consumers
must not rely on the draft in production; the shape is owned by this task
until the handoff task accepts it.

## Deferred: provider-scoped session identity — migration plan (design only)

> PRD req 1. Deliberately deferred from this slice: it rewrites the `ses_v1_*`
> identity and touches 20+ test fixtures (dispatch constraint #5).

### Current state (baseline `main` @ f4175a0, this worktree)

- `StableId::native(IdKind::Session, raw)` adopts the provider-native
  session id verbatim into a `ses_v1_*` wire id, **unscoped** — a Claude
  Code session and a Codex session with the same native id would collide.
- `ids.rs` already documents the intended mechanism: the `_v1_` infix is a
  format version; a changed hashing scheme bumps to `_v2_` and **both can
  coexist during migration**.
- The 08-14 integration branch introduces the derivation side
  (`SessionIdentityNamespace`, `StableId::native_session_scoped`, and a
  composition-root `installation_namespace` from the provider data-root
  path prefix `.claude`/`.codex`), with known RFC-0001 §5.1 debt: no
  persisted registry, no `id_alias`, no Windows path-case normalization,
  and resume claims not keyed by namespace id.

### Target contract (additive, non-destructive)

1. **Namespace-scoped wire ids**: scoped canonical Session ids become
   `ses_v2_*` (hash inputs: provider id + installation namespace key +
   native session id). `ses_v1_*` legacy ids remain valid and searchable
   during migration — coexistence by design.
2. **Persisted registry (schema v9/v10)**: new table
   `installation_namespaces(namespace_key TEXT PRIMARY KEY, provider_id TEXT
   NOT NULL, installation_path_hash TEXT NOT NULL, created_at INTEGER NOT
   NULL)`; namespace key derivation is the composition-root rule (path
   prefix) normalized for Windows path case (lowercased on Windows).
3. **`id_alias(old_id, new_id, ttl)`**: legacy `ses_v1_*` → `ses_v2_*`
   mapping with TTL, written in the same outbox transaction as the rewrite
   (two-phase, CAS-guarded generation advance — no destructive rewrite).
4. **Backfill**: on next complete re-scan of a source, derive its
   namespace, compute the scoped id, insert `id_alias`, and rewrite
   memberships/claims (catalog, placements, FTS identity sidecar, resume
   claims keyed by `(namespace_registry_id, session_id)`) in one
   transaction. Sources without a resolvable root (legacy absolute paths)
   keep `ses_v1_*` and an alias row with a short TTL.
5. **Multi-ID fail-closed**: a source whose session resolves to multiple
   scoped identities (path case variants, relocated roots) fails closed per
   the existing multi-Session-source rule — never pick by path or order;
   disclose nothing about the conflicting values.
6. **Rollback**: registry/alias tables are additive and unused by older
   binaries; a failed migration leaves only a side-effect-free outbox
   intent. No downgrade path needed.

### Sequencing and ownership

Executed as its own task after this contract slice (do not parallelize with
other contract edits — it rewrites `ses_v1_*` assertions in 20+ fixtures).
Owner of this plan: `08-15-unified-release-contract` (this file); the
executing task must not change the additive contract without updating this
plan and the schema-drift gates.

## Compatibility

- CLI: `agent-session-grep` and `asg` behave identically (same main.rs).
- Protocol: 1.1 frames are a strict superset of 1.0 frames; existing 1.0
  assertions updated to 1.1 in protocol.rs tests and `tests/e2e.rs`.
- Schema registry: robot schemas are the single source for error codes
  (drift-tested); handoff-pack draft is additive and isolated under its own
  version directory.

## Rollout / rollback

- Rollout: ports types → protocol bump → schema files → `asg` alias →
  docs (all landed together in this slice).
- Rollback: revert `[[bin]] asg` (Cargo.toml), revert SCHEMA_VERSION and
  the four codes (protocol.rs + both robot schemas + tests). No migration
  involved — this slice touches no SQLite schema.
