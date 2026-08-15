# Resume metadata execution — Read-only surfaces (Implementation)

## Preconditions

- Slice scope: read-only preview surfaces only. No process spawn, no
  provider file writes, no clipboard, no auto-cd.
- Worktree `agent-a02fbc1d15a731bef` (based on `main`, pre-resume-core).
  Sibling branch `session-metadata-search-08-15` holds the resume CORE
  (uncommitted) — read from it, never write to it.
- Design: `design.md` (D1-D5).

## Steps

### 1. Ports — mirror resume claim types (byte-identical)

- `crates/agent-session-grep-ports/src/lib.rs`: insert the ADR-0009 resume
  block after `ParseReport` and before `MessageEvent`, byte-identical to
  `session-metadata-search-08-15` (`MetadataResolution<T>`,
  `ProviderSessionObservation`, `SessionResumeMetadata`, `ResumeClaimsStore`,
  `SourceResumeClaim`, `NoResumeClaims`, `&T` blanket impl).
- Verified: diff of the block between worktrees is empty.

### 2. Application — `ResumePreviewService` + preview types

- `crates/agent-session-grep-application/src/lib.rs`: new section before the
  tests module: `EXECUTION_NEVER_BY_THIS_TOOL` ("never-by-this-tool"),
  `ResumeCommandTemplate { argv, execution }`, `ResumeUnavailableReason`
  (NoClaims / ConflictingClaims / UnsupportedProvider + `as_str()`),
  `ResumePreview`, `ResumePreviewService<R: ResumeClaimsStore>`
  (`resume_preview(&StableId) -> PortResult<ResumePreview>`),
  private `resume_command_template(provider_id)` mapping (claude-code /
  codex; unknown → None), `preview_from_metadata`, `classify_unavailable_reason`.
- Fail-closed rules: `resume_available=false` + reason for missing /
  conflicting claims and unknown providers; command template only when
  available; cwd passes through only from pair-observed resolved claims.

### 3. CLI — `resume` subcommand (read-only)

- `crates/agent-session-grep-cli/src/main.rs`:
  - `"resume"` dispatch arm: `no_extra_args(rest, 1, ...)`, parse wire id
    (`StableId::from_wire`, invalid → usage error exit 2), wire
    `ResumePreviewService::new(&NoResumeClaims)` (comment marks the
    one-line swap to `store_ref(store)` after merge), build data JSON
    (session_id, provider_id, resume_available, provider_session_id,
    original_working_directory, command { argv, execution },
    unavailable_reason), return success envelope.
  - Register in `known_subcommand`, top-level `help_text` COMMANDS,
    `subcommand_help_text`, unknown-command suggestion list, test
    `KNOWN_COMMANDS` const. `resume` is NOT in the writer-lease set
    (read-only open).
- `crates/agent-session-grep-cli/src/human.rs`: `"resume" => render_resume(data)`
  — labeled copy-by-hand preview (placeholder substituted in the command
  line only) or unavailable + reason.

### 4. Tests (synthetic fixtures only)

- Application (`resume_preview_tests`): resolved claude claim → available +
  template `["claude","--resume","<provider-session-id>"]` +
  `never-by-this-tool`; resolved codex claim → codex template; ambiguous →
  ConflictingClaims; missing → NoClaims; unknown provider →
  UnsupportedProvider; reason codes match store vocabulary.
- CLI human tests: available preview lines incl. copy-by-hand command;
  fail-closed preview lines with reason.
- `crates/agent-session-grep-cli/tests/e2e.rs`: envelope shape for
  `resume ses_v1_synthetic` (fail-closed, `command: null`, reason
  "no resume metadata claims"); human-mode output; missing/malformed
  session id → exit 2 invalid_request; `resume --help` subcommand help.

### 5. Main-repo task artifacts (the only main-repo writes)

- `design.md` / `implement.md` created; `implement.jsonl` / `check.jsonl`
  appended with spec file entries.

## Validation

```
CARGO_TARGET_DIR='C:/AgentSessions/target-08-15-resume-execution' \
  cargo fmt --all --check
CARGO_TARGET_DIR='C:/AgentSessions/target-08-15-resume-execution' \
  cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR='C:/AgentSessions/target-08-15-resume-execution' \
  cargo test --workspace
```

## Review gates

- No absolute transcript/source path in any output or fixture (synthetic
  `/tmp/...` only).
- No process spawn anywhere in the slice.
- Ports resume block byte-identical to the sibling branch.
- `--discover`/other-task surfaces untouched; no git commit/push.
