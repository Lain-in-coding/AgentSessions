# Research: Provider CI verification for main run 37534613157

- Query: Does GitHub Actions run `37534613157` at `fa7a0a92bb22270a522b0b4b3b95a16d4365ae1a` justify changing `IB-CI-PROVIDER-EVIDENCE-001` to `ci_verified`, and which target/command claims does it support?
- Scope: mixed — current repository contracts and implementation, pinned GitHub workflow/source, run/job metadata, and all four downloaded artifact payloads. Provider tests + open-source gate only; no archive verification or task planning.
- Date: 2026-10-07
- Mode: planning/research only. No product documentation, code, workflow, task manifests, PRD, or Git state was changed.

## Findings

### 1. Decision

**Yes: the existing, narrowly scoped provider-tests + open-source-gate row has sufficient artifact-level evidence for `ci_verified`.** Record this exact run and its four jobs/artifacts in the matrix before changing the row. The missing cross-target execution evidence for these two steps is now available; this does **not** satisfy every requirement of RFC-0002 provider promotion.

All four archives were downloaded and inspected in memory. Their archive SHA-256 digests match GitHub metadata. Each contained the provider test log, gate manifest, environment report, benchmark smoke JSON/Markdown summary, and unsigned target binary. The extracted binary hash and size match `environment.json`; its hash also matches the gate manifest and smoke summary. The conclusion is therefore not based merely on green job labels.

**No artifact access operation is blocked or failed.** There is no need to rerun or dispatch a workflow. The remaining action is a narrowly scoped documentation change by the main/implement session after planning approval.

### 2. Run identity and exact targets

- Repository: `LainHappy/AgentSessions`.
- Run: <https://github.com/LainHappy/AgentSessions/actions/runs/37534613157>.
- Workflow: `core-beta-evidence`; workflow ID `320357062`; run number `235`; attempt `1`.
- Trigger: `push` on `main`; conclusion `completed/success`.
- Head: `fa7a0a92bb22270a522b0b4b3b95a16d4365ae1a`.
- Started: `2026-10-06T21:32:29Z`; completed/updated: `2026-10-06T21:40:22Z` (2026-10-07 in Asia/Shanghai).
- The pinned workflow, `open_source_gate_benchmark.py`, and `core_beta_benchmark.py` were fetched through the GitHub Contents API and compared with the current workspace. All three are identical after CRLF/LF normalization. These are the exact current step/harness definitions, not an older approximation.

| Profile / job name | Hosted runner and observed OS | Exact target triple | Job ID / URL | Artifact name / ID / URL |
|---|---|---|---|---|
| `linux-gnu-x64` / Ubuntu 22.04 / GNU x64 | `ubuntu-22.04`; Ubuntu 22.04.5 LTS | `x86_64-unknown-linux-gnu` | [112512280759](https://github.com/LainHappy/AgentSessions/actions/runs/37534613157/job/112512280759) | [`core-beta-linux-gnu-x64-37534613157` / 11445254709](https://github.com/LainHappy/AgentSessions/actions/runs/37534613157/artifacts/11445254709) |
| `windows-x64` / Windows x64 / MSVC | `windows-2022`; Windows kernel `10.0.20348` | `x86_64-pc-windows-msvc` | [112512281180](https://github.com/LainHappy/AgentSessions/actions/runs/37534613157/job/112512281180) | [`core-beta-windows-x64-37534613157` / 11446346270](https://github.com/LainHappy/AgentSessions/actions/runs/37534613157/artifacts/11446346270) |
| `macos-intel` / macOS Intel / x64 | `macos-15-intel`; macOS 15.7.9 | `x86_64-apple-darwin` | [112512281151](https://github.com/LainHappy/AgentSessions/actions/runs/37534613157/job/112512281151) | [`core-beta-macos-intel-37534613157` / 11446511431](https://github.com/LainHappy/AgentSessions/actions/runs/37534613157/artifacts/11446511431) |
| `macos-arm64` / macOS Apple Silicon / ARM64 | `macos-15`; macOS 15.7.9 | `aarch64-apple-darwin` | [112512281060](https://github.com/LainHappy/AgentSessions/actions/runs/37534613157/job/112512281060) | [`core-beta-macos-arm64-37534613157` / 11446670557](https://github.com/LainHappy/AgentSessions/actions/runs/37534613157/artifacts/11446670557) |

Every job reports success for step 9, `Claude and Codex provider evidence tests`, and step 10, `Open-source gate benchmark`. These are **four successful target jobs and eight successful relevant step executions**, with no failed/skipped relevant step. This is not a count of test cases or CLI invocations.

Artifact downloads used the authenticated REST endpoint `https://api.github.com/repos/LainHappy/AgentSessions/actions/artifacts/<artifact-id>/zip`. All four metadata records have `expired: false` and the same run/head identity.

| Profile | Verified archive SHA-256 | Expiry (UTC) |
|---|---|---|
| Linux GNU x64 | `ddfd192fcb6c2fff6f6c0529a1184765d5e1e99d62b431e8ea4aa6cf1267d6cd` | `2026-10-13T21:36:49Z` |
| Windows x64 | `2150c912cd5f50ac86cdedb651bf1670660323e7961ccbee900d0b032eee557a` | `2026-10-13T21:36:48Z` |
| macOS Intel | `f197fcc8c1614bb376f6a20ee3150695a8299ddce6e2f697c51cd59b13599a36` | `2026-10-13T21:40:17Z` |
| macOS ARM64 | `3293e7e65e14aa6bad62771507af9b35ab9259e8470e13b76d476e18bf60ae24` | `2026-10-13T21:37:22Z` |

| Profile | Extracted binary bytes | Verified binary SHA-256 |
|---|---:|---|
| Linux GNU x64 | 8,084,480 | `c5b29605542ce25511c98a258b233f7b7eef81e2dd438b18edc4a79deb22130d` |
| Windows x64 | 7,393,280 | `437d68078ab466cbf6f21fc63b7f941f4e72d4715f106f855d4056b6a7ed5164` |
| macOS Intel | 7,626,224 | `d7fdc850a1b9b26e2072e0ed9d391b7fd898d85086535a65826b75e6de67e013` |
| macOS ARM64 | 6,905,168 | `a5fda97bcac998a053851c8e7dd50d518727a603d0d632e014123d04d1942e2a` |

### 3. Exact workflow commands covered

Use `<target>` and `<profile>` **only** for the paired values in the table above; `<binary>` is that target's release `agent-session-grep[.exe]`. The workflow first builds it with `cargo build --locked --release --target <target> -p agent-session-grep-cli` (`.github/workflows/core-beta-evidence.yml:81-88`).

Provider step (`.github/workflows/core-beta-evidence.yml:128-140`):

```text
cargo test --locked --target <target> -p agent-session-grep-provider-claude -p agent-session-grep-provider-codex --all-targets -- --nocapture
```

Gate step (`.github/workflows/core-beta-evidence.yml:142-155`):

```text
python -m unittest scripts/evidence/test_open_source_gate_benchmark.py
python scripts/evidence/open_source_gate_benchmark.py run --profile <profile> --workspace . --output-dir evidence/ci/open-source-gate --binary <binary>
python scripts/evidence/open_source_gate_benchmark.py validate-report evidence/ci/open-source-gate/gate-manifest-<profile>.json
```

Both steps set `$ErrorActionPreference = 'Stop'` and `$PSNativeCommandUseErrorActionPreference = $true`. The provider command does **not** use `--release`; its archived test executable paths are under `target/<target>/debug/deps`. The gate executes the previously built **release CLI**. Do not describe this as release-profile provider tests, workspace-wide tests, or certification of every implemented provider.

The Python unittest and explicit validator commands are present in the successful exact step. The archives do not contain a separate Python unittest result log/count, so the Rust totals below must not be presented as totals for those Python tests.

### 4. Provider test results: actual archived counts

Source in **each** archive: `evidence/ci/provider-evidence-tests.txt`. All four have the same eight Rust test-binary summaries:

| Package / suite | Passed | Failed | Ignored (not run) |
|---|---:|---:|---:|
| Claude `src/lib.rs` unit tests | 69 | 0 | 1 |
| Claude `tests/golden.rs` | 9 | 0 | 1 |
| Claude `tests/lifecycle.rs` | 8 | 0 | 0 |
| Claude `tests/properties.rs` | 6 | 0 | 0 |
| Codex `src/lib.rs` unit tests | 61 | 0 | 1 |
| Codex `tests/golden.rs` | 6 | 0 | 1 |
| Codex `tests/lifecycle.rs` | 8 | 0 | 0 |
| Codex `tests/properties.rs` | 2 | 0 | 0 |
| **Per target** | **169** | **0** | **4** |
| **All four targets, execution occurrences** | **676** | **0** | **16** |

Per provider per target: Claude **92 passed / 0 failed / 2 ignored**; Codex **77 / 0 / 2**. Across targets these are repeated executions of the same suites, not 676 distinct tests. All summaries report zero measured and zero filtered-out cases.

The four ignored cases per target are explicitly identified in the downloaded logs:

1. Claude `tests::prefilter_raw_timing_10k_ignored_rows`: raw timing microbenchmark; explicit release-mode execution required.
2. Claude `print_actual_thinking_canonical_output_for_regeneration`: manual canonical-fixture regeneration helper.
3. Codex `tests::prefilter_raw_timing_10k_ignored_rows`: the corresponding manual timing microbenchmark.
4. Codex `print_actual_canonical_output_for_regeneration`: manual canonical-fixture regeneration helper.

These are **not real-E5 skips**. They must remain visible as ignored, rather than being silently folded into passes.

Named passing results include canonical golden output and provenance revision checks; exact-source-line/span round trips; probe/parse source-byte nonmutation; deterministic/fixed-seed properties; and append/shrink/torn-tail/same-length-rewrite lifecycle cases. Examples include `golden_spans_slice_back_to_exact_source_lines`, `golden_basic_spans_slice_back_to_source_envelope_lines`, `prop_span_roundtrips_to_source_record`, `properties_hold_for_fixed_seed_corpus`, and each provider's `prop_append_keeps_prefix_byte_stable`. The log also preserves negative-contract cases such as Codex `linear_rollout_never_fabricates_parent_edges`; no extra upstream-provider capability should be inferred from their names.

### 5. Open-source gate manifests and supported live CLI claims

Files: `evidence/ci/open-source-gate/gate-manifest-<profile>.json`, schema `agent-session-grep.open-source-gate/v1`. All four identify the requested head, their release binary hash, and `gate-synthetic-v1` containing Claude Code and Codex fixtures. Every manifest records `corpus.kind = deterministic_synthetic_labeled`, `contains_real_transcripts = false`, and `file_count = 3`.

| Metric | Value in every target manifest | Threshold | Verdict |
|---|---:|---:|---|
| `lexical_recall_at_10` | 1.0 | >= 0.95 | pass |
| `parse_loss_ratio` | 0.0 | <= 0.05 | pass |
| `discovery_coverage` | 1.0 | >= 0.95 | pass |
| `resume_handoff_success` | 1.0 | >= 1.0 | pass |
| `semantic_recall_at_10` | 1.0 | null | **informational; `pass: null`** |
| `hybrid_recall_at_10` | 1.0 | null | **informational; `pass: null`** |

For every target, `gate = {pass: true, failures: [], deferred: []}`. Thus the accounting is **4 threshold passes / 0 failures / 0 deferred per target**, or **16 / 0 / 0** across four targets, plus **8 informational metric observations with no pass/fail verdict**. Do not count six gate passes per target, and do not turn the absence of gate-deferred metrics into a claim that real E5 was tested.

All four manifests' detailed results also agree:

- `parse_loss_detail`: **12 emitted, 0 skipped**, ratio 0.0 per target (48 emitted execution occurrences and 0 parse skips in aggregate).
- `discovery_detail`: **3 planted / 3 found**, both planted providers complete; Claude **2/2**, Codex **1/1**. `reads_real_transcripts: false`; the mechanism is `sync --discover` against a redirected synthetic HOME.
- `resume_handoff_detail`: **9 attempted / 9 succeeded** per target = **3/3 resume previews + 6/6 handoff packs**. Every resume detail has `executed: false`. Across targets: **36/36**, comprising **12 previews + 24 handoff packs**, not 12 launched provider sessions.

The following commands have live **ASG CLI process** evidence on the synthetic corpus on each of the four named targets. The common wrapper is `<binary> --db <temporary-db> --output json ...` (`scripts/evidence/core_beta_benchmark.py:190-195`).

| Command/operation | Manifest-backed observations per target | Exact limit of the claim |
|---|---:|---|
| Initial `sync` over the gate fixtures | 1 latency sample; 12 emitted / 0 skipped | Synthetic fixtures only; not a real-corpus regression |
| `sync --discover` | 1 sample; 3/3 sources found | Redirected HOME/USERPROFILE; not discovery on an owner's installed provider data |
| `search <query> --max-items 10` | 6 samples; lexical recall 1.0 | Six labeled synthetic queries |
| `search <query> --max-items 10 --mode semantic` | 6 samples; informational recall 1.0 | Bigram-hash vectorizer, **not real E5 semantic quality** |
| `search <query> --max-items 10 --mode hybrid` | 6 samples; informational recall 1.0 | Lexical + bigram-hash fusion; no semantic gate threshold |
| `show <known-wire-id>` | 10 samples | Repeated known-ID lookup, not ten distinct source records |
| `get <known-wire-id>` | 10 samples | Same limitation; not an independent privacy certification |
| `resume <canonical-session-id>` | 3 samples; 3 successful previews | No `--execute`; no provider process spawned |
| `handoff <query>` | 6 samples; 6 nonempty evidence packs | Synthetic pack-generation check, not actual downstream delivery |

The nine latency categories total **49 recorded samples per target / 196 across targets**. This is only the sum of `latency_p50_p95_ms.*.count`, **not** a complete count of all harness subprocesses and **not** a standalone pass/fail/skip command matrix. No such comprehensive command-result manifest is present in these archives.

Relevant implementation patterns:

- `scripts/evidence/core_beta_benchmark.py:141-195`: runs actual subprocesses; a nonzero exit raises, and the Robot frame must have `ok: true`. These are not mocked command successes.
- `scripts/evidence/open_source_gate_benchmark.py:117-160`: actually executes each labeled search with `--max-items 10`, adding `--mode` for semantic/hybrid and recording requested/effective modes.
- `scripts/evidence/open_source_gate_benchmark.py:174-244`: plants fixture copies, redirects HOME/USERPROFILE, executes discovery, and reports per-provider counts.
- `scripts/evidence/open_source_gate_benchmark.py:284-380`: resume is explicitly dry-run; success requires an available command and `executed: false`; handoff success requires nonempty evidence.
- `scripts/evidence/open_source_gate_benchmark.py:490-598,649-709`: semantic/hybrid metrics intentionally have null threshold/pass; validator distinguishes them from the four threshold metrics.

### 6. Minimal recommended matrix edit — not applied

Anchors below are from the inspected current `docs/operations/core-beta-evidence-matrix.md`; relocate by heading/evidence ID if concurrent edits move the lines.

1. **Line 20**, under `## Recorded CI run`: change “Two runs” to “Three runs”; retain the roles of `30165919066` and `36391073119`, and add that `37534613157` backs only the provider-tests/open-source-gate row.
2. **After the correction paragraph at line 37, before line 39**: append a named-run record with this run URL, full head, `push/main`, four exact target/job/artifact links above, successful relevant steps, review date, and seven-day retention. Putting it after the correction preserves that paragraph's existing “Both runs above” reference to the historical pair. Include the **169/0/4 per-target provider count** and **4/0/0 threshold-gate count**, with the synthetic/dry-run/non-E5 caveat.
3. **Line 52**, `IB-CI-PROVIDER-EVIDENCE-001`:
   - Change only its status from `ci_configured_only` to `ci_verified`.
   - Replace the missing-run wording with the new named run and the exact step names, referring to the four job/artifact links in the new run record.
   - Keep the existing bounded claim: Claude/Codex provider evidence tests plus the open-source gate, per named target. Do not expand it to every provider, real data, actual provider launch, privacy certification, or E5.
   - Keep the prior `95e6f5d` local verification as historical context if retaining it; it is no longer the basis for the cross-target conclusion.
4. **Line 39** can remain: `ci_verified` does not automatically imply artifact review. The new run paragraph should explicitly state that these four particular artifacts **were** reviewed; do not rewrite the older rows to claim review of their artifacts.
5. Preserve status definitions at **lines 12-16**, all other evidence rows, the release/nonpublication boundary at **line 81**, the promotion rule at **line 83**, and the composite milestone rule at **line 87**. No incidental upgrade of installer, minimum-OS, signing, notarization, release, or provider-maturity accounting is justified by this bounded review.

Suggested replacement caveat for the provider row:

> Run `37534613157` covers both named steps on all four listed runner/target pairs; the four artifacts were downloaded and reviewed on 2026-10-07. Each target reports 169 provider tests passed, 0 failed, and 4 explicitly ignored manual helpers; all four threshold gate metrics pass with no deferred threshold metric. Gate evidence uses synthetic Claude/Codex fixtures; resume is preview-only and semantic/hybrid figures use a bigram-hash vectorizer, not real E5. This supports hosted cross-target CI execution only, not provider Beta/GA promotion, privacy or real-data certification, minimum-OS/clean-machine support, or release certification.

### 7. Files found and related specs

| File / anchor | Relevance |
|---|---|
| `.trellis/workflow.md:353-380` | Research-phase persistence contract; no task activation required for this research |
| `docs/operations/core-beta-evidence-matrix.md:12-16,18-39,52,81-87` | Status vocabulary, missing named-run basis, and release/milestone boundaries |
| `.github/workflows/core-beta-evidence.yml:43-65,128-155,201-245` | Exact four targets, commands, environment provenance, unsigned upload, seven-day retention |
| `scripts/evidence/core_beta_benchmark.py:141-195` | Shared real-subprocess / successful-Robot-frame execution contract |
| `scripts/evidence/open_source_gate_benchmark.py:117-380,490-709` | Search/discovery/preview measurement, metric distinctions, and manifest validation |
| `crates/agent-session-grep-provider-claude/tests/{golden,lifecycle,properties}.rs` | Claude suites identified in every archived test log |
| `crates/agent-session-grep-provider-codex/tests/{golden,lifecycle,properties}.rs` | Codex suites identified in every archived test log |
| `crates/agent-session-grep-ports/src/manifest.rs:93-125` | Current manifest construction still sets `last_certified_targets: Vec::new()` at line 117; research does not change this |
| `.trellis/spec/agentsessions-cli/backend/index.md:263-282` | Synthetic smoke, aggregate-only real-data/privacy reporting, literal evidence-status rules |
| `.trellis/spec/agentsessions-provider-claude/backend/index.md:73-94` | Read-only/synthetic fixture and lifecycle evidence requirements |
| `.trellis/spec/agentsessions-provider-codex/backend/index.md:75-93` | Equivalent Codex evidence limits and no invented parent edges |
| `.trellis/spec/agentsessions-testkit/backend/index.md:19-29` | Test-only helpers and synthetic/redacted fixture requirement |
| `docs/architecture/RFC-0002-provider-adapter-contract.md:90-99,117-125` | Cross-target evidence is necessary for certification records, not automatic maturity promotion |
| `docs/product/PROVIDER-BETA-READINESS.md:104-110,140-149` | ADR-0010 acceptance and explicit owner promotion decision remain separate gates |
| `docs/product/PROVIDER-MATURITY-MATRIX.md:28-29` | Claude and Codex remain Experimental |
| `docs/security/FIXTURE-REDACTION-POLICY.md:16-26,39-42` | Synthetic fixture policy is distinct from an executed secret/PII scan |

External references and versions:

- Run/job/artifact URLs above are the authoritative execution records. Metadata was read with structured `gh api` responses, not inferred from search summaries.
- Pinned workflow: <https://github.com/LainHappy/AgentSessions/blob/fa7a0a92bb22270a522b0b4b3b95a16d4365ae1a/.github/workflows/core-beta-evidence.yml>.
- Pinned gate: <https://github.com/LainHappy/AgentSessions/blob/fa7a0a92bb22270a522b0b4b3b95a16d4365ae1a/scripts/evidence/open_source_gate_benchmark.py>.
- Pinned shared harness: <https://github.com/LainHappy/AgentSessions/blob/fa7a0a92bb22270a522b0b4b3b95a16d4365ae1a/scripts/evidence/core_beta_benchmark.py>.
- Retrieval tool: GitHub CLI `2.94.0` (2026-06-10); installed `gh run view --help` / `gh api --help` were consulted.
- All four environment reports record Rust `1.99.0` (`b940084d7`, 2026-09-28), Cargo `1.99.0` (`5f94df478`, 2026-08-27), and unsigned binaries. Workflow requests Python `3.10`; upload action is pinned `actions/upload-artifact` v6.0.0.
- These are multiple corroborating records from **one CI run**, not evidence of repeated independent successful runs.

## Caveats / Not Found

1. **Preserve `skip_real_e5`.** This run does not execute or certify real E5. Its semantic/hybrid metrics explicitly use bigram-hash and carry null verdicts. No exact `skip_real_e5` field/result was found in these artifact manifests, the inspected workflow, or a literal search of product docs/schemas/scripts/workflows. Therefore do not invent an E5 skip count for this run, and do not erase or promote any separately tracked `skip_real_e5` result. Rust's 16 ignored execution occurrences are unrelated manual helpers.
2. **No provider-certification or privacy report.** Each archive has 14 entries, but none is a standalone provider certification, authorized real-data regression, or privacy-scan report. `contains_real_transcripts: false` describes the corpus; it does not prove a PII/secret scan passed. Golden/read-only tests are adapter evidence, not successful execution of the upstream Claude/Codex CLIs or proof of their login/runtime compatibility.
3. **No maturity/release promotion.** Current `last_certified_targets` remains empty; Claude/Codex remain Experimental. Do not alter these or assume owner approval/ADR acceptance. Hosted Ubuntu 22.04, Windows Server 2022, and macOS 15 do not establish glibc 2.31, Windows 10 clean-machine support, macOS 12 runtime support, musl, signing, notarization, or release certification. Composite maturity/release checkboxes remain unchanged.
4. **Self-reported labels are not accounting authority.** `environment.json` retains `accounting_status_before_review: ci_configured_only`; the separate smoke summary labels its measurements `locally_verified` and its binary `caller_supplied_prebuilt`. Neither field changes the matrix automatically. The smoke summary also says recovery evidence is `not_implemented`; it supplies no additional provider promotion claim. Artifact digest/binary agreement is an integrity cross-check, not an independent build attestation or signature.
5. **Do not claim identical gate corpus bytes across hosts.** Gate fixture hash is `09197b648d17f4f619edc8bc97857c0783214f0087e70cc3f620c974fb29f7be` on Linux/macOS and `39b8814e0a63307dbaaa8c143e0f65f17501e2b3f35bd55b53073041366b1361` on Windows. The manifests agree on fixture set, provider counts and results, but do not justify a byte-identical-corpus claim. The reason for the hash difference was not independently diagnosed in this bounded review.
6. **Retention and handling.** All downloads and ZIP entry inspections were in memory; no raw artifact, transcript, extracted executable, runner path, or local scratch path was written into the repository. This file persists reviewable aggregates, IDs and hashes, not raw artifacts. GitHub archives expire at the UTC times above; this research record does not make the hosted evidence release-certified or relabel the matrix row `locally_verified`.
7. **Access outcome.** All four artifact downloads, metadata reads, and pinned source checks succeeded. No access failure is being hidden or left waiting. No workflow dispatch, local replay of downloaded binaries, task start/archive, commit, push, or Git command was performed.
