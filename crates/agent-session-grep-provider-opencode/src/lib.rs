//! OpenCode provider adapter.
//!
//! Parses OpenCode's `opencode.db` SQLite format: session/message/part tables.
//! The adapter receives the SQLite file as a byte stream, writes it to a
//! temporary file, and opens it read-only (SQLITE_OPEN_READONLY + busy_timeout)
//! per PRD requirement #4.
//!
//! Format evidence: fast-resume (MIT) `src/adapters/opencode.rs`.
//! The schema queries are adapted from fast-resume under its MIT license.

use std::io::Write;

use agent_session_grep_ports::MetadataResolution;
use agent_session_grep_ports::{
    AdapterManifest, CanonicalEventSink, Confidence, MessageEvent, ParseReport, ProbeResult,
    ProviderAdapter, ProviderError, manifest_for, rfc3339_utc_from_epoch_millis,
};
use rusqlite::{Connection, OpenFlags};

/// Variant id surfaced in probe results.
const VARIANT_ID: &str = "opencode/sqlite-v1";

/// SQLite magic header: every SQLite database starts with "SQLite format 3\0".
const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";

/// OpenCode's migration bookkeeping table (Drizzle-managed schema).
const OPENCODE_MIGRATION_TABLE: &str = "__drizzle_migrations";

/// zcode's migration bookkeeping table.
///
/// zcode's CLI database (`~/.zcode/cli/db/db.sqlite`) is a **fork of OpenCode's
/// schema**, so it is not merely similar — it answers every question this
/// adapter used to ask. Measured against a real zcode 3.7.7 database and a real
/// `opencode.db` side by side:
///
/// * `session` / `message` / `part` all exist in both, so the table-presence
///   test alone reports `Confirmed` for zcode;
/// * all three of this adapter's parse queries succeed on zcode and return
///   *plausible* rows (20 sessions, 51 text parts, 121 messages, roles exactly
///   `user`/`assistant`);
/// * both formats put the role at `$.role` and text parts at
///   `$.type == 'text'` / `$.text`.
///
/// The failure mode was therefore **silent, not loud**: claiming a zcode
/// database produced clean-looking transcripts attributed to the wrong
/// provider, never a parse error. That is why the probe has to discriminate
/// here rather than let `parse` discover the problem.
///
/// The discriminator is the migration table name, which is a lineage
/// fingerprint: zcode uses `schema_migration`, OpenCode uses
/// `__drizzle_migrations`. Distinguishing columns (zcode's
/// `session.task_type` / `title_source` / `trace_id`, or its extra
/// `message.sequence` / `part.sequence`) were rejected as the primary signal
/// because either product may add a column in a later version, whereas the
/// migration-table name identifies the schema's ancestry.
///
/// **Do not "tighten" this into a positive `__drizzle_migrations` requirement.**
/// The committed golden fixture (`tests/golden/basic.db`) contains neither
/// migration table — only `session`, `message`, and `part` — so demanding a
/// positive OpenCode marker would reject this adapter's own golden. Declining
/// when the zcode fingerprint is present is the only form that both fixes the
/// collision and keeps the fixture honest.
const ZCODE_MIGRATION_TABLE: &str = "schema_migration";

/// OpenCode adapter: parses `opencode.db` (SQLite: session/message/part).
///
/// The adapter writes the byte stream to a temp file and opens it read-only.
/// This is necessary because rusqlite requires a file path (no in-memory
/// deserialize in 0.40). The temp file is cleaned up after parsing.
pub struct OpenCodeAdapter;

impl OpenCodeAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for OpenCodeAdapter {
    fn provider_id(&self) -> &str {
        "opencode"
    }

    fn manifest(&self) -> AdapterManifest {
        manifest_for(
            self.provider_id(),
            Some(1),
            &[
                "SQLite source has no byte spans; messages are attributed without source offsets",
                "only text parts with role user/assistant are committed; tool/other parts are ignored",
                "per-message timestamps come from `message.time_created` (epoch milliseconds); rows whose value is missing, non-positive, or out of range carry no timestamp",
                "zcode's CLI database forks this schema (same session/message/part tables, same `$.role` and `$.type`='text'/`$.text` shapes); the probe refuses it on the `schema_migration` vs `__drizzle_migrations` fingerprint rather than attributing it to opencode",
            ],
        )
    }

    fn probe(&self, bytes: &[u8]) -> Result<ProbeResult, ProviderError> {
        let mut matched = Vec::new();
        let mut unmatched = Vec::new();

        // Quick check: SQLite files start with a magic header.
        if bytes.len() < SQLITE_MAGIC.len() || &bytes[..SQLITE_MAGIC.len()] != SQLITE_MAGIC {
            return Err(ProviderError::AmbiguousVariant(
                "not a SQLite database (missing magic header)".into(),
            ));
        }

        matched.push("SQLite magic header detected".into());

        // Open read-only and check for OpenCode tables.
        // _temp_db drops after conn: the guard deletes the temp file once the
        // connection is closed (Windows cannot delete an open file).
        let (conn, _temp_db) = open_readonly_from_bytes(bytes)
            .map_err(|e| ProviderError::StructuralFatal(format!("failed to open SQLite: {e}")))?;

        // Check for OpenCode-specific tables: session, message, part.
        let has_session = table_exists(&conn, "session");
        let has_message = table_exists(&conn, "message");
        let has_part = table_exists(&conn, "part");

        // Refuse a zcode database before any table-presence test can claim it:
        // zcode forked this schema, so `session`/`message`/`part` are all
        // present and the queries all succeed. See `ZCODE_MIGRATION_TABLE`.
        if table_exists(&conn, ZCODE_MIGRATION_TABLE)
            && !table_exists(&conn, OPENCODE_MIGRATION_TABLE)
        {
            return Err(ProviderError::AmbiguousVariant(format!(
                "database carries the `{ZCODE_MIGRATION_TABLE}` migration table and no \
                 `{OPENCODE_MIGRATION_TABLE}`, which fingerprints zcode's fork of the OpenCode \
                 schema — session/message/part are present in both, so this adapter refuses \
                 rather than attribute a zcode transcript to opencode"
            )));
        }

        if !has_session {
            return Err(ProviderError::AmbiguousVariant(
                "no `session` table found — not an OpenCode database".into(),
            ));
        }

        let confidence = if has_session && has_message && has_part {
            matched.push("OpenCode schema confirmed (session + message + part tables)".into());
            Confidence::Confirmed
        } else if has_session && has_message {
            matched.push("OpenCode schema (session + message tables, no part)".into());
            Confidence::High
        } else {
            matched.push("partial OpenCode schema (session table only)".into());
            unmatched.push("missing message/part tables".into());
            Confidence::Low
        };

        Ok(ProbeResult {
            variant_id: VARIANT_ID.to_string(),
            confidence,
            matched_evidence: matched,
            unmatched_evidence: unmatched,
        })
    }

    fn parse(
        &self,
        bytes: &[u8],
        sink: &mut dyn CanonicalEventSink,
    ) -> Result<ParseReport, ProviderError> {
        let mut report = ParseReport::default();

        // _temp_db drops after conn (see probe): the temp file is deleted once
        // the connection is closed.
        let (conn, _temp_db) = open_readonly_from_bytes(bytes)
            .map_err(|e| ProviderError::StructuralFatal(format!("failed to open SQLite: {e}")))?;

        // Query messages with their session and role.
        // Schema (from fast-resume):
        //   session(id, title, directory, time_created, time_updated)
        //   message(id, session_id, data)  -- data is JSON with role
        //   part(id, message_id, data)     -- data is JSON with type/text

        // Collect session metadata.
        let mut session_meta: std::collections::HashMap<String, (Option<String>, Option<String>)> =
            std::collections::HashMap::new();
        if let Ok(mut stmt) = conn.prepare("SELECT id, title, directory FROM session")
            && let Ok(rows) = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
        {
            for row in rows.filter_map(Result::ok) {
                session_meta.insert(row.0, (row.1, row.2));
            }
        }

        // Collect text parts by message_id.
        let mut parts_by_message: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT message_id, json_extract(data, '$.text') FROM part \
             WHERE json_extract(data, '$.type') = 'text' ORDER BY time_created ASC",
        ) && let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        }) {
            for (msg_id, text) in rows.filter_map(Result::ok) {
                if !text.is_empty() {
                    parts_by_message.entry(msg_id).or_default().push(text);
                }
            }
        }

        // Query messages ordered by time.
        let mut seq: u32 = 0;
        let mut session_ids: Vec<String> = Vec::new();

        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, session_id, json_extract(data, '$.role'), time_created \
             FROM message ORDER BY time_created ASC",
        ) && let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?, // message id
                row.get::<_, String>(1)?, // session_id
                row.get::<_, String>(2)?, // role
                // Unexpected column types must not drop the message: the
                // timestamp is optional, the text is not.
                row.get::<_, Option<i64>>(3).ok().flatten(), // time_created (epoch ms)
            ))
        }) {
            for (msg_id, sess_id, role, time_created) in rows.filter_map(Result::ok) {
                if !matches!(role.as_str(), "user" | "assistant") {
                    continue;
                }
                let parts = parts_by_message.get(&msg_id);
                let text = match parts {
                    Some(parts) if !parts.is_empty() => parts.join("\n"),
                    _ => continue,
                };
                if text.trim().is_empty() {
                    continue;
                }

                // Session identity from first message.
                if report.session_native_id.is_none() {
                    report.session_native_id = Some(sess_id.clone());
                    report.session_observation.provider_session_id =
                        MetadataResolution::Resolved(sess_id.clone());
                    if let Some((_, cwd)) = session_meta.get(&sess_id)
                        && let Some(cwd) = cwd
                        && !cwd.trim().is_empty()
                    {
                        report.session_observation.original_working_directory =
                            MetadataResolution::Resolved(cwd.trim().to_string());
                        report.session_observation.pair_observed = true;
                    }
                }
                if !session_ids.iter().any(|s| s == &sess_id) {
                    session_ids.push(sess_id.clone());
                }

                sink.emit_message(MessageEvent {
                    seq,
                    native_id: &msg_id,
                    parent_native_id: None,
                    role: &role,
                    text: &text,
                    timestamp: time_created
                        .and_then(rfc3339_utc_from_epoch_millis)
                        .as_deref(),
                    is_sidechain: false,
                    span: None, // SQLite doesn't have byte spans
                })
                .map_err(|e| ProviderError::StructuralFatal(e.to_string()))?;
                seq += 1;
                report.committed += 1;
            }
        }

        // Multi-session diagnostic.
        if session_ids.len() > 1 {
            report.session_observation.multi_session = true;
            report.session_observation.provider_session_id = MetadataResolution::Ambiguous;
            report.diagnostics.push(format!(
                "数据库包含 {} 个不同 session——单文件=单会话，全部消息归属首个会话 {}",
                session_ids.len(),
                session_ids[0]
            ));
        }

        Ok(report)
    }
}

/// Process-unique suffix for temp file names: pid + atomic counter.
///
/// Wall-clock nanoseconds alone can collide across parallel test threads when
/// the OS clock granularity is coarse (two `SystemTime::now()` calls within one
/// tick), and `File::create` would silently truncate the other thread's DB.
fn temp_file_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Deletes the temp SQLite file when dropped. Must be dropped AFTER the
/// `Connection` (Windows cannot delete an open file), so callers bind it in a
/// tuple pattern after the connection: `let (conn, _temp) = ...`.
struct TempDbGuard {
    path: std::path::PathBuf,
}

impl Drop for TempDbGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Open a SQLite database from bytes, read-only.
///
/// Writes bytes to a temp file, opens with SQLITE_OPEN_READONLY + busy_timeout,
/// and returns the connection plus a guard that deletes the temp file when the
/// connection has been dropped.
fn open_readonly_from_bytes(bytes: &[u8]) -> Result<(Connection, TempDbGuard), String> {
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!("asg-opencode-{}.db", temp_file_suffix()));
    let mut file = std::fs::File::create(&temp_path).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);

    let conn = Connection::open_with_flags(
        &temp_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;

    Ok((conn, TempDbGuard { path: temp_path }))
}

/// Check if a table exists in the database.
fn table_exists(conn: &Connection, table_name: &str) -> bool {
    conn.prepare(&format!(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='{table_name}'"
    ))
    .and_then(|mut stmt| stmt.exists([]))
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_matches_provider_matrix() {
        let adapter = OpenCodeAdapter::new();
        let manifest = adapter.manifest();
        assert_eq!(manifest.provider_id, adapter.provider_id());
        assert_eq!(manifest.supported_variants, vec![VARIANT_ID.to_string()]);
        assert_eq!(manifest.capabilities.provider_id, adapter.provider_id());
        assert_eq!(manifest.capabilities.variant_id, VARIANT_ID);
        assert!(manifest.last_certified_targets.is_empty());
        assert_eq!(manifest.fixture_revision, Some(1));
    }

    #[test]
    fn probe_rejects_non_sqlite_bytes() {
        let adapter = OpenCodeAdapter::new();
        let result = adapter.probe(b"not a sqlite file");
        assert!(result.is_err());
    }

    #[test]
    fn probe_rejects_empty_bytes() {
        let adapter = OpenCodeAdapter::new();
        let result = adapter.probe(b"");
        assert!(result.is_err());
    }

    #[test]
    fn probe_confirms_opencode_database() {
        let adapter = OpenCodeAdapter::new();
        let db_bytes = create_test_opencode_db();
        let result = adapter.probe(&db_bytes).unwrap();
        assert_eq!(result.variant_id, VARIANT_ID);
        assert_eq!(result.confidence, Confidence::Confirmed);
    }

    #[test]
    fn probe_refuses_zcode_fork_database() {
        // The real collision (measured against zcode 3.7.7's
        // `~/.zcode/cli/db/db.sqlite`): session/message/part all exist, so the
        // table-presence test reports Confirmed and every parse query returns
        // plausible rows. Attribution must be refused, not guessed.
        let adapter = OpenCodeAdapter::new();
        let db_bytes = create_test_zcode_db();
        let error = adapter
            .probe(&db_bytes)
            .expect_err("a zcode database must not be claimed as opencode");
        assert!(
            matches!(error, ProviderError::AmbiguousVariant(_)),
            "expected AmbiguousVariant, got {error:?}"
        );
    }

    #[test]
    fn probe_still_confirms_opencode_when_its_own_migration_table_is_present() {
        // A real `opencode.db` carries `__drizzle_migrations`. The zcode guard
        // must not make the genuine article ambiguous.
        let adapter = OpenCodeAdapter::new();
        let db_bytes = create_test_opencode_db_with_migrations();
        let result = adapter.probe(&db_bytes).unwrap();
        assert_eq!(result.confidence, Confidence::Confirmed);
    }

    #[test]
    fn probe_confirms_golden_fixture_without_either_migration_table() {
        // Pins the reason the guard is written as a negative test: the committed
        // golden has neither migration table, so a positive
        // `__drizzle_migrations` requirement would reject this adapter's own
        // fixture. If someone "tightens" the probe, this fails.
        let adapter = OpenCodeAdapter::new();
        let db_bytes = include_bytes!("../tests/golden/basic.db");
        let result = adapter.probe(db_bytes).unwrap();
        assert_eq!(result.confidence, Confidence::Confirmed);
    }

    #[test]
    fn parse_extracts_messages_from_opencode_db() {
        let adapter = OpenCodeAdapter::new();
        let db_bytes = create_test_opencode_db();

        #[derive(Default)]
        struct CountSink {
            count: usize,
            timestamps: Vec<Option<String>>,
        }
        impl CanonicalEventSink for CountSink {
            fn emit_message(
                &mut self,
                event: MessageEvent<'_>,
            ) -> agent_session_grep_ports::PortResult<()> {
                self.count += 1;
                self.timestamps.push(event.timestamp.map(str::to_string));
                Ok(())
            }
        }

        let mut sink = CountSink::default();
        let report = adapter.parse(&db_bytes, &mut sink).unwrap();
        assert_eq!(report.committed, 2);
        assert_eq!(sink.count, 2);
        assert!(report.session_native_id.is_some());
        // `message.time_created` (epoch ms) must reach the sink as RFC3339 UTC.
        assert_eq!(
            sink.timestamps,
            vec![
                Some("2026-02-14T09:15:00.000Z".to_string()),
                Some("2026-02-14T09:15:04.250Z".to_string()),
            ]
        );
    }

    #[test]
    fn epoch_millis_render_as_rfc3339_utc() {
        // The conversion itself is covered in ports (`time.rs`); this pins the
        // two instants the fixture DB depends on, so a shared-helper change that
        // shifted them would fail here too.
        assert_eq!(
            rfc3339_utc_from_epoch_millis(1_771_060_500_000).as_deref(),
            Some("2026-02-14T09:15:00.000Z")
        );
        assert_eq!(
            rfc3339_utc_from_epoch_millis(1_771_060_504_250).as_deref(),
            Some("2026-02-14T09:15:04.250Z")
        );
    }

    #[test]
    fn non_positive_or_absurd_epoch_millis_yield_no_timestamp() {
        // Better no timestamp than a fabricated one.
        assert!(rfc3339_utc_from_epoch_millis(0).is_none());
        assert!(rfc3339_utc_from_epoch_millis(-1).is_none());
        assert!(rfc3339_utc_from_epoch_millis(i64::MAX).is_none());
    }

    /// Serialize an in-memory database built from `sql` to on-disk bytes.
    fn db_bytes_from_sql(sql: &str) -> Vec<u8> {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(sql).unwrap();
        let temp_path = std::env::temp_dir().join(format!("asg-test-{}.db", temp_file_suffix()));
        conn.execute_batch(&format!("VACUUM INTO '{}'", temp_path.display()))
            .unwrap();
        drop(conn);
        let bytes = std::fs::read(&temp_path).unwrap();
        let _ = std::fs::remove_file(&temp_path);
        bytes
    }

    /// A zcode CLI database: OpenCode's forked schema plus zcode's own
    /// `schema_migration` bookkeeping table and `sequence` columns.
    ///
    /// Column list and `data` shapes transcribed from a real zcode 3.7.7
    /// `~/.zcode/cli/db/db.sqlite`. The point of the fixture is that everything
    /// this adapter reads is present and well-formed — the role is at `$.role`,
    /// text parts are `$.type == 'text'` with the text at `$.text` — so only the
    /// migration-table fingerprint separates it from OpenCode.
    fn create_test_zcode_db() -> Vec<u8> {
        db_bytes_from_sql(
            "CREATE TABLE schema_migration (version INTEGER PRIMARY KEY, applied_at INTEGER);
             CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, workspace_id TEXT, parent_id TEXT, slug TEXT, directory TEXT, path TEXT, title TEXT, version TEXT, time_created INTEGER, time_updated INTEGER, task_type TEXT, title_source TEXT, title_message_id TEXT, time_title_updated INTEGER, trace_id TEXT);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT, sequence INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT, sequence INTEGER);
             INSERT INTO schema_migration VALUES (42, 1771060000000);
             INSERT INTO session (id, directory, title, time_created) VALUES ('ses_z1', '/zwork', 'zcode session', 1771060499000);
             INSERT INTO message (id, session_id, time_created, data, sequence) VALUES ('zmsg_1', 'ses_z1', 1771060500000, '{\"role\":\"user\"}', 1);
             INSERT INTO message (id, session_id, time_created, data, sequence) VALUES ('zmsg_2', 'ses_z1', 1771060504250, '{\"role\":\"assistant\"}', 2);
             INSERT INTO part (id, message_id, session_id, time_created, data, sequence) VALUES ('zpart_1', 'zmsg_1', 'ses_z1', 1771060500000, '{\"type\":\"text\",\"text\":\"hello from zcode\",\"time\":{\"start\":1771060500000}}', 1);
             INSERT INTO part (id, message_id, session_id, time_created, data, sequence) VALUES ('zpart_2', 'zmsg_2', 'ses_z1', 1771060504250, '{\"type\":\"text\",\"text\":\"reply from zcode\",\"time\":{\"start\":1771060504250}}', 2);",
        )
    }

    /// An OpenCode database that carries `__drizzle_migrations`, as a real
    /// `opencode.db` does.
    fn create_test_opencode_db_with_migrations() -> Vec<u8> {
        db_bytes_from_sql(
            "CREATE TABLE __drizzle_migrations (id INTEGER PRIMARY KEY, hash TEXT, created_at INTEGER);
             CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT, directory TEXT, time_created INTEGER, time_updated INTEGER);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, data TEXT, time_created INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, data TEXT, time_created INTEGER);
             INSERT INTO __drizzle_migrations VALUES (1, 'deadbeef', 1771060000000);
             INSERT INTO session VALUES ('ses_1', 'test', '/work', 1771060499000, 1771060800000);
             INSERT INTO message VALUES ('msg_1', 'ses_1', '{\"role\":\"user\"}', 1771060500000);
             INSERT INTO part VALUES ('part_1', 'msg_1', '{\"type\":\"text\",\"text\":\"hello world\"}', 1771060500000);",
        )
    }

    /// Create a test OpenCode SQLite database in memory and return its bytes.
    fn create_test_opencode_db() -> Vec<u8> {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT, directory TEXT, time_created INTEGER, time_updated INTEGER);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, data TEXT, time_created INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, data TEXT, time_created INTEGER);
             INSERT INTO session VALUES ('ses_1', 'test', '/work', 1771060499000, 1771060800000);
             INSERT INTO message VALUES ('msg_1', 'ses_1', '{\"role\":\"user\"}', 1771060500000);
             INSERT INTO message VALUES ('msg_2', 'ses_1', '{\"role\":\"assistant\"}', 1771060504250);
             INSERT INTO part VALUES ('part_1', 'msg_1', '{\"type\":\"text\",\"text\":\"hello world\"}', 1771060500000);
             INSERT INTO part VALUES ('part_2', 'msg_2', '{\"type\":\"text\",\"text\":\"hi there\"}', 1771060504250);",
        )
        .unwrap();

        // Serialize the in-memory DB to bytes via backup.
        let temp_path = std::env::temp_dir().join(format!("asg-test-{}.db", temp_file_suffix()));
        conn.execute_batch(&format!("VACUUM INTO '{}'", temp_path.display()))
            .unwrap();
        drop(conn);
        let bytes = std::fs::read(&temp_path).unwrap();
        let _ = std::fs::remove_file(&temp_path);
        bytes
    }
}
