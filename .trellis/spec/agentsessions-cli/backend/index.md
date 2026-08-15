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
- `src/tui/` — interactive read-only TUI Preview (`core.rs` pure reducer/view-model, `mod.rs` terminal glue)

---

## Pre-Development Checklist

Before writing code in this crate:

- [ ] Argument parsing is **hand-written**, no third-party CLI framework. Match
      the existing style in `main.rs` (`parse_db_flag`, `command_name`,
      `arg(rest, i, usage)`, `extract_flag` for value flags); do not introduce
      clap/structopt. Boolean flags use `take_bool_flag` (MCP uses `opt_bool`);
      every new flag must be registered in every prefix scanner that skips
      value-bearing flags (`command_name`, request-id/output-mode scans),
      otherwise a value can hide a later `--robot`/`--output` flag.
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
      schema-defined but v1 emits none. Progress identifies inputs by bounded
      source ordinal/count only; it never includes a source path or native ID.
- [ ] `--request-id` is validated by `protocol::valid_request_id`
      (`^[A-Za-z0-9._:-]+$`, 1-128) and echoed verbatim in every frame;
      invalid values are usage errors, never silently replaced.
- [ ] Source staging de-duplicates stable Message entities but retains every
      `MessagePlacement` and `MessageEdge`. Native IDs are preferred; the
      no-native fallback is `Unstable` and derives from provider, variant,
      content-addressed document, and ordinal — never source path.
- [ ] Preserve the complete provider `ParseReport`: `committed` must equal
      emitted occurrences, `skipped` is reported honestly, and only zero-skipped
      sources are relation-complete. Parent/sidechain/span/session payload keys
      are compatibility projections; Application context consumes typed edges.
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
      frozen: search_sessions / get_session_context / get_message /
      list_sessions / list_providers / get_status / doctor.
- [ ] MCP legacy compatibility duplicates a successful payload in
      `structuredContent` and `content[0].text`. Application/Robot byte estimates
      cover one rendered payload, not the complete duplicated JSON-RPC frame;
      do not claim a hard MCP frame-byte limit until the MCP boundary enforces
      and tests that separate contract.
- [ ] TUI (`tui` subcommand, `src/tui/`): all state transitions and rendering
      decisions live in the PURE `core.rs` (no ratatui/crossterm/store/App
      imports; unit-tested without a terminal); `mod.rs` is thin glue only.
      Data access goes through `AppRequest::{Search, MessageContexts, Context}`
      and the shared `render()` projection — no SQL, compatibility-alias
      session selection, cursor construction, or branch logic in the TUI.
      Reverse candidates are grouped by distinct Session: zero is index-only,
      one opens, several are explicit ambiguity. Evidence aligns by
      placement/occurrence ID. Non-tty stdout → usage error before
      raw mode; terminal restore covers normal, error, and panic paths; App
      errors become status-line text, never a crash. The TUI is the only
      place the `ratatui`/`crossterm` workspace deps may be used.
- [ ] New error conditions map to an existing `CanonicalCode`. If you need a new
      code, register it in the `CanonicalCode` enum, its `as_str`, `exit_code`,
      `retryable`, **and** the published `schemas/robot/v1/error-catalog.json`
      (the schema-drift test enforces they stay in sync).
- [ ] Error envelopes and human diagnostics must preserve canonical category
      while keeping messages bounded and path/native-ID-free. Never relay raw
      provider, SQLite, or conflict payloads to stdout or stderr.

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
- [ ] Surface changes are mirrored in `scripts/install/smoke.{ps1,sh}`. That
      script is the installed-artifact contract check (CLI envelopes, exit
      codes, MCP handshake) and runs on all three OS targets in the `installer`
      CI job; a new command or exit-code rule that it does not assert is
      untested against a real install.

---

## Operational surfaces and evidence

- **Install scripts** (`scripts/install/`) build from source with `--locked` and
  copy one binary into a user-level prefix. They must never modify `PATH`, the
  registry, or shell profiles, never request elevation, and never download
  anything beyond what `cargo build` fetches — environment changes are the
  user's decision, and that constraint is what keeps uninstall a single-file
  delete. `uninstall` removes exactly that one file, is idempotent (a second run
  reports "not installed" and exits 0), and never deletes a directory
  recursively.
- **Smoke script** never builds. It requires an already-built binary and fails
  if absent, so a build failure can never masquerade as a smoke pass. Its
  fixture is synthetic and inlined; it must not read real transcripts.
- **Real-data regression** (`scripts/evidence/real_data_regression.py`) runs
  only locally against a throwaway temp store. Its report is a closed field set
  of aggregate counts and invariant verdicts — no message text, source paths,
  native ids, fingerprints, usernames, or hostnames. Reports default under the
  gitignored `evidence-output/`; a report generated from real data is never
  committed. Adding a report field means extending `validate_report` and the
  privacy test in the same change.
- **No-loss accounting.** Provider-emitted source occurrences must equal
  persisted source-placement claims and skipped records must be zero. Stable
  Message count is a separate de-duplicated census; legal shared identity is
  not parse loss.
- **Evidence honesty.** Rows in `docs/operations/core-beta-evidence-matrix.md`
  follow that file's status vocabulary literally: `ci_configured_only` until a
  specific successful run is named, and a failing local run is recorded as a
  failing run. Neither a passing smoke script nor a green CI job is
  clean-machine, minimum-OS, or signed-release evidence — hosted runners ship a
  preinstalled toolchain.
- **Historical blocker (recorded 2026-07-27).** Real corpora proved stable
  Message identity is shared while session/document/span/parent are contextual.
  The v7 implementation now stages placements/edges and removes those fields
  from stable conflict authority without weakening role/text/timestamp
  conflicts. This is code/process evidence only until the authorized aggregate
  real-data regression is rerun successfully; providers remain Experimental.
- **Resume protocol (ADR-0009, recorded 2026-08-14).** `get-session-resume`
  CLI command and `get_session_resume` MCP tool return ONLY the fixed nullable
  fields: `session_id`, `provider_id`, `resume_available`,
  `provider_session_id`, `original_working_directory`, `unavailable_reason`.
  Never emit `command`, `resume_command`, `source_path`, or `transcript_path`
  (asserted in `tests/mcp_e2e.rs` and `tests/e2e.rs`). Resume Metadata is
  isolated in its own port struct and SQLite table; it never enters FTS text,
  opaque session payload, diagnostics, progress frames, or error messages.
  Multi-Session Sources fail closed. Canonical `session_id` (`ses_v1_*`) is the
  catalog identity; the Provider-native ID is Resume Metadata only — the two
  are not interchangeable and `ses_v1_*` cannot be reversed to the native ID.
- **Scoped canonical Session identity (2026-08-14).** Native Session IDs are
  namespaced by `SessionIdentityNamespace` (provider + installation
  namespace) before hashing (`StableId::native_session_scoped`). The
  composition-root `installation_namespace` derives the namespace from the
  provider data-root prefix path (`.claude`/`.codex`) or, for unknown
  providers, the source's parent directory. Known RFC-0001 §5.1 debt: this
  derives from absolute path and lacks a persisted registry, `id_alias` table,
  and path case normalization; relocation does not preserve Session identity.
  The domain layer is correct; the gap is composition-root only.
- **Human session table (2026-08-14).** Human search renders a frozen
  five-column table: `日期 | Provider | 会话标题 | 工作目录 | Session ID`.
  Provider and Session ID are never truncated; title uses tail ellipsis;
  working directory uses middle collapse; missing renders `—`. 日期 is the
  per-Session latest message timestamp truncated to `YYYY-MM-DD` (batched
  `MAX(json_extract(catalog.payload,'$.timestamp'))`, no N+1); 会话标题 is the
  highest-relevance hit `text` on the current page. Robot/MCP output is
  unchanged (Human-mode only projection).

---

**Language**: write all guideline docs in **English**.
