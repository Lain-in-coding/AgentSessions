//! Ignored scale probe for `query_semantic` at the 100k-message target.
//!
//! Run with:
//!
//! ```text
//! cargo test -p agent-session-grep-adapters-sqlite --release \
//!     --test semantic_scale_bench -- --ignored --nocapture
//! ```
//!
//! Not part of the default suite: it materializes 100k 384-dimension vectors,
//! which is far past what a unit test should spend. The numbers it prints are
//! the before/after evidence for the tiered-retrieval work (plan M2-4).
//!
//! The corpus is populated over the probe's own SQLite connection rather than
//! through the write API, so the setup cost stays proportional to the data and
//! no bench-only method has to be added to the production surface.

use agent_session_grep_adapters_sqlite::SqliteStore;
use agent_session_grep_domain::{IdKind, Stability, StableId};
use agent_session_grep_ports::SemanticIndex;
use std::time::Instant;

const DIMENSION: usize = 384;
const MODEL: &str = "bench-model";
/// Share of corpus messages that contain the query term the probe searches for.
/// 3% of 100k is 3000 lexical matches — comfortably more than any candidate
/// limit, so the tiered path is measured under a saturated candidate set (its
/// worst case), not a lucky sparse one.
const MATCH_PERMILLE: usize = 30;

/// Deterministic vector source: a plain LCG, so the corpus is reproducible
/// without pulling in a rand dependency.
struct Lcg(u64);

impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) - 0.5
    }

    fn unit_vector(&mut self, dimension: usize) -> Vec<f32> {
        let mut values: Vec<f32> = (0..dimension).map(|_| self.next_f32()).collect();
        let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in &mut values {
                *value /= norm;
            }
        }
        values
    }
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
    sorted[index]
}

fn f32_slice_to_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "scale probe: materializes 100k vectors"]
fn semantic_query_latency_at_scale() {
    let corpus = env_usize("ASG_BENCH_VECTORS", 100_000);
    let queries = env_usize("ASG_BENCH_QUERIES", 20);

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bench.db");
    let db_path = db.to_str().unwrap().to_string();
    // Open once so migrations create the schema, then drop so the probe's own
    // connection is the only writer while it loads the corpus.
    drop(SqliteStore::open_for_write(&db_path).unwrap());

    let mut rng = Lcg(0x5eed_1234_9abc_def0);
    let build = Instant::now();
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=OFF;")
            .unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        {
            let mut insert_fts = tx
                .prepare("INSERT INTO fts(id, text) VALUES(?1, ?2)")
                .unwrap();
            let mut insert_ids = tx
                .prepare("INSERT INTO fts_ids(wire_id, id_json, fts_rowid) VALUES(?1, ?2, ?3)")
                .unwrap();
            let mut insert_vec = tx
                .prepare(
                    "INSERT INTO message_vec(wire_id, model_id, dimension, embedding)
                     VALUES(?1, ?2, ?3, ?4)",
                )
                .unwrap();
            for index in 0..corpus {
                let id = StableId::derive(
                    IdKind::Message,
                    Stability::Reconstructed,
                    &[format!("bench-{index}").as_bytes()],
                );
                let id_json = serde_json::to_string(&id).unwrap();
                // Every row shares filler tokens; a deterministic slice also
                // carries the probe's query term.
                let text = if index % 1000 < MATCH_PERMILLE {
                    format!("benchterm filler row {index} about retrieval latency")
                } else {
                    format!("filler row {index} about unrelated bookkeeping")
                };
                insert_fts
                    .execute(rusqlite::params![&id_json, &text])
                    .unwrap();
                let rowid = tx.last_insert_rowid();
                insert_ids
                    .execute(rusqlite::params![id.as_str(), &id_json, rowid])
                    .unwrap();
                insert_vec
                    .execute(rusqlite::params![
                        id.as_str(),
                        MODEL,
                        DIMENSION as i64,
                        f32_slice_to_bytes(&rng.unit_vector(DIMENSION))
                    ])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
    }
    let build_secs = build.elapsed().as_secs_f64();

    let store = SqliteStore::open(&db_path).unwrap();
    store.set_semantic_model(MODEL);
    // Tiered path unless the probe is asked for the full-scan baseline.
    let tiered = std::env::var("ASG_BENCH_FULL_SCAN").is_err();
    if tiered {
        store.set_semantic_candidate_query("benchterm");
    }

    // Warm the page cache so the reported numbers are steady state rather than
    // first-touch disk.
    let warm = rng.unit_vector(DIMENSION);
    assert_eq!(store.query_semantic(&warm, 20).unwrap().len(), 20);

    let mut samples = Vec::with_capacity(queries);
    for _ in 0..queries {
        let query = rng.unit_vector(DIMENSION);
        let start = Instant::now();
        let hits = store.query_semantic(&query, 20).unwrap();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(hits.len(), 20);
    }
    samples.sort_by(f64::total_cmp);

    let db_bytes: u64 = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum();

    println!(
        "query_semantic scale probe: path={} vectors={corpus} dimension={DIMENSION} \
         queries={queries} build_s={build_secs:.1} db_bytes={db_bytes} \
         p50_ms={:.2} p95_ms={:.2} min_ms={:.2} max_ms={:.2}",
        if tiered { "tiered" } else { "full_scan" },
        percentile(&samples, 0.50),
        percentile(&samples, 0.95),
        samples[0],
        samples[samples.len() - 1],
    );
}
