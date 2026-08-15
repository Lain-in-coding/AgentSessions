# Final validation: session metadata search

- Worktree: `C:/AgentSessions/.claude/worktrees/session-metadata-search-08-15`
- Scope: schema v10 `session_fts` projection, metadata/message merge, canonical identity preservation, privacy and system-only regression.

## Delivered

- Session metadata projection indexes only:
  - resolved Provider-native Session ID;
  - `original_working_directory` when `pair_observed` and resolved;
  - first chronological valid user request as the deterministic title-like field.
- Provider custom title and structural summary are not available through the current Canonical provider contract and remain explicitly deferred. Opaque provider records are not scraped or guessed.
- Source/transcript paths are excluded from the projection and result construction.
- Metadata-only Sessions without a user Message or placement return their canonical Session identity using the identity sidecar, preserving `Reconstructed`/native stability.
- A matching system/developer Message does not suppress a metadata-only Session before Application-level system filtering.
- Default search remains Message-grained; `group_by_session=true` performs canonical Session dedup with `occurrences`.
- Application preserves an adapter-supplied canonical `session_id` instead of overwriting it with a lossy `session_of()` lookup.

## Verification

- `cargo fmt --all --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test --workspace`: pass.
- Isolated `cargo build --release`: pass.
- Adapter metadata/privacy/system-only regression: pass.
- Application grouped/default search regressions: pass.
- `git diff --check`: pass.

## Deferred risks

- `bm25(fts)` and `bm25(session_fts)` are backend-local scores; current merge keeps deterministic score/ID ordering but does not claim cross-projection relevance comparability. A future ranking calibration should be a separate task.
- Provider custom titles and structural summaries require an explicit provider-neutral DTO and privacy/budget contract before indexing.
- The task remains uncommitted and unpushed pending owner authorization.
