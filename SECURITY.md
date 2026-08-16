# Security Policy

agent-session-grep reads AI coding-agent session transcripts and surfaces
their content through search, context assembly, resume, and handoff
features. The security posture below applies to every boundary that emits
that content.

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x   | ✅ |
| < 0.1   | ❌ |

## Reporting a vulnerability

Please report security issues privately — do not open a public issue.
Open a [GitHub Security Advisory](https://github.com/qin-devs/AgentSessions/security/advisories/new)
or email the maintainers directly. You can expect:

- An acknowledgement within 3 business days.
- A severity assessment and fix timeline within 10 business days.

## Boundaries and guarantees

| Boundary | Behavior |
|---|---|
| Human CLI / TUI | Local output. Session content is shown unredacted (ADR-0004). |
| Robot JSON/JSONL, MCP, HTTP API, Web UI, Handoff Pack | Cross-boundary outputs. Secret-shaped values (AWS keys, GitHub PATs, OpenAI/Anthropic/xAI keys, Bearer tokens, PEM private keys) and secret-named JSON fields are redacted with `[redacted:...]` markers (ADR-0009). |
| HTTP serve | Loopback-only (Host check), random bearer token per invocation, no TLS, GET-only. Do not expose the port to a network. |
| Provider transcripts | Read-only. Real user session data is never modified, uploaded, or committed. |

Redaction is a conservative, pattern-based ruleset (see
`crates/agent-session-grep-cli/src/redaction.rs`). It is not a substitute for
secret hygiene: treat any transcript content as potentially sensitive, and do
not rely on redaction for secrets whose format is not covered by the ruleset.

## Dependencies

Supply-chain auditing runs in CI via cargo-deny (see
`.github/workflows/ci.yml`). Keep `Cargo.lock` committed for all releases.
