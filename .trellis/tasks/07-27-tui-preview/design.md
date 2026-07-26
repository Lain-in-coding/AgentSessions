# Design: TUI Preview

Frozen spec for the implementing agent. Deviations must be reported, never
silently applied.

## 0. Design decisions

1. **Dependencies (owner-visible)**: `ratatui = "0.29"` + `crossterm =
   "0.28"` — a known-compatible pair, both MIT; added to
   `[workspace.dependencies]` and referenced with `.workspace = true` from
   the cli crate. First UI deps in the repo (PRD callout). If `cargo deny`
   flags a transitive issue, STOP and report; do not edit `deny.toml`
   yourself.
2. **Elm-style split for testability**: `src/tui/core.rs` is PURE (no
   ratatui/crossterm/store imports): `Model`, `Msg`, `Effect`,
   `update(Model, Msg) -> (Model, Option<Effect>)`, and view-model
   functions returning plain `Vec<String>`/`String`. `src/tui/mod.rs` is
   the thin glue: terminal init/restore, crossterm event → `Msg` mapping,
   `Effect` execution against `App`, ratatui widget assembly from
   view-model strings. All unit tests live against `core.rs`; the glue has
   no tests (manual smoke instead).
3. **Synchronous effects**: searches/context loads run inline on the UI
   thread (no worker threads, no async). A Preview may briefly block on a
   query; documented, not hidden.
4. **Terminal lifecycle**: `mod.rs` guards with `ratatui::init()` /
   `ratatui::restore()` semantics (alternate screen + raw mode + panic-hook
   restore). Before init: if `!std::io::stdout().is_terminal()` (std
   `IsTerminal`) → usage error "tui requires an interactive terminal"
   (exit 2, no raw mode touched). Terminal-io failures after init →
   `CanonicalCode::SourceIo` (exit 5) with restore guaranteed.
5. **No business rules in the TUI**: paging = pass `next_cursor` back into
   `AppRequest::Search`; branch policy = `ContextPolicy` passed to
   `AppRequest::Context`; hit→session resolution = `AppRequest::Show` on
   the hit id, then read the canonical payload's `"session"` field (missing
   → status-line error "hit has no session (index-only row)", stay on
   Results). No SQL, no cursor math, no selection logic.
6. **Output modes**: `tui` ignores `--output`/`--robot`/`--request-id`
   (interactive surface). Machine surfaces are untouched.
7. **Page semantics**: `n` on Results appends the next page's hits to the
   accumulated list (selection jumps to the first newly loaded row);
   `next_cursor`/`has_more` state comes from the App response only.

## 1. File ownership

| Path | Owner |
|---|---|
| `crates/agentsessions-cli/src/tui/mod.rs`, `src/tui/core.rs` | agent **tui-impl** (stub `mod.rs` exists at start) |
| `crates/agentsessions-cli/Cargo.toml`, root `Cargo.toml` (dep entries only) | agent **tui-impl** |
| `crates/agentsessions-cli/src/main.rs` (`mod tui;` + interception + help) | main session ONLY |
| spec indexes, deny.toml (if ever), commits | main session ONLY |

## 2. Frozen contracts

Entry (stub already wired):

```rust
pub(crate) fn run(store: &SqliteStore) -> Result<protocol::Outcome, CliError>
```

Core types (names frozen; fields may grow as needed):

```rust
pub(crate) enum Screen { Search, Results, Context }
pub(crate) struct Model { screen, input, hits, selected, next_cursor,
                          has_more, page_note, context, scroll, policy,
                          status, quit, generation, ... }
pub(crate) enum Msg { Key(KeyInput), SearchLoaded(SearchPage),
                      ContextLoaded(ContextView), EffectFailed(String), ... }
pub(crate) enum Effect { Search { query, cursor: Option<String> },
                         ResolveAndLoadContext { hit_id: String },
                         LoadContext { session_id: String, policy } }
pub(crate) fn update(model: Model, msg: Msg) -> (Model, Option<Effect>)
```

`KeyInput` is core.rs's own tiny enum (Char(char), Enter, Esc, Up, Down,
PgUp, PgDn, Backspace, CtrlC) so core stays crossterm-free; mod.rs maps
`crossterm::event::KeyEvent` → `KeyInput`. `SearchPage`/`ContextView` are
plain-data structs built by mod.rs from `AppResponse` (hits as (id, score);
context as message lines + per-message evidence precision + truncation +
warnings + generation).

Key map (frozen; tests assert it):
- global: Ctrl+C → quit.
- Search: printable chars append; Backspace pops; Enter (non-empty input) →
  `Effect::Search{cursor: None}` and clears accumulated hits; Esc on empty
  input → quit, Esc on non-empty input → clear input.
- Results: Up/Down move selection (saturating); Enter →
  `Effect::ResolveAndLoadContext`; `n` when `has_more` →
  `Effect::Search{cursor}`; `q` quit; Esc → Search.
- Context: Up/Down scroll ±1, PgUp/PgDn ±10 (saturating); `f` → toggle
  policy + `Effect::LoadContext` re-fetch; `q` quit; Esc → Results.

View-model (pure, tested): `hit_lines(&Model) -> Vec<String>` (selected row
prefixed `> `), `context_lines(&Model) -> Vec<String>` (one line per
message: `role: text-first-line [precision]`, unknown precision renders
`[unknown]`), `status_line(&Model) -> String` (generation, `PARTIAL:
<reason>` when truncated, first warning, or the last error as
`error [<code>]: <msg>`), `title_line(&Model) -> String` per screen.

## 3. Glue specifics (mod.rs)

- Event loop: `crossterm::event::poll(Duration::from_millis(250))` → `read`
  → only `KeyEventKind::Press` (Windows sends Release too — filter or keys
  double-fire) → `Msg::Key` → `update` → execute returned `Effect` inline →
  feed resulting `Msg` back into `update` → draw.
- Effects: build `AppRequest` exactly like main.rs dispatch does
  (`App::new(store_ref(store), store_ref(store))`,
  `ResponseBudget::default()`, search limit 20); project `AppResponse`
  into `SearchPage`/`ContextView`; `AppError` → `ProtocolError` → 
  `Msg::EffectFailed(format!("error [{code}]: {message}"))` — the UI keeps
  running; errors are status-line content, never a crash or exit.
- Draw: ratatui `List`/`Paragraph` fed from view-model strings; no styling
  beyond the selected-row prefix and a reversed status bar.

## 4. Tests (core.rs `#[cfg(test)]`)

Reducer: typing/backspace edits input; Enter empty input is a no-op; Enter
non-empty emits Search effect and resets hits; SearchLoaded populates hits +
cursor + moves to Results (empty page → status "no hits", stay functional);
`n` with has_more emits Search with the stored cursor, SearchLoaded then
APPENDS and selects first new row; `n` without has_more is a no-op; Up/Down
saturate at list edges incl. empty list; Enter on a hit emits
ResolveAndLoadContext with the selected id; ContextLoaded switches to
Context with scroll 0; `f` toggles Mainline↔Full and emits LoadContext;
scroll saturates; Esc chain Context→Results→Search(clear)→quit; `q` quits on
Results/Context but TYPES into the search input; Ctrl+C quits everywhere;
EffectFailed sets status and does not change screen. View-model: selected
prefix, precision markers incl. unknown, PARTIAL status on truncation,
error status rendering.

## 5. Integration (main session)

1. Pre-wire: `mod tui;` + stub `tui/mod.rs` (usage error), interception in
   `run()` right after the mcp interception (`tui` is read-only), help line.
2. After tui-impl lands: read the module, run full gates + `cargo deny`,
   non-tty smoke (`tui` with piped stdin → exit 2 usage error, terminal
   untouched). True interactive smoke is a user action item at wrap-up.
3. Spec update (cli backend index: tui section). Commit slicing:
   (a) planning docs; (b) deps + tui module + wiring; (c) spec docs.
