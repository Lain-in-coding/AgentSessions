//! SQLite 适配器：在单个 SQLite 文件上同时落实 `CatalogStore` 与 `SearchIndex`
//! 两个端口（见 ADR-0001：FTS5 单存主线）。
//!
//! 本 crate 是 hexagonal 架构里的 driven adapter——只依赖 domain + ports 的抽象，
//! 把端口契约翻译成具体的 SQLite/FTS5 SQL，绝不反向依赖 application。

mod cas;
mod lease;
mod source_fs;

pub use cas::{cas_activate, read_current, write_current};
pub use lease::WriterLease;
pub use source_fs::{SnapshotFs, capture, read_verified, verify_snapshot};

use agentsessions_domain::{IdKind, StableId};
use agentsessions_ports::{
    CatalogEntry, CatalogStore, PortError, PortResult, SearchHit, SearchIndex,
};
use rusqlite::{Connection, OptionalExtension};
use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Translate adapter failures into the stable port error vocabulary.
///
/// SQLite BUSY/LOCKED conditions are expected writer contention and therefore
/// retryable. The diagnostic is intentionally generic so backend paths or raw
/// SQLite messages cannot escape through the protocol boundary.
fn backend<E: std::fmt::Display + 'static>(e: E) -> PortError {
    let any = &e as &dyn Any;
    if any.downcast_ref::<rusqlite::Error>().is_some_and(|error| {
        matches!(
            error,
            rusqlite::Error::SqliteFailure(sqlite, _)
                if matches!(
                    sqlite.code,
                    rusqlite::ErrorCode::DatabaseBusy
                        | rusqlite::ErrorCode::DatabaseLocked
                )
        )
    }) {
        PortError::WriterBusy("SQLite storage is busy or locked by another writer".into())
    } else {
        PortError::Backend(e.to_string())
    }
}

static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(0);

fn unix_ms() -> PortResult<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(backend)?
        .as_millis();
    i64::try_from(millis).map_err(backend)
}

fn operation_id() -> PortResult<String> {
    let seq = NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed);
    Ok(format!(
        "op_v1_{}_{}_{}",
        unix_ms()?,
        std::process::id(),
        seq
    ))
}

const INDEX_PROJECTION_VERSION: &[u8] = b"sqlite-fts5-v1";

/// 从存储的 catalog payload 投影出可检索正文——rebuild 的规范投影函数。
///
/// 约定：payload 里首个制表符之前是 role 前缀，之后是消息正文；无制表符则整体即正文。
/// 这是 ingest/sync 写入路径（`role\ttext`）的精确逆运算，也兼容切片期 `index`
/// 命令写入的无前缀纯文本（无制表符→整体为正文）。因此仅凭 catalog 即可无损重建
/// FTS 投影，无需依赖可能已损坏/丢失的旧 FTS 内容。
///
/// 已知限制：切片期 `index` 命令若写入本身含制表符的正文，投影会截断到首个制表符
/// 之后——该命令仅供切片期测试，真实数据均经 ingest/sync 带 role 前缀写入。
fn searchable_text(payload: &[u8]) -> String {
    let text = String::from_utf8_lossy(payload);
    match text.split_once('\t') {
        Some((_role, body)) => body.to_string(),
        None => text.into_owned(),
    }
}

fn hash_field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// Union two projections of the same message entity.
///
/// Claude Code copies a conversation's history into the new transcript when a
/// session is resumed or forked, so one message legitimately belongs to several
/// sessions. Everything about such a copy is identical except the `session`
/// back-reference, so that one field becomes a union (`sessions`, sorted, with
/// `session` kept as a single-value alias) and every other field must still
/// agree byte-for-byte. A message whose text, parent, or span depends on which
/// file it came from is a real inconsistency and is still rejected.
fn merge_message_payloads(wire: &str, left: &[u8], right: &[u8]) -> PortResult<Vec<u8>> {
    let parse = |bytes: &[u8]| -> PortResult<serde_json::Map<String, serde_json::Value>> {
        match serde_json::from_slice::<serde_json::Value>(bytes) {
            Ok(serde_json::Value::Object(map)) => Ok(map),
            // Slice-era rows hold bare text rather than canonical JSON. Those
            // cannot be reconciled field by field, so the conflict stands.
            _ => Err(PortError::Backend(format!(
                "message {wire} has conflicting projections across sources"
            ))),
        }
    };

    let left_map = parse(left)?;
    let right_map = parse(right)?;

    let mut sessions: BTreeSet<String> = BTreeSet::new();
    // Spans are keyed by contributing document because the same message text
    // sits at different byte offsets in each file that carries it. Keying by
    // document also makes a re-sync idempotent: the same file always maps to
    // the same entry rather than appending a duplicate.
    let mut spans: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for map in [&left_map, &right_map] {
        if let Some(session) = map.get("session").and_then(|v| v.as_str()) {
            sessions.insert(session.to_string());
        }
        if let Some(list) = map.get("sessions").and_then(|v| v.as_array()) {
            for entry in list {
                if let Some(session) = entry.as_str() {
                    sessions.insert(session.to_string());
                }
            }
        }
        if let Some(list) = map.get("spans").and_then(|v| v.as_array()) {
            for entry in list {
                let Some(document) = entry.get("document").and_then(|v| v.as_str()) else {
                    return Err(PortError::Backend(format!(
                        "message {wire} has a span without a document reference"
                    )));
                };
                spans.insert(document.to_string(), entry.clone());
            }
        }
    }

    // A row written before spans carried document attribution has only the
    // singular `span`. It cannot be keyed by document, so it is kept verbatim as
    // the alias rather than dropped — losing it would silently downgrade the
    // evidence for that message from byte precision to unknown.
    let legacy_span = [&left_map, &right_map]
        .into_iter()
        .find_map(|map| map.get("span").filter(|value| value.is_object()).cloned());

    // Only the per-source fields may differ. Comparing every other key in both
    // directions catches a field present on one side and absent on the other,
    // which a one-way comparison would silently accept.
    for (a, b) in [(&left_map, &right_map), (&right_map, &left_map)] {
        for (key, value) in a {
            if matches!(key.as_str(), "session" | "sessions" | "span" | "spans") {
                continue;
            }
            if b.get(key) != Some(value) {
                return Err(PortError::Backend(format!(
                    "message {wire} has conflicting projections across sources"
                )));
            }
        }
    }

    let mut merged = left_map;
    let sessions: Vec<String> = sessions.into_iter().collect();
    merged.insert(
        "session".to_string(),
        sessions
            .first()
            .cloned()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    merged.insert("sessions".to_string(), serde_json::json!(sessions));

    let spans: Vec<serde_json::Value> = spans.into_values().collect();
    // `span` stays as a single-value alias holding the first contributing
    // document's offsets, so evidence assembly written against the pre-union
    // shape keeps reporting byte precision. On a message shared by several
    // files it names one location, not all of them.
    merged.insert(
        "span".to_string(),
        match spans.first() {
            Some(first) => serde_json::json!({
                "start": first.get("start").cloned().unwrap_or(serde_json::Value::Null),
                "end": first.get("end").cloned().unwrap_or(serde_json::Value::Null),
            }),
            None => legacy_span.unwrap_or(serde_json::Value::Null),
        },
    );
    merged.insert("spans".to_string(), serde_json::json!(spans));
    serde_json::to_vec(&serde_json::Value::Object(merged)).map_err(backend)
}

/// Union two projections of the same session container entity.
///
/// One logical session is routinely split across many transcript files, so each
/// source contributes only the members it actually carries. Merging appends the
/// right side's new members after the left side's and unions the contributing
/// documents. Both inputs must be canonical session JSON; a malformed stored
/// payload is a real inconsistency and is reported rather than silently
/// discarded.
///
/// Determinism comes from the caller: `sync` rejects duplicate source paths and
/// the CLI passes sources in a fixed order, so the same corpus yields the same
/// merged bytes and an unchanged re-sync still registers as a content-level
/// no-op.
fn merge_session_payloads(wire: &str, left: &[u8], right: &[u8]) -> PortResult<Vec<u8>> {
    fn parse(wire: &str, bytes: &[u8]) -> PortResult<serde_json::Value> {
        serde_json::from_slice(bytes).map_err(|error| {
            PortError::Backend(format!(
                "session {wire} payload is not canonical JSON: {error}"
            ))
        })
    }

    let left_value = parse(wire, left)?;
    let right_value = parse(wire, right)?;

    // Member order is load-bearing: readers treat a member's position in this
    // array as its in-session sequence number, and branch selection picks the
    // highest-sequence non-sidechain leaf. Sorting by wire id would therefore
    // scramble conversation order for real provider-native ids, so the union is
    // append-only. `documents` carries no such meaning and is sorted.
    let mut members: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut documents: BTreeSet<String> = BTreeSet::new();

    for value in [&left_value, &right_value] {
        // `document` (single) is the pre-union shape; `documents` (array) is what
        // a merged payload carries. Accept both so a store written by an older
        // binary merges cleanly instead of losing its attribution.
        if let Some(document) = value.get("document").and_then(|v| v.as_str()) {
            documents.insert(document.to_string());
        }
        if let Some(list) = value.get("documents").and_then(|v| v.as_array()) {
            for entry in list {
                if let Some(document) = entry.as_str() {
                    documents.insert(document.to_string());
                }
            }
        }
        let list = value
            .get("messages")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                PortError::Backend(format!("session {wire} payload lacks a messages array"))
            })?;
        for entry in list {
            let member = entry.as_str().ok_or_else(|| {
                PortError::Backend(format!("session {wire} has a non-string message member"))
            })?;
            if seen.insert(member.to_string()) {
                members.push(member.to_string());
            }
        }
    }

    let documents: Vec<String> = documents.into_iter().collect();
    let merged = serde_json::json!({
        // `document` stays as a single-value alias for the first contributing
        // document so readers written against the pre-union shape keep working.
        // On a multi-document session it names one contributor, not all of them.
        "document": documents.first().cloned(),
        "documents": documents,
        "messages": members,
    });
    serde_json::to_vec(&merged).map_err(backend)
}

/// Canonicalize and fingerprint one generation change set.
///
/// Sorting by wire ID makes the digest independent of discovery order. Duplicate IDs and
/// upsert/delete overlap are rejected so the journal always describes an unambiguous set.
fn batch_manifest(
    upserts: &[(StableId, Vec<u8>, String)],
    deletes: &[StableId],
) -> PortResult<(Vec<String>, Vec<String>, String)> {
    let mut ordered_upserts: Vec<_> = upserts.iter().collect();
    ordered_upserts.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    let mut ordered_deletes: Vec<_> = deletes.iter().collect();
    ordered_deletes.sort_by(|a, b| a.as_str().cmp(b.as_str()));

    let upsert_ids: Vec<String> = ordered_upserts
        .iter()
        .map(|(id, _, _)| id.as_str().to_string())
        .collect();
    let delete_ids: Vec<String> = ordered_deletes
        .iter()
        .map(|id| id.as_str().to_string())
        .collect();
    if upsert_ids.windows(2).any(|w| w[0] == w[1]) || delete_ids.windows(2).any(|w| w[0] == w[1]) {
        return Err(PortError::Backend(
            "index batch contains duplicate entity ids".into(),
        ));
    }
    if upsert_ids
        .iter()
        .any(|id| delete_ids.binary_search(id).is_ok())
    {
        return Err(PortError::Backend(
            "index batch cannot upsert and delete the same entity".into(),
        ));
    }

    let mut hasher = blake3::Hasher::new();
    hash_field(&mut hasher, b"agentsessions-index-batch-v1");
    hash_field(&mut hasher, INDEX_PROJECTION_VERSION);
    for (id, payload, text) in ordered_upserts {
        hash_field(&mut hasher, b"upsert");
        hash_field(&mut hasher, id.as_str().as_bytes());
        hash_field(&mut hasher, payload);
        hash_field(&mut hasher, text.as_bytes());
    }
    for id in ordered_deletes {
        hash_field(&mut hasher, b"delete");
        hash_field(&mut hasher, id.as_str().as_bytes());
    }
    Ok((
        upsert_ids,
        delete_ids,
        hasher.finalize().to_hex().to_string(),
    ))
}

/// Durable outbox row for an index-generation operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexBatch {
    pub operation_id: String,
    pub base_generation: u64,
    pub target_generation: u64,
    pub state: String,
    pub operation_digest: String,
    pub upsert_ids: Vec<String>,
    pub delete_ids: Vec<String>,
    pub durable_point: String,
    pub error_code: Option<String>,
}

/// Handle returned after an outbox intent reaches its first durable point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingIndexBatch {
    pub operation_id: String,
    pub base_generation: u64,
    pub target_generation: u64,
    pub operation_digest: String,
}

/// 一个 source 完整成功 scan 后的全部消息条目。
///
/// `sync`/`ingest` 为每个只读源构造一个 `SourceBatch`，store 据此推导：本次出现的
/// message id 是 upsert；该源上次成功 scan 有、本次没有的 id 是 tombstone（删除）。
/// 只有整批全部源都 stage 成功后才提交；missing/tombstone 只能由完整成功 scan 确认。
pub struct SourceBatch {
    /// 该源的稳定标识（当前用其只读路径字符串）。
    pub source_path: String,
    /// 本次 scan 得到的全部 (message id, catalog payload, 索引正文)。
    ///
    /// 自 v6 起，条目不限于消息：组合根把该源派生的 session（`ses_v1_*`）与
    /// document（`doc_v1_*`）目录实体放进同一批 entries，随消息走同一事务提交、
    /// 同一 membership/tombstone 推导——源消失时容器实体随消息一起退役。
    pub entries: Vec<(StableId, Vec<u8>, String)>,
}

/// 提交时落库的 source→message membership 快照（内部使用）。
struct SourceMembership {
    source_path: String,
    /// 从 entries 推导的文档实体 wire id（首个 `Document` kind 条目）；
    /// 落库到 `source_membership.document_id` 供来源归属查询，无则 NULL。
    document_id: Option<String>,
    message_ids: Vec<String>,
}

/// SQLite 支撑的存储：catalog 表存规范化实体负载，FTS5 表提供全文检索。
///
/// 单连接 + `RefCell` 内部可变：端口 trait 以 `&self` 取用，而 rusqlite 的写操作
/// 需要可变连接。首个垂直切片单线程使用，故不引入连接池。
///
/// 可选持有 [`WriterLease`]：经 [`SqliteStore::open_for_write`] 打开时，
/// lease 与 store 同生命周期，Drop store 时释放 data-root 写锁。
pub struct SqliteStore {
    conn: RefCell<Connection>,
    /// 写入路径持有的 data-root 独占 lease；只读打开时为 None。
    _lease: Option<WriterLease>,
}

impl SqliteStore {
    /// 只读打开（不抢 writer lease）。供 search/get/doctor 等读路径。
    pub fn open(path: &str) -> PortResult<Self> {
        let conn = Connection::open(path).map_err(backend)?;
        Self::init(&conn)?;
        Ok(SqliteStore {
            conn: RefCell::new(conn),
            _lease: None,
        })
    }

    /// 写入路径打开：先在 db 所在目录获取 data-root writer lease，再打开库。
    ///
    /// 若另一进程已持 lease，立即失败（不阻塞）。lease 随本 store 存活，
    /// Drop 时释放，以维持每个 data root 单写者不变量。
    pub fn open_for_write(path: &str) -> PortResult<Self> {
        let db_path = Path::new(path);
        let data_root = db_path.parent().unwrap_or_else(|| Path::new("."));
        let lease = WriterLease::try_acquire(data_root)?;
        let conn = Connection::open(path).map_err(backend)?;
        Self::init(&conn)?;
        let store = SqliteStore {
            conn: RefCell::new(conn),
            _lease: Some(lease),
        };
        // lease 已到手，当前进程是唯一写者；安全收敛上次崩溃留下的无副作用 intent。
        store.recover_interrupted()?;
        Ok(store)
    }

    /// 打开内存存储（测试用，无 lease）。
    pub fn open_in_memory() -> PortResult<Self> {
        let conn = Connection::open_in_memory().map_err(backend)?;
        Self::init(&conn)?;
        Ok(SqliteStore {
            conn: RefCell::new(conn),
            _lease: None,
        })
    }

    /// 打开并把 schema 迁移到当前版本，为版本化 migration 与可重建索引奠基。
    fn init(conn: &Connection) -> PortResult<()> {
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(backend)?;
        Self::migrate(conn)
    }

    /// 按 `PRAGMA user_version` 门控的顺序迁移。
    ///
    /// 每次 schema 变更追加一个版本步骤并递增 [`SCHEMA_VERSION`]；旧库打开时
    /// 从其记录的版本逐步升级。`user_version` 是 SQLite 内建的每库整数，
    /// 不占额外表，正是 migration 追踪的标准落点。
    fn migrate(conn: &Connection) -> PortResult<()> {
        let current: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(backend)?;
        if current > SCHEMA_VERSION {
            // 库比本二进制更新——拒绝而非静默降级，避免按旧 schema 误读新数据。
            return Err(PortError::SchemaIncompatible(format!(
                "catalog schema version {current} is newer than supported {SCHEMA_VERSION}; \
                 upgrade agentsessions or rebuild the data root"
            )));
        }
        if current < 1 {
            // v1：catalog（按 id 主键存 payload）+ contentful-id FTS5（id 回带，text 索引）。
            conn.execute_batch(
                "CREATE TABLE catalog (
                     id      TEXT PRIMARY KEY,
                     payload BLOB NOT NULL
                 );
                 CREATE VIRTUAL TABLE fts USING fts5(id UNINDEXED, text);",
            )
            .map_err(backend)?;
        }
        if current < 2 {
            // v2：活动 generation + durable outbox。FTS5 与 catalog 同事务提交，
            // journal 记录 intent、激活结果以及崩溃恢复结论。
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS store_metadata (
                     singleton         INTEGER PRIMARY KEY CHECK(singleton = 1),
                     active_generation INTEGER NOT NULL CHECK(active_generation >= 0)
                 );
                 INSERT OR IGNORE INTO store_metadata(singleton, active_generation)
                 VALUES(1, 0);
                 CREATE TABLE IF NOT EXISTS index_batches (
                     operation_id     TEXT PRIMARY KEY,
                     base_generation  INTEGER NOT NULL,
                     target_generation INTEGER NOT NULL,
                     state            TEXT NOT NULL CHECK(state IN (
                         'building', 'search_built', 'activated', 'aborted',
                         'superseded', 'cleanup_pending'
                     )),
                     operation_digest TEXT NOT NULL,
                     upsert_ids_json  TEXT NOT NULL,
                     delete_ids_json  TEXT NOT NULL,
                     durable_point    TEXT NOT NULL,
                     created_at_ms    INTEGER NOT NULL,
                     committed_at_ms  INTEGER,
                     error_code       TEXT,
                     CHECK(target_generation = base_generation + 1)
                 );
                 CREATE INDEX IF NOT EXISTS index_batches_state
                 ON index_batches(state);",
            )
            .map_err(backend)?;
        }
        if current < 3 {
            // v3：以 wire id 为唯一索引键，隔离稳定性元数据，保证外部 wire round-trip
            // 形成的 Unstable id 也能删除原有 Native/Reconstructed FTS 行。
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS fts_ids (
                     wire_id TEXT PRIMARY KEY,
                     id_json TEXT NOT NULL UNIQUE
                 );",
            )
            .map_err(backend)?;
            let mut stmt = conn.prepare("SELECT id FROM fts").map_err(backend)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(backend)?;
            let mut ids = Vec::new();
            for row in rows {
                let id_json = row.map_err(backend)?;
                let id: StableId = serde_json::from_str(&id_json).map_err(backend)?;
                ids.push((id.as_str().to_string(), id_json));
            }
            drop(stmt);
            for (wire_id, id_json) in ids {
                conn.execute(
                    "INSERT OR REPLACE INTO fts_ids(wire_id, id_json) VALUES(?1, ?2)",
                    rusqlite::params![wire_id, id_json],
                )
                .map_err(backend)?;
            }
        }
        if current < 4 {
            // v4：记录每个 source 最近一次完整成功 scan 的 message membership，
            // 只有完整 scan 成功后才可安全推导删除/tombstone。
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS source_membership (
                     source_path TEXT NOT NULL,
                     message_id  TEXT PRIMARY KEY
                 );
                 CREATE INDEX IF NOT EXISTS source_membership_source
                 ON source_membership(source_path);",
            )
            .map_err(backend)?;
        }
        if current < 5 {
            // v5：空 source scan 也必须留下“已成功扫描”的证据；membership 改为
            // 多对多主键，避免删除一个 source 时误删仍被其它 source 引用的实体。
            conn.execute_batch(
                "DROP INDEX IF EXISTS source_membership_source;
                 ALTER TABLE source_membership RENAME TO source_membership_v4;
                 CREATE TABLE source_membership (
                     source_path TEXT NOT NULL,
                     message_id  TEXT NOT NULL,
                     PRIMARY KEY(source_path, message_id)
                 );
                 INSERT INTO source_membership(source_path, message_id)
                 SELECT source_path, message_id FROM source_membership_v4;
                 DROP TABLE source_membership_v4;
                 CREATE INDEX source_membership_source
                 ON source_membership(source_path);
                 CREATE TABLE IF NOT EXISTS source_scans (
                     source_path   TEXT PRIMARY KEY,
                     scanned_at_ms INTEGER NOT NULL
                 );",
            )
            .map_err(backend)?;
        }
        if current < 6 {
            // v6：source_membership 增加可空 document_id——记录各 source 所属文档实体的
            // wire id，使 source 消失的 tombstone 清理能同步退役其 session/document 目录行。
            // 旧行保持 NULL（v6 前的 membership 无文档归属信息）。
            conn.execute_batch("ALTER TABLE source_membership ADD COLUMN document_id TEXT;")
                .map_err(backend)?;
        }
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
            .map_err(backend)?;
        Ok(())
    }

    /// 当前存储读回的 schema 版本（供 doctor/诊断）。
    pub fn schema_version(&self) -> PortResult<i64> {
        self.conn
            .borrow()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(backend)
    }

    /// 以 durable outbox 包裹一批 upsert，再原子提交 catalog + FTS + generation。
    ///
    /// 阶段一先持久化 intent；阶段二在单个 SQLite 事务中应用所有实体并激活目标
    /// generation。任一实体失败都会回滚整批数据；若进程在两阶段之间终止，下一次
    /// 写打开会把无副作用的 `building` intent 标记为 `aborted`。
    pub fn commit_batch(&self, entries: &[(StableId, Vec<u8>, String)]) -> PortResult<()> {
        self.commit_batch_if_changed(entries).map(|_| ())
    }

    /// Durable batch commit that reports whether a new generation was activated.
    ///
    /// `false` means every catalog payload and indexed text already matched the batch;
    /// no outbox row or generation was created.
    pub fn commit_batch_if_changed(
        &self,
        entries: &[(StableId, Vec<u8>, String)],
    ) -> PortResult<bool> {
        if entries.is_empty() {
            return Ok(false);
        }
        // Validate the complete change set before the no-op shortcut; duplicate IDs must
        // never be silently accepted just because the first copy is already current.
        batch_manifest(entries, &[])?;
        if self.batch_is_current(entries)? {
            return Ok(false);
        }
        let pending = self.begin_index_batch(entries, &[])?;
        self.commit_index_batch(&pending, entries, &[])?;
        Ok(true)
    }

    fn batch_is_current(&self, entries: &[(StableId, Vec<u8>, String)]) -> PortResult<bool> {
        let conn = self.conn.borrow();
        for (id, payload, text) in entries {
            let catalog_payload: Option<Vec<u8>> = conn
                .query_row(
                    "SELECT payload FROM catalog WHERE id = ?1",
                    [id.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if catalog_payload.as_deref() != Some(payload.as_slice()) {
                return Ok(false);
            }
            let id_json: Option<String> = conn
                .query_row(
                    "SELECT id_json FROM fts_ids WHERE wire_id = ?1",
                    [id.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            let Some(id_json) = id_json else {
                return Ok(false);
            };
            // 非 Message 实体不进 fts 全文表（见 commit_index_batch_with_membership），
            // 其"内容一致"只看 catalog payload 与 fts_ids 身份边车。
            if id.kind() != IdKind::Message {
                continue;
            }
            let indexed_text: Option<String> = conn
                .query_row("SELECT text FROM fts WHERE id = ?1", [&id_json], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(backend)?;
            if indexed_text.as_deref() != Some(text.as_str()) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 以 source membership 为依据提交一批只读源的完整 scan 结果，报告是否推进了 generation。
    ///
    /// 对每个 [`SourceBatch`]：本次出现的 message id 为 upsert；该源上次成功 scan 有、
    /// 本次没有的 id 推导为 tombstone（删除）。catalog/FTS/membership/generation 在
    /// [`commit_index_batch`](Self::commit_index_batch) 的同一事务内提交。若整批 upsert 与
    /// tombstone 都与当前状态一致（内容级 no-op），返回 `false`，不生成新 generation。
    ///
    /// 只有完整成功 scan（所有源都已 stage）才可据此确认 missing/tombstone。
    pub fn commit_source_batches_if_changed(&self, sources: &[SourceBatch]) -> PortResult<bool> {
        // 源路径不得重复，否则 membership 推导有歧义。
        let mut paths: Vec<&str> = sources.iter().map(|s| s.source_path.as_str()).collect();
        paths.sort_unstable();
        if paths.windows(2).any(|w| w[0] == w[1]) {
            return Err(PortError::Backend(
                "source batch contains duplicate source paths".into(),
            ));
        }

        // 汇总所有源的 upsert，先按 wire id 合并跨 source 重叠实体；只有 payload/text
        // 完全相同才允许共享同一实体，避免一次 batch 的重复 id 被拒绝或产生不确定结果。
        // 必须先收集完整 incoming 集合，再基于旧 membership 推导 tombstone；否则推导结果
        // 会依赖 source 输入顺序，并把“从旧 source 移到新 source”的实体同时列入删除与 upsert。
        let scanned_paths: BTreeSet<&str> = paths.into_iter().collect();
        let mut merged: BTreeMap<String, (StableId, Vec<u8>, String)> = BTreeMap::new();
        let mut incoming_ids = BTreeSet::new();
        let mut present_by_source: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for source in sources {
            let present: BTreeSet<&str> = source
                .entries
                .iter()
                .map(|(id, _, _)| id.as_str())
                .collect();
            if present.len() != source.entries.len() {
                return Err(PortError::Backend(
                    "source batch contains duplicate message ids".into(),
                ));
            }
            for (id, payload, text) in &source.entries {
                incoming_ids.insert(id.as_str());
                if let Some((old_id, old_payload, old_text)) = merged.get(id.as_str()) {
                    if old_id != id {
                        return Err(PortError::Backend(format!(
                            "message {} has conflicting identity metadata across sources",
                            id.as_str()
                        )));
                    }
                    if old_payload != payload || old_text != text {
                        // Both entity kinds legitimately differ per source, for
                        // the same underlying reason: neither "one session per
                        // file" nor "one session per message" holds in real
                        // transcripts. A session is often split across many
                        // files, and resume/fork copies a message into the new
                        // session's file. Each source therefore carries only a
                        // partial view, and the union is the whole truth.
                        let union = match id.kind() {
                            IdKind::Session => {
                                merge_session_payloads(id.as_str(), old_payload, payload)?
                            }
                            IdKind::Message => {
                                merge_message_payloads(id.as_str(), old_payload, payload)?
                            }
                            // Documents are content-addressed: identical bytes
                            // give the same id, so a differing payload under one
                            // document id is a real inconsistency.
                            _ => {
                                return Err(PortError::Backend(format!(
                                    "entity {} has conflicting projections across sources",
                                    id.as_str()
                                )));
                            }
                        };
                        merged.insert(id.as_str().to_string(), (id.clone(), union, text.clone()));
                        continue;
                    }
                } else {
                    merged.insert(
                        id.as_str().to_string(),
                        (id.clone(), payload.clone(), text.clone()),
                    );
                }
            }
            present_by_source.insert(source.source_path.as_str(), present);
        }

        // Fold in what the catalog already holds for each shared entity. Without
        // this, syncing a corpus in several batches would make each batch's view
        // overwrite the previous one: a session's stored membership would only
        // reflect the files in the final batch, and a message shared by several
        // sessions would keep only the last session that claimed it.
        for (id, payload, _) in merged.values_mut() {
            let stored = match self.get(id)? {
                // Identical bytes need no merge, and attempting one would force
                // every stored payload to be canonical JSON — including rows
                // written by the pre-container slice path, which are opaque
                // blobs. Re-syncing an unchanged corpus must stay a no-op.
                Some(stored) if stored != *payload => stored,
                _ => continue,
            };
            *payload = match id.kind() {
                IdKind::Session => merge_session_payloads(id.as_str(), &stored, payload)?,
                IdKind::Message => merge_message_payloads(id.as_str(), &stored, payload)?,
                _ => continue,
            };
        }

        let mut deletes = BTreeMap::new();
        for source in sources {
            if !self.source_was_scanned(&source.source_path)? {
                continue;
            }
            let present = present_by_source
                .get(source.source_path.as_str())
                .ok_or_else(|| PortError::Backend("source membership derivation failed".into()))?;
            for prior in self.source_message_ids(&source.source_path)? {
                if !present.contains(prior.as_str())
                    && !incoming_ids.contains(prior.as_str())
                    && !self.message_referenced_by_unscanned_source(&prior, &scanned_paths)?
                {
                    let id = StableId::from_wire(&prior).ok_or_else(|| {
                        PortError::Backend(format!("invalid membership id: {prior}"))
                    })?;
                    deletes.insert(prior, id);
                }
            }
        }
        let upserts: Vec<(StableId, Vec<u8>, String)> = merged.into_values().collect();
        let deletes: Vec<StableId> = deletes.into_values().collect();

        // 校验整体变更集合（重复/交叠即拒绝），再判定是否内容级 no-op。
        batch_manifest(&upserts, &deletes)?;
        if self.source_batches_are_current(sources, &upserts)? {
            return Ok(false);
        }

        let membership: Vec<SourceMembership> = sources
            .iter()
            .map(|s| SourceMembership {
                source_path: s.source_path.clone(),
                // 文档归属从 entries 推导：首个 Document kind 实体的 wire id。
                document_id: s
                    .entries
                    .iter()
                    .find(|(id, _, _)| id.kind() == IdKind::Document)
                    .map(|(id, _, _)| id.as_str().to_string()),
                message_ids: s
                    .entries
                    .iter()
                    .map(|(id, _, _)| id.as_str().to_string())
                    .collect(),
            })
            .collect();
        let pending = self.begin_index_batch(&upserts, &deletes)?;
        self.commit_index_batch_with_membership(&pending, &upserts, &deletes, &membership)?;
        Ok(true)
    }

    /// 读回某源上次成功 scan 记录的 message id 集合（membership）。
    fn source_message_ids(&self, source_path: &str) -> PortResult<Vec<String>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare("SELECT message_id FROM source_membership WHERE source_path = ?1")
            .map_err(backend)?;
        let rows = stmt
            .query_map([source_path], |row| row.get::<_, String>(0))
            .map_err(backend)?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(backend)?);
        }
        Ok(ids)
    }

    fn source_batches_are_current(
        &self,
        sources: &[SourceBatch],
        upserts: &[(StableId, Vec<u8>, String)],
    ) -> PortResult<bool> {
        if !self.batch_is_current(upserts)? {
            return Ok(false);
        }
        for source in sources {
            if !self.source_was_scanned(&source.source_path)? {
                return Ok(false);
            }
            let mut expected: Vec<String> = source
                .entries
                .iter()
                .map(|(id, _, _)| id.as_str().to_string())
                .collect();
            expected.sort_unstable();
            let mut actual = self.source_message_ids(&source.source_path)?;
            actual.sort_unstable();
            if actual != expected {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn source_was_scanned(&self, source_path: &str) -> PortResult<bool> {
        let conn = self.conn.borrow();
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM source_scans WHERE source_path = ?1",
                [source_path],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(exists.is_some())
    }

    fn message_referenced_by_unscanned_source(
        &self,
        message_id: &str,
        scanned_paths: &BTreeSet<&str>,
    ) -> PortResult<bool> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare("SELECT source_path FROM source_membership WHERE message_id = ?1")
            .map_err(backend)?;
        let rows = stmt
            .query_map([message_id], |row| row.get::<_, String>(0))
            .map_err(backend)?;
        for row in rows {
            if !scanned_paths.contains(row.map_err(backend)?.as_str()) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 读回当前活动 generation（v2 起可用）。
    pub fn active_generation(&self) -> PortResult<u64> {
        let conn = self.conn.borrow();
        let g: i64 = conn
            .query_row(
                "SELECT active_generation FROM store_metadata WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(backend)?;
        u64::try_from(g).map_err(backend)
    }

    /// 阶段一（durable intent）：写入一条 `building` outbox 行并提交。
    ///
    /// Durable outbox 状态机的第一个 durable point——在任何 catalog/FTS
    /// 变更落盘之前，先持久化"应该构建什么"（upsert/delete 集合 + digest）。
    /// 此后崩溃，恢复只会看到一条无副作用的 `building` 行并将其 `aborted`。
    ///
    /// `base_generation` 必须等于当前活动 generation，否则说明并发写者抢先推进过
    /// generation，本次 intent 作废（CAS 前置条件）。
    pub fn begin_index_batch(
        &self,
        upserts: &[(StableId, Vec<u8>, String)],
        deletes: &[StableId],
    ) -> PortResult<PendingIndexBatch> {
        let base = self.active_generation()?;
        let target = base
            .checked_add(1)
            .ok_or_else(|| PortError::Backend("generation overflow".into()))?;
        let target_sql = i64::try_from(target).map_err(backend)?;
        let base_sql = i64::try_from(base).map_err(backend)?;
        let op = operation_id()?;
        let (upsert_ids, delete_ids, digest) = batch_manifest(upserts, deletes)?;
        let upsert_json = serde_json::to_string(&upsert_ids).map_err(backend)?;
        let delete_json = serde_json::to_string(&delete_ids).map_err(backend)?;
        let conn = self.conn.borrow();
        conn.execute(
            "INSERT INTO index_batches(
                 operation_id, base_generation, target_generation, state,
                 operation_digest, upsert_ids_json, delete_ids_json,
                 durable_point, created_at_ms
             ) VALUES(?1, ?2, ?3, 'building', ?4, ?5, ?6, 'intent', ?7)",
            rusqlite::params![
                op,
                base_sql,
                target_sql,
                digest,
                upsert_json,
                delete_json,
                unix_ms()?,
            ],
        )
        .map_err(backend)?;
        Ok(PendingIndexBatch {
            operation_id: op,
            base_generation: base,
            target_generation: target,
            operation_digest: digest,
        })
    }

    /// 阶段二（apply + activate）：在单个事务内应用 catalog+FTS 变更、推进活动
    /// generation，并把 outbox 行标记为 `activated`。
    ///
    /// FTS5 单存储下 catalog 与 FTS 同事务域提交，故 `catalog_committed` 与
    /// `search_built` 合为一个 durable point；generation 切换也在同一事务，
    /// 因此不存在"搜索已建但未激活"的中间崩溃窗口。
    ///
    /// CAS 前置：`active_generation == pending.base_generation`。不匹配则拒绝，
    /// 防止旧基线覆盖更新的同步结果。
    pub fn commit_index_batch(
        &self,
        pending: &PendingIndexBatch,
        upserts: &[(StableId, Vec<u8>, String)],
        deletes: &[StableId],
    ) -> PortResult<()> {
        self.commit_index_batch_with_membership(pending, upserts, deletes, &[])
    }

    /// 事务内校验 pending 句柄仍可安全激活：generation CAS + intent 行状态 + manifest 匹配。
    ///
    /// 从 [`commit_index_batch_with_membership`](Self::commit_index_batch_with_membership) 抽出，
    /// 使 rebuild 路径复用同一套“不覆盖更新基线 / 不与 durable intent 分歧”的前置检查。
    fn verify_pending_in_tx(
        tx: &rusqlite::Transaction<'_>,
        pending: &PendingIndexBatch,
        upserts: &[(StableId, Vec<u8>, String)],
        deletes: &[StableId],
    ) -> PortResult<()> {
        // CAS：活动 generation 必须仍等于 intent 记录的 base，否则中止本批次。
        let current: i64 = tx
            .query_row(
                "SELECT active_generation FROM store_metadata WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if current as u64 != pending.base_generation {
            return Err(PortError::Backend(format!(
                "generation CAS failed: active {current} != expected base {}",
                pending.base_generation
            )));
        }
        // intent 行及其完整 manifest 必须仍与 pending handle 匹配。
        let (
            declared_base,
            declared_target,
            state,
            declared_digest,
            declared_upserts,
            declared_deletes,
        ): (i64, i64, String, String, String, String) = tx
            .query_row(
                "SELECT base_generation, target_generation, state, operation_digest,
                        upsert_ids_json, delete_ids_json
                 FROM index_batches WHERE operation_id = ?1",
                [&pending.operation_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .map_err(backend)?;
        if state != "building" {
            return Err(PortError::Backend(format!(
                "index batch {} is in state {state}, expected building",
                pending.operation_id
            )));
        }
        let (actual_upsert_ids, actual_delete_ids, actual_digest) =
            batch_manifest(upserts, deletes)?;
        let actual_upserts = serde_json::to_string(&actual_upsert_ids).map_err(backend)?;
        let actual_deletes = serde_json::to_string(&actual_delete_ids).map_err(backend)?;
        let handle_matches = declared_base == pending.base_generation as i64
            && declared_target == pending.target_generation as i64
            && declared_digest == pending.operation_digest;
        if !handle_matches
            || declared_digest != actual_digest
            || declared_upserts != actual_upserts
            || declared_deletes != actual_deletes
        {
            return Err(PortError::Backend(format!(
                "index batch {} payload does not match durable intent",
                pending.operation_id
            )));
        }
        Ok(())
    }

    fn commit_index_batch_with_membership(
        &self,
        pending: &PendingIndexBatch,
        upserts: &[(StableId, Vec<u8>, String)],
        deletes: &[StableId],
        membership: &[SourceMembership],
    ) -> PortResult<()> {
        let mut conn = self.conn.borrow_mut();
        let tx = conn.transaction().map_err(backend)?;
        Self::verify_pending_in_tx(&tx, pending, upserts, deletes)?;

        for (id, payload, text) in upserts {
            tx.execute(
                "INSERT INTO catalog(id, payload) VALUES(?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
                rusqlite::params![id.as_str(), payload],
            )
            .map_err(backend)?;
            let id_json = serde_json::to_string(id).map_err(backend)?;
            tx.execute(
                "DELETE FROM fts
                 WHERE id = ?1 OR id = (SELECT id_json FROM fts_ids WHERE wire_id = ?2)",
                rusqlite::params![id_json, id.as_str()],
            )
            .map_err(backend)?;
            tx.execute("DELETE FROM fts_ids WHERE wire_id = ?1", [id.as_str()])
                .map_err(backend)?;
            // 只有 Message 实体进入 fts 全文表——session/document 是检索容器实体，
            // 索引其正文会让搜索命中重复计数。fts_ids 身份边车则对所有 kind 保留：
            // 它保真 kind+stability，rebuild 依赖它恢复非 Unstable 身份（见 rebuild_index）。
            if id.kind() == IdKind::Message {
                tx.execute(
                    "INSERT INTO fts(id, text) VALUES(?1, ?2)",
                    rusqlite::params![id_json, text],
                )
                .map_err(backend)?;
            }
            tx.execute(
                "INSERT INTO fts_ids(wire_id, id_json) VALUES(?1, ?2)",
                rusqlite::params![id.as_str(), id_json],
            )
            .map_err(backend)?;
        }
        for id in deletes {
            tx.execute("DELETE FROM catalog WHERE id = ?1", [id.as_str()])
                .map_err(backend)?;
            let id_json = serde_json::to_string(id).map_err(backend)?;
            tx.execute(
                "DELETE FROM fts
                 WHERE id = ?1 OR id = (SELECT id_json FROM fts_ids WHERE wire_id = ?2)",
                rusqlite::params![id_json, id.as_str()],
            )
            .map_err(backend)?;
            tx.execute("DELETE FROM fts_ids WHERE wire_id = ?1", [id.as_str()])
                .map_err(backend)?;
        }

        for source in membership {
            tx.execute(
                "DELETE FROM source_membership WHERE source_path = ?1",
                [&source.source_path],
            )
            .map_err(backend)?;
            for message_id in &source.message_ids {
                tx.execute(
                    "INSERT INTO source_membership(source_path, message_id, document_id)
                     VALUES(?1, ?2, ?3)",
                    rusqlite::params![&source.source_path, message_id, &source.document_id],
                )
                .map_err(backend)?;
            }
            tx.execute(
                "INSERT INTO source_scans(source_path, scanned_at_ms) VALUES(?1, ?2)
                 ON CONFLICT(source_path) DO UPDATE SET scanned_at_ms = excluded.scanned_at_ms",
                rusqlite::params![&source.source_path, unix_ms()?],
            )
            .map_err(backend)?;
        }

        tx.execute(
            "UPDATE store_metadata SET active_generation = ?1 WHERE singleton = 1",
            [pending.target_generation as i64],
        )
        .map_err(backend)?;
        tx.execute(
            "UPDATE index_batches
             SET state = 'activated', durable_point = 'activated', committed_at_ms = ?2
             WHERE operation_id = ?1",
            rusqlite::params![pending.operation_id, unix_ms()?],
        )
        .map_err(backend)?;

        tx.commit().map_err(backend)?;
        Ok(())
    }

    /// 从权威 catalog 全量重投影 FTS 索引，通过 durable outbox + generation 保证
    /// 重建期崩溃不污染当前活动 generation。
    ///
    /// catalog 是内容的权威事实源，`fts` 搜索索引是可重建的派生投影（ADR-0001）。本方法：
    /// 1. 以 catalog 为权威实体集，读取全部 `(id, payload)`，用 [`searchable_text`] 投影检索正文；
    ///    实体身份优先取 `fts_ids.id_json`（保真 kind+stability），缺失时回退 `from_wire`；
    /// 2. 先持久化一条 `building` intent，记录本次将索引的完整 id 集合与 digest；
    /// 3. 在单事务内**整表清空** `fts`/`fts_ids` 后按 catalog 集合重新写入，推进 generation，
    ///    并把 intent 标记 `activated`。
    ///
    /// 整表清空（而非逐条 upsert）是刻意的：rebuild 的场景正是“搜索索引已漂移/损坏”，
    /// 需清除任何不在 catalog 中的孤儿 `fts`/`fts_ids` 行。若事务中途失败，整个重建回滚，旧
    /// generation 及其索引原样保留，search 仍可用旧投影（“失败不污染旧 generation”）。
    ///
    /// 身份保真取 `fts_ids` 而非纯从 catalog wire 串还原：wire 串不含 stability，
    /// `from_wire` 只能得到 `Unstable`，会让 rebuild 后的搜索结果丢失原 Native/Reconstructed
    /// 身份（见 `SearchIndex::query` 存 id_json 的原因）。catalog 权威决定“有哪些实体、正文是什么”，
    /// `fts_ids` 保真“每个实体的完整身份”。
    ///
    /// rebuild 是显式维护命令，即使内容与现有投影一致也照常推进 generation——操作者
    /// 主动请求“干净重建”，不做 no-op 短路。返回重新索引的实体条数。
    pub fn rebuild_index(&self) -> PortResult<usize> {
        // 1) 以 catalog 为权威实体集，投影检索正文；身份优先取 fts_ids 保真。
        let upserts: Vec<(StableId, Vec<u8>, String)> = {
            let conn = self.conn.borrow();
            let mut stmt = conn
                .prepare("SELECT id, payload FROM catalog ORDER BY id ASC")
                .map_err(backend)?;
            let rows = stmt
                .query_map([], |row| {
                    let wire: String = row.get(0)?;
                    let payload: Vec<u8> = row.get(1)?;
                    Ok((wire, payload))
                })
                .map_err(backend)?;
            let mut out = Vec::new();
            for row in rows {
                let (wire, payload) = row.map_err(backend)?;
                // 优先用 fts_ids 里保真的 id_json（含 kind+stability）；缺失才回退 from_wire。
                let id_json: Option<String> = conn
                    .query_row(
                        "SELECT id_json FROM fts_ids WHERE wire_id = ?1",
                        [&wire],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(backend)?;
                let id = match id_json {
                    Some(json) => serde_json::from_str(&json).map_err(backend)?,
                    None => StableId::from_wire(&wire).ok_or_else(|| {
                        PortError::Backend(format!("invalid StableId stored in catalog: {wire}"))
                    })?,
                };
                let text = searchable_text(&payload);
                out.push((id, payload, text));
            }
            out
        };

        // 2) durable intent：记录本次重建将索引的完整集合（崩溃后 recover 会 abort 它）。
        let pending = self.begin_index_batch(&upserts, &[])?;

        // 3) 单事务：校验句柄 → 整表清空 FTS → 按 catalog 重投影 → 推进 generation → 标记 activated。
        let mut conn = self.conn.borrow_mut();
        let tx = conn.transaction().map_err(backend)?;
        Self::verify_pending_in_tx(&tx, &pending, &upserts, &[])?;

        tx.execute("DELETE FROM fts", []).map_err(backend)?;
        tx.execute("DELETE FROM fts_ids", []).map_err(backend)?;
        for (id, _payload, text) in &upserts {
            let id_json = serde_json::to_string(id).map_err(backend)?;
            // 与提交路径一致：只有 Message 实体重投影进 fts；
            // session/document 仅重建 fts_ids 身份边车。
            if id.kind() == IdKind::Message {
                tx.execute(
                    "INSERT INTO fts(id, text) VALUES(?1, ?2)",
                    rusqlite::params![id_json, text],
                )
                .map_err(backend)?;
            }
            tx.execute(
                "INSERT INTO fts_ids(wire_id, id_json) VALUES(?1, ?2)",
                rusqlite::params![id.as_str(), id_json],
            )
            .map_err(backend)?;
        }

        tx.execute(
            "UPDATE store_metadata SET active_generation = ?1 WHERE singleton = 1",
            [pending.target_generation as i64],
        )
        .map_err(backend)?;
        tx.execute(
            "UPDATE index_batches
             SET state = 'activated', durable_point = 'activated', committed_at_ms = ?2
             WHERE operation_id = ?1",
            rusqlite::params![pending.operation_id, unix_ms()?],
        )
        .map_err(backend)?;

        tx.commit().map_err(backend)?;
        Ok(upserts.len())
    }

    /// 崩溃恢复：把所有停在 `building` 的 outbox 行标记为 `aborted`。
    ///
    /// FTS5 单存储下 `building` 行必然无已提交副作用（apply 与 activate 同事务，
    /// 要么全成要么全滚），故恢复动作是幂等的纯 journal 清理，不触碰 catalog/FTS。
    /// 返回被 abort 的批次数，供诊断输出。写路径打开时自动调用。
    pub fn recover_interrupted(&self) -> PortResult<usize> {
        let conn = self.conn.borrow();
        let n = conn
            .execute(
                "UPDATE index_batches
                 SET state = 'aborted', error_code = 'interrupted_before_activation'
                 WHERE state = 'building'",
                [],
            )
            .map_err(backend)?;
        Ok(n)
    }

    /// 只读统计停在 `building` 的 outbox 行数——中断恢复的"待收敛"证据。
    ///
    /// 与 [`recover_interrupted`](Self::recover_interrupted) 不同，本方法不改状态：
    /// 供 doctor 等只读路径观测“有多少无副作用 intent 尚待下次写打开收敛”，
    /// 作为 durable outbox 中断恢复与 generation 一致性的证据。
    pub fn interrupted_batch_count(&self) -> PortResult<u64> {
        let conn = self.conn.borrow();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM index_batches WHERE state = 'building'",
                [],
                |row| row.get(0),
            )
            .map_err(backend)?;
        u64::try_from(n).map_err(backend)
    }

    /// 读回一条 outbox 行（供测试与诊断）。
    pub fn index_batch(&self, operation_id: &str) -> PortResult<Option<IndexBatch>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(
                "SELECT operation_id, base_generation, target_generation, state,
                        operation_digest, upsert_ids_json, delete_ids_json,
                        durable_point, error_code
                 FROM index_batches WHERE operation_id = ?1",
            )
            .map_err(backend)?;
        let mut rows = stmt.query([operation_id]).map_err(backend)?;
        match rows.next().map_err(backend)? {
            None => Ok(None),
            Some(row) => {
                let upsert_json: String = row.get(5).map_err(backend)?;
                let delete_json: String = row.get(6).map_err(backend)?;
                Ok(Some(IndexBatch {
                    operation_id: row.get(0).map_err(backend)?,
                    base_generation: u64::try_from(row.get::<_, i64>(1).map_err(backend)?)
                        .map_err(backend)?,
                    target_generation: u64::try_from(row.get::<_, i64>(2).map_err(backend)?)
                        .map_err(backend)?,
                    state: row.get(3).map_err(backend)?,
                    operation_digest: row.get(4).map_err(backend)?,
                    upsert_ids: serde_json::from_str(&upsert_json).map_err(backend)?,
                    delete_ids: serde_json::from_str(&delete_json).map_err(backend)?,
                    durable_point: row.get(7).map_err(backend)?,
                    error_code: row.get(8).map_err(backend)?,
                }))
            }
        }
    }
}

/// 当前 catalog schema 版本。每次结构变更 +1 并在 [`SqliteStore::migrate`] 追加步骤。
pub const SCHEMA_VERSION: i64 = 6;

impl CatalogStore for SqliteStore {
    fn get(&self, id: &StableId) -> PortResult<Option<Vec<u8>>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare("SELECT payload FROM catalog WHERE id = ?1")
            .map_err(backend)?;
        let mut rows = stmt.query([id.as_str()]).map_err(backend)?;
        match rows.next().map_err(backend)? {
            Some(row) => Ok(Some(row.get::<_, Vec<u8>>(0).map_err(backend)?)),
            None => Ok(None),
        }
    }

    fn put(&self, id: &StableId, payload: &[u8]) -> PortResult<()> {
        let conn = self.conn.borrow();
        conn.execute(
            "INSERT INTO catalog(id, payload) VALUES(?1, ?2)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            rusqlite::params![id.as_str(), payload],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn list(&self, limit: usize) -> PortResult<Vec<CatalogEntry>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare("SELECT id, payload FROM catalog ORDER BY id ASC LIMIT ?1")
            .map_err(backend)?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                let wire: String = row.get(0)?;
                let payload: Vec<u8> = row.get(1)?;
                Ok((wire, payload))
            })
            .map_err(backend)?;
        let mut entries = Vec::new();
        for row in rows {
            let (wire, payload) = row.map_err(backend)?;
            let id = StableId::from_wire(&wire).ok_or_else(|| {
                PortError::Backend(format!("invalid StableId stored in catalog: {wire}"))
            })?;
            entries.push(CatalogEntry { id, payload });
        }
        Ok(entries)
    }

    fn count(&self) -> PortResult<u64> {
        let conn = self.conn.borrow();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM catalog", [], |row| row.get(0))
            .map_err(backend)?;
        u64::try_from(count).map_err(backend)
    }

    fn active_generation(&self) -> PortResult<u64> {
        SqliteStore::active_generation(self)
    }
}

impl SearchIndex for SqliteStore {
    fn index(&self, id: &StableId, text: &str) -> PortResult<()> {
        // StableId 无字符串反解构造器，故存其 serde JSON 以便查询时无损重建
        // （wire 串不含 stability，无法从 as_str() 还原完整身份）。
        let id_json = serde_json::to_string(id).map_err(backend)?;
        let mut conn = self.conn.borrow_mut();
        let tx = conn.transaction().map_err(backend)?;
        tx.execute(
            "DELETE FROM fts
             WHERE id = ?1 OR id = (SELECT id_json FROM fts_ids WHERE wire_id = ?2)",
            rusqlite::params![id_json, id.as_str()],
        )
        .map_err(backend)?;
        tx.execute("DELETE FROM fts_ids WHERE wire_id = ?1", [id.as_str()])
            .map_err(backend)?;
        tx.execute(
            "INSERT INTO fts(id, text) VALUES(?1, ?2)",
            rusqlite::params![id_json, text],
        )
        .map_err(backend)?;
        tx.execute(
            "INSERT INTO fts_ids(wire_id, id_json) VALUES(?1, ?2)",
            rusqlite::params![id.as_str(), id_json],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(())
    }

    fn query(&self, query: &str, limit: usize) -> PortResult<Vec<SearchHit>> {
        let conn = self.conn.borrow();
        // bm25() 越小越相关，ASC 排序即"相关性降序"（契约要求最相关在前）。
        // 次序键补 id：等分命中获得跨次运行稳定的全序，cursor 分页依赖它（CONTRACT §7）。
        let mut stmt = conn
            .prepare(
                "SELECT id, bm25(fts) FROM fts WHERE fts MATCH ?1
                 ORDER BY bm25(fts), id LIMIT ?2",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(rusqlite::params![query, limit as i64], |row| {
                let id_json: String = row.get(0)?;
                let bm25: f64 = row.get(1)?;
                Ok((id_json, bm25))
            })
            .map_err(backend)?;
        let mut hits = Vec::new();
        for r in rows {
            let (id_json, bm25) = r.map_err(backend)?;
            let id: StableId = serde_json::from_str(&id_json).map_err(backend)?;
            // 取负使"分数越高越相关"，符合 SearchHit.score 的直觉（后端相对值）。
            hits.push(SearchHit {
                id,
                score: -bm25 as f32,
            });
        }
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentsessions_domain::{IdKind, Stability};

    fn sid(kind: IdKind, fact: &[u8]) -> StableId {
        StableId::derive(kind, Stability::Reconstructed, &[fact])
    }

    fn sqlite_failure(code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None)
    }

    #[test]
    fn sqlite_busy_maps_to_retryable_writer_busy() {
        let error = backend(sqlite_failure(rusqlite::ffi::SQLITE_BUSY));
        assert!(matches!(
            error,
            PortError::WriterBusy(message)
                if message == "SQLite storage is busy or locked by another writer"
        ));
    }

    #[test]
    fn sqlite_locked_maps_to_retryable_writer_busy() {
        let error = backend(sqlite_failure(rusqlite::ffi::SQLITE_LOCKED));
        assert!(matches!(
            error,
            PortError::WriterBusy(message)
                if message == "SQLite storage is busy or locked by another writer"
        ));
    }

    #[test]
    fn non_contention_sqlite_failure_remains_backend() {
        let error = backend(sqlite_failure(rusqlite::ffi::SQLITE_CORRUPT));
        assert!(matches!(error, PortError::Backend(_)));
    }

    type SourceState = (u64, Vec<(String, Vec<u8>)>, Vec<(String, String)>, String);

    fn source_state(store: &SqliteStore) -> SourceState {
        let conn = store.conn.borrow();
        let catalog = {
            let mut stmt = conn
                .prepare("SELECT id, payload FROM catalog ORDER BY id")
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap();
            rows.map(Result::unwrap).collect()
        };
        let membership = {
            let mut stmt = conn
                .prepare(
                    "SELECT source_path, message_id FROM source_membership
                     ORDER BY source_path, message_id",
                )
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap();
            rows.map(Result::unwrap).collect()
        };
        let digest = conn
            .query_row(
                "SELECT operation_digest FROM index_batches
                 WHERE state = 'activated' ORDER BY target_generation DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        (
            store.active_generation().unwrap(),
            catalog,
            membership,
            digest,
        )
    }

    #[test]
    fn catalog_put_get_roundtrip() {
        let store = SqliteStore::open_in_memory().unwrap();
        let id = sid(IdKind::Session, b"s1");
        assert!(store.get(&id).unwrap().is_none());
        store.put(&id, b"hello payload").unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap(), b"hello payload");
    }

    #[test]
    fn catalog_put_overwrites() {
        let store = SqliteStore::open_in_memory().unwrap();
        let id = sid(IdKind::Session, b"s1");
        store.put(&id, b"first").unwrap();
        store.put(&id, b"second").unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap(), b"second");
    }

    #[test]
    fn search_finds_indexed_and_rebuilds_id() {
        let store = SqliteStore::open_in_memory().unwrap();
        let id = sid(IdKind::Message, b"m1");
        store.index(&id, "the quick brown fox").unwrap();
        let hits = store.query("brown", 10).unwrap();
        assert_eq!(hits.len(), 1);
        // 关键：从 FTS 取回的 id 与原 id 完全相等（含 kind/stability），
        // 证明 serde JSON 往返无损。
        assert_eq!(hits[0].id, id);
        assert_eq!(hits[0].id.kind(), IdKind::Message);
        assert_eq!(hits[0].id.stability(), Stability::Reconstructed);
    }

    #[test]
    fn reindex_is_idempotent() {
        let store = SqliteStore::open_in_memory().unwrap();
        let id = sid(IdKind::Message, b"m1");
        store.index(&id, "alpha beta").unwrap();
        store.index(&id, "alpha gamma").unwrap();
        // 重索引后旧文本不再命中，新文本命中，且不产生重复行。
        assert!(store.query("beta", 10).unwrap().is_empty());
        assert_eq!(store.query("gamma", 10).unwrap().len(), 1);
        assert_eq!(store.query("alpha", 10).unwrap().len(), 1);
    }

    #[test]
    fn query_respects_limit() {
        let store = SqliteStore::open_in_memory().unwrap();
        for i in 0..5u32 {
            let id = sid(IdKind::Message, &i.to_le_bytes());
            store.index(&id, "shared term").unwrap();
        }
        assert_eq!(store.query("shared", 3).unwrap().len(), 3);
    }

    #[test]
    fn fresh_db_reports_current_schema_version() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn reopen_preserves_data_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.db");
        let p = path.to_string_lossy().into_owned();
        let id = sid(IdKind::Session, b"s1");
        {
            let store = SqliteStore::open(&p).unwrap();
            store.put(&id, b"persisted").unwrap();
            store.index(&id, "persisted body").unwrap();
        }
        // 重开：migration 幂等（IF NOT EXISTS 已换成版本门控），数据与版本不变。
        let store = SqliteStore::open(&p).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(store.get(&id).unwrap().unwrap(), b"persisted");
        assert_eq!(store.query("persisted", 10).unwrap().len(), 1);
    }

    #[test]
    fn newer_schema_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.db");
        let p = path.to_string_lossy().into_owned();
        // 先正常建库，再把 user_version 拨到未来版本，模拟更新的二进制写过的库。
        SqliteStore::open(&p).unwrap();
        {
            let conn = rusqlite::Connection::open(&p).unwrap();
            conn.execute_batch(&format!("PRAGMA user_version = {};", SCHEMA_VERSION + 1))
                .unwrap();
        }
        // 旧二进制拒绝打开更新版本的库，而非按旧 schema 误读。
        // 用 match 而非 unwrap_err()——SqliteStore 内含 Connection，不实现 Debug。
        let err = match SqliteStore::open(&p) {
            Err(e) => e,
            Ok(_) => panic!("expected newer schema to be rejected"),
        };
        assert!(
            matches!(err, PortError::SchemaIncompatible(m) if m.contains("newer than supported"))
        );
    }

    #[test]
    fn commit_batch_writes_all_entries() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"a");
        let b = sid(IdKind::Message, b"b");
        store
            .commit_batch(&[
                (a.clone(), b"role\talpha".to_vec(), "alpha text".into()),
                (b.clone(), b"role\tbeta".to_vec(), "beta text".into()),
            ])
            .unwrap();
        assert_eq!(store.get(&a).unwrap().unwrap(), b"role\talpha");
        assert_eq!(store.get(&b).unwrap().unwrap(), b"role\tbeta");
        assert_eq!(store.query("alpha", 10).unwrap().len(), 1);
        assert_eq!(store.query("beta", 10).unwrap().len(), 1);
    }

    #[test]
    fn commit_batch_is_idempotent() {
        let store = SqliteStore::open_in_memory().unwrap();
        let id = sid(IdKind::Message, b"m");
        let entry = [(id.clone(), b"role\tv1".to_vec(), "version one".into())];
        store.commit_batch(&entry).unwrap();
        let entry2 = [(id.clone(), b"role\tv2".to_vec(), "version two".into())];
        store.commit_batch(&entry2).unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap(), b"role\tv2");
        assert!(store.query("one", 10).unwrap().is_empty());
        assert_eq!(store.query("two", 10).unwrap().len(), 1);
    }

    #[test]
    fn deleting_by_wire_id_removes_search_row() {
        let store = SqliteStore::open_in_memory().unwrap();
        let original = sid(IdKind::Message, b"wire-delete");
        store.index(&original, "wire deletion text").unwrap();
        let wire_id = StableId::from_wire(original.as_str()).unwrap();
        let entries: [(StableId, Vec<u8>, String); 0] = [];
        let pending = store
            .begin_index_batch(&entries, std::slice::from_ref(&wire_id))
            .unwrap();
        store
            .commit_index_batch(&pending, &entries, &[wire_id])
            .unwrap();
        assert_eq!(store.count().unwrap(), 0);
        assert!(store.query("deletion", 10).unwrap().is_empty());
    }

    #[test]
    fn standalone_index_populates_wire_mapping() {
        let store = SqliteStore::open_in_memory().unwrap();
        let original = sid(IdKind::Message, b"standalone-map");
        store.index(&original, "standalone mapping text").unwrap();
        let wire_id = StableId::from_wire(original.as_str()).unwrap();
        let entries: [(StableId, Vec<u8>, String); 0] = [];
        let pending = store
            .begin_index_batch(&entries, std::slice::from_ref(&wire_id))
            .unwrap();
        store
            .commit_index_batch(&pending, &entries, &[wire_id])
            .unwrap();
        assert!(store.query("mapping", 10).unwrap().is_empty());
    }

    #[test]
    fn source_rescan_tombstones_removed_messages() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"source-a");
        let b = sid(IdKind::Message, b"source-b");
        let first = SourceBatch {
            source_path: "fixture.jsonl".into(),
            entries: vec![
                (a.clone(), b"a".to_vec(), "keep alpha".into()),
                (b.clone(), b"b".to_vec(), "remove beta".into()),
            ],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&first))
                .unwrap()
        );
        assert_eq!(store.count().unwrap(), 2);
        assert_eq!(store.query("beta", 10).unwrap().len(), 1);

        let second = SourceBatch {
            source_path: "fixture.jsonl".into(),
            entries: vec![(a.clone(), b"a".to_vec(), "keep alpha".into())],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&second))
                .unwrap()
        );
        assert_eq!(store.count().unwrap(), 1);
        assert!(store.get(&b).unwrap().is_none());
        assert!(store.query("beta", 10).unwrap().is_empty());
        assert_eq!(store.query("alpha", 10).unwrap().len(), 1);
    }

    #[test]
    fn unchanged_source_rescan_does_not_advance_generation() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"same-source");
        let source = SourceBatch {
            source_path: "same.jsonl".into(),
            entries: vec![(a, b"payload".to_vec(), "same text".into())],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&source))
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), 1);
        assert!(
            !store
                .commit_source_batches_if_changed(std::slice::from_ref(&source))
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), 1);
    }

    #[test]
    fn v3_db_migrates_source_membership_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v3.db");
        let p = path.to_string_lossy().into_owned();
        {
            let conn = rusqlite::Connection::open(&p).unwrap();
            conn.execute_batch(
                "CREATE TABLE catalog (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
                 CREATE VIRTUAL TABLE fts USING fts5(id UNINDEXED, text);
                 CREATE TABLE store_metadata (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), active_generation INTEGER NOT NULL);
                 INSERT INTO store_metadata(singleton, active_generation) VALUES(1, 0);
                 CREATE TABLE index_batches (
                     operation_id TEXT PRIMARY KEY, base_generation INTEGER NOT NULL,
                     target_generation INTEGER NOT NULL, state TEXT NOT NULL,
                     operation_digest TEXT NOT NULL, upsert_ids_json TEXT NOT NULL,
                     delete_ids_json TEXT NOT NULL, durable_point TEXT NOT NULL,
                     created_at_ms INTEGER NOT NULL, committed_at_ms INTEGER,
                     error_code TEXT
                 );
                 CREATE TABLE fts_ids (wire_id TEXT PRIMARY KEY, id_json TEXT NOT NULL UNIQUE);
                 PRAGMA user_version = 3;",
            )
            .unwrap();
        }
        let store = SqliteStore::open(&p).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let conn = rusqlite::Connection::open(&p).unwrap();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'source_membership'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1);
        drop(store);
    }

    #[test]
    fn v5_db_migrates_membership_document_id_column() {
        // 带数据的 v5 库升级到 v6：membership 旧行保留且 document_id 为 NULL。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v5.db");
        let p = path.to_string_lossy().into_owned();
        {
            let conn = rusqlite::Connection::open(&p).unwrap();
            conn.execute_batch(
                "CREATE TABLE catalog (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
                 CREATE VIRTUAL TABLE fts USING fts5(id UNINDEXED, text);
                 CREATE TABLE store_metadata (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), active_generation INTEGER NOT NULL);
                 INSERT INTO store_metadata(singleton, active_generation) VALUES(1, 1);
                 CREATE TABLE index_batches (
                     operation_id TEXT PRIMARY KEY, base_generation INTEGER NOT NULL,
                     target_generation INTEGER NOT NULL, state TEXT NOT NULL,
                     operation_digest TEXT NOT NULL, upsert_ids_json TEXT NOT NULL,
                     delete_ids_json TEXT NOT NULL, durable_point TEXT NOT NULL,
                     created_at_ms INTEGER NOT NULL, committed_at_ms INTEGER,
                     error_code TEXT
                 );
                 CREATE TABLE fts_ids (wire_id TEXT PRIMARY KEY, id_json TEXT NOT NULL UNIQUE);
                 CREATE TABLE source_membership (
                     source_path TEXT NOT NULL,
                     message_id  TEXT NOT NULL,
                     PRIMARY KEY(source_path, message_id)
                 );
                 CREATE TABLE source_scans (
                     source_path   TEXT PRIMARY KEY,
                     scanned_at_ms INTEGER NOT NULL
                 );
                 INSERT INTO catalog(id, payload) VALUES('msg_v1_legacy', X'01');
                 INSERT INTO source_membership(source_path, message_id)
                 VALUES('legacy.jsonl', 'msg_v1_legacy');
                 INSERT INTO source_scans(source_path, scanned_at_ms) VALUES('legacy.jsonl', 1);
                 PRAGMA user_version = 5;",
            )
            .unwrap();
        }
        let store = SqliteStore::open(&p).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        // 旧数据完整保留。
        let id = StableId::from_wire("msg_v1_legacy").unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap(), vec![1u8]);
        drop(store);
        let conn = rusqlite::Connection::open(&p).unwrap();
        let doc: Option<String> = conn
            .query_row(
                "SELECT document_id FROM source_membership WHERE message_id = 'msg_v1_legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // v6 前的 membership 行无文档归属信息——显式 NULL，不臆造。
        assert_eq!(doc, None);
    }

    #[test]
    fn non_message_entities_are_catalog_only() {
        // session/document 实体入 catalog、可 get/list，但绝不进入全文搜索。
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"cat-only-msg");
        let ses = sid(IdKind::Session, b"cat-only-ses");
        let doc = sid(IdKind::Document, b"cat-only-doc");
        let source = SourceBatch {
            source_path: "mixed.jsonl".into(),
            entries: vec![
                (msg.clone(), b"m".to_vec(), "unique searchable body".into()),
                (ses.clone(), b"s".to_vec(), "unique searchable body".into()),
                (doc.clone(), b"d".to_vec(), "unique searchable body".into()),
            ],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&source))
                .unwrap()
        );
        // catalog 三个实体都在。
        assert_eq!(store.count().unwrap(), 3);
        assert!(store.get(&ses).unwrap().is_some());
        assert!(store.get(&doc).unwrap().is_some());
        // 搜索只命中消息——容器实体不参与全文命中，避免重复计数。
        let hits = store.query("searchable", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.as_str(), msg.as_str());
        // fts_ids 身份边车对所有 kind 保留（rebuild 依赖它保真身份）。
        let conn = store.conn.borrow();
        let sidecar: i64 = conn
            .query_row("SELECT COUNT(*) FROM fts_ids", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sidecar, 3);
        // 重复提交同一批是内容级 no-op（非消息实体不因缺 fts 行而误判为变更）。
        drop(conn);
        assert!(
            !store
                .commit_source_batches_if_changed(std::slice::from_ref(&source))
                .unwrap()
        );
    }

    #[test]
    fn rebuild_keeps_non_message_entities_out_of_fts() {
        // 混合库 rebuild：身份保真、消息重投影、容器实体仍不进 fts。
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"rebuild-msg");
        let ses = sid(IdKind::Session, b"rebuild-ses");
        let source = SourceBatch {
            source_path: "rebuild.jsonl".into(),
            entries: vec![
                (
                    msg.clone(),
                    b"role\tbody words".to_vec(),
                    "body words".into(),
                ),
                (ses.clone(), b"s".to_vec(), String::new()),
            ],
        };
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&source))
            .unwrap();
        let rebuilt = store.rebuild_index().unwrap();
        assert_eq!(rebuilt, 2);
        // 消息可搜、身份保真（非 Unstable——来自 fts_ids 边车）。
        let hits = store.query("body", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.stability(), Stability::Reconstructed);
        // 容器实体：无 fts 行、有 fts_ids 边车。
        let conn = store.conn.borrow();
        let fts_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fts_rows, 1);
        let sidecar: i64 = conn
            .query_row("SELECT COUNT(*) FROM fts_ids", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sidecar, 2);
    }

    #[test]
    fn source_rescan_retires_session_and_document_rows() {
        // 源缩水成空 scan：其 session/document 目录行随消息一起 tombstone。
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"retire-msg");
        let ses = sid(IdKind::Session, b"retire-ses");
        let doc = sid(IdKind::Document, b"retire-doc");
        let full = SourceBatch {
            source_path: "retire.jsonl".into(),
            entries: vec![
                (msg.clone(), b"m".to_vec(), "text".into()),
                (ses.clone(), b"s".to_vec(), String::new()),
                (doc.clone(), b"d".to_vec(), String::new()),
            ],
        };
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&full))
            .unwrap();
        // membership 记录了该源的文档归属。
        {
            let conn = store.conn.borrow();
            let recorded: Option<String> = conn
                .query_row(
                    "SELECT document_id FROM source_membership WHERE message_id = ?1",
                    [msg.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(recorded.as_deref(), Some(doc.as_str()));
        }
        let empty = SourceBatch {
            source_path: "retire.jsonl".into(),
            entries: vec![],
        };
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&empty))
            .unwrap();
        assert_eq!(store.count().unwrap(), 0);
        assert!(store.get(&ses).unwrap().is_none());
        assert!(store.get(&doc).unwrap().is_none());
    }

    #[test]
    fn shared_entity_survives_other_source_rescan() {
        // 两个源共享同一实体：一个源消失不退役另一源仍引用的实体。
        let store = SqliteStore::open_in_memory().unwrap();
        let shared = sid(IdKind::Document, b"shared-doc");
        let m1 = sid(IdKind::Message, b"share-m1");
        let m2 = sid(IdKind::Message, b"share-m2");
        let sources = [
            SourceBatch {
                source_path: "one.jsonl".into(),
                entries: vec![
                    (m1.clone(), b"m1".to_vec(), "one text".into()),
                    (shared.clone(), b"d".to_vec(), String::new()),
                ],
            },
            SourceBatch {
                source_path: "two.jsonl".into(),
                entries: vec![
                    (m2.clone(), b"m2".to_vec(), "two text".into()),
                    (shared.clone(), b"d".to_vec(), String::new()),
                ],
            },
        ];
        store.commit_source_batches_if_changed(&sources).unwrap();
        assert_eq!(store.count().unwrap(), 3);
        // 源 one 变空：m1 退役；shared 仍被 two 引用，保留。
        let shrunk = SourceBatch {
            source_path: "one.jsonl".into(),
            entries: vec![],
        };
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&shrunk))
            .unwrap();
        assert!(store.get(&m1).unwrap().is_none());
        assert!(store.get(&shared).unwrap().is_some());
        assert!(store.get(&m2).unwrap().is_some());
    }

    /// Canonical session payload for a source contributing `members`.
    fn session_payload(document: &str, members: &[&str]) -> Vec<u8> {
        serde_json::json!({
            "document": document,
            "documents": [document],
            "messages": members,
        })
        .to_string()
        .into_bytes()
    }

    fn session_members(store: &SqliteStore, id: &StableId) -> Vec<String> {
        let bytes = store.get(id).unwrap().expect("session must be present");
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["messages"]
            .as_array()
            .expect("messages array")
            .iter()
            .map(|entry| entry.as_str().expect("member is a string").to_string())
            .collect()
    }

    fn session_documents(store: &SqliteStore, id: &StableId) -> Vec<String> {
        let bytes = store.get(id).unwrap().expect("session must be present");
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["documents"]
            .as_array()
            .expect("documents array")
            .iter()
            .map(|entry| entry.as_str().expect("document is a string").to_string())
            .collect()
    }

    #[test]
    fn session_spanning_two_sources_in_one_batch_unions_its_members() {
        // 真实形态：一个逻辑会话被拆到多个 transcript 文件，每个源只声明自己那部分
        // 成员。旧行为把这判为冲突投影并拒绝整批（exit 6）；正确行为是取并集。
        let store = SqliteStore::open_in_memory().unwrap();
        let ses = sid(IdKind::Session, b"split-session");
        let doc_a = sid(IdKind::Document, b"split-doc-a");
        let doc_b = sid(IdKind::Document, b"split-doc-b");
        let m1 = sid(IdKind::Message, b"split-m1");
        let m2 = sid(IdKind::Message, b"split-m2");
        let sources = [
            SourceBatch {
                source_path: "part-a.jsonl".into(),
                entries: vec![
                    (m1.clone(), b"m1".to_vec(), "first half".into()),
                    (
                        ses.clone(),
                        session_payload(doc_a.as_str(), &[m1.as_str()]),
                        String::new(),
                    ),
                    (doc_a.clone(), b"da".to_vec(), String::new()),
                ],
            },
            SourceBatch {
                source_path: "part-b.jsonl".into(),
                entries: vec![
                    (m2.clone(), b"m2".to_vec(), "second half".into()),
                    (
                        ses.clone(),
                        session_payload(doc_b.as_str(), &[m2.as_str()]),
                        String::new(),
                    ),
                    (doc_b.clone(), b"db".to_vec(), String::new()),
                ],
            },
        ];
        assert!(store.commit_source_batches_if_changed(&sources).unwrap());
        assert_eq!(
            session_members(&store, &ses),
            vec![m1.as_str().to_string(), m2.as_str().to_string()],
        );
        // 两个贡献文档都保留；单值别名取升序首个，供旧读取方使用。
        let mut expected_docs = vec![doc_a.as_str().to_string(), doc_b.as_str().to_string()];
        expected_docs.sort();
        assert_eq!(session_documents(&store, &ses), expected_docs);
        let bytes = store.get(&ses).unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["document"], expected_docs[0].as_str());
    }

    #[test]
    fn session_synced_in_separate_batches_accumulates_members() {
        // 真实语料按批提交（命令行长度上限），所以合并必须以库中现值为起点：
        // 否则第二批的成员列表会覆盖第一批，只剩最后一批的成员。
        let store = SqliteStore::open_in_memory().unwrap();
        let ses = sid(IdKind::Session, b"batched-session");
        let doc_a = sid(IdKind::Document, b"batched-doc-a");
        let doc_b = sid(IdKind::Document, b"batched-doc-b");
        let m1 = sid(IdKind::Message, b"batched-m1");
        let m2 = sid(IdKind::Message, b"batched-m2");

        let first = SourceBatch {
            source_path: "batch-a.jsonl".into(),
            entries: vec![
                (m1.clone(), b"m1".to_vec(), "batch a".into()),
                (
                    ses.clone(),
                    session_payload(doc_a.as_str(), &[m1.as_str()]),
                    String::new(),
                ),
                (doc_a.clone(), b"da".to_vec(), String::new()),
            ],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&first))
                .unwrap()
        );

        let second = SourceBatch {
            source_path: "batch-b.jsonl".into(),
            entries: vec![
                (m2.clone(), b"m2".to_vec(), "batch b".into()),
                (
                    ses.clone(),
                    session_payload(doc_b.as_str(), &[m2.as_str()]),
                    String::new(),
                ),
                (doc_b.clone(), b"db".to_vec(), String::new()),
            ],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&second))
                .unwrap()
        );

        assert_eq!(
            session_members(&store, &ses),
            vec![m1.as_str().to_string(), m2.as_str().to_string()],
            "第二批不得覆盖第一批的成员",
        );
        assert_eq!(session_documents(&store, &ses).len(), 2);
    }

    #[test]
    fn resyncing_a_cross_source_session_is_a_content_level_noop() {
        // 合并结果必须稳定：同一语料重复 sync 不得推进 generation，否则每次运行都
        // 会作废所有分页 cursor。
        let store = SqliteStore::open_in_memory().unwrap();
        let ses = sid(IdKind::Session, b"noop-session");
        let doc_a = sid(IdKind::Document, b"noop-doc-a");
        let doc_b = sid(IdKind::Document, b"noop-doc-b");
        let m1 = sid(IdKind::Message, b"noop-m1");
        let m2 = sid(IdKind::Message, b"noop-m2");
        let sources = [
            SourceBatch {
                source_path: "noop-a.jsonl".into(),
                entries: vec![
                    (m1.clone(), b"m1".to_vec(), "noop a".into()),
                    (
                        ses.clone(),
                        session_payload(doc_a.as_str(), &[m1.as_str()]),
                        String::new(),
                    ),
                    (doc_a.clone(), b"da".to_vec(), String::new()),
                ],
            },
            SourceBatch {
                source_path: "noop-b.jsonl".into(),
                entries: vec![
                    (m2.clone(), b"m2".to_vec(), "noop b".into()),
                    (
                        ses.clone(),
                        session_payload(doc_b.as_str(), &[m2.as_str()]),
                        String::new(),
                    ),
                    (doc_b.clone(), b"db".to_vec(), String::new()),
                ],
            },
        ];
        assert!(store.commit_source_batches_if_changed(&sources).unwrap());
        let generation = store.active_generation().unwrap();
        assert!(
            !store.commit_source_batches_if_changed(&sources).unwrap(),
            "重复提交同一跨源语料应为内容级 no-op",
        );
        assert_eq!(store.active_generation().unwrap(), generation);
    }

    #[test]
    fn legacy_single_document_session_upgrades_without_losing_members() {
        // 升级前入库的会话行只有单值 `document`，且没有 `documents` 数组。
        // 新二进制再次 sync 时必须把旧成员并进来，而不是丢弃或报错。
        let store = SqliteStore::open_in_memory().unwrap();
        let ses = sid(IdKind::Session, b"legacy-session");
        let doc_a = sid(IdKind::Document, b"legacy-doc-a");
        let doc_b = sid(IdKind::Document, b"legacy-doc-b");
        let m1 = sid(IdKind::Message, b"legacy-m1");
        let m2 = sid(IdKind::Message, b"legacy-m2");

        let legacy_payload = serde_json::json!({
            "document": doc_a.as_str(),
            "messages": [m1.as_str()],
        })
        .to_string()
        .into_bytes();
        let legacy = SourceBatch {
            source_path: "legacy-a.jsonl".into(),
            entries: vec![
                (m1.clone(), b"m1".to_vec(), "legacy a".into()),
                (ses.clone(), legacy_payload, String::new()),
                (doc_a.clone(), b"da".to_vec(), String::new()),
            ],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&legacy))
                .unwrap()
        );

        let modern = SourceBatch {
            source_path: "legacy-b.jsonl".into(),
            entries: vec![
                (m2.clone(), b"m2".to_vec(), "legacy b".into()),
                (
                    ses.clone(),
                    session_payload(doc_b.as_str(), &[m2.as_str()]),
                    String::new(),
                ),
                (doc_b.clone(), b"db".to_vec(), String::new()),
            ],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&modern))
                .unwrap()
        );

        assert_eq!(
            session_members(&store, &ses),
            vec![m1.as_str().to_string(), m2.as_str().to_string()],
        );
        assert_eq!(session_documents(&store, &ses).len(), 2);
    }

    #[test]
    fn conflicting_message_projections_are_still_rejected() {
        // 合并只对容器实体开放。同一条消息在不同源上投影不同是真实的不一致
        // （同一 native id 却内容不同），必须继续拒绝，不能被容器合并顺带放行。
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"conflict-msg");
        let sources = [
            SourceBatch {
                source_path: "conflict-a.jsonl".into(),
                entries: vec![(msg.clone(), b"first projection".to_vec(), "one".into())],
            },
            SourceBatch {
                source_path: "conflict-b.jsonl".into(),
                entries: vec![(msg.clone(), b"second projection".to_vec(), "two".into())],
            },
        ];
        let error = store
            .commit_source_batches_if_changed(&sources)
            .expect_err("conflicting message projections must be rejected");
        assert!(
            format!("{error}").contains("conflicting projections"),
            "{error}"
        );
    }

    /// Build the canonical message payload shape that ingest writes.
    fn message_payload(session: &str, text: &str) -> Vec<u8> {
        serde_json::json!({
            "role": "user",
            "text": text,
            "parent": null,
            "session": session,
            "span": { "start": 0, "end": 10 },
        })
        .to_string()
        .into_bytes()
    }

    /// Same message as it appears in one specific file: the copy sits at that
    /// file's own byte offsets and names the document it came from.
    fn message_payload_with_span(
        session: &str,
        text: &str,
        document: &str,
        start: u64,
        end: u64,
    ) -> Vec<u8> {
        serde_json::json!({
            "role": "user",
            "text": text,
            "parent": null,
            "session": session,
            "sessions": [session],
            "span": { "start": start, "end": end },
            "spans": [{ "document": document, "start": start, "end": end }],
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn message_copied_into_another_file_unions_its_per_document_spans() {
        // A resumed conversation's history is rewritten into the new transcript,
        // so the same message sits at a different byte offset in each file. Those
        // offsets are per-source facts: keep both, keyed by document, instead of
        // calling the difference a conflict.
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"respanned-msg");
        let sources = [
            SourceBatch {
                source_path: "original.jsonl".into(),
                entries: vec![(
                    msg.clone(),
                    message_payload_with_span("ses_v1_aaa", "same body", "doc_v1_aaa", 0, 929),
                    "same body".into(),
                )],
            },
            SourceBatch {
                source_path: "resumed.jsonl".into(),
                entries: vec![(
                    msg.clone(),
                    message_payload_with_span("ses_v1_bbb", "same body", "doc_v1_bbb", 512, 1322),
                    "same body".into(),
                )],
            },
        ];
        assert!(store.commit_source_batches_if_changed(&sources).unwrap());

        let stored: serde_json::Value =
            serde_json::from_slice(&store.get(&msg).unwrap().unwrap()).unwrap();
        let spans = stored["spans"].as_array().expect("spans array");
        assert_eq!(spans.len(), 2, "both locations must survive: {stored}");
        // Keyed by document and ordered by it, so the result does not depend on
        // which file happened to be scanned first.
        assert_eq!(spans[0]["document"], "doc_v1_aaa");
        assert_eq!(spans[0]["end"], 929);
        assert_eq!(spans[1]["document"], "doc_v1_bbb");
        assert_eq!(spans[1]["start"], 512);
        // The singular alias still names one real location, so evidence
        // assembly keeps reporting byte precision rather than degrading.
        assert_eq!(stored["span"]["start"], 0);
        assert_eq!(stored["span"]["end"], 929);

        // Re-syncing the same corpus changes nothing: spans are keyed by
        // document, so a second pass maps onto the same two entries.
        assert!(!store.commit_source_batches_if_changed(&sources).unwrap());
    }

    #[test]
    fn message_shared_by_resumed_sessions_unions_its_session_refs() {
        // Resuming or forking a session copies history into the new transcript,
        // so one message id legitimately appears under several session ids with
        // otherwise identical content. That must union, not conflict.
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"resumed-msg");
        let sources = [
            SourceBatch {
                source_path: "first.jsonl".into(),
                entries: vec![(
                    msg.clone(),
                    message_payload("ses_v1_aaa", "shared body"),
                    "shared body".into(),
                )],
            },
            SourceBatch {
                source_path: "second.jsonl".into(),
                entries: vec![(
                    msg.clone(),
                    message_payload("ses_v1_bbb", "shared body"),
                    "shared body".into(),
                )],
            },
        ];
        assert!(store.commit_source_batches_if_changed(&sources).unwrap());

        let stored: serde_json::Value =
            serde_json::from_slice(&store.get(&msg).unwrap().unwrap()).unwrap();
        assert_eq!(
            stored["sessions"],
            serde_json::json!(["ses_v1_aaa", "ses_v1_bbb"]),
            "both owning sessions must be recorded: {stored}"
        );
        // The single-value alias keeps pre-union readers working.
        assert_eq!(stored["session"], "ses_v1_aaa");
        // Everything else is untouched by the merge.
        assert_eq!(stored["text"], "shared body");
        assert_eq!(stored["span"]["end"], 10);
    }

    #[test]
    fn message_with_genuinely_different_content_still_conflicts() {
        // Only the session back-reference may differ. Diverging text under one
        // id is a real inconsistency and must not be papered over.
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"divergent-msg");
        let sources = [
            SourceBatch {
                source_path: "first.jsonl".into(),
                entries: vec![(
                    msg.clone(),
                    message_payload("ses_v1_aaa", "original body"),
                    "original body".into(),
                )],
            },
            SourceBatch {
                source_path: "second.jsonl".into(),
                entries: vec![(
                    msg.clone(),
                    message_payload("ses_v1_aaa", "rewritten body"),
                    "rewritten body".into(),
                )],
            },
        ];
        let error = store
            .commit_source_batches_if_changed(&sources)
            .expect_err("diverging message content must be rejected");
        assert!(
            format!("{error}").contains("conflicting projections"),
            "{error}"
        );
    }

    #[test]
    fn message_session_refs_accumulate_across_separate_batches() {
        // The same message arriving in a later batch must add its session
        // without dropping the ones already recorded.
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"batched-msg");
        let first = SourceBatch {
            source_path: "first.jsonl".into(),
            entries: vec![(
                msg.clone(),
                message_payload("ses_v1_aaa", "body"),
                "body".into(),
            )],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&first))
                .unwrap()
        );
        let second = SourceBatch {
            source_path: "second.jsonl".into(),
            entries: vec![(
                msg.clone(),
                message_payload("ses_v1_bbb", "body"),
                "body".into(),
            )],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&second))
                .unwrap()
        );

        let stored: serde_json::Value =
            serde_json::from_slice(&store.get(&msg).unwrap().unwrap()).unwrap();
        assert_eq!(
            stored["sessions"],
            serde_json::json!(["ses_v1_aaa", "ses_v1_bbb"]),
            "earlier batch's session must survive: {stored}"
        );
    }

    #[test]
    fn shared_message_survives_one_source_shrinking() {
        let store = SqliteStore::open_in_memory().unwrap();
        let shared = sid(IdKind::Message, b"shared");
        let first = [
            SourceBatch {
                source_path: "one".into(),
                entries: vec![(shared.clone(), b"p".to_vec(), "shared text".into())],
            },
            SourceBatch {
                source_path: "two".into(),
                entries: vec![(shared.clone(), b"p".to_vec(), "shared text".into())],
            },
        ];
        assert!(store.commit_source_batches_if_changed(&first).unwrap());
        let second = [
            SourceBatch {
                source_path: "one".into(),
                entries: Vec::new(),
            },
            SourceBatch {
                source_path: "two".into(),
                entries: vec![(shared.clone(), b"p".to_vec(), "shared text".into())],
            },
        ];
        assert!(store.commit_source_batches_if_changed(&second).unwrap());
        assert_eq!(store.count().unwrap(), 1);
        assert!(store.get(&shared).unwrap().is_some());
        assert_eq!(store.query("shared", 10).unwrap().len(), 1);
    }

    #[test]
    fn moving_message_to_new_source_is_atomic() {
        let store = SqliteStore::open_in_memory().unwrap();
        let moved = sid(IdKind::Message, b"move-to-new-source");
        let original = SourceBatch {
            source_path: "source-a".into(),
            entries: vec![(moved.clone(), b"p".to_vec(), "moved text".into())],
        };
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&original))
            .unwrap();

        let moved_batches = [
            SourceBatch {
                source_path: "source-a".into(),
                entries: Vec::new(),
            },
            SourceBatch {
                source_path: "source-b".into(),
                entries: vec![(moved.clone(), b"p".to_vec(), "moved text".into())],
            },
        ];
        assert!(
            store
                .commit_source_batches_if_changed(&moved_batches)
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), 2);
        assert_eq!(store.get(&moved).unwrap().unwrap(), b"p");
        assert_eq!(store.query("moved", 10).unwrap().len(), 1);
        assert_eq!(
            store.source_message_ids("source-a").unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            store.source_message_ids("source-b").unwrap(),
            vec![moved.as_str().to_string()]
        );
    }

    #[test]
    fn source_batch_permutations_produce_identical_state() {
        fn run(order: [usize; 3]) -> SourceState {
            let store = SqliteStore::open_in_memory().unwrap();
            let moved = sid(IdKind::Message, b"permuted-move");
            let removed = sid(IdKind::Message, b"permuted-remove");
            let kept = sid(IdKind::Message, b"permuted-keep");
            let initial = [
                SourceBatch {
                    source_path: "source-a".into(),
                    entries: vec![
                        (moved.clone(), b"m".to_vec(), "moved text".into()),
                        (removed.clone(), b"r".to_vec(), "removed text".into()),
                    ],
                },
                SourceBatch {
                    source_path: "source-c".into(),
                    entries: vec![(kept.clone(), b"k".to_vec(), "kept text".into())],
                },
            ];
            store.commit_source_batches_if_changed(&initial).unwrap();

            let mut replacement = [
                Some(SourceBatch {
                    source_path: "source-a".into(),
                    entries: Vec::new(),
                }),
                Some(SourceBatch {
                    source_path: "source-b".into(),
                    entries: vec![(moved, b"m".to_vec(), "moved text".into())],
                }),
                Some(SourceBatch {
                    source_path: "source-c".into(),
                    entries: vec![(kept, b"k".to_vec(), "kept text".into())],
                }),
            ];
            let ordered: Vec<SourceBatch> = order
                .into_iter()
                .map(|index| replacement[index].take().unwrap())
                .collect();
            store.commit_source_batches_if_changed(&ordered).unwrap();
            assert!(store.get(&removed).unwrap().is_none());
            source_state(&store)
        }

        let permutations = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let expected = run(permutations[0]);
        for permutation in permutations.into_iter().skip(1) {
            assert_eq!(run(permutation), expected);
        }
    }

    #[test]
    fn empty_source_scan_tombstones_prior_membership() {
        let store = SqliteStore::open_in_memory().unwrap();
        let id = sid(IdKind::Message, b"becomes-empty");
        let populated = SourceBatch {
            source_path: "empty-later".into(),
            entries: vec![(id.clone(), b"p".to_vec(), "will disappear".into())],
        };
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&populated))
            .unwrap();
        let empty = SourceBatch {
            source_path: "empty-later".into(),
            entries: Vec::new(),
        };
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&empty))
            .unwrap();
        assert_eq!(store.count().unwrap(), 0);
        assert!(store.query("disappear", 10).unwrap().is_empty());
    }

    #[test]
    fn duplicate_source_paths_are_rejected() {
        let store = SqliteStore::open_in_memory().unwrap();
        let sources = [
            SourceBatch {
                source_path: "same".into(),
                entries: Vec::new(),
            },
            SourceBatch {
                source_path: "same".into(),
                entries: Vec::new(),
            },
        ];
        let err = store
            .commit_source_batches_if_changed(&sources)
            .unwrap_err();
        assert!(matches!(err, PortError::Backend(m) if m.contains("duplicate source paths")));
    }

    #[test]
    fn duplicate_message_error_does_not_disclose_source_path() {
        let store = SqliteStore::open_in_memory().unwrap();
        let id = sid(IdKind::Message, b"duplicate-in-source");
        let private_path = "C:/private/provider/session.jsonl";
        let source = SourceBatch {
            source_path: private_path.into(),
            entries: vec![
                (id.clone(), b"p".to_vec(), "text".into()),
                (id, b"p".to_vec(), "text".into()),
            ],
        };
        let err = store
            .commit_source_batches_if_changed(std::slice::from_ref(&source))
            .unwrap_err();
        let PortError::Backend(message) = err else {
            panic!("expected backend error");
        };
        assert!(message.contains("duplicate message ids"));
        assert!(!message.contains(private_path));
    }

    #[test]
    fn shared_wire_id_with_conflicting_identity_metadata_is_rejected() {
        let store = SqliteStore::open_in_memory().unwrap();
        let reconstructed = sid(IdKind::Message, b"shared-wire-identity");
        let unstable = StableId::from_wire(reconstructed.as_str()).unwrap();
        assert_ne!(reconstructed, unstable);
        assert_eq!(reconstructed.as_str(), unstable.as_str());

        let sources = [
            SourceBatch {
                source_path: "one".into(),
                entries: vec![(reconstructed, b"p".to_vec(), "same text".into())],
            },
            SourceBatch {
                source_path: "two".into(),
                entries: vec![(unstable, b"p".to_vec(), "same text".into())],
            },
        ];
        let err = store
            .commit_source_batches_if_changed(&sources)
            .unwrap_err();
        assert!(
            matches!(err, PortError::Backend(m) if m.contains("conflicting identity metadata"))
        );
    }

    #[test]
    fn catalog_list_is_sorted_and_limited() {
        let store = SqliteStore::open_in_memory().unwrap();
        let b = sid(IdKind::Message, b"b");
        let a = sid(IdKind::Message, b"a");
        store.put(&b, b"B").unwrap();
        store.put(&a, b"A").unwrap();
        assert_eq!(store.count().unwrap(), 2);
        let all = store.list(10).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all[0].id.as_str() < all[1].id.as_str());
        let one = store.list(1).unwrap();
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn fresh_store_starts_at_generation_zero() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.active_generation().unwrap(), 0);
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn index_batch_commits_and_advances_generation() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"a");
        let b = sid(IdKind::Message, b"b");
        let entries = [
            (a.clone(), b"role\talpha".to_vec(), "alpha text".into()),
            (b.clone(), b"role\tbeta".to_vec(), "beta text".into()),
        ];
        let pending = store.begin_index_batch(&entries, &[]).unwrap();
        assert_eq!(pending.base_generation, 0);
        assert_eq!(pending.target_generation, 1);
        // durable intent 已落盘；catalog 仍空。
        assert_eq!(store.count().unwrap(), 0);
        let batch = store.index_batch(&pending.operation_id).unwrap().unwrap();
        assert_eq!(batch.state, "building");
        assert_eq!(batch.durable_point, "intent");
        assert_eq!(batch.operation_digest, pending.operation_digest);

        store.commit_index_batch(&pending, &entries, &[]).unwrap();
        assert_eq!(store.active_generation().unwrap(), 1);
        assert_eq!(store.get(&a).unwrap().unwrap(), b"role\talpha");
        assert_eq!(store.query("alpha", 10).unwrap().len(), 1);
        let batch = store.index_batch(&pending.operation_id).unwrap().unwrap();
        assert_eq!(batch.state, "activated");
        assert_eq!(batch.durable_point, "activated");
    }

    #[test]
    fn commit_rejects_mismatched_payload() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"a");
        let b = sid(IdKind::Message, b"b");
        let intended = [(a.clone(), b"x".to_vec(), "x".into())];
        let pending = store.begin_index_batch(&intended, &[]).unwrap();
        // 实际 upsert 含未声明的 b —— 必须拒绝，journal 与数据不能分歧。
        let err = store
            .commit_index_batch(
                &pending,
                &[
                    (a, b"x".to_vec(), "x".into()),
                    (b, b"y".to_vec(), "y".into()),
                ],
                &[],
            )
            .unwrap_err();
        assert!(
            matches!(err, PortError::Backend(m) if m.contains("does not match durable intent"))
        );
        assert_eq!(store.active_generation().unwrap(), 0);
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn commit_rejects_stale_base_generation() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"a");
        // 先成功推进到 gen 1。
        let first = [(a.clone(), b"v1".to_vec(), "v1".into())];
        let p1 = store.begin_index_batch(&first, &[]).unwrap();
        store.commit_index_batch(&p1, &first, &[]).unwrap();
        // 伪造一个 base=0 的 pending（模拟旧写者持过期 handle）。
        let stale = PendingIndexBatch {
            operation_id: p1.operation_id.clone(), // 已 activated，非 building
            base_generation: 0,
            target_generation: 1,
            operation_digest: p1.operation_digest.clone(),
        };
        let err = store
            .commit_index_batch(&stale, &[(a, b"v2".to_vec(), "v2".into())], &[])
            .unwrap_err();
        assert!(matches!(err, PortError::Backend(_)));
        assert_eq!(store.active_generation().unwrap(), 1);
    }

    #[test]
    fn recover_aborts_orphan_building_intents() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"a");
        let entries = [(a, b"x".to_vec(), "x".into())];
        let pending = store.begin_index_batch(&entries, &[]).unwrap();
        // 模拟崩溃：intent 已 durable，apply 未发生。
        assert_eq!(
            store
                .index_batch(&pending.operation_id)
                .unwrap()
                .unwrap()
                .state,
            "building"
        );
        let n = store.recover_interrupted().unwrap();
        assert_eq!(n, 1);
        let batch = store.index_batch(&pending.operation_id).unwrap().unwrap();
        assert_eq!(batch.state, "aborted");
        assert_eq!(
            batch.error_code.as_deref(),
            Some("interrupted_before_activation")
        );
        // 恢复后 generation 与 catalog 不受影响。
        assert_eq!(store.active_generation().unwrap(), 0);
        assert_eq!(store.count().unwrap(), 0);
        // 幂等：再 recover 0 行。
        assert_eq!(store.recover_interrupted().unwrap(), 0);
    }

    #[test]
    fn rebuild_reprojects_fts_from_catalog_and_advances_generation() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"a");
        let b = sid(IdKind::Message, b"b");
        let entries = [
            (
                a.clone(),
                b"user\talpha searchable".to_vec(),
                "alpha searchable".into(),
            ),
            (
                b.clone(),
                b"assistant\tbeta searchable".to_vec(),
                "beta searchable".into(),
            ),
        ];
        store.commit_batch(&entries).unwrap();
        assert_eq!(store.active_generation().unwrap(), 1);

        let n = store.rebuild_index().unwrap();
        assert_eq!(n, 2, "rebuild 应重新索引全部 catalog 实体");
        // rebuild 是显式维护动作：即便内容一致也推进 generation。
        assert_eq!(store.active_generation().unwrap(), 2);
        // 重建后搜索仍可命中，且 id 无损。
        assert_eq!(store.query("alpha", 10).unwrap().len(), 1);
        assert_eq!(store.query("beta", 10).unwrap().len(), 1);
        assert_eq!(store.query("alpha", 10).unwrap()[0].id, a);
    }

    #[test]
    fn rebuild_removes_orphan_fts_rows_not_in_catalog() {
        let store = SqliteStore::open_in_memory().unwrap();
        let real = sid(IdKind::Message, b"real");
        store
            .commit_batch(&[(
                real.clone(),
                b"user\treal body".to_vec(),
                "real body".into(),
            )])
            .unwrap();
        // 直接往 FTS 塞一条 catalog 里没有的孤儿行，模拟索引漂移。
        let orphan = sid(IdKind::Message, b"orphan");
        store.index(&orphan, "orphan drifted body").unwrap();
        assert_eq!(store.query("drifted", 10).unwrap().len(), 1);

        // rebuild 从 catalog 权威重投影：孤儿行应被清除，真实行保留。
        store.rebuild_index().unwrap();
        assert!(
            store.query("drifted", 10).unwrap().is_empty(),
            "rebuild 应清除不在 catalog 中的孤儿 FTS 行"
        );
        assert_eq!(store.query("real", 10).unwrap().len(), 1);
    }

    #[test]
    fn rebuild_restores_search_after_index_data_wiped() {
        // 验证 ADR-0001 的“Catalog 权威、Search 可删除重建”不变量：
        // 直接清空全文索引数据（模拟索引损坏/删除），rebuild 应仅凭权威 catalog 完全恢复搜索。
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"survivor-a");
        let b = sid(IdKind::Message, b"survivor-b");
        store
            .commit_batch(&[
                (
                    a.clone(),
                    b"user\tthe catalog is authoritative".to_vec(),
                    "the catalog is authoritative".into(),
                ),
                (
                    b.clone(),
                    b"assistant\tsearch is a derived projection".to_vec(),
                    "search is a derived projection".into(),
                ),
            ])
            .unwrap();
        assert_eq!(store.query("authoritative", 10).unwrap().len(), 1);

        // 删除全文引擎索引数据（catalog 保持不动，作为权威事实源）。
        {
            let conn = store.conn.borrow();
            conn.execute("DELETE FROM fts", []).unwrap();
            conn.execute("DELETE FROM fts_ids", []).unwrap();
        }
        assert!(
            store.query("authoritative", 10).unwrap().is_empty(),
            "清空后搜索应无结果"
        );
        // catalog 仍完好——rebuild 的权威来源未受影响。
        assert_eq!(store.count().unwrap(), 2);

        let n = store.rebuild_index().unwrap();
        assert_eq!(n, 2, "rebuild 应从 catalog 恢复全部 2 条");
        // 搜索完全恢复，两条都可命中。
        assert_eq!(store.query("authoritative", 10).unwrap().len(), 1);
        assert_eq!(store.query("projection", 10).unwrap().len(), 1);
        // 结果集等价性在 wire id 层面成立——catalog 权威保留的正是 wire 串（其主键）。
        // 注意 stability：fts_ids 边车一并被清空后，身份只能从 catalog wire 串还原，
        // 按域模型（StableId::from_wire）降级为 Unstable，与 catalog-only 的 `list` 读一致。
        // 这落在回滚 runbook“在声明的 identity stability 范围内等价”的语义内：全文索引
        // 数据（含 fts_ids 边车）被删除时，声明的 stability 范围即 Unstable。
        assert_eq!(
            store.query("authoritative", 10).unwrap()[0].id.as_str(),
            a.as_str()
        );
        assert_eq!(
            store.query("projection", 10).unwrap()[0].id.as_str(),
            b.as_str()
        );
        assert_eq!(
            store.query("authoritative", 10).unwrap()[0].id.stability(),
            Stability::Unstable,
            "全文索引数据被整体清空后，身份从 catalog wire 还原为 Unstable"
        );
    }

    #[test]
    fn rebuild_on_empty_catalog_yields_empty_index() {
        let store = SqliteStore::open_in_memory().unwrap();
        let n = store.rebuild_index().unwrap();
        assert_eq!(n, 0);
        assert_eq!(store.active_generation().unwrap(), 1);
        assert!(store.query("anything", 10).unwrap().is_empty());
    }

    #[test]
    fn rebuild_leaves_durable_activated_journal_row() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"journal");
        store
            .commit_batch(&[(a, b"user\tjournal body".to_vec(), "journal body".into())])
            .unwrap();
        let before = store.active_generation().unwrap();
        store.rebuild_index().unwrap();
        // 找到本次 rebuild 产生的 activated 批次：target = before + 1。
        let conn = store.conn.borrow();
        let (state, target): (String, i64) = conn
            .query_row(
                "SELECT state, target_generation FROM index_batches
                 ORDER BY target_generation DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "activated");
        assert_eq!(target as u64, before + 1);
    }

    #[test]
    fn interrupted_batch_count_observes_without_mutating() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"a");
        let entries = [(a, b"x".to_vec(), "x".into())];
        // 干净：0 个待收敛。
        assert_eq!(store.interrupted_batch_count().unwrap(), 0);
        // 写 durable intent 但不 commit（模拟崩溃前）：1 个 building。
        let _pending = store.begin_index_batch(&entries, &[]).unwrap();
        assert_eq!(store.interrupted_batch_count().unwrap(), 1);
        // 只读观测不改状态：再查仍是 1，且 recover 仍能收敛它。
        assert_eq!(store.interrupted_batch_count().unwrap(), 1);
        assert_eq!(store.recover_interrupted().unwrap(), 1);
        assert_eq!(store.interrupted_batch_count().unwrap(), 0);
    }

    #[test]
    fn recover_does_not_touch_activated_batches() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"a");
        let entries = [(a, b"x".to_vec(), "x".into())];
        let pending = store.begin_index_batch(&entries, &[]).unwrap();
        store.commit_index_batch(&pending, &entries, &[]).unwrap();
        assert_eq!(store.recover_interrupted().unwrap(), 0);
        assert_eq!(
            store
                .index_batch(&pending.operation_id)
                .unwrap()
                .unwrap()
                .state,
            "activated"
        );
    }

    #[test]
    fn v1_db_migrates_to_v2_with_generation_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        let p = path.to_string_lossy().into_owned();
        // 手工造一个 v1 库（只有 catalog + fts，无 store_metadata）。
        {
            let conn = rusqlite::Connection::open(&p).unwrap();
            conn.execute_batch(
                "CREATE TABLE catalog (
                     id TEXT PRIMARY KEY,
                     payload BLOB NOT NULL
                 );
                 CREATE VIRTUAL TABLE fts USING fts5(id UNINDEXED, text);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO catalog(id, payload) VALUES('msg_v1_legacy', x'01')",
                [],
            )
            .unwrap();
        }
        // 新二进制打开：自动迁到 v2，数据保留，generation 从 0 起步。
        let store = SqliteStore::open(&p).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(store.active_generation().unwrap(), 0);
        let id = StableId::from_wire("msg_v1_legacy").unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap(), vec![1u8]);
    }
}
