# Owner Release Checklist — agent-session-grep v0.1.0

> **Historical snapshot (2026-08-29, `main` at `077e529`).** The 2026-10-06
> refresh below supersedes any status line that now disagrees with it; this
> block is kept as the record of that wave.
>
> Generated 2026-08-26 by the release-hardening waves; every gate below
> re-verified 2026-08-29 against `main` at `077e529`.
> This document does **not** change any governance state; it compresses the
> remaining owner-only actions into exact steps. Every engineering gate the
> repository can verify locally is green:
>
> - `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` — all green (1557 passed, 0 failed, 16 ignored, 78 suites)
> - `python scripts/evidence/privacy_scan.py` — 0 findings
> - `python scripts/verify-release.py --asg target/release/asg.exe` — 10/10
> - both shipped command names agree: `asg` and `agent-session-grep` return
>   byte-identical `--version` / `--help` / `doctor` output, because the CLI
>   lives in `crates/agent-session-grep-cli/src/lib.rs` and both bin targets
>   are three-line shims calling `run_cli()`
> - `cargo deny check` — advisories/bans/licenses/sources all ok
>   (`cargo audit` still cannot fetch the RustSec database from this network:
>   the `advisory-db.git` clone aborts mid-pack)
> - `scripts/release/export_public_tree.py --destination <empty dir>` — 504
>   tracked files exported from `077e529`, no `.trellis/` (or other internal
>   working-record) path leaks
>
> Full history of what landed: `CHANGELOG.md [Unreleased]` and the
> "Closed 2026-08-25/26" sections of
> `docs/release/go-no-go.2026-08-16.md`.

---

## 2026-10-06 status refresh (B6 release closure)

Verified live on 2026-10-06 for commit `1d65a27`:

- **CI is running again and green.** `gh run list` shows completed runs for
  `main` and for `fix/session-relocation-identity`; the Actions outage and the
  billing suspicion in §1 are historical, not current. Re-check before
  repeating either claim.
- **The installer path has named CI evidence.** Run `36391073119`
  (2026-09-28, pull request, head `2b8f895`) finished `success` for every job:
  `test` and `installer smoke` on `ubuntu-latest`, `windows-latest`, and
  `macos-latest`, plus `cargo-deny`. That is `ci_verified` hosted-runner script
  evidence, not clean-machine, minimum-OS, or release certification.
- **Windows install → first run → upgrade → uninstall ran locally.** Build
  `4e009759864a4c4680a39731a5ad15afbbd6fe5c50818577bb082631d43d4442`
  (6,929,408 bytes, cargo/rustc 1.97.1): install exit 0 with both command names
  (`agent-session-grep.exe`, `asg.exe`) byte-identical; first run
  (`--version`, `--robot config paths` under a sandboxed profile, `doctor`);
  in-place upgrade over a synthetic 0.1.1 successor build; uninstall removing
  exactly the two managed files; a second uninstall a no-op. Logs and
  `summary.json` under
  `.trellis/tasks/10-06-release-closure/research/windows-smoke/`.
- **`release-verify.yml` was added (definition only).** It exercises the same
  locked/no-default-features/target-pinned build, the synthetic release smoke,
  packaging, and `SHA256SUMS` on one runner per OS family, uploads the results
  as workflow artifacts, and has no publish path and no tag trigger. It has not
  run on GitHub yet: `ci_configured_only` until a run id is recorded.
- **The release chain has never produced an artifact or a Release.** No `v*`
  tag and no GitHub Release exists (checked through the GitHub API on
  2026-10-06); every run ever attributed to `release.yml` is a zero-job startup
  failure from the 2026-08-17 Actions outage, and it only triggers on a tag
  push or a manual dispatch naming an existing tag.
- **Still unsigned, unnotarized, unpublished.** This refresh changed nothing
  about that.
- **Metric wording.** The Windows smoke is installer evidence only. It adds no
  native-resume, evidence-pack-validity, or task-completion evidence; the
  `resume: derived` rows in §4 still mean a derived CLI preview, not an
  executed native resume.

Remaining owner-only actions, in the order that unblocks the release:

1. Accept the governance documents (§3, ADR-0010 first) and decide the release,
   then push `v0.1.0`. Pushing the tag is the publish action; `release.yml`
   then builds the four unsigned targets, writes `SHA256SUMS`, and creates the
   unsigned GitHub Release.
2. Record which history-scrub option published the repository
   (`PUBLIC-HISTORY-SCRUB.md`). The repository is public; that decision record
   was not verified by this task.
3. Signing / notarization credentials and store channels (§6) — still yours.
4. Provider Beta promotions (§4) with named CI run ids; the anchors above are
   installer/CI evidence, not provider-level evidence.
5. Optional: dispatch `release-verify.yml` once and record the run id so its
   evidence stops being definition-only.


## 1. Unblock CI (historical: account-level Actions billing)

**Resolved (verified 2026-10-06).** CI runs and is green; see the refresh
above. Keep the diagnosis below only for the case where scheduled runs stop
starting again with an empty `runner_name`.

Diagnosis (verified 2026-08-26 via `gh api`):

- Repo Actions settings are normal: `{"enabled":true,"allowed_actions":"all"}`.
- Every job of every run fails 4–7 s after start with `runner_name: ""` and
  `steps: 0` — no runner is ever assigned, even for a docs-only commit.
- This signature matches the GitHub community's documented "hidden non-payment"
  case (discussion #164954) and the billing doc: private repos consume the
  account's Actions minutes allowance; anything beyond it is billed, and a
  failed payment silently stops runner assignment.

Owner steps (either one fixes it):

- **Option A (recommended, aligns with open-sourcing):** make the repository
  public first (step 2) — public repositories get unlimited free Actions
  minutes.
- **Option B:** open `Settings → Billing & plans` on your account, check
  Actions usage and payment method, fix the payment or raise the spending
  limit, then re-run any workflow.

After either option, the 4-target `ci.yml` matrix (Windows MSVC / Ubuntu /
macOS Intel / macOS ARM) plus `core-beta-evidence` and `security-audit` should
run. The first named green run unblocks provider Beta promotion (step 4) and
lets `last_certified_targets` be recorded.

## 2. Make the repository public

**Done (verified 2026-10-06: repository visibility is `public`).**

History-scrub decision first: read `docs/operations/PUBLIC-HISTORY-SCRUB.md`
and decide whether to rewrite history (Option B) or publish the current tree
via `scripts/release/export_public_tree.py` into a fresh repository
(Option A, already dry-run verified). Then:

```powershell
gh repo edit qin-devs/AgentSessions --visibility public
```

## 3. Sign the governance documents

| Document | Current | Action |
|---|---|---|
| `docs/adr/ADR-0001` .. `ADR-0010` | Proposed | Set `Status: Accepted`, record date + approver. ADR-0010 (provider maturity rollback policy) is a hard prerequisite for any Beta promotion. |
| `docs/release/go-no-go.2026-08-16.md` §9 | No-Go (draft) | Re-evaluate against the closed waves listed in that file, record decision + signature. |
| REUSE reuse-matrix / SBOM | Draft | Approve or scope to a later release; `NOTICE` already ships in every archive. |
| THREAT-MODEL open items | 2 pending | Decide: privacy mode semantics; network-filesystem rejection policy. |

## 4. Promote providers to Beta (after steps 1–3)

Rule (from `docs/product/PROVIDER-BETA-READINESS.md`): a provider is Beta only
when every local column is `ok` (or an exception is recorded), a named
cross-target CI success exists, ADR-0010 is Accepted, and the owner records
the decision with evidence paths.

Local status after the 08-29 waves — the binding per-row reasons live in the
ledger (`docs/product/PROVIDER-BETA-READINESS.md`), not here:

| provider | property | read-only | golden | resume | main remaining local gaps |
|---|---|---|---|---|---|
| claude-code | ok | ok | ok | derived | richer tool-call extraction (optional; partial today) |
| codex | ok | ok | ok | derived | richer tool-call extraction (optional) |
| grok-build | ok | ok | ok | derived | tool_activity impossible to anchor (no per-message id) — record as exception if promoting |
| pi | ok | ok | ok | derived | format does carry `toolCall`/`toolResult`, but its record ids are 8-hex and file-scoped; `context`/`tool_activity` wait on document-scoped message identity — recorded decision, not pending work |
| openclaw | ok | ok | ok | unsupported (intentional) | exception already reasoned in the ledger |
| tencent-codebuddy | ok | ok | ok | derived | extension surface stays unimplemented for lack of format evidence — recorded decision, not pending work |
| kimi-code | ok | ok | ok | derived | user prompts (`turn.prompt` / `turn.steer`) now indexed; loop events stay unparsed because they are tool activity with no per-message anchor — recorded decision |
| opencode | ok | ok | ok | derived | source_span unsupported by construction (SQLite) — exception |
| antigravity | ok | ok | ok | derived | no in-file session id |
| qoder | ok | ok | ok | unknown | no resume evidence |
| hermes | ok | ok | ok | unknown | span unsupported (JSON doc); resume evidence conflicting |
| cursor / cline / aider | ok | ok | ok | unknown/unsupported | several structural exceptions (no discovery root, no session id) |

For each promotion: update `crates/agent-session-grep-ports/src/capability.rs`
(maturity field), the maturity matrix doc, and the ledger — the drift guards
(`crates/agent-session-grep-cli/tests/provider_matrix.rs`) will verify the
documents stay consistent.

## 5. Semantic model weights (explicitly deferred)

The `semantic-candle` runtime, `model import`, and `model status` are
implemented; default builds stay honestly labeled bigram-hash/lexical.
Deliverables if/when you want the semantic path enabled: the E5 bundle
(`intfloat-multilingual-e5-small`, 384-dim, per
`docs/operations/SEMANTIC-MODEL-BUNDLE.md`) with SHA-256 manifest, plus a
recall benchmark design. This is not required for the 0.1.0 release.

## 6. Signing / notarization / channels (needs your credentials)

Windows Authenticode certificate, macOS Developer ID + notarytool, cosign
keypair, crates.io token, Homebrew/Scoop credentials. All are post-release
promotion work; none blocks the initial public release.

## 7. Tag and release

After step 3, the tag itself runs `.github/workflows/release.yml`:

```powershell
git tag -a v0.1.0 -m 'agent-session-grep v0.1.0'
git push origin v0.1.0
```

Pushing the tag is the publish action. The workflow builds the four targets
(Windows MSVC x64 with static CRT, Linux GNU x64, macOS Intel x64, macOS
ARM64) with `--locked --release --no-default-features --target <triple>`, runs
the synthetic release smoke against each built binary, writes `SHA256SUMS`
plus per-target provenance manifests, and creates or updates an **unsigned**
GitHub Release with those assets (`--verify-tag` ties the assets to your tag).
Do not hand-create the Release first.

To inspect artifacts before any Release exists, dispatch `release-verify.yml`
(workflow artifacts, 30-day retention) or run the local smoke described in
`docs/operations/INSTALL-AND-UPGRADE.md`.

The `[Unreleased]` CHANGELOG section becomes the v0.1.0 release notes; bump the
CHANGELOG header after tagging.

---

**Order that unblocks everything:** 2 (public — done 2026-10-06) → 3
(ADR-0010 first) → 4 (record CI run ids + promotions) → 7 (tag and release,
which now runs the release workflow for you). Steps 5 and 6 are explicitly
optional for 0.1.0.
