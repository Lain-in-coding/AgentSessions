# Advance 0.3 Integration Beta

## Goal

Advance Claude Code and Codex from Experimental toward Beta by completing the
shared canonical and application contracts first, then add frontend and
distribution surfaces in dependency order.

## Confirmed Facts

- The current runtime stores message payloads but does not persist independent
  SourceDocument, Session, Thread, Branch, or EvidenceSpan entities.
- Claude Code and Codex are explicitly Experimental. Their documented Beta
  blockers are golden fixtures, property/fuzz coverage, source spans, and
  formal target evidence.
- The current `SourceSnapshot` contract is file-oriented. The untracked
  `spikes/sqlite-source-identity/` evidence shows that SQLite-backed provider
  sources require row-level identity and cannot safely reuse whole-file
  identity.
- MCP and TUI would duplicate cursor, generation, response-budget, evidence,
  branch-selection, and partial-result rules if built before the shared
  Application contract.
- R0 governance records remain pending owner/approver decisions. Planning may
  continue, but implementation must not claim those records are Accepted.

## Requirements

- Split this umbrella into independently verifiable child tasks; implementation
  occurs in children, followed by a parent integration review.
- Add synthetic or irreversibly redacted golden fixtures with provenance.
- Add source spans, complete canonical Source/Thread/Branch/Evidence modeling,
  and independent Session/SourceDocument identity with metadata-preservation
  coverage.
- Preserve the local-first, strictly read-only provider boundary.
- Add property/fuzz coverage for malformed, Unicode, large-field, threading,
  and duplicate-mirror cases.
- Expand the Application ADT and shared contracts before MCP/TUI so frontends
  do not duplicate cursor, generation, response-budget, evidence, or
  partial-result business rules.
- Complete Human/Robot protocol behavior before adding new frontend transports.
- Implement stdio MCP v0 and Skill beta, then TUI Preview, only after their
  shared-contract prerequisites are met.
- Add installation scripts, clean-machine smoke evidence, and a
  migration/rebuild runbook before promotion.
- Keep SQLite-backed provider product support outside the first implementation
  child unless the owner explicitly expands that scope. The existing spike may
  be used as design evidence without being modified or committed by planning.

## Proposed Child Task Map

1. Canonical Source/Session/Span foundation and storage migration.
2. Claude/Codex golden fixtures, contract tests, and property/fuzz hardening.
3. Shared Application ADT for branch selection, cursor, generation,
   response budget, evidence, and partial results.
4. Human/Robot protocol completion against the shared ADT.
5. stdio MCP v0 and Skill beta.
6. TUI Preview.
7. Installation, clean-machine smoke, migration/rebuild runbook, and promotion
   evidence.

The ordering is normative: later children may not duplicate or bypass contracts
owned by earlier children.

## Acceptance Criteria

- [ ] Each proposed deliverable is represented by an independently verifiable
      child task with explicit prerequisites and release scope.
- [ ] Claude Code and Codex satisfy the documented Beta promotion gates or
      remain explicitly Experimental with precise blockers.
- [ ] Provider fixtures are auditable, privacy-safe, and reproducible.
- [ ] Canonical source/session/thread/branch/span information survives
      ingestion, storage migration, retrieval, and index rebuild.
- [ ] Shared Application contracts cover cursor, generation, response budget,
      evidence, branch selection, and partial-result semantics before new
      frontends consume them.
- [ ] Human/Robot, MCP/Skill, TUI, and install deliverables each have their own
      tests and release-scope status.
- [ ] Parent integration review confirms that child contracts and migration
      behavior are mutually consistent.

## Out of Scope

- Dynamic third-party adapter loading.
- Semantic or vector search.
- Marking any Draft/Proposed governance record Accepted without owner and
  approver action.
- Adding a new SQLite-backed provider in the first child unless explicitly
  approved.
- Signing, notarization, publishing, or external credential use without
  separate authorization.

## Resolved Open Question

- Decision (2026-07-26, per standing autonomous-execution authorization): the
  first implementation child stays focused on the canonical
  Source/Session/Span foundation for the existing file-backed Claude/Codex
  providers. The source-unit contract must be designed so a future SQLite
  row-source identity (see `spikes/sqlite-source-identity/EVIDENCE.md`) can
  implement it without contract changes, but SQLite row-source discovery and
  verification are NOT productized in this child.
- Rationale: the spike's own recommendation orders work as identity modeling
  first, file-backed providers second, SQLite providers last (behind an ADR
  plus a column-adaptation layer that does not exist yet); and this PRD's Out
  of Scope already excludes new SQLite-backed providers from the first child.
