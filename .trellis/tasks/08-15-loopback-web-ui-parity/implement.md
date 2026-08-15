# Loopback Web UI parity — Implementation (slice 1)

## Preconditions

- Worktree `C:/AgentSessions/.claude/worktrees/agent-ab5d10e0870a8bce9` on top
  of the verified 08-13/08-14 baseline (main branch).
- Design: `design.md` (decisions D1-D7); research:
  `08-15-open-source-product-roadmap/research/2026-08-15-borrowable-code-inventory.md`
  (W1 agentsview auth pattern, MIT attribution).
- Isolated build target to avoid polluting other agents' artifacts:
  `CARGO_TARGET_DIR='C:/AgentSessions/target-08-15-web-ui'`.

## Steps (as executed)

### 1. Dependencies (CLI crate only)

`crates/agent-session-grep-cli/Cargo.toml`:
`axum = "0.8"`, `tokio = { version = "1", features = ["rt", "net", "signal"] }`,
`rust-embed = "8"`, `getrandom = "0.4"`. All four were already present in the
local cargo registry cache (axum 0.8.9, rust-embed 8.12.0, getrandom 0.4.3);
`Cargo.lock` updated by the build. All licenses MIT/Apache-2.0 (deny.toml
allow-list).

### 2. `src/serve.rs` (new module)

- `pub(crate) fn run(store: SqliteStore) -> Result<Outcome, CliError>` —
  current-thread tokio runtime; binds `127.0.0.1:0`; prints
  `http://127.0.0.1:PORT/?token=<hex>` and shutdown guidance to stderr;
  blocks until Ctrl+C (graceful shutdown).
- `ServerState { store: Mutex<SqliteStore>, token: String }` — shared via
  `Arc`; per-request `App::new(&store, &store)` under the mutex.
- `guard` middleware (covers ALL routes incl. static + 404 fallback):
  Host loopback check (403 `forbidden_host`) → forwarding-header check
  (403 `forbidden_proxy`) → token check (401 `unauthorized`; `?token=` or
  `Authorization: Bearer`).
- Handlers: `status`, `search` (q required; limit default 20; cursor
  passthrough; default `ResponseBudget`), `sessions` (`List` at
  `budget.max_items` then filter `ses_v1_*`; limit default 100; honest
  `has_more`), `session_detail` (`Context` mainline; invalid id 400,
  missing 404), static assets (rust-embed, `nosniff`).
- Response contract: success `{data, page, warnings}` with `data` from the
  shared `render()` projection; errors as Robot error body with HTTP status
  mapping (400/404/409/500).
- Hand-written query parsing (`percent_decode` + `query_params`, ~25 lines)
  instead of a url crate.
- Attribution: agentsview MIT (c) 2026 Kenn Software LLC documented in the
  module header; `has_forwarding_header` carries the source reference.

### 3. Static UI assets

`crates/agent-session-grep-cli/assets/`: `index.html` (CSP meta tag
`default-src 'self'; script-src 'self'; ...`), `app.js` (vanilla; status bar,
search, session list, detail pane; token from page URL; relative fetches;
`textContent` only), `style.css`. No external URLs anywhere.

### 4. `main.rs` wiring

- `mod serve;`
- `run()`: `serve` interception next to `mcp`/`tui` (before `dispatch`),
  usage error on extra positionals; store opened read-only (not in the
  writer-lease set).
- `known_subcommand`, `subcommand_help_text`, `help_text` COMMANDS,
  unknown-subcommand suggestion list, `KNOWN_COMMANDS` test constant.

### 5. Tests

- `serve.rs` unit tests (9): loopback bind only; token required (401 missing/
  wrong; query + Bearer accepted); non-loopback Host rejected (403) with
  loopback names accepted; forwarding header rejected (403); search data
  parity vs direct `App::handle`+`render` (structural, float tolerance
  1e-12); sessions list is `ses_v1_*` only + detail/404/400; static page
  served with token; `hostname_of`/`percent_decode` unit cases; same-query
  determinism.
- `tests/e2e.rs`: `serve_api_matches_cli_robot_search_json` — real binary:
  ingest fixture → `--robot search` envelope `data` vs `serve`
  `/api/search` `data` (exact comparison; both sides share the
  serialize→parse pipeline); 401/401/403 guards; child killed after.

### 6. Known measurement (documented in design.md D5)

serde_json float parsing has 1-ulp noise through a JSON text round-trip
(measured on 9e-7-magnitude BM25 scores). Unit parity therefore compares
floats with 1e-12 relative tolerance; the e2e parity (two text round-trips)
compares exactly.

## Validation (all green, run with the isolated target dir)

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --release`

## Review gates

- No absolute transcript path in any response/error/progress (data is the
  path-free canonical projection; transport errors are fixed strings).
- `--db <path> serve` runs with a read-only store (no writer lease).
- Every written file verified on disk after write (Windows Defender hazard).
- Do NOT commit/push.
