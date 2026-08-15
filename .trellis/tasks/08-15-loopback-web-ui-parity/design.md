# Loopback Web UI parity — Design (slice 1)

> Task: `08-15-loopback-web-ui-parity` (parent `08-15-open-source-product-roadmap`)
> Slice 1: `asg serve` loopback HTTP API + minimal static UI. Read-only GETs only.

## Context

The PRD requires `asg serve` (Q40): a loopback HTTP API that reuses the
Application ADT and the Robot contract — the same search/budget/error/cursor/
generation semantics, with no CLI/Web behavior fork; loopback-only binding,
random per-run token, Host/Origin validation (Q27); browser as a pure protocol
client, no Electron/Tauri; zero telemetry (no external requests from the page).

This slice delivers the minimal real vertical: the HTTP server, the auth
model, four read-only API routes, an embedded static UI, and parity tests.
LAN mode, mutation routes, SSE, gzip, and pagination UI are explicitly
deferred (see Deferred items).

Borrowed patterns (per `08-15-open-source-product-roadmap/research/
2026-08-15-borrowable-code-inventory.md` W1): the auth middleware chain of
`agentsview/internal/server/auth.go` (MIT License, (c) 2026 Kenn Software
LLC) — loopback determination, Host check, bearer token, fail-closed
forwarding-header detection. Ported idea-level to Rust/axum; attribution
recorded in `crates/agent-session-grep-cli/src/serve.rs` module docs.
`cc-switch` session-management structure was reviewed (inventory J9/J10:
head/tail line reading, timestamp normalization) — those are provider-crate
concerns for later slices, not needed by the HTTP layer.

## Decisions

### D1 — HTTP stack: axum 0.8 + tokio + rust-embed + getrandom (CLI crate only)

- `axum = "0.8"` — mainstream, lightweight, tower-based; the only HTTP stack
  (no Electron/Tauri, no bespoke server).
- `tokio = { version = "1", features = ["rt", "net", "signal"] }` — a single
  current-thread runtime; `net` for the listener, `signal` for Ctrl+C
  graceful shutdown. No `macros`/`time` features needed.
- `rust-embed = "8"` — static UI compiled into the binary (no build step, no
  runtime file reads, no external network requests from the page).
- `getrandom = "0.4"` — system CSPRNG for the per-run token (16 bytes, hex).
- No `serde` derive needed (handlers return `serde_json::Value`); no
  `tower-http`, no `mime_guess` (three static assets, hand-written
  content-type map), no reqwest (tests use a raw `TcpStream` HTTP/1.1 client).

Rationale: these four crates are the smallest mainstream set that covers the
slice; everything else is std + existing workspace crates.

### D2 — `serve` subcommand wiring (read-only store, no writer lease)

- `agent-session-grep --db <path> serve` — no flags, no positionals (usage
  error otherwise). Registered in `known_subcommand`, `subcommand_help_text`,
  top-level `help_text`, the unknown-subcommand suggestion list, and the
  `KNOWN_COMMANDS` test constant.
- Like `mcp`/`tui`, `serve` takes over the process: it is dispatched in
  `run()` before `dispatch()`, uses `SqliteStore::open` (read-only — NOT in
  the writer-lease set), and ignores output-mode/request-id flags.
- stdout stays zero-output; the startup URL (with token) and shutdown
  guidance print to stderr only.
- Binds `127.0.0.1:0` (ephemeral port — no fixed-port collision risk; the
  printed URL is the single entry point). No `--port`, no `--lan` this slice.

### D3 — Auth model (agentsview `auth.go` pattern, idea-level port)

Every request (API and static page alike) passes one middleware guard:

1. **Host check** — the Host header hostname must be `127.0.0.1`,
   `localhost`, or `::1` (with any port). Anything else → 403
   (`forbidden_host`). This is the DNS-rebinding defense (we bind loopback,
   but a rebinding page would carry a non-loopback Host).
2. **Forwarding-header check** — presence of `X-Forwarded-For`,
   `X-Real-IP`, or `Forwarded` means the connection was relayed by a proxy,
   i.e. not a direct local one → 403 (`forbidden_proxy`). Fails closed
   (agentsview `hasForwardingHeader` semantics).
3. **Token check** — the per-run token (32 hex chars from 16 CSPRNG bytes)
   must match either the `Authorization: Bearer <token>` header or the
   `?token=` query parameter → 401 (`unauthorized`) otherwise.

Deviations from agentsview (deliberate, documented in serve.rs):
- agentsview restricts `?token=` to SSE paths; here the task requires token
  via query *or* header on every request, and the printed URL must open
  directly in a browser — query token is accepted everywhere. Risk is bounded
  by per-run randomness + loopback-only bind.
- agentsview echoes `Origin` in CORS headers on 401s so a cross-origin
  frontend can read the failure; our UI is same-origin and cross-origin
  callers should get no information — no CORS headers at all (fail closed).
- Token comparison is plain equality, not constant-time: on loopback with a
  per-run secret, timing attacks are out of threat model (documented).

Transport-layer rejections use transport codes (`forbidden_host`,
`forbidden_proxy`, `unauthorized`) that are NOT part of the Robot error
catalog — they are HTTP-boundary facts, not business failures.

### D4 — Routes (read-only; canonical JSON only, no behavior fork)

| Route | ADT request | Notes |
|---|---|---|
| `/`, `/index.html`, `/app.js`, `/style.css` | — | Embedded static UI; `nosniff` + content-type headers; guarded like everything else |
| `/api/status` | `AppRequest::Status` | catalog/generation/placement counts |
| `/api/sessions?limit=` | `AppRequest::List` then filter `ses_v1_*` | Stable wire-id order; default limit 100; `page.has_more` is honest; cursor deferred (the List cursor spans all entity kinds, meaningless for a sessions-only view) |
| `/api/search?q=&limit=&cursor=` | `AppRequest::Search` | `q` required (empty → 400); default limit 20 (same as CLI); cursor passthrough; default `ResponseBudget` (4 MiB / 1000 items — bounded responses reuse the existing budget constants) |
| `/api/session/{id}` | `AppRequest::Context` (mainline) | Session detail pane (messages + evidence); invalid wire id → 400; missing session → 404 |

Success body: `{ "data": <canonical Robot `data`>, "page": {next_cursor,
has_more}, "warnings": [...] }` — `data` is produced by the same
`render()` projection the CLI/Robot envelope uses. Errors: Robot error body
`{error: {code, message, retryable, details}}` with HTTP status mapped from
the canonical code: invalid_request/cursor_invalid/cursor_expired → 400,
not_found → 404, generation_mismatch/schema_incompatible → 409, everything
else → 500. Partial (budget truncation) stays HTTP 200 with
`data.truncation.truncated = true` — the CLI exits 10, but HTTP transports
truncation in the body (documented; no exit-code notion on the wire).

No response ever contains a source/transcript filesystem path — `data` is
the same path-free canonical projection as the robot surface.

### D5 — Parity: same projection, tested at two levels

- The HTTP handlers call `App::handle` + `render()` — the identical code path
  as the CLI `dispatch` arms, so parity holds by construction.
- **Unit parity** (`serve.rs` tests): HTTP `/api/search` body vs a direct
  `App::handle` + `render()` call on the same in-memory store. Structural
  comparison with float tolerance (1e-12 relative): serde_json float parsing
  has a measured 1-ulp noise through the JSON text round-trip (9e-7-magnitude
  scores shift 1 ulp after serialize→parse), so exact `Value` equality is not
  achievable across the transport. Strings/integers/booleans compare exactly;
  only floats tolerate the 1-ulp transport noise. A determinism test pins
  that the same query twice in-process is bit-identical.
- **E2E parity** (`tests/e2e.rs`): real binary `--robot search` envelope
  `data` vs real binary `serve` `/api/search` `data` on the same fixture db.
  Both sides went through the same serialize→parse pipeline, so this
  comparison is exact (shared fields identical, including floats).

### D6 — Static UI (vanilla, embedded, offline-verifiable)

- Three assets in `crates/agent-session-grep-cli/assets/` embedded via
  rust-embed: `index.html`, `app.js`, `style.css`. No build step, no CDN,
  no external font/icon/network requests.
- CSP meta tag: `default-src 'self'; script-src 'self'; style-src 'self';
  connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action
  'none'; object-src 'none'` — script/style/connect all same-origin.
- Page features (minimal but functional): status bar (generation/counts via
  `/api/status`), search box → hits list (id/score/session/text), session
  list → click to open detail pane (messages with role/timestamp), error
  surfaces. Token read from the page URL `?token=` and sent as both query
  param and Bearer header; all fetches relative.
- All message/session data rendered via `textContent` (no innerHTML with
  data) — XSS-safe by construction.
- **Offline verification** (zero telemetry): with the network disabled
  (airplane mode / firewall block), `serve` starts, the page loads, search
  and session views work — the page makes no external request by design; a
  `connect-src 'self'` CSP violation in devtools console would be the only
  way an external fetch could surface, and none exists in the code.

### D7 — Static-asset responses

`X-Content-Type-Options: nosniff` on assets; explicit content types
(`text/html; charset=utf-8`, `text/javascript; charset=utf-8`,
`text/css; charset=utf-8`). Cache headers deferred (no aggressive caching
this slice).

## Compatibility

- Additive: new `serve` subcommand + 4 new crate deps in the CLI crate only.
- No protocol/envelope/schema changes; `list_providers` and all MCP tools
  unchanged.
- `Cargo.lock` gains the axum/tokio/rust-embed/getrandom trees (all MIT /
  Apache-2.0, matching `deny.toml` allow-list).

## Rollout / rollback

- Rollout: serve module + assets → wiring → tests.
- Rollback: revert `serve.rs` + `assets/` + the `serve` subcommand wiring and
  the Cargo.toml additions; nothing else is affected. The binary without
  `serve` behaves exactly as before.

## Deferred items (documented, not built)

- **LAN mode** (`--lan` opt-in + warning + audit log) — explicit, per PRD;
  not needed for the loopback slice.
- **Mutation routes** (sync/ingest/resume preview/handoff) — next slice;
  they bring Origin checks, CORS preflight handling (OPTIONS), and the
  writer-lease lifecycle for the server process.
- **CORS/Origin echo on auth errors** (agentsview `setCORSOnAuthError`) —
  only meaningful once cross-origin clients exist (mutation slice).
- **UI polish**: pagination (cursor "load more"), session metadata
  (titles/dates per the Human five-column table), evidence span highlighting,
  provider/time filters.
- **SSE** (index progress / background tasks), **gzip**, immutable-cache
  headers, `<base href>` support (agentsview W4/W5 pattern).
