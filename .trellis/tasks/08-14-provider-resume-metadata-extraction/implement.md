# Provider Resume Metadata extraction — Implementation

## Preconditions

- `task.py start` this task before editing.
- Work in the task's isolated worktree on top of the verified baseline (HEAD
  `f4175a0`, 33 modified/untracked paths, quality gate green).

## Steps

1. **ports**: extend `ParseReport` with `ProviderSessionObservation` (additive). Update `ParseReport::default()` and any construction. Keep `session_native_id` derived as today so identity composition is unaffected.
2. **provider-claude**:
   - Add `cwd: Option<String>` to `RawLine`.
   - In `parse`, track `(session_id, cwd)` association from records carrying both; set `pair_observed`.
   - Keep existing multi-session diagnostic; set `multi_session` when >1 distinct IDs.
   - Add fixture tests: missing, blank, repeated, conflicting, multi-session, pair preservation.
3. **provider-codex**:
   - Add `cwd: Option<String>` to `RawPayload`.
   - In `session_meta` branch, capture `session_id` + `cwd` from the same payload → `pair_observed`.
   - Add a comment + test proving `turn_context.cwd` is ignored.
   - Add the same fixture matrix.
4. **Cross-crate**: update `provider_matrix`/golden tests only if a constructor signature changed; prefer additive fields to avoid churn.

## Validation

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --release`

## Review gates

- `trellis-check` after edits; verify no privacy leak in diagnostics; no real Source paths in fixtures/tests/docs.
- Do NOT commit/push.
