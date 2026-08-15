# Provider-scoped Session identity — Implementation

## Preconditions

- `task.py start` this task before editing.
- Work in the integration worktree. Requires the provider observation type (provider task) to be present or a temporary compatible stub; coordinate ordering.

## Steps

1. **domain**: add `SessionIdentityNamespace` (provider_id + installation namespace) and use it in the native Session-id derivation; add collision tests.
2. **Composition**: in the store/sync path where `session_native_id` becomes `ses_v1_*`, apply the namespace; multi-ID `multi_session` observation fails closed (no resumable claim).
3. **Migration**: bump `SCHEMA_VERSION` 7 → 8; deterministic re-key of provider-native Session identities; preserve reconstructed IDs; no `ses_v1_`→native reverse path.
4. **Round-trip**: migrated fixture keeps grouping, cursors, relations, and search working.
5. **Fail-closed**: multi-ID Sources remain searchable but not resumable.

## Validation

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --release`

## Review gates

- `trellis-check`; verify schema 7→8 migration tests, no silent reinterpretation, collision tests green.
- Do NOT commit/push.
