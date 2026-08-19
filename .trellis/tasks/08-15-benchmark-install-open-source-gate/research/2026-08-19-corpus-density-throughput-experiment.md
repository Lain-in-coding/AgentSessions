# Corpus density vs. initial-index throughput

Question: is the initial-index cost per message or per byte? If per message,
the gated corpus (509 bytes/message) understates MiB/s relative to real data
(7,174 bytes/message), and the 3.5 MiB/s threshold is being measured against a
corpus that packs 14x more messages into a stated gigabyte than real data does.

Method: `synthetic_corpus.py generate --body-scale F` multiplies every sampled
body length, holding the message count (100,000), the provider mix, the
per-session message counts and the language distribution fixed. `F = 25.66`
lands the corpus at 7,002 bytes/message, the real corpus's density to within
2.4%. Both corpora are then measured with `performance_gate_benchmark.py run
--profile full --skip-embeddings`, same binary, paired back to back, per the
two-consistent-runs rule.

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
| per-provider message split | identical | identical |
| CJK char ratio | 0.1358 | 0.1357 |
| message chars p50 / mean | 115 / 223.3 | 2,491 / 5,206.3 |

Real frozen corpus, for reference: 1,242 files / 1,177,479,794 bytes /
164,136 messages = 7,174 bytes/message.

Both corpora ingest completely: the harness aborts unless the initial sync
emits exactly 100,000 messages, and every run below cleared that check.

## Runs

Binary: `target/release/agent-session-grep.exe`, one build, all eight runs.
Runs 3-8 use `--noop-reps 1 --search-reps 10 --mcp-reps 10 --overhead-reps 10`,
which does not touch the initial-index phase.

| # | corpus | MiB/s | messages/s | initial pass s | overhead p50 ms | noop ms | search p95 ms | mcp p95 ms |
|---:|---|---:|---:|---:|---:|---:|---:|---:|
| 1 | dense | **7.8297** | 1172.46 | 85.29 | 22.66 | 843.38 | 115.60 | 86.44 |
| 2 | dense | **6.3300** | 947.88 | 105.50 | 38.17 | 883.67 | 204.82 | 158.78 |
| 3 | sparse | 1.3679 | 2816.85 | 35.50 | 46.37 | 762.45 | 104.40 | 20.10 |
| 4 | sparse | 0.7905 | 1627.86 | 61.43 | 26.89 | 793.05 | 35.09 | 18.08 |
| 5 | sparse | 0.9490 | 1954.26 | 51.17 | 28.94 | 817.65 | 44.11 | 19.83 |
| 6 | dense | 6.5728 | 984.24 | 101.60 | 25.61 | 876.19 | 127.11 | 93.27 |
| 7 | sparse | 0.9910 | 2040.75 | 49.00 | 28.11 | 918.10 | 38.55 | 28.87 |
| 8 | dense | 6.1110 | 915.08 | 109.28 | 42.22 | 1650.71 | 258.06 | 279.75 |

Runs 5/6 and 7/8 are back-to-back interleaved pairs, added because the four
sparse runs spread 0.79-1.37 MiB/s (1.73x). `cli_process_overhead_p50_ms`
ranges 22.7-46.4 ms against a documented 21.7-30.9, so the machine was loaded
throughout; interleaving makes each dense number comparable to a sparse number
taken minutes earlier under the same load. Run 3 (1.3679) reproduces the
documented 1.36 baseline exactly; runs 4/5/7 are 28-42% below it, and the dense
runs were taken under the same degraded conditions, which makes the dense
figures conservative rather than flattering.

## Per-message vs. per-byte decomposition

Solving `ms_per_message = a + b * bytes_per_message` on each pair:

| pair | MiB/s ratio | msg/s ratio | a (per message) | b (per byte) | byte-only ceiling |
|---|---:|---:|---:|---:|---:|
| 3 -> 1 | 5.72x | 0.416x | 316 us | 76.7 ns | 12.44 MiB/s |
| 5 -> 6 | 6.93x | 0.504x | 472 us | 77.7 ns | 12.28 MiB/s |
| 7 -> 8 | 6.17x | 0.448x | 443 us | 92.8 ns | 10.27 MiB/s |

The per-byte coefficient reproduces to within 20% across three independent
pairs. At real density the split is roughly half fixed per message, half
proportional to bytes.

## 1 GiB at real density

| density | messages in 1 GiB | at 915.08 msg/s | at 1172.46 msg/s | requirement |
|---|---:|---:|---:|---:|
| dense corpus, 7,002.45 B/msg | 153,338 | 2.79 min | 2.18 min | <= 5 min |
| real corpus, 7,174 B/msg | 149,671 | 2.73 min | 2.13 min | <= 5 min |
| gated corpus, 509.202 B/msg | 2,108,676 | 17.2 min | 12.5 min | <= 5 min |

The same binary, on the same machine, meets the owner's stated requirement on a
real-density gigabyte and misses it by 2.5-3.4x on a synthetic-density
gigabyte. The difference is not performance; it is that the two gigabytes hold
14x different amounts of work.

## Side finding: store amplification is also a density artifact

`store_size_ratio` is 9.267 on the sparse corpus and **3.557** on the dense one
(471,859,200 vs 2,490,843,136 store bytes). The 9.27x figure recorded against
the gated corpus is dominated by fixed per-row overhead on 509-byte messages;
at real density the store is 3.56x the source.

## Verdict

Mechanism: **refuted.** Cost is not almost entirely per message. Messages/s
fell by half on the dense corpus (2,041 -> 915 in the closest pair), and about
half of the dense-corpus cost is proportional to bytes at roughly 77-93 ns/byte.
The predicted ~19 MiB/s (1.36 x 14) did not appear, and it could not: the
byte-proportional term alone caps this design near 10-12 MiB/s.

Conclusion: **confirmed.** Every dense run measured 6.11-7.83 MiB/s against a
3.5 threshold -- 1.75-2.24x above it, with the worst run still passing. A
real-density gigabyte indexes in 2.1-2.8 projected minutes against a 5-minute
requirement. The 3.5 MiB/s threshold is not unreachable; it is unreachable *on
the gated corpus*, whose 509 bytes/message is 14.1x denser in messages than the
data the requirement was written about.

Recommendation: leave `PERFORMANCE_THRESHOLDS` alone and fix the corpus density
instead. The section 1.3.4 ceiling argument (3.0 MiB/s maximum) is a statement
about the gated corpus's density, not about the design.
