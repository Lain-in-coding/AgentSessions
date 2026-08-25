# Owner Release Checklist — agent-session-grep v0.1.0

> Generated 2026-08-26 by the release-hardening waves (`main` at `4f4d379`).
> This document does **not** change any governance state; it compresses the
> remaining owner-only actions into exact steps. Every engineering gate the
> repository can verify locally is green:
>
> - `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` — all green
> - `python scripts/evidence/privacy_scan.py` — 0 findings
> - `python scripts/verify-release.py --asg target/release/asg.exe` — 10/10
> - `cargo deny check` — advisories/bans/licenses/sources all ok
>   (`cargo audit` cannot fetch the RustSec database from this network)
> - `scripts/release/export_public_tree.py` dry-run — all tracked files exported, no `.trellis/` records leak
>
> Full history of what landed: `CHANGELOG.md [Unreleased]` and the
> "Closed 2026-08-25/26" sections of
> `docs/release/go-no-go.2026-08-16.md`.

---

## 1. Unblock CI (root cause: account-level Actions billing)

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

Local status after the 08-26 waves:

| provider | property | read-only | golden | resume | main remaining local gaps |
|---|---|---|---|---|---|
| claude-code | ok | ok | ok | derived | richer tool-call extraction (optional; partial today) |
| codex | ok | ok | ok | derived | richer tool-call extraction (optional) |
| grok-build | ok | ok | ok | derived | tool_activity impossible to anchor (no per-message id) — record as exception if promoting |
| pi | ok | ok | ok | derived | tool_activity: format carries none — exception |
| openclaw | ok | ok | ok | unsupported (intentional) | exception already reasoned in the ledger |
| tencent-codebuddy | ok | ok | ok | derived | extension variant pending |
| kimi-code | ok | ok | ok | derived | loop events not parsed |
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

After steps 1–4:

```powershell
git tag -a v0.1.0 -m 'agent-session-grep v0.1.0'
git push origin v0.1.0
gh release create v0.1.0 --draft --title 'agent-session-grep v0.1.0' --notes-from-tag
```

Then attach the CI-built archives (or the local release build) and publish.
The `[Unreleased]` CHANGELOG section becomes the v0.1.0 release notes; bump the
CHANGELOG header after tagging.

---

**Order that unblocks everything:** 2 (public, also fixes CI) → 3 (ADR-0010
first) → 4 (record CI run ids + promotions) → 7 (tag + release). Steps 5 and 6
are explicitly optional for 0.1.0.
