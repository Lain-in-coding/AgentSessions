# Read-only experience and entrypoint consistency

## Goal and approval
On 2026-10-05 the owner selected **experience first**, reviewed the five-batch plan in this conversation, and explicitly requested implementation in a subsequent message. Continue from immutable baseline `f0831afdf8d5512cde15231d447956d875a8f1f2` in the existing remediation branch; do not create a duplicate task.

Users and machine clients must reliably complete search -> read context -> extract context/handoff, without lost supported filters, misleading empty/complete states, blocked literal input, or damaged reference identities. This is a bounded correctness/interaction improvement, not a visual redesign.

## Requirements
- Carry HTTP Context max_bytes and Handoff max_tokens/max_bytes, retaining existing provider/time/max_evidence support. Repeated providers are an OR set. Unknown, empty, duplicate scalar and malformed parameters fail explicitly using the existing argument-safety boundary.
- Web budgets are optional; omitted values use backend defaults. Handoff inherits q/provider/since/until from the last accepted search snapshot. Unsupported repo/sidechain/tool/mode inheritance is explained, not silently promised or implemented.
- Display raw/talks/sessions using effective_level, including existing messages/warnings, level fallback/hint and structured truncation facts. Separate empty, failure and partial states; retain safe text rendering, cancellation and current-request guards.
- Literal m/k are always text in the search editor. Alt+M/Alt+K cycle existing facets; result-screen m/k remain available. Reuse clock/current_repo injection without changing ranking or adding filter dimensions.
- P0-3 dependency, coordinated with review-boundaries: remove raw private identity/path echoes from diagnostics and preserve typed correlation/cursor/stable IDs through machine output redaction. Unknown payload fields do not gain blanket exemptions. Local Human/TUI content policy stays unchanged.
- Validate real Robot 1.1 response/error/partial/progress frames against the already-published schema. Freeze historical 1.0; no protocol/parser/database version bump. Pin jsonschema 4.26.0 for tests only, with local schemas and failure (not skip) if required validation inputs are missing.
- GET resume remains stateless and must not create/consume CLI ack; POST stays unsupported. Historical tool failures do not determine the current read request Outcome.

## Acceptance
- [x] Each actual defect has a baseline counterexample and passing regression; existing fixes are revalidated rather than reimplemented.
- [x] Parameter/default/zero/boundary/duplicate/encoding cases preserve declared entrypoint semantics; HTTP follows CLI, MCP retains its documented stricter floors.
- [x] Nonempty/empty/partial fixtures at every context level render truthfully, with stale-response and language-switch coverage.
- [x] TUI input and fixed-clock/cwd cross-entry ranking regressions pass.
- [x] Identity round-trip and safe diagnostic regressions cover actual machine output, not only helpers.
- [x] Real frame schema validation includes negative controls, plus browser/terminal user-journey verification.
- [x] Independent review and local/remote gates passed for immutable implementation checkpoint a867a01 (14/14 remote checks); remaining interactive-platform/IME limits and deferred work are explicit.

## Deferred / preserved scope
This task originally tracks P1-9/P2-1/P2-2/P2-3/P2-4, withdrawn P2-7 regression, C1/C2/P2-9. P2-1 and C2 already have implementation/evidence and remain regression guards. C1/P2-9 conditional filesystem work remains open outside this experience phase. Release rehearsal, performance scale work, ranking policy changes, Pi tree enhancement, provider pruning, model hot reload and cleanup remain deferred. No merge, official release/tag, history rewrite, task archive, or removal of other worktrees is authorized by this phase.

## Verified phase result
Experience-first implementation and its acceptance are complete at checkpoint a867a01. CI/security/evidence runs 37311676480/37311676337/37311676379 passed; the task itself is not archived because C1/P2-9 remain deferred under its original scope. See implement.md for scoped P0-3 limits and reproducible validation evidence.
