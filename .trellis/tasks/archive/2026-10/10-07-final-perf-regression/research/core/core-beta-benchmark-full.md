# Core/Beta benchmark evidence

- Evidence status: `locally_verified`
- Profile: `full`
- Commit: `4bea67f4d113976d439e91a5bac825bb10a7106d`
- OS/target: `windows` / `x86_64-pc-windows-msvc`
- SQLite runtime: `3.53.2` (spikes/sqlite-snapshot-wal rebuilt+run at HEAD 4bea67f (workspace-identical locks rusqlite 0.40.1 / libsqlite3-sys 0.38.1), rusqlite::version()=3.53.2; not queried through measured CLI)
- Dataset: 4000 synthetic messages, SHA-256 `8ebb21d29ce589b9a36b94819a6b4baf2cf0f6c45bf591d552e8d40967833b4e`
- Binary SHA-256: `5924c24d9ecb625427ba97a52ecb2e60497ae1b3945e8bbf0edd4da050a6123d`
- Binary provenance: `caller_supplied_prebuilt`

These results are local evidence anchors, not formal SLOs or release certification.

## Metrics

| Metric | Samples | P50 | P95 | P99 | Mean | Sample stddev |
|---|---:|---:|---:|---:|---:|---:|
| `cli_startup_cold_latency_ms` | 20 | 19.278 | 23.284 | 35.756 | 20.290 | 4.033 |
| `cli_startup_warm_latency_ms` | 20 | 5.561 | 6.007 | 6.890 | 5.693 | 0.387 |
| `initial_sync_latency_ms` | 3 | 748.014 | 777.271 | 777.271 | 754.991 | 19.739 |
| `noop_sync_latency_ms` | 3 | 14.969 | 15.202 | 15.202 | 14.816 | 0.480 |
| `shrink_sync_latency_ms` | 3 | 638.429 | 638.775 | 638.775 | 634.060 | 7.868 |
| `search_latency_ms` | 100 | 63.600 | 72.133 | 79.199 | 64.955 | 4.497 |
| `show_latency_ms` | 100 | 9.230 | 13.567 | 25.912 | 10.008 | 3.200 |
| `get_latency_ms` | 100 | 9.405 | 11.018 | 27.927 | 10.053 | 3.699 |
| `initial_index_throughput_mb_s` | 3 | 1.431 | 1.447 | 1.447 | 1.418 | 0.037 |

## Sizes

- Release artifact: 6951936 bytes
- Synthetic source: 1122164 bytes
- Store after initial sync including sidecars: 20025344 bytes
- Store after shrink including sidecars: 20058112 bytes
- Store/source ratio: 17.845292

## Recovery

- Status: `not_implemented`
- Reason: The production CLI exposes recovery on open but no fault-injection command; a clean doctor open is not recovery evidence.

## Limitations

- These local measurements are evidence anchors, not formal SLOs or release certification.
- Cold startup uses a fresh copy of the release binary per sample; the harness does not flush OS filesystem caches.
- Startup measures process launch through --version completion, not an interactive readiness signal.
- Peak RSS uses the Windows process peak API where available; Linux /proc and macOS ps are sampled every 100 ms, so short-lived peaks may be missed.
- The SQLite runtime version is operator-supplied and its evidence source is recorded separately; Python's sqlite3 version is diagnostic only.
- Storage size includes the database and present SQLite sidecars after commands exit; no explicit checkpoint command is available.
- Recovery duration is unavailable without a production fault-injection entry point and is not inferred from a clean open.
- The measured binary was caller-supplied; its hash is authoritative, but source-to-binary linkage is not independently attested.
