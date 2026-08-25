# Provider Resume Metadata extraction — Design

## Boundaries

- Touches `provider-claude`, `provider-codex`, and the minimum `ports` types.
- Does NOT implement Session identity namespacing (owned by identity task), persistence, or surfaces.

## Contract

`ports::ParseReport` gains typed, additive metadata observations while keeping `session_native_id`:

```rust
/// A single authoritative observation of provider-native Session identity.
pub struct ProviderSessionObservation {
    pub provider_session_id: Option<String>,
    pub original_working_directory: Option<String>,
    /// True when both values came from the same authoritative record.
    pub pair_observed: bool,
    /// True when the source carried multiple distinct Session IDs (diagnostic).
    pub multi_session: bool,
}

pub struct ParseReport {
    pub committed: usize,
    pub skipped: usize,
    pub diagnostics: Vec<String>,
    pub session_native_id: Option<String>,          // unchanged, still derived
    pub session_observation: ProviderSessionObservation, // additive
}
```

`ParseReport::default()` keeps `session_observation` at all-`None`/`false`, so every existing construction stays valid.

## Provider changes

### Claude Code (`provider-claude/src/lib.rs`)
- `RawLine` adds `#[serde(default)] cwd: Option<String>`.
- Track `cwd_observed: Option<String>` alongside `session_ids`. Only accept a `cwd` record that also carries a non-empty `sessionId`; then both are `pair_observed = true`. If `cwd` appears on a record whose `sessionId` differs from the first, mark `multi_session` via the existing diagnostic path.
- `session_observation` = first pair where both present; single value → the other field stays `None`; no value → all `None`.

### Codex (`provider-codex/src/lib.rs`)
- `RawPayload` adds `#[serde(default)] cwd: Option<String>`.
- In `session_meta` handling, capture both `session_id` and `cwd` from the same payload record → `pair_observed = true`.
- Explicitly ignore `turn_context` (turn-scoped); add a comment and a test proving `turn_context.cwd` is never used.

## Ambiguity semantics

- Repeated identical values: not ambiguous.
- Missing/blank: `None`.
- Multiple distinct Session IDs: existing bounded diagnostic; `multi_session = true`; do not claim first-ID metadata as authoritative — keep the ID for catalog composition but the Resume feature will treat the source as not-resumable.

## Privacy

- Diagnostics reference line numbers and bounded ID lists only, never Source paths or content.

## Tests

- Fixture/property tests per provider: missing, blank, repeated, conflicting, multi-session, `turn_context.cwd` ignored, pair association preserved, diagnostics privacy-safe.
