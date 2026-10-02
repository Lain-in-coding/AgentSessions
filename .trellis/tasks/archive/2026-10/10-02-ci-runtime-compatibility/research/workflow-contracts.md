# Current workflow contracts and scope boundary

Verified: 2026-10-02T03:11:12Z. Baseline: `ffaf69b93d696d07708516a326436caae48326da`.
This is repository/official-source research, not an implementation or future-runner certification.

## Current pins and use sites

| Action | Current immutable commit | Workflow and line | Proposed first-batch use sites |
|---|---|---|---:|
| `actions/checkout` | `11bd71901bbe5b1630ceea73d27597364c9af683` | `ci.yml:21`, `ci.yml:93`, `ci.yml:184`, `core-beta-evidence.yml:69`, `release.yml:35`, `release.yml:135`, `release.yml:205`, `security-audit.yml:21` | 5 |
| `dtolnay/rust-toolchain` | `4360b52568e2003a75bf9bc1d59f33a8e3fc893c` | `ci.yml:24`, `ci.yml:96`, `core-beta-evidence.yml:77`, `release.yml:44`, `release.yml:144` | 0 |
| `actions/setup-python` | `a26af69be951a213d495a4c3e4e4022e16d87065` | `ci.yml:28`, `ci.yml:98`, `core-beta-evidence.yml:72`, `release.yml:40`, `release.yml:140`, `release.yml:209` | 3 |
| `actions/cache` | `0057852bfaa89a56745cba8c7296529d2fc39830` | `ci.yml:33`, `ci.yml:103` | 2 |
| `EmbarkStudios/cargo-deny-action` | `3c6349835b2b7b196a839186cb8b78e02f7b5f25` | `ci.yml:185` | 0 |
| `actions/upload-artifact` | `ea165f8d65b6e75b540449e92b4886f43607fa02` | `core-beta-evidence.yml:238`, `release.yml:96`, `release.yml:193`, `release.yml:259` | 1 |
| `actions/download-artifact` | `d3f86a106a0bac45b974a628896c90dbdf5c8093` | `release.yml:149`, `release.yml:214`, `release.yml:221`, `release.yml:275` | 0 |

Fixed-source action.yml reads also confirmed `dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c` uses `composite`, and `EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25` uses `docker` (official GitHub contents API, 2026-10-02); neither is a direct JavaScript runtime migration target.

The proposed first batch contains **11 replacements, four direct Actions, three workflow files**: checkout (5), setup-python (3), cache (2), upload-artifact (1). All release.yml use sites are deferred; download-artifact is release-only. dtolnay/rust-toolchain and EmbarkStudios/cargo-deny-action remain unchanged. Exact candidate versions/runtime provenance are owned by `action-runtime-upgrades.md`, not inferred from version comments here.

## Contract inventory

- `.github/workflows/ci.yml:3-7`: main push, pull_request, and workflow_call. Existing three-OS test and installer matrices plus cargo-deny expand to **7 jobs**. Keep every existing command, timeout, cache key/path, Python version, matrix label and implicit/explicit permission setting.
- `.github/workflows/security-audit.yml:3-18`: scheduled, pull_request and manual security audit; **1 job**, contents:read, existing tool pins. Its checkout explicitly has persist-credentials:false (`:21-23`); do not change it or widen permissions.
- `.github/workflows/core-beta-evidence.yml:3-28`: manual dispatch plus path-filtered PR/main-push events, contents:read. Keep both path filters unchanged; changing this workflow itself naturally triggers its **4 jobs**.
- The fixed evidence matrix (`core-beta-evidence.yml:43-66`) remains windows-2022 / Ubuntu 22.04 / macos-15-intel / macos-15, with the same target triples, binaries and RUSTFLAGS. MACOSX_DEPLOYMENT_TARGET stays 12.0. A hosted result is not a minimum-OS, glibc-floor, signing or notarization certificate.
- Evidence upload (`core-beta-evidence.yml:237-245`) keeps name `core-beta-${{ matrix.id }}-${{ github.run_id }}`, paths `evidence/ci/` and the target release binary, `if-no-files-found: error`, and `retention-days: 7`. A new uploader must be tested by inspecting the actual four uploaded artifacts, not only the green step badge.
- Existing local `scripts/release/test_release_workflow.py` checks nonempty workflow inventory, full 40-character Action SHA pins (local reusable workflow excepted), and explicit GH_REPO/--repo for gh steps. Baseline command `python -B -m unittest discover -s scripts/release -p test_release_workflow.py` passed **3/3** on 2026-10-02. No new Python/YAML dependency is proposed; an already-installed local YAML reader was used for inspection only.

## Release rehearsal prerequisite and narrowing decision

Authenticated `gh api repos/qin-devs/AgentSessions/tags?per_page=100` returned **[]** at the verification time above. There is currently no existing remote v* tag suitable for the existing manual release rehearsal.

`release.yml:3-12` requires an existing v* tag for workflow_dispatch. Its prepare step checks tag syntax, commit identity and package version. `release.yml:267-272` enables publish only for a push/tag event, with contents:write limited to that job. Creating a v* tag as an improvised test would trigger the real publication path and is outside this task's authority.

**Recommendation:** leave release.yml byte-identical in the first implementation batch. Do not create/move/delete tags, fabricate release inputs, weaken checks, change publication conditions, or dispatch an impossible rehearsal. Release-only JavaScript pins and a complete release rehearsal belong to a separately approved follow-up once safe inputs exist. The old release run history is historical and is not evidence of a new current-head result.

`release.yml:24` reuses ci.yml, so its quality workflow will indirectly consume any approved CI pin updates. This does **not** justify claiming that the complete release DAG was tested. The first batch's acceptance is the three named workflows, plus preservation of the unchanged release file and the existing workflow-contract suite.

## Hosted runner transition (official primary source)

Official issue: https://github.com/actions/runner-images/issues/14748
Official issue updated_at: `2026-09-28T19:20:56Z`; read with `gh api repos/actions/runner-images/issues/14748` on 2026-10-02.

The announced ubuntu-latest transition to Ubuntu 26.04 begins **2026-10-19**, with planned completion **2026-11-19**. The generic CI and security audit use floating labels; the release/evidence GNU build stays on Ubuntu 22.04. Preserve those choices in this batch. Do not pin to Ubuntu 24.04, add a new gate/matrix row or claim Ubuntu 26.04 execution without a separately approved probe and actual result.

The release/evidence scripts already state that Ubuntu 22.04 hosted execution does not certify the proposed glibc 2.31 floor. This migration task does not resolve or lower that separate external gate. Canonical boundaries remain in `.trellis/spec/agentsessions-cli/backend/index.md:200-231`, `docs/operations/core-beta-evidence-matrix.md`, and `docs/operations/external-readiness-gate.md`.

## Scope and validation consequences

- Proposed editable product-infrastructure paths: only the three workflow files above, and only the four approved Action references plus matching version comments. Task planning/evidence files are separate coordination artifacts.
- Fresh expected PR checks: 7 ci + 1 security-audit + 4 core-beta-evidence = **12**. Fresh main push: **7 ci + 4 core-beta-evidence**, on the actual merge SHA.
- Preserve job names/counts, triggers, paths, steps, permissions, inputs/outputs, cache and artifact settings by comparing the final diff to this baseline; a green YAML parse alone is insufficient.
- No cargo command is needed for this planning-only inspection. If implementation later runs any Cargo gate, use --offline (and --locked for dependency-resolving commands).
- Root worktree changes, previous completed tasks, stale relocation pointer, external Wake checkout and auxiliary baseline worktree are unrelated and must remain untouched.

## Planning validation receipt (2026-10-02)

The final planning inventory consists of eight task-local files. `task.py validate` passed both curated manifests (6 entries each); the existing workflow contract suite passed 3/3. A separate UTF-8/no-BOM, whitespace, local-path/credential-pattern and scope check passed for all eight untracked planning files. Candidate SHAs in design and official research agree; the current 11 target use sites were recounted. There are no tracked or staged modifications, no workflow edits, no implementation activation and no CI dispatch. A fresh fetch still resolves origin/main and the isolated branch HEAD to ffaf69b; repository visibility was read back PRIVATE. Task status remains planning with final owner review pending, and current session selection remains none.

## Coordinator receipt-method preparation (historical sample only)

Preparing future runner acceptance exposed two local extraction assumptions, not CI failures: gh run view rendered startup records as UNKNOWN STEP, and reparsing an automatically decoded PowerShell DateTime as text lost its UTC offset. Both unsuccessful extraction commands rejected their input; neither produced an accepted runner verdict.

The verified method reads the authenticated raw job log and JSON metadata, preserves ISO timestamps with ConvertFrom-Json -DateKind String, anchors the first raw runner-version record inside the successful setup step's UTC second-precision bounds, and compares its version with 2.327.1. A negative control reproduced the offset loss under default JSON date conversion; preserving the original string corrected it. Local parameter help and official PowerShell documentation (queried via Context7 in two calls) confirm this behavior. No shared helper or product code was changed.

Method-only sample: historical main run 36956529885, job 110680590532, runner 2.337.0 satisfied the minimum. This is not new-head evidence and does not complete AC1. Fresh PR/main jobs must supply their own raw startup receipts. The four new core-beta artifacts must likewise supply their own source/target identities, names/layout and binary hashes; prior run artifacts are only a layout/reference aid.
