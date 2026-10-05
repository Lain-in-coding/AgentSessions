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
    ProviderAdapter, ProviderError, manifest_for,
};
use rusqlite::{Connection, OpenFlags};

/// Variant id surfaced in probe results.
const VARIANT_ID: &str = "opencode/sqlite-v1";

/// SQLite magic header: every SQLite database starts with "SQLite format 3\0".
const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";

/// OpenCode adapter: parses `opencode.db` (SQLite: session/message/part).
///
/// The adapter writes snapshot bytes to an exclusively created temp database
/// and opens it read-only. Its owned database, sidecars, and directory receive
/// best-effort cleanup when the temporary database owner drops.
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
                "per-message timestamps are not extracted",
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

        // Open read-only and check for OpenCode tables. `db` unlinks the temp
        // copy when it goes out of scope (see `TempDb`).
        let db = open_readonly_from_bytes(bytes)
            .map_err(|e| ProviderError::StructuralFatal(format!("failed to open SQLite: {e}")))?;
        let conn = &db.conn;

        // Check for OpenCode-specific tables: session, message, part.
        let has_session = table_exists(conn, "session")?;
        let has_message = table_exists(conn, "message")?;
        let has_part = table_exists(conn, "part")?;

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

        // `db` unlinks the temp copy when it goes out of scope (see `TempDb`).
        let db = open_readonly_from_bytes(bytes)
            .map_err(|e| ProviderError::StructuralFatal(format!("failed to open SQLite: {e}")))?;
        let conn = &db.conn;

        // Query messages with their session and role.
        // Schema (from fast-resume):
        //   session(id, title, directory, time_created, time_updated)
        //   message(id, session_id, data)  -- data is JSON with role
        //   part(id, message_id, data)     -- data is JSON with type/text

        let sql_error = |error: rusqlite::Error| {
            ProviderError::StructuralFatal(format!("OpenCode SQLite query failed: {error}"))
        };
        let mut session_meta = std::collections::BTreeMap::new();
        let mut stmt = conn
            .prepare("SELECT id, directory FROM session ORDER BY id")
            .map_err(sql_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(sql_error)?;
        for row in rows {
            let (id, directory) = row.map_err(sql_error)?;
            if id.trim().is_empty() {
                return Err(ProviderError::StructuralFatal(
                    "OpenCode session id is empty".into(),
                ));
            }
            let cwd = directory.filter(|value| !value.trim().is_empty());
            session_meta.insert(
                id.clone(),
                agent_session_grep_ports::ProviderSessionIdentity {
                    source_key: id.clone(),
                    observation: agent_session_grep_ports::ProviderSessionObservation {
                        provider_session_id: MetadataResolution::Resolved(id),
                        original_working_directory: cwd
                            .clone()
                            .map(MetadataResolution::Resolved)
                            .unwrap_or_default(),
                        pair_observed: cwd.is_some(),
                        multi_session: false,
                    },
                },
            );
        }

        let mut parts_by_message: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut stmt = conn
            .prepare(
                "SELECT message_id, json_extract(data, '$.text') FROM part \
             WHERE json_extract(data, '$.type') = 'text' ORDER BY time_created ASC, id ASC",
            )
            .map_err(sql_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                ))
            })
            .map_err(sql_error)?;
        for row in rows {
            let (msg_id, text) = row.map_err(sql_error)?;
            if !text.is_empty() {
                parts_by_message.entry(msg_id).or_default().push(text);
            }
        }

        let mut seq: u32 = 0;
        let mut session_ids = std::collections::BTreeSet::new();
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, json_extract(data, '$.role') \
             FROM message ORDER BY time_created ASC, id ASC",
            )
            .map_err(sql_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, rusqlite::types::Value>(2)?,
                ))
            })
            .map_err(sql_error)?;
        for row in rows {
            let (msg_id, sess_id, role) = row.map_err(sql_error)?;
            // A decoded SQL value lacking a textual role is a malformed
            // provider record, counted as skipped. Prepare/query/row-decoding
            // failures above remain fatal; no backend error is swallowed.
            let rusqlite::types::Value::Text(role) = role else {
                report.skipped += 1;
                report
                    .diagnostics
                    .push("OpenCode message has no textual role; skipped".into());
                continue;
            };
            if !matches!(role.as_str(), "user" | "assistant") {
                continue;
            }
            let text = match parts_by_message.get(&msg_id) {
                Some(parts) => parts.join("\n"),
                None => continue,
            };
            if text.trim().is_empty() {
                continue;
            }
            let session = session_meta.get(&sess_id).ok_or_else(|| {
                ProviderError::StructuralFatal(
                    "OpenCode message references an unknown session".into(),
                )
            })?;
            if report.session_native_id.is_none() {
                report.session_native_id = Some(sess_id.clone());
                report.session_observation = session.observation.clone();
            }
            session_ids.insert(sess_id);
            sink.emit_message(MessageEvent {
                session: Some(session),
                seq,
                native_id: &msg_id,
                parent_native_id: None,
                role: &role,
                text: &text,
                timestamp: None,
                is_sidechain: false,
                span: None,
            })
            .map_err(|e| ProviderError::StructuralFatal(e.to_string()))?;
            seq = seq.checked_add(1).ok_or_else(|| {
                ProviderError::StructuralFatal("too many OpenCode messages".into())
            })?;
            report.committed += 1;
        }
        if session_ids.len() > 1 {
            report.session_observation.multi_session = true;
            report.session_observation.provider_session_id = MetadataResolution::Ambiguous;
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

/// Owns only an exclusively-created directory. Unknown entries keep it alive:
/// cleanup deliberately removes an empty directory nonrecursively.
struct TempDirGuard {
    path: std::path::PathBuf,
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
    }
}

/// Constructed only after exclusive DB creation succeeds in the owned namespace.
/// A final read-only connection can leave sidecars behind. Remove that fixed
/// group first; the directory guard then removes the now-empty directory.
struct TempDbGuard {
    path: std::path::PathBuf,
    _directory: TempDirGuard,
}

impl Drop for TempDbGuard {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut owned_file = self.path.as_os_str().to_os_string();
            owned_file.push(suffix);
            let _ = std::fs::remove_file(std::path::Path::new(&owned_file));
        }
    }
}

/// A read-only connection over a temp copy of the source bytes, bundled with the
/// guard that unlinks that copy.
///
/// **Field order is load-bearing.** Struct fields drop in declaration order, so
/// `conn` closes the database before `_guard` removes its file group. On Windows,
/// handles without delete sharing can block removal; cleanup errors are ignored,
/// so reversing the order can silently leak the copy. A tuple binding
/// (`let (conn, guard) = ...`) drops the *later* binding first — i.e. the guard
/// while the connection is still open — which is exactly the broken order this
/// struct exists to prevent.
struct TempDb {
    conn: Connection,
    _guard: TempDbGuard,
}

impl TempDb {
    /// Path of the temp copy backing this connection.
    #[cfg(test)]
    fn temp_path(&self) -> &std::path::Path {
        &self._guard.path
    }
}

/// Open a SQLite database from bytes, read-only.
///
/// Writes bytes to a temp file, opens with SQLITE_OPEN_READONLY + busy_timeout,
/// and returns a [`TempDb`] that deletes the temp copy once it goes out of scope.
fn open_readonly_from_bytes(bytes: &[u8]) -> Result<TempDb, String> {
    open_readonly_from_bytes_at(bytes, temp_db_path(&std::env::temp_dir()))
}

fn temp_db_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(format!("asg-opencode-{}", temp_file_suffix()))
        .join("snapshot.db")
}

fn create_temp_db_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn open_readonly_from_bytes_at(
    bytes: &[u8],
    temp_path: std::path::PathBuf,
) -> Result<TempDb, String> {
    open_readonly_from_bytes_with(
        bytes,
        temp_path,
        create_temp_db_file,
        |file, bytes| file.write_all(bytes),
        std::fs::File::sync_all,
        |path, flags| Connection::open_with_flags(path, flags),
    )
}

// Keep fault injection local to this same production lifecycle, not a copied opener.
fn open_readonly_from_bytes_with(
    bytes: &[u8],
    temp_path: std::path::PathBuf,
    create_file: impl FnOnce(&std::path::Path) -> std::io::Result<std::fs::File>,
    write_file: impl FnOnce(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
    sync_file: impl FnOnce(&std::fs::File) -> std::io::Result<()>,
    open_db: impl FnOnce(&std::path::Path, OpenFlags) -> rusqlite::Result<Connection>,
) -> Result<TempDb, String> {
    let directory = temp_path.parent().expect("temporary DB path has a parent");
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    // Never reuse even an empty directory or a symlink. Windows directory/file
    // permissions remain inherited ACLs, not an owner-only DACL guarantee.
    builder
        .recursive(false)
        .create(directory)
        .map_err(|e| e.to_string())?;
    let directory = TempDirGuard {
        path: directory.to_path_buf(),
    };
    let file = create_file(&temp_path).map_err(|e| e.to_string())?;
    let guard = TempDbGuard {
        path: temp_path,
        _directory: directory,
    };
    {
        // Move, rather than borrow, the handle into a later binding so every
        // write/sync error closes it before the DB guard starts cleanup.
        let mut file = file;
        write_file(&mut file, bytes).map_err(|e| e.to_string())?;
        sync_file(&file).map_err(|e| e.to_string())?;
    }

    let conn = open_db(
        &guard.path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;

    Ok(TempDb {
        conn,
        _guard: guard,
    })
}

/// Check if a table exists in the database.
fn table_exists(conn: &Connection, table_name: &str) -> Result<bool, ProviderError> {
    conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name=?1")
        .and_then(|mut stmt| stmt.exists([table_name]))
        .map_err(|error| {
            ProviderError::StructuralFatal(format!("OpenCode SQLite schema query failed: {error}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod temp_ownership {
        use super::*;
        use std::io::Write;
        use std::path::{Path, PathBuf};

        struct Case {
            root: PathBuf,
            path: PathBuf,
        }

        impl Case {
            fn new() -> Self {
                let root = std::env::temp_dir().join(format!(
                    "asg-opencode-ownership-test-{}",
                    temp_file_suffix()
                ));
                std::fs::create_dir(&root).expect("exclusive test scratch directory");
                let path = temp_db_path(&root);
                Self { root, path }
            }

            fn occupy_namespace(&self) {
                let parent = self.path.parent().unwrap();
                if parent != self.root.as_path() {
                    std::fs::create_dir(parent).expect("pre-existing candidate namespace");
                }
            }

            fn foreign_files(&self, suffixes: &[&str]) -> Vec<(PathBuf, Vec<u8>)> {
                suffixes
                    .iter()
                    .map(|suffix| {
                        let mut path = self.path.as_os_str().to_os_string();
                        path.push(suffix);
                        let path = PathBuf::from(path);
                        let bytes = format!("foreign {suffix} bytes must survive").into_bytes();
                        std::fs::write(&path, &bytes).unwrap();
                        (path, bytes)
                    })
                    .collect()
            }
        }

        impl Drop for Case {
            fn drop(&mut self) {
                // Only this test's exclusively-created scratch tree; all preservation
                // assertions run before it is removed, including on the RED baseline.
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }

        fn bytes() -> Vec<u8> {
            create_test_opencode_db()
        }

        fn assert_conflict_preserved(
            result: Result<TempDb, String>,
            foreign: &[(PathBuf, Vec<u8>)],
        ) {
            let snapshot = || {
                foreign
                    .iter()
                    .map(|(path, _)| std::fs::read(path).map_err(|error| error.kind()))
                    .collect::<Vec<_>>()
            };
            // Check both while a mistakenly accepted copy could still be alive and
            // after it has dropped: refusal must neither truncate nor unlink a file.
            let before_drop = snapshot();
            let rejected = result.is_err();
            drop(result);
            let after_drop = snapshot();
            let expected: Vec<Result<Vec<u8>, std::io::ErrorKind>> = foreign
                .iter()
                .map(|(_, contents)| Ok(contents.clone()))
                .collect();
            assert_eq!(
                (before_drop, after_drop),
                (expected.clone(), expected),
                "foreign files changed before or after dropping the result"
            );
            assert!(rejected, "an occupied candidate must be rejected");
        }

        #[test]
        fn existing_main_and_sidecars_are_preserved() {
            let case = Case::new();
            case.occupy_namespace();
            let foreign = case.foreign_files(&["", "-wal", "-shm", "-journal"]);
            let result = open_readonly_from_bytes_at(&bytes(), case.path.clone());
            assert_conflict_preserved(result, &foreign);
        }

        #[test]
        fn sidecar_only_namespace_is_preserved() {
            let case = Case::new();
            case.occupy_namespace();
            let foreign = case.foreign_files(&["-wal", "-shm", "-journal"]);
            assert!(!case.path.exists());
            let result = open_readonly_from_bytes_at(&bytes(), case.path.clone());
            assert_conflict_preserved(result, &foreign);
            assert!(
                !case.path.exists(),
                "a refused namespace must not gain a DB"
            );
        }

        #[test]
        fn existing_empty_namespace_is_not_reused() {
            let case = Case::new();
            case.occupy_namespace();
            // Panic closures pin the rejection to the directory layer: a future
            // implementation that reused the existing directory would reach one
            // of these seams and fail here instead of passing on an unrelated
            // earlier error.
            let result = open_readonly_from_bytes_with(
                &bytes(),
                case.path.clone(),
                |_| panic!("namespace collision must fail before file creation"),
                |_, _| panic!("namespace collision must fail before writing"),
                |_| panic!("namespace collision must fail before syncing"),
                |_, _| panic!("namespace collision must fail before opening"),
            );
            let rejected = result.is_err();
            drop(result);
            assert!(case.path.parent().unwrap().is_dir());
            assert!(!case.path.exists());
            assert!(
                rejected,
                "an existing empty directory is not an owned namespace"
            );
        }

        #[test]
        fn failed_file_create_preserves_foreign_sidecars() {
            let case = Case::new();
            case.occupy_namespace();
            std::fs::create_dir(&case.path).unwrap();
            let foreign = case.foreign_files(&["-wal", "-shm", "-journal"]);
            let result = open_readonly_from_bytes_at(&bytes(), case.path.clone());
            assert_conflict_preserved(result, &foreign);
            assert!(
                case.path.is_dir(),
                "the conflicting DB directory must survive"
            );
        }

        #[test]
        fn file_collision_after_namespace_acquisition_is_preserved() {
            let case = Case::new();
            let mut foreign = Vec::new();
            let result = open_readonly_from_bytes_with(
                &bytes(),
                case.path.clone(),
                |path| {
                    assert_eq!(path, case.path.as_path());
                    foreign = case.foreign_files(&["", "-wal", "-shm", "-journal"]);
                    create_temp_db_file(path)
                },
                |file, bytes| file.write_all(bytes),
                std::fs::File::sync_all,
                |path, flags| Connection::open_with_flags(path, flags),
            );
            assert_conflict_preserved(result, &foreign);
        }

        #[test]
        fn file_create_error_after_namespace_acquisition_preserves_sidecars() {
            let case = Case::new();
            let mut foreign = Vec::new();
            let result = open_readonly_from_bytes_with(
                &bytes(),
                case.path.clone(),
                |path: &Path| {
                    // Deterministic real OS create failure, not a permission/umask race.
                    std::fs::create_dir(path)?;
                    foreign = case.foreign_files(&["-wal", "-shm", "-journal"]);
                    create_temp_db_file(path)
                },
                |file, bytes| file.write_all(bytes),
                std::fs::File::sync_all,
                |path, flags| Connection::open_with_flags(path, flags),
            );
            assert_conflict_preserved(result, &foreign);
            assert!(case.path.is_dir(), "a failed create never owns the DB path");
        }
        fn group_path(path: &Path, suffix: &str) -> PathBuf {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            PathBuf::from(name)
        }

        fn assert_group_removed(path: &Path) {
            for suffix in ["", "-wal", "-shm", "-journal"] {
                assert!(
                    !group_path(path, suffix).exists(),
                    "owned SQLite file remained: {suffix}"
                );
            }
        }

        fn assert_copy_removed(path: &Path) {
            assert_group_removed(path);
            assert!(
                !path.parent().unwrap().exists(),
                "the empty owned namespace must also be removed"
            );
        }

        fn create_non_delete_sharing_file(path: &Path) -> std::io::Result<std::fs::File> {
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // A real handle that denies deletion: default File sharing would let a
                // wrong cleanup-before-close order pass on Windows.
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .share_mode(0)
                    .open(path)
            }
            #[cfg(not(windows))]
            create_temp_db_file(path)
        }

        fn assert_io_failure_cleanup(fail_sync: bool) {
            let case = Case::new();
            let input = bytes();
            let message = if fail_sync {
                "injected sync failure"
            } else {
                "injected partial-write failure"
            };
            let result = open_readonly_from_bytes_with(
                &input,
                case.path.clone(),
                create_non_delete_sharing_file,
                |file, bytes| {
                    file.write_all(if fail_sync { bytes } else { &bytes[..32] })?;
                    // These sidecars belong to the newly acquired copy namespace.
                    case.foreign_files(&["-wal", "-shm", "-journal"]);
                    #[cfg(windows)]
                    assert!(
                        std::fs::remove_file(&case.path).is_err(),
                        "the live test handle must actually prevent deletion"
                    );
                    if fail_sync {
                        Ok(())
                    } else {
                        Err(std::io::Error::other(message))
                    }
                },
                |file| {
                    assert!(fail_sync, "sync must not run after a write error");
                    file.sync_all()?;
                    #[cfg(windows)]
                    assert!(std::fs::remove_file(&case.path).is_err());
                    // The reported sync error is injected, not an OS disk/sync fault.
                    // Handle liveness and close-before-cleanup checks use real OS I/O.
                    Err(std::io::Error::other(message))
                },
                |_, _| panic!("SQLite open must not run after a write/sync error"),
            );
            let error = result.err().expect("injected I/O failure must propagate");
            assert_eq!(error, message);
            assert_copy_removed(&case.path);
        }

        #[test]
        fn partial_write_failure_closes_handle_before_cleanup() {
            assert_io_failure_cleanup(false);
        }

        #[test]
        fn sync_failure_closes_handle_before_cleanup() {
            assert_io_failure_cleanup(true);
        }

        #[test]
        fn namespace_file_is_preserved() {
            let case = Case::new();
            let namespace = case.path.parent().unwrap().to_path_buf();
            let contents = b"foreign namespace file".to_vec();
            std::fs::write(&namespace, &contents).unwrap();
            let result = open_readonly_from_bytes_at(&bytes(), case.path.clone());
            assert_conflict_preserved(result, &[(namespace, contents)]);
        }

        #[test]
        fn missing_parent_is_not_created_recursively() {
            let case = Case::new();
            let missing = case.root.join("missing");
            let result =
                open_readonly_from_bytes_at(&bytes(), missing.join("copy").join("snapshot.db"));
            assert!(result.is_err());
            drop(result);
            assert!(!missing.exists());
        }

        #[test]
        fn sqlite_open_error_cleans_owned_files_and_directory() {
            let case = Case::new();
            let mut reached_open = false;
            let result = open_readonly_from_bytes_with(
                &bytes(),
                case.path.clone(),
                create_non_delete_sharing_file,
                |file, bytes| {
                    file.write_all(bytes)?;
                    case.foreign_files(&["-wal", "-shm", "-journal"]);
                    Ok(())
                },
                std::fs::File::sync_all,
                |path, flags| {
                    reached_open = true;
                    assert!(path.is_file());
                    assert_eq!(
                        flags,
                        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    );
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::OpenOptionsExt;
                        // The non-sharing write handle must already be closed here.
                        let file = std::fs::OpenOptions::new()
                            .read(true)
                            .share_mode(0)
                            .open(path)
                            .unwrap();
                        drop(file);
                    }
                    Err(rusqlite::Error::InvalidQuery)
                },
            );
            assert!(reached_open);
            let error = result
                .err()
                .expect("injected SQLite open failure must propagate");
            assert_eq!(error, rusqlite::Error::InvalidQuery.to_string());
            assert_copy_removed(&case.path);
        }

        #[test]
        fn malformed_snapshot_query_error_cleans_owned_files_and_directory() {
            let case = Case::new();
            let mut sqlite_opened = false;
            let result = (|| -> Result<bool, ProviderError> {
                let result = open_readonly_from_bytes_with(
                    b"not a SQLite database",
                    case.path.clone(),
                    create_temp_db_file,
                    |file, bytes| {
                        file.write_all(bytes)?;
                        case.foreign_files(&["-wal", "-shm", "-journal"]);
                        Ok(())
                    },
                    std::fs::File::sync_all,
                    |path, flags| {
                        let conn = Connection::open_with_flags(path, flags)?;
                        sqlite_opened = true;
                        Ok(conn)
                    },
                );
                let db = result.map_err(ProviderError::StructuralFatal)?;
                table_exists(&db.conn, "session")
            })();
            assert!(
                sqlite_opened,
                "this must cover a query failure, not SQLite open"
            );
            assert!(matches!(
                &result,
                Err(ProviderError::StructuralFatal(message))
                    if message.starts_with("OpenCode SQLite schema query failed:")
            ));
            drop(result);
            assert_copy_removed(&case.path);
        }

        #[test]
        fn successful_copy_preserves_read_policy_and_removes_directory() {
            let case = Case::new();
            let db = open_readonly_from_bytes_at(&bytes(), case.path.clone()).unwrap();
            assert_ne!(case.path.parent().unwrap(), case.root);
            assert_eq!(
                db.conn
                    .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1000
            );
            assert!(
                db.conn.is_autocommit(),
                "OpenCode must not gain a new transaction policy"
            );
            assert!(
                db.conn
                    .execute("CREATE TABLE forbidden_write (value INTEGER)", [])
                    .is_err(),
                "the private copy must still be opened read-only"
            );
            #[cfg(windows)]
            assert!(
                std::fs::remove_file(&case.path).is_err(),
                "a live SQLite connection must actually prevent deletion"
            );
            drop(db);
            assert_copy_removed(&case.path);
        }

        #[test]
        fn unknown_entries_survive_owned_group_cleanup() {
            let case = Case::new();
            let db = open_readonly_from_bytes_at(&bytes(), case.path.clone()).unwrap();
            let directory = case.path.parent().unwrap();
            let unknown = directory.join("unrelated.bin");
            let unknown_dir = directory.join("unrelated-directory");
            std::fs::write(&unknown, b"keep unknown bytes").unwrap();
            std::fs::create_dir(&unknown_dir).unwrap();
            let nested = unknown_dir.join("keep.bin");
            std::fs::write(&nested, b"keep nested bytes").unwrap();
            drop(db);
            assert_group_removed(&case.path);
            assert_eq!(std::fs::read(&unknown).unwrap(), b"keep unknown bytes");
            assert_eq!(std::fs::read(&nested).unwrap(), b"keep nested bytes");
            assert!(
                directory.is_dir(),
                "unknown entries must prevent directory removal"
            );
        }

        fn marker_bytes(marker: i64) -> Vec<u8> {
            let fixture = Case::new();
            let path = fixture.root.join("source.db");
            {
                let writer = Connection::open(&path).unwrap();
                writer
                    .execute_batch(
                        "PRAGMA journal_mode=WAL; CREATE TABLE ownership_marker (value INTEGER);",
                    )
                    .unwrap();
                writer
                    .execute("INSERT INTO ownership_marker VALUES (?1)", [marker])
                    .unwrap();
            }
            // A closed synthetic WAL-mode database, with its committed pages in main.
            std::fs::read(&path).unwrap()
        }

        fn read_marker(db: &TempDb) -> i64 {
            db.conn
                .query_row("SELECT value FROM ownership_marker", [], |row| row.get(0))
                .unwrap()
        }

        #[test]
        fn a_second_open_cannot_damage_a_live_copy_or_its_sidecars() {
            let case = Case::new();
            let input = marker_bytes(7);
            let first = open_readonly_from_bytes_at(&input, case.path.clone()).unwrap();
            assert_eq!(read_marker(&first), 7);
            let owned: Vec<_> = ["", "-wal", "-shm"]
                .iter()
                .map(|suffix| {
                    let path = group_path(&case.path, suffix);
                    let contents = std::fs::read(&path).unwrap();
                    (path, contents)
                })
                .collect();
            let second = open_readonly_from_bytes_at(&input, case.path.clone());
            assert_conflict_preserved(second, &owned);
            assert_eq!(read_marker(&first), 7);
            drop(first);
            assert_copy_removed(&case.path);
        }

        #[test]
        fn concurrent_copies_have_independent_directories_contents_and_lifetimes() {
            let case = Case::new();
            let first_bytes = marker_bytes(11);
            let second_bytes = marker_bytes(22);
            let start = std::sync::Barrier::new(2);
            let (first, second) = std::thread::scope(|scope| {
                let first = scope.spawn(|| {
                    start.wait();
                    open_readonly_from_bytes_at(&first_bytes, temp_db_path(&case.root)).unwrap()
                });
                let second = scope.spawn(|| {
                    start.wait();
                    open_readonly_from_bytes_at(&second_bytes, temp_db_path(&case.root)).unwrap()
                });
                (first.join().unwrap(), second.join().unwrap())
            });
            let first_path = first.temp_path().to_path_buf();
            let second_path = second.temp_path().to_path_buf();
            assert_ne!(first_path, second_path);
            assert_ne!(first_path.parent(), second_path.parent());
            assert_eq!(read_marker(&first), 11);
            assert_eq!(read_marker(&second), 22);
            drop(first);
            assert_copy_removed(&first_path);
            assert!(second_path.is_file());
            assert_eq!(read_marker(&second), 22);
            drop(second);
            assert_copy_removed(&second_path);
        }

        #[cfg(unix)]
        #[test]
        fn unix_copy_directory_and_file_have_no_group_or_other_access() {
            use std::os::unix::fs::PermissionsExt;
            let case = Case::new();
            let db = open_readonly_from_bytes_at(&bytes(), case.path.clone()).unwrap();
            for path in [case.path.as_path(), case.path.parent().unwrap()] {
                // Stricter inherited umasks may remove owner bits; never change umask
                // in this shared test process or claim exact owner bits under all masks.
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
                    0
                );
            }
            drop(db);
            assert_copy_removed(&case.path);
        }

        #[cfg(unix)]
        #[test]
        fn unix_existing_directory_symlinks_are_not_followed() {
            use std::os::unix::fs::symlink;
            for target_exists in [true, false] {
                let case = Case::new();
                let target = case.root.join("foreign-directory");
                if target_exists {
                    std::fs::create_dir(&target).unwrap();
                }
                let namespace = case.path.parent().unwrap();
                symlink(&target, namespace).unwrap();
                let foreign = if target_exists {
                    case.foreign_files(&["", "-wal", "-shm", "-journal"])
                } else {
                    Vec::new()
                };
                let result = open_readonly_from_bytes_at(&bytes(), case.path.clone());
                assert_conflict_preserved(result, &foreign);
                assert!(
                    std::fs::symlink_metadata(namespace)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
                assert_eq!(target.exists(), target_exists);
            }
        }

        #[cfg(unix)]
        #[test]
        fn unix_file_symlinks_after_namespace_acquisition_are_preserved() {
            use std::os::unix::fs::symlink;
            for target_exists in [true, false] {
                let case = Case::new();
                let target = case.root.join("foreign-target.db");
                if target_exists {
                    std::fs::write(&target, b"foreign symlink target").unwrap();
                }
                let mut foreign = Vec::new();
                let result = open_readonly_from_bytes_with(
                    &bytes(),
                    case.path.clone(),
                    |path| {
                        symlink(&target, path)?;
                        foreign = case.foreign_files(&["-wal", "-shm", "-journal"]);
                        create_temp_db_file(path)
                    },
                    |file, bytes| file.write_all(bytes),
                    std::fs::File::sync_all,
                    |path, flags| Connection::open_with_flags(path, flags),
                );
                assert_conflict_preserved(result, &foreign);
                assert!(
                    std::fs::symlink_metadata(&case.path)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
                if target_exists {
                    assert_eq!(std::fs::read(&target).unwrap(), b"foreign symlink target");
                } else {
                    assert!(
                        !target.exists(),
                        "exclusive create must not follow a dangling symlink"
                    );
                }
            }
        }
    }

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

    fn assert_wal_temp_copy_cleanup(query: &str, should_fail: bool) {
        let directory =
            std::env::temp_dir().join(format!("asg-opencode-wal-fixture-{}", temp_file_suffix()));
        std::fs::create_dir(&directory).unwrap();
        let directory = TempDirGuard { path: directory };
        let path = directory.path.join("fixture.db");
        let file = create_temp_db_file(&path).unwrap();
        let fixture = TempDbGuard {
            path,
            _directory: directory,
        };
        drop(file);
        let writer = Connection::open(&fixture.path).unwrap();
        writer
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE cleanup_fixture (value INTEGER);
                 INSERT INTO cleanup_fixture VALUES (7);",
            )
            .unwrap();
        drop(writer);
        let bytes = std::fs::read(&fixture.path).unwrap();
        let mut copy_path = None;
        let result = (|| -> rusqlite::Result<i64> {
            let db = open_readonly_from_bytes(&bytes).unwrap();
            let path = db.temp_path().to_path_buf();
            // A real read materializes the private WAL/SHM files.
            db.conn
                .query_row("SELECT value FROM cleanup_fixture", [], |row| {
                    row.get::<_, i64>(0)
                })?;
            for suffix in ["-wal", "-shm"] {
                let mut sidecar = path.as_os_str().to_os_string();
                sidecar.push(suffix);
                assert!(std::path::Path::new(&sidecar).exists());
            }
            #[cfg(windows)]
            assert!(
                std::fs::remove_file(&path).is_err(),
                "the live SQLite connection must prevent premature deletion"
            );
            copy_path = Some(path);
            // The error case returns while TempDb is still a local owner.
            db.conn.query_row(query, [], |row| row.get(0))
        })();
        assert_eq!(result.is_err(), should_fail);
        let path = copy_path.unwrap();
        let mut remaining = Vec::new();
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut owned_file = path.as_os_str().to_os_string();
            owned_file.push(suffix);
            let owned_file = std::path::Path::new(&owned_file);
            if owned_file.exists() {
                remaining.push(suffix);
                // Do not leave this test's files behind when the assertion fails.
                std::fs::remove_file(owned_file).unwrap();
            }
        }
        assert!(
            remaining.is_empty(),
            "temporary SQLite files leaked: {remaining:?}"
        );
        assert!(
            !path.parent().unwrap().exists(),
            "the private copy directory must be removed after success or query error"
        );
    }

    #[test]
    fn wal_temp_copy_cleans_sidecars_after_success() {
        assert_wal_temp_copy_cleanup("SELECT value FROM cleanup_fixture", false);
    }

    #[test]
    fn wal_temp_copy_cleans_sidecars_after_query_error() {
        assert_wal_temp_copy_cleanup("SELECT missing_column FROM cleanup_fixture", true);
    }

    #[test]
    fn temp_copy_is_unlinked_once_the_connection_goes_out_of_scope() {
        // Regression: the guard used to be returned next to the connection in a
        // tuple, and a tuple binding drops the *later* binding first — so the
        // unlink ran while SQLite still held the file open. Windows refuses that
        // delete and `remove_file`'s error is discarded, so every parse silently
        // leaked its temp copy. `TempDb`'s field order fixes the sequence; this
        // test fails if the pairing is ever unbundled again.
        let db_bytes = create_test_opencode_db();
        let leaked_path = {
            let db = open_readonly_from_bytes(&db_bytes).unwrap();
            let path = db.temp_path().to_path_buf();
            assert!(path.exists(), "temp copy must exist while the db is open");
            path
        };
        assert!(
            !leaked_path.exists(),
            "temp copy must be unlinked after the connection is dropped"
        );
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
    fn parse_extracts_messages_from_opencode_db() {
        let adapter = OpenCodeAdapter::new();
        let db_bytes = create_test_opencode_db();

        struct CountSink {
            count: usize,
        }
        impl CanonicalEventSink for CountSink {
            fn emit_message(
                &mut self,
                _event: MessageEvent<'_>,
            ) -> agent_session_grep_ports::PortResult<()> {
                self.count += 1;
                Ok(())
            }
        }

        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(&db_bytes, &mut sink).unwrap();
        assert_eq!(report.committed, 2);
        assert!(report.session_native_id.is_some());
    }

    /// Create a test OpenCode SQLite database in memory and return its bytes.
    fn create_test_opencode_db() -> Vec<u8> {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT, directory TEXT, time_created INTEGER, time_updated INTEGER);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, data TEXT, time_created INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, data TEXT, time_created INTEGER);
             INSERT INTO session VALUES ('ses_1', 'test', '/work', 1, 2);
             INSERT INTO message VALUES ('msg_1', 'ses_1', '{\"role\":\"user\"}', 1);
             INSERT INTO message VALUES ('msg_2', 'ses_1', '{\"role\":\"assistant\"}', 2);
             INSERT INTO part VALUES ('part_1', 'msg_1', '{\"type\":\"text\",\"text\":\"hello world\"}', 1);
             INSERT INTO part VALUES ('part_2', 'msg_2', '{\"type\":\"text\",\"text\":\"hi there\"}', 2);",
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

    #[test]
    fn parse_passes_noise_shaped_user_text_through_verbatim() {
        // 钉住测试：OpenCode SQLite 没有 system-reminder / AGENTS.md /
        // 环境上下文等注入概念（message/part 的 text 就是消息原文；part 查询
        // 只按 `type='text'` 过滤块类型，不检查文本形状）。形似噪声的文本必须
        // 逐字透传，防止将来把别家格式的过滤规则盲目搬来造成 silent drift。
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT, directory TEXT, time_created INTEGER, time_updated INTEGER);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, data TEXT, time_created INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, data TEXT, time_created INTEGER);
             INSERT INTO session VALUES ('ses_1', 'test', '/work', 1, 2);
             INSERT INTO message VALUES ('msg_1', 'ses_1', '{\"role\":\"user\"}', 1);
             INSERT INTO message VALUES ('msg_2', 'ses_1', '{\"role\":\"user\"}', 2);
             INSERT INTO part VALUES ('part_1', 'msg_1', '{\"type\":\"text\",\"text\":\"<system-reminder>reminder text</system-reminder>\"}', 1);
             INSERT INTO part VALUES ('part_2', 'msg_2', '{\"type\":\"text\",\"text\":\"# AGENTS.md instructions\"}', 2);",
        )
        .unwrap();
        let temp_path = std::env::temp_dir().join(format!("asg-test-{}.db", temp_file_suffix()));
        conn.execute_batch(&format!("VACUUM INTO '{}'", temp_path.display()))
            .unwrap();
        drop(conn);
        let db_bytes = std::fs::read(&temp_path).unwrap();
        let _ = std::fs::remove_file(&temp_path);

        struct TextSink {
            texts: Vec<String>,
        }
        impl CanonicalEventSink for TextSink {
            fn emit_message(
                &mut self,
                event: MessageEvent<'_>,
            ) -> agent_session_grep_ports::PortResult<()> {
                self.texts.push(event.text.to_string());
                Ok(())
            }
        }

        let adapter = OpenCodeAdapter::new();
        let mut sink = TextSink { texts: Vec::new() };
        let report = adapter.parse(&db_bytes, &mut sink).unwrap();
        assert_eq!(report.committed, 2);
        assert_eq!(report.skipped, 0);
        assert_eq!(
            sink.texts,
            vec![
                "<system-reminder>reminder text</system-reminder>".to_string(),
                "# AGENTS.md instructions".to_string(),
            ]
        );
    }
}
