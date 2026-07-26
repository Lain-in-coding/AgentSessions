# agentsessions-cli — Backend Guidelines

> The composition root. The only crate that binds abstract ports to concrete
> adapters and owns the human/robot-facing surface.

---

## Role in the architecture

`agentsessions-cli` is the **composition root** (see the hexagonal layering in
the plan §5): it is the single place that `new`s a concrete `SqliteStore` and
injects it into `App`. Every other crate is backend-agnostic. This crate also
owns the CLI argument parsing, the Robot v1 protocol envelope, and the mapping
from `AppError` to canonical error codes / exit codes.

Real files:
- `src/main.rs` — arg parsing, command dispatch, composition root, platform paths
- `src/protocol.rs` — Robot v1 envelope, `CanonicalCode` error catalog, output modes
- `src/human.rs` — human-mode success renderer (text lines, no envelope)
- `src/mcp.rs` — stdio MCP server (JSON-RPC 2.0, contract §8 tools)

---

## Pre-Development Checklist

Before writing code in this crate:

- [ ] Argument parsing is **hand-written**, no third-party CLI framework. Match
      the existing style in `main.rs` (`parse_db_flag`, `command_name`,
      `arg(rest, i, usage)`, `extract_flag` for value flags); do not introduce
      clap/structopt. New value flags must also be added to `command_name`'s
      skip list so error envelopes label the right command.
- [ ] Every new subcommand goes through `dispatch()` and returns
      `(&'static str command, Outcome, serde_json::Value, protocol::Page, Vec<String> warnings)`.
      Errors return `CliError`, never `panic!` / `unwrap` on user input.
      App responses are projected by `render()` — truncated results become
      `Outcome::Partial` (process exit 10, contract §5); pagination tokens fill
      the envelope `page.next_cursor` / `page.has_more`; honest degradations
      (e.g. unknown-precision evidence) become `warnings` entries.
- [ ] Output truth table (contract §6): `emit_result()` is the single success
      exit — Human mode renders via `human::render_success` (text lines, no
      envelope, warnings to stderr); Json/Jsonl emit one envelope. ALL protocol
      stdout goes through `protocol::write_stdout_line` (EPIPE → silent exit 0,
      other write errors → stderr + exit 5) — never bare `println!` for frames.
- [ ] Progress frames (`protocol::progress_frame`) are emitted ONLY under
      `--output jsonl` (never Json/`--robot`/Human). `diagnostic` frames are
      schema-defined but v1 emits none.
- [ ] `--request-id` is validated by `protocol::valid_request_id`
      (`^[A-Za-z0-9._:-]+$`, 1-128) and echoed verbatim in every frame;
      invalid values are usage errors, never silently replaced.
- [ ] Message canonical payloads persist BOTH `parent_native_id` (provider
      fact) and `parent` (resolved `msg_v1_` wire id via the same native
      derivation rule) — context assembly consumes the resolved edge.
- [ ] Read paths use `SqliteStore::open`; write paths (`index`/`ingest`/`sync`)
      use `SqliteStore::open_for_write` to take the writer lease. Do not open a
      write connection for a read-only command.
- [ ] stdout carries **only** protocol data; human diagnostics go to stderr
      (plan §7.5). `protocol::emit` no longer exists — success goes through
      `emit_result`, errors through `error_envelope`, every frame through
      `write_stdout_line`.
- [ ] MCP (`mcp` subcommand, `src/mcp.rs`, contract §8): stdout carries only
      JSON-RPC frames (same `write_stdout_line` exit; EOF → exit 0). Handlers
      are thin — validate protocol, build `AppRequest`, project via the same
      `render()`; never duplicate search/branch/pagination/budget rules and
      never expose file read/SQL/exec. Error split: protocol problems
      (malformed JSON, batch arrays, unknown method/tool, invalid params,
      not-initialized) → JSON-RPC errors (`-32602` carries
      `data.canonical_code = invalid_request`); business failures after a
      well-formed `AppRequest` → `isError: true` tool results carrying
      `structuredContent.error.canonical_code`. `initialize`/`ping` bypass the
      initialize gate; supported protocol versions are pinned in
      `SUPPORTED_PROTOCOL_VERSIONS` (negotiation never lies). Tool set is
      frozen: search_sessions / get_session_context / list_sessions /
      list_providers / get_status / doctor.
- [ ] New error conditions map to an existing `CanonicalCode`. If you need a new
      code, register it in the `CanonicalCode` enum, its `as_str`, `exit_code`,
      `retryable`, **and** the published `schemas/robot/v1/error-catalog.json`
      (the schema-drift test enforces they stay in sync).

---

## Quality Check

Before proposing a commit for this crate:

- [ ] `cargo fmt --all --check` clean.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo test -p agentsessions-cli` green, including `tests/e2e.rs` and
      `tests/mcp_e2e.rs` (both drive the real compiled binary end-to-end).
- [ ] Any protocol/envelope change is reflected in both `protocol.rs` tests and
      the `tests/e2e.rs` envelope-shape assertions.
- [ ] Exit codes match the error catalog (invalid_request/cursor_invalid/
      cursor_expired → 2, not_found → 4, source_io/source_changed → 5,
      catalog/writer_busy → 6, provider → 7, schema_incompatible/
      generation_mismatch → 9, partial success → 10, internal → 70).

---

**Language**: write all guideline docs in **English**.
