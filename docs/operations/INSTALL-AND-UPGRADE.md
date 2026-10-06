# Install and upgrade (from source)

Scope: building `agent-session-grep` from a checkout of this repository and placing
both the canonical command and the `asg` alias in a user-level directory. There
is no released, signed, or published artifact — see
[What this does not give you](#what-this-does-not-give-you).

## Verified locally (Windows, 2026-10-06)

Evidence labels follow the vocabulary in `core-beta-evidence-matrix.md`.

| Item | Result |
|---|---|
| Commit built | `1d65a27` (`fix/session-relocation-identity`), built in a detached worktree so concurrent edits could not change the artifact mid-run |
| Toolchain / host | cargo and rustc 1.97.1, PowerShell 7.6.1, Windows 10.0.22631 x64 |
| Build | `cargo build --release --locked -p agent-session-grep-cli --bin agent-session-grep` (exit 0) |
| Artifact | `agent-session-grep.exe`, 6,929,408 bytes, SHA-256 `4e009759864a4c4680a39731a5ad15afbbd6fe5c50818577bb082631d43d4442` |
| Install | `pwsh -File scripts/install/install.ps1 -Prefix <dir>` (exit 0); both command names written, `asg.exe` byte-identical to the canonical file, hash equal to the artifact hash |
| First run | `--version` on both names reports `agent-session-grep-cli 0.1.0`; `--robot config paths` resolves config/data/cache/logs under the sandboxed profile; `doctor` reports `db: not-checked` (environment only, no data root touched) |
| Upgrade | the same installer re-run over the existing install replaced both managed files in place; both names then reported `agent-session-grep-cli 0.1.1`; no temporary files were left behind. The successor was a real second build (workspace version bumped to 0.1.1 in a scratch worktree, never committed) because the repository has only one committed version |
| Uninstall | removed exactly the two managed command files, left the install directory in place, did not touch the sandboxed config/data/cache/logs; a second run printed `not installed` and exited 0 |
| Surface smoke | `scripts/install/smoke.ps1` against the installed pair (robot envelopes, documented exit codes, MCP stdio handshake, relocation and provider filters): all assertions passed |
| Raw evidence | `.trellis/tasks/10-06-release-closure/research/windows-smoke/` (`summary.json`, per-step logs, reusable `run-windows-smoke.ps1`) |

That run establishes installer/upgrade/uninstall mechanics on one Windows
machine that already had a Rust toolchain. It is not clean-machine evidence,
not a second-release compatibility test, and not signed-artifact evidence.

## CI: what is defined and what has actually run

- `ci.yml` — three-OS (`ubuntu-latest`, `windows-latest`, `macos-latest`)
  format/clippy/test jobs plus an installer/uninstaller/surface smoke job.
  **Executed and green**: run `36391073119` (2026-09-28, pull request, head
  `2b8f895`) completed with every job `success`, including `test` and
  `installer smoke` on all three operating systems. That is `ci_verified`
  script evidence on hosted runners, not clean-machine certification. The
  earlier CI outage documented in older release notes is not current: the
  repository is public and Actions runs (re-checked 2026-10-06).
- `release.yml` — the tag-driven chain: provenance validation, four targets
  across three OS families, unsigned archives plus `SHA256SUMS`, and an
  unsigned GitHub Release on a tag push only. **No `v*` tag exists, so this
  workflow has never built or published anything**; every run ever attributed
  to it is a zero-job startup failure from the 2026-08-17 Actions outage, and
  nothing in it is triggered by branches or pull requests.
- `release-verify.yml` (added 2026-10-06) — `ci_configured_only`: on demand,
  pull requests that touch release inputs, or a push to `main`, it runs the
  same `--locked --no-default-features --target` build on one runner per OS
  family, runs the synthetic release smoke against the built binary, packages
  unsigned archives with `scripts/release/build-manifest.py`, writes and
  re-verifies `SHA256SUMS`, and uploads them as **GitHub Actions artifacts**
  (30-day retention). It has no publish step, no tag trigger, and
  `contents: read` only. Until it runs on GitHub its evidence is a definition,
  not a result.

## What the installer does

`scripts/install/install.ps1` (Windows) and `scripts/install/install.sh`
(Linux, macOS) perform exactly these steps:

1. Resolve the repository root from the script location and confirm
   `Cargo.toml` is present.
2. Check that `cargo` is on `PATH`; if not, print the rustup.rs pointer and
   exit non-zero.
3. `cargo build --locked --release -p agent-session-grep-cli`.
4. Compute the SHA-256 of the built binary, create the destination directory
   if needed, and atomically replace the canonical command.
5. Install `asg.exe` as a second executable copy on Windows. On Unix, install
   `asg` as a relative symlink to `agent-session-grep`, with a marked wrapper
   fallback when symlinks are unavailable. An unrelated existing alias is never
   overwritten.
6. Run both installed command names with `--version` and require identical
   output.
7. Print both install paths, the SHA-256, the version, and a one-line
   instruction for adding the directory to `PATH`.

Any failing step exits non-zero with a diagnostic. The script does not modify
`PATH`, write to the registry, edit shell profiles, request elevation, or
download anything beyond what `cargo build` itself fetches.

## Default install location

| Platform | Destination |
|---|---|
| Windows | `%LOCALAPPDATA%\agent-session-grep\bin` |
| Linux, macOS | `${XDG_BIN_HOME:-$HOME/.local/bin}` |

This is deliberately separate from the config/data/cache/logs directories the
CLI itself reports. To see those, run:

```
agent-session-grep --robot config paths
```

## Install

Windows (PowerShell 7+):

```powershell
pwsh -File scripts/install/install.ps1
```

Linux, macOS:

```sh
bash scripts/install/install.sh
```

`install.sh` requires Bash (it uses `set -o pipefail`); invoking it as
`sh scripts/install/install.sh` works on macOS but fails on Debian/Ubuntu
where `sh` is dash.

Both scripts accept:

| Windows | Unix | Meaning |
|---|---|---|
| `-Prefix <path>` | `--prefix <path>` | Install to this directory instead of the default. |
| `-SkipBuild` | `--skip-build` | Skip the build and copy the existing `target/release` binary. Fails if it is absent. |
| `-DryRun` | `--dry-run` | Print the planned actions and write nothing. |

## Add the directory to PATH

The installer prints the exact line for your platform; it never applies it.
Apply it yourself, in the shell configuration you actually use.

Windows, current user, persistent:

```powershell
[Environment]::SetEnvironmentVariable(
  'Path',
  [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + "$env:LOCALAPPDATA\agent-session-grep\bin",
  'User')
```

bash or zsh:

```sh
export PATH="$HOME/.local/bin:$PATH"   # add to ~/.bashrc or ~/.zshrc
```

## Verify the install

```
agent-session-grep --version
asg --version
agent-session-grep doctor
agent-session-grep --robot config paths
```

`doctor` without `--db` checks only the environment: it reports the tool name and version, plus `db: not-checked` / `schema: null`, and prints an actionable hint pointing at `doctor --db <path>`. Only the db/schema validation is skipped; no data root is touched. To also check that a store opens, pass a path:

```
agent-session-grep --robot doctor --db C:/data/example.db
```

That form reports `schema`, `generation`, and `interrupted_batches` for the
store. See `rebuild-and-migration-runbook.md` for how to read those fields.

When loading transcripts, `ingest <file>` and each input passed to
`sync <file>...` treat one file as one session source. A transcript file should
therefore contain a single distinct `sessionId`. If several are detected, the
file remains assigned to the first session and the command returns a bounded
warning listing the detected session count and IDs.

## Upgrade

Pull the new commit and re-run the installer. It upgrades both managed command
files in place; there is no version pinning, rollback, or update channel. Running
the installer again over an existing install is supported and preserves the
canonical/alias relationship.

```
git pull
pwsh -File scripts/install/install.ps1        # or bash scripts/install/install.sh
agent-session-grep --version
asg --version
```

Measured 2026-10-06 (see the evidence table above): re-running the installer
over an existing install replaced both managed files in place and the version
output followed the new artifact. There is still no second released version to
upgrade across, so this proves the replacement mechanics, not compatibility
between two real releases.

An upgraded binary may need to migrate an existing data root on first open.
Migration is automatic, transactional, and stepwise; the current store schema
is v17, whose last step records the store-level index-projection version. The
per-step list is not restated here — `rebuild-and-migration-runbook.md` owns it,
and the v5 → v6 and v6 → v7 steps in `migration-v5-to-v6.md` and
`migration-v6-to-v7.md` are historical. An older binary refuses to open a newer
store with `schema_incompatible` (exit 9) rather than downgrading it.

## Uninstall

```powershell
pwsh -File scripts/install/uninstall.ps1
```

```sh
sh scripts/install/uninstall.sh
```

Uninstall deletes only the two managed command files (`agent-session-grep.exe`
and `asg.exe` on Windows; `agent-session-grep` and `asg` on Unix) from the
install directory. It never removes a directory recursively and never touches
your data root. Running it when neither command is installed reports "not
installed" and exits 0, so it is safe to repeat. If `asg` no longer matches the
managed copy/link, uninstall refuses to remove it.

Your config, data, cache, and logs survive uninstall. To remove them, delete
the paths reported by `agent-session-grep --robot config paths` yourself — the
scripts will not do it for you.

## Common failures

**`cargo` not found.** Install a Rust toolchain from rustup.rs, open a new
shell so `PATH` picks it up, and re-run.

**Build fails.** The failure is a `cargo build` failure, not an installer
failure. Re-run `cargo build --locked --release -p agent-session-grep-cli`
directly and read its output. `--locked` means a lockfile that disagrees with
`Cargo.toml` is an error rather than being silently updated.

**Destination not writable.** Pick a directory you own with `-Prefix` /
`--prefix`. Do not run the installer elevated to work around this.

**`agent-session-grep` or `asg` not found after install.** The installer does not
change `PATH`. Either apply the printed `PATH` line or invoke the command by its
full path.

**`asg` already exists in a custom prefix.** The installer refuses to overwrite
an alias it cannot identify as its own. Choose another prefix or move the
unrelated file aside, then re-run the installer.

**Windows blocks or removes the binary.** Endpoint protection can quarantine
freshly built, unsigned executables. The binary is unsigned by design (see
below); resolving that is a decision for whoever administers the machine.

## What this does not give you

Stated plainly, because it is easy to assume otherwise:

- **Not signed, not notarized.** The binary carries no Authenticode signature
  and no Apple notarization ticket. Both are blocked on credentials this
  project does not have (`CB-SIGNING-001`, `CB-NOTARIZATION-001` in
  `core-beta-evidence-matrix.md`).
- **Not published.** No release tag, no GitHub Release, no crates.io
  package, no Homebrew, winget, or scoop manifest, no container image (checked
  through the GitHub API on 2026-10-06: zero tags, zero releases). Building
  from source is the only supported path; the artifacts `release-verify.yml`
  uploads are workflow artifacts, not downloads you should cite as a release.
- **Not clean-machine certified.** CI runs the installer scripts on
  GitHub-hosted runners whose images already ship a Rust toolchain (green run
  `36391073119`, 2026-09-28), and the 2026-10-06 Windows smoke ran on a
  developer machine that also had a toolchain. Both are installer-script smoke
  evidence: the scripts run and the installed binary executes. Neither is
  evidence that installation works on a machine without a toolchain.
- **No minimum-OS certification.** The OS versions used in CI are the runner
  images, not a certified floor. See `external-readiness-gate.md` for the
  full list of externally blocked release requirements.
