//! End-to-end coverage for the additive `hermes/sqlite-state-v1` variant.
//!
//! Two facts can only be proven through the real composition root and cannot be
//! seen from the adapter alone:
//!
//! 1. `~/.hermes/state.db` and `~/.hermes/profiles/<name>/state.db` are separate
//!    *sources*, and the profile path feeds the installation namespace - so the
//!    same native session id in two profiles stays two canonical Sessions
//!    instead of collapsing into one entity.
//! 2. A state.db source is ingested, committed and searchable through the real
//!    CLI (`sync <path>` -> probe -> parse -> commit -> search).

use rusqlite::Connection;
use std::process::{Command, Output};

/// The freshly built binary (Cargo injects this path at compile time).
const BIN: &str = env!("CARGO_BIN_EXE_agent-session-grep");

/// Fixed clock, matching the other e2e suites so ranking stays deterministic.
const E2E_CLOCK_MS: &str = "1787616000000";

const SCHEMA: &str = "CREATE TABLE sessions (id TEXT PRIMARY KEY, started_at REAL);\n\
     CREATE TABLE messages (id INTEGER PRIMARY KEY, session_id TEXT, role TEXT, content TEXT, \
     tool_calls TEXT, tool_call_id TEXT, tool_name TEXT, reasoning TEXT, timestamp REAL);\n";

fn run(db: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("--db")
        .arg(db)
        .arg("--robot")
        .args(args)
        .env("ASG_CLOCK_MS", E2E_CLOCK_MS)
        .output()
        .expect("failed to spawn agent-session-grep binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn parse_first_line(output: &Output) -> serde_json::Value {
    let text = stdout(output);
    let line = text.lines().next().expect("output must have a line");
    serde_json::from_str(line).unwrap_or_else(|error| panic!("not JSON: {error}\n{line}"))
}

fn temp_db(tag: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("{tag}.db"));
    let db = path.to_string_lossy().into_owned();
    let store = agent_session_grep_adapters_sqlite::SqliteStore::open_for_write(&db)
        .expect("initialize empty catalog fixture under writer lease");
    drop(store);
    (dir, db)
}

/// Write one synthetic Hermes `state.db` with a single session and one message.
fn write_state_db(path: &std::path::Path, session_id: &str, text: &str, timestamp: f64) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create state.db parent");
    }
    let conn = Connection::open(path).expect("open synthetic state.db");
    conn.execute_batch(&format!(
        "{SCHEMA}INSERT INTO sessions VALUES ('{session_id}', 1700000000.0);\n\
         INSERT INTO messages (id, session_id, role, content, timestamp) \
         VALUES (1, '{session_id}', 'user', '{text}', {timestamp});\n"
    ))
    .expect("populate synthetic state.db");
}

/// Distinct `session_id` values from a Robot `search` frame.
fn hit_sessions(frame: &serde_json::Value) -> Vec<String> {
    let mut sessions: Vec<String> = frame["data"]["hits"]
        .as_array()
        .expect("search hits array")
        .iter()
        .filter_map(|hit| hit["session_id"].as_str().map(str::to_string))
        .collect();
    sessions.sort_unstable();
    sessions.dedup();
    sessions
}

#[test]
fn state_db_is_ingested_by_explicit_path_and_searchable() {
    let (dir, db) = temp_db("hermes-state-db");
    let source = dir.path().join(".hermes/state.db");
    write_state_db(
        &source,
        "hermes-state-sess-1",
        "synthetic hermes state probe text",
        1700000001.5,
    );
    let path = source.to_string_lossy().into_owned();

    let sync = run(&db, &["sync", &path]);
    assert!(sync.status.success(), "sync failed: {}", stdout(&sync));
    let frame = parse_first_line(&sync);
    assert_eq!(frame["data"]["committed"], 1, "{frame}");
    // Exactly one diagnostic: the identity note explaining that rowids are not
    // adopted as canonical message ids.
    assert_eq!(frame["data"]["diagnostics"], 1, "{frame}");
    assert_eq!(frame["data"]["skipped"], 0, "{frame}");

    let search = run(&db, &["search", "synthetic hermes state"]);
    assert!(
        search.status.success(),
        "search failed: {}",
        stdout(&search)
    );
    let frame = parse_first_line(&search);
    let hits = frame["data"]["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1, "{frame}");
    assert_eq!(hit_sessions(&frame).len(), 1, "{frame}");

    // A second sync of the unchanged snapshot is a content-level no-op.
    let resync = run(&db, &["sync", &path]);
    assert!(
        resync.status.success(),
        "resync failed: {}",
        stdout(&resync)
    );
    let frame = parse_first_line(&resync);
    assert_eq!(frame["data"]["committed"], 0, "{frame}");
    assert_eq!(frame["data"]["unchanged"], 1, "{frame}");
}

#[test]
fn profiles_with_the_same_native_session_id_stay_separate_sources() {
    let (dir, db) = temp_db("hermes-state-profiles");
    // Same native session id in both profile databases and the same message
    // rowid: only the profile path separates them.
    let profile_a = dir.path().join(".hermes/profiles/a/state.db");
    let profile_b = dir.path().join(".hermes/profiles/b/state.db");
    write_state_db(
        &profile_a,
        "shared-native-session",
        "synthetic shared token from profile a",
        1700000010.0,
    );
    write_state_db(
        &profile_b,
        "shared-native-session",
        "synthetic shared token from profile b",
        1700000011.0,
    );

    for source in [&profile_a, &profile_b] {
        let path = source.to_string_lossy().into_owned();
        let sync = run(&db, &["sync", &path]);
        assert!(sync.status.success(), "sync failed: {}", stdout(&sync));
        let frame = parse_first_line(&sync);
        assert_eq!(frame["data"]["committed"], 1, "{frame}");
    }

    let search = run(&db, &["search", "synthetic shared token"]);
    assert!(
        search.status.success(),
        "search failed: {}",
        stdout(&search)
    );
    let frame = parse_first_line(&search);
    let sessions = hit_sessions(&frame);
    assert_eq!(
        sessions.len(),
        2,
        "equal native session ids in two profiles must stay two Sessions: {frame}"
    );
    let texts: Vec<&str> = frame["data"]["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .filter_map(|hit| hit["text"].as_str())
        .collect();
    assert_eq!(texts.len(), 2, "{frame}");
    for expected in ["from profile a", "from profile b"] {
        assert!(
            texts.iter().any(|text| text.contains(expected)),
            "missing {expected} in {frame}"
        );
    }
}

/// Known limitation, reported to the owner rather than hidden: the composition
/// root's installation resolution is *not* part of this adapter, and an already
/// registered shallower installation root absorbs a deeper source that lives
/// under it. Syncing `~/.hermes/state.db` first therefore makes
/// `~/.hermes/profiles/<name>/state.db` join the main installation, and the
/// same native session id in both databases collapses into one Session.
///
/// Fixing it is an identity-layer decision (a single-file SQLite source needs a
/// file-level installation boundary, variant-aware and with migration
/// evidence), not an adapter change - the variant-aware probe/dispatch in this
/// crate cannot see the source path at all. This test pins the current
/// behaviour so such a change cannot land silently.
#[test]
fn known_limitation_main_database_absorbs_a_profile_source() {
    let (dir, db) = temp_db("hermes-state-absorb");
    let main_db = dir.path().join(".hermes/state.db");
    let profile_db = dir.path().join(".hermes/profiles/work/state.db");
    write_state_db(
        &main_db,
        "shared-native-session",
        "synthetic shared token from main",
        1700000010.0,
    );
    write_state_db(
        &profile_db,
        "shared-native-session",
        "synthetic shared token from profile",
        1700000011.0,
    );

    for source in [&main_db, &profile_db] {
        let path = source.to_string_lossy().into_owned();
        let sync = run(&db, &["sync", &path]);
        assert!(sync.status.success(), "sync failed: {}", stdout(&sync));
    }

    let search = run(&db, &["search", "synthetic shared token"]);
    assert!(
        search.status.success(),
        "search failed: {}",
        stdout(&search)
    );
    let frame = parse_first_line(&search);
    assert_eq!(
        hit_sessions(&frame).len(),
        1,
        "documented limitation: the shallower installation root currently owns \
         the deeper profile source, so both databases share one Session: {frame}"
    );
}
