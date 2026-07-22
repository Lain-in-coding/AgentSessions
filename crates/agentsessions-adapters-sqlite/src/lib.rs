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

use agentsessions_domain::StableId;
use agentsessions_ports::{
    CatalogEntry, CatalogStore, PortError, PortResult, SearchHit, SearchIndex,
};
use rusqlite::{Connection, OptionalExtension};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 把任意可显示的底层错误归一为端口层的 `PortError::Backend`。
fn backend<E: std::fmt::Display>(e: E) -> PortError {
    PortError::Backend(e.to_string())
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
/// 只有整批全部源都 stage 成功后才提交，满足计划 §16.3“完整成功 scan 后才确认 missing”。
pub struct SourceBatch {
    /// 该源的稳定标识（当前用其只读路径字符串）。
    pub source_path: String,
    /// 本次 scan 得到的全部 (message id, catalog payload, 索引正文)。
    pub entries: Vec<(StableId, Vec<u8>, String)>,
}

/// 提交时落库的 source→message membership 快照（内部使用）。
struct SourceMembership {
    source_path: String,
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
    /// Drop 时释放——落实计划 §6.5 / 审查 #5。
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

    /// 打开并把 schema 迁移到当前版本（migration/rebuild 的地基，计划 §14 0.2）。
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
    /// 落实计划 §16.3：只有完整成功 scan（所有源都已 stage）才据此确认 missing/tombstone。
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
        let mut merged: BTreeMap<String, (StableId, Vec<u8>, String)> = BTreeMap::new();
        let mut incoming_ids = BTreeSet::new();
        let mut deletes: Vec<StableId> = Vec::new();
        for source in sources {
            let present: BTreeSet<&str> = source
                .entries
                .iter()
                .map(|(id, _, _)| id.as_str())
                .collect();
            if present.len() != source.entries.len() {
                return Err(PortError::Backend(format!(
                    "source {} contains duplicate message ids",
                    source.source_path
                )));
            }
            for (id, payload, text) in &source.entries {
                incoming_ids.insert(id.as_str());
                if let Some((_, old_payload, old_text)) = merged.get(id.as_str()) {
                    if old_payload != payload || old_text != text {
                        return Err(PortError::Backend(format!(
                            "message {} has conflicting projections across sources",
                            id.as_str()
                        )));
                    }
                } else {
                    merged.insert(
                        id.as_str().to_string(),
                        (id.clone(), payload.clone(), text.clone()),
                    );
                }
            }
            if self.source_was_scanned(&source.source_path)? {
                for prior in self.source_message_ids(&source.source_path)? {
                    if !present.contains(prior.as_str())
                        && !incoming_ids.contains(prior.as_str())
                        && !self.message_referenced_by_other_source(&prior, &source.source_path)?
                    {
                        let id = StableId::from_wire(&prior).ok_or_else(|| {
                            PortError::Backend(format!("invalid membership id: {prior}"))
                        })?;
                        deletes.push(id);
                    }
                }
            }
        }
        let upserts: Vec<(StableId, Vec<u8>, String)> = merged.into_values().collect();

        // 校验整体变更集合（重复/交叠即拒绝），再判定是否内容级 no-op。
        batch_manifest(&upserts, &deletes)?;
        if self.source_batches_are_current(sources, &upserts)? {
            return Ok(false);
        }

        let membership: Vec<SourceMembership> = sources
            .iter()
            .map(|s| SourceMembership {
                source_path: s.source_path.clone(),
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

    fn message_referenced_by_other_source(
        &self,
        message_id: &str,
        source_path: &str,
    ) -> PortResult<bool> {
        let conn = self.conn.borrow();
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM source_membership
                 WHERE message_id = ?1 AND source_path <> ?2 LIMIT 1",
                rusqlite::params![message_id, source_path],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(exists.is_some())
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
    /// 落实计划 §6.5 Outbox 状态机的第一个 durable point——在任何 catalog/FTS
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
    /// 防止旧基线覆盖更新的同步结果（计划 §6.5）。
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
                    "INSERT INTO source_membership(source_path, message_id) VALUES(?1, ?2)",
                    rusqlite::params![&source.source_path, message_id],
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
    /// 重建期崩溃不污染当前活动 generation（计划 §14 0.2 `index rebuild`）。
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
    /// 供 doctor 等只读路径观测"有多少无副作用 intent 尚待下次写打开收敛"，
    /// 落实 0.2 退出条件"中断恢复/generation 一致性有证据"（计划 §14）。
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
pub const SCHEMA_VERSION: i64 = 5;

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
        let mut stmt = conn
            .prepare("SELECT id, bm25(fts) FROM fts WHERE fts MATCH ?1 ORDER BY bm25(fts) LIMIT ?2")
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
        assert_eq!(store.schema_version().unwrap(), 5);
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
        // 落实 0.2 退出条件“Catalog/Search 可删除重建”与回滚 runbook（计划 §14 行 1382）：
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
        assert_eq!(store.schema_version().unwrap(), 5);
        assert_eq!(store.active_generation().unwrap(), 0);
        let id = StableId::from_wire("msg_v1_legacy").unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap(), vec![1u8]);
    }
}
