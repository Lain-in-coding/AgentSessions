# PRD: TUI Preview

Parent: `07-24-advance-integration-beta` child 6. Prerequisites (landed):
child 3 shared ADT, child 4 protocol surfaces, child 5 MCP. The TUI is a
frontend over the SAME Application ADT: it MUST NOT duplicate or bypass
search/branch/pagination/budget/evidence business rules (parent PRD,
normative ordering).

## Problem

AgentSessions has machine surfaces (robot CLI, MCP) but no interactive one.
A human exploring their own history today must compose CLI invocations by
hand. Child 6 ships a minimal read-only terminal UI — explicitly a
**Preview**: usable, honest, small; not a design showcase.

## Decision on dependencies (owner-visible)

An interactive TUI needs terminal raw mode + an event stream; on Windows
that is not reachable from `std` alone. This child adds the repo's first UI
dependencies: **`ratatui` + `crossterm`** (both MIT-licensed, vetted by
`cargo deny`). This deviates from the zero-new-deps pattern of children 1-5
and is called out here so the owner can veto at PR review. Rollback = revert
the feature commits; no schema or contract surface depends on the TUI.

## Requirements

- R1 `agentsessions tui` subcommand: read-only interactive browser over the
  `--db` store (`SqliteStore::open`; write commands not reachable).
- R2 Three screens, keyboard-only: (a) **Search** — input line + hits list;
  (b) **Results** — ↑/↓ select, Enter opens context, `n` fetches the next
  page via the ADT cursor (never re-implements paging), Esc back to input;
  (c) **Context** — mainline messages of the selected hit's session with
  evidence precision markers, ↑/↓/PgUp/PgDn scroll, `f` toggles
  mainline/full policy, Esc back. `q` quits from any screen.
- R3 All data access goes through `AppRequest::{Search, Context, Show}`;
  truncation/partial results and warnings are rendered honestly (status
  line), never hidden.
- R4 Errors surface in-UI via the same canonical mapping (`ProtocolError`);
  the TUI never panics on App errors; terminal state is restored on exit
  (including panic and error paths — raw-mode guard).
- R5 View layer is testable without a terminal: pure view-model functions
  (state → widget inputs / lines) + pure key-event → state-transition
  reducer, unit-tested. The crossterm/ratatui glue loop stays thin.
- R6 stdout discipline: the TUI runs on the alternate screen; nothing is
  printed to stdout after exit except nothing (clean restore). Diagnostics
  go to stderr. Human/Robot/MCP surfaces are untouched.

## Acceptance criteria

- [ ] `agentsessions --db <db> tui` opens on a real store; search "x" lists
      hits; Enter shows the session context with evidence markers; q exits
      restoring the terminal (manual smoke on Windows Terminal, recorded in
      the session journal).
- [ ] Reducer unit tests cover: typing/backspace in search, submit, hit
      navigation incl. empty results, next-page merge via cursor, screen
      transitions, policy toggle, quit; view-model tests cover hit-line and
      context-line rendering incl. partial/truncation and unknown-precision
      evidence markers.
- [ ] Zero business-rule duplication: the TUI module contains no SQL, no
      cursor construction, no branch-selection logic (code review gate).
- [ ] `cargo deny check` green with the two new dependencies; licenses
      recorded; no bans tripped.
- [ ] Full gates: fmt / clippy -D warnings / workspace tests; existing CLI
      e2e (44) + MCP e2e (9) untouched and green.

## Out of scope

- Mouse support, theming/colors config, resize edge-perfection, non-UTF-8
  terminals; ingest/sync from inside the TUI (read-only).
- List/browse-all-sessions screen (search-first Preview; list lands with a
  session-kind filter in a later round).
- Publishing/installation (child 7); marking providers Beta.
