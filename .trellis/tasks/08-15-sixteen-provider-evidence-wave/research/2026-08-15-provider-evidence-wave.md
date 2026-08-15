# Research: 16-Provider Evidence Wave (Local Machine Sweep)

- **Query**: 16-provider evidence wave — per-provider local data root existence, format, identity fields, resume evidence, competitor borrowing
- **Scope**: internal (local filesystem) + external (deep-read competitor notes)
- **Date**: 2026-08-15
- **Machine**: `<user-home>` = `C:\Users\<user>` (paths redacted in notes; real paths used during search only)

> Privacy: No real transcript content is copied. Only structural first-line / file-listing shapes are recorded. Synthetic ids are used in examples. Real absolute user paths/hostnames are not reproduced.

## Summary Table

| # | Provider | Local root on this machine | Format (observed) | Identity field (structural) | Resume evidence | Maturity candidate | Competitor w/ adapter | License of competitor |
|---|---|---|---|---|---|---|---|---|
| 1 | Claude Code | present (`~/.claude/projects/*/`) | JSONL, `type:user/assistant`, `sessionId`/`uuid`/`parentUuid` | `sessionId` (first non-empty) | `claude --resume <id>` (known) | Certified candidate | hstry / cass / AgentRecall / ctx | MIT / restricted / MIT / OSS |
| 2 | Codex CLI | present (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`) | rollout envelope JSONL, `session_meta` + `response_item/message` | `session_meta.payload.session_id` AND `payload.id` (both present — confirms PRD conflict) | `codex resume <id>` (known) | Certified candidate | hstry / cass / AgentRecall | MIT / restricted / MIT |
| 3 | DeepSeek Harness | **absent** (`~/.deepseek` not found) | unknown | unknown | unknown | Experimental / deferred | none in deep-read set | — |
| 4 | Grok Build | **absent** (`~/.grok` not found; no `GROK_HOME`) | unknown (no local sample) | unknown (no local sample) | `grok --resume <id>` (PRD-claimed, not locally verifiable) | Beta (deferred locally) | cass (Grok connector) | restricted (clean-room only) |
| 5 | Antigravity | present (`~/.gemini/antigravity-cli/brain/<uuid>/.system_generated/logs/`) | JSONL `transcript.jsonl` + `transcript_full.jsonl` + `messages/*.json` + `steps/N/content.md` | brain dir `<uuid>` = session id; line has `step_index`/`source`/`type`/`created_at` | `agy --conversation <id>` vs `--resume` (PRD conflict, unverified) | Beta | cass (Antigravity connector) | restricted |
| 6 | OpenCode | present (`~/.local/share/opencode/`) | SQLite `opencode.db` (+ `-wal`/`-shm`) + legacy `storage/session_diff/ses_*.json` | session table `id` (`ses_...`) | `opencode <dir> --session <id>` vs `-s <id>` (version conflict) | Beta | hstry / cass / AgentRecall | MIT / restricted / MIT |
| 7 | Pi | present (`~/.pi/agent/sessions/<encoded-cwd>/`) | session JSONL; first line `{type:"session",version:3,id,timestamp,cwd}` | line `id` (UUID) | `pi --session <id>` (known) | Beta | hstry / cass | MIT / restricted |
| 8 | Aider | **absent** (no `.aider*` files in home) | unknown (no local sample) | unknown | none | Experimental / deferred | hstry (aider adapter) / cass (Aider connector) | MIT / restricted |
| 9 | Cline | **absent** (`~/.cline` not found; no VS Code globalStorage `saoudrizwan.claude-dev`) | unknown (no local sample) | unknown | none (VS Code internal) | Experimental / deferred | cass (Cline connector) | restricted |
| 10 | Cursor | **absent** (`~/.cursor` not found; no `AppData/Roaming/Cursor`) | unknown (no local sample) | unknown | `cursor agent --resume <id>` (PRD-claimed) | Experimental / deferred | hstry / cass / AgentRecall | MIT / restricted / MIT |
| 11 | OpenClaw | config-only (`~/.openclaw/openclaw.json` + `.back`); no `agents/` tree | unknown (no transcript sample) | unknown | intentionally unsupported (gateway) | Beta (format deferred) | cass (OpenClaw connector) | restricted |
| 12 | Hermes | config-only (`~/.hermes/config.yaml`); no `state.db` | unknown (no DB sample) | unknown | `hermes --resume <id>` (PRD conflict) | Beta (SQLite deferred) | hstry / cass | MIT / restricted |
| 13 | Kimi Code | **absent** (`~/.kimi-code` not found) | unknown (no local sample) | unknown | `kimi --resume <id>` vs `--session <id>` (PRD conflict) | Beta (deferred locally) | cass (Kimi connector) | restricted |
| 14 | ZCode | **absent** (`~/.zcode` not found) | unknown | unknown | unknown | Experimental / deferred | none in deep-read set | — |
| 15 | Qoder | present (`~/.qoder/`) but **no transcript**; only `shared_client/` index/bin | unknown (no transcript sample) | unknown | unknown | Beta (deferred locally) | AgentRecall (qoder source) | MIT |
| 16 | Tencent CodeBuddy | present (`~/.codebuddy/`) but **no CLI transcript**; only skills/plugins | unknown (no transcript sample) | unknown | unknown | Beta (deferred locally) | AgentRecall (codebuddy source) / cass | MIT / restricted |

## Per-Provider Detail

### 1. Claude Code — present (Certified candidate)

- **Local root**: `~/.claude/projects/<encoded-project-dir>/<sessionId>.jsonl` (185+ files observed across multiple project dirs). Also `subagents/agent-*.jsonl` + `.meta.json` under session dirs.
- **Format**: JSONL, one JSON object per line. Adapter already implemented (`crates/agent-session-grep-provider-claude/src/lib.rs`), variant `claude-code/jsonl-v1`.
- **Identity fields (structural)**: top-level `sessionId`, per-line `uuid`, `parentUuid` (threading), `timestamp`, `isSidechain`. Adapter takes first non-empty `sessionId`.
- **Resume**: `claude --resume <id>` — well-documented, already implemented in-repo.
- **Competitor borrowing**: hstry (`claude-code` adapter, MIT), cass (`Claude` connector — restricted, clean-room ideas only), AgentRecall (`claude`/`claude-internal` sources, MIT), ctx (`providers/claude.rs`).
- **Next step**: Gate D / cross-platform target CI for Certified.

### 2. Codex CLI — present (Certified candidate)

- **Local root**: `~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl` (143 files observed). No `sessions/archived/` subtree present on this machine.
- **Format**: rollout envelope JSONL. First line structurally: `{"timestamp":..., "type":"session_meta", "payload":{"session_id":"<uuid>", "id":"<uuid>", "timestamp":..., "cwd":..., "originator":"codex-tui", "cli_version":"...", ...}}`.
- **Identity fields (structural)**: `payload.session_id` AND `payload.id` are **both present and identical** on the sampled file — this confirms the PRD's provenance conflict note. In-repo contract uses `session_meta.payload.session_id`; external reports of `payload.id` are the same value but must be layered by source/version. Also `cwd`, `cli_version`, `originator`, `model_provider`.
- **Resume**: `codex resume <id>` — already implemented in-repo.
- **Competitor borrowing**: hstry (`codex` adapter, MIT), cass (`codex.rs`, 666 lines, restricted), AgentRecall (`codex` source, MIT).
- **Next step**: Lock field provenance by version; Gate D.

### 3. DeepSeek Harness — absent (deferred)

- **Local root**: `~/.deepseek` does **not** exist. No `DEEPSEEK_HOME` env evidence.
- **Format / identity / resume**: unknown — no local transcript evidence.
- **PRD confirmation**: Confirms PRD claim "证据不足". No transcript evidence on this machine.
- **Competitor borrowing**: No deep-read competitor has a DeepSeek Harness adapter.
- **Next step**: Owner must either supply a real transcript fixture or record `deferred` by release cutoff. Do NOT fabricate.

### 4. Grok Build — absent (deferred locally)

- **Local root**: `~/.grok` does **not** exist. No `GROK_HOME` env evidence.
- **Format / identity / resume**: not locally verifiable. PRD documents `summary.json`+ACP `updates.jsonl` as main variant, `chat_history.jsonl` as fallback variant; identity `summary.json.info.id`; resume `grok --resume <id>`.
- **Competitor borrowing**: cass has a Grok connector (restricted — clean-room ideas only, never copy code/fixtures).
- **Next step**: Probe+parse deferred until a real `~/.grok` sample or owner-supplied fixture exists. Record variant layering when evidence arrives.

### 5. Antigravity — present (Beta)

- **Local root**: `~/.gemini/antigravity-cli/brain/<session-uuid>/.system_generated/logs/transcript.jsonl` + `transcript_full.jsonl`. Many brain dirs observed. Also `messages/<uuid>.json`, `steps/N/content.md`, `tasks/task-N.log` per brain.
- **Format**: JSONL. First line structurally: `{"step_index":N, "source":"MODEL"|"USER"|..., "type":"GENERIC"|"PLANNER_RESPONSE"|..., "status":"DONE", "created_at":"ISO-8601Z", "content":"...", "tool_calls":[...]}`. Content can embed permission-grant text (do not copy verbatim).
- **Identity fields (structural)**: brain directory `<uuid>` is the session id. Per-line has `step_index`, `created_at`. No explicit `cwd` field seen in the first line (cwd appears inside content text, needs deeper probe). PRD marks identity as "unknown (needs fixture)".
- **Resume**: PRD conflict — source/tests show `agy --conversation <id>`; a report summary says `--resume`. Unverified locally. Keep null/— until authoritative source checked.
- **Competitor borrowing**: cass has an Antigravity connector (restricted).
- **Next step**: Probe+parse can proceed (format is clear); resume stays null until version-verified.

### 6. OpenCode — present (Beta)

- **Local root**: `~/.local/share/opencode/`. Contains `opencode.db` (+ `-wal`, `-shm`) — SQLite. Also legacy `storage/session_diff/ses_<id>.json` (9 files observed) and `log/*.log`.
- **Format**: SQLite primary (`opencode.db`, session/message/part tables) + legacy JSON tree double path. Adapter must open read-only (`SQLITE_OPEN_READONLY` + `busy_timeout`).
- **Identity fields (structural)**: session table `id` with `ses_...` prefix (visible in `storage/session_diff/ses_*.json` filenames).
- **Resume**: PRD conflict — `opencode <dir> --session <id>` vs `opencode -s <id>` (version difference). Keep null/— until verified.
- **Competitor borrowing**: hstry (`opencode` adapter, MIT), cass (`OpenCode` connector, restricted), AgentRecall (`opencode-cli` source, MIT — notes Hermes/OpenCode/CodeWiz use SQLite session table id).
- **Next step**: Discovery/parse can proceed (read-only SQLite + legacy JSON); resume null until version-locked.

### 7. Pi — present (Beta)

- **Local root**: `~/.pi/agent/sessions/<encoded-cwd>/<timestamp>_<sessionId>.jsonl`. Multiple files observed under `--C--Users--<user>--/`.
- **Format**: session JSONL. First line structurally: `{"type":"session", "version":3, "id":"<uuid>", "timestamp":"ISO-8601Z", "cwd":"<absolute-path>"}`.
- **Identity fields (structural)**: line `id` (UUID); also `cwd`, `timestamp`, `version`.
- **Resume**: `pi --session <id>` — PRD-claimed; path-override envs `PI_CODING_AGENT_SESSION_DIR` / `PI_CODING_AGENT_DIR`.
- **Competitor borrowing**: hstry (`pi` adapter, MIT), cass (`Pi Agent` connector, restricted).
- **Next step**: Probe+parse; supply synthetic fixture.

### 8. Aider — absent (deferred)

- **Local root**: No `.aider*` files found in home. `.aider.chat.history.md` candidate not present.
- **Format / identity / resume**: unknown; resume = none.
- **Competitor borrowing**: hstry has an `aider` adapter (MIT); cass has an Aider connector (restricted).
- **Next step**: Need real fixture + root evidence before implementation.

### 9. Cline — absent (deferred)

- **Local root**: `~/.cline` does not exist. VS Code globalStorage `saoudrizwan.claude-dev` not found under `AppData/Roaming/Code/User/globalStorage/`.
- **Format / identity / resume**: unknown; resume = none (VS Code internal).
- **Competitor borrowing**: cass has a Cline connector (restricted).
- **Next step**: Research-then-implement; need real fixture.

### 10. Cursor — absent (deferred)

- **Local root**: `~/.cursor` does not exist. `AppData/Roaming/Cursor` does not exist.
- **Format / identity / resume**: not locally verifiable. PRD documents multi-generation formats: CLI `~/.cursor/projects/*/agent-transcripts/` JSONL; `~/.cursor/chats/<id>/store.db` SQLite KV; `state.vscdb`. Resume `cursor agent --resume <id>`.
- **Competitor borrowing**: hstry (`cursor` adapter, MIT), cass (`Cursor` connector, restricted), AgentRecall (`cursor-agent` source, MIT — notes path `~/.cursor/projects/{workspaceSlug}/agent-transcripts/{sessionId}/{sessionId}.jsonl`, identity = filename, `cursor:{workspaceSlug}:{rawId}` sessionKey).
- **Next step**: Need local or owner-supplied fixture; multi-generation format layering required.

### 11. OpenClaw — config-only (Beta, format deferred)

- **Local root**: `~/.openclaw/` contains only `openclaw.json` and `openclaw - 副本.back`. No `agents/<agent>/sessions/*.jsonl` tree, no legacy `~/.clawdbot`, no `~/.moltbot`.
- **Format / identity / resume**: no transcript sample locally. PRD documents v3 JSONL header `{type:session,id,timestamp,cwd}` + `{type:message,...}`; identity = header `id`; resume intentionally unsupported (gateway-managed).
- **Competitor borrowing**: cass has an OpenClaw connector (restricted). AgentRecall has `openclaw` source (MIT).
- **Next step**: Discovery/parse can proceed once a real `agents/` tree is available; resume = unsupported by design.

### 12. Hermes — config-only (Beta, SQLite deferred)

- **Local root**: `~/.hermes/` contains only `config.yaml`. No `state.db`.
- **Format / identity / resume**: no DB sample locally. PRD documents SQLite + JSONL dual storage, `HERMES_HOME` override, identity = sessions table `id`. Resume conflict: cc-switch/fast-resume = None; agf = `hermes --resume <id>`.
- **Competitor borrowing**: hstry (`hermes` adapter, MIT), cass (`Hermes` connector, restricted).
- **Next step**: SQLite path can proceed once a real `state.db` is available; resume null until verified.

### 13. Kimi Code — absent (deferred locally)

- **Local root**: `~/.kimi-code` does not exist. No `KIMI_CODE_HOME`.
- **Format / identity / resume**: not locally verifiable. PRD documents `session_index.jsonl` + `sessions/*/*/agents/*/wire.jsonl` + `state.json`; identity candidate = state.json's session dir name; resume conflict `kimi --resume <id>` vs `kimi [--yolo] --session <id>`.
- **Competitor borrowing**: cass has a Kimi connector (restricted).
- **Next step**: Need local or owner-supplied fixture; resume null until version-verified.

### 14. ZCode — absent (deferred)

- **Local root**: `~/.zcode` does not exist.
- **Format / identity / resume**: unknown.
- **PRD confirmation**: Confirms PRD claim "证据不足" — same status as DeepSeek Harness.
- **Competitor borrowing**: No deep-read competitor has a ZCode adapter.
- **Next step**: Owner supplies real transcript or records `deferred` by cutoff. Do NOT fabricate.

### 15. Qoder — present but no transcript (Beta, deferred locally)

- **Local root**: `~/.qoder/` exists but contains only `argv.json`, `shared_client/` (binaries, completion/graph indexes, memory_graph). No `projects/<project>/transcript/*.jsonl` tree found.
- **Format / identity / resume**: no transcript sample locally. PRD documents JSONL (`session_meta/user/assistant/progress/tool_use/tool_result`); AgentRecall has a `qoder` source noting sessionKey `qoder:{rawId}`.
- **Competitor borrowing**: AgentRecall (`qoder` source, MIT). Also AgentRecall cache variant note.
- **Next step**: Need a real transcript path sample; official path can be implemented once located.

### 16. Tencent CodeBuddy — present but no CLI transcript (Beta, deferred locally)

- **Local root**: `~/.codebuddy/` exists but contains only `skills/`, `plugins/`, `commands/`, `expert-history.json`, `memery/`. No `projects/**/*.jsonl` CLI transcript tree found.
- **Format / identity / resume**: no transcript sample locally. PRD documents two variants: CLI JSONL (OpenAI-style, `~/.codebuddy/projects/**/*.jsonl`) + extension JSON (`history/<md5>/<session>/index.json` + `messages/*.json`). AgentRecall notes CodeBuddy CLI: each line `CodeBuddyConversationLine`, parse only `type==="message"` role user/assistant, filter root user message with content `"code"` (startup keyword), identity = line `sessionId`.
- **Competitor borrowing**: AgentRecall (`codebuddy-cli` source, MIT), cass (restricted).
- **Next step**: Variant-split CLI vs extension once a real transcript sample is available.

## Competitor Borrowing Matrix (cass legal boundary)

> cass (`coding_agent_session_search`) LICENSE carries a restricted-party rider: **clean-room ideas only, never copy code or fixtures**. Its provider knowledge lives in `franken-agent-detection` (features `cursor`/`chatgpt`/`opencode`/`crush`/`hermes`). Use cass only as idea-level format reference; do not reproduce code or fixture data.

| Provider | hstry (MIT) | AgentRecall (MIT) | cass (restricted) | ctx (OSS) |
|---|---|---|---|---|
| Claude Code | yes | yes | yes | yes |
| Codex | yes | yes | yes (666-line `codex.rs`) | yes |
| Antigravity | — | — | yes | — |
| OpenCode | yes | yes | yes | — |
| Pi | yes | — | yes | — |
| Aider | yes | — | yes | — |
| Cline | — | — | yes | — |
| Cursor | yes | yes | yes | — |
| OpenClaw | — | yes | yes | — |
| Hermes | yes | — | yes | — |
| Kimi Code | — | — | yes | — |
| Grok Build | — | — | yes | — |
| Qoder | — | yes | — | — |
| CodeBuddy | — | yes | yes | — |
| DeepSeek Harness | — | — | — | — |
| ZCode | — | — | — | — |

## Realistic First-Wave Beta Candidates

Providers with **local transcript evidence on this machine** and a clear format/identity shape — ready for probe+parse:

1. **Antigravity** — JSONL format clear, many brain dirs present.
2. **OpenCode** — SQLite + legacy JSON, db present, read-only safe.
3. **Pi** — JSONL with explicit session header, files present.
4. **Claude Code** — already Experimental; path to Certified via Gate D.
5. **Codex** — already Experimental; path to Certified via Gate D.

Deferred (no local transcript, but PRD/competitor evidence exists): Grok Build, Cursor, Kimi Code, OpenClaw, Hermes, Qoder, CodeBuddy — need owner-supplied fixture or a machine where the CLI has run.

Deferred (no evidence anywhere): DeepSeek Harness, ZCode — owner must supply real transcript or record `deferred`.

## Caveats / Not Found

- No `GROK_HOME`, `KIMI_CODE_HOME`, `DEEPSEEK_HOME`, `OPENCLAW_STATE_DIR`, `HERMES_HOME` env vars were inspected (would require a shell call; not in scope of read-only listing). If set, they would relocate the respective roots.
- OpenCode `opencode.db` schema was not opened with a SQLite query (constraint: read-only, and a structural listing sufficed). Table/column names are taken from PRD + AgentRecall notes, not directly verified against this machine's db.
- Codex `sessions/archived/` subtree was checked and is absent on this machine; archived layout unverified.
- Aider `.aider.chat.history.md` may exist inside specific project dirs rather than home; only home was checked.
- Resume command evidence is knowledge-based or PRD-claimed for all providers except Claude Code and Codex (which are in-repo implemented). Antigravity / OpenCode / Hermes / Kimi Code resume flags remain unverified conflicts.
