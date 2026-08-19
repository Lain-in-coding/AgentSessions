# Corpus density vs. initial-index throughput

Question: is the initial-index cost per message or per byte? If per message,
the gated corpus (509 bytes/message) understates MiB/s by the ratio between its
density and the real corpus's (7,174 bytes/message), and the 3.5 MiB/s
threshold is being measured against a corpus 14x denser in *work* per stated
gigabyte than real data.

Method: `synthetic_corpus.py generate --body-scale F` multiplies every sampled
body length, holding the message count (100,000), the provider mix, the
per-session message counts and the language distribution fixed. `F = 25.66`
lands the corpus at 7,002 bytes/message. Both corpora are then measured with
`performance_gate_benchmark.py run --profile full --skip-embeddings`, same
binary, paired back to back, per the two-consistent-runs rule.

No threshold was changed. The dense corpus is not the frozen contract, so the
harness records its values with `state = off_frozen_corpus_contract` and
`gate.pass = null` -- it cannot manufacture a pass.

## Corpora

| | sparse (frozen) | dense (variant) |
|---|---:|---:|
| body_scale | 1.0 | 25.66 |
| files / sessions | 5,000 / 5,000 | 5,000 / 5,000 |
| messages | 100,000 | 100,000 |
| bytes | 50,920,199 | 700,245,019 |
| bytes/message | 509.202 | 7,002.45 |
| session messages min/mean/max | 4 / 20.0 / 95 | 4 / 20.0 / 95 |
| provider message split | identical | identical |
| CJK char ratio | 0.1358 | 0.1357 |
| fixture hash | 65cc08e9... | (variant, not frozen) |

Real frozen corpus for reference: 1,242 files / 1,177,479,794 bytes /
164,136 messages = 7,174 bytes/message.

## Runs

Binary: `target/release/agent-session-grep.exe`, one build, all runs.

| run | corpus | MiB/s | messages/s | noop ms | search p95 ms | mcp p95 ms | store ratio | initial pass ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| dense 1 | dense | 7.829722 | 1172.455 | 843.38 | 115.60 | 86.44 | 3.557 | 85,291.1 |
| dense 2 | dense | (pending) | | | | | | |
| sparse 1 | sparse | (pending) | | | | | | |
| sparse 2 | sparse | (pending) | | | | | | |

## 1 GiB at real density

`requirement_projection` from dense run 1: 1 GiB at 7,002 bytes/message is
153,338 messages; at 1,172.46 messages/s that is **2.18 minutes**, against the
5-minute requirement.
