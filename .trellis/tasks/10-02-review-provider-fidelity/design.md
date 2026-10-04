# Design
Follow parent decisions and revised finding semantics.

## Exclusive ownership
provider crates and their tests only; no Cargo.lock/CLI/app/sqlite edits. Main owns docs, task artifacts, commit and push; never revert another worker.

## Approach
Normalize format-proven epoch units in Cursor/Cline and single-document BOM; never numeric timestamp to empty string. Grok promptId is not durable session ID; retain whitespace/rewind. OpenClaw thinking-only loss gets skipped/diagnostics without indexing reasoning. Codex payload.id is the current thread identity only in session_meta; root session_id may differ and is not a conflicting alias. Retain session_id-only compatibility. Kimi probe accepts supported prompt/steer discriminator, not arbitrary JSON. Qoder SID/cwd must come from one authoritative record/session. Temporary DB files are exclusive/private; only successful creation owns cleanup; conflict never deletes another file. Cover Cursor/OpenCode and check new Hermes for same pattern. Preserve Pi and provider maturity.

## Compatibility
Retain contracts except specified corrections. Document migration/cache changes. Revert isolated commits only after accounting for new data; never remove validation to roll back.


## Timestamp/BOM integration slice
Provider workers keep their own crate scopes. Main owns the parser-5/schema-19
compatibility boundary, specs and commits, and explicitly delegates CLI test-only
integration scopes when needed. No production CLI/API expansion or Cargo.lock
change is included. Cross-source Cursor compatibility is aggregate-only, gated
by each claimant's associated ItemTable Document and exact timestamp instant;
raw source observations and generic intrinsic conflict rules are unchanged.


## Conversation-fidelity slice (P1-4/P1-5/P1-6)
Grok turn/chunk IDs are not native session identity or proof of multiple sessions.
Retain grouping, rewind and whitespace inside reconstructed messages; never gain
Resume authority from promptId or another unproven field. Any supported genuine
multi-session evidence must remain fail-closed without per-message attribution.
OpenClaw textless non-indexable content gets honest loss accounting and bounded
content-free diagnostics, while genuine empty messages remain distinct. Do not
index thinking text or add tool execution/activity capabilities to fix counters.
Main owns cross-provider CLI/re-ingestion tests, any required parser-version bump,
shared specs, commits and push; provider workers have explicit disjoint scopes.


## Metadata/probe slice (P2-5/P2-6; base 57529db)
- Only Codex `session_meta.payload` treats `id` as the current thread identity.
  Official protocol and rollout metadata at c5d242fa7907bff1b7a7e26e95febc548c0a6963
  distinguish it from root `session_id`; different values are legal, not an alias
  conflict. This supersedes the original plan's rejection rule (verified 2026-10-04).
  Prefer trimmed nonempty `id`; retain existing session_id-only compatibility when
  no usable id exists. Native Message IDs and other envelope classification are
  unchanged. Missing IDs do not authorize cwd-only pairing; existing type errors
  are not relaxed. Distinct current-thread headers still trigger multi-session
  ambiguity; the root ID alone does not.
- Kimi probe recognizes the existing parser's exact `turn.prompt`/`turn.steer`
  discriminators, with evidence/confidence for supported input, rather than a
  generic JSON or `turn.*` fallback. Other provider selection, sampling bounds,
  input extraction and loop/tool behavior remain unchanged.
- Qoder cwd authority requires a complete SID/cwd pair belonging to the selected
  first Session. Never combine cwd-only and SID-only records or mismatched nested
  bodies. Preserve existing top-level SID precedence; use cwd from a body carrying
  that same SID, not independently selected aliases. A later same-session complete
  pair is valid; another Session's pair is not. Do not broaden multi-session support.
- Main owns shared specs, CLI regression/upgrade tests, parser version (7 if required
  by these observation changes), fixture matrix, commits and push. Provider workers
  have disjoint crate scopes. Temp SQLite ownership (P1-7) remains a separate slice.
