# agent-session-grep

**Local-first search engine over AI coding-agent session history.**

`agent-session-grep` (CLI alias `asg`) normalizes heterogeneous provider
transcripts into a canonical domain model with stable identity and nonlinear
message graphs, then serves full-text retrieval, resume, and handoff through
CLI, MCP, Robot, TUI, and loopback Web UI surfaces — all sharing one
Application ADT.

## Why?

Every AI coding agent (Claude Code, Codex, Grok, Pi, …) writes its session
history in a different format, in a different local directory. When you need
to find "why did we do that three weeks ago?" or "what error did we hit in
that refactor?", you're stuck grepping raw JSONL files that don't share a
schema. `agent-session-grep` solves this by:

1. **Discovering** transcripts across multiple provider directories
2. **Normalizing** them into a canonical model (Message, Session, Placement)
3. **Indexing** for fast full-text search (FTS5 with CJK bigram support)
4. **Serving** search/resume/handoff through a unified contract

## Quickstart

```bash
# Build
cargo build --workspace

# Index your Claude Code + Codex sessions
asg sync --discover

# Search across all providers
asg search "authentication refactor"

# Get session context
asg context <session-id>

# Preview resume command for a session
asg resume <session-id>

# Generate a handoff pack for another agent
asg handoff "how did we configure the database?"
```

## Providers

Currently implemented (4/16 planned):

| Provider | Status | Format |
|---|---|---|
| Claude Code | Experimental | JSONL (`type:user/assistant`, `sessionId`/`uuid`/`parentUuid`) |
| Codex CLI | Experimental | rollout JSONL (`response_item`/`message`) |
| Grok Build | Experimental | ACP `updates.jsonl` (`session/update` stream) |
| Pi | Experimental | session JSONL (`type:session/message`) |

See [Provider Maturity Matrix](docs/product/PROVIDER-MATURITY-MATRIX.md) for
the full 16-provider roadmap.

## Architecture

```
domain ← ports ← application ← adapters
```

- **domain**: canonical entities (Message, Session, StableId, Placement)
- **ports**: trait contracts (CatalogStore, SearchIndex, ProviderAdapter)
- **application**: use cases (Search, Context, Handoff, Resume)
- **adapters**: concrete implementations (SQLite, Claude/Codex/Grok/Pi providers)

### Key design decisions

- **Stable Identity**: BLAKE3-based content-addressed IDs that survive file
  moves, renames, and incremental appends
- **Evidence-first**: every search hit carries a source span for verification
- **Privacy**: zero telemetry, zero upload, offline by default; cross-boundary
  outputs (Robot/MCP/Web/Handoff) are redacted by default (ADR-0009)
- **Honest maturity**: providers are graded certified/GA/beta/experimental/
  unsupported — never inflated

## License

MIT OR Apache-2.0

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Community provider adapters follow
the Provider Adapter Protocol (versioned external process, manifest-declared,
read-only, no network by default).
