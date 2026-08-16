# Research: Grok Build provider adapter reference (fast-resume + Recall)

- **Query**: Deep-read fast-resume Grok adapter implementation as direct reference for our GrokBuildAdapter
- **Scope**: external (competitor source code, MIT-licensed)
- **Date**: 2026-08-16

## Competitor sources read

| Source | Path | License | Borrowing boundary |
|---|---|---|---|
| fast-resume | `src/adapters/grok.rs` | MIT (Copyright (c) 2025 Stanislas Lange) | MIT permits copy/modity/merge with license+notice retention. Code may be adapted. |
| fast-resume config | `src/config.rs` | MIT | Same as above. |
| fast-resume docs | `docs/how-it-works.md` | MIT | Same. |
| fast-resume tests | `tests/cli.rs` | MIT | Same. |
| Recall | `src/adapters/grok.rs` | (Recall license not checked in this pass; treat as reference-only unless verified) | Reference-only unless license verified. |
| Our AgentSessions ports | `crates/agent-session-grep-ports/src/lib.rs` | (this repo) | Our own contract — the target trait. |
| Our Claude adapter | `crates/agent-session-grep-provider-claude/src/lib.rs` | (this repo) | Our own reference implementation pattern. |

**License conclusion**: fast-resume is MIT-licensed. We MAY adapt its code (copy + modify) provided we retain the MIT copyright notice and permission text in our adaptation. Recall's license was not checked this pass — treat Recall as idea-level reference only until its LICENSE is verified.

---

## Findings

### 1. Source root path resolution

**fast-resume** (`config.rs:172-176`):

```rust
pub fn grok_sessions_dir() -> PathBuf {
    env_path("GROK_HOME")
        .unwrap_or_else(|| home_dir().join(".grok"))
        .join("sessions")
}
```

- `GROK_HOME` env var overrides the base directory (the `.grok` part).
- Default: `~/.grok/sessions/`.
- `env_path` (config.rs:212-217) trims whitespace, skips empty values, and applies `expand_tilde` (handles `~` and `~/` and `~\` on Windows).
- The `.join("sessions")` is always appended after the base, so `GROK_HOME=/custom` resolves to `/custom/sessions/`.

**Recall** (`Recall/src/adapters/grok.rs:106-108`):

```rust
fn resolve_grok_sessions_dir() -> anyhow::Result<Option<PathBuf>> {
    resolve_home_dir(".grok/sessions", "~/.grok/sessions not found, skipping Grok")
}
```

Recall does NOT respect `GROK_HOME` — it hardcodes `~/.grok/sessions`. This is a gap; fast-resume's approach is more correct.

**Directory layout** (confirmed by both + tests):

```
$GROK_HOME/sessions/<percent-encoded-workspace>/<session-uuid>/
    summary.json
    updates.jsonl
```

- The workspace directory name is the percent-encoded absolute path of the working directory (e.g., `%2Fwork%2Fgrok` decodes to `/work/grok`).
- The session directory name is a UUID v7 (e.g., `019edf9c-0000-7000-8000-000000000001`).
- fast-resume also notes `session_search.sqlite` may appear in the sessions root — Recall explicitly skips it (`if workspace_name == "session_search.sqlite" { continue; }`).

### 2. File format: summary.json

**fast-resume** (`grok.rs:96-109`) reads `summary.json` as a `serde_json::Value` and extracts via JSON pointer:

| JSON path | Field | Usage |
|---|---|---|
| `/info/id` | Session identity (UUID string) | `session.id` — fallback to directory name if empty |
| `/info/cwd` | Working directory | `session.directory` — fallback to percent-decoded workspace dir name |
| `generated_title` (top-level string) | Session title | Preferred title source |
| `session_summary` (top-level string) | Session title | Fallback if `generated_title` empty |
| `created_at` (top-level string) | Timestamp | Fallback if no activity timestamp from updates |
| `updated_at` (top-level string) | Timestamp | Second fallback |
| `last_active_at` (top-level string) | Timestamp | Third fallback |

**Recall** (`grok.rs:303-334`) additionally extracts:

| JSON path | Field | Usage |
|---|---|---|
| `current_model_id` (top-level string) | Model ID | Fallback model for usage events |

**summary.json structure** (synthesized from test fixtures):

```json
{
  "info": {
    "id": "019edf9c-0000-7000-8000-000000000001",
    "cwd": "/work/grok"
  },
  "created_at": "2026-07-17T10:00:00Z",
  "updated_at": "2026-07-17T10:30:00Z",
  "generated_title": "Grok adapter work",
  "session_kind": "subagent" | "subagent_resume" | (absent for normal),
  "current_model_id": "grok-composer-2.5-fast"
}
```

**`session_kind` field** (Recall only): `summary.json` may carry `"session_kind": "subagent"` or `"subagent_resume"`. Recall skips these sessions entirely (they are subagent sidechains, not primary sessions). fast-resume does NOT filter by session_kind — it processes all sessions. For our adapter, subagent filtering is relevant to the sidechain facet work in `08-15-structured-activity-context-facets`.

### 3. File format: updates.jsonl (ACP session/update stream)

Each line is a JSON object. The ACP (Agent Communication Protocol) envelope:

```json
{
  "timestamp": "2026-07-17T10:00:01Z" | 1721239201 | 1721239201000,
  "method": "session/update" | "_x.ai/session/update" | (absent),
  "params": {
    "sessionId": "019edf9c-...",
    "update": {
      "sessionUpdate": "user_message_chunk" | "agent_message_chunk" | "rewind_marker" | "tool_call" | "tool_call_update" | "agent_thought_chunk" | "available_commands_update",
      "content": { "type": "text", "text": "..." } | "plain string" | [{...}],
      "_meta": {
        "promptIndex": 0,
        "promptId": "p1",
        "bashCommand": "...",
        "modelId": "grok-composer-2.5-fast",
        "totalTokens": 250,
        "turnStartMs": 1000
      }
    },
    "_meta": {
      "promptId": "p1",
      "modelId": "grok-composer-2.5-fast",
      "totalTokens": 250,
      "turnStartMs": 1000
    }
  }
}
```

**Key `sessionUpdate` types** and how fast-resume handles them:

| `sessionUpdate` value | fast-resume handling | Recall handling |
|---|---|---|
| `user_message_chunk` | Accumulate text by `promptIndex`; skip if `_meta.bashCommand` present (tool echo, not real user input) | Push as `Role::User` message |
| `agent_message_chunk` | Accumulate text by `promptId`; skip empty/whitespace | Accumulate by `AgentChunkKey` (promptId + turnStartMs) |
| `rewind_marker` | Truncate messages at `target_prompt_index` / `targetPromptIndex` | Not handled |
| `tool_call` | Not handled (falls to `_ => {}`) | Format as `[title] rawInput` assistant message |
| `tool_call_update` | Not handled | If status=="completed", format tool result as `[title] -> content` (truncated 500 chars) |
| `agent_thought_chunk` | Not handled | Ignored (but token tracking still runs) |
| `available_commands_update` | Not handled | Ignored |

**Timestamp format** (`grok.rs:346-359`): `timestamp` field can be:
- ISO-8601 string (parsed via `parse_datetime`)
- Integer in seconds (if ≤ 100_000_000_000)
- Integer in milliseconds (if > 100_000_000_000)

### 4. Session identity extraction

**fast-resume** (`grok.rs:98-103`):

```rust
let id = summary
    .pointer("/info/id")
    .and_then(Value::as_str)
    .filter(|id| !id.is_empty())
    .unwrap_or(&fallback_id)  // fallback: session directory name (UUID)
    .to_string();
```

- Primary: `summary.json` → `info.id` (non-empty string).
- Fallback: the session directory's file name (which is a UUID).
- Both are the same value in practice — `info.id` matches the directory name.

**Recall** (`grok.rs:308-314`): same logic — `info.id` with fallback to directory name. Additionally validates the directory name is a valid UUID (`is_grok_session_id` uses `uuid::Uuid::try_parse`).

### 5. Probe / identification logic

fast-resume does NOT have a separate probe phase — it discovers sessions by directory structure (`scan_session_files`):

1. Iterate `sessions_dir` for workspace subdirectories.
2. For each workspace, iterate for session subdirectories.
3. A session directory is valid if it contains `updates.jsonl` (is a file).
4. The session ID is the directory name.
5. mtime = max(updates.jsonl mtime, summary.json mtime).

For our `ProviderAdapter` trait, the **probe** must identify whether a byte stream is a Grok `updates.jsonl`. Evidence signals:
- Lines are JSON objects with `params.update.sessionUpdate` field.
- `sessionUpdate` values match the known enum (`user_message_chunk`, `agent_message_chunk`, `rewind_marker`, etc.).
- `timestamp` field present (string or number).
- `method` field may be `session/update` or `_x.ai/session/update`.

Note: our adapter receives the **updates.jsonl bytes** (not summary.json — that is a separate file). The summary.json is metadata that lives alongside the updates file. Our `SourceDiscovery` port discovers files; `probe` receives the bytes of one discovered file. We need to decide: does our discovery unit = the session directory (multi-file) or the individual updates.jsonl file?

Looking at our Claude adapter: it receives a single JSONL file's bytes. For Grok, the challenge is that identity + cwd come from `summary.json`, not from `updates.jsonl`. This is a structural difference from Claude/Codex where identity is in-band.

**Resolution options**:
- **Option A**: Discovery discovers the `updates.jsonl` file; the adapter's `parse` reads only the updates bytes and cannot access summary.json. Identity would need to come from the directory name (path-derived) or from in-band `params.sessionId`. This loses `cwd` and `generated_title`.
- **Option B**: Discovery discovers the session directory; `read_verified` returns the concatenated bytes of summary.json + updates.jsonl (or a container). This requires a custom discovery that reads multiple files.
- **Option C**: The adapter is constructed with the session directory path, and `parse` reads both files internally. This breaks the "stateless adapter" contract.

**Recommendation**: Option B with a multi-file snapshot, OR extend our `SourceSnapshot` to carry a directory path and let the adapter read sibling files. This needs architecture discussion. The fast-resume adapter sidesteps this because it owns the full scan+parse pipeline (no port separation).

### 6. Parse logic: extracting user/assistant messages

**fast-resume** (`grok.rs:93-251`) — the most complete reference:

1. Read `summary.json` for identity (`info.id`), cwd (`info.cwd`), title, timestamp.
2. Open `updates.jsonl`, iterate lines with `BufReader`.
3. For each line, parse as `serde_json::Value`.
4. Extract `timestamp` for last-activity tracking.
5. Navigate to `params.update` and match on `sessionUpdate`:
   - **`user_message_chunk`**: Extract content text. Skip if `_meta.bashCommand` present. Group by `promptIndex` (accumulate chunks for same promptIndex). Complex logic: if no `promptIndex` seen yet, treat as user; once seen, only chunks WITH promptIndex count as user.
   - **`agent_message_chunk`**: Extract content text. Skip if empty/whitespace. Group by `promptId` (accumulate chunks for same promptId).
   - **`rewind_marker`**: Truncate messages vector at `target_prompt_index` / `targetPromptIndex`, also truncate the user_message_indices vector. Reset pending user and agent.
   - **default**: Reset pending user (breaks chunk accumulation).
6. After iterating, find first non-empty user message for title fallback.
7. Count user turns for `message_count`.
8. Build content string: user messages prefixed with `» `, assistant with `  ` (two spaces).
9. Set timestamp = last activity from updates, or summary fallback fields, or file mtime.

**Content text extraction** (`grok.rs:330-344`):

```rust
fn grok_content_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    if let Some(text) = content.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}
```

Handles three content shapes: plain string, `{type:"text", text:"..."}` object, or array of `{type:"text", text:"..."}` blocks.

**Recall** adds:
- `tool_call` and `tool_call_update` handling (formats tool calls as `[title] input` messages).
- Token usage tracking from `_meta.totalTokens` / `_meta.modelId`.
- Subagent session filtering via `session_kind`.

### 7. Resume command

**fast-resume** (`grok.rs:316-323`):

```rust
fn resume_command(&self, session: &Session, yolo: bool) -> Vec<String> {
    let mut command = vec!["grok".to_string()];
    if yolo {
        command.push("--always-approve".to_string());
    }
    command.extend(["--resume".to_string(), session.id.clone()]);
    command
}
```

- Normal: `grok --resume <id>`
- YOLO: `grok --always-approve --resume <id>`

**Recall** (`grok.rs:35-39`):

```rust
fn resume_command(&self, source_id: &str) -> Option<ResumeCommand> {
    Some(ResumeCommand {
        program: "grok".to_string(),
        args: vec!["--resume".to_string(), source_id.to_string()],
    })
}
```

- `grok --resume <id>` (no YOLO variant in Recall).

**Our PRD evidence table** says: `grok --resume <id>` (已核验来源). Confirmed by both implementations.

### 8. chat_history.jsonl fallback

**Neither fast-resume nor Recall implements a `chat_history.jsonl` fallback.** Both exclusively use `summary.json` + `updates.jsonl`. The PRD's evidence table notes "另一 adapter/version 有 `chat_history.jsonl` fallback,不得混为单一格式" — this variant exists in some other source (possibly cass or a different Grok version) but is NOT implemented in the two MIT/reference implementations we read.

**Conclusion**: The `chat_history.jsonl` variant is unverified in the implementations we have access to. It should be layered as a separate variant (`grok-build/chat-history-v1`?) only when real evidence arrives. Do NOT implement it speculatively.

### 9. Incremental sync / mtime tracking

**fast-resume** (`grok.rs:85-87`):

```rust
let mtime = file_mtime_seconds(&updates)
    .max(file_mtime_seconds(&session_dir.join("summary.json")));
files.insert(id.to_string(), (updates, mtime));
```

- mtime = max of `updates.jsonl` and `summary.json` mtimes.
- This ensures a summary.json update triggers re-parse.

**fast-resume** also handles incomplete scans (`grok.rs:43-91`): if `read_dir` fails on the sessions root, returns `None` (adapter unavailable). If individual workspace/session reads fail, `complete = false` and `deleted_ids` are cleared (fail-safe: don't delete known sessions on partial scan failure). This directly aligns with our PRD requirement R6: "失败不删除".

### 10. Percent-decoding workspace directory names

**fast-resume** (`grok.rs:361-392`):

```rust
fn grok_directory_from_path(path: &Path) -> String {
    let encoded = path
        .parent()        // session dir
        .and_then(Path::parent)  // workspace dir
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let decoded = percent_decode(encoded);
    Path::new(&decoded)
        .is_absolute()
        .then_some(decoded)
        .unwrap_or_default()
}

fn percent_decode(value: &str) -> String {
    // manual %XX decoding
}
```

- Only returns the decoded path if it is absolute (valid workspace encoding).
- Used as fallback when `summary.json`'s `info.cwd` is missing.

**Recall** (`grok.rs:226-246`): similar percent-decode, but always returns the decoded string (doesn't check is_absolute).

---

## Suggested implementation outline for our GrokBuildAdapter

Based on the above evidence and our `ProviderAdapter` trait contract (`agent-session-grep-ports/src/lib.rs:708-727`):

### Architecture decision needed: multi-file source

Our `ProviderAdapter::probe(&self, bytes: &[u8])` and `parse(&self, bytes: &[u8], sink)` receive a single byte slice. Grok sessions span two files (`summary.json` + `updates.jsonl`). Options:

1. **Discovery returns updates.jsonl; adapter derives identity from path**: `SourceDiscovery` discovers `updates.jsonl` files. The `SourceSnapshot.path` encodes the session directory. The adapter's `probe` checks for ACP `sessionUpdate` markers. The adapter's `parse` only sees updates bytes — identity/cwd/title come from path-based fallback (directory name = UUID, workspace dir = percent-encoded cwd). This is lossy (no `generated_title`, no `info.id` cross-check).

2. **Discovery returns a multi-file bundle**: Extend discovery to snapshot both files and pass a combined byte payload (e.g., summary.json bytes + delimiter + updates.jsonl bytes). More complex but preserves all metadata. This is the architecturally clean approach.

3. **Adapter owns directory access**: The adapter is constructed with the session directory and reads both files in `parse`. Breaks statelessness.

**Recommendation**: Option 2 is cleanest but requires `SourceDiscovery` changes. For the first slice, Option 1 is pragmatic — derive identity from path (UUID directory name), derive cwd from percent-decoded workspace dir, and defer `generated_title` to a future enhancement when multi-file source is available.

### Variant ID

```
grok-build/updates-jsonl-v1
```

### Probe logic

```rust
const VARIANT_ID: &str = "grok-build/updates-jsonl-v1";
const SAMPLE_LINE_LIMIT: usize = 16;

fn probe(&self, bytes: &[u8]) -> Result<ProbeResult, ProviderError> {
    // 1. UTF-8 check, strip BOM
    // 2. Sample first N non-empty lines
    // 3. For each line, parse as serde_json::Value
    // 4. Check for params.update.sessionUpdate field
    // 5. Confirmed: ≥1 line has sessionUpdate in known enum
    //    High: lines are JSON but no sessionUpdate field
    //    Low: some JSON but no ACP markers
    //    Ambiguous: no JSON lines at all
    // 6. Collect matched/unmatched evidence
}
```

### Parse logic

```rust
fn parse(&self, bytes: &[u8], sink: &mut dyn CanonicalEventSink) -> Result<ParseReport, ProviderError> {
    // 1. UTF-8 check, strip BOM
    // 2. Iterate lines with byte offset tracking (for span)
    // 3. For each line:
    //    a. Parse as Value (skip broken lines, report diagnostics)
    //    b. Navigate to params.update
    //    c. Match sessionUpdate:
    //       - user_message_chunk: extract text (skip bashCommand meta)
    //       - agent_message_chunk: extract text, accumulate by promptId
    //       - rewind_marker: truncate messages at target_prompt_index
    //       - tool_call / tool_call_update: optional, emit as assistant/tool
    //       - agent_thought_chunk / available_commands_update: skip
    //    d. Emit MessageEvent to sink with seq, role, text, timestamp, span
    // 4. Report: committed count, skipped count, diagnostics
    //    session_native_id: None (not in updates.jsonl; derived from path)
    //    session_observation: provider_session_id from path, cwd from path
}
```

### Message event mapping

| Grok concept | Our MessageEvent field |
|---|---|
| `user_message_chunk` text | `role: "user"`, `text: <extracted>` |
| `agent_message_chunk` text (accumulated) | `role: "assistant"`, `text: <accumulated>` |
| `rewind_marker` | Truncate already-emitted messages (sink must support this? or we just stop emitting and let the sink handle truncation) |
| `timestamp` field | `timestamp: Some(<as string>)` |
| Byte offset of line | `span: Some((start, end))` |
| `params.sessionId` | `native_id: <sessionId>` (if present in-band) |

**Chunk accumulation challenge**: Our `CanonicalEventSink::emit_message` pushes one message at a time. Grok streams chunks that must be accumulated into a single message. fast-resume accumulates internally before building the final content. We should accumulate chunks into a pending message buffer and only emit when the chunk group is complete (next chunk has different promptId/promptIndex, or a different sessionUpdate type appears).

**Rewind marker challenge**: `rewind_marker` requires truncating already-emitted messages. Our sink is append-only (no truncation API). Options: (a) buffer all messages and apply rewinds before emitting any; (b) ignore rewinds for the first slice and note as a known limitation. fast-resume buffers internally and truncates the in-memory vector. **Recommendation**: buffer internally, apply rewinds, then emit the final ordered message list. This matches fast-resume's approach but requires the adapter to hold state during parse (acceptable — parse is a single call).

### Resume metadata (ADR-0009)

- `provider_session_id`: Derived from path (session directory name = UUID). Set `MetadataResolution::Resolved` if the path component is a valid UUID.
- `original_working_directory`: Derived from percent-decoded workspace directory name. Set `Resolved` if the decoded path is absolute.
- `pair_observed`: `false` — these values come from path, not from the same provider record. (Unless we implement multi-file source, then `summary.json`'s `info.id` + `info.cwd` would be a true pair.)
- `multi_session`: `false` — one updates.jsonl = one session.

### Resume command

```rust
// capability matrix entry:
// resume_command: "grok --resume <id>"
// yolo: "grok --always-approve --resume <id>"
```

### Test fixtures needed

Based on fast-resume's test patterns (`grok.rs:394-495`):

1. **Basic streamed messages**: Two user chunks (same promptIndex) + two agent chunks (same promptId) → verify accumulation.
2. **Rewind marker**: Messages before rewind are discarded, replacement messages appear.
3. **Percent-decode workspace**: `%2Fwork%2Fproject` → `/work/project`.
4. **bashCommand skip**: User chunk with `_meta.bashCommand` is skipped.
5. **Empty/whitespace agent chunks**: Skipped.
6. **Mixed timestamp formats**: ISO-8601 string, seconds int, millis int.

All fixtures must be synthetic (no real user data), per PRD requirement.

---

## Caveats / Not Found

1. **No `chat_history.jsonl` implementation found** in either fast-resume or Recall. This variant is documented in our PRD evidence table as existing in another source, but no MIT-licensed implementation was available for study. Do NOT implement it speculatively.

2. **Multi-file source challenge unresolved**: Our `ProviderAdapter` trait receives a single `&[u8]`, but Grok sessions require reading both `summary.json` and `updates.jsonl`. The architecture decision (Options 1-3 above) must be resolved before implementation. This may require a `SourceDiscovery` extension or a convention for multi-file payloads.

3. **Rewind marker truncation vs. append-only sink**: Our `CanonicalEventSink` has no truncation API. The adapter must buffer messages internally and apply rewinds before emitting. This is a deviation from the streaming intent of the trait (which says "绝不整体加载" in docs), but is necessary for correctness. The Claude adapter also does line-by-line streaming; Grok's chunk-accumulation + rewind semantics make true streaming impossible without a truncation API.

4. **Recall license not verified**: Recall's `src/adapters/grok.rs` was read for additional format understanding (tool_call handling, usage tracking, subagent filtering), but its LICENSE was not checked in this pass. Treat Recall code as reference-only until verified. fast-resume (MIT) is the safe-to-adapt source.

5. **`session_search.sqlite` in sessions root**: Recall notes this file may appear in the sessions directory. Our discovery must skip it (it is not a workspace directory).

6. **Subagent sessions (`session_kind: "subagent"` / `"subagent_resume"`)**: Recall filters these out. For our adapter, this is relevant to the sidechain facet work. The first slice may include all sessions and defer subagent filtering to `08-15-structured-activity-context-facets`.
