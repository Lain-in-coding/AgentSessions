# Provider Beta Readiness Ledger

> Machine-readable companion to `PROVIDER-MATURITY-MATRIX.md`.
> Authority for maturity/capability values remains
> `crates/agent-session-grep-ports/src/capability.rs`.
> This ledger records **why no provider is Beta yet** and separates
> repository-local gaps from external/owner gates. Do not promote from this
> file alone.

Last updated: 2026-08-27 (richer tool-call extraction wave: `claude-code` /
`codex` tool_activity extraction now matches the record shapes real
transcripts write — codex registers `function_call` and pairs both
`*_call_output` types by `call_id`, reads the string `input` of
`custom_tool_call`, and resolves `apply_patch` envelopes to the patched path;
the kind closed set gained the tool names a real-corpus census found; Claude
`tool_use` blocks now render as `Name(target)` summaries in the canonical body,
so tool-use-only assistant messages are no longer indexed with empty text.
Both stay `partial` — the residual limits are recorded per row, not fixed.
Previous wave, 2026-08-25: resume matrix `antigravity` / `opencode` /
`kimi-code` / `tencent-codebuddy` resume column → `derived`, evidence per row;
tool_activity honesty wave: seven formats reviewed — four carry structured tool
records but no per-message native id to anchor, three carry none — all seven
stay `Unsupported` with the reason recorded per row and pinned by golden-corpus
drift tests.)

## Global external blockers (apply to every promotion)

| Blocker | Owner | Notes |
|---|---|---|
| Named successful cross-target CI run | GitHub billing + CI | `last_certified_targets` stays empty until a green Windows/Linux/macOS run id is recorded |
| ADR-0010 accepted_at | owner/approver | Rollback policy is Proposed only |
| Independent owner promotion decision | owner | RFC-0002 §6 forbids code-existence promotion |

## Per-provider local readiness (implemented 14)

Legend for local columns: `ok` = present with tests; `partial` = present with known holes; `missing` = not implemented / unsupported in capability matrix. `property` = seeded randomized property suite (`tests/properties.rs`, mirroring claude-code/codex).

| provider_id | golden | read-only | property | discover | source_span | tool_activity | resume | incremental | local Beta blockers (beyond global) |
|---|---|---|---|---|---|---|---|---|---|
| claude-code | ok | ok | ok | native | ok (native) | partial | derived | derived | tool_activity stays `partial` by format, not by missing work: kind comes from a closed set of documented tool names, so user-defined and MCP (`mcp__*`) tools are recorded with kind `unknown` (guessing a kind from an arbitrary name would be fabrication), and a call whose `tool_result` never arrives reports status `unknown`. No other local gap; owner promotion still required |
| codex | ok | ok | ok | native | ok (native) | partial | derived | derived | tool_activity stays `partial` by format: rollout tool outputs carry no failure marker at all (no `is_error`, no `metadata.exit_code` in 12805 observed outputs), so status is success-or-unknown and `error` is unreachable without inventing it; `web_search_call` / `tool_search_call` records carry neither a tool name nor a `call_id`, so they are not extracted; rollout has no sidechain concept, so actor is always `main`. No other local gap; owner promotion still required |
| grok-build | ok | ok | ok | native | ok (native) | missing | derived | derived | format carries structured tool records (`_meta.bashCommand` meta chunks) but no per-message native id — tool_activity cannot anchor, honestly Unsupported; synthetic msg ids |
| antigravity | ok | ok | ok | native | ok (native) | missing | derived | derived | no in-file session id; format carries `tool_calls` on step records but `step_index` is not a durable cross-document id — tool_activity cannot anchor, honestly Unsupported |
| opencode | ok | ok | ok | native | missing | missing | derived | derived | no span (SQLite source has no in-file byte offsets) |
| pi | ok | ok | ok | native | ok (native) | missing | derived | derived | format carries no structured tool-call records — tool_activity honestly Unsupported |
| hermes | ok | ok | ok | native | missing | missing | unknown | derived | JSON doc; no span; SQLite `state.db` surface not parsed; resume evidence conflicting across reference projects (agf `hermes --resume <id>` vs hstry `hermes --session <id>` vs cc-switch/AgentRecall no CLI) — stays unknown |
| cursor | ok | ok | ok | missing | missing | missing | unknown | derived | no discovery root: the adapter parses the VS Code `workspaceStorage/*/state.vscdb` ItemTable surface, whose per-workspace hash directories sit under a platform-specific application-data path, not a home-relative root this table can express; `~/.cursor/chats/<id>/store.db` is the separate Cursor CLI `meta`/`blobs` schema, which this adapter's probe rejects. No span; multi-gen format layering pending; Cursor CLI resume (`agent --resume`) is a different surface than this adapter — stays unknown |
| kimi-code | ok | ok | ok | native | ok (native) | missing | derived | derived | loop events (step/tool, incl. tool.call/tool.result) not parsed and no per-message native id — tool_activity cannot anchor, honestly Unsupported |
| openclaw | ok | ok | ok | native | ok (native) | missing | unsupported | derived | resume intentionally unsupported; format carries no structured tool-call records — tool_activity honestly Unsupported |
| qoder | ok | ok | ok | native | ok (native) | missing | unknown | derived | discovery covers the transcript-JSONL surface only (the Electron SQLite store is a separate, unimplemented surface); format carries `tool_use`/`tool_result` records but no per-message native id — tool_activity cannot anchor, honestly Unsupported; no authoritative resume command in reference projects (AgentRecall: resume false) |
| tencent-codebuddy | ok | ok | ok | native | ok (native) | missing | derived | derived | extension variant pending; documented format knowledge carries no structured tool-call records — tool_activity honestly Unsupported |
| cline | ok | ok | ok | native | missing | missing | unsupported | derived | no session id / span; discovery covers the `~/.cline/data/tasks` tree only (the VS Code extension `globalStorage` tree is not home-relative and is not registered) |
| aider | ok | ok | ok | missing | derived | missing | unsupported | derived | approximate spans; no resume; no discovery root by construction — `.aider.chat.history.md` lives at the root of each user repository, so upstream agentsview discovers it by walking working trees rather than one canonical home directory; no tool_activity (blockquote tool output is folded into assistant text, no structured call/result records) |

## Deferred providers (not Beta candidates)

| provider_id | status | blocker |
|---|---|---|
| deepseek-harness | Unsupported | no transcript evidence |
| zcode | Unsupported | no transcript evidence |

## Honest promotion rule

A provider may be advertised as **Beta** only when:

1. every local Beta column above is `ok` (or an explicit, owner-approved exception is recorded);
2. a named cross-target CI success is written into `AdapterManifest.last_certified_targets`;
3. ADR-0010 is Accepted; and
4. the owner records the promotion decision with evidence paths.

Until then the public matrix stays **Experimental** for all 14 implemented providers.
