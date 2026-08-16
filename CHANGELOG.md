# Changelog

All notable changes to agent-session-grep are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- 16-provider capability matrix (`agent-session-grep-ports`) with deferred
  provider rows (deepseek-harness, zcode) and per-provider maturity grading.
- New provider adapters (14 implemented, all with evidence-backed fixtures):
  - `opencode` — parses opencode.db SQLite transcripts (temp-file + read-only
    + busy_timeout).
  - `hermes` — parses OpenHermes `session_<id>.json` transcripts.
  - `antigravity` — parses `.gemini/antigravity-cli/brain` transcript JSONL.
  - `cursor` — parses Cursor `state.vscdb` chat history.
  - `aider`, `cline`, plus the earlier-wave adapters (claude-code, codex,
    gemini, gpt-codex, bolt, cody, continue, windsurf).
- `handoff <query>` CLI subcommand — deterministic handoff-pack/v1 generation
  with evidence/inference separation and budget truncation.
- `resume <session-id>` CLI subcommand — dry-run by default (prints the
  provider command, original working directory, and permission mode);
  `--yes` spawns the provider in that directory. Providers whose resume
  command is unverified report `available:false` rather than a fabricated
  command.
- `search --mode lexical|semantic|hybrid` — retrieval mode selection. Semantic
  and hybrid require a ready semantic index; without one the response is
  explicitly marked `retrieval_mode: lexical_fallback` with a warning, never
  silently downgraded.
- `hook <session-start|user-prompt-submit>` CLI subcommand — Claude Code hook
  integration, disabled by default. Reads the hook payload from stdin and emits
  the `hookSpecificOutput.additionalContext` contract; nothing is injected
  unless `--enable` is passed. Injected text is redacted (ADR-0009).
- `serve --port <n>` CLI subcommand — loopback HTTP server (random bearer
  token, Host loopback check, embedded Web UI, JSON API).
- MCP tools `search_sessions` and `get_session_resume`.
- Cross-boundary output redaction (ADR-0009): Robot JSON/JSONL, MCP, HTTP API,
  Handoff Pack, and Web UI redact standalone and prose-embedded secrets
  (AWS keys, GitHub PATs, OpenAI/Anthropic/xAI keys, Bearer tokens, PEM
  private keys, secret-named JSON fields). Human CLI/TUI output stays
  unredacted (ADR-0004).

### Fixed

- serve query-string routing: `/api/search?q=...` no longer 404s.
- Smoke scripts assert the actual 8 MCP tools (was 7 after the provider wave).
- `verify-release.py` runs with `--db` and a committed gate fixture.
- Redaction now covers secrets embedded inside prose (previously only
  whole-string secrets were matched).

## [0.1.0] — 2026-08-15

### Added

- First open-source-ready release: CLI, Robot JSON protocol, MCP server,
  TUI, and Web UI sharing one Application ADT (consistent search semantics
  across all five entry points).
- Session search over Claude Code and Codex transcripts (lexical FTS5 plus
  ranked retrieval), resume metadata extraction, and handoff packs.
- Installer scripts for Windows (PowerShell) and Unix (sh), release
  rehearsal automation, and an evidence-backed open-source gate manifest.
- Core Beta evidence harness: lexical recall at 10 = 1.00, parse loss = 0.00
  on the committed gate fixture.

[Unreleased]: https://github.com/qin-devs/AgentSessions/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/qin-devs/AgentSessions/releases/tag/v0.1.0
