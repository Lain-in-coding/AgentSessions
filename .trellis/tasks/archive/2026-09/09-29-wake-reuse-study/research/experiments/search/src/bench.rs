use crate::{
    Result,
    corpus::{Fixture, update_hash},
    query::{self, Method, Sql},
};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize)]
pub struct Sample {
    pub index: usize,
    pub query_ms: f64,
    pub open_ms: f64,
    pub lifecycle_ms: f64,
    pub ids: Vec<u64>,
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn open_read(db: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_secs(1))?;
    #[cfg(feature = "cache")]
    conn.set_prepared_statement_cache_capacity(16);
    Ok(conn)
}

/// Preparation, stepping, collecting and statement drop are inside query_ms.
/// Reopen samples also record open and full connection drop in lifecycle_ms.
fn measure(db: &Path, sql: &Sql, method: Method, reopen: bool, count: usize) -> Result<Value> {
    let start = Instant::now();
    let warm = open_read(db)?;
    let initial_open_ms = ms(start.elapsed());
    let warm_start = Instant::now();
    let ids = query::execute(&warm, sql, method)?;
    let warmup_ms = ms(warm_start.elapsed());
    let persistent = if reopen {
        drop(warm);
        None
    } else {
        Some(warm)
    };
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let start = Instant::now();
        let opened = if reopen { Some(open_read(db)?) } else { None };
        let open_ms = if reopen { ms(start.elapsed()) } else { 0.0 };
        let conn = opened
            .as_ref()
            .or(persistent.as_ref())
            .ok_or("missing connection")?;
        let query_start = Instant::now();
        let observed = query::execute(conn, sql, method)?;
        let query_ms = ms(query_start.elapsed());
        drop(opened);
        let lifecycle_ms = ms(start.elapsed());
        if observed != ids {
            return Err("immutable corpus returned inconsistent IDs".into());
        }
        samples.push(Sample {
            index,
            query_ms,
            open_ms,
            lifecycle_ms,
            ids: observed,
        });
    }
    Ok(json!({
        "method": method,
        "lifetime": if reopen { "reopen_per_request" } else { "persistent_connection" },
        "required_sample_count": count,
        "warmup": {"count": 1, "query_ms": warmup_ms, "initial_open_ms": initial_open_ms,
                   "statement_retained": !reopen && cfg!(feature = "cache") && matches!(method_name(method), "prepare_cached")},
        "ids": ids,
        "raw_samples": samples
    }))
}

fn method_name(method: Method) -> &'static str {
    match method {
        Method::Prepare => "prepare",
        #[cfg(feature = "cache")]
        Method::PrepareCached => "prepare_cached",
    }
}

fn differences(left: &[u64], right: &[u64]) -> Vec<u64> {
    left.iter()
        .copied()
        .filter(|id| !right.contains(id))
        .collect()
}

fn strategy(db: &Path, conn: &Connection, sql: Sql, qrels: &[u64], count: usize) -> Result<Value> {
    let measurement = measure(db, &sql, Method::Prepare, false, count)?;
    let ids: Vec<u64> = serde_json::from_value(measurement["ids"].clone())?;
    let relevant = ids.iter().filter(|id| qrels.contains(id)).count();
    Ok(json!({
        "query_plan": query::plan(conn, &sql)?,
        "statement": sql,
        "measurement": measurement,
        "relevant_retrieved": relevant,
        "recall_at_10": if qrels.is_empty() { None } else { Some(relevant as f64 / qrels.len() as f64) },
        "missing_relevant": differences(qrels, &ids),
        "unexpected_ids": differences(&ids, qrels)
    }))
}

fn progress(stage: &str, messages: u64) {
    println!(
        "{}",
        json!({"kind": "progress", "stage": stage, "messages": messages})
    );
}

pub fn run(db: &Path, messages: u64, count: usize) -> Result<Value> {
    let fixture = Fixture::load()?;
    if messages < fixture.minimum_messages() || messages > 1_000_000 || count == 0 || count > 10_000
    {
        return Err("messages must be 96..1000000 and samples 1..10000".into());
    }
    if !db.is_absolute() {
        return Err("--db must be an explicit absolute scratch path".into());
    }
    // No overwrite, cleanup, path discovery, or writes outside the explicit DB.
    std::fs::File::create_new(db)?;
    let mut conn = Connection::open(db)?;
    conn.busy_timeout(Duration::from_secs(1))?;
    conn.execute_batch(
        "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
        CREATE TABLE corpus(id INTEGER PRIMARY KEY, text TEXT NOT NULL);
        CREATE VIRTUAL TABLE cjk_fts USING fts5(text, tokenize='unicode61');
        CREATE VIRTUAL TABLE trigram_fts USING fts5(text, tokenize='trigram case_sensitive 0');",
    )?;
    progress("schema_created", 0);
    let start = Instant::now();
    let mut hash = Sha256::new();
    {
        let tx = conn.transaction()?;
        {
            let mut insert = tx.prepare("INSERT INTO corpus(id,text) VALUES (?1,?2)")?;
            for id in 1..=messages {
                let text = fixture.text(id);
                update_hash(&mut hash, id, &text);
                insert.execute(rusqlite::params![id as i64, text])?;
            }
        }
        tx.commit()?;
    }
    let corpus_ms = ms(start.elapsed());
    let data_hash = format!("{:x}", hash.finalize());
    progress("corpus_committed", messages);
    let mut index_times = Vec::new();
    for table in ["cjk_fts", "trigram_fts"] {
        let start = Instant::now();
        let tx = conn.transaction()?;
        {
            let mut read = tx.prepare("SELECT id,text FROM corpus ORDER BY id")?;
            let rows = read.query_map([], |row| {
                Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?))
            })?;
            let mut insert =
                tx.prepare(&format!("INSERT INTO {table}(rowid,text) VALUES (?1,?2)"))?;
            for row in rows {
                let (id, text) = row?;
                let text = if table == "cjk_fts" {
                    agent_session_grep_application::fts_tokens_cjk(&text)
                } else {
                    text
                };
                insert.execute(rusqlite::params![id as i64, text])?;
            }
        }
        tx.commit()?;
        index_times.push(ms(start.elapsed()));
        progress(table, messages);
    }
    let mut row_counts = serde_json::Map::new();
    for table in ["corpus", "cjk_fts", "trigram_fts"] {
        let actual: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })?;
        if actual != messages as i64 {
            return Err("index row count differs from generated corpus".into());
        }
        row_counts.insert(table.into(), json!(actual));
    }
    let mut queries = Vec::new();
    for query in &fixture.queries {
        let qrels = fixture.qrels(&query.id);
        let cjk = strategy(db, &conn, query::cjk(&query.text), &qrels, count)?;
        let trigram = strategy(db, &conn, query::trigram_like(&query.text), &qrels, count)?;
        let a: Vec<u64> = serde_json::from_value(cjk["measurement"]["ids"].clone())?;
        let b: Vec<u64> = serde_json::from_value(trigram["measurement"]["ids"].clone())?;
        queries.push(json!({
            "query": query, "qrels": qrels, "cjk": cjk, "trigram_like": trigram,
            "mismatch": { "same_ordered_ids": a == b,
                "only_cjk": differences(&a, &b), "only_trigram_like": differences(&b, &a) }
        }));
    }
    progress("strategies_measured", messages);
    let cache_statements = [
        Sql {
            route: "primary_key_range".into(),
            sql: "SELECT id FROM corpus WHERE id >= ?1 ORDER BY id LIMIT 10".into(),
            parameters: vec!["13".into()],
        },
        query::cjk("qarzneedle"),
        query::trigram_like("配置"),
    ];
    let mut cache_cases = Vec::new();
    for sql in cache_statements {
        let reference = query::execute(&conn, &sql, Method::Prepare)?;
        let mut modes = Vec::new();
        for reopen in [false, true] {
            for method in query::methods() {
                let measured = measure(db, &sql, method, reopen, count)?;
                if measured["ids"] != json!(reference) {
                    return Err("prepare/cache/lifetime result mismatch".into());
                }
                modes.push(measured);
            }
        }
        cache_cases.push(json!({
            "statement": sql, "query_plan": query::plan(&conn, &sql)?,
            "reference_ids": reference, "modes": modes, "all_ids_equal": true
        }));
    }
    drop(conn);
    Ok(json!({
        "schema_version": "wake-reuse-search.micro/v1",
        "status": "complete", "contains_real_transcripts": false,
        "messages": messages, "samples_per_query": count,
        "fixture_revision": fixture.revision, "fixture_hash": fixture.fixture_hash(),
        "dataset_hash": data_hash, "hash_method": crate::corpus::HASH_METHOD,
        "runtime": crate::runtime()?, "row_counts": row_counts,
        "build": {"corpus_ms": corpus_ms, "cjk_ms": index_times[0], "trigram_ms": index_times[1],
            "db_bytes": std::fs::metadata(db)?.len(), "db_hash": crate::sha256_file(db)?,
            "journal_mode": "delete", "synchronous": "full", "insertion_order": "corpus,cjk_fts,trigram_fts"},
        "queries": queries, "cache_cases": cache_cases,
        "cache_capacity": if cfg!(feature="cache") { Some(16) } else { None::<u64> },
        "limitations": [
            "Projection microbenchmark; no product ranking, RRF, sessions, provider, budgets, snippets, or filters.",
            "FTS uses BM25 then rowid; LIKE uses rowid, not Wake recency. Qrels expose non-equivalent semantics.",
            "One warmup per method/lifetime; OS cache not flushed; fixed sequential run order, not cold-disk evidence.",
            "Cache builds compare identical SQL and parameters; connection-local caches do not survive reopening.",
            "Index timings include same row streaming/insert loop; CJK alone also transforms text. DELETE journal differs from product WAL.",
            "Parent Python runner supplies hard process timeouts and resource/error evidence."
        ]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_sql_covers_semantics_and_identical_cache_results() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE corpus(id INTEGER PRIMARY KEY,text TEXT);
            CREATE VIRTUAL TABLE cjk_fts USING fts5(text);
            CREATE VIRTUAL TABLE trigram_fts USING fts5(text,tokenize='trigram case_sensitive 0');",
        )
        .unwrap();
        let fixture = Fixture::load().unwrap();
        let tx = conn.transaction().unwrap();
        for id in 1..=96 {
            let text = fixture.text(id);
            tx.execute(
                "INSERT INTO corpus VALUES (?1,?2)",
                rusqlite::params![id as i64, text],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO cjk_fts(rowid,text) VALUES (?1,?2)",
                rusqlite::params![
                    id as i64,
                    agent_session_grep_application::fts_tokens_cjk(&text)
                ],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO trigram_fts(rowid,text) VALUES (?1,?2)",
                rusqlite::params![id as i64, text],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        for (text, expected) in [
            ("%", vec![58, 59, 60, 94, 95, 96]),
            ("p_", vec![64, 65, 66]),
            ("\\", vec![31, 32, 33, 70, 71, 72]),
            ("zzabsentbeacon", vec![]),
        ] {
            let sql = query::trigram_like(text);
            let observed = query::execute(&conn, &sql, Method::Prepare).unwrap();
            assert_eq!(observed, expected);
            for method in query::methods() {
                for _ in 0..2 {
                    assert_eq!(query::execute(&conn, &sql, method).unwrap(), observed);
                }
            }
        }
        assert_eq!(
            query::execute(&conn, &query::cjk("qarzneedle"), Method::Prepare)
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            query::execute(&conn, &query::trigram_like("qarzneedle"), Method::Prepare)
                .unwrap()
                .len(),
            6
        );
        assert!(
            query::execute(&conn, &query::cjk("%"), Method::Prepare)
                .unwrap()
                .is_empty()
        );
        assert!(
            query::plan(&conn, &query::trigram_like("配置"))
                .unwrap()
                .iter()
                .any(|row| row.detail.contains("SCAN corpus"))
        );
        assert!(
            query::plan(&conn, &query::trigram_like("数据库"))
                .unwrap()
                .iter()
                .any(|row| row.detail.contains("VIRTUAL TABLE"))
        );
    }
}
