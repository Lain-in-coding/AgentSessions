# Documentation index

Start here. Every document under `docs/` is listed below with its audience and
what it is for, so nothing is discoverable only by browsing the tree.

Two audiences are mixed in this directory, and the difference matters:

- **Product documentation** tells you how to use the tool. It describes current
  behaviour and is kept current.
- **Governance records** (ADRs, RFCs, contracts, evidence, release rehearsals)
  are a record of how decisions were made and what was measured at a point in
  time. **They are history, not a statement of current product state.** A
  governance record is not updated when the product moves on; it is superseded
  by a later record. If a governance record and the product documentation
  disagree, the product documentation wins.

## Using the tool

| Document | For | What it covers |
|---|---|---|
| [guide/quickstart.md](guide/quickstart.md) | New users | Install, index your history, first search — copy-pasteable |
| [guide/providers.md](guide/providers.md) | All users | Per provider: where transcripts live, what is parsed, what is not |
| [guide/serve-tui-hook.md](guide/serve-tui-hook.md) | All users | The `serve`, `tui`, and `hook` entry points |
| [guide/troubleshooting.md](guide/troubleshooting.md) | All users | Indexed by the error text the tool actually prints |
| [guide/mcp-clients.md](guide/mcp-clients.md) | Agent authors | Wiring the MCP server into MCP clients |
| [reference/cli.md](reference/cli.md) | All users | Complete CLI reference: subcommands, flags, exit codes |
| [operations/INSTALL-AND-UPGRADE.md](operations/INSTALL-AND-UPGRADE.md) | All users | Install prefixes, PATH, upgrades, safe uninstall |
| [PROVIDER-ADAPTER-CONTRIBUTOR-GUIDE.md](PROVIDER-ADAPTER-CONTRIBUTOR-GUIDE.md) | Contributors | Writing a provider adapter |

## Product and project

| Document | For | What it covers |
|---|---|---|
| [product/PROVIDER-MATURITY-MATRIX.md](product/PROVIDER-MATURITY-MATRIX.md) | All users | Per-provider maturity grades and capabilities |
| [product/PROVIDER-BETA-READINESS.md](product/PROVIDER-BETA-READINESS.md) | Maintainers | What a provider must satisfy to be graded Beta |
| [product/COMPETITOR-COMPARISON.md](product/COMPETITOR-COMPARISON.md) | Evaluators | How this tool differs from comparable projects |
| [product/OPEN-SOURCE-ROADMAP.md](product/OPEN-SOURCE-ROADMAP.md) | All | Direction and scope |
| [security/THREAT-MODEL.md](security/THREAT-MODEL.md) | Security reviewers | Trust boundaries, assets, enforced controls |
| [performance-baseline.md](performance-baseline.md) | Maintainers | Recorded performance baseline |

Security boundaries and vulnerability reporting live in
[SECURITY.md](../SECURITY.md) at the repository root.

## Operations runbooks

For maintainers running releases and regressions, not needed to use the tool.

| Document | What it covers |
|---|---|
| [operations/rebuild-and-migration-runbook.md](operations/rebuild-and-migration-runbook.md) | Rebuilding an index and migrating a data root |
| [operations/REAL-DATA-REGRESSION.md](operations/REAL-DATA-REGRESSION.md) | Running the real-corpus regression harness |
| [operations/SEMANTIC-MODEL-BUNDLE.md](operations/SEMANTIC-MODEL-BUNDLE.md) | Importing the optional local semantic model bundle |
| [operations/PUBLIC-HISTORY-SCRUB.md](operations/PUBLIC-HISTORY-SCRUB.md) | Preparing a public tree from private history |
| [operations/REUSE-LICENSE-AUDIT.md](operations/REUSE-LICENSE-AUDIT.md) | Third-party licence audit procedure |
| [operations/core-beta-evidence-matrix.md](operations/core-beta-evidence-matrix.md) | Which evidence claims are locally vs CI verified |
| [operations/external-readiness-gate.md](operations/external-readiness-gate.md) | External readiness checklist |
| [operations/migration-v5-to-v6.md](operations/migration-v5-to-v6.md) | **Historical.** Superseded — the store schema is now v12 |
| [operations/migration-v6-to-v7.md](operations/migration-v6-to-v7.md) | **Historical.** Superseded — the store schema is now v12 |

## Governance records — history, not current state

Read the caveat at the top of this file before citing anything below.

**Architecture and contracts**

| Document | What it records |
|---|---|
| [architecture/RFC-0001-canonical-model-and-stable-id.md](architecture/RFC-0001-canonical-model-and-stable-id.md) | The canonical model and stable identity scheme |
| [architecture/RFC-0002-provider-adapter-contract.md](architecture/RFC-0002-provider-adapter-contract.md) | The provider adapter contract, including refuse-rather-than-guess |
| [architecture/R0-ARCHITECTURE-REVIEW.md](architecture/R0-ARCHITECTURE-REVIEW.md) | The initial architecture review |
| [contracts/CONTRACT-cli-robot-mcp-draft.md](contracts/CONTRACT-cli-robot-mcp-draft.md) | The CLI / Robot / MCP surface contract (draft) |

**Decision records.** Numbered ADRs in decision order.

| ADR | Decision |
|---|---|
| [0001](adr/ADR-0001-fulltext-search-engine.md) | Full-text search engine choice |
| [0002](adr/ADR-0002-platform-targets.md) | Platform targets |
| [0003](adr/ADR-0003-plain-text-only-search.md) | Plain-text-only search |
| [0004](adr/ADR-0004-output-time-snippet-redaction.md) | Output-time snippet redaction (scope later revised by 0009) |
| [0005](adr/ADR-0005-not-found-exit-4.md) | `not_found` exits 4 |
| [0006](adr/ADR-0006-machine-mode-help-envelope.md) | Machine-mode help envelope |
| [0007](adr/ADR-0007-cjk-bigram-indexing.md) | CJK bigram indexing |
| [0008](adr/ADR-0008-search-hit-session-context.md) | Session context on search hits |
| [0009](adr/ADR-0009-cross-boundary-output-redaction.md) | Cross-boundary output redaction by default |
| [0009](adr/ADR-0009-session-resume-metadata.md) | Session resume metadata: dual IDs and progressive disclosure |
| [0010](adr/ADR-0010-provider-maturity-rollback.md) | Provider maturity rollback |

> **Known defect:** two distinct ADRs were both numbered 0009. Cite them by
> filename, never by number alone. Renumbering would break existing references
> from code comments and other records, so both keep their filenames until a
> decision is recorded to renumber.

**Evidence and release records.** Measurements and rehearsals as they stood on
a given commit or date. Superseded by later measurements rather than edited.

| Document | What it records |
|---|---|
| [evidence/core-beta/88d86f4/manifest.md](evidence/core-beta/88d86f4/manifest.md) | Core-beta evidence manifest at commit `88d86f4` |
| [evidence/core-beta/88d86f4/core-beta-benchmark-full.md](evidence/core-beta/88d86f4/core-beta-benchmark-full.md) | Full benchmark run at that commit |
| [evidence/integration-beta/real-data-regression.md](evidence/integration-beta/real-data-regression.md) | Real-corpus regression runs, including harness defects found |
| [release/rehearsal-runbook.md](release/rehearsal-runbook.md) | The release rehearsal procedure |
| [release/go-no-go.template.md](release/go-no-go.template.md) | Go/no-go decision template |
| [release/go-no-go.2026-08-16.md](release/go-no-go.2026-08-16.md) | The go/no-go record dated 2026-08-16 |
| [product/SLI-AND-BENCHMARK-FORMAT.md](product/SLI-AND-BENCHMARK-FORMAT.md) | The benchmark and SLI reporting format |
| [security/FIXTURE-REDACTION-POLICY.md](security/FIXTURE-REDACTION-POLICY.md) | How test fixtures are redacted |
