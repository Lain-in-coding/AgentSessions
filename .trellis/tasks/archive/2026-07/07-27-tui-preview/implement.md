# Implement: TUI Preview

Gates: `cargo fmt --all --check` && `cargo clippy --workspace --all-targets
-- -D warnings` && `cargo test --workspace` && `cargo deny check`.

## Phase A — pre-wire (main session)

- [ ] `mod tui;` + stub `tui/mod.rs` (usage error), `run()` interception,
      help line; `cargo test -p agentsessions-cli` green with the stub.

## Phase B — implement (single agent tui-impl)

- [ ] Add ratatui/crossterm to workspace + cli Cargo.toml (design §0.1).
- [ ] `tui/core.rs` pure Model/Msg/Effect/update + view-model + unit tests
      (design §2, §4). `tui/mod.rs` glue (design §3).
- [ ] Agent verifies: `cargo test -p agentsessions-cli`, clippy -p, fmt -p
      --check; reports deviations. No git, write-survival checks.

## Phase C — integrate & verify (main session)

- [ ] Independent review: no business rules in tui/, pure core has no
      ratatui/crossterm imports, raw-mode guard on all exits.
- [ ] Full gates incl. `cargo deny check` (new deps!).
- [ ] Non-tty smoke: piped stdin → usage error exit 2, no escape codes on
      stdout.

## Phase D — spec, commit, wrap (main session)

- [ ] Spec index update (tui section).
- [ ] Commits: (a) planning docs; (b) deps + module + wiring; (c) spec.
      Push, archive, journal. User action item: one interactive smoke run.
