# Implement: stdio MCP v0 and Skill beta

Execution order. Validation command set (the "gates"):
`cargo fmt --all --check` && `cargo clippy --workspace --all-targets -- -D warnings`
&& `cargo test --workspace` && `cargo deny check`.

## Phase A — pre-wire (main session)

- [ ] Add `mod mcp;` + stub `crates/agentsessions-cli/src/mcp.rs` with the
      frozen `serve` signature returning a usage error; intercept `mcp` in
      `run()` after the read-only store open; add help text line.
- [ ] `cargo test -p agentsessions-cli` green with the stub (rollback point:
      revert the 3 hunks).

## Phase B — parallel agents

- [ ] Dispatch **mcp-server** → implement `mcp.rs` per design §2-§3 with unit
      tests (version negotiation, initialize gate, batch rejection, params →
      AppRequest mapping incl. -32602 cases, tool list shape, business error
      → isError projection). Owned file only; verify write survival.
- [ ] Dispatch **mcp-e2e** → `tests/mcp_e2e.rs` per design §4 (9 scenarios,
      purity assertion in helper) + `skills/agentsessions/SKILL.md` per
      design §5. Compile-check tests (`cargo test -p agentsessions-cli
      --test mcp_e2e --no-run`); full runs only make sense after mcp-server
      lands.

## Phase C — integrate & verify (main session)

- [ ] Independently verify both deliverables (read files, run
      `cargo test -p agentsessions-cli` incl. `--test mcp_e2e`).
- [ ] Reconcile mismatches: message owning agent, or minimal mechanical fix
      in main session (report it).
- [ ] Full gates green at workspace level.
- [ ] Manual smoke: pipe an initialize + tools/list session into
      `agentsessions --db <tmp> mcp` on Windows, eyeball frames.

## Phase D — spec, commit, wrap (main session)

- [ ] Update `.trellis/spec/agentsessions-cli/backend/index.md` (MCP section).
- [ ] Commit plan → user confirmation → commits per design §6.4 → push.
- [ ] `task.py archive` + `add_session.py` journal.
