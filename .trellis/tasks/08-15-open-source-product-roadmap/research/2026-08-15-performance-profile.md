# Research: Performance Profile — agent-session-grep hot paths (2026-08-15)

- **Query**: Profile agent-session-grep's performance hot paths; find concrete optimization targets (benchmark tables + static analysis + ranked targets)
- **Scope**: internal (code + local read-only benchmarks)
- **Date**: 2026-08-15
- **Binary**: isolated `target-08-15-profile/release/agent-session-grep.exe` build, sha256 `a3cc60fefcbbd11b2ffcc118e53ee865a26282635f02e69dbfc1552e6de57d4e`, commit `f4175a0`
- **Environment**: Windows 11 Home 22631, release build, Python 3.10, WAL mode SQLite (rusqlite). Absolute numbers have machine variance (Defender scans); relative scaling is the signal.

---

## 1. Gate benchmark (existing harness, current numbers)

`open_source_gate_benchmark.py run --binary <isolated exe> --sync-reps 3 --latency-reps 10`, corpus = 3 fixture files (3,482 B source):

| Metric | p50 | p95 | n |
|---|---|---|---|
| search | 12.42 ms | 15.03 ms | 6 |
| show | 12.06 ms | 14.32 ms | 10 |
| get | 9.67 ms | 11.97 ms | 10 |
| initial sync | 10.88 ms | 70.77 ms | 3 |

- Gate: **pass** (lexical_recall_at_10 = 1.0, parse_loss_ratio = 0.0)
- `index_size_ratio` = **48.23** (store 167,936 B incl. sidecars / source 3,482 B)

## 2. Scaling benchmark (new harness, deterministic corpus)

Generator: `%TEMP%/asg-perf-bench/scaling_bench.py` (temp dir, not in repo). Corpus: 100 sessions × 40 messages = 4,000 messages, 1,348,401 B source, mixed EN/CJK/tool_use content; queries include common token, rare token, CJK bigram, punctuation literal, phrase. Results JSON: `%TEMP%/asg-perf-bench/scaling-results.json`.

| Corpus | Files | Source | Fresh sync p50 | Noop sync p50 | Search p50 | Show p50 | Get p50 | Store | Store/source | Peak RSS |
|---|---|---|---|---|---|---|---|---|---|---|
| 1x | 1 | 13,498 B | 78 ms | 17 ms | 18.5 ms | 17.4 ms | 18.3 ms | 320 KB | 24.3x | 1.7 MB |
| 10x | 10 | 134,886 B | 160 ms | 20 ms | 15.2 ms | 15.3 ms | 14.8 ms | 1.97 MB | 15.0x | 13.9 MB |
| 100x | 100 | 1,348,401 B | **1,278 ms** | **42 ms** | 18.3 ms | 17.9 ms | 19.6 ms | **18.5 MB** | **14.0x** | **69.8 MB** |

- Incremental sync at 100x (1 of 100 files appended, 99 unchanged): **58 ms / 191 ms** (n=2) — vs 42 ms pure noop.
- Per-query latency at 100x (n=2 each): `pipeline` 17.8/18.6, `tombstone` 16.8/21.5, `配置` 15.5/16.0, `mcp.json` 19.3/18.3, `retry budget` 24.3/20.2 ms — all ≈ startup floor, no selectivity effect.
- `--version` floor: 10.2 / 13.3 / 27.0 ms across runs (Defender variance).
- Store is a single db file (no WAL/SHM survives process exit — checkpointed on close); sizes are exact multiples of 4096 B.
- Fresh sync per-message cost at 100x: ~0.32 ms/msg (1,278 ms − ~20 ms startup over 4,000 msgs); at 1x it is dominated by fixed per-process/per-file overhead.

**Headline scaling facts**
- **Search/show/get latency is flat (15–19 ms) from 320 KB to 18.9 MB DB — it is process-startup-bound, not query-bound.** The FTS5 bm25 query itself is sub-millisecond at this scale.
- Fresh ingest scales ~linearly at the top (0.32 ms/msg); noop sync is cheap (42 ms at 1.35 MB) but grows with total bytes, not message count.
- Store/source ratio falls 24x → 14x as corpus grows, but stays ~14x; on the tiny gate corpus it is 48x.

## 3. Store composition at 100x (18,923,520 B = 4,620 × 4096)

Content bytes vs index overhead (measured via sqlite3 on the bench db):

| Table | Rows | Content bytes |
|---|---|---|
| catalog payloads (4,001 msg + 100 ses + 100 doc) | 4,201 | 2,237,157 |
| fts bigrammed text | 4,001 | 224,038 |
| fts_ids id_json + wire_id | 4,201 | 566,936 |
| message_placements id fields | 4,001 | 568,142 |
| **visible content total** | | **≈ 3.6 MB (19%)** |
| B-tree / FTS5 shadow-table / index overhead | | **≈ 15.3 MB (81%)** |

Each message is stored 4–6 times across tables (catalog payload, fts text, fts_ids, placement, edge, membership claims); on top of that the FTS5 shadow tables + per-table B-tree overhead dominate the file.

## 4. Static hot-path analysis (file:line)

### 4.1 Write path (sync ingest)

- `commit_source_batches_if_changed` — `adapters-sqlite/src/lib.rs:1618`: no-op fast path first (`sources_are_current`, :2080), then **O(whole catalog) in-memory merge every sync even when one file changed**: `source_entity_membership_state` (:2322), `source_placement_membership_state` (:2362), `stored_placements` (:2383), `stored_edges` (:2441) each load the full membership/placement/edge tables into BTreeMaps; per-source `observed_placements`/`observed_edges`/`prepared_sources` maps; per-entity conflict merge via `merge_message_payloads` (:141) — 2× serde_json parse + full `Value` clone trees (:185-196) + re-serialize. This is why incremental sync at 100x (58–191 ms) costs ~2-4x a noop sync (42 ms) despite only 1 changed file.
- `commit_index_batch_with_relations` — `lib.rs:3090`: entity-loop statements are hoisted (comment :3101-3104, hoisted at :3106-3133 — 5 prepared statements), but per entity still: `serde_json::to_string(id)` (:3139), `bigram_cjk(text)` full-copy transform (:3153), 5 executes. **Placement/edge upserts (:3202, :3229) are NOT hoisted** — `tx.execute` per row = prepare+finalize per placement and per edge (2 per message).
- `upsert_fts_row_in_tx` — `lib.rs:3541` (single-row `put`/`index` path): id_json serialize + 2 executes.
- One transaction per sync (`begin_index_batch_with_relations` :2069 / `commit_index_batch_with_relations` :2070) — commit/fsync count is NOT a problem; the only PRAGMA is `journal_mode=WAL` (:957), no `synchronous`/`cache_size`/`wal_autocheckpoint` tuning.
- `bigram_cjk` — `application/src/cjk.rs:30`: **non-CJK input does a full `s.to_string()` copy (:32)**; CJK input builds `Vec<String>` tokens + `Vec<(bool, Vec<char>)>` runs (per-run `Vec<char>`) + `Vec<String>` parts + `format!` per bigram pair (:34-59). Called once per message on write, once per entry on current-check, once per query.

### 4.2 Read path

- `batch_is_current_with_derived_context` — `lib.rs:1519` (ingest path): **per entry 3× `conn.query_row`** (prepare+execute+finalize each): payload select (:1526), fts_ids select (:1568), fts text select via rowid subquery (:1593), plus optional `merge_message_payloads` and a full `bigram_cjk` re-transform per entry (:1605). O(entries × 3 prepares + 1 body copy).
- `sources_are_current` — `lib.rs:2080` (sync path): chunked IN reads (1 prepare per ≤~900-id chunk, :2115-2130), then fts text via `JOIN fts_ids fi ON fi.id_json = f.id` (:2143) — the only remaining read of the fts `id` content column; requires an fts scan or id_json-index lookup per chunk. Only runs for sources whose fingerprint changed.
- `SearchIndex::query` — `lib.rs:4300`: per-invocation prepare; per hit `id_json` String + `serde_json::from_str` (:4331). App assembly — `application/src/lib.rs:553-571`: `get_many` (chunked IN, `lib.rs:3784`) + `session_of` (chunked IN, `lib.rs:4225`) + per-hit payload JSON parse for snippet (:562) + byte-budget clamp (:575).
- CLI `sync_files` — `cli/src/main.rs:1581`: `capture()` (`adapters-sqlite/src/source_fs.rs:32` = full file read + blake3 hash) then `verify_snapshot` (`source_fs.rs:56` = **second full read + re-hash, returned bytes discarded**) for **every** file at :1676-1678, including fingerprint-unchanged ones. The fingerprint cache (:1631-1638) only skips parse+commit, not file I/O — noop sync cost is O(total bytes × 2 reads + 2 hashes).

## 5. Top optimization targets (ranked by expected win / effort)

| # | Target | Expected win | Effort | Evidence / how to verify |
|---|---|---|---|---|
| 1 | **Long-lived mode / spawn floor for search** — all read commands pay a 12–19 ms process+DB-open floor (measured: `--version` ≈ search at any corpus size); query itself is sub-ms. MCP mode (`mcp` subcommand) and TUI already keep the process alive; the product question is making the fast path the default. | 15 ms → <1 ms per interactive search (≈98% latency reduction) | medium (product/architectural) | gate benchmark search p50 on MCP/loopback vs per-process; `--version` floor as baseline |
| 2 | **Skip the second full read+hash for fingerprint-unchanged files** in `sync_files` (`main.rs:1676` + `source_fs.rs:56`) — verify only changed files, or fold verify into a single read. | ~50% of noop sync time; scales linearly with corpus (at 100 MB corpus, ~2–3 s per noop sync today) | low-medium | noop-sync latency at 100x before/after; unchanged-sync semantics tests (equal-length replacement detection is the guard) |
| 3 | **Store bloat: 14x ratio, 81% index overhead.** Concrete cuts: (a) drop the `fts.id` content column (id_json duplicated per row; only reader is the `sources_are_current` join at `lib.rs:2143` — switch to `fts_rowid` join; schema v8); (b) drop `fts_ids.id_json UNIQUE` index (`lib.rs:1027`, redundant with wire_id PK); (c) payload compression for catalog (2.2 MB of 18.9 MB). | est. 30–50% store reduction (~14x → ~7-10x ratio) | medium (migration + integrity evidence) | rebuild + gate manifest `index_size_ratio` + scaling `store_bytes`; recall/search e2e unchanged |
| 4 | **Hoist placement/edge upsert statements** (`lib.rs:3202, :3229`) — same pattern already applied to the entity loop (:3106). | ~1–2% of ingest (2 prepares/message saved ≈ 8,000 prepares per 4,000-msg sync) | trivial | `cargo test -p agent-session-grep-adapters-sqlite` + fresh-sync latency at 100x |
| 5 | **`bigram_cjk` allocation fast path** (`cjk.rs:31-32`): `Cow<str>`/borrow for non-CJK input instead of `s.to_string()`; iterate `&str` windows instead of per-run `Vec<char>`. | few % of ingest on EN-heavy corpora; also removes a full body copy from every search query and every current-check entry | trivial | `cargo test -p agent-session-grep-application` (cjk idempotence tests) + gate benchmark |

Additional small wins in the same family as #4/#5: `serde_json::to_string(id)` per entity (`lib.rs:3139`) and per-hit `serde_json::from_str` (:4331); `merge_message_payloads` Value-clone trees (:185-196); `PRAGMA synchronous=NORMAL`/`cache_size` tuning (currently only `journal_mode=WAL`).

## 6. Caveats

- Numbers are single-machine (Windows 11 + Defender); the `--version` floor varied 10–27 ms across runs. Scaling ratios are the reliable signal.
- `session_search_text` / "bm25 merge" (from the task prompt) do not exist in the codebase — there is no application-side merge; ranking is a single `ORDER BY bm25(fts), id` SQL query (`lib.rs:4317`).
- Peak RSS uses 100 ms sampling (Windows psapi peak); short-lived peaks are underreported, and sync RSS at 100x (69.8 MB for 1.35 MB source) includes SQLite page cache + full-catalog BTreeMap materialization (§4.1).
- Isolated build dir `target-08-15-profile/` left at repo root (untracked, same convention as sibling `target-08-15-*` dirs); benchmark scripts and corpus live in `%TEMP%/asg-perf-bench/`.
