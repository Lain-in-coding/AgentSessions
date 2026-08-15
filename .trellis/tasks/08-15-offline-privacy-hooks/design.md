# Offline Privacy Hooks — Design

> Task: `08-15-offline-privacy-hooks` (slice 1)
> Slice scope: ADR-0009 redaction engine + output-boundary wiring + `--offline`
> flag + audit type + hook config documentation (no hook executor yet).
>
> NOTE: This design.md lives in the worktree task-dir copy because the main
> repo task dir (`C:/AgentSessions/.trellis/tasks/08-15-offline-privacy-hooks/`)
> was not writable from the isolated worktree. On integration, copy this file
> (and the check.jsonl entry below) to the main repo.

## Context

Three trust guarantees (PRD): zero-telemetry default offline, cross-boundary
output redaction (ADR-0009), hooks disabled by default with explicit opt-in —
all verifiable. This slice delivers the first two plus the hook doc contract.

ADR-0004 narrowed to local-human output only; ADR-0009 draws the cross-boundary
rule (Robot JSON/JSONL, MCP content + structuredContent, future Web/HTTP/
Handoff) with default redaction and a `redaction` metadata block on every
machine response.

## Decisions

### D1 — Redaction engine lives in the application crate

`crates/agent-session-grep-application/src/redaction.rs` hosts:
- `Redactor` — versioned ruleset (`RULESET_VERSION = 1`), built-in rules.
- `redact_value(&mut serde_json::Value, boundary) -> (Vec<RedactionAudit>, RedactionMeta)`
  — recursive walk over every string leaf (arrays, objects, any depth),
  replacing matches with the stable marker `▮▮redacted▮▮`.
- `RedactionMeta { mode, status, ruleset_version, redacted_count, audit_id? }`
  — the exact ADR-0009 field shape, serialized as `redaction` on machine envelopes.
- `RedactionAudit { rule_id, count, boundary }` — never contains matched text
  or secret content (unit + e2e asserted).
- `Boundary { Robot, Mcp }` — output-boundary identifier for audit/metadata.

Why application: the redactor is a projection shared by every frontend (CLI
robot, MCP, future Web/HTTP). No port dependency; presentation-layer logic.

### D2 — Ruleset: conservative, high-confidence, fail-safe

Hand-written byte-level matchers (no regex dependency added; workspace deps
unchanged). Each rule documented in the module rule catalogue.

| id | pattern | notes |
|----|---------|-------|
| `pem_private_key` | `-----BEGIN ... PRIVATE KEY-----` block | whole block redacted |
| `openai_sk` | `sk-[A-Za-z0-9]{20,}` | |
| `github_pat` | `ghp_[A-Za-z0-9]{36,}` | |
| `slack_token` | `xox[bap]-[A-Za-z0-9-]{10,}` | |
| `aws_akid` | `AKIA[0-9A-Z]{16}` | |
| `bearer_token` | `Bearer [A-Za-z0-9._\-]{20,}` (case-insensitive) | |
| `connection_string` | `://user:pass@` userinfo in URIs | scheme + host survive |
| `generic_password` | `(password|passwd|pwd)[:=]\S+` | |
| `generic_secret` | `(secret|api_key|apikey)[:=]\S+` | |
| `generic_token` | `token[:=]\S+` | |

Fail-safe direction: false positives acceptable, false negatives documented.
Known v1 gaps: prefix-less tokens (random 64-hex), JSON-escaped/unicode/base64
encoded secrets — documented in module doc.

### D3 — Wiring points: Robot and MCP boundaries only

- Robot JSON/JSONL (`protocol.rs`): `redact_success_envelope` /
  `redact_error_envelope` replace the plain envelopes on the production path
  (`emit_result`, `help_envelope`, every error path in `main()`). Both redact
  `data`, `warnings`, and error `message`/`details` recursively and attach the
  `redaction` block. Plain `success_envelope`/`error_envelope` remain for
  protocol unit tests (baseline shape).
- MCP (`mcp.rs`): `handle_tools_call` redacts the tool payload before building
  `content[0].text` and `structuredContent` (same redacted payload — equality
  assertions in existing tests stay valid), attaches `redaction` to
  `structuredContent`. `error_frame` redacts protocol-error messages (they
  echo input such as unknown param names); `business_error_result` redacts
  error message/details.
- Human CLI/TUI: NOT redacted (ADR-0004). No change to `human.rs`/`tui/`.
- Progress frames: ordinals/counts only — unchanged.

### D4 — Audit sink: minimal

`protocol::emit_redaction_audits` writes one JSON line per audit record to
stderr (never protocol stdout), prefix `redaction-audit:`, only when
redactions occurred. Records: rule id, count, boundary — never matched text.
Future unified sink (agent 1 OutputBoundary types) can replace; TODO(integration).

### D5 — `--offline` global flag

- Parsed in the global flag prefix position (`extract_offline_flag`);
  registered in every prefix scanner (`command_name`, `parse_db_flag`,
  `bare_positionals`, `is_known_flag_name`, output-mode/request-id scans).
- Semantics: tool has no network features today, so the flag changes no
  behavior; it asserts offline intent and threads into `dispatch` as a
  fail-closed gate for future network features (model download, external
  Embedding API, telemetry) — under `--offline` they must refuse to connect.
  Documented in help text.
- Flag after the command name is a positional, never a flag.

### D6 — Hooks: documentation-only in this slice

Config contract (no executor built):
- Hook points: Claude Code `SessionStart` / `UserPromptSubmit`.
- Installed but NOT enabled — explicit user opt-in required.
- When enabled: max_tokens budget, provider/time filters, time decay,
  `--offline`, one-command disable.
- Injected content redacted by default; output follows
  `hookSpecificOutput.additional_context` contract.
- Never silently inject history into the current prompt.

### D7 — THREAT-MODEL proposed additions (recorded here)

Main-repo THREAT-MODEL.md edit was not possible from the isolated worktree
(shared-checkout path blocked). Per task instruction, the four proposed
attack surfaces are recorded here for review:

| # | Attack surface | Proposed controls |
|---|----------------|-------------------|
| A1 | Web serve (LAN mode) | default loopback; LAN explicit opt-in + warning; ADR-0009 redaction on all endpoints; auth required |
| A2 | Hook (Claude Code) | default-disabled, explicit opt-in; budget/filter/decay/`--offline`; redacted injection; no silent injection |
| A3 | Semantic model download | explicit confirmation + allowlist; fail closed under `--offline`; source verification |
| A4 | External Embedding API | default-off; explicit config + confirmation; ADR-0009 projection before egress; fail closed under `--offline` |

On integration, append as THREAT-MODEL.md section 8 (Proposed Additions).

## Boundaries

- Provider transcripts read-only; no real personal paths/hostnames/identities
  in code/tests/fixtures/docs. Fixtures use synthetic secrets.
- No secret text in audit records, redaction metadata, or protocol stdout.
- Additive: envelope schema_version 1.0 unchanged; `redaction` additive.
  Human output unchanged. No schema/store/port change.

## Compatibility

- Robot/MCP responses gain additive `redaction` field; older clients ignore
  unknown fields.
- `--offline` additive; unknown flags remain usage errors.

## Rollout / rollback

- Rollback: revert CLI wiring + `--offline` flag; application redaction
  module is dead code until re-wired (harmless).
- Rollout order: application engine + tests → protocol/MCP wiring → e2e
  fixtures → docs.

## Verification (green at slice completion 2026-08-15)

```
$env:CARGO_TARGET_DIR='C:/AgentSessions/target-08-15-privacy-hooks'
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```
