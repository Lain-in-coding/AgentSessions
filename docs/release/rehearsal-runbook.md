# Release Rehearsal Runbook

> Task: `08-15-final-integration-release-rehearsal`
> Status: framework ready — feature-dependent steps marked `pending feature <subtask>`

This runbook defines the full release-rehearsal procedure for
agent-session-grep. It is executed per-platform (Windows, macOS, Linux) in a
clean environment before the owner makes the final go/no-go decision.

**Hard rule**: every step records its command, run id, wall-clock time, errors,
and a pass/fail verdict. Anything the docs don't cover, a command that doesn't
work, or an unexpected result is a defect and flows back into the task tree.

---

## 0. Clean environment setup

### 0.1 Platform requirements

Each rehearsal run uses a clean VM or container image — **never** a
GitHub-hosted runner. The `core-beta-evidence.yml` CI job marks hosted runners
as `ci_configured_only` (preinstalled toolchains disqualify them from
release-certified status).

| Platform | Image requirement | Notes |
|---|---|---|
| Windows | Windows 11 (clean install, no dev tools beyond the rehearsal toolchain) | PowerShell 7+ |
| macOS | macOS 14+ (clean VM or container, no Homebrew dev packages preinstalled) | zsh default shell |
| Linux | Ubuntu 22.04 LTS (clean container or VM) | bash default shell |

### 0.2 Toolchain

Install only what the rehearsal needs:

- Rust stable toolchain (`rustup` — the minimum supported toolchain for the
  workspace `rust-version`)
- Python 3.10+ (for the consistency script and evidence harnesses)
- Git (to clone the repo at the release tag)

### 0.3 Environment manifest

Before starting, fill in the environment manifest template at
`docs/release/environment-manifest.template.json`. Record:

- OS name, version, build number
- Clean image identifier (VM snapshot name or container image digest)
- Installer artifact SHA-256 hash
- Tool versions (rustc, cargo, python, git)
- Provider fixture license + redaction status

A completed manifest is the entry ticket — no manifest, no rehearsal.

---

## 1. Install

### 1.1 Build and install from source

```bash
cargo build --release --locked
# Run the platform install script:
#   Windows: pwsh scripts/install/install.ps1   (PowerShell 7+; scripts use $IsWindows)
#   macOS/Linux: bash scripts/install/install.sh
```

**Expected evidence**: install script exit code 0; binary present at the
user-level prefix; `agent-session-grep --version` prints the expected version.

**Run id**: record as `install-<platform>-<date>`.

---

## 2. Ingest / index synthetic corpus

### 2.1 Prepare fixture corpus

Use the synthetic fixture generator from `scripts/evidence/core_beta_benchmark.py`
or a hand-written set of Claude Code JSONL files with synthetic content only.
**Never** use real provider transcripts without explicit operator authorization.

```bash
# Generate a smoke-profile corpus (2 files, 12 messages each):
python scripts/evidence/core_beta_benchmark.py run --profile smoke \
    --output-dir /tmp/rehearsal-corpus/ --workspace .

# Or use a hand-written fixture directory with .jsonl files.
```

(Full `core_beta_benchmark.py` usage: `--help`. It also supports
`validate-report <report>` for evidence verification.)

### 2.2 First index

```bash
agent-session-grep --db /tmp/rehearsal.db ingest /tmp/rehearsal-corpus/*.jsonl
```

**Expected evidence**: exit code 0; robot envelope reports `committed > 0`,
`skipped == 0`.

**Run id**: `ingest-<platform>-<date>`.

---

## 3. Search

### 3.1 Lexical search

```bash
agent-session-grep --db /tmp/rehearsal.db --robot search "<canonical-query>"
```

**Expected evidence**: hits returned; each hit has `id`, `score`, `session_id`,
`text`; `page.has_more` is a boolean; `outcome` is `success` or `partial`.

### 3.2 Semantic / hybrid search

```text
[pending feature 08-15-semantic-hybrid-local-retrieval]
```

When semantic search lands, repeat §3.1 with the semantic and hybrid retrieval
modes. Record per-mode latency and result-set overlap.

---

## 4. Context

```bash
agent-session-grep --db /tmp/rehearsal.db --robot context "<session-id-from-search>"
```

**Expected evidence**: mainline messages returned in order; sidechains excluded
by default; evidence spans are present.

**Run id**: `context-<platform>-<date>`.

---

## 5. Resume dry-run

```text
[pending feature 08-15-resume-metadata-execution]
```

When the resume command matrix lands:

```bash
agent-session-grep --db /tmp/rehearsal.db --robot get-session-resume "<session-id>"
```

Verify: `resume_available` is a boolean; `provider_session_id` and
`original_working_directory` are present only when resolved; dry-run mode is
the default (no side effects); first-run forces a preview.

---

## 6. Handoff pack generation

```text
[pending feature 08-15-evidence-handoff-pack]
```

When the handoff pack schema lands:

```bash
agent-session-grep --db /tmp/rehearsal.db --robot handoff "<session-id>"
```

Verify: pack conforms to `handoff-pack/v1` schema; evidence and inference are
in separate sections; budget/truncation/redaction rules are applied; the pack
is deterministic (two runs produce identical output).

---

## 7. Web UI walkthrough

```text
[pending feature 08-15-loopback-web-ui-parity]
```

When `asg serve` lands:

```bash
agent-session-grep --db /tmp/rehearsal.db serve
```

Walk through: search → session detail → context → evidence highlight →
resume action (dry-run). Verify parity with CLI output for the same queries.

---

## 8. Evidence review

```bash
python scripts/evidence/core_beta_benchmark.py run --profile smoke \
    --output-dir /tmp/rehearsal-bench-evidence/ --binary $(which agent-session-grep)
```

**Expected evidence**: benchmark report JSON with all invariant verdicts
passing, then `validate-report` confirms the report is well-formed.

**Run id**: `evidence-<platform>-<date>`.

---

## 9. Five-entry-point consistency

```bash
python scripts/rehearsal/compare_entrypoints.py \
    --binary $(which agent-session-grep) \
    --out /tmp/consistency-report.json
```

**Expected evidence**: `overall_verdict == "consistent"`; every declared entry
point appears as compared, aliased, or explicitly skipped; pending entry points
have `status: "skipped"`, `reason: "not implemented"`.

**Run id**: `consistency-<platform>-<date>`.

---

## 10. Privacy final check

### 10.1 Zero telemetry

```text
[pending feature 08-15-offline-privacy-hooks]
```

When offline mode lands:

1. Run the full rehearsal with network capture active (Wireshark / tcpdump /
   `netstat`). Verify zero outbound connections except explicit model downloads.
2. Run `agent-session-grep --offline` through all core commands. All must
   succeed without network.

### 10.2 Redaction spot-check

```text
[pending feature 08-15-offline-privacy-hooks]
```

Verify cross-boundary outputs (Web, Handoff, MCP, Robot) apply default
redaction. Inject a secret-pattern fixture and confirm it never appears in
output.

### 10.3 Log / diagnostic audit

Inspect stderr and any log files produced during the rehearsal. Confirm no
transcript content, absolute source paths, provider-native ids, fingerprints,
usernames, or hostnames appear in diagnostics.

---

## 11. Performance final check

```text
[pending feature 08-15-benchmark-install-open-source-gate]
```

Run the full Gate D benchmark suite against the rehearsal corpus. Verify all
six invariants pass. Refresh the benchmark report with this run's numbers.

---

## 12. Release materials check

Verify that README, quickstart, comparison table, demo dataset, and benchmark
numbers are mutually consistent. Check LICENSE, third-party attributions, and
fixture provenance.

---

## 13. Uninstall

```bash
# Windows: pwsh scripts/install/uninstall.ps1
# macOS/Linux: bash scripts/install/uninstall.sh
```

**Expected evidence**: exit code 0; binary removed; idempotent (second run
reports "not installed", exit 0); no directory recursively deleted.

**Run id**: `uninstall-<platform>-<date>`.

---

## 14. Reinstall idempotency

Repeat §1. Verify the second install succeeds identically. The rehearsal db is
untouched (install must not delete user data).

**Run id**: `reinstall-<platform>-<date>`.

---

## 15. Go/No-Go report

Fill in `docs/release/go-no-go.template.md` with:

- All run ids and their pass/fail verdicts
- Residual risk list
- Five-entry consistency report
- Privacy / performance / materials check results
- Owner sign-off block

Submit to owner for the final public-release decision.

---

## Evidence artifact checklist

| Step | Artifact | Format |
|---|---|---|
| 0 | environment manifest | JSON |
| 1 | install log | text |
| 2 | ingest robot envelope | JSON |
| 3 | search robot envelope | JSON |
| 4 | context robot envelope | JSON |
| 5 | resume robot envelope | JSON |
| 6 | handoff pack | JSON |
| 7 | Web UI screenshots + parity notes | PNG + markdown |
| 8 | benchmark report | JSON |
| 9 | consistency report | JSON |
| 10 | privacy audit notes | markdown |
| 11 | Gate D benchmark report | JSON |
| 12 | materials checklist | markdown |
| 13 | uninstall log | text |
| 14 | reinstall log | text |
| 15 | go/no-go report | markdown |

All artifacts are stored under `evidence-output/rehearsal-<date>/` (gitignored).
