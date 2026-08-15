# GitHub release, CLI, MCP, and Skill readiness — 2026-08-13

## Executive assessment

Protocol behavior is approaching beta, but distribution is still prerelease. The repository can become a source-only developer preview after public-entry and license work; it should not yet claim supported, signed, prebuilt Windows/macOS/Linux releases.

This report separates official standards from project recommendations. Sources were checked on 2026-08-13; key official URLs are listed at the end.

## P0 — blockers before a public beta claim

### Repository and legal identity

- Add a real root README and owner-selected OSI-approved license texts. `Cargo.toml` declaring `MIT OR Apache-2.0` does not replace the license files.
- Add `SECURITY.md`, human-readable release notes/changelog policy, issue/PR templates, and privacy-safe reporting guidance.
- Reconcile product/version identity. The binary is `agent-session-grep`, while some version output and documents use crate names or different planned versions.
- Audit complete Git history and every outward artifact: refs, LFS, submodules, releases, wiki, issues/PRs, Actions logs/artifacts/caches, personal paths/hostnames/emails, real transcripts, secrets, third-party code, and vendored binaries. Revoke/rotate secrets before history cleanup.

### Branch and PR governance

- Keep `main` as default; use short-lived feature/fix branches and PRs.
- Protect `main` with a GitHub ruleset: required CI, resolved conversations, linear history, no force-push/delete. A single-maintainer project should avoid a mandatory independent approval that self-locks the repository; enable one independent approval when a second maintainer exists.
- Enable squash merge only and use the PR title as the final Conventional Commit. Delete merged branches. Do not enable merge queue at low traffic.
- Git defines commit author/committer, `-S` cryptographic signatures, and `-s` signoff trailers; it does not define Conventional Commits or atomic commits. `Signed-off-by` (DCO) is not a verified GPG/SSH/S/MIME signature.

### MCP legacy contract

The current server may accurately claim legacy `2024-11-05`, `2025-03-26`, and `2025-06-18` negotiation only. It must not claim the current 2026 protocol era without `server/discover`, stateless per-request metadata negotiation, and the current error/result model.

Before legacy beta:

- Validate `jsonrpc == "2.0"`; reject request `id: null`; validate required initialize fields (`protocolVersion`, `capabilities`, `clientInfo`) and `tools/list` params.
- Publish a real `outputSchema` per MCP tool. A tool name is already the discriminator; do not merge all tools into one command tool. Each schema must cover both success structured content and the existing structured business-error shape.
- Keep `content` plus `structuredContent` for compatibility, but enforce budgets against the complete serialized MCP frame. Duplicating the same payload in both representations can approach twice the application payload budget.
- Bound input frame, query, cursor, array, numeric parameter, and response sizes. Do not claim cancellation support while the synchronous loop cannot observe cancellation during work.
- Make `list_sessions` return sessions only.
- Test with the official Inspector in the claimed legacy era and at least two real MCP hosts.

### Robot and response budget contract

- JSON Schema `$defs` do nothing unless reached by `$ref` or another applicator. The integrated change now binds `command == "search"` to `searchData -> hits[] -> searchHit`, so guidance limits are no longer an orphan definition.
- Add full Draft 2020-12 positive/negative schema validation in CI. The local Rust drift test checks the reference chain but is not a standards-complete validator.
- `max_response_bytes` is documented as a hard limit on final serialized bytes. Every Robot and MCP path must test final frames, including oversized anchors, summaries/hints, JSON escaping, page metadata, and the MCP duplicate text representation.

### CLI correctness and discoverability

- Fix public help/examples so multi-word queries are quoted or formally support operand joining. Add `--` end-of-options so literal flag-looking queries are unambiguous.
- Revisit exit 10 for ordinary pagination/budget truncation: useful first pages are normally success in shell conventions. If retained, document it prominently with `set -e` examples.
- Bound `get/show` output and sanitize every human stdout/stderr path against terminal control/OSC injection; Robot JSON escaping is not a substitute for terminal-safe rendering.
- Generate one request ID per process request and reuse it for progress plus final frames.
- Be explicit about configuration truth: `config paths` currently reports locations; it is not a config loader.

## P1 — supported distribution quality

### Release pipeline

- Use SemVer and signed annotated `vX.Y.Z` tags. `v` is a Git tag convention, not part of the SemVer version string.
- Build release artifacts from the tag in CI. Produce target/version-named archives containing README and licenses, final-byte SHA-256 sums, SBOM, and provenance attestations.
- Publish as a draft only after all assets are assembled and verified; then enable immutable GitHub Releases.
- Pin third-party Actions to full commit SHAs; default workflow permissions to `contents: read` and elevate per job using least privilege/OIDC.
- Enable Dependabot, dependency graph/review, secret scanning/push protection, and Rust CodeQL.

### Platform claims

- Rust Tier 1 means builds and tests; Tier 2 generally means builds without runtime testing. Project minimum OS/glibc claims require running the final artifact in those minimum environments, not only on newer hosted runners.
- A supported public binary release should use Authenticode plus timestamp on Windows and Developer ID, hardened runtime, notarization, and stapling on macOS. Otherwise label artifacts unsigned/unnotarized developer previews and document Gatekeeper/SmartScreen friction.
- Prefer Windows x64, Linux GNU x64, Linux musl x64, macOS x64, and macOS ARM64 only after artifact smoke tests. Add ARM64 targets after real runtime verification.
- Replace in-place installer overwrites with same-directory staging, checksum/self-check, and atomic replacement. Failure must preserve the old binary. Verify publisher checksum/signature before activation.
- Correct any documentation that invokes a Bash-only installer via generic `sh`.

### CLI UX

- Consider migrating the hand-written parser to `clap` to centralize help, value validation, `--`, suggestions, and completions. This is a later refactor, not required by POSIX.
- Keep machine results on stdout and diagnostics on stderr. Show progress only when stderr is a TTY; disable it in pipes, CI, Robot, and JSONL unless using structured progress frames.
- For TUI, require both stdin/stdout TTY and test resize, small windows, CJK display width, combining/emoji, long unbroken fields, and restoration after signals/errors.
- Define language policy: English default with Chinese documentation, or explicitly Chinese-first. Machine codes/schemas remain language-neutral and messages are never parsing interfaces.

### Agent Skill packaging

Agent Skills are an open directory format, not an execution engine. MCP/CLI provide capability; the Skill teaches when and how to use it.

- Maintain one portable truth source at `.agents/skills/agent-session-grep/` with `SKILL.md` and references. Point a Claude plugin marketplace manifest at it rather than duplicating three skill directories.
- First release is read-only search/retrieval guidance. Do not bundle hooks, silent MCP configuration edits, dependency installation, source sync/indexing, or background service startup.
- Treat transcript content as untrusted data, never instructions. Search once with a literal query, optionally retry one keyword variation, then report no evidence rather than inventing history.
- Keep plugin, binary, MCP registration, and user data as separate lifecycles. Uninstalling one must not delete the others.
- Lock the Agent Skills validator version or specification commit; the open spec currently lacks a stable release/tag and validator command documentation has drifted.
- Evaluate positive triggers, near-miss negatives, prompt injection, truncation, missing MCP, repeated runs, token/time cost, and with-Skill versus without-Skill behavior.

## P2 — later hardening and ecosystem promotion

- DCO 1.1 status check for external contributions; use a lawyer-reviewed CLA only if a legal entity needs additional relicensing/patent rights. Consider required signed commits later because it raises contributor friction; sign maintainer release tags now.
- Homebrew tap/formula and WinGet manifests only after GitHub Release is the tested source of truth.
- crates.io/cargo-install only after package metadata, licenses, and workspace path dependencies support publishing.
- Add man pages, shell completions, universal2, additional architectures, and optional pager/plain-input paths without breaking Robot schema.
- Current-era MCP dual support, Registry metadata, true cancellation, and optional progress after legacy beta is stable.

## Official sources

- GitHub Flow: https://docs.github.com/en/get-started/using-github/github-flow
- GitHub rulesets: https://docs.github.com/en/repositories/configuring-branches-and-merges/managing-rulesets/about-rulesets
- GitHub secure use reference: https://docs.github.com/en/actions/reference/secure-use-reference
- Immutable releases: https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases
- Artifact attestations: https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds
- Repository visibility changes: https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/setting-repository-visibility
- SemVer 2.0.0: https://semver.org/
- Conventional Commits 1.0.0: https://www.conventionalcommits.org/en/v1.0.0/
- DCO 1.1: https://developercertificate.org/
- OpenSSF baseline: https://baseline.openssf.org/versions/devel.html
- JSON Schema 2020-12 core: https://json-schema.org/draft/2020-12/json-schema-core
- POSIX utility conventions: https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap12.html
- XDG Base Directory 0.8: https://specifications.freedesktop.org/basedir-spec/latest/
- Rust platform support: https://doc.rust-lang.org/rustc/platform-support.html
- Cargo install/manifest/publish: https://doc.rust-lang.org/cargo/commands/cargo-install.html
- WinGet manifest: https://learn.microsoft.com/en-us/windows/package-manager/package/manifest
- Windows SmartScreen/signing: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
- Apple Developer ID/notarization: https://developer.apple.com/developer-id/
- Agent Skills specification: https://agentskills.io/specification
- MCP legacy lifecycle: https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle
- MCP current versioning/tools: https://modelcontextprotocol.io/docs/2026-07-28/learn/versioning
