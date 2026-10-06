# B6 release-closure evidence index

Task: `.trellis/tasks/10-06-release-closure` (parent
`10-05-competitive-source-audit-plan`, workstream B6).
Owner boundary honored: nothing was published, signed, tagged, or pushed by
this task.

## What is actually verified

| Evidence | Status | Where |
|---|---|---|
| Release build from committed HEAD `1d65a27` (`cargo build --release --locked -p agent-session-grep-cli --bin agent-session-grep`) | executed, exit 0, artifact SHA-256 `4e009759864a4c4680a39731a5ad15afbbd6fe5c50818577bb082631d43d4442`, 6,929,408 bytes | `windows-smoke/logs/01-release-build.log` |
| Windows clean install (`install.ps1 -Prefix`), both command names, alias byte-identical to the canonical file | executed, exit 0 | `windows-smoke/logs/02-install-first.log`, assertions in `windows-smoke/summary.json` |
| First run: `--version` (both names), `--robot config paths` in a sandboxed profile, `doctor` (environment only) | executed, exit 0; sandbox config/data/cache/logs were reported but not created by these read-only commands | `windows-smoke/logs/03..05` |
| Surface smoke against the installed pair (robot envelopes, documented exit codes, MCP stdio handshake, relocation, provider filters) | executed, all assertions passed | `windows-smoke/logs/06-surface-smoke.log` |
| Upgrade: installer re-run over the existing install with a real successor build (synthetic workspace version 0.1.1 in a scratch worktree, never committed) | executed, exit 0; both files replaced in place, both names then reported 0.1.1, no temp files left | `windows-smoke/logs/07-upgrade-build.log`, `08-upgrade-install.log` |
| Uninstall: removes exactly `agent-session-grep.exe` + `asg.exe`, keeps the directory, leaves config/data/cache/logs alone; second run is a no-op exit 0 | executed, exit 0 | `windows-smoke/logs/09..10`, `windows-smoke/summary.json` |
| Whole workspace test suite on the same committed HEAD | executed, exit 0: 87 suites, 1788 passed, 0 failed, 20 ignored | `verification/01-cargo-test-workspace.log` |
| Workflow syntax + action pinning + permissions + matrix/flag contract | executed: PyYAML parse of all 5 workflow files and 12 contract tests pass | `workflow-validation/01-validate-workflows.log`, `scripts/release/validate_workflows.py` |
| The exact CI build line (`cargo build --locked --release --no-default-features --target x86_64-pc-windows-msvc -p agent-session-grep-cli --bin agent-session-grep`) | executed on the clean worktree, exit 0; artifact SHA-256 `92e13fe8c2b28ec25c58db83a7865da6c119d97658701527e58e9b5a2a6c20d1` (a `--target` build hashes differently from the installer's host build `4e009759…d4442`; both report `agent-session-grep-cli 0.1.0`) | `verification/02-release-build-explicit-target.log` |
| CI status snapshot | read live 2026-10-06 through `gh`: repository public; runs green on `main` and on this branch's last PR (run `36391073119`, all `test`/`installer smoke` jobs on three OS + `cargo-deny`); zero tags, zero releases | documented in the two updated docs; raw queries are in `windows-smoke/logs/99-script-stdout.log` only for the smoke, so re-run `gh run list` to reconfirm |

## What is definition-only (not executed)

| Definition | Status | Caveat |
|---|---|---|
| `.github/workflows/release.yml` (tag-driven release: 4 targets, `SHA256SUMS`, unsigned GitHub Release) | `ci_configured_only` | no `v*` tag exists, so it has never built or published anything (the only runs attributed to it are zero-job startup failures from the 2026-08-17 Actions outage); the tag push would be the owner's publish action |
| `.github/workflows/release-verify.yml` (added 2026-10-06) | `ci_configured_only` | never dispatched; produces workflow artifacts only (30-day retention), cannot publish, does not trigger on tags |
| Linux/macOS install/upgrade/uninstall on a real machine | not executed | only the hosted-runner installer smoke from run `36391073119` exists (`ci_verified`); no local Linux/macOS host was used |
| Upgrade across two real releases | not executed | the probe used a synthetic 0.1.1 successor because the repository has one committed version (0.1.0) |
| Signing, notarization, crates.io/Homebrew/winget/scoop, GitHub Release | not executed, owner-only | unchanged by this task |

## Reproducing

```powershell
# workflow validation (PyYAML parse + contract tests)
python scripts/release/validate_workflows.py

# Windows install/upgrade/uninstall smoke (builds in a scratch worktree)
pwsh -File .trellis/tasks/10-06-release-closure/research/windows-smoke/run-windows-smoke.ps1

# workspace test suite in the same detached worktree
git -C C:/AgentSessions-worktrees/b6-release-closure rev-parse HEAD
```

## Notes and anomalies

- The first release build attempt ran inside the shared main checkout while a
  concurrent workstream (B4) was mid-edit in `crates/`; a follow-up probe in
  that checkout failed to compile that in-flight state. Neither is evidence
  about the release path, so both are kept for the record under
  `windows-smoke/logs/00-superseded-main-checkout-build.log`, and all evidence
  above was produced from a clean detached worktree at HEAD.
- `summary.json` records every step's exit code and every assertion; the
  successor hash changes between runs because the synthetic worktree is rebuilt
  (the committed artifact hash `4e009759…d4442` is the stable one).
- The main checkout still contained uncommitted B4 edits when this report was
  written; nothing in `crates/` was modified or reverted by B6.

## Scratch state left behind (optional cleanup)

- `C:/AgentSessions-worktrees/b6-release-closure` — clean detached worktree at
  `1d65a27`; held the build, test suite, and installer smoke.
- `C:/AgentSessions-worktrees/b6-release-successor` — synthetic 0.1.1
  successor worktree (uncommitted version bump); the smoke script recreates it
  on every run.
- `.trellis/.runtime/b6-smoke/` — install prefix (left empty by uninstall) and
  the sandboxed profile used for first-run commands.
- `.trellis/.runtime/target-b6/` — target dir of the first, superseded build
  attempt (kept only because its logs are referenced).

Cleanup, after verifying each resolved path:
`git worktree remove --force <worktree>` and
`Remove-Item -LiteralPath <dir> -Recurse -Force`.
