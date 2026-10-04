# Design
Follow parent decisions and revised finding semantics.

## Exclusive ownership
provider crates and their tests only; no Cargo.lock/CLI/app/sqlite edits. Main owns docs, task artifacts, commit and push; never revert another worker.

## Approach
Normalize format-proven epoch units in Cursor/Cline and single-document BOM; never numeric timestamp to empty string. Grok promptId is not durable session ID; retain whitespace/rewind. OpenClaw thinking-only loss gets skipped/diagnostics without indexing reasoning. Codex payload.id is SID only in session_meta, reject conflicting aliases. Kimi probe accepts supported prompt/steer discriminator, not arbitrary JSON. Qoder SID/cwd must come from one authoritative record/session. Temporary DB files are exclusive/private; only successful creation owns cleanup; conflict never deletes another file. Cover Cursor/OpenCode and check new Hermes for same pattern. Preserve Pi and provider maturity.

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
