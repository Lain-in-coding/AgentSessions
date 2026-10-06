# Core/Beta benchmark 复测对比（audit 基线 vs B7）

- baseline: commit `5b232cdedbff33a251e7fb266558be7af8496e1a` binary `acd7a5055c25…` generated `2026-10-05T12:12:49.301029Z`
- B7: commit `4bea67f4d113976d439e91a5bac825bb10a7106d` binary `5924c24d9ecb…` generated `2026-10-06T17:00:53.804910Z`
- dataset: 4000 messages / 20 files (hash `8ebb21d29ce5…`; baseline `8ebb21d29ce5…`)

| Metric | Samples | Base P50 | B7 P50 | Base P95 | B7 P95 | P95 Δ |
|---|---:|---:|---:|---:|---:|---:|
| `cli_startup_cold_latency_ms` | 20 | 20.337 | 19.278 | 23.600 | 23.284 | -0.317 |
| `cli_startup_warm_latency_ms` | 20 | 7.038 | 5.561 | 8.014 | 6.007 | -2.007 |
| `initial_sync_latency_ms` | 3 | 917.095 | 748.014 | 1059.322 | 777.271 | -282.050 |
| `noop_sync_latency_ms` | 3 | 15.486 | 14.969 | 16.221 | 15.202 | -1.020 |
| `shrink_sync_latency_ms` | 3 | 728.598 | 638.429 | 742.797 | 638.775 | -104.022 |
| `search_latency_ms` | 100 | 114.505 | 63.600 | 162.896 | 72.133 | -90.764 |
| `show_latency_ms` | 100 | 53.965 | 9.230 | 59.781 | 13.567 | -46.214 |
| `get_latency_ms` | 100 | 54.426 | 9.405 | 62.897 | 11.018 | -51.879 |
| `initial_index_throughput_mb_s` | 3 | 1.167 | 1.431 | 1.221 | 1.447 | +0.226 |

## Sizes

| Item | Base | B7 |
|---|---:|---:|
| `artifact_size_bytes` | 6919168 | 6951936 |
| `checkpoint` | not_available_cli_sidecars_measured_after_process_exit | not_available_cli_sidecars_measured_after_process_exit |
| `dataset_source_bytes` | 1122164 | 1122164 |
| `index_size_ratio` | 17.830691 | 17.845292 |
| `initial_store_bytes_including_sidecars` | 20008960 | 20025344 |
| `initial_store_bytes_raw_samples` | [20008960, 20008960, 20008960] | [20025344, 20025344, 20025344] |
| `initial_store_files` | ['catalog.db'] | ['catalog.db'] |
| `post_shrink_store_bytes_including_sidecars` | 20041728 | 20058112 |
| `post_shrink_store_files` | ['catalog.db', 'catalog.db-shm', 'catalog.db-wal'] | ['catalog.db', 'catalog.db-shm', 'catalog.db-wal'] |

## Notes

- Same harness (`scripts/evidence/core_beta_benchmark.py`), same profile (`full`), same machine class (NVMe SAMSUNG MZVL81T0HELB-00BTW SSD / NTFS), same binary provenance kind (`caller_supplied_prebuilt`).
- Baseline binary was built from commit `5b232cd` before the B1 hot-path fix; B7 binary is built from the current HEAD.
- Rows are not formal SLOs; they are local evidence anchors.
