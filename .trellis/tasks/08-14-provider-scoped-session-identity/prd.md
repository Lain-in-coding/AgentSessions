# Provider-scoped Session identity

## Goal

Namespace canonical native Session identity by provider and stable installation namespace so equal native IDs from different providers or installations cannot collide, and migrate legacy catalogs without guessing native IDs.

## Requirements

### R1 Namespace
- Canonical Session identity derived from a native Provider Session ID must include the Provider and a stable installation namespace.
- Collision tests must prove that the same native Session ID under different providers or different installation namespaces produces distinct canonical Session IDs.
- Do not recover a native ID by stripping a `ses_v1_` prefix; that transform is lossy and unreliable.

### R2 Migration
- Legacy catalogs open and remain readable.
- A deterministic, documented migration re-keys existing canonical Session identities with the provider/installation namespace when reliable.
- Migration must not silently reinterpret existing `ses_v1_*` IDs or reorder grouping, cursors, relations, and search results.
- When a migration cannot determine a namespace reliably, the legacy Session remains readable but must not be claimed as Provider-resumable.

### R3 Multi-ID fail-closed
- A Source carrying multiple distinct native Session IDs currently uses first-ID-wins.
- With namespaced identity, such a Source must split when association is reliable; otherwise it must fail closed for Resume Metadata and emit a bounded diagnostic.

### R4 Compatibility
- Grouping, context, search hit `session_id`, and session relations continue to use the canonical Session identity.
- Provider metadata on a search hit still describes the deterministic selected Session, never a guess.

## Acceptance Criteria

- [ ] Provider + installation namespace is part of canonical Session identity.
- [ ] Equal native IDs across providers and installations do not collide.
- [ ] Legacy catalogs migrate deterministically and remain readable; grouping, cursors, relations, and search are preserved.
- [ ] No `ses_v1_*` value is reinterpreted as a native ID.
- [ ] Multi-ID Sources fail closed for Resume Metadata with a bounded diagnostic.
- [ ] fmt, clippy `-D warnings`, test workspace, and release build are green.

## Constraints

- This task owns the identity namespace and migration only; persistence of Resume Metadata fields is owned by the core task.
- Provider Source files remain read-only.
- No commit or push without explicit owner authorization.
