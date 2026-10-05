# Design: experience-first remediation

## Existing contracts
Keep thin adapters around the existing Application/CLI behavior. Robot emits 1.1 and its schema exists; v1/1.0 is historical, not a file to overwrite. Context byte budgets and Handoff content budgets retain their separate contracts. Explicit zero is never lost via a frontend truthiness check. Preserve the stricter declared MCP budget floors rather than changing public behavior to force symmetry.

## HTTP and Web
Extend the existing strict query_pairs/argv validation to Context and Handoff with route-specific allowed parameters. Permit only provider repetition on Handoff; reuse shared integer/budget validation downstream. Update marshalled-flag safety coverage.

Use last accepted search parameters for supported Handoff inheritance (q/provider/since/until), and invalidate stale previews when the query/filter/budget changes. Add compact optional controls for Context max_messages/max_bytes and Handoff max_evidence/max_tokens/max_bytes; labels explain omitted defaults and unsupported inheritance. No independent frontend clamping or new framework.

Project existing response fields according to effective_level. Render talks and summary without losing existing messages/warnings, show requested/effective differences and supplied hint/truncation data, and distinguish empty/partial/failure. Continue textContent, i18n and per-kind AbortController/request ownership.

## TUI
Add explicit pure-core facet actions produced by Alt+M/Alt+K in terminal glue. Ordinary characters, including initial m/k, are text on Search. Preserve result-screen shortcuts, Ctrl+C/Enter/Esc and Unicode; modifier combinations must not insert shortcut text. Use the existing composition helper for clock/current_repo; do not invent another ranking path.

## Output boundary (P0-3 dependency)
Reuse secret detection. Protect only contract identity fields selected by command and structural path, not arbitrary keys named id. Normalize invalid-identity diagnostics at their source, with no unsafe debug bypass or broad path regex fallback. Keep Human/TUI content policy. CLI, MCP and HTTP integration must retain their existing envelope/correlation contracts and truthful redaction accounting.

## Validation and ownership
- TUI worker owns only src/tui/core.rs and src/tui/mod.rs.
- HTTP worker owns only src/serve.rs and embedded HTTP tests.
- Web worker owns only src/web/index.html and tests/web_ui.test.cjs.
- Boundary worker owns src/redaction.rs, relevant src/lib.rs/src/mcp.rs/src/protocol.rs and CLI/MCP regression slices; coordinate HTTP call-site changes rather than editing serve.rs concurrently.
- Main owns task/spec/product docs, schema test harness/CI integration, shared integration, commits and push. Workers must never revert peer changes, commit, push or recursively dispatch.

Use test-only jsonschema==4.26.0 Draft202012Validator with local resources for actual response/error/partial/progress frames. Negative controls delete required fields, change types and use wrong schema versions. Diagnostic is fixture-only until runtime emits it. No claim that helper shape assertions are full schema validation.

## Rollout / rollback
Independent bounded commits after review and tests. Preserve existing schema/parser/protocol versions and old clients' published contracts. Roll back isolated changes without weakening safety gates; do not modify user catalogs, main, release tags, or unrelated worktrees.
