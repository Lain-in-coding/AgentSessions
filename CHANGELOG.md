# Changelog

All notable changes to agent-session-grep are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

> Planned first public version: `0.1.0`. No tag or release has been published.

### Added

- 16-provider capability matrix (`agent-session-grep-ports`) with deferred
  provider rows (`deepseek-harness`, `zcode`) and per-provider maturity grading.
- Provider adapters for the 14 implemented, Experimental providers:
  `claude-code`, `codex`, `grok-build`, `antigravity`, `opencode`, `pi`,
  `hermes`, `cursor`, `kimi-code`, `openclaw`, `qoder`, `tencent-codebuddy`,
  `cline`, and `aider`. Each adapter has evidence-backed synthetic fixtures;
  DeepSeek Harness and ZCode remain deferred because no transcript evidence is
  available.
- Source installers install both `agent-session-grep` and `asg`: Windows uses
  two executable copies; Unix uses a managed symlink or wrapper. Upgrade and
  uninstall are idempotent and refuse unrelated aliases.
- `handoff <query>` CLI subcommand — deterministic handoff-pack/v1 generation
  with evidence/inference separation and budget truncation.
- `resume <session-id>` CLI subcommand — dry-run by default (prints the
  provider command, original working directory, and permission mode);
  `--yes` spawns the provider in that directory. Providers whose resume
  command is unverified report `available:false` rather than a fabricated
  command.
- `search --mode lexical|semantic|hybrid` — retrieval mode selection. The
  current vector mode uses bigram hashes for fuzzy lexical matching, not a
  semantic model; hybrid combines lexical and vector rankings with RRF.
  Semantic and hybrid metrics remain informational and carry no release
  threshold or quality claim.
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

### Planned for 0.1.0

- First public release: CLI, Robot JSON protocol, MCP server, TUI, and Web UI
  adapters sharing one Application ADT. Full cross-entry release rehearsal is
  still required before publication.
- Session search over the implemented provider set (lexical FTS5 plus ranked
  fuzzy-lexical retrieval), resume metadata extraction, and handoff packs.
- Installer scripts for Windows (PowerShell) and Unix (Bash), release
  rehearsal automation, and an evidence-backed open-source gate manifest.
- Core Beta evidence harness: lexical recall at 10 = 1.00 and parse loss =
  0.00 on the committed synthetic gate fixture.
