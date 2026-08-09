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

## Child status (2026-07-28)

All eight children are implemented and archived. A ninth child is required
before this umbrella can close — see the blocking finding below.

| # | Child | State |
|---|---|---|
| 1 | `07-26-canonical-source-session-span` | archived |
| 2 | `07-26-provider-golden-hardening` | archived |
| 3 | `07-26-shared-application-adt` | archived |
| 4 | `07-26-human-robot-protocol` | archived |
| 5 | `07-26-mcp-v0-skill` | archived |
| 6 | `07-27-tui-preview` | archived |
| 7 | `07-27-install-promotion` | archived |
| 8 | `07-27-cross-source-session` | archived (layers ①-③ fixed, ④ open) |

## Blocking finding: per-source facts are stored on the message entity

The authorized real-data regression added by child 7 fails on the operator's
own corpus (707 transcripts, 605 MB): `sync` exits 6 `catalog_error`. Child 8
diagnosed four layers of the same root cause and fixed the first three.

A message's identity (its provider-native uuid) is shared across sessions, but
its *position* — which session owns it, who its parent is, where its bytes sit
in a file — is a per-source fact. The current implementation stores position on
the message entity, so every position field becomes a cross-source conflict.

| Layer | Field | Real-data shape | State |
|---|---|---|---|
| ① | session member list | one `sessionId` declared by 55 files | fixed (union + single-value alias) |
| ② | `session` back-reference | one message in 3 different sessions | fixed (`sessions` array + alias) |
| ③ | `span` byte range | same message at different offsets per file | fixed (`spans` keyed by document + alias) |
| ④ | `parentUuid` | same message re-parented per file | **open** |

Layer ④ cannot be unioned: `select_mainline` walks a single parent chain to pick
a leaf, so a message with several parents leaves "mainline" undefined and a union
would make branch selection silently arbitrary. The route is to make the
implementation match RFC-0001 §3.2 — move position out of the message entity and
express parentage as a separate `MessageEdge` relation. That is a canonical-model
change; details and the reasoning for stopping rather than patching a fourth time
are in `.trellis/tasks/archive/2026-07/07-27-cross-source-session/design.md` §7.

Evidence: `docs/evidence/integration-beta/real-data-regression.md`,
matrix rows `IB-SESSION-CROSS-SOURCE-001` / `IB-REAL-DATA-REGRESSION-001`.

## Acceptance Criteria

- [x] Each proposed deliverable is represented by an independently verifiable
      child task with explicit prerequisites and release scope.
- [x] Claude Code and Codex satisfy the documented Beta promotion gates or
      remain explicitly Experimental with precise blockers.
      (Both remain Experimental; blockers are gaps 4-5 in the maturity matrix.)
- [x] Provider fixtures are auditable, privacy-safe, and reproducible.
- [ ] Canonical source/session/thread/branch/span information survives
      ingestion, storage migration, retrieval, and index rebuild.
      **Fails on real data** — see the blocking finding above. It holds for
      synthetic fixtures and for any single source; it does not hold for a
      session whose records span multiple files with re-parenting.
- [x] Shared Application contracts cover cursor, generation, response budget,
      evidence, branch selection, and partial-result semantics before new
      frontends consume them.
- [x] Human/Robot, MCP/Skill, TUI, and install deliverables each have their own
      tests and release-scope status.
- [ ] Parent integration review confirms that child contracts and migration
      behavior are mutually consistent. **Not started** — should follow the
      layer-④ fix, since that changes the canonical message shape the other
      children read.

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
