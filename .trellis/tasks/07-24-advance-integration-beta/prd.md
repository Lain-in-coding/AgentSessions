# Advance 0.3 integration beta

## Goal

Advance Claude Code and Codex from Experimental toward Beta, then build the next shared-application surfaces in roadmap order.

## Requirements

- Add synthetic or irreversibly redacted golden fixtures with provenance.
- Add source spans, complete canonical Source/Thread/Branch/Evidence modeling, and independent Session/SourceDocument identity with metadata-preservation coverage.
- Add property/fuzz coverage for malformed, Unicode, large-field, threading, and duplicate-mirror cases.
- Expand the Application ADT and shared contracts before MCP/TUI so frontends do not duplicate cursor, generation, response-budget, evidence, or partial-result business rules.
- Complete Human/Robot protocol behavior before adding new frontend transports.
- Implement stdio MCP v0 and Skill beta, then TUI Preview, only after their shared-contract prerequisites are met.
- Add installation scripts, clean-machine smoke evidence, and a migration/rebuild runbook before promotion.

## Acceptance Criteria

- [ ] Claude Code and Codex satisfy the documented Beta promotion gates or remain explicitly Experimental with precise blockers.
- [ ] Provider fixtures are auditable, privacy-safe, and reproducible.
- [ ] Canonical source/session/span information survives ingestion and retrieval.
- [ ] Shared Application contracts cover cursor, generation, response budget, and evidence semantics before new frontends consume them.
- [ ] Any MCP/TUI/install deliverable has its own tests and release-scope status.
