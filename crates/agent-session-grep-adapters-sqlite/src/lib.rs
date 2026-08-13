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

use agent_session_grep_domain::{
    EvidenceSpan, IdKind, Message, MessageEdge, MessagePlacement, MessageRelation, PlacementId,
    Role, SessionContextGraph, SourceDocument, StableId,
};
use agent_session_grep_ports::{
    CatalogEntry, CatalogStore, ContextGraphStore, ContextStats, MessageContextCandidate,
    PortError, PortResult, SearchHit, SearchIndex,
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

/// 批量 `IN (...)` 查询的单块 id 上限。SQLite 的变量上限是 999（旧版）/
/// 32766（3.32+），一个大 batch 的 placement/entity 数远超此限，必须分块。
const BATCH_IN_CHUNK: usize = 500;

/// 把 id 列表切成不超过 [`BATCH_IN_CHUNK`] 的块（每块一个 `IN (...)` 查询）。
fn chunk_ids<T: AsRef<str>>(ids: &[T]) -> Vec<&[T]> {
    ids.chunks(BATCH_IN_CHUNK).collect()
}

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
/// 约定：现代 ingest/sync 写入的是完整 JSON payload（`{"role":..,"text":..,..}`），
/// 先尝试解析 JSON 取 `text` 字段；解析失败再回退历史格式——payload 里首个制表符
/// 之前是 role 前缀、之后是消息正文，无制表符则整体即正文（切片期 `index` 命令写入的
/// 无前缀纯文本）。因此仅凭 catalog 即可无损重建 FTS 投影，无需依赖可能已损坏/丢失的
/// 旧 FTS 内容。
///
/// 已知限制：切片期 `index` 命令若写入本身含制表符的正文，历史格式投影会截断到首个
/// 制表符之后——该命令仅供切片期测试，真实数据均经 ingest/sync 以 JSON payload 写入。
fn searchable_text(payload: &[u8]) -> String {
    // Modern shape: `{"role":...,"text":...,...}`. Indexing the raw JSON would
    // let structural tokens (`user`, `null`, `sessions`) match every message.
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(payload) {
        if let Some(text) = value.get("text").and_then(serde_json::Value::as_str) {
            return text.to_string();
        }
        // JSON that lacks a string `text` field must not fall back to
        // indexing the raw JSON (structural-token pollution). It carries no
        // searchable body.
        return String::new();
    }
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
/// sessions. The `session` back-reference becomes a union (`sessions`, sorted,
/// with `session` kept as a single-value alias). The content projection
/// (`text`) is exempt from conflict authority too: a copy may carry a different
/// number of content blocks than the original (e.g. a truncated tool_result),
/// so the merged value deterministically keeps the longer projection and no
/// retrieved content is lost. `span`/`spans` are unioned keyed by contributing
/// document, and the contextual aliases (`parent`, `parent_native_id`,
/// `is_sidechain`, `seq`) may differ because they are regenerated from v7
/// relations once every contributing source is relation-complete. Only the
/// remaining stable fields must still agree byte-for-byte; a message whose
/// stable projection depends on which file it came from is a real
/// inconsistency and is still rejected.
fn merge_message_payloads(_wire: &str, left: &[u8], right: &[u8]) -> PortResult<Vec<u8>> {
    let parse = |bytes: &[u8]| -> PortResult<serde_json::Map<String, serde_json::Value>> {
        match serde_json::from_slice::<serde_json::Value>(bytes) {
            Ok(serde_json::Value::Object(map)) => Ok(map),
            // Slice-era rows hold bare text rather than canonical JSON. Those
            // cannot be reconciled field by field, so the conflict stands.
            _ => Err(PortError::Backend(
                "message has conflicting projections across sources".into(),
            )),
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
                    return Err(PortError::Backend(
                        "message has a span without a document reference".into(),
                    ));
                };
                let document = document.to_string();
                // 同一 document 的多次出现按字段联合（右侧覆盖左侧）：保留左侧
                // 独有的字段——最典型的是 v7 再生回写的 `placement_id`——使 merge
                // 结果与再生后的 stored 逐字节一致，重同步才能收敛为内容级 no-op；
                // 偏移更新仍生效（右侧的 start/end 覆盖左侧）。
                match spans.get(&document).cloned() {
                    Some(mut existing) if entry.is_object() && existing.is_object() => {
                        let existing_obj = existing.as_object_mut().expect("checked is_object");
                        for (key, value) in entry.as_object().expect("checked is_object") {
                            existing_obj.insert(key.clone(), value.clone());
                        }
                        spans.insert(document, existing);
                    }
                    _ => {
                        spans.insert(document, entry.clone());
                    }
                }
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

    // Only stable Message fields are conflict authority. Contextual compatibility
    // aliases may differ and are regenerated from v7 relations once every known
    // contributing source is relation-complete.
    for (a, b) in [(&left_map, &right_map), (&right_map, &left_map)] {
        for (key, value) in a {
            if matches!(
                key.as_str(),
                "session"
                    | "sessions"
                    | "span"
                    | "spans"
                    | "parent"
                    | "parent_native_id"
                    | "is_sidechain"
                    | "seq"
                    | "text"
            ) {
                continue;
            }
            if b.get(key) != Some(value) {
                // Codex's old adapter stored the occurrence-local outer
                // envelope timestamp as the message timestamp; the current
                // adapter emits no stable timestamp (different occurrences
                // carry different envelope timestamps). Re-ingesting such a
                // source therefore compares a string against null for the
                // same stable message, which must not be a conflict: the
                // merged value is null (no stable timestamp exists). A
                // missing key is treated like an explicit null for this
                // convergence.
                if key == "timestamp"
                    && matches!(
                        (value, b.get(key)),
                        (serde_json::Value::String(_), Some(serde_json::Value::Null))
                            | (serde_json::Value::Null, Some(serde_json::Value::String(_)))
                            | (serde_json::Value::String(_), None)
                            | (serde_json::Value::Null, None)
                    )
                {
                    continue;
                }
                return Err(PortError::Backend(
                    "message has conflicting projections across sources".into(),
                ));
            }
        }
    }

    // Timestamp is occurrence-local for Codex: an old adapter wrote the outer
    // envelope timestamp, the current one emits none. When projections disagree
    // on it (string vs null), converge deterministically on null regardless of
    // which projection happens to be on the left. A missing key is treated like
    // an explicit null for this convergence (the conflict check above already
    // treats them the same), so (string, missing) also converges to null
    // instead of keeping the string.
    let timestamp_states: [Option<bool>; 2] =
        [&left_map, &right_map].map(|map| map.get("timestamp").map(serde_json::Value::is_null));
    let timestamp_converges_to_null = timestamp_states
        .iter()
        .any(|state| matches!(state, Some(false)))
        && timestamp_states
            .iter()
            .any(|state| state.is_none_or(|is_null| is_null));

    let mut merged = left_map;
    if timestamp_converges_to_null {
        merged.insert("timestamp".to_string(), serde_json::Value::Null);
    }
    // `text` is a content projection, not a stable identity field: Claude Code
    // copies a conversation's history into the new transcript when a session is
    // resumed or forked, and a copy may carry a different number of content
    // blocks than the original (e.g. a truncated tool_result). The projections
    // legitimately differ, so text is exempt from conflict authority; the
    // merged value deterministically keeps the longer projection so no
    // retrieved content is lost.
    match (&merged.get("text"), right_map.get("text")) {
        (Some(left_text), Some(right_text)) => {
            let left_len = left_text.as_str().map_or(0, |s| s.len());
            let right_len = right_text.as_str().map_or(0, |s| s.len());
            if right_len > left_len {
                merged.insert("text".to_string(), right_text.clone());
            }
        }
        (None, Some(right_text)) => {
            merged.insert("text".to_string(), right_text.clone());
        }
        _ => {}
    }
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
fn merge_session_payloads(_wire: &str, left: &[u8], right: &[u8]) -> PortResult<Vec<u8>> {
    fn parse(bytes: &[u8]) -> PortResult<serde_json::Value> {
        serde_json::from_slice(bytes).map_err(|error| {
            PortError::Backend(format!("session payload is not canonical JSON: {error}"))
        })
    }

    let left_value = parse(left)?;
    let right_value = parse(right)?;

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
            .ok_or_else(|| PortError::Backend("session payload lacks a messages array".into()))?;
        for entry in list {
            let member = entry.as_str().ok_or_else(|| {
                PortError::Backend("session has a non-string message member".into())
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

/// One relation row to insert or replace in a relation-aware batch.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RelationUpsertManifest {
    Placement(MessagePlacement),
    Edge(MessageEdge),
}

impl RelationUpsertManifest {
    fn canonical_key(&self) -> String {
        match self {
            Self::Placement(placement) => format!("placement:{}", placement.id.as_str()),
            Self::Edge(edge) => format!("edge:{}", edge.child_placement_id.as_str()),
        }
    }

    fn canonical_value(&self) -> serde_json::Value {
        match self {
            Self::Placement(placement) => serde_json::json!({
                "kind": "message_placement",
                "placement": placement,
            }),
            Self::Edge(edge) => serde_json::json!({
                "kind": "message_edge",
                "edge": edge,
            }),
        }
    }
}

/// One relation row to delete in a relation-aware batch.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RelationDeleteManifest {
    Placement(PlacementId),
    Edge(PlacementId),
}

impl RelationDeleteManifest {
    fn canonical_key(&self) -> String {
        match self {
            Self::Placement(id) => format!("placement:{}", id.as_str()),
            Self::Edge(id) => format!("edge:{}", id.as_str()),
        }
    }

    fn canonical_value(&self) -> serde_json::Value {
        match self {
            Self::Placement(id) => serde_json::json!({
                "kind": "message_placement",
                "placement_id": id,
            }),
            Self::Edge(id) => serde_json::json!({
                "kind": "message_edge",
                "child_placement_id": id,
            }),
        }
    }
}

/// One durable source-to-entity membership row.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SourceEntityMembershipManifest {
    entity_id: String,
    document_id: Option<String>,
}

impl SourceEntityMembershipManifest {
    fn canonical_value(&self) -> serde_json::Value {
        serde_json::json!({
            "entity_id": self.entity_id,
            "document_id": self.document_id,
        })
    }
}

/// Complete source-scoped state after applying one scan.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceReplacementManifest {
    source_path: String,
    entity_memberships: Vec<SourceEntityMembershipManifest>,
    placement_ids: Vec<PlacementId>,
    relation_complete: bool,
    /// 捕获时源字节长度与内容指纹（source-scan 指纹缓存）。
    len_bytes: Option<i64>,
    fingerprint: Option<String>,
}

impl SourceReplacementManifest {
    fn canonical_value(&self) -> serde_json::Value {
        let mut entity_memberships = self.entity_memberships.clone();
        entity_memberships.sort();
        let entity_memberships: Vec<_> = entity_memberships
            .iter()
            .map(SourceEntityMembershipManifest::canonical_value)
            .collect();
        let mut placement_ids = self.placement_ids.clone();
        placement_ids.sort();
        serde_json::json!({
            "source_path": self.source_path,
            "entity_memberships": entity_memberships,
            "placement_ids": placement_ids,
            "relation_complete": self.relation_complete,
            "len_bytes": self.len_bytes,
            "fingerprint": self.fingerprint,
        })
    }
}

/// Canonical relation/source manifests stored beside the entity manifest.
#[derive(Debug, Clone, Default)]
struct RelationManifests {
    relation_upserts: Vec<RelationUpsertManifest>,
    relation_deletes: Vec<RelationDeleteManifest>,
    source_replacements: Vec<SourceReplacementManifest>,
}

impl RelationManifests {
    fn validate(&self) -> PortResult<()> {
        for upsert in &self.relation_upserts {
            match upsert {
                RelationUpsertManifest::Placement(placement) => {
                    validate_placement(placement)?;
                }
                RelationUpsertManifest::Edge(edge) => validate_edge(edge)?,
            }
        }
        let mut upsert_keys: Vec<_> = self
            .relation_upserts
            .iter()
            .map(RelationUpsertManifest::canonical_key)
            .collect();
        upsert_keys.sort();
        if upsert_keys.windows(2).any(|window| window[0] == window[1]) {
            return Err(PortError::Backend(
                "index batch contains duplicate relation upserts".into(),
            ));
        }

        let mut delete_keys: Vec<_> = self
            .relation_deletes
            .iter()
            .map(RelationDeleteManifest::canonical_key)
            .collect();
        delete_keys.sort();
        if delete_keys.windows(2).any(|window| window[0] == window[1]) {
            return Err(PortError::Backend(
                "index batch contains duplicate relation deletes".into(),
            ));
        }
        if upsert_keys
            .iter()
            .any(|key| delete_keys.binary_search(key).is_ok())
        {
            return Err(PortError::Backend(
                "index batch cannot upsert and delete the same relation".into(),
            ));
        }

        let mut source_paths: Vec<_> = self
            .source_replacements
            .iter()
            .map(|replacement| replacement.source_path.as_str())
            .collect();
        source_paths.sort_unstable();
        if source_paths.windows(2).any(|window| window[0] == window[1]) {
            return Err(PortError::Backend(
                "index batch contains duplicate source replacements".into(),
            ));
        }

        for replacement in &self.source_replacements {
            let mut entity_ids: Vec<_> = replacement
                .entity_memberships
                .iter()
                .map(|membership| membership.entity_id.as_str())
                .collect();
            entity_ids.sort_unstable();
            if entity_ids.windows(2).any(|window| window[0] == window[1]) {
                return Err(PortError::Backend(
                    "source replacement contains duplicate entity memberships".into(),
                ));
            }
            for membership in &replacement.entity_memberships {
                StableId::from_wire(&membership.entity_id).ok_or_else(|| {
                    PortError::Backend("source replacement has an invalid entity id".into())
                })?;
                if let Some(document_id) = &membership.document_id {
                    let document_id = StableId::from_wire(document_id).ok_or_else(|| {
                        PortError::Backend("source replacement has an invalid document id".into())
                    })?;
                    if document_id.kind() != IdKind::Document {
                        return Err(PortError::Backend(
                            "source membership document id has wrong kind".into(),
                        ));
                    }
                }
            }

            let mut placement_ids = replacement.placement_ids.clone();
            placement_ids.sort();
            if placement_ids
                .windows(2)
                .any(|window| window[0] == window[1])
            {
                return Err(PortError::Backend(
                    "source replacement contains duplicate placement claims".into(),
                ));
            }
        }
        Ok(())
    }

    fn canonical_json(&self) -> PortResult<(String, String, String)> {
        self.validate()?;
        let mut upserts: Vec<_> = self.relation_upserts.iter().collect();
        upserts.sort_by_key(|item| item.canonical_key());
        let upserts: Vec<_> = upserts
            .into_iter()
            .map(RelationUpsertManifest::canonical_value)
            .collect();

        let mut deletes: Vec<_> = self.relation_deletes.iter().collect();
        deletes.sort_by_key(|item| item.canonical_key());
        let deletes: Vec<_> = deletes
            .into_iter()
            .map(RelationDeleteManifest::canonical_value)
            .collect();

        let mut replacements: Vec<_> = self.source_replacements.iter().collect();
        replacements.sort_by(|left, right| left.source_path.cmp(&right.source_path));
        let replacements: Vec<_> = replacements
            .into_iter()
            .map(SourceReplacementManifest::canonical_value)
            .collect();

        Ok((
            serde_json::to_string(&upserts).map_err(backend)?,
            serde_json::to_string(&deletes).map_err(backend)?,
            serde_json::to_string(&replacements).map_err(backend)?,
        ))
    }
}

/// Canonical durable representation of one generation change set.
struct CanonicalBatchManifest {
    upsert_ids: Vec<String>,
    delete_ids: Vec<String>,
    relation_upserts_json: String,
    relation_deletes_json: String,
    source_replacements_json: String,
    operation_digest: String,
}

/// Canonicalize and fingerprint one generation change set.
///
/// Sorting by wire ID makes the digest independent of discovery order. Duplicate IDs and
/// upsert/delete overlap are rejected so the journal always describes an unambiguous set.
fn batch_manifest(
    upserts: &[(StableId, Vec<u8>, String)],
    deletes: &[StableId],
    relations: &RelationManifests,
) -> PortResult<CanonicalBatchManifest> {
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
    // 数据兼容性：分隔串刻意保留旧名 `agentsessions`——operation_digest 持久化在
    // index_batches 表并与既有 data root 中已存摘要交叉比对，改名会破坏 v7 数据兼容。
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

    let (relation_upserts_json, relation_deletes_json, source_replacements_json) =
        relations.canonical_json()?;
    hash_field(&mut hasher, b"relation_upserts");
    hash_field(&mut hasher, relation_upserts_json.as_bytes());
    hash_field(&mut hasher, b"relation_deletes");
    hash_field(&mut hasher, relation_deletes_json.as_bytes());
    hash_field(&mut hasher, b"source_replacements");
    hash_field(&mut hasher, source_replacements_json.as_bytes());

    Ok(CanonicalBatchManifest {
        upsert_ids,
        delete_ids,
        relation_upserts_json,
        relation_deletes_json,
        source_replacements_json,
        operation_digest: hasher.finalize().to_hex().to_string(),
    })
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
    pub relation_upserts: Vec<serde_json::Value>,
    pub relation_deletes: Vec<serde_json::Value>,
    pub source_replacements: Vec<serde_json::Value>,
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
#[derive(Clone)]
pub struct SourceBatch {
    /// 该源的稳定标识（当前用其只读路径字符串）。
    pub source_path: String,
    /// 本次 scan 得到的全部 (message id, catalog payload, 索引正文)。
    ///
    /// 自 v6 起，条目不限于消息：组合根把该源派生的 session（`ses_v1_*`）与
    /// document（`doc_v1_*`）目录实体放进同一批 entries，随消息走同一事务提交、
    /// 同一 membership/tombstone 推导——源消失时容器实体随消息一起退役。
    pub entries: Vec<(StableId, Vec<u8>, String)>,
    /// 本次 source scan 观察到的全部 contextual message occurrences。
    ///
    /// B1 只携带数据；B2 才会把这些关系写入 v7 表。
    pub placements: Vec<MessagePlacement>,
    /// 本次 source scan 观察到的全部 contextual parent edges。
    pub edges: Vec<MessageEdge>,
    /// 该 source 是否完成了零 skipped 的 relation scan。
    ///
    /// B1 不提交 completeness marker；B2 将据此替换或撤销 marker。
    pub relation_complete: bool,
    /// 捕获时的源字节长度与内容指纹（source-scan 指纹缓存，用于跳过
    /// 未变化源的重复解析）。None = 未提供（测试/旧调用方）。
    pub len_bytes: Option<i64>,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredPlacement {
    session_id: String,
    document_id: String,
    message_id: String,
    source_ordinal: u32,
    is_sidechain: bool,
    span: Option<(u64, u64)>,
}

impl StoredPlacement {
    fn matches(&self, placement: &MessagePlacement) -> bool {
        self.session_id == placement.session_id.as_str()
            && self.document_id == placement.source_document_id.as_str()
            && self.message_id == placement.message_id.as_str()
            && self.source_ordinal == placement.source_ordinal
            && self.is_sidechain == placement.is_sidechain
            && self.span == placement.span.as_ref().map(|span| (span.start, span.end))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredEdge {
    parent_message_id: String,
    parent_native_id: Option<String>,
    relation: String,
}

impl StoredEdge {
    fn matches(&self, edge: &MessageEdge) -> bool {
        self.parent_message_id == edge.parent_message_id.as_str()
            && self.parent_native_id == edge.parent_native_id
            && self.relation == edge.relation.as_str()
    }
}

struct PreparedSource {
    relation_complete: bool,
    prior_entity_memberships: BTreeMap<String, Option<String>>,
    prior_placement_ids: BTreeSet<String>,
    observed_placements: BTreeMap<String, MessagePlacement>,
    observed_edges: BTreeMap<String, MessageEdge>,
    replacement: SourceReplacementManifest,
}

fn validate_placement(placement: &MessagePlacement) -> PortResult<()> {
    if placement.session_id.kind() != IdKind::Session
        || placement.source_document_id.kind() != IdKind::Document
        || placement.message_id.kind() != IdKind::Message
    {
        return Err(PortError::Backend(
            "message placement contains an entity id with the wrong kind".into(),
        ));
    }
    let expected = PlacementId::derive(
        &placement.session_id,
        &placement.source_document_id,
        &placement.message_id,
        placement.source_ordinal,
    );
    if placement.id != expected {
        return Err(PortError::Backend(
            "message placement id does not match its contextual facts".into(),
        ));
    }
    if let Some(span) = &placement.span {
        if span.end < span.start {
            return Err(PortError::Backend(
                "message placement span end precedes start".into(),
            ));
        }
        i64::try_from(span.start).map_err(backend)?;
        i64::try_from(span.end).map_err(backend)?;
    }
    Ok(())
}

fn validate_edge(edge: &MessageEdge) -> PortResult<()> {
    if edge.parent_message_id.kind() != IdKind::Message {
        return Err(PortError::Backend(
            "message edge parent id has the wrong kind".into(),
        ));
    }
    Ok(())
}

fn stored_role(value: &str) -> PortResult<Role> {
    match value {
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "system" => Ok(Role::System),
        // Codex's authoritative conversation role for the system/permission
        // layer; the codex adapter emits it verbatim (see provider-codex
        // is_conversational_role), so the read path must accept it.
        "developer" => Ok(Role::Developer),
        "tool" => Ok(Role::Tool),
        _ => Err(PortError::Backend(
            "stored message has an unsupported role".into(),
        )),
    }
}

fn stored_relation(value: &str) -> PortResult<MessageRelation> {
    match value {
        "reply" => Ok(MessageRelation::Reply),
        "retry" => Ok(MessageRelation::Retry),
        "fork" => Ok(MessageRelation::Fork),
        "continuation" => Ok(MessageRelation::Continuation),
        "subagent" => Ok(MessageRelation::Subagent),
        "tool_result" => Ok(MessageRelation::ToolResult),
        _ => Err(PortError::Backend(
            "stored message edge has an unsupported relation".into(),
        )),
    }
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

/// 一批源路径的指纹缓存项：捕获时长度与内容指纹。
pub type SourceFingerprint = (Option<i64>, Option<String>);

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
        // 裸相对文件名（如 "catalog.db"）的 parent() 是空串 ""，create_dir_all("")
        // 会报错；空 parent 按当前工作目录处理（与只读 open 一致，目录解析交给
        // Connection::open / WriterLease）。
        let data_root = match db_path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
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
                 upgrade agent-session-grep or rebuild the data root"
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
                     id_json TEXT NOT NULL UNIQUE,
                     fts_rowid INTEGER
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
                     scanned_at_ms INTEGER NOT NULL,
                     len_bytes     INTEGER,
                     fingerprint   TEXT
                 );",
            )
            .map_err(backend)?;
        }
        if current < 6 {
            // v6：source_membership 增加可空 document_id——记录各 source 所属文档实体的
            // wire id，使 source 消失的 tombstone 清理能同步退役其 session/document 目录行。
            // 旧行保持 NULL（v6 前的 membership 无文档归属信息）。
            // 幂等：v6 步骤此前可能在 PRAGMA user_version=6 之前崩溃（两个独立
            // autocommit），重跑必须容忍列已存在，否则旧库永久打不开。
            let has_document_id = conn
                .prepare("PRAGMA table_info(source_membership)")
                .map_err(backend)?
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?
                .iter()
                .any(|name| name == "document_id");
            if !has_document_id {
                conn.execute_batch("ALTER TABLE source_membership ADD COLUMN document_id TEXT;")
                    .map_err(backend)?;
            }
        }
        if current < 6 {
            // v1-v6 predate the explicit per-step transaction added for v7.
            // Mark their completed state before entering the atomic v6->v7 step.
            conn.execute_batch("PRAGMA user_version = 6;")
                .map_err(backend)?;
        }
        if current < 7 {
            Self::migrate_v6_to_v7(conn)?;
        }
        // 不随 user_version 门控：旧 v7 库（本列存在前建成的）打开时同样需要。
        Self::ensure_fts_ids_rowid(conn)?;
        Ok(())
    }

    /// 确保 `fts_ids` 边车携带 `fts_rowid` 列（v7 内的加法扩展，`user_version` 不变）。
    ///
    /// FTS5 表的 `id` 列是内容列而非 rowid，旧删除语句按内容比较会整表扫描
    /// （`SCAN fts VIRTUAL TABLE INDEX 0`），每批提交成本 O(全库)。本列把
    /// fts5 行的 rowid 回写到边车，删除改按 rowid 定位（O(1)）。
    /// 新库在 v3 建表时已带本列，此处直接短路——open（含只读 open）不再为
    /// 新库执行 ALTER+回填事务；只有 fts_rowid 列加入前建成的旧 v7 库首次
    /// 打开时走 ALTER+回填：`fts` 与 `fts_ids` 自 v3 起同事务写入、一一对应，
    /// 用 `id_json` 连接即可把 fts5 已分配的行 rowid 抄进边车；session/document
    /// 实体无 fts 行，保持 NULL（删除按 NULL 定位即无操作）。
    /// ALTER 与回填在同一事务内，崩溃不留半成品；重跑因列已存在直接短路。
    /// 并发打开旧库时 ALTER/回填可能遇 BUSY/LOCKED——经 [`backend`] 归一为
    /// 可重试的 WriterBusy，调用方应重试而非当作锁损坏。
    fn ensure_fts_ids_rowid(conn: &Connection) -> PortResult<()> {
        let has_fts_rowid = conn
            .prepare("PRAGMA table_info(fts_ids)")
            .map_err(backend)?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?
            .iter()
            .any(|name| name == "fts_rowid");
        if has_fts_rowid {
            return Ok(());
        }
        let tx = conn.unchecked_transaction().map_err(backend)?;
        tx.execute_batch("ALTER TABLE fts_ids ADD COLUMN fts_rowid INTEGER;")
            .map_err(backend)?;
        let rows: Vec<(String, i64)> = {
            let mut stmt = tx
                .prepare(
                    "SELECT fi.wire_id, f.rowid
                     FROM fts f JOIN fts_ids fi ON fi.id_json = f.id",
                )
                .map_err(backend)?;
            let mapped = stmt
                .query_map([], |row| {
                    let wire: String = row.get(0)?;
                    let rid: i64 = row.get(1)?;
                    Ok((wire, rid))
                })
                .map_err(backend)?;
            let mut out = Vec::new();
            for row in mapped {
                out.push(row.map_err(backend)?);
            }
            out
        };
        for (wire_id, fts_rowid) in rows {
            tx.execute(
                "UPDATE fts_ids SET fts_rowid = ?2 WHERE wire_id = ?1",
                rusqlite::params![wire_id, fts_rowid],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(())
    }

    /// Add the v7 relational schema in one explicit transaction.
    ///
    /// Legacy catalog and source-membership rows are retained byte-for-byte.
    /// No placement, edge, source claim, or relation-complete marker can be
    /// reconstructed safely from v6 aliases, so all new relation tables start
    /// empty. `user_version = 7` is part of the same transaction as the DDL.
    fn migrate_v6_to_v7(conn: &Connection) -> PortResult<()> {
        Self::migrate_v6_to_v7_inner(conn, false)
    }

    fn migrate_v6_to_v7_inner(conn: &Connection, inject_failure: bool) -> PortResult<()> {
        let tx = conn.unchecked_transaction().map_err(backend)?;
        tx.execute_batch(
            "CREATE TABLE message_placements (
                 placement_id   TEXT PRIMARY KEY,
                 session_id     TEXT NOT NULL,
                 document_id    TEXT NOT NULL,
                 message_id     TEXT NOT NULL,
                 source_ordinal INTEGER NOT NULL CHECK(source_ordinal >= 0),
                 is_sidechain   INTEGER NOT NULL CHECK(is_sidechain IN (0, 1)),
                 byte_start     INTEGER,
                 byte_end       INTEGER,
                 CHECK(
                     (byte_start IS NULL AND byte_end IS NULL)
                     OR (byte_start >= 0 AND byte_end >= byte_start)
                 ),
                 UNIQUE(session_id, document_id, source_ordinal)
             );
             CREATE TABLE message_edges (
                 child_placement_id TEXT PRIMARY KEY,
                 parent_message_id  TEXT NOT NULL,
                 parent_native_id   TEXT,
                 relation           TEXT NOT NULL
             );
             CREATE TABLE source_placement_membership (
                 source_path  TEXT NOT NULL,
                 placement_id TEXT NOT NULL,
                 PRIMARY KEY(source_path, placement_id)
             );
             CREATE TABLE source_relation_scans (
                 source_path             TEXT PRIMARY KEY,
                 relation_schema_version INTEGER NOT NULL
                     CHECK(relation_schema_version >= 7)
             );
             CREATE INDEX message_placements_session_order
             ON message_placements(session_id, document_id, source_ordinal, placement_id);
             CREATE INDEX message_placements_message
             ON message_placements(message_id);
             CREATE INDEX message_placements_document
             ON message_placements(document_id);
             CREATE INDEX source_placement_membership_placement
             ON source_placement_membership(placement_id);
             ALTER TABLE index_batches
             ADD COLUMN relation_upserts_json TEXT NOT NULL DEFAULT '[]';
             ALTER TABLE index_batches
             ADD COLUMN relation_deletes_json TEXT NOT NULL DEFAULT '[]';
             ALTER TABLE index_batches
             ADD COLUMN source_replacements_json TEXT NOT NULL DEFAULT '[]';
             PRAGMA user_version = 7;",
        )
        .map_err(backend)?;

        // Source-scan fingerprint cache columns (additive within v7): used by
        // the CLI to skip re-parsing sources whose bytes are unchanged.
        // Idempotent for catalogs that reached v7 before these columns
        // existed.
        let has_len_bytes: bool = tx
            .prepare("PRAGMA table_info(source_scans)")
            .map_err(backend)?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?
            .iter()
            .any(|name| name == "len_bytes");
        if !has_len_bytes {
            tx.execute_batch(
                "ALTER TABLE source_scans ADD COLUMN len_bytes INTEGER;
                 ALTER TABLE source_scans ADD COLUMN fingerprint TEXT;",
            )
            .map_err(backend)?;
        }

        if inject_failure {
            return Err(PortError::Backend(
                "injected v6-to-v7 migration failure".into(),
            ));
        }

        tx.commit().map_err(backend)
    }

    /// 当前存储读回的 schema 版本（供 doctor/诊断）。
    pub fn schema_version(&self) -> PortResult<i64> {
        self.conn
            .borrow()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(backend)
    }

    /// 读取一批源路径的指纹缓存（source_scans 的 len/fingerprint 列）。
    ///
    /// 返回 `path -> (len_bytes, fingerprint)`；从未扫描过的源不在 map 中。
    /// CLI 用它跳过未变化源的重复解析（capture 后先比指纹，相同则不再
    /// parse，直接按 no-op 处理）。
    pub fn source_fingerprints(
        &self,
        paths: &[String],
    ) -> PortResult<BTreeMap<String, SourceFingerprint>> {
        let conn = self.conn.borrow();
        let mut out = BTreeMap::new();
        for path in paths {
            let row: Option<SourceFingerprint> = conn
                .query_row(
                    "SELECT len_bytes, fingerprint FROM source_scans WHERE source_path = ?1",
                    [path],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(backend)?;
            if let Some(row) = row {
                out.insert(path.clone(), row);
            }
        }
        Ok(out)
    }

    /// 读取一批源路径已提交的 message 实体数（membership 中 msg_v1_ 行数）。
    ///
    /// CLI 在指纹缓存命中、跳过 parse 时用它上报 unchanged 消息数，保持
    /// `unchanged` 与 `emitted` 同单位（消息数）。
    pub fn source_message_counts(&self, paths: &[String]) -> PortResult<BTreeMap<String, usize>> {
        let conn = self.conn.borrow();
        let mut out = BTreeMap::new();
        for path in paths {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM source_membership
                     WHERE source_path = ?1 AND message_id LIKE 'msg_v1_%'",
                    [path],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            out.insert(path.clone(), count as usize);
        }
        Ok(out)
    }

    fn stable_id_from_store(conn: &Connection, wire: &str) -> PortResult<StableId> {
        let id_json: Option<String> = conn
            .query_row(
                "SELECT id_json FROM fts_ids WHERE wire_id = ?1",
                [wire],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let id = match id_json {
            Some(json) => serde_json::from_str::<StableId>(&json).map_err(backend)?,
            None => StableId::from_wire(wire).ok_or_else(|| {
                PortError::Backend("catalog contains an invalid entity id".into())
            })?,
        };
        if id.as_str() != wire {
            return Err(PortError::Backend(
                "stored identity sidecar does not match its catalog key".into(),
            ));
        }
        Ok(id)
    }

    /// 批量加载场景下的身份解析：优先用已加载的 fts_ids 映射，缺失回退
    /// `from_wire`（与 `stable_id_from_store` 语义一致，避免逐条查询）。
    fn stable_id_from_wire(
        wire: &str,
        id_json_by_wire: &BTreeMap<String, String>,
    ) -> PortResult<StableId> {
        let id = match id_json_by_wire.get(wire) {
            Some(json) => serde_json::from_str::<StableId>(json).map_err(backend)?,
            None => StableId::from_wire(wire).ok_or_else(|| {
                PortError::Backend("catalog contains an invalid entity id".into())
            })?,
        };
        if id.as_str() != wire {
            return Err(PortError::Backend(
                "stored identity sidecar does not match its catalog key".into(),
            ));
        }
        Ok(id)
    }

    fn ensure_stored_identity_metadata_matches(
        &self,
        entries: &[(StableId, Vec<u8>, String)],
    ) -> PortResult<()> {
        let conn = self.conn.borrow();
        // prepare 提升到循环外：200K 实体 × 每次 prepare/finalize 的常数因子
        // 在批量提交里会被放大（见 commit_index_batch_with_relations 的同类 hoist）。
        let mut stmt = conn
            .prepare("SELECT id_json FROM fts_ids WHERE wire_id = ?1")
            .map_err(backend)?;
        for (id, _, _) in entries {
            let id_json: Option<String> = stmt
                .query_row([id.as_str()], |row| row.get(0))
                .optional()
                .map_err(backend)?;
            let Some(id_json) = id_json else {
                continue;
            };
            let stored_id: StableId = serde_json::from_str(&id_json)
                .map_err(|_| PortError::Backend("stored identity sidecar is not valid".into()))?;
            if &stored_id != id {
                return Err(PortError::Backend(
                    "entity has conflicting identity metadata with stored catalog".into(),
                ));
            }
        }
        Ok(())
    }

    fn relation_sources_for_session(
        conn: &Connection,
        session_id: &str,
    ) -> PortResult<BTreeSet<String>> {
        let mut stmt = conn
            .prepare(
                "SELECT source_path FROM source_membership WHERE message_id = ?1
                 UNION
                 SELECT claims.source_path
                 FROM source_placement_membership AS claims
                 JOIN message_placements AS placements
                   ON placements.placement_id = claims.placement_id
                 WHERE placements.session_id = ?1",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([session_id], |row| row.get::<_, String>(0))
            .map_err(backend)?;
        let mut sources = BTreeSet::new();
        for row in rows {
            sources.insert(row.map_err(backend)?);
        }
        Ok(sources)
    }

    fn relation_sources_for_message(
        conn: &Connection,
        message_id: &str,
    ) -> PortResult<BTreeSet<String>> {
        let mut stmt = conn
            .prepare(
                "SELECT source_path FROM source_membership WHERE message_id = ?1
                 UNION
                 SELECT claims.source_path
                 FROM source_placement_membership AS claims
                 JOIN message_placements AS placements
                   ON placements.placement_id = claims.placement_id
                 WHERE placements.message_id = ?1",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([message_id], |row| row.get::<_, String>(0))
            .map_err(backend)?;
        let mut sources = BTreeSet::new();
        for row in rows {
            sources.insert(row.map_err(backend)?);
        }
        Ok(sources)
    }

    fn require_relation_complete_sources(
        conn: &Connection,
        sources: &BTreeSet<String>,
        require_known_source: bool,
        subject: &str,
    ) -> PortResult<()> {
        if require_known_source && sources.is_empty() {
            return Err(PortError::SchemaIncompatible(format!(
                "{subject} contextual relations are unavailable; re-ingest required"
            )));
        }
        for source_path in sources {
            let complete: bool = conn
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM source_relation_scans WHERE source_path = ?1
                     )",
                    [source_path],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if !complete {
                return Err(PortError::SchemaIncompatible(format!(
                    "{subject} contextual relations are incomplete; re-ingest required"
                )));
            }
        }
        Ok(())
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
        batch_manifest(entries, &[], &RelationManifests::default())?;
        self.ensure_stored_identity_metadata_matches(entries)?;
        if self.batch_is_current(entries)? {
            return Ok(false);
        }
        let pending = self.begin_index_batch(entries, &[])?;
        self.commit_index_batch(&pending, entries, &[])?;
        Ok(true)
    }

    fn batch_is_current(&self, entries: &[(StableId, Vec<u8>, String)]) -> PortResult<bool> {
        self.batch_is_current_with_derived_context(entries, false)
    }

    fn batch_is_current_with_derived_context(
        &self,
        entries: &[(StableId, Vec<u8>, String)],
        contextual_payloads_are_derived: bool,
    ) -> PortResult<bool> {
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
            let Some(catalog_payload) = catalog_payload else {
                return Ok(false);
            };
            let payload_is_current = if contextual_payloads_are_derived
                && matches!(id.kind(), IdKind::Message | IdKind::Session)
            {
                if catalog_payload.as_slice() == payload.as_slice() {
                    true
                } else {
                    // 存储 payload 可能携带从 v7 关系再生的上下文别名（parent/
                    // is_sidechain/session/spans 等），与传入的合并结果未必逐字节
                    // 一致；因此用与提交路径相同的合并操作判定：把传入 payload 并入
                    // 存储值，若能还原出存储字节才是内容级 no-op。只查存在性会把
                    // merge 实际会应用的 payload-only 变更静默丢弃（如消息新增一个
                    // session 引用而 text 未变）。字节相同则直接短路——与提交路径
                    // “stored == payload 时不做 merge”的行为一致（非 JSON 的旧式
                    // 裸文本行在此保持 no-op 而非误报）。
                    let merged = match id.kind() {
                        IdKind::Message => {
                            merge_message_payloads(id.as_str(), &catalog_payload, payload)?
                        }
                        IdKind::Session => {
                            merge_session_payloads(id.as_str(), &catalog_payload, payload)?
                        }
                        _ => unreachable!(),
                    };
                    merged == catalog_payload
                }
            } else {
                catalog_payload.as_slice() == payload.as_slice()
            };
            if !payload_is_current {
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
            let stored_id: StableId = serde_json::from_str(&id_json)
                .map_err(|_| PortError::Backend("stored identity sidecar is not valid".into()))?;
            if &stored_id != id {
                return Ok(false);
            }
            // 非 Message 实体不进 fts 全文表（见 commit_index_batch_with_relations），
            // 其"内容一致"只看 catalog payload 与 fts_ids 身份边车。
            if id.kind() != IdKind::Message {
                continue;
            }
            // fts 的 `id` 是内容列（UNINDEXED），按它比较会让每条消息的 current
            // 判定整表扫描（B1 路径 O(N²)）；改经 fts_ids 边车的 fts_rowid 按
            // rowid O(1) 定位。边车缺行或 fts_rowid 为 NULL 时按“无 fts 行”处理
            // （rowid = NULL 匹配不到行）→ 不 current，提交路径会重建该行。
            let indexed_text: Option<String> = conn
                .query_row(
                    "SELECT text FROM fts
                     WHERE rowid = (SELECT fts_rowid FROM fts_ids WHERE wire_id = ?1)",
                    [id.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if indexed_text.as_deref() != Some(text.as_str()) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Commit source-owned entity and relation facts, reporting generation change.
    ///
    /// Complete relation scans replace both entity and placement claims and may
    /// derive tombstones. Incomplete scans union observed claims, derive no
    /// tombstones, and clear the source relation-completeness marker.
    pub fn commit_source_batches_if_changed(&self, sources: &[SourceBatch]) -> PortResult<bool> {
        let mut ordered_sources: Vec<&SourceBatch> = sources.iter().collect();
        ordered_sources.sort_by(|left, right| left.source_path.cmp(&right.source_path));
        let paths: Vec<&str> = ordered_sources
            .iter()
            .map(|source| source.source_path.as_str())
            .collect();
        if paths.windows(2).any(|window| window[0] == window[1]) {
            return Err(PortError::Backend(
                "source batch contains duplicate source paths".into(),
            ));
        }

        // Cheap no-op check FIRST: building the merged view, claimer graph,
        // and manifest below costs O(whole catalog). When every source in
        // this batch is already current (entries, relations, membership,
        // claims, scans), skip all of it and report no generation change.
        // The per-batch cost is then proportional to the batch, not the
        // catalog — this is what makes an unchanged re-sync fast.
        if self.sources_are_current(&ordered_sources)? {
            return Ok(false);
        }

        let scanned_paths: BTreeSet<String> = paths.into_iter().map(str::to_string).collect();
        let current_entities_by_source = self.source_entity_membership_state()?;
        let current_placements_by_source = self.source_placement_membership_state()?;
        let stored_placements = self.stored_placements()?;
        let stored_edges = self.stored_edges()?;

        let mut merged = BTreeMap::<String, (StableId, Vec<u8>, String)>::new();
        let mut observed_placements = BTreeMap::<String, MessagePlacement>::new();
        let mut observed_edges = BTreeMap::<String, MessageEdge>::new();
        let mut prepared_sources = BTreeMap::<String, PreparedSource>::new();

        for source in ordered_sources {
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

            let document_id = source
                .entries
                .iter()
                .find(|(id, _, _)| id.kind() == IdKind::Document)
                .map(|(id, _, _)| id.as_str().to_string());
            let incoming_entities: BTreeMap<String, Option<String>> = source
                .entries
                .iter()
                .map(|(id, _, _)| (id.as_str().to_string(), document_id.clone()))
                .collect();
            let prior_entity_memberships = current_entities_by_source
                .get(&source.source_path)
                .cloned()
                .unwrap_or_default();
            let mut final_entities = if source.relation_complete {
                BTreeMap::new()
            } else {
                prior_entity_memberships.clone()
            };
            final_entities.extend(incoming_entities);

            let mut source_placements = BTreeMap::new();
            let mut placement_slots = BTreeSet::new();
            for placement in &source.placements {
                validate_placement(placement)?;
                let placement_id = placement.id.as_str().to_string();
                if source_placements
                    .insert(placement_id.clone(), placement.clone())
                    .is_some()
                {
                    return Err(PortError::Backend(
                        "source batch contains duplicate placement ids".into(),
                    ));
                }
                let slot = (
                    placement.session_id.as_str().to_string(),
                    placement.source_document_id.as_str().to_string(),
                    placement.source_ordinal,
                );
                if !placement_slots.insert(slot) {
                    return Err(PortError::Backend(
                        "source batch contains duplicate placement ordinals".into(),
                    ));
                }
                if let Some(existing) = observed_placements.get(&placement_id) {
                    if existing != placement {
                        return Err(PortError::Backend(format!(
                            "placement {placement_id} has conflicting projections across sources"
                        )));
                    }
                } else {
                    observed_placements.insert(placement_id, placement.clone());
                }
            }

            let mut source_edges = BTreeMap::new();
            for edge in &source.edges {
                validate_edge(edge)?;
                let placement_id = edge.child_placement_id.as_str().to_string();
                if !source_placements.contains_key(&placement_id) {
                    return Err(PortError::Backend(
                        "source batch edge does not belong to an observed placement".into(),
                    ));
                }
                if source_edges
                    .insert(placement_id.clone(), edge.clone())
                    .is_some()
                {
                    return Err(PortError::Backend(
                        "source batch contains duplicate edge children".into(),
                    ));
                }
                if let Some(existing) = observed_edges.get(&placement_id) {
                    if existing != edge {
                        return Err(PortError::Backend(format!(
                            "edge {placement_id} has conflicting projections across sources"
                        )));
                    }
                } else {
                    observed_edges.insert(placement_id, edge.clone());
                }
            }

            let prior_placement_ids = current_placements_by_source
                .get(&source.source_path)
                .cloned()
                .unwrap_or_default();
            let mut final_placement_ids = if source.relation_complete {
                BTreeSet::new()
            } else {
                prior_placement_ids.clone()
            };
            final_placement_ids.extend(source_placements.keys().cloned());

            let replacement = SourceReplacementManifest {
                source_path: source.source_path.clone(),
                entity_memberships: final_entities
                    .into_iter()
                    .map(|(entity_id, document_id)| SourceEntityMembershipManifest {
                        entity_id,
                        document_id,
                    })
                    .collect(),
                placement_ids: final_placement_ids
                    .iter()
                    .map(|wire| {
                        PlacementId::from_wire(wire).ok_or_else(|| {
                            PortError::Backend(format!("invalid placement claim id: {wire}"))
                        })
                    })
                    .collect::<PortResult<Vec<_>>>()?,
                relation_complete: source.relation_complete,
                len_bytes: source.len_bytes,
                fingerprint: source.fingerprint.clone(),
            };
            prepared_sources.insert(
                source.source_path.clone(),
                PreparedSource {
                    relation_complete: source.relation_complete,
                    prior_entity_memberships,
                    prior_placement_ids,
                    observed_placements: source_placements,
                    observed_edges: source_edges,
                    replacement,
                },
            );

            for (id, payload, text) in &source.entries {
                if let Some((old_id, old_payload, old_text)) = merged.get(id.as_str()) {
                    if old_id != id {
                        return Err(PortError::Backend(
                            "entity has conflicting identity metadata across sources".into(),
                        ));
                    }
                    if old_payload != payload || old_text != text {
                        let union = match id.kind() {
                            IdKind::Session => {
                                merge_session_payloads(id.as_str(), old_payload, payload)?
                            }
                            IdKind::Message => {
                                merge_message_payloads(id.as_str(), old_payload, payload)?
                            }
                            _ => {
                                return Err(PortError::Backend(
                                    "entity has conflicting projections across sources".into(),
                                ));
                            }
                        };
                        // 合并后的 payload 是内容的权威投影：FTS 正文必须从它重投影
                        // （与 rebuild_index 同一投影函数），而不是取“排序最后处理的
                        // 源的原始 text”——后者在 text 较短源排最后时会与 payload 分叉
                        // （长文本在 payload 但搜不到，需 rebuild 才恢复），也会让按源
                        // 分批 sync 时 generation 反复推进、内容级 no-op 失效。
                        let merged_text = if id.kind() == IdKind::Message {
                            searchable_text(&union)
                        } else {
                            text.clone()
                        };
                        merged.insert(id.as_str().to_string(), (id.clone(), union, merged_text));
                        continue;
                    }
                } else {
                    merged.insert(
                        id.as_str().to_string(),
                        (id.clone(), payload.clone(), text.clone()),
                    );
                }
            }
        }

        for (id, payload, text) in merged.values_mut() {
            let stored = match self.get(id)? {
                Some(stored) if stored != *payload => stored,
                _ => continue,
            };
            match id.kind() {
                IdKind::Session => {
                    *payload = merge_session_payloads(id.as_str(), &stored, payload)?;
                }
                IdKind::Message => {
                    let union = merge_message_payloads(id.as_str(), &stored, payload)?;
                    // 与 stored 合并后再次重投影正文：合并结果可能以 stored 中更长的
                    // text 为准，FTS 必须索引 searchable_text(合并后 payload) 而非
                    // 来源侧原始 text，否则 payload 与搜索索引再次分叉。
                    *text = searchable_text(&union);
                    *payload = union;
                }
                _ => continue,
            };
        }

        let mut final_entity_claimers = BTreeMap::<String, BTreeSet<String>>::new();
        for (source_path, memberships) in &current_entities_by_source {
            if scanned_paths.contains(source_path) {
                continue;
            }
            for entity_id in memberships.keys() {
                final_entity_claimers
                    .entry(entity_id.clone())
                    .or_default()
                    .insert(source_path.clone());
            }
        }
        let mut final_placement_claimers = BTreeMap::<String, BTreeSet<String>>::new();
        for (source_path, placement_ids) in &current_placements_by_source {
            if scanned_paths.contains(source_path) {
                continue;
            }
            for placement_id in placement_ids {
                final_placement_claimers
                    .entry(placement_id.clone())
                    .or_default()
                    .insert(source_path.clone());
            }
        }
        for (source_path, prepared) in &prepared_sources {
            for membership in &prepared.replacement.entity_memberships {
                final_entity_claimers
                    .entry(membership.entity_id.clone())
                    .or_default()
                    .insert(source_path.clone());
            }
            for placement_id in &prepared.replacement.placement_ids {
                final_placement_claimers
                    .entry(placement_id.as_str().to_string())
                    .or_default()
                    .insert(source_path.clone());
            }
        }

        let mut deletes = BTreeMap::new();
        let mut placement_delete_ids = BTreeSet::new();
        for prepared in prepared_sources.values() {
            if !prepared.relation_complete {
                continue;
            }
            let final_entity_ids: BTreeSet<&str> = prepared
                .replacement
                .entity_memberships
                .iter()
                .map(|membership| membership.entity_id.as_str())
                .collect();
            for prior in prepared.prior_entity_memberships.keys() {
                if !final_entity_ids.contains(prior.as_str())
                    && !final_entity_claimers.contains_key(prior)
                {
                    let id = StableId::from_wire(prior).ok_or_else(|| {
                        PortError::Backend(format!("invalid membership id: {prior}"))
                    })?;
                    deletes.insert(prior.clone(), id);
                }
            }

            let final_placement_ids: BTreeSet<&str> = prepared
                .replacement
                .placement_ids
                .iter()
                .map(PlacementId::as_str)
                .collect();
            for prior in &prepared.prior_placement_ids {
                if !final_placement_ids.contains(prior.as_str())
                    && !final_placement_claimers.contains_key(prior)
                    && stored_placements.contains_key(prior)
                {
                    placement_delete_ids.insert(prior.clone());
                }
            }
        }

        for (placement_id, placement) in &observed_placements {
            let Some(stored) = stored_placements.get(placement_id) else {
                continue;
            };
            if stored.matches(placement) {
                continue;
            }
            let claimers = final_placement_claimers
                .get(placement_id)
                .cloned()
                .unwrap_or_default();
            let all_claimers_observed_same_placement = !claimers.is_empty()
                && claimers.iter().all(|source_path| {
                    prepared_sources.get(source_path).is_some_and(|prepared| {
                        prepared.observed_placements.get(placement_id) == Some(placement)
                    })
                });
            if !all_claimers_observed_same_placement {
                return Err(PortError::Backend(format!(
                    "placement {placement_id} conflicts with a source that did not observe the same placement"
                )));
            }
        }

        for (placement_id, edge) in &observed_edges {
            let current_matches = stored_edges
                .get(placement_id)
                .is_some_and(|stored| stored.matches(edge));
            let claimers = final_placement_claimers
                .get(placement_id)
                .cloned()
                .unwrap_or_default();
            if !current_matches {
                let all_claimers_observed_same_edge = !claimers.is_empty()
                    && claimers.iter().all(|source_path| {
                        prepared_sources.get(source_path).is_some_and(|prepared| {
                            prepared.observed_edges.get(placement_id) == Some(edge)
                        })
                    });
                if !all_claimers_observed_same_edge {
                    return Err(PortError::Backend(format!(
                        "edge {placement_id} conflicts with a source that did not observe the same edge"
                    )));
                }
            }
        }

        let mut edge_delete_ids: BTreeSet<String> = placement_delete_ids
            .iter()
            .filter(|placement_id| stored_edges.contains_key(*placement_id))
            .cloned()
            .collect();
        for prepared in prepared_sources.values() {
            for placement_id in prepared.observed_placements.keys() {
                if prepared.observed_edges.contains_key(placement_id)
                    || !stored_edges.contains_key(placement_id)
                {
                    continue;
                }
                if observed_edges.contains_key(placement_id) {
                    return Err(PortError::Backend(format!(
                        "edge {placement_id} has inconsistent complete-source claims"
                    )));
                }
                let claimers = final_placement_claimers
                    .get(placement_id)
                    .cloned()
                    .unwrap_or_default();
                let all_claimers_observed_root = !claimers.is_empty()
                    && claimers.iter().all(|source_path| {
                        prepared_sources.get(source_path).is_some_and(|claimer| {
                            claimer.observed_placements.contains_key(placement_id)
                                && !claimer.observed_edges.contains_key(placement_id)
                        })
                    });
                if !all_claimers_observed_root {
                    return Err(PortError::Backend(format!(
                        "edge {placement_id} conflicts with a source that did not observe the same root"
                    )));
                }
                edge_delete_ids.insert(placement_id.clone());
            }
        }

        let upserts: Vec<(StableId, Vec<u8>, String)> = merged.into_values().collect();
        let deletes: Vec<StableId> = deletes.into_values().collect();
        let relations = RelationManifests {
            relation_upserts: observed_placements
                .into_values()
                .map(RelationUpsertManifest::Placement)
                .chain(
                    observed_edges
                        .into_values()
                        .map(RelationUpsertManifest::Edge),
                )
                .collect(),
            relation_deletes: edge_delete_ids
                .into_iter()
                .map(|wire| {
                    PlacementId::from_wire(&wire)
                        .map(RelationDeleteManifest::Edge)
                        .ok_or_else(|| {
                            PortError::Backend(format!("invalid edge tombstone id: {wire}"))
                        })
                })
                .chain(placement_delete_ids.into_iter().map(|wire| {
                    PlacementId::from_wire(&wire)
                        .map(RelationDeleteManifest::Placement)
                        .ok_or_else(|| {
                            PortError::Backend(format!("invalid placement tombstone id: {wire}"))
                        })
                }))
                .collect::<PortResult<Vec<_>>>()?,
            source_replacements: prepared_sources
                .into_values()
                .map(|prepared| prepared.replacement)
                .collect(),
        };

        batch_manifest(&upserts, &deletes, &relations)?;
        self.ensure_stored_identity_metadata_matches(&upserts)?;
        if self.source_batches_are_current(&upserts, &relations)? {
            return Ok(false);
        }

        let pending = self.begin_index_batch_with_relations(&upserts, &deletes, &relations)?;
        self.commit_index_batch_with_relations(&pending, &upserts, &deletes, &relations)?;
        Ok(true)
    }

    /// True when every source in the batch is already fully current: catalog
    /// entries (payload + fts text), placements, edges, entity membership,
    /// placement claims, scan record, and relation-completeness marker all
    /// match the stored state. Called before any heavy merge/manifest work
    /// so an unchanged re-sync costs O(batch), not O(whole catalog). Every
    /// query is scoped to this batch's sources/ids — no full-table loads.
    fn sources_are_current(&self, ordered_sources: &[&SourceBatch]) -> PortResult<bool> {
        let conn = self.conn.borrow();
        for source in ordered_sources {
            // A source that has never been scanned cannot be current; skip the
            // per-entity queries (which dominate on first ingest of an
            // empty catalog) and go straight to the heavy path.
            let stored_scan: Option<(i64, Option<i64>, Option<String>)> = conn
                .query_row(
                    "SELECT 1, len_bytes, fingerprint
                     FROM source_scans WHERE source_path = ?1",
                    [&source.source_path],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(backend)?;
            let Some((_, stored_len, stored_fingerprint)) = stored_scan else {
                return Ok(false);
            };
            // 指纹缓存参与 current 判定：len/fingerprint 任一变说明源字节已变而
            // 缓存未更新，必须重解析并重写缓存。只查扫描行存在会让缓存永不收敛，
            // CLI 每次运行都重解析全部源。
            if source.len_bytes != stored_len
                || source.fingerprint.as_deref() != stored_fingerprint.as_deref()
            {
                return Ok(false);
            }

            // Catalog entries: batched payload reads, chunked under the
            // SQLite variable limit.
            let ids: Vec<&str> = source
                .entries
                .iter()
                .map(|(id, _, _)| id.as_str())
                .collect();
            let mut payloads: BTreeMap<String, Vec<u8>> = BTreeMap::new();
            for chunk in chunk_ids(&ids) {
                let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let mut stmt = conn
                    .prepare(&format!(
                        "SELECT id, payload FROM catalog WHERE id IN ({placeholders})"
                    ))
                    .map_err(backend)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(chunk.iter().copied()), |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                    })
                    .map_err(backend)?;
                for row in rows {
                    let (id, payload) = row.map_err(backend)?;
                    payloads.insert(id, payload);
                }
            }
            for (id, payload, _text) in &source.entries {
                if payloads.get(id.as_str()).map(Vec::as_slice) != Some(payload.as_slice()) {
                    return Ok(false);
                }
            }
            // Indexed text: batched reads mapping wire_id -> fts text.
            let mut fts_text: BTreeMap<String, String> = BTreeMap::new();
            for chunk in chunk_ids(&ids) {
                let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let mut stmt = conn
                    .prepare(&format!(
                        "SELECT fi.wire_id, f.text FROM fts f
                         JOIN fts_ids fi ON fi.id_json = f.id
                         WHERE fi.wire_id IN ({placeholders})"
                    ))
                    .map_err(backend)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(chunk.iter().copied()), |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(backend)?;
                for row in rows {
                    let (wire_id, text) = row.map_err(backend)?;
                    fts_text.insert(wire_id, text);
                }
            }
            for (id, _payload, text) in &source.entries {
                if fts_text.get(id.as_str()).map(String::as_str) != Some(text.as_str()) {
                    return Ok(false);
                }
            }
            // Entity membership for this source only.
            let mut stmt = conn
                .prepare(
                    "SELECT message_id, document_id FROM source_membership
                     WHERE source_path = ?1 ORDER BY message_id",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([&source.source_path], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .map_err(backend)?;
            let mut stored_membership = BTreeMap::new();
            for row in rows {
                let (entity_id, document_id) = row.map_err(backend)?;
                stored_membership.insert(entity_id, document_id);
            }
            let document_id = source
                .entries
                .iter()
                .find(|(id, _, _)| id.kind() == IdKind::Document)
                .map(|(id, _, _)| id.as_str().to_string());
            let incoming_entities: BTreeMap<String, Option<String>> = source
                .entries
                .iter()
                .map(|(id, _, _)| (id.as_str().to_string(), document_id.clone()))
                .collect();
            if stored_membership != incoming_entities {
                return Ok(false);
            }
            // Placement claims for this source only.
            let mut stmt = conn
                .prepare(
                    "SELECT placement_id FROM source_placement_membership
                     WHERE source_path = ?1 ORDER BY placement_id",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([&source.source_path], |row| row.get::<_, String>(0))
                .map_err(backend)?;
            let stored_placements: BTreeSet<String> =
                rows.collect::<Result<_, _>>().map_err(backend)?;
            let expected_placements: BTreeSet<String> = source
                .placements
                .iter()
                .map(|p| p.id.as_str().to_string())
                .collect();
            if stored_placements != expected_placements {
                return Ok(false);
            }
            // Stored placements for this source's ids (batched, chunked).
            let mut stored_placements: BTreeMap<String, StoredPlacement> = BTreeMap::new();
            let pids: Vec<&str> = source.placements.iter().map(|p| p.id.as_str()).collect();
            for chunk in chunk_ids(&pids) {
                let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let mut stmt = conn
                    .prepare(&format!(
                        "SELECT placement_id, session_id, document_id, message_id,
                                source_ordinal, is_sidechain, byte_start, byte_end
                         FROM message_placements WHERE placement_id IN ({placeholders})"
                    ))
                    .map_err(backend)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(chunk.iter().copied()), |row| {
                        let start: Option<i64> = row.get(6)?;
                        let end: Option<i64> = row.get(7)?;
                        Ok((
                            row.get::<_, String>(0)?,
                            StoredPlacement {
                                session_id: row.get(1)?,
                                document_id: row.get(2)?,
                                message_id: row.get(3)?,
                                source_ordinal: row.get(4)?,
                                is_sidechain: row.get(5)?,
                                span: match (start, end) {
                                    (Some(start), Some(end)) => Some((start as u64, end as u64)),
                                    _ => None,
                                },
                            },
                        ))
                    })
                    .map_err(backend)?;
                for row in rows {
                    let (pid, stored) = row.map_err(backend)?;
                    stored_placements.insert(pid, stored);
                }
            }
            for placement in &source.placements {
                if !stored_placements
                    .get(placement.id.as_str())
                    .is_some_and(|stored| stored.matches(placement))
                {
                    return Ok(false);
                }
            }
            // Stored edges for this source's ids (batched, chunked).
            let mut stored_edges: BTreeMap<String, (String, Option<String>, String)> =
                BTreeMap::new();
            let cids: Vec<&str> = source
                .edges
                .iter()
                .map(|e| e.child_placement_id.as_str())
                .collect();
            for chunk in chunk_ids(&cids) {
                let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let mut stmt = conn
                    .prepare(&format!(
                        "SELECT child_placement_id, parent_message_id, parent_native_id, relation
                         FROM message_edges WHERE child_placement_id IN ({placeholders})"
                    ))
                    .map_err(backend)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(chunk.iter().copied()), |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            (
                                row.get::<_, String>(1)?,
                                row.get::<_, Option<String>>(2)?,
                                row.get::<_, String>(3)?,
                            ),
                        ))
                    })
                    .map_err(backend)?;
                for row in rows {
                    let (cid, edge) = row.map_err(backend)?;
                    stored_edges.insert(cid, edge);
                }
            }
            for edge in &source.edges {
                let matches = stored_edges
                    .get(edge.child_placement_id.as_str())
                    .is_some_and(|(parent, native, relation)| {
                        parent == edge.parent_message_id.as_str()
                            && native.as_deref() == edge.parent_native_id.as_deref()
                            && relation == edge.relation.as_str()
                    });
                if !matches {
                    return Ok(false);
                }
            }
            // Completeness marker (scan record already checked at loop head).
            let complete: bool = conn
                .query_row(
                    "SELECT 1 FROM source_relation_scans WHERE source_path = ?1",
                    [&source.source_path],
                    |_| Ok(()),
                )
                .optional()
                .map_err(backend)?
                .is_some();
            if complete != source.relation_complete {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn source_entity_membership_state(
        &self,
    ) -> PortResult<BTreeMap<String, BTreeMap<String, Option<String>>>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(
                "SELECT source_path, message_id, document_id
                 FROM source_membership ORDER BY source_path, message_id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(backend)?;
        let mut state = BTreeMap::<String, BTreeMap<String, Option<String>>>::new();
        for row in rows {
            let (source_path, entity_id, document_id) = row.map_err(backend)?;
            state
                .entry(source_path)
                .or_default()
                .insert(entity_id, document_id);
        }
        Ok(state)
    }

    #[cfg(test)]
    fn source_message_ids(&self, source_path: &str) -> PortResult<Vec<String>> {
        Ok(self
            .source_entity_membership_state()?
            .remove(source_path)
            .unwrap_or_default()
            .into_keys()
            .collect())
    }

    fn source_placement_membership_state(&self) -> PortResult<BTreeMap<String, BTreeSet<String>>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(
                "SELECT source_path, placement_id
                 FROM source_placement_membership ORDER BY source_path, placement_id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(backend)?;
        let mut state = BTreeMap::<String, BTreeSet<String>>::new();
        for row in rows {
            let (source_path, placement_id) = row.map_err(backend)?;
            state.entry(source_path).or_default().insert(placement_id);
        }
        Ok(state)
    }

    fn stored_placements(&self) -> PortResult<BTreeMap<String, StoredPlacement>> {
        let conn = self.conn.borrow();
        Self::stored_placements_from(&conn)
    }

    fn stored_placements_from(conn: &Connection) -> PortResult<BTreeMap<String, StoredPlacement>> {
        let mut stmt = conn
            .prepare(
                "SELECT placement_id, session_id, document_id, message_id,
                        source_ordinal, is_sidechain, byte_start, byte_end
                 FROM message_placements ORDER BY placement_id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            })
            .map_err(backend)?;
        let mut placements = BTreeMap::new();
        for row in rows {
            let (placement_id, session_id, document_id, message_id, ordinal, sidechain, start, end) =
                row.map_err(backend)?;
            let span = match (start, end) {
                (None, None) => None,
                (Some(start), Some(end)) => Some((
                    u64::try_from(start).map_err(backend)?,
                    u64::try_from(end).map_err(backend)?,
                )),
                _ => {
                    return Err(PortError::Backend(
                        "stored placement has a partial span".into(),
                    ));
                }
            };
            placements.insert(
                placement_id,
                StoredPlacement {
                    session_id,
                    document_id,
                    message_id,
                    source_ordinal: u32::try_from(ordinal).map_err(backend)?,
                    is_sidechain: sidechain != 0,
                    span,
                },
            );
        }
        Ok(placements)
    }

    fn stored_edges(&self) -> PortResult<BTreeMap<String, StoredEdge>> {
        let conn = self.conn.borrow();
        Self::stored_edges_from(&conn)
    }

    fn stored_edges_from(conn: &Connection) -> PortResult<BTreeMap<String, StoredEdge>> {
        let mut stmt = conn
            .prepare(
                "SELECT child_placement_id, parent_message_id, parent_native_id, relation
                 FROM message_edges ORDER BY child_placement_id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    StoredEdge {
                        parent_message_id: row.get(1)?,
                        parent_native_id: row.get(2)?,
                        relation: row.get(3)?,
                    },
                ))
            })
            .map_err(backend)?;
        let mut edges = BTreeMap::new();
        for row in rows {
            let (placement_id, edge) = row.map_err(backend)?;
            edges.insert(placement_id, edge);
        }
        Ok(edges)
    }

    fn regenerate_compatibility_aliases_in_tx(
        tx: &rusqlite::Transaction<'_>,
        batch_sources: &[String],
    ) -> PortResult<()> {
        let complete_sources = {
            let mut stmt = tx
                .prepare("SELECT source_path FROM source_relation_scans")
                .map_err(backend)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(backend)?;
            let mut sources = BTreeSet::new();
            for row in rows {
                sources.insert(row.map_err(backend)?);
            }
            sources
        };

        let mut claimers_by_entity = BTreeMap::<String, BTreeSet<String>>::new();
        let mut session_document_claims = BTreeMap::<String, BTreeSet<String>>::new();
        {
            let mut stmt = tx
                .prepare(
                    "SELECT source_path, message_id, document_id
                     FROM source_membership",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(backend)?;
            for row in rows {
                let (source_path, entity_id, document_id) = row.map_err(backend)?;
                claimers_by_entity
                    .entry(entity_id.clone())
                    .or_default()
                    .insert(source_path);
                if entity_id.starts_with(IdKind::Session.prefix())
                    && let Some(document_id) = document_id
                {
                    session_document_claims
                        .entry(entity_id)
                        .or_default()
                        .insert(document_id);
                }
            }
        }
        {
            let mut stmt = tx
                .prepare(
                    "SELECT claims.source_path, placements.session_id,
                            placements.document_id, placements.message_id
                     FROM source_placement_membership AS claims
                     JOIN message_placements AS placements
                       ON placements.placement_id = claims.placement_id",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(backend)?;
            for row in rows {
                let (source_path, session_id, document_id, message_id) = row.map_err(backend)?;
                for entity_id in [session_id, document_id, message_id] {
                    claimers_by_entity
                        .entry(entity_id)
                        .or_default()
                        .insert(source_path.clone());
                }
            }
        }

        let fully_complete_entities: BTreeSet<String> = claimers_by_entity
            .into_iter()
            .filter_map(|(entity_id, claimers)| {
                // Only entities whose claimers intersect this batch's sources
                // can have their aliases changed by this commit; regenerating
                // the whole catalog per batch is what made first ingest
                // O(n²). Fully-complete still requires every claimer scanned.
                let touched_by_batch = claimers
                    .iter()
                    .any(|source_path| batch_sources.iter().any(|batch| batch == source_path));
                (!claimers.is_empty()
                    && touched_by_batch
                    && claimers
                        .iter()
                        .all(|source_path| complete_sources.contains(source_path)))
                .then_some(entity_id)
            })
            .collect();
        if fully_complete_entities.is_empty() {
            return Ok(());
        }

        let placements = Self::stored_placements_from(tx)?;
        let edges = Self::stored_edges_from(tx)?;
        let mut placements_by_message = BTreeMap::<String, Vec<(String, StoredPlacement)>>::new();
        let mut placements_by_session = BTreeMap::<String, Vec<(String, StoredPlacement)>>::new();
        for (placement_id, placement) in placements {
            placements_by_message
                .entry(placement.message_id.clone())
                .or_default()
                .push((placement_id.clone(), placement.clone()));
            placements_by_session
                .entry(placement.session_id.clone())
                .or_default()
                .push((placement_id, placement));
        }
        for placements in placements_by_message
            .values_mut()
            .chain(placements_by_session.values_mut())
        {
            placements.sort_by(|left, right| {
                (
                    left.1.document_id.as_str(),
                    left.1.source_ordinal,
                    left.0.as_str(),
                )
                    .cmp(&(
                        right.1.document_id.as_str(),
                        right.1.source_ordinal,
                        right.0.as_str(),
                    ))
            });
        }

        for entity_id in fully_complete_entities {
            let Some(id) = StableId::from_wire(&entity_id) else {
                return Err(PortError::Backend(
                    "source membership contains an invalid entity id".into(),
                ));
            };
            if !matches!(id.kind(), IdKind::Message | IdKind::Session) {
                continue;
            }
            let payload: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT payload FROM catalog WHERE id = ?1",
                    [&entity_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            let Some(payload) = payload else {
                continue;
            };
            let mut map = match serde_json::from_slice::<serde_json::Value>(&payload) {
                Ok(serde_json::Value::Object(map)) => map,
                _ => continue,
            };

            match id.kind() {
                IdKind::Message => {
                    let placements = placements_by_message
                        .get(&entity_id)
                        .cloned()
                        .unwrap_or_default();
                    if placements.is_empty() {
                        continue;
                    }
                    let sessions: Vec<String> = placements
                        .iter()
                        .map(|(_, placement)| placement.session_id.clone())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    map.insert(
                        "session".into(),
                        sessions
                            .first()
                            .cloned()
                            .map_or(serde_json::Value::Null, serde_json::Value::String),
                    );
                    map.insert("sessions".into(), serde_json::json!(sessions));

                    let spans: Vec<serde_json::Value> = placements
                        .iter()
                        .filter_map(|(placement_id, placement)| {
                            placement.span.map(|(start, end)| {
                                serde_json::json!({
                                    "placement_id": placement_id,
                                    "document": placement.document_id,
                                    "start": start,
                                    "end": end,
                                })
                            })
                        })
                        .collect();
                    map.insert(
                        "span".into(),
                        spans.first().map_or(serde_json::Value::Null, |span| {
                            serde_json::json!({
                                "start": span.get("start").cloned().unwrap_or(serde_json::Value::Null),
                                "end": span.get("end").cloned().unwrap_or(serde_json::Value::Null),
                            })
                        }),
                    );
                    map.insert("spans".into(), serde_json::json!(spans));

                    let parent_facts: Vec<Option<String>> = placements
                        .iter()
                        .map(|(placement_id, _)| {
                            edges
                                .get(placement_id)
                                .map(|edge| edge.parent_message_id.clone())
                        })
                        .collect();
                    let parent = match parent_facts.first() {
                        Some(first) if parent_facts.iter().all(|fact| fact == first) => {
                            first.clone()
                        }
                        _ => None,
                    };
                    map.insert(
                        "parent".into(),
                        parent.map_or(serde_json::Value::Null, serde_json::Value::String),
                    );

                    let parent_native_facts: Vec<Option<String>> = placements
                        .iter()
                        .map(|(placement_id, _)| {
                            edges
                                .get(placement_id)
                                .and_then(|edge| edge.parent_native_id.clone())
                        })
                        .collect();
                    let parent_native_id = match parent_native_facts.first() {
                        Some(first) if parent_native_facts.iter().all(|fact| fact == first) => {
                            first.clone()
                        }
                        _ => None,
                    };
                    map.insert(
                        "parent_native_id".into(),
                        parent_native_id.map_or(serde_json::Value::Null, serde_json::Value::String),
                    );

                    let is_sidechain = match placements.first() {
                        Some((_, first))
                            if placements.iter().all(|(_, placement)| {
                                placement.is_sidechain == first.is_sidechain
                            }) =>
                        {
                            serde_json::Value::Bool(first.is_sidechain)
                        }
                        _ => serde_json::Value::Null,
                    };
                    map.insert("is_sidechain".into(), is_sidechain);
                }
                IdKind::Session => {
                    let placements = placements_by_session
                        .get(&entity_id)
                        .cloned()
                        .unwrap_or_default();
                    if placements.is_empty()
                        && map
                            .get("messages")
                            .and_then(serde_json::Value::as_array)
                            .is_some_and(|messages| !messages.is_empty())
                    {
                        continue;
                    }
                    let mut seen_messages = BTreeSet::new();
                    let mut messages = Vec::new();
                    let mut documents = session_document_claims
                        .remove(&entity_id)
                        .unwrap_or_default();
                    for (_, placement) in placements {
                        documents.insert(placement.document_id);
                        if seen_messages.insert(placement.message_id.clone()) {
                            messages.push(placement.message_id);
                        }
                    }
                    let documents: Vec<String> = documents.into_iter().collect();
                    map.insert(
                        "document".into(),
                        documents
                            .first()
                            .cloned()
                            .map_or(serde_json::Value::Null, serde_json::Value::String),
                    );
                    map.insert("documents".into(), serde_json::json!(documents));
                    map.insert("messages".into(), serde_json::json!(messages));
                }
                _ => unreachable!(),
            }

            let payload = serde_json::to_vec(&serde_json::Value::Object(map)).map_err(backend)?;
            // Skip the write when the rebuilt aliases equal the stored bytes:
            // regeneration must not rewrite the catalog (and inflate the WAL)
            // on every commit once aliases are stable.
            let stored: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT payload FROM catalog WHERE id = ?1",
                    [&entity_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if stored.as_deref() != Some(payload.as_slice()) {
                tx.execute(
                    "UPDATE catalog SET payload = ?2 WHERE id = ?1",
                    rusqlite::params![entity_id, payload],
                )
                .map_err(backend)?;
            }
        }
        Ok(())
    }

    fn source_batches_are_current(
        &self,
        upserts: &[(StableId, Vec<u8>, String)],
        relations: &RelationManifests,
    ) -> PortResult<bool> {
        if !self.batch_is_current_with_derived_context(upserts, true)? {
            return Ok(false);
        }
        let stored_placements = self.stored_placements()?;
        let stored_edges = self.stored_edges()?;
        for upsert in &relations.relation_upserts {
            let current = match upsert {
                RelationUpsertManifest::Placement(placement) => stored_placements
                    .get(placement.id.as_str())
                    .is_some_and(|stored| stored.matches(placement)),
                RelationUpsertManifest::Edge(edge) => stored_edges
                    .get(edge.child_placement_id.as_str())
                    .is_some_and(|stored| stored.matches(edge)),
            };
            if !current {
                return Ok(false);
            }
        }
        for delete in &relations.relation_deletes {
            let exists = match delete {
                RelationDeleteManifest::Placement(id) => {
                    stored_placements.contains_key(id.as_str())
                }
                RelationDeleteManifest::Edge(id) => stored_edges.contains_key(id.as_str()),
            };
            if exists {
                return Ok(false);
            }
        }

        let entity_state = self.source_entity_membership_state()?;
        let placement_state = self.source_placement_membership_state()?;
        let conn = self.conn.borrow();
        for replacement in &relations.source_replacements {
            let expected_entities: BTreeMap<String, Option<String>> = replacement
                .entity_memberships
                .iter()
                .map(|membership| (membership.entity_id.clone(), membership.document_id.clone()))
                .collect();
            if entity_state
                .get(&replacement.source_path)
                .cloned()
                .unwrap_or_default()
                != expected_entities
            {
                return Ok(false);
            }
            let expected_placements: BTreeSet<String> = replacement
                .placement_ids
                .iter()
                .map(|id| id.as_str().to_string())
                .collect();
            if placement_state
                .get(&replacement.source_path)
                .cloned()
                .unwrap_or_default()
                != expected_placements
            {
                return Ok(false);
            }
            let stored_scan: Option<(i64, Option<i64>, Option<String>)> = conn
                .query_row(
                    "SELECT 1, len_bytes, fingerprint
                     FROM source_scans WHERE source_path = ?1",
                    [&replacement.source_path],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(backend)?;
            let Some((_, stored_len, stored_fingerprint)) = stored_scan else {
                return Ok(false);
            };
            // 与 sources_are_current 同口径：指纹缓存参与 no-op 判定。字节已变而
            // 缓存未更新的源必须走提交路径重写 source_scans，否则指纹缓存永不收敛。
            if replacement.len_bytes != stored_len
                || replacement.fingerprint.as_deref() != stored_fingerprint.as_deref()
            {
                return Ok(false);
            }
            let relation_complete: bool = conn
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM source_relation_scans WHERE source_path = ?1
                     )",
                    [&replacement.source_path],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if relation_complete != replacement.relation_complete {
                return Ok(false);
            }
        }
        Ok(true)
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
        self.begin_index_batch_with_relations(upserts, deletes, &RelationManifests::default())
    }

    fn begin_index_batch_with_relations(
        &self,
        upserts: &[(StableId, Vec<u8>, String)],
        deletes: &[StableId],
        relations: &RelationManifests,
    ) -> PortResult<PendingIndexBatch> {
        let base = self.active_generation()?;
        let target = base
            .checked_add(1)
            .ok_or_else(|| PortError::Backend("generation overflow".into()))?;
        let target_sql = i64::try_from(target).map_err(backend)?;
        let base_sql = i64::try_from(base).map_err(backend)?;
        let op = operation_id()?;
        let manifest = batch_manifest(upserts, deletes, relations)?;
        let upsert_json = serde_json::to_string(&manifest.upsert_ids).map_err(backend)?;
        let delete_json = serde_json::to_string(&manifest.delete_ids).map_err(backend)?;
        let conn = self.conn.borrow();
        conn.execute(
            "INSERT INTO index_batches(
                 operation_id, base_generation, target_generation, state,
                 operation_digest, upsert_ids_json, delete_ids_json,
                 relation_upserts_json, relation_deletes_json,
                 source_replacements_json, durable_point, created_at_ms
             ) VALUES(
                 ?1, ?2, ?3, 'building', ?4, ?5, ?6, ?7, ?8, ?9, 'intent', ?10
             )",
            rusqlite::params![
                op,
                base_sql,
                target_sql,
                manifest.operation_digest,
                upsert_json,
                delete_json,
                manifest.relation_upserts_json,
                manifest.relation_deletes_json,
                manifest.source_replacements_json,
                unix_ms()?,
            ],
        )
        .map_err(backend)?;
        Ok(PendingIndexBatch {
            operation_id: op,
            base_generation: base,
            target_generation: target,
            operation_digest: manifest.operation_digest,
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
        self.commit_index_batch_with_relations(
            pending,
            upserts,
            deletes,
            &RelationManifests::default(),
        )
    }

    /// 事务内校验 pending 句柄仍可安全激活：generation CAS + intent 行状态 + manifest 匹配。
    ///
    /// 从 [`commit_index_batch_with_relations`](Self::commit_index_batch_with_relations) 抽出，
    /// 使 rebuild 路径复用同一套“不覆盖更新基线 / 不与 durable intent 分歧”的前置检查。
    fn verify_pending_in_tx(
        tx: &rusqlite::Transaction<'_>,
        pending: &PendingIndexBatch,
        upserts: &[(StableId, Vec<u8>, String)],
        deletes: &[StableId],
        relations: &RelationManifests,
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
            declared_relation_upserts,
            declared_relation_deletes,
            declared_source_replacements,
        ): (
            i64,
            i64,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        ) = tx
            .query_row(
                "SELECT base_generation, target_generation, state, operation_digest,
                        upsert_ids_json, delete_ids_json, relation_upserts_json,
                        relation_deletes_json, source_replacements_json
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
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
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
        let actual = batch_manifest(upserts, deletes, relations)?;
        let actual_upserts = serde_json::to_string(&actual.upsert_ids).map_err(backend)?;
        let actual_deletes = serde_json::to_string(&actual.delete_ids).map_err(backend)?;
        let handle_matches = declared_base == pending.base_generation as i64
            && declared_target == pending.target_generation as i64
            && declared_digest == pending.operation_digest;
        if !handle_matches
            || declared_digest != actual.operation_digest
            || declared_upserts != actual_upserts
            || declared_deletes != actual_deletes
            || declared_relation_upserts != actual.relation_upserts_json
            || declared_relation_deletes != actual.relation_deletes_json
            || declared_source_replacements != actual.source_replacements_json
        {
            return Err(PortError::Backend(format!(
                "index batch {} payload does not match durable intent",
                pending.operation_id
            )));
        }
        Ok(())
    }

    fn commit_index_batch_with_relations(
        &self,
        pending: &PendingIndexBatch,
        upserts: &[(StableId, Vec<u8>, String)],
        deletes: &[StableId],
        relations: &RelationManifests,
    ) -> PortResult<()> {
        let mut conn = self.conn.borrow_mut();
        let tx = conn.transaction().map_err(backend)?;
        Self::verify_pending_in_tx(&tx, pending, upserts, deletes, relations)?;

        // Prepared statements hoisted out of the per-entity loops: 200K
        // entities × 5 statements per batch paid a prepare/finalize per
        // execute, which dominates the constant factor of first ingest.
        // Scoped so the borrow ends before the relation/source loops below.
        {
            let mut stmt_catalog_upsert = tx
                .prepare(
                    "INSERT INTO catalog(id, payload) VALUES(?1, ?2)
                     ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
                )
                .map_err(backend)?;
            // 按 rowid 删除而非按内容列 id 比较：fts5 的 id 是内容列不是 rowid，
            // 内容比较会让每次删除整表扫描（O(N²) 病根）。rowid 在插入时回写进
            // fts_ids.fts_rowid 边车，此处经边车 wire_id 主键 O(1) 定位；wire 与
            // id_json 两种删除源都归一到该主键（同一实体只占一行 fts_ids）。
            let mut stmt_fts_delete = tx
                .prepare(
                    "DELETE FROM fts
                     WHERE rowid = (SELECT fts_rowid FROM fts_ids WHERE wire_id = ?1)",
                )
                .map_err(backend)?;
            let mut stmt_fts_ids_delete = tx
                .prepare("DELETE FROM fts_ids WHERE wire_id = ?1")
                .map_err(backend)?;
            let mut stmt_fts_insert = tx
                .prepare("INSERT INTO fts(id, text) VALUES(?1, ?2)")
                .map_err(backend)?;
            let mut stmt_fts_ids_insert = tx
                .prepare("INSERT INTO fts_ids(wire_id, id_json, fts_rowid) VALUES(?1, ?2, ?3)")
                .map_err(backend)?;
            let mut stmt_catalog_delete = tx
                .prepare("DELETE FROM catalog WHERE id = ?1")
                .map_err(backend)?;

            for (id, payload, text) in upserts {
                stmt_catalog_upsert
                    .execute(rusqlite::params![id.as_str(), payload])
                    .map_err(backend)?;
                let id_json = serde_json::to_string(id).map_err(backend)?;
                stmt_fts_delete.execute([id.as_str()]).map_err(backend)?;
                stmt_fts_ids_delete
                    .execute([id.as_str()])
                    .map_err(backend)?;
                // 只有 Message 实体进入 fts 全文表——session/document 是检索容器实体，
                // 索引其正文会让搜索命中重复计数。fts_ids 身份边车则对所有 kind 保留：
                // 它保真 kind+stability，rebuild 依赖它恢复非 Unstable 身份（见 rebuild_index）。
                // fts_rowid 只对进入 fts 的 Message 行回写，其余保持 NULL——删除按
                // NULL 定位即无操作，与旧语义一致（无 fts 行可删）。
                let fts_rowid = if id.kind() == IdKind::Message {
                    stmt_fts_insert
                        .execute(rusqlite::params![id_json, text])
                        .map_err(backend)?;
                    Some(tx.last_insert_rowid())
                } else {
                    None
                };
                stmt_fts_ids_insert
                    .execute(rusqlite::params![id.as_str(), id_json, fts_rowid])
                    .map_err(backend)?;
            }
            for id in deletes {
                stmt_catalog_delete
                    .execute([id.as_str()])
                    .map_err(backend)?;
                stmt_fts_delete.execute([id.as_str()]).map_err(backend)?;
                stmt_fts_ids_delete
                    .execute([id.as_str()])
                    .map_err(backend)?;
            }
        }

        for delete in &relations.relation_deletes {
            match delete {
                RelationDeleteManifest::Edge(placement_id) => {
                    tx.execute(
                        "DELETE FROM message_edges WHERE child_placement_id = ?1",
                        [placement_id.as_str()],
                    )
                    .map_err(backend)?;
                }
                RelationDeleteManifest::Placement(placement_id) => {
                    tx.execute(
                        "DELETE FROM message_placements WHERE placement_id = ?1",
                        [placement_id.as_str()],
                    )
                    .map_err(backend)?;
                }
            }
        }
        for upsert in &relations.relation_upserts {
            match upsert {
                RelationUpsertManifest::Placement(placement) => {
                    let (byte_start, byte_end) = match &placement.span {
                        Some(span) => (
                            Some(i64::try_from(span.start).map_err(backend)?),
                            Some(i64::try_from(span.end).map_err(backend)?),
                        ),
                        None => (None, None),
                    };
                    tx.execute(
                        "INSERT INTO message_placements(
                             placement_id, session_id, document_id, message_id,
                             source_ordinal, is_sidechain, byte_start, byte_end
                         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                         ON CONFLICT(placement_id) DO UPDATE SET
                             session_id = excluded.session_id,
                             document_id = excluded.document_id,
                             message_id = excluded.message_id,
                             source_ordinal = excluded.source_ordinal,
                             is_sidechain = excluded.is_sidechain,
                             byte_start = excluded.byte_start,
                             byte_end = excluded.byte_end",
                        rusqlite::params![
                            placement.id.as_str(),
                            placement.session_id.as_str(),
                            placement.source_document_id.as_str(),
                            placement.message_id.as_str(),
                            i64::from(placement.source_ordinal),
                            i64::from(placement.is_sidechain),
                            byte_start,
                            byte_end,
                        ],
                    )
                    .map_err(backend)?;
                }
                RelationUpsertManifest::Edge(edge) => {
                    tx.execute(
                        "INSERT INTO message_edges(
                             child_placement_id, parent_message_id,
                             parent_native_id, relation
                         ) VALUES(?1, ?2, ?3, ?4)
                         ON CONFLICT(child_placement_id) DO UPDATE SET
                             parent_message_id = excluded.parent_message_id,
                             parent_native_id = excluded.parent_native_id,
                             relation = excluded.relation",
                        rusqlite::params![
                            edge.child_placement_id.as_str(),
                            edge.parent_message_id.as_str(),
                            &edge.parent_native_id,
                            edge.relation.as_str(),
                        ],
                    )
                    .map_err(backend)?;
                }
            }
        }

        for source in &relations.source_replacements {
            tx.execute(
                "DELETE FROM source_membership WHERE source_path = ?1",
                [&source.source_path],
            )
            .map_err(backend)?;
            for membership in &source.entity_memberships {
                tx.execute(
                    "INSERT INTO source_membership(source_path, message_id, document_id)
                     VALUES(?1, ?2, ?3)",
                    rusqlite::params![
                        &source.source_path,
                        &membership.entity_id,
                        &membership.document_id
                    ],
                )
                .map_err(backend)?;
            }
            tx.execute(
                "DELETE FROM source_placement_membership WHERE source_path = ?1",
                [&source.source_path],
            )
            .map_err(backend)?;
            for placement_id in &source.placement_ids {
                tx.execute(
                    "INSERT INTO source_placement_membership(source_path, placement_id)
                     VALUES(?1, ?2)",
                    rusqlite::params![&source.source_path, placement_id.as_str()],
                )
                .map_err(backend)?;
            }
            tx.execute(
                "INSERT INTO source_scans(source_path, scanned_at_ms, len_bytes, fingerprint)
                 VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(source_path) DO UPDATE SET
                     scanned_at_ms = excluded.scanned_at_ms,
                     len_bytes = excluded.len_bytes,
                     fingerprint = excluded.fingerprint",
                rusqlite::params![
                    &source.source_path,
                    unix_ms()?,
                    source.len_bytes,
                    source.fingerprint,
                ],
            )
            .map_err(backend)?;
            if source.relation_complete {
                tx.execute(
                    "INSERT INTO source_relation_scans(source_path, relation_schema_version)
                     VALUES(?1, ?2)
                     ON CONFLICT(source_path) DO UPDATE SET
                         relation_schema_version = excluded.relation_schema_version",
                    rusqlite::params![&source.source_path, SCHEMA_VERSION],
                )
                .map_err(backend)?;
            } else {
                tx.execute(
                    "DELETE FROM source_relation_scans WHERE source_path = ?1",
                    [&source.source_path],
                )
                .map_err(backend)?;
            }
        }

        let batch_sources: Vec<String> = relations
            .source_replacements
            .iter()
            .map(|replacement| replacement.source_path.clone())
            .collect();
        Self::regenerate_compatibility_aliases_in_tx(&tx, &batch_sources)?;

        // 本批触碰的关系行：只校验这些 id 的引用完整性。
        let mut touched_placements: Vec<String> = Vec::new();
        let mut touched_edges: Vec<String> = Vec::new();
        let mut touched_claims: Vec<String> = Vec::new();
        for source in &relations.source_replacements {
            touched_claims.extend(
                source
                    .placement_ids
                    .iter()
                    .map(|id| id.as_str().to_string()),
            );
        }
        for upsert in &relations.relation_upserts {
            match upsert {
                RelationUpsertManifest::Placement(placement) => {
                    touched_placements.push(placement.id.as_str().to_string());
                }
                RelationUpsertManifest::Edge(edge) => {
                    touched_edges.push(edge.child_placement_id.as_str().to_string());
                }
            }
        }
        for delete in &relations.relation_deletes {
            match delete {
                RelationDeleteManifest::Placement(id) => {
                    touched_placements.push(id.as_str().to_string());
                }
                RelationDeleteManifest::Edge(id) => {
                    touched_edges.push(id.as_str().to_string());
                }
            }
        }
        Self::verify_relational_integrity_in_tx(
            &tx,
            &touched_placements,
            &touched_edges,
            &touched_claims,
        )?;
        // B1 路径（裸 commit_batch/commit_index_batch）只删 catalog 实体、不维护
        // v7 关系行：被删实体若仍被 message_placements/message_edges 引用，会留下
        // 悬空引用，必须在此拒绝（B2 路径的删除按 claimer 推导，天然无悬空）。
        Self::verify_deleted_entities_unreferenced_in_tx(&tx, deletes)?;

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

    /// 校验本批触碰的关系行引用完整性。
    ///
    /// 只检查本批 upsert/delete 涉及的 placement/edge/claim ids：未触碰行的
    /// 完整性由归纳保持（每次提交维护自身行、删除只删本批 claims）。全表
    /// 扫描版本使每批提交成本 O(全库)，是首次 ingest O(n²) 的来源之一。
    fn verify_relational_integrity_in_tx(
        tx: &rusqlite::Transaction<'_>,
        touched_placement_ids: &[String],
        touched_edge_ids: &[String],
        touched_claim_ids: &[String],
    ) -> PortResult<()> {
        for chunk in chunk_ids(touched_placement_ids) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let missing_entity: Option<String> = tx
                .query_row(
                    &format!(
                        "SELECT placement_id
                         FROM message_placements
                         WHERE placement_id IN ({placeholders})
                           AND (NOT EXISTS(
                                    SELECT 1 FROM catalog WHERE id = message_placements.session_id
                                )
                             OR NOT EXISTS(
                                    SELECT 1 FROM catalog WHERE id = message_placements.document_id
                                )
                             OR NOT EXISTS(
                                    SELECT 1 FROM catalog WHERE id = message_placements.message_id
                                ))
                         LIMIT 1"
                    ),
                    rusqlite::params_from_iter(chunk.iter()),
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if missing_entity.is_some() {
                return Err(PortError::Backend(
                    "message placement references a missing catalog entity".into(),
                ));
            }
        }

        for chunk in chunk_ids(touched_edge_ids) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let missing_placement: Option<String> = tx
                .query_row(
                    &format!(
                        "SELECT child_placement_id
                         FROM message_edges
                         WHERE child_placement_id IN ({placeholders})
                           AND NOT EXISTS(
                               SELECT 1 FROM message_placements
                               WHERE placement_id = message_edges.child_placement_id
                           )
                         LIMIT 1"
                    ),
                    rusqlite::params_from_iter(chunk.iter()),
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if missing_placement.is_some() {
                return Err(PortError::Backend(
                    "message edge references a missing child placement".into(),
                ));
            }
        }

        for chunk in chunk_ids(touched_claim_ids) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let missing_claim: Option<String> = tx
                .query_row(
                    &format!(
                        "SELECT placement_id
                         FROM source_placement_membership
                         WHERE placement_id IN ({placeholders})
                           AND NOT EXISTS(
                               SELECT 1 FROM message_placements
                               WHERE placement_id = source_placement_membership.placement_id
                           )
                         LIMIT 1"
                    ),
                    rusqlite::params_from_iter(chunk.iter()),
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if missing_claim.is_some() {
                return Err(PortError::Backend(
                    "source placement claim references a missing placement".into(),
                ));
            }
        }
        Ok(())
    }

    /// 校验 delete 列表中的 catalog 实体不被任何 v7 关系行引用。
    ///
    /// B1 路径（裸 commit_batch/commit_index_batch）不维护关系行：若被删实体仍被
    /// `message_placements`（session/document/message 任一身份）或 `message_edges`
    /// （parent_message_id）引用，提交会留下悬空引用且事后才被发现。按 chunk 检查
    /// 引用存在性，任一命中即拒绝整批。
    fn verify_deleted_entities_unreferenced_in_tx(
        tx: &rusqlite::Transaction<'_>,
        deletes: &[StableId],
    ) -> PortResult<()> {
        let ids: Vec<&str> = deletes.iter().map(|id| id.as_str()).collect();
        for chunk in chunk_ids(&ids) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let dangling_placement: Option<String> = tx
                .query_row(
                    &format!(
                        "SELECT placement_id
                         FROM message_placements
                         WHERE session_id IN ({placeholders})
                            OR document_id IN ({placeholders})
                            OR message_id IN ({placeholders})
                         LIMIT 1"
                    ),
                    rusqlite::params_from_iter(
                        chunk.iter().chain(chunk.iter()).chain(chunk.iter()),
                    ),
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if dangling_placement.is_some() {
                return Err(PortError::Backend(
                    "cannot delete a catalog entity still referenced by message placements".into(),
                ));
            }
            let dangling_edge: Option<String> = tx
                .query_row(
                    &format!(
                        "SELECT child_placement_id
                         FROM message_edges
                         WHERE parent_message_id IN ({placeholders})
                         LIMIT 1"
                    ),
                    rusqlite::params_from_iter(chunk.iter()),
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if dangling_edge.is_some() {
                return Err(PortError::Backend(
                    "cannot delete a catalog entity still referenced by message edges".into(),
                ));
            }
        }
        Ok(())
    }

    /// 在给定事务内维护单条实体的 fts 行与 fts_ids 身份边车。
    ///
    /// 与批量提交路径同语义：先按边车记录的 rowid 删除旧 fts 行（避免内容列整表
    /// 扫描），再按 kind 门控——只有 Message 实体进入 fts 全文表，session/document
    /// 是容器实体，索引其正文会让搜索命中重复计数；非 Message 只保留身份边车
    /// （fts_rowid 为 NULL，按 NULL 定位删除即无操作）。
    /// 单条 `SearchIndex::index` 与 `CatalogStore::put` 共用（后者投影自 payload）。
    fn upsert_fts_row_in_tx(
        tx: &rusqlite::Transaction<'_>,
        id: &StableId,
        text: &str,
    ) -> PortResult<()> {
        // StableId 无字符串反解构造器，故存其 serde JSON 以便查询时无损重建
        // （wire 串不含 stability，无法从 as_str() 还原完整身份）。
        let id_json = serde_json::to_string(id).map_err(backend)?;
        tx.execute(
            "DELETE FROM fts
             WHERE rowid = (SELECT fts_rowid FROM fts_ids WHERE wire_id = ?1)",
            [id.as_str()],
        )
        .map_err(backend)?;
        tx.execute("DELETE FROM fts_ids WHERE wire_id = ?1", [id.as_str()])
            .map_err(backend)?;
        let fts_rowid = if id.kind() == IdKind::Message {
            tx.execute(
                "INSERT INTO fts(id, text) VALUES(?1, ?2)",
                rusqlite::params![id_json, text],
            )
            .map_err(backend)?;
            Some(tx.last_insert_rowid())
        } else {
            None
        };
        tx.execute(
            "INSERT INTO fts_ids(wire_id, id_json, fts_rowid) VALUES(?1, ?2, ?3)",
            rusqlite::params![id.as_str(), id_json, fts_rowid],
        )
        .map_err(backend)?;
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
                        PortError::Backend("catalog contains an invalid entity id".into())
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
        Self::verify_pending_in_tx(&tx, &pending, &upserts, &[], &RelationManifests::default())?;

        tx.execute("DELETE FROM fts", []).map_err(backend)?;
        tx.execute("DELETE FROM fts_ids", []).map_err(backend)?;
        // 与提交路径一致：只有 Message 实体重投影进 fts，且把 fts5 行 rowid 回写
        // 进 fts_ids 边车，删除才能按 rowid 定位（见 ensure_fts_ids_rowid）。
        {
            let mut stmt_fts_insert = tx
                .prepare("INSERT INTO fts(id, text) VALUES(?1, ?2)")
                .map_err(backend)?;
            let mut stmt_fts_ids_insert = tx
                .prepare("INSERT INTO fts_ids(wire_id, id_json, fts_rowid) VALUES(?1, ?2, ?3)")
                .map_err(backend)?;
            for (id, _payload, text) in &upserts {
                let id_json = serde_json::to_string(id).map_err(backend)?;
                let fts_rowid = if id.kind() == IdKind::Message {
                    stmt_fts_insert
                        .execute(rusqlite::params![id_json, text])
                        .map_err(backend)?;
                    Some(tx.last_insert_rowid())
                } else {
                    None
                };
                stmt_fts_ids_insert
                    .execute(rusqlite::params![id.as_str(), id_json, fts_rowid])
                    .map_err(backend)?;
            }
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
                        relation_upserts_json, relation_deletes_json,
                        source_replacements_json, durable_point, error_code
                 FROM index_batches WHERE operation_id = ?1",
            )
            .map_err(backend)?;
        let mut rows = stmt.query([operation_id]).map_err(backend)?;
        match rows.next().map_err(backend)? {
            None => Ok(None),
            Some(row) => {
                let upsert_json: String = row.get(5).map_err(backend)?;
                let delete_json: String = row.get(6).map_err(backend)?;
                let relation_upserts_json: String = row.get(7).map_err(backend)?;
                let relation_deletes_json: String = row.get(8).map_err(backend)?;
                let source_replacements_json: String = row.get(9).map_err(backend)?;
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
                    relation_upserts: serde_json::from_str(&relation_upserts_json)
                        .map_err(backend)?,
                    relation_deletes: serde_json::from_str(&relation_deletes_json)
                        .map_err(backend)?,
                    source_replacements: serde_json::from_str(&source_replacements_json)
                        .map_err(backend)?,
                    durable_point: row.get(10).map_err(backend)?,
                    error_code: row.get(11).map_err(backend)?,
                }))
            }
        }
    }
}

/// 当前 catalog schema 版本。每次结构变更 +1 并在 [`SqliteStore::migrate`] 追加步骤。
pub const SCHEMA_VERSION: i64 = 7;

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
        let mut conn = self.conn.borrow_mut();
        let tx = conn.transaction().map_err(backend)?;
        tx.execute(
            "INSERT INTO catalog(id, payload) VALUES(?1, ?2)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            rusqlite::params![id.as_str(), payload],
        )
        .map_err(backend)?;
        // catalog 是内容的权威事实源：put 更新 payload 后必须同步维护 fts/边车，
        // 否则消息内容更新后旧文本仍可搜（与 rebuild_index 用同一 searchable_text
        // 投影函数，避免再次分叉）。
        Self::upsert_fts_row_in_tx(&tx, id, &searchable_text(payload))?;
        tx.commit().map_err(backend)?;
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
                PortError::Backend("catalog contains an invalid entity id".into())
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

impl ContextGraphStore for SqliteStore {
    fn load_session_graph(&self, session_id: &StableId) -> PortResult<SessionContextGraph> {
        if session_id.kind() != IdKind::Session {
            return Err(PortError::NotFound("session context not found".into()));
        }
        let conn = self.conn.borrow();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM catalog WHERE id = ?1)",
                [session_id.as_str()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if !exists {
            return Err(PortError::NotFound("session context not found".into()));
        }

        let sources = Self::relation_sources_for_session(&conn, session_id.as_str())?;
        Self::require_relation_complete_sources(&conn, &sources, true, "session")?;
        let stored_session_id = Self::stable_id_from_store(&conn, session_id.as_str())?;

        let raw_placements = {
            let mut stmt = conn
                .prepare(
                    "SELECT placement_id, document_id, message_id, source_ordinal,
                            is_sidechain, byte_start, byte_end
                     FROM message_placements
                     WHERE session_id = ?1
                     ORDER BY document_id, source_ordinal, placement_id",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([session_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                    ))
                })
                .map_err(backend)?;
            let mut placements = Vec::new();
            for row in rows {
                placements.push(row.map_err(backend)?);
            }
            placements
        };

        let mut message_wires = BTreeSet::new();
        let mut document_wires = BTreeSet::new();

        // 先收集全部 wires，再批量加载 payload/identity（避免 N+1）。
        for (_, document_id, message_id, _, _, _, _) in &raw_placements {
            message_wires.insert(message_id.clone());
            document_wires.insert(document_id.clone());
        }
        {
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT document_id
                     FROM source_membership
                     WHERE message_id = ?1 AND document_id IS NOT NULL",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([session_id.as_str()], |row| row.get::<_, String>(0))
                .map_err(backend)?;
            for row in rows {
                document_wires.insert(row.map_err(backend)?);
            }
        }

        // Batch-load message payloads and fts_ids identity for all wires at
        // once (was N+1 per message: one payload read + two identity reads).
        // Chunked under the SQLite variable limit for very large sessions.
        let mut payload_by_id: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut id_json_by_wire: BTreeMap<String, String> = BTreeMap::new();
        let all_wires: Vec<&str> = message_wires
            .iter()
            .chain(document_wires.iter())
            .map(|wire| wire.as_str())
            .collect();
        for chunk in chunk_ids(&all_wires) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT id, payload FROM catalog WHERE id IN ({placeholders})"
                ))
                .map_err(backend)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(chunk.iter().copied()), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(backend)?;
            for row in rows {
                let (id, payload) = row.map_err(backend)?;
                payload_by_id.insert(id, payload);
            }
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT wire_id, id_json FROM fts_ids WHERE wire_id IN ({placeholders})"
                ))
                .map_err(backend)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(chunk.iter().copied()), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(backend)?;
            for row in rows {
                let (wire_id, id_json) = row.map_err(backend)?;
                id_json_by_wire.insert(wire_id, id_json);
            }
        }

        let mut placements = Vec::with_capacity(raw_placements.len());
        for (placement_id, document_id, message_id, ordinal, sidechain, start, end) in
            raw_placements
        {
            let span = match (start, end) {
                (None, None) => None,
                (Some(start), Some(end)) => Some(EvidenceSpan {
                    start: u64::try_from(start).map_err(backend)?,
                    end: u64::try_from(end).map_err(backend)?,
                }),
                _ => {
                    return Err(PortError::Backend(
                        "stored placement has a partial span".into(),
                    ));
                }
            };
            placements.push(MessagePlacement {
                id: PlacementId::from_wire(&placement_id).ok_or_else(|| {
                    PortError::Backend("stored placement has an invalid id".into())
                })?,
                session_id: stored_session_id.clone(),
                source_document_id: Self::stable_id_from_wire(&document_id, &id_json_by_wire)?,
                message_id: Self::stable_id_from_wire(&message_id, &id_json_by_wire)?,
                source_ordinal: u32::try_from(ordinal).map_err(backend)?,
                is_sidechain: sidechain != 0,
                span,
            });
        }

        let mut messages = Vec::with_capacity(message_wires.len());
        for wire in &message_wires {
            let payload = payload_by_id.get(wire).cloned().ok_or_else(|| {
                PortError::Backend("session placement references a missing message".into())
            })?;
            let map = match serde_json::from_slice::<serde_json::Value>(&payload) {
                Ok(serde_json::Value::Object(map)) => map,
                _ => {
                    return Err(PortError::Backend(
                        "stored message payload is not an object".into(),
                    ));
                }
            };
            let role = map
                .get("role")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| PortError::Backend("stored message is missing role".into()))
                .and_then(stored_role)?;
            let text = map
                .get("text")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| PortError::Backend("stored message is missing text".into()))?
                .to_string();
            let timestamp = match map.get("timestamp") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(value)) => Some(value.clone()),
                Some(_) => {
                    return Err(PortError::Backend(
                        "stored message timestamp is not a string".into(),
                    ));
                }
            };
            messages.push(Message {
                id: Self::stable_id_from_wire(wire, &id_json_by_wire)?,
                role,
                text,
                timestamp,
            });
        }

        let mut source_documents = Vec::with_capacity(document_wires.len());
        for wire in &document_wires {
            let payload = payload_by_id.get(wire).cloned().ok_or_else(|| {
                PortError::Backend("session context references a missing document".into())
            })?;
            let map = match serde_json::from_slice::<serde_json::Value>(&payload) {
                Ok(serde_json::Value::Object(map)) => map,
                _ => {
                    return Err(PortError::Backend(
                        "stored document payload is not an object".into(),
                    ));
                }
            };
            source_documents.push(SourceDocument {
                id: Self::stable_id_from_wire(wire, &id_json_by_wire)?,
                provider_id: map
                    .get("provider")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        PortError::Backend("stored document is missing provider".into())
                    })?
                    .to_string(),
                variant_id: map
                    .get("variant")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| PortError::Backend("stored document is missing variant".into()))?
                    .to_string(),
                fingerprint: map
                    .get("fingerprint")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        PortError::Backend("stored document is missing fingerprint".into())
                    })?
                    .to_string(),
                len: map
                    .get("len")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        PortError::Backend("stored document is missing byte length".into())
                    })?,
            });
        }

        let raw_edges = {
            let mut stmt = conn
                .prepare(
                    "SELECT edges.child_placement_id, edges.parent_message_id,
                            edges.parent_native_id, edges.relation
                     FROM message_edges AS edges
                     JOIN message_placements AS placements
                       ON placements.placement_id = edges.child_placement_id
                     WHERE placements.session_id = ?1
                     ORDER BY edges.child_placement_id",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([session_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(backend)?;
            let mut edges = Vec::new();
            for row in rows {
                edges.push(row.map_err(backend)?);
            }
            edges
        };
        let mut edges = Vec::with_capacity(raw_edges.len());
        for (child_placement_id, parent_message_id, parent_native_id, relation) in raw_edges {
            edges.push(MessageEdge {
                child_placement_id: PlacementId::from_wire(&child_placement_id).ok_or_else(
                    || PortError::Backend("stored edge has an invalid child placement id".into()),
                )?,
                parent_message_id: Self::stable_id_from_store(&conn, &parent_message_id)?,
                parent_native_id,
                relation: stored_relation(&relation)?,
            });
        }

        let graph = SessionContextGraph {
            session_id: stored_session_id,
            messages,
            source_documents,
            placements,
            edges,
        };
        graph
            .validate()
            .map_err(|error| PortError::Backend(error.to_string()))?;
        Ok(graph)
    }

    fn message_contexts(&self, message_id: &StableId) -> PortResult<Vec<MessageContextCandidate>> {
        if message_id.kind() != IdKind::Message {
            return Err(PortError::NotFound("message context not found".into()));
        }
        let conn = self.conn.borrow();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM catalog WHERE id = ?1)",
                [message_id.as_str()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if !exists {
            return Err(PortError::NotFound("message context not found".into()));
        }

        let message_sources = Self::relation_sources_for_message(&conn, message_id.as_str())?;
        Self::require_relation_complete_sources(&conn, &message_sources, false, "message")?;
        let mut grouped = BTreeMap::<String, Vec<PlacementId>>::new();
        {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id, placement_id
                     FROM message_placements
                     WHERE message_id = ?1
                     ORDER BY session_id, placement_id",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([message_id.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(backend)?;
            for row in rows {
                let (session_id, placement_id) = row.map_err(backend)?;
                grouped.entry(session_id).or_default().push(
                    PlacementId::from_wire(&placement_id).ok_or_else(|| {
                        PortError::Backend("stored placement has an invalid id".into())
                    })?,
                );
            }
        }

        let mut candidates = Vec::with_capacity(grouped.len());
        for (session_wire, mut placement_ids) in grouped {
            let session_sources = Self::relation_sources_for_session(&conn, &session_wire)?;
            Self::require_relation_complete_sources(&conn, &session_sources, true, "session")?;
            placement_ids.sort();
            candidates.push(MessageContextCandidate {
                session_id: Self::stable_id_from_store(&conn, &session_wire)?,
                placement_ids,
            });
        }
        Ok(candidates)
    }

    fn context_stats(&self) -> PortResult<ContextStats> {
        let conn = self.conn.borrow();
        let placements: i64 = conn
            .query_row("SELECT COUNT(*) FROM message_placements", [], |row| {
                row.get(0)
            })
            .map_err(backend)?;
        let source_placement_claims: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM source_placement_membership",
                [],
                |row| row.get(0),
            )
            .map_err(backend)?;
        Ok(ContextStats {
            placements: u64::try_from(placements).map_err(backend)?,
            source_placement_claims: u64::try_from(source_placement_claims).map_err(backend)?,
        })
    }
}

impl SearchIndex for SqliteStore {
    fn index(&self, id: &StableId, text: &str) -> PortResult<()> {
        let mut conn = self.conn.borrow_mut();
        let tx = conn.transaction().map_err(backend)?;
        Self::upsert_fts_row_in_tx(&tx, id, text)?;
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
    use agent_session_grep_domain::{EvidenceSpan, IdKind, MessageRelation, Stability};

    fn sid(kind: IdKind, fact: &[u8]) -> StableId {
        StableId::derive(kind, Stability::Reconstructed, &[fact])
    }

    fn sqlite_failure(code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None)
    }

    fn create_v6_schema(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE catalog (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
             CREATE VIRTUAL TABLE fts USING fts5(id UNINDEXED, text);
             CREATE TABLE store_metadata (
                 singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                 active_generation INTEGER NOT NULL
             );
             INSERT INTO store_metadata(singleton, active_generation) VALUES(1, 1);
             CREATE TABLE index_batches (
                 operation_id TEXT PRIMARY KEY,
                 base_generation INTEGER NOT NULL,
                 target_generation INTEGER NOT NULL,
                 state TEXT NOT NULL,
                 operation_digest TEXT NOT NULL,
                 upsert_ids_json TEXT NOT NULL,
                 delete_ids_json TEXT NOT NULL,
                 durable_point TEXT NOT NULL,
                 created_at_ms INTEGER NOT NULL,
                 committed_at_ms INTEGER,
                 error_code TEXT
             );
             CREATE INDEX index_batches_state ON index_batches(state);
             CREATE TABLE fts_ids (
                 wire_id TEXT PRIMARY KEY,
                 id_json TEXT NOT NULL UNIQUE
             );
             CREATE TABLE source_membership (
                 source_path TEXT NOT NULL,
                 message_id TEXT NOT NULL,
                 document_id TEXT,
                 PRIMARY KEY(source_path, message_id)
             );
             CREATE INDEX source_membership_source
             ON source_membership(source_path);
             CREATE TABLE source_scans (
                 source_path TEXT PRIMARY KEY,
                 scanned_at_ms INTEGER NOT NULL
             );
             PRAGMA user_version = 6;",
        )
        .unwrap();
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

    fn entity_entry(id: &StableId) -> (StableId, Vec<u8>, String) {
        (
            id.clone(),
            format!("payload:{}", id.as_str()).into_bytes(),
            if id.kind() == IdKind::Message {
                format!("text:{}", id.as_str())
            } else {
                String::new()
            },
        )
    }

    fn relational_message_payload(
        session_id: &StableId,
        document_id: &StableId,
        parent_id: &StableId,
        parent_native_id: &str,
        is_sidechain: bool,
        span: (u64, u64),
    ) -> Vec<u8> {
        serde_json::json!({
            "role": "user",
            "text": "shared stable body",
            "timestamp": "2026-07-28T00:00:00Z",
            "parent": parent_id.as_str(),
            "parent_native_id": parent_native_id,
            "is_sidechain": is_sidechain,
            "session": session_id.as_str(),
            "sessions": [session_id.as_str()],
            "span": { "start": span.0, "end": span.1 },
            "spans": [{
                "document": document_id.as_str(),
                "start": span.0,
                "end": span.1,
            }],
        })
        .to_string()
        .into_bytes()
    }

    fn typed_message_entry(id: &StableId, text: &str) -> (StableId, Vec<u8>, String) {
        (
            id.clone(),
            serde_json::json!({
                "role": "user",
                "text": text,
                "timestamp": "2026-07-28T00:00:00Z",
                "parent": null,
                "parent_native_id": null,
                "is_sidechain": false,
                "session": null,
                "sessions": [],
                "span": null,
                "spans": [],
            })
            .to_string()
            .into_bytes(),
            text.to_string(),
        )
    }

    fn typed_document_entry(id: &StableId) -> (StableId, Vec<u8>, String) {
        (
            id.clone(),
            serde_json::json!({
                "provider": "synthetic",
                "variant": "synthetic/jsonl-v1",
                "fingerprint": "0123456789abcdef",
                "len": 128,
            })
            .to_string()
            .into_bytes(),
            String::new(),
        )
    }

    fn placement(
        session_id: &StableId,
        document_id: &StableId,
        message_id: &StableId,
        source_ordinal: u32,
        is_sidechain: bool,
        span: Option<(u64, u64)>,
    ) -> MessagePlacement {
        MessagePlacement::new(
            session_id.clone(),
            document_id.clone(),
            message_id.clone(),
            source_ordinal,
            is_sidechain,
            span.map(|(start, end)| EvidenceSpan { start, end }),
        )
    }

    fn reply_edge(placement: &MessagePlacement, parent: &StableId) -> MessageEdge {
        MessageEdge {
            child_placement_id: placement.id.clone(),
            parent_message_id: parent.clone(),
            parent_native_id: Some("synthetic-parent".into()),
            relation: MessageRelation::Reply,
        }
    }

    fn source_batch(
        source_path: &str,
        entries: Vec<(StableId, Vec<u8>, String)>,
        placements: Vec<MessagePlacement>,
        edges: Vec<MessageEdge>,
        relation_complete: bool,
    ) -> SourceBatch {
        SourceBatch {
            source_path: source_path.into(),
            entries,
            placements,
            edges,
            relation_complete,
            len_bytes: None,
            fingerprint: None,
        }
    }

    fn table_count(store: &SqliteStore, table: &str) -> i64 {
        store
            .conn
            .borrow()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn source_placement_claims(store: &SqliteStore, source_path: &str) -> Vec<String> {
        let conn = store.conn.borrow();
        let mut stmt = conn
            .prepare(
                "SELECT placement_id FROM source_placement_membership
                 WHERE source_path = ?1 ORDER BY placement_id",
            )
            .unwrap();
        stmt.query_map([source_path], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn relation_complete_marker(store: &SqliteStore, source_path: &str) -> bool {
        store
            .conn
            .borrow()
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM source_relation_scans WHERE source_path = ?1
                 )",
                [source_path],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn latest_index_batch(store: &SqliteStore) -> IndexBatch {
        let operation_id: String = store
            .conn
            .borrow()
            .query_row(
                "SELECT operation_id FROM index_batches
                 ORDER BY target_generation DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        store.index_batch(&operation_id).unwrap().unwrap()
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
        // 用 Message id：非 Message 实体不进 fts 全文表（kind 门，见 index/put）。
        let id = sid(IdKind::Message, b"s1");
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
    fn v6_db_migrates_to_v7_without_fabricating_relations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v6.db");
        let p = path.to_string_lossy().into_owned();
        let payload = vec![0x00, 0xff, 0x7f, 0x01, 0x80];
        {
            let conn = rusqlite::Connection::open(&p).unwrap();
            create_v6_schema(&conn);
            conn.execute(
                "INSERT INTO catalog(id, payload) VALUES('msg_v1_legacy', ?1)",
                [&payload],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO source_membership(source_path, message_id, document_id)
                 VALUES('legacy.jsonl', 'msg_v1_legacy', NULL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO source_scans(source_path, scanned_at_ms)
                 VALUES('legacy.jsonl', 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO index_batches(
                     operation_id, base_generation, target_generation, state,
                     operation_digest, upsert_ids_json, delete_ids_json,
                     durable_point, created_at_ms, committed_at_ms
                 ) VALUES(
                     'legacy-op', 0, 1, 'activated', 'legacy-digest', '[]', '[]',
                     'activated', 1, 2
                 )",
                [],
            )
            .unwrap();
        }

        let store = SqliteStore::open(&p).unwrap();
        assert_eq!(store.schema_version().unwrap(), 7);
        let id = StableId::from_wire("msg_v1_legacy").unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap(), payload);
        let batch = store.index_batch("legacy-op").unwrap().unwrap();
        assert!(batch.relation_upserts.is_empty());
        assert!(batch.relation_deletes.is_empty());
        assert!(batch.source_replacements.is_empty());
        drop(store);

        let conn = rusqlite::Connection::open(&p).unwrap();
        let document_id: Option<String> = conn
            .query_row(
                "SELECT document_id FROM source_membership
                 WHERE source_path = 'legacy.jsonl' AND message_id = 'msg_v1_legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(document_id, None);

        for table in [
            "message_placements",
            "message_edges",
            "source_placement_membership",
            "source_relation_scans",
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} must start empty");
        }
        let relation_scan_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM source_relation_scans", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(relation_scan_count, 0);

        let manifests: (String, String, String) = conn
            .query_row(
                "SELECT relation_upserts_json, relation_deletes_json,
                        source_replacements_json
                 FROM index_batches WHERE operation_id = 'legacy-op'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            manifests,
            ("[]".to_string(), "[]".to_string(), "[]".to_string())
        );

        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'index' AND name IN (
                     'message_placements_session_order',
                     'message_placements_message',
                     'message_placements_document',
                     'source_placement_membership_placement'
                 )
                 ORDER BY name",
            )
            .unwrap();
        let indexes: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            indexes,
            vec![
                "message_placements_document",
                "message_placements_message",
                "message_placements_session_order",
                "source_placement_membership_placement",
            ]
        );
    }

    #[test]
    fn injected_v6_to_v7_failure_rolls_back_schema_and_version() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        create_v6_schema(&conn);

        let err = SqliteStore::migrate_v6_to_v7_inner(&conn, true).unwrap_err();
        assert!(
            matches!(err, PortError::Backend(message) if message.contains("injected v6-to-v7"))
        );
        assert!(conn.is_autocommit());
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 6);

        for table in [
            "message_placements",
            "message_edges",
            "source_placement_membership",
            "source_relation_scans",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 0, "{table} must roll back");
        }
        let mut stmt = conn.prepare("PRAGMA table_info(index_batches)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(!columns.iter().any(|name| {
            matches!(
                name.as_str(),
                "relation_upserts_json" | "relation_deletes_json" | "source_replacements_json"
            )
        }));
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
    fn relation_and_marker_only_changes_advance_once_then_noop() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"relation-generation-session");
        let document = sid(IdKind::Document, b"relation-generation-document");
        let parent = sid(IdKind::Message, b"relation-generation-parent");
        let child = sid(IdKind::Message, b"relation-generation-child");
        let child_placement = placement(&session, &document, &child, 1, false, Some((10, 20)));
        let entries = || {
            [&session, &document, &parent, &child]
                .into_iter()
                .map(entity_entry)
                .collect()
        };

        let initial = source_batch(
            "relation-generation.jsonl",
            entries(),
            vec![child_placement.clone()],
            Vec::new(),
            true,
        );
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&initial))
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), 1);

        let edge = reply_edge(&child_placement, &parent);
        let relation_only = source_batch(
            "relation-generation.jsonl",
            entries(),
            vec![child_placement.clone()],
            vec![edge.clone()],
            true,
        );
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&relation_only))
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), 2);
        let relation_batch = latest_index_batch(&store);
        assert_eq!(relation_batch.relation_upserts.len(), 2);
        assert!(relation_batch.relation_deletes.is_empty());
        assert_eq!(relation_batch.source_replacements.len(), 1);

        assert!(
            !store
                .commit_source_batches_if_changed(std::slice::from_ref(&relation_only))
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), 2);

        let marker_only = source_batch(
            "relation-generation.jsonl",
            entries(),
            vec![child_placement.clone()],
            vec![edge.clone()],
            false,
        );
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&marker_only))
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), 3);
        assert!(!relation_complete_marker(
            &store,
            "relation-generation.jsonl"
        ));
        assert_eq!(
            latest_index_batch(&store).source_replacements[0]["relation_complete"],
            serde_json::Value::Bool(false)
        );

        assert!(
            !store
                .commit_source_batches_if_changed(std::slice::from_ref(&marker_only))
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), 3);

        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&relation_only))
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), 4);
        assert!(relation_complete_marker(
            &store,
            "relation-generation.jsonl"
        ));
    }

    #[test]
    fn complete_relations_regenerate_divergent_context_aliases_without_stable_conflict() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session_a = sid(IdKind::Session, b"compat-session-a");
        let session_b = sid(IdKind::Session, b"compat-session-b");
        let document_a = sid(IdKind::Document, b"compat-document-a");
        let document_b = sid(IdKind::Document, b"compat-document-b");
        let parent_a = sid(IdKind::Message, b"compat-parent-a");
        let parent_b = sid(IdKind::Message, b"compat-parent-b");
        let child = sid(IdKind::Message, b"compat-child");
        let placement_a = placement(&session_a, &document_a, &child, 1, false, Some((10, 20)));
        let placement_b = placement(&session_b, &document_b, &child, 2, true, Some((30, 40)));
        let edge_a = MessageEdge {
            child_placement_id: placement_a.id.clone(),
            parent_message_id: parent_a.clone(),
            parent_native_id: Some("native-parent-a".into()),
            relation: MessageRelation::Reply,
        };
        let edge_b = MessageEdge {
            child_placement_id: placement_b.id.clone(),
            parent_message_id: parent_b.clone(),
            parent_native_id: Some("native-parent-b".into()),
            relation: MessageRelation::Reply,
        };
        let sources = [
            source_batch(
                "compat-a.jsonl",
                vec![
                    (
                        child.clone(),
                        relational_message_payload(
                            &session_a,
                            &document_a,
                            &parent_a,
                            "native-parent-a",
                            false,
                            (10, 20),
                        ),
                        "shared stable body".into(),
                    ),
                    (
                        session_a.clone(),
                        session_payload(document_a.as_str(), &[child.as_str()]),
                        String::new(),
                    ),
                    entity_entry(&document_a),
                    entity_entry(&parent_a),
                ],
                vec![placement_a.clone()],
                vec![edge_a],
                true,
            ),
            source_batch(
                "compat-b.jsonl",
                vec![
                    (
                        child.clone(),
                        relational_message_payload(
                            &session_b,
                            &document_b,
                            &parent_b,
                            "native-parent-b",
                            true,
                            (30, 40),
                        ),
                        "shared stable body".into(),
                    ),
                    (
                        session_b.clone(),
                        session_payload(document_b.as_str(), &[child.as_str()]),
                        String::new(),
                    ),
                    entity_entry(&document_b),
                    entity_entry(&parent_b),
                ],
                vec![placement_b.clone()],
                vec![edge_b],
                true,
            ),
        ];
        assert!(store.commit_source_batches_if_changed(&sources).unwrap());

        let stored: serde_json::Value =
            serde_json::from_slice(&store.get(&child).unwrap().unwrap()).unwrap();
        assert_eq!(stored["parent"], serde_json::Value::Null);
        assert_eq!(stored["parent_native_id"], serde_json::Value::Null);
        assert_eq!(stored["is_sidechain"], serde_json::Value::Null);
        assert_eq!(stored["sessions"].as_array().unwrap().len(), 2);
        let spans = stored["spans"].as_array().unwrap();
        assert_eq!(spans.len(), 2);
        assert!(
            spans
                .iter()
                .any(|span| span["placement_id"] == placement_a.id.as_str())
        );
        assert!(
            spans
                .iter()
                .any(|span| span["placement_id"] == placement_b.id.as_str())
        );

        let generation = store.active_generation().unwrap();
        assert!(!store.commit_source_batches_if_changed(&sources).unwrap());
        assert_eq!(store.active_generation().unwrap(), generation);
    }

    #[test]
    fn alias_regeneration_is_scoped_to_batch_sources_and_skips_unchanged_rows() {
        // P0-1: regenerating aliases for the whole catalog per batch made
        // first ingest O(n²). Only entities whose claimers intersect the
        // batch's sources may be rewritten, and a rewrite whose bytes are
        // unchanged must not hit the UPDATE (WAL stays flat).
        let store = SqliteStore::open_in_memory().unwrap();
        let session_a = sid(IdKind::Session, b"scope-session-a");
        let document_a = sid(IdKind::Document, b"scope-document-a");
        let parent_a = sid(IdKind::Message, b"scope-parent-a");
        let child_a = sid(IdKind::Message, b"scope-child-a");
        let placement_a = placement(&session_a, &document_a, &child_a, 1, false, Some((10, 20)));
        let edge_a = MessageEdge {
            child_placement_id: placement_a.id.clone(),
            parent_message_id: parent_a.clone(),
            parent_native_id: Some("native-parent-a".into()),
            relation: MessageRelation::Reply,
        };
        let batch_a = source_batch(
            "scope-a.jsonl",
            vec![
                (
                    child_a.clone(),
                    relational_message_payload(
                        &session_a,
                        &document_a,
                        &parent_a,
                        "native-parent-a",
                        false,
                        (10, 20),
                    ),
                    // entry text 必须与 payload 内嵌 text 一致（生产 CLI 恒等）：
                    // 合并路径按 searchable_text(payload) 重投影 FTS，不一致的
                    // fixture 会把重同步误判为需要"修复"fts 文本而推进 generation。
                    "shared stable body".into(),
                ),
                (
                    session_a.clone(),
                    session_payload(document_a.as_str(), &[child_a.as_str()]),
                    String::new(),
                ),
                entity_entry(&document_a),
                entity_entry(&parent_a),
            ],
            vec![placement_a.clone()],
            vec![edge_a],
            true,
        );
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&batch_a))
                .unwrap()
        );

        // Second batch touches only source B; entity A's stored payload must
        // remain byte-identical after commit B.
        let session_b = sid(IdKind::Session, b"scope-session-b");
        let document_b = sid(IdKind::Document, b"scope-document-b");
        let parent_b = sid(IdKind::Message, b"scope-parent-b");
        let child_b = sid(IdKind::Message, b"scope-child-b");
        let placement_b = placement(&session_b, &document_b, &child_b, 1, false, Some((30, 40)));
        let edge_b = MessageEdge {
            child_placement_id: placement_b.id.clone(),
            parent_message_id: parent_b.clone(),
            parent_native_id: Some("native-parent-b".into()),
            relation: MessageRelation::Reply,
        };
        let batch_b = source_batch(
            "scope-b.jsonl",
            vec![
                (
                    child_b.clone(),
                    relational_message_payload(
                        &session_b,
                        &document_b,
                        &parent_b,
                        "native-parent-b",
                        false,
                        (30, 40),
                    ),
                    "shared stable body".into(),
                ),
                (
                    session_b.clone(),
                    session_payload(document_b.as_str(), &[child_b.as_str()]),
                    String::new(),
                ),
                entity_entry(&document_b),
                entity_entry(&parent_b),
            ],
            vec![placement_b.clone()],
            vec![edge_b],
            true,
        );
        let stored_a_before = store.get(&child_a).unwrap().unwrap();
        assert!(store.commit_source_batches_if_changed(&[batch_b]).unwrap());
        let stored_a_after = store.get(&child_a).unwrap().unwrap();
        assert_eq!(
            stored_a_before, stored_a_after,
            "entity owned only by an untouched source must not be rewritten"
        );
        // And a full re-sync of A is a no-op that does not advance generation.
        let generation = store.active_generation().unwrap();
        assert!(!store.commit_source_batches_if_changed(&[batch_a]).unwrap());
        assert_eq!(store.active_generation().unwrap(), generation);
    }

    #[test]
    fn mixed_relation_completeness_preserves_alias_until_last_contributor_is_complete() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session_a = sid(IdKind::Session, b"mixed-session-a");
        let session_b = sid(IdKind::Session, b"mixed-session-b");
        let document_a = sid(IdKind::Document, b"mixed-document-a");
        let document_b = sid(IdKind::Document, b"mixed-document-b");
        let parent_a = sid(IdKind::Message, b"mixed-parent-a");
        let parent_b = sid(IdKind::Message, b"mixed-parent-b");
        let child = sid(IdKind::Message, b"mixed-child");
        let placement_a = placement(&session_a, &document_a, &child, 1, false, Some((10, 20)));
        let placement_b = placement(&session_b, &document_b, &child, 2, true, Some((30, 40)));
        let source_a = |complete| {
            source_batch(
                "mixed-a.jsonl",
                vec![
                    (
                        child.clone(),
                        relational_message_payload(
                            &session_a,
                            &document_a,
                            &parent_a,
                            "native-parent-a",
                            false,
                            (10, 20),
                        ),
                        "shared stable body".into(),
                    ),
                    (
                        session_a.clone(),
                        session_payload(document_a.as_str(), &[child.as_str()]),
                        String::new(),
                    ),
                    entity_entry(&document_a),
                    entity_entry(&parent_a),
                ],
                vec![placement_a.clone()],
                vec![MessageEdge {
                    child_placement_id: placement_a.id.clone(),
                    parent_message_id: parent_a.clone(),
                    parent_native_id: Some("native-parent-a".into()),
                    relation: MessageRelation::Reply,
                }],
                complete,
            )
        };
        let source_b = source_batch(
            "mixed-b.jsonl",
            vec![
                (
                    child.clone(),
                    relational_message_payload(
                        &session_b,
                        &document_b,
                        &parent_b,
                        "native-parent-b",
                        true,
                        (30, 40),
                    ),
                    "shared stable body".into(),
                ),
                (
                    session_b.clone(),
                    session_payload(document_b.as_str(), &[child.as_str()]),
                    String::new(),
                ),
                entity_entry(&document_b),
                entity_entry(&parent_b),
            ],
            vec![placement_b.clone()],
            vec![MessageEdge {
                child_placement_id: placement_b.id.clone(),
                parent_message_id: parent_b.clone(),
                parent_native_id: Some("native-parent-b".into()),
                relation: MessageRelation::Reply,
            }],
            true,
        );

        store
            .commit_source_batches_if_changed(std::slice::from_ref(&source_a(false)))
            .unwrap();
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&source_b))
            .unwrap();
        let mixed: serde_json::Value =
            serde_json::from_slice(&store.get(&child).unwrap().unwrap()).unwrap();
        assert_eq!(mixed["parent"], parent_a.as_str());
        assert_eq!(mixed["parent_native_id"], "native-parent-a");
        assert_eq!(mixed["is_sidechain"], false);

        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&source_a(true)))
                .unwrap()
        );
        let complete: serde_json::Value =
            serde_json::from_slice(&store.get(&child).unwrap().unwrap()).unwrap();
        assert_eq!(complete["parent"], serde_json::Value::Null);
        assert_eq!(complete["parent_native_id"], serde_json::Value::Null);
        assert_eq!(complete["is_sidechain"], serde_json::Value::Null);
        assert_eq!(complete["spans"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn context_graph_store_loads_typed_graph_and_groups_message_candidates_by_session() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"typed-context-session");
        let document = sid(IdKind::Document, b"typed-context-document");
        let message = sid(IdKind::Message, b"typed-context-message");
        let first = placement(&session, &document, &message, 0, false, Some((0, 4)));
        let second = placement(&session, &document, &message, 1, true, Some((5, 9)));
        let source = source_batch(
            "typed-context.jsonl",
            vec![
                typed_message_entry(&message, "typed context body"),
                (
                    session.clone(),
                    session_payload(document.as_str(), &[message.as_str()]),
                    String::new(),
                ),
                typed_document_entry(&document),
            ],
            vec![first.clone(), second.clone()],
            Vec::new(),
            true,
        );
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&source))
            .unwrap();

        let graph = store.load_session_graph(&session).unwrap();
        assert_eq!(graph.session_id, session);
        assert_eq!(graph.messages.len(), 1);
        assert_eq!(graph.messages[0].id, message);
        assert_eq!(graph.source_documents.len(), 1);
        assert_eq!(graph.source_documents[0].id, document);
        assert_eq!(graph.placements.len(), 2);
        assert!(graph.edges.is_empty());
        graph.validate().unwrap();

        let candidates = store.message_contexts(&message).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].session_id, session);
        assert_eq!(
            candidates[0].placement_ids,
            vec![first.id.clone(), second.id.clone()]
        );
        assert_eq!(
            store.context_stats().unwrap(),
            ContextStats {
                placements: 2,
                source_placement_claims: 2,
            }
        );
    }

    #[test]
    fn context_graph_store_keeps_zero_message_session_document_attribution() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"zero-message-session");
        let document = sid(IdKind::Document, b"zero-message-document");
        let source = source_batch(
            "zero-message.jsonl",
            vec![
                (
                    session.clone(),
                    session_payload(document.as_str(), &[]),
                    String::new(),
                ),
                typed_document_entry(&document),
            ],
            Vec::new(),
            Vec::new(),
            true,
        );
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&source))
            .unwrap();

        let graph = store.load_session_graph(&session).unwrap();
        assert!(graph.messages.is_empty());
        assert!(graph.placements.is_empty());
        assert!(graph.edges.is_empty());
        assert_eq!(graph.source_documents.len(), 1);
        assert_eq!(graph.source_documents[0].id, document);
        graph.validate().unwrap();
    }

    #[test]
    fn every_contributing_source_must_be_relation_complete_before_context_reads() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"incomplete-context-session");
        let document_a = sid(IdKind::Document, b"complete-context-document");
        let document_b = sid(IdKind::Document, b"incomplete-context-document");
        let message_a = sid(IdKind::Message, b"complete-context-message");
        let message_b = sid(IdKind::Message, b"incomplete-context-message");
        let placement_a = placement(&session, &document_a, &message_a, 0, false, Some((0, 4)));
        let placement_b = placement(&session, &document_b, &message_b, 0, false, Some((0, 4)));
        let complete_source_path = "private-complete-source.jsonl";
        let incomplete_source_path = "private-incomplete-source.jsonl";
        let complete_source = source_batch(
            complete_source_path,
            vec![
                typed_message_entry(&message_a, "complete context body"),
                (
                    session.clone(),
                    session_payload(document_a.as_str(), &[message_a.as_str()]),
                    String::new(),
                ),
                typed_document_entry(&document_a),
            ],
            vec![placement_a],
            Vec::new(),
            true,
        );
        let incomplete_source = |complete| {
            source_batch(
                incomplete_source_path,
                vec![
                    typed_message_entry(&message_b, "incomplete context body"),
                    (
                        session.clone(),
                        session_payload(document_b.as_str(), &[message_b.as_str()]),
                        String::new(),
                    ),
                    typed_document_entry(&document_b),
                ],
                vec![placement_b.clone()],
                Vec::new(),
                complete,
            )
        };
        store
            .commit_source_batches_if_changed(&[complete_source, incomplete_source(false)])
            .unwrap();
        assert!(relation_complete_marker(&store, complete_source_path));
        assert!(!relation_complete_marker(&store, incomplete_source_path));

        let session_error = store.load_session_graph(&session).unwrap_err();
        assert!(
            matches!(&session_error, PortError::SchemaIncompatible(message) if message.contains("re-ingest required"))
        );
        assert!(!session_error.to_string().contains(complete_source_path));
        assert!(!session_error.to_string().contains(incomplete_source_path));

        let message_error = store.message_contexts(&message_b).unwrap_err();
        assert!(
            matches!(&message_error, PortError::SchemaIncompatible(message) if message.contains("re-ingest required"))
        );
        assert!(!message_error.to_string().contains(incomplete_source_path));

        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&incomplete_source(true)))
                .unwrap()
        );
        assert!(relation_complete_marker(&store, incomplete_source_path));

        let graph = store.load_session_graph(&session).unwrap();
        assert_eq!(graph.placements.len(), 2);
        assert!(
            graph
                .placements
                .iter()
                .any(|placement| placement.id == placement_b.id)
        );
        let contexts = store.message_contexts(&message_b).unwrap();
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].session_id, session);
        assert_eq!(contexts[0].placement_ids, vec![placement_b.id.clone()]);
    }

    #[test]
    fn rebuild_preserves_relations_context_claims_and_completeness() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"rebuild-context-session");
        let document = sid(IdKind::Document, b"rebuild-context-document");
        let parent = sid(IdKind::Message, b"rebuild-context-parent");
        let child = sid(IdKind::Message, b"rebuild-context-child");
        let parent_placement = placement(&session, &document, &parent, 0, false, Some((0, 4)));
        let child_placement = placement(&session, &document, &child, 1, false, Some((5, 9)));
        let source_path = "rebuild-context.jsonl";
        let source = source_batch(
            source_path,
            vec![
                typed_message_entry(&parent, "parent body"),
                typed_message_entry(&child, "child body"),
                (
                    session.clone(),
                    session_payload(document.as_str(), &[parent.as_str(), child.as_str()]),
                    String::new(),
                ),
                typed_document_entry(&document),
            ],
            vec![parent_placement, child_placement.clone()],
            vec![reply_edge(&child_placement, &parent)],
            true,
        );
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&source))
            .unwrap();
        let graph_before = store.load_session_graph(&session).unwrap();
        let placements_before = store.stored_placements().unwrap();
        let edges_before = store.stored_edges().unwrap();
        let claims_before = source_placement_claims(&store, source_path);
        let stats_before = store.context_stats().unwrap();
        assert!(relation_complete_marker(&store, source_path));

        store.rebuild_index().unwrap();

        assert_eq!(store.load_session_graph(&session).unwrap(), graph_before);
        assert_eq!(store.stored_placements().unwrap(), placements_before);
        assert_eq!(store.stored_edges().unwrap(), edges_before);
        assert_eq!(source_placement_claims(&store, source_path), claims_before);
        assert_eq!(store.context_stats().unwrap(), stats_before);
        assert!(relation_complete_marker(&store, source_path));
    }

    #[test]
    fn incomplete_scan_updates_observed_relations_without_tombstoning_unseen_facts() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"incomplete-update-session");
        let document = sid(IdKind::Document, b"incomplete-update-document");
        let parent_a = sid(IdKind::Message, b"incomplete-update-parent-a");
        let parent_b = sid(IdKind::Message, b"incomplete-update-parent-b");
        let observed_message = sid(IdKind::Message, b"incomplete-update-observed");
        let unseen_message = sid(IdKind::Message, b"incomplete-update-unseen");
        let original = placement(
            &session,
            &document,
            &observed_message,
            1,
            false,
            Some((1, 5)),
        );
        let changed = placement(
            &session,
            &document,
            &observed_message,
            1,
            true,
            Some((2, 6)),
        );
        let unseen = placement(
            &session,
            &document,
            &unseen_message,
            2,
            false,
            Some((7, 11)),
        );
        let entries = || {
            [
                &session,
                &document,
                &parent_a,
                &parent_b,
                &observed_message,
                &unseen_message,
            ]
            .into_iter()
            .map(entity_entry)
            .collect()
        };
        let initial = source_batch(
            "incomplete-update.jsonl",
            entries(),
            vec![original.clone(), unseen.clone()],
            vec![
                reply_edge(&original, &parent_a),
                reply_edge(&unseen, &parent_a),
            ],
            true,
        );
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&initial))
            .unwrap();

        let changed_observation = source_batch(
            "incomplete-update.jsonl",
            entries(),
            vec![changed.clone()],
            vec![reply_edge(&changed, &parent_b)],
            false,
        );
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&changed_observation))
                .unwrap()
        );
        assert!(
            store
                .stored_placements()
                .unwrap()
                .get(changed.id.as_str())
                .unwrap()
                .matches(&changed)
        );
        assert!(
            store
                .stored_edges()
                .unwrap()
                .get(changed.id.as_str())
                .unwrap()
                .matches(&reply_edge(&changed, &parent_b))
        );
        assert!(
            store
                .stored_placements()
                .unwrap()
                .contains_key(unseen.id.as_str())
        );
        assert!(
            store
                .stored_edges()
                .unwrap()
                .contains_key(unseen.id.as_str())
        );
        assert!(!relation_complete_marker(&store, "incomplete-update.jsonl"));

        let observed_root = source_batch(
            "incomplete-update.jsonl",
            entries(),
            vec![changed.clone()],
            Vec::new(),
            false,
        );
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&observed_root))
                .unwrap()
        );
        assert!(
            !store
                .stored_edges()
                .unwrap()
                .contains_key(changed.id.as_str())
        );
        assert!(
            store
                .stored_edges()
                .unwrap()
                .contains_key(unseen.id.as_str())
        );
    }

    #[test]
    fn incomplete_scan_unions_claims_and_complete_scan_replaces_with_tombstones() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"replacement-session");
        let document = sid(IdKind::Document, b"replacement-document");
        let parent = sid(IdKind::Message, b"replacement-parent");
        let old_message = sid(IdKind::Message, b"replacement-old");
        let new_message = sid(IdKind::Message, b"replacement-new");
        let old_placement = placement(&session, &document, &old_message, 1, false, Some((1, 5)));
        let new_placement = placement(&session, &document, &new_message, 2, false, Some((6, 10)));

        let initial = source_batch(
            "replacement.jsonl",
            [&session, &document, &parent, &old_message]
                .into_iter()
                .map(entity_entry)
                .collect(),
            vec![old_placement.clone()],
            vec![reply_edge(&old_placement, &parent)],
            true,
        );
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&initial))
            .unwrap();

        let incomplete = source_batch(
            "replacement.jsonl",
            [&session, &document, &parent, &new_message]
                .into_iter()
                .map(entity_entry)
                .collect(),
            vec![new_placement.clone()],
            vec![reply_edge(&new_placement, &parent)],
            false,
        );
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&incomplete))
                .unwrap()
        );
        assert!(store.get(&old_message).unwrap().is_some());
        assert!(store.get(&new_message).unwrap().is_some());
        assert!(
            store
                .stored_placements()
                .unwrap()
                .contains_key(old_placement.id.as_str())
        );
        assert!(
            store
                .stored_edges()
                .unwrap()
                .contains_key(old_placement.id.as_str())
        );
        let expected_claims: Vec<_> = [
            old_placement.id.as_str().to_string(),
            new_placement.id.as_str().to_string(),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
        assert_eq!(
            source_placement_claims(&store, "replacement.jsonl"),
            expected_claims
        );
        assert!(!relation_complete_marker(&store, "replacement.jsonl"));

        let complete = source_batch(
            "replacement.jsonl",
            [&session, &document, &parent, &new_message]
                .into_iter()
                .map(entity_entry)
                .collect(),
            vec![new_placement.clone()],
            vec![reply_edge(&new_placement, &parent)],
            true,
        );
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&complete))
                .unwrap()
        );
        assert!(store.get(&old_message).unwrap().is_none());
        assert!(store.get(&new_message).unwrap().is_some());
        assert!(
            !store
                .stored_placements()
                .unwrap()
                .contains_key(old_placement.id.as_str())
        );
        assert!(
            !store
                .stored_edges()
                .unwrap()
                .contains_key(old_placement.id.as_str())
        );
        assert_eq!(
            source_placement_claims(&store, "replacement.jsonl"),
            vec![new_placement.id.as_str().to_string()]
        );
        assert!(relation_complete_marker(&store, "replacement.jsonl"));
    }

    #[test]
    fn complete_empty_replacement_preserves_shared_facts_then_tombstones_last_claim() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"shared-survival-session");
        let document = sid(IdKind::Document, b"shared-survival-document");
        let parent = sid(IdKind::Message, b"shared-survival-parent");
        let child = sid(IdKind::Message, b"shared-survival-child");
        let child_placement = placement(&session, &document, &child, 1, false, Some((2, 8)));
        let entries = || {
            [&session, &document, &parent, &child]
                .into_iter()
                .map(entity_entry)
                .collect()
        };
        let edge = reply_edge(&child_placement, &parent);
        let sources = [
            source_batch(
                "shared-a.jsonl",
                entries(),
                vec![child_placement.clone()],
                vec![edge.clone()],
                true,
            ),
            source_batch(
                "shared-b.jsonl",
                entries(),
                vec![child_placement.clone()],
                vec![edge],
                true,
            ),
        ];
        store.commit_source_batches_if_changed(&sources).unwrap();

        let empty_a = source_batch("shared-a.jsonl", Vec::new(), Vec::new(), Vec::new(), true);
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&empty_a))
                .unwrap()
        );
        assert!(store.get(&child).unwrap().is_some());
        assert!(
            store
                .stored_placements()
                .unwrap()
                .contains_key(child_placement.id.as_str())
        );
        assert!(
            store
                .stored_edges()
                .unwrap()
                .contains_key(child_placement.id.as_str())
        );
        assert!(source_placement_claims(&store, "shared-a.jsonl").is_empty());
        assert_eq!(
            source_placement_claims(&store, "shared-b.jsonl"),
            vec![child_placement.id.as_str().to_string()]
        );
        let empty_manifest = latest_index_batch(&store).source_replacements.remove(0);
        assert_eq!(empty_manifest["source_path"], "shared-a.jsonl");
        assert_eq!(empty_manifest["entity_memberships"], serde_json::json!([]));
        assert_eq!(empty_manifest["placement_ids"], serde_json::json!([]));
        assert_eq!(empty_manifest["relation_complete"], true);

        let generation = store.active_generation().unwrap();
        assert!(
            !store
                .commit_source_batches_if_changed(std::slice::from_ref(&empty_a))
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), generation);

        let empty_b = source_batch("shared-b.jsonl", Vec::new(), Vec::new(), Vec::new(), true);
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&empty_b))
                .unwrap()
        );
        assert!(store.get(&child).unwrap().is_none());
        assert!(
            !store
                .stored_placements()
                .unwrap()
                .contains_key(child_placement.id.as_str())
        );
        assert!(
            !store
                .stored_edges()
                .unwrap()
                .contains_key(child_placement.id.as_str())
        );
    }

    #[test]
    fn shared_relation_change_requires_all_claimants_in_one_complete_batch() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"shared-change-session");
        let document = sid(IdKind::Document, b"shared-change-document");
        let parent_a = sid(IdKind::Message, b"shared-change-parent-a");
        let parent_b = sid(IdKind::Message, b"shared-change-parent-b");
        let child = sid(IdKind::Message, b"shared-change-child");
        let original = placement(&session, &document, &child, 1, false, Some((10, 20)));
        let changed = placement(&session, &document, &child, 1, true, Some((11, 21)));
        assert_eq!(original.id, changed.id);
        let entries = || {
            [&session, &document, &parent_a, &parent_b, &child]
                .into_iter()
                .map(entity_entry)
                .collect()
        };
        let initial = [
            source_batch(
                "change-a.jsonl",
                entries(),
                vec![original.clone()],
                vec![reply_edge(&original, &parent_a)],
                true,
            ),
            source_batch(
                "change-b.jsonl",
                entries(),
                vec![original.clone()],
                vec![reply_edge(&original, &parent_a)],
                true,
            ),
        ];
        store.commit_source_batches_if_changed(&initial).unwrap();
        let generation = store.active_generation().unwrap();

        let only_a = source_batch(
            "change-a.jsonl",
            entries(),
            vec![changed.clone()],
            vec![reply_edge(&changed, &parent_a)],
            true,
        );
        let err = store
            .commit_source_batches_if_changed(std::slice::from_ref(&only_a))
            .unwrap_err();
        assert!(
            matches!(err, PortError::Backend(message) if message.contains("did not observe the same placement"))
        );
        assert_eq!(store.active_generation().unwrap(), generation);
        assert!(
            store
                .stored_placements()
                .unwrap()
                .get(original.id.as_str())
                .unwrap()
                .matches(&original)
        );

        let changed_both = [
            source_batch(
                "change-a.jsonl",
                entries(),
                vec![changed.clone()],
                vec![reply_edge(&changed, &parent_b)],
                true,
            ),
            source_batch(
                "change-b.jsonl",
                entries(),
                vec![changed.clone()],
                vec![reply_edge(&changed, &parent_b)],
                true,
            ),
        ];
        assert!(
            store
                .commit_source_batches_if_changed(&changed_both)
                .unwrap()
        );
        assert_eq!(store.active_generation().unwrap(), generation + 1);
        assert!(
            store
                .stored_placements()
                .unwrap()
                .get(changed.id.as_str())
                .unwrap()
                .matches(&changed)
        );
        assert!(
            store
                .stored_edges()
                .unwrap()
                .get(changed.id.as_str())
                .unwrap()
                .matches(&reply_edge(&changed, &parent_b))
        );
    }

    #[test]
    fn edge_only_change_requires_every_shared_claimant_to_observe_the_new_edge() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"shared-edge-session");
        let document = sid(IdKind::Document, b"shared-edge-document");
        let parent_a = sid(IdKind::Message, b"shared-edge-parent-a");
        let parent_b = sid(IdKind::Message, b"shared-edge-parent-b");
        let child = sid(IdKind::Message, b"shared-edge-child");
        let child_placement = placement(&session, &document, &child, 1, false, Some((3, 9)));
        let entries = || {
            [&session, &document, &parent_a, &parent_b, &child]
                .into_iter()
                .map(entity_entry)
                .collect()
        };
        let initial = [
            source_batch(
                "shared-edge-a.jsonl",
                entries(),
                vec![child_placement.clone()],
                vec![reply_edge(&child_placement, &parent_a)],
                true,
            ),
            source_batch(
                "shared-edge-b.jsonl",
                entries(),
                vec![child_placement.clone()],
                vec![reply_edge(&child_placement, &parent_a)],
                true,
            ),
        ];
        store.commit_source_batches_if_changed(&initial).unwrap();
        let generation = store.active_generation().unwrap();

        let only_a = source_batch(
            "shared-edge-a.jsonl",
            entries(),
            vec![child_placement.clone()],
            vec![reply_edge(&child_placement, &parent_b)],
            true,
        );
        let err = store
            .commit_source_batches_if_changed(std::slice::from_ref(&only_a))
            .unwrap_err();
        assert!(
            matches!(err, PortError::Backend(message) if message.contains("did not observe the same edge"))
        );
        assert_eq!(store.active_generation().unwrap(), generation);
        assert!(
            store
                .stored_edges()
                .unwrap()
                .get(child_placement.id.as_str())
                .unwrap()
                .matches(&reply_edge(&child_placement, &parent_a))
        );

        let changed_both = [
            source_batch(
                "shared-edge-a.jsonl",
                entries(),
                vec![child_placement.clone()],
                vec![reply_edge(&child_placement, &parent_b)],
                true,
            ),
            source_batch(
                "shared-edge-b.jsonl",
                entries(),
                vec![child_placement.clone()],
                vec![reply_edge(&child_placement, &parent_b)],
                true,
            ),
        ];
        assert!(
            store
                .commit_source_batches_if_changed(&changed_both)
                .unwrap()
        );
        assert!(
            store
                .stored_edges()
                .unwrap()
                .get(child_placement.id.as_str())
                .unwrap()
                .matches(&reply_edge(&child_placement, &parent_b))
        );
    }

    #[test]
    fn relation_apply_failure_rolls_back_everything_except_building_intent() {
        let store = SqliteStore::open_in_memory().unwrap();
        store
            .conn
            .borrow()
            .execute_batch(
                "CREATE TRIGGER fail_relation_insert
                 BEFORE INSERT ON message_placements
                 BEGIN
                     SELECT RAISE(ABORT, 'injected relation failure');
                 END;",
            )
            .unwrap();
        let session = sid(IdKind::Session, b"rollback-session");
        let document = sid(IdKind::Document, b"rollback-document");
        let message = sid(IdKind::Message, b"rollback-message");
        let message_placement = placement(&session, &document, &message, 0, false, Some((0, 4)));
        let source = source_batch(
            "rollback.jsonl",
            [&session, &document, &message]
                .into_iter()
                .map(entity_entry)
                .collect(),
            vec![message_placement],
            Vec::new(),
            true,
        );

        let err = store
            .commit_source_batches_if_changed(std::slice::from_ref(&source))
            .unwrap_err();
        assert!(matches!(err, PortError::Backend(_)));
        assert_eq!(store.active_generation().unwrap(), 0);
        for table in [
            "catalog",
            "fts",
            "fts_ids",
            "source_membership",
            "source_scans",
            "message_placements",
            "message_edges",
            "source_placement_membership",
            "source_relation_scans",
        ] {
            assert_eq!(table_count(&store, table), 0, "{table} must roll back");
        }
        let batch = latest_index_batch(&store);
        assert_eq!(batch.state, "building");
        assert_eq!(batch.durable_point, "intent");
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
    fn fts_rowids_track_rows_across_commit_rebuild_and_delete() {
        // 回归：fts5 的 id 列是内容列不是 rowid；fts_ids.fts_rowid 边车必须与
        // fts 行一一对应，且对批量提交、rebuild、按 wire 别名删除保持一致。
        let store = SqliteStore::open_in_memory().unwrap();
        let messages: Vec<StableId> = (0..50)
            .map(|i| sid(IdKind::Message, format!("rowid-{i}").as_bytes()))
            .collect();
        let ses = sid(IdKind::Session, b"rowid-container");
        let source = SourceBatch {
            source_path: "rowid.jsonl".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
            entries: messages
                .iter()
                .enumerate()
                // payload 需与 text 在 rebuild 重投影下 round-trip：rebuild 从
                // catalog 经 searchable_text 重新提取正文（'\t' 前是 role），
                // 若 payload 不携带正文，rebuild 后 fts 行将不再可搜。
                .map(|(i, id)| {
                    (
                        id.clone(),
                        format!("user\trowid text {i}").into_bytes(),
                        format!("rowid text {i}"),
                    )
                })
                .chain(std::iter::once((ses.clone(), b"s".to_vec(), String::new())))
                .collect(),
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&source))
                .unwrap()
        );
        let conn = store.conn.borrow();
        let mismatch: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts f
                 JOIN fts_ids fi ON fi.id_json = f.id
                 WHERE fi.fts_rowid IS NULL OR fi.fts_rowid != f.rowid",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            mismatch, 0,
            "every fts row must carry its rowid in the sidecar"
        );
        let fts_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fts_rows, 50);
        let session_rid: Option<i64> = conn
            .query_row(
                "SELECT fts_rowid FROM fts_ids WHERE wire_id = ?1",
                [ses.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_rid, None, "non-message sidecar rows stay NULL");
        drop(conn);

        // rebuild 整表清空重投影后映射仍然成立。
        store.rebuild_index().unwrap();
        let conn = store.conn.borrow();
        let mismatch: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts f
                 JOIN fts_ids fi ON fi.id_json = f.id
                 WHERE fi.fts_rowid IS NULL OR fi.fts_rowid != f.rowid",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mismatch, 0, "rebuild must restore the rowid sidecar");
        drop(conn);

        // 按 wire 别名（from_wire 降级为 Unstable）删除仍能定位到 fts 行。
        let victim = messages[17].clone();
        let wire_id = StableId::from_wire(victim.as_str()).unwrap();
        let entries: [(StableId, Vec<u8>, String); 0] = [];
        let pending = store
            .begin_index_batch(&entries, std::slice::from_ref(&wire_id))
            .unwrap();
        store
            .commit_index_batch(&pending, &entries, &[wire_id])
            .unwrap();
        assert!(
            store.query("rowid text 17", 10).unwrap().is_empty(),
            "alias delete must remove the fts row"
        );
        assert_eq!(store.query("rowid text 16", 10).unwrap().len(), 1);
        assert_eq!(store.count().unwrap(), 50);
    }

    #[test]
    fn large_store_delete_is_rowid_scoped_not_content_scanned() {
        // 回归（性能护栏）：10K 消息库上删除单条。旧实现按内容列 id 比较，
        // 每次删除整表扫描 fts（10K 行约 6.3s）；新实现经 fts_ids.fts_rowid
        // 按 rowid 定位（µs 级）。3s 宽限只拦内容扫描回归，对 rowid 路径有
        // 数个数量级的余量，不依赖计时精度。
        let store = SqliteStore::open_in_memory().unwrap();
        let messages: Vec<StableId> = (0..10_000)
            .map(|i| sid(IdKind::Message, format!("bulk-{i}").as_bytes()))
            .collect();
        let source = SourceBatch {
            source_path: "bulk.jsonl".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
            entries: messages
                .iter()
                .enumerate()
                .map(|(i, id)| (id.clone(), b"m".to_vec(), format!("bulk text {i}")))
                .collect(),
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&source))
                .unwrap()
        );
        assert_eq!(store.query("bulk text 5000", 10).unwrap().len(), 1);
        let victim = messages[5000].clone();
        let entries: [(StableId, Vec<u8>, String); 0] = [];
        let pending = store
            .begin_index_batch(&entries, std::slice::from_ref(&victim))
            .unwrap();
        let started = std::time::Instant::now();
        store
            .commit_index_batch(&pending, &entries, &[victim])
            .unwrap();
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "delete must not scan the fts table: {elapsed:?}"
        );
        assert!(store.query("bulk text 5000", 10).unwrap().is_empty());
        assert_eq!(store.query("bulk text 4999", 10).unwrap().len(), 1);
    }

    #[test]
    fn v7_open_backfills_fts_rowid_for_legacy_rows() {
        // 旧 v7 库（fts_rowid 列加入前建成）首次打开必须回填边车：fts 行按
        // id_json 与 fts_ids 一一对应，回填后按 wire 别名删除才能按 rowid 定位。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy-fts.db");
        let p = path.to_string_lossy().into_owned();
        let legacy = sid(IdKind::Message, b"legacy-fts-row");
        let legacy_json = serde_json::to_string(&legacy).unwrap();
        {
            let conn = rusqlite::Connection::open(&p).unwrap();
            create_v6_schema(&conn);
            // 旧式 fts/fts_ids：fts 行由 fts5 自动分配 rowid，边车没有 rowid
            // 概念；随后 open 会走 v6→v7 + 本列一次性回填。
            conn.execute(
                "INSERT INTO catalog(id, payload) VALUES(?1, ?2)",
                rusqlite::params![legacy.as_str(), b"legacy".to_vec()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO fts(id, text) VALUES(?1, ?2)",
                rusqlite::params![legacy_json, "legacy fts body"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO fts_ids(wire_id, id_json) VALUES(?1, ?2)",
                rusqlite::params![legacy.as_str(), legacy_json],
            )
            .unwrap();
            // 容器实体：无 fts 行，回填后 fts_rowid 必须保持 NULL。
            conn.execute(
                "INSERT INTO fts_ids(wire_id, id_json) VALUES(?1, ?2)",
                rusqlite::params![
                    "legacy-session",
                    serde_json::to_string(&sid(IdKind::Session, b"legacy-session")).unwrap()
                ],
            )
            .unwrap();
        }
        let store = SqliteStore::open(&p).unwrap();
        let conn = store.conn.borrow();
        let mismatch: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts f
                 JOIN fts_ids fi ON fi.id_json = f.id
                 WHERE fi.fts_rowid IS NULL OR fi.fts_rowid != f.rowid",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mismatch, 0, "legacy fts rows must be backfilled on open");
        let session_rid: Option<i64> = conn
            .query_row(
                "SELECT fts_rowid FROM fts_ids WHERE wire_id = 'legacy-session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_rid, None);
        drop(conn);

        // 回填后按 wire 别名删除能定位到旧 fts 行（无 fts 残留）。
        let wire_id = StableId::from_wire(legacy.as_str()).unwrap();
        let entries: [(StableId, Vec<u8>, String); 0] = [];
        let pending = store
            .begin_index_batch(&entries, std::slice::from_ref(&wire_id))
            .unwrap();
        store
            .commit_index_batch(&pending, &entries, &[wire_id])
            .unwrap();
        assert!(store.query("legacy fts body", 10).unwrap().is_empty());
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn source_rescan_tombstones_removed_messages() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"source-a");
        let b = sid(IdKind::Message, b"source-b");
        let first = SourceBatch {
            source_path: "fixture.jsonl".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![
                    (m1.clone(), b"m1".to_vec(), "one text".into()),
                    (shared.clone(), b"d".to_vec(), String::new()),
                ],
            },
            SourceBatch {
                source_path: "two.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
        let native_id = "private-provider-native-id";
        let msg = StableId::native(IdKind::Message, native_id);
        let sources = [
            SourceBatch {
                source_path: "conflict-a.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(msg.clone(), b"first projection".to_vec(), "one".into())],
            },
            SourceBatch {
                source_path: "conflict-b.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
        assert!(!format!("{error}").contains(native_id), "{error}");
        assert!(!format!("{error}").contains(msg.as_str()), "{error}");
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
    fn codex_timestamp_occurrence_projections_merge_without_conflict() {
        // Codex's old adapter stored the occurrence-local envelope timestamp
        // as the message timestamp; the current adapter emits no stable
        // timestamp. Re-ingesting an old catalog must therefore merge a
        // string timestamp with null instead of reporting a conflict.
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = StableId::native(IdKind::Message, "codex-msg-timestamp");
        let old = serde_json::json!({
            "role": "assistant",
            "text": "same body",
            "timestamp": "2026-07-19T23:40:01.000Z",
        })
        .to_string()
        .into_bytes();
        let new = serde_json::json!({
            "role": "assistant",
            "text": "same body",
            "timestamp": null,
        })
        .to_string()
        .into_bytes();
        let sources = [
            SourceBatch {
                source_path: "old-codex.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(msg.clone(), old, "one".into())],
            },
            SourceBatch {
                source_path: "new-codex.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(msg.clone(), new, "two".into())],
            },
        ];
        store
            .commit_source_batches_if_changed(&sources)
            .expect("occurrence timestamp string/null projections must merge");
        let stored = store.get(&msg).unwrap().unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&stored).unwrap();
        assert_eq!(
            payload.get("timestamp"),
            Some(&serde_json::Value::Null),
            "merged message must carry no stable timestamp"
        );
        assert_eq!(payload.get("text"), Some(&serde_json::json!("same body")));
    }

    #[test]
    fn codex_timestamp_merge_converges_to_null_regardless_of_side() {
        // The string/null convergence must not depend on which projection is
        // the left (merge) side.
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = StableId::native(IdKind::Message, "codex-msg-timestamp-side");
        let string_payload = serde_json::json!({
            "role": "assistant",
            "text": "same body",
            "timestamp": "2026-07-19T23:40:01.000Z",
        })
        .to_string()
        .into_bytes();
        let null_payload = serde_json::json!({
            "role": "assistant",
            "text": "same body",
            "timestamp": null,
        })
        .to_string()
        .into_bytes();
        let sources = [
            SourceBatch {
                source_path: "null-first.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(msg.clone(), null_payload, "one".into())],
            },
            SourceBatch {
                source_path: "string-second.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(msg.clone(), string_payload, "two".into())],
            },
        ];
        store
            .commit_source_batches_if_changed(&sources)
            .expect("null-first/string-second must merge");
        let stored = store.get(&msg).unwrap().unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&stored).unwrap();
        assert_eq!(
            payload.get("timestamp"),
            Some(&serde_json::Value::Null),
            "timestamp must converge to null in either order"
        );
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(
                    msg.clone(),
                    message_payload_with_span("ses_v1_aaa", "same body", "doc_v1_aaa", 0, 929),
                    "same body".into(),
                )],
            },
            SourceBatch {
                source_path: "resumed.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(
                    msg.clone(),
                    message_payload("ses_v1_aaa", "shared body"),
                    "shared body".into(),
                )],
            },
            SourceBatch {
                source_path: "second.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
    fn message_with_different_text_merges_to_longer_projection() {
        // text is a content projection, not a stable identity field: Claude
        // Code copies a conversation's history into a new transcript on resume
        // or fork, and a copy may carry a different number of content blocks
        // (e.g. a truncated tool_result). Diverging text under one id must not
        // conflict; the merged projection deterministically keeps the longer
        // body so no retrieved content is lost.
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"divergent-msg");
        let sources = [
            SourceBatch {
                source_path: "first.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(
                    msg.clone(),
                    message_payload("ses_v1_aaa", "original body"),
                    "original body".into(),
                )],
            },
            SourceBatch {
                source_path: "second.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(
                    msg.clone(),
                    message_payload("ses_v1_aaa", "a much longer rewritten body"),
                    "a much longer rewritten body".into(),
                )],
            },
        ];
        store
            .commit_source_batches_if_changed(&sources)
            .expect("diverging text is exempt from conflict and must merge");
        // The longer projection wins deterministically.
        let stored = store
            .get(&msg)
            .expect("message must be stored")
            .expect("message must be present");
        let stored_payload: serde_json::Value =
            serde_json::from_slice::<serde_json::Value>(&stored)
                .expect("stored payload must parse");
        assert_eq!(
            stored_payload["text"], "a much longer rewritten body",
            "merged text must keep the longer projection"
        );
    }

    #[test]
    fn merged_message_fts_projects_from_merged_payload() {
        // 回归（Major-1）：合并 payload 后，FTS 正文必须从合并后 payload 经
        // searchable_text 重投影（与 rebuild_index 同一投影函数），而不是取
        // “source_path 排序最后处理的源”的原始 text。长文本在排序靠前的源、
        // 短文本在排序靠后的源时，旧实现把长文本写进 catalog payload 却把短
        // 文本写进 fts——长文本搜不到（需 rebuild 才恢复），且按源分批重同步
        // 会交替改写 fts、每批推进 generation、内容级 no-op 失效。
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"merged-fts-msg");
        let sources = [
            SourceBatch {
                source_path: "aaa-long.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(
                    msg.clone(),
                    message_payload("ses_v1_aaa", "the long body that must stay searchable"),
                    "the long body that must stay searchable".into(),
                )],
            },
            SourceBatch {
                source_path: "zzz-short.jsonl".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(
                    msg.clone(),
                    message_payload("ses_v1_aaa", "short body"),
                    "short body".into(),
                )],
            },
        ];
        assert!(store.commit_source_batches_if_changed(&sources).unwrap());
        // 长文本必须可搜——它是合并 payload 的权威投影。
        assert_eq!(
            store.query("long body", 10).unwrap().len(),
            1,
            "merged payload text must be searchable"
        );
        assert!(
            store.query("short", 10).unwrap().is_empty(),
            "short projection must not shadow the merged text"
        );
        // fts 行文本 == searchable_text(合并后 payload)。
        let conn = store.conn.borrow();
        let fts_text: String = conn
            .query_row(
                "SELECT f.text FROM fts f
                 JOIN fts_ids fi ON fi.id_json = f.id
                 WHERE fi.wire_id = ?1",
                [msg.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        drop(conn);
        assert_eq!(
            fts_text, "the long body that must stay searchable",
            "fts must project from the merged payload"
        );
        // 内容级 no-op 恢复：同一语料重同步不推进 generation。
        let generation = store.active_generation().unwrap();
        assert!(!store.commit_source_batches_if_changed(&sources).unwrap());
        assert_eq!(store.active_generation().unwrap(), generation);
    }

    #[test]
    fn message_payload_only_change_is_not_dropped() {
        // 回归（Minor-2）：contextual payload 判定必须比对字节（经与提交路径相同
        // 的合并），而不是只查 catalog 行存在。text 不变、仅 payload 变化（新增
        // session 引用）时，旧实现把 batch 判为 current，payload-only 变更被静默
        // 丢弃，搜索与上下文永远缺该 session。
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"payload-only-msg");
        let first = SourceBatch {
            source_path: "payload-only.jsonl".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
            entries: vec![(
                msg.clone(),
                message_payload("ses_v1_aaa", "stable body"),
                "stable body".into(),
            )],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&first))
                .unwrap()
        );

        // text 不变、payload 换 session（同一源路径，membership/relations 均不变）。
        let changed = SourceBatch {
            source_path: "payload-only.jsonl".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
            entries: vec![(
                msg.clone(),
                message_payload("ses_v1_bbb", "stable body"),
                "stable body".into(),
            )],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&changed))
                .unwrap(),
            "payload-only change must not be silently dropped"
        );
        let stored: serde_json::Value =
            serde_json::from_slice(&store.get(&msg).unwrap().unwrap()).unwrap();
        assert_eq!(
            stored["sessions"],
            serde_json::json!(["ses_v1_aaa", "ses_v1_bbb"]),
            "merged payload must accumulate the new session ref"
        );
        // 已收敛：内容与存储一致后重同步是 no-op。
        assert!(
            !store
                .commit_source_batches_if_changed(std::slice::from_ref(&changed))
                .unwrap()
        );
    }

    #[test]
    fn changed_source_fingerprint_forces_reparse_and_converges() {
        // 回归（Minor-3）：source_scans 的 len/fingerprint 指纹参与 current 判定
        // （cheap 与 B2 no-op 两条路径都查）。内容等长替换后，缓存指纹不匹配的源
        // 必须重解析并重写缓存；只查扫描行存在会让指纹缓存永不收敛，CLI 每次运行
        // 都重解析全部源。
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"fingerprint-msg");
        let source = |fingerprint: &str| SourceBatch {
            source_path: "fingerprint.jsonl".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: Some(10),
            fingerprint: Some(fingerprint.to_string()),
            entries: vec![(a.clone(), b"payload".to_vec(), "same text".into())],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&source("aaa")))
                .unwrap()
        );
        // 等长异容替换：len 相同、fingerprint 不同 → 不得判为 current。
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&source("bbb")))
                .unwrap(),
            "fingerprint change must force a re-scan commit"
        );
        // 指纹已重写为 bbb → 再次重同步是 no-op，缓存收敛。
        assert!(
            !store
                .commit_source_batches_if_changed(std::slice::from_ref(&source("bbb")))
                .unwrap()
        );
        let fingerprints = store
            .source_fingerprints(&["fingerprint.jsonl".to_string()])
            .unwrap();
        assert_eq!(
            fingerprints.get("fingerprint.jsonl"),
            Some(&(Some(10), Some("bbb".to_string())))
        );
    }

    #[test]
    fn open_for_write_accepts_bare_relative_filename() {
        // 回归（Minor-4）：裸相对文件名（"catalog.db"）的 parent() 是空串 "",
        // create_dir_all("") 会报错；空 parent 应按当前工作目录处理。
        let dir = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = SqliteStore::open_for_write("catalog.db");
        std::env::set_current_dir(original).unwrap();
        let store = result.unwrap();
        assert_eq!(store.count().unwrap(), 0);
        assert!(
            dir.path().join("writer.lock").exists(),
            "writer lease must be created next to the bare filename"
        );
        assert!(dir.path().join("catalog.db").exists());
    }

    #[test]
    fn standalone_index_keeps_non_message_out_of_fts() {
        // 回归（Minor-5）：单条 SearchIndex::index 与批量路径一样只让 Message
        // 实体进入 fts 全文表——session/document 是容器实体，索引其正文会让搜索
        // 命中重复计数。
        let store = SqliteStore::open_in_memory().unwrap();
        let ses = sid(IdKind::Session, b"index-container");
        store.index(&ses, "container body").unwrap();
        assert!(
            store.query("container", 10).unwrap().is_empty(),
            "non-message entities must not enter the fts table"
        );
        let conn = store.conn.borrow();
        let fts_rowid: Option<i64> = conn
            .query_row(
                "SELECT fts_rowid FROM fts_ids WHERE wire_id = ?1",
                [ses.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        drop(conn);
        assert_eq!(fts_rowid, None, "non-message sidecar row stays NULL");
        // Message 走同一路径仍正常进 fts。
        let msg = sid(IdKind::Message, b"index-message");
        store.index(&msg, "searchable body").unwrap();
        assert_eq!(store.query("searchable", 10).unwrap().len(), 1);
    }

    #[test]
    fn fresh_schema_carries_fts_rowid_without_open_time_ddl() {
        // 回归（Minor-6）：fts_rowid 列直接进 v3 建表 DDL，新库首次 open（含只读）
        // 不再执行 ALTER+回填事务；旧 v7 库仍走 ensure_fts_ids_rowid 的一次性回填。
        let store = SqliteStore::open_in_memory().unwrap();
        let conn = store.conn.borrow();
        let has_fts_rowid: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('fts_ids') WHERE name = 'fts_rowid'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(conn);
        assert_eq!(has_fts_rowid, 1);
    }

    #[test]
    fn batch_current_check_is_rowid_scoped_not_content_scanned() {
        // 回归（性能护栏，Minor-8）：batch_is_current 逐条读 fts text 时，旧实现
        // 按内容列 id 比较整表扫描（每条 O(全库)，B1 批量 no-op 判定 O(N²)）；改经
        // fts_ids.fts_rowid 按 rowid 定位后 O(N)。10K 库上内容扫描路径远超 3s 宽限
        // （单次整表扫描约 6.3s），rowid 路径有数量级余量。
        let store = SqliteStore::open_in_memory().unwrap();
        let entries: Vec<(StableId, Vec<u8>, String)> = (0..10_000)
            .map(|i| {
                let id = sid(IdKind::Message, format!("current-{i}").as_bytes());
                (
                    id,
                    format!("payload {i}").into_bytes(),
                    format!("current text {i}"),
                )
            })
            .collect();
        assert!(store.commit_batch_if_changed(&entries).unwrap());
        let started = std::time::Instant::now();
        assert!(!store.commit_batch_if_changed(&entries).unwrap());
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "current check must not scan the fts table per message: {elapsed:?}"
        );
    }

    #[test]
    fn b1_delete_rejects_catalog_entity_still_referenced_by_relations() {
        // 回归（Minor-10）：B1 路径（裸 commit_index_batch）删 catalog 实体而 v7
        // 关系行仍引用它时，必须拒绝整批，而不是留下事后才发现的悬空引用。
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"dangling-session");
        let document = sid(IdKind::Document, b"dangling-document");
        let message = sid(IdKind::Message, b"dangling-message");
        let p = placement(&session, &document, &message, 0, false, Some((1, 5)));
        let source = source_batch(
            "dangling.jsonl",
            vec![
                (message.clone(), b"m".to_vec(), "dangling text".into()),
                (session.clone(), b"s".to_vec(), String::new()),
                (document.clone(), b"d".to_vec(), String::new()),
            ],
            vec![p],
            Vec::new(),
            true,
        );
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&source))
                .unwrap()
        );

        // 裸 B1 删除被 placement 引用的 message 实体 → 拒绝。
        let pending = store
            .begin_index_batch(&[], std::slice::from_ref(&message))
            .unwrap();
        let error = store
            .commit_index_batch(&pending, &[], &[message])
            .expect_err("deleting a referenced catalog entity must fail");
        assert!(format!("{error}").contains("still referenced"), "{error}");
        // 事务回滚：实体仍在，generation 未推进。
        assert!(
            store
                .get(&sid(IdKind::Message, b"dangling-message"))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn put_maintains_fts_projection() {
        // 回归（Minor-11）：put 更新 payload 后必须同步维护 fts/边车——旧文本不可
        // 再搜、新 payload 的 text 可搜（与 rebuild 同一 searchable_text 投影），
        // 且按 wire 删除仍能经边车定位新 fts 行。
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"put-fts-msg");
        let source = SourceBatch {
            source_path: "put.jsonl".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
            entries: vec![(
                msg.clone(),
                message_payload("ses_v1_aaa", "old body"),
                "old body".into(),
            )],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&source))
                .unwrap()
        );
        assert_eq!(store.query("old", 10).unwrap().len(), 1);
        store
            .put(&msg, &message_payload("ses_v1_aaa", "brand new body"))
            .unwrap();
        assert!(
            store.query("old", 10).unwrap().is_empty(),
            "put must retire the old indexed text"
        );
        assert_eq!(store.query("brand", 10).unwrap().len(), 1);
        let entries: [(StableId, Vec<u8>, String); 0] = [];
        let pending = store
            .begin_index_batch(&entries, std::slice::from_ref(&msg))
            .unwrap();
        store
            .commit_index_batch(&pending, &entries, std::slice::from_ref(&msg))
            .unwrap();
        assert!(store.query("brand", 10).unwrap().is_empty());
    }

    #[test]
    fn timestamp_missing_key_is_symmetric_with_explicit_null() {
        // 回归（Minor-7）：timestamp 缺失键与显式 null 对称收敛——一侧带字符串
        // 时间戳、另一侧缺失该键时，合并结果必须收敛为 null（与 string/null 相同），
        // 而不是保留字符串；两侧都缺失时不引入 timestamp 键。
        let string_payload = serde_json::json!({
            "role": "assistant",
            "text": "same body",
            "timestamp": "2026-07-19T23:40:01.000Z",
        })
        .to_string()
        .into_bytes();
        let missing_payload = serde_json::json!({
            "role": "assistant",
            "text": "same body",
        })
        .to_string()
        .into_bytes();
        let merged = merge_message_payloads("wire", &string_payload, &missing_payload).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&merged).unwrap();
        assert_eq!(value.get("timestamp"), Some(&serde_json::Value::Null));
        // 方向对称。
        let merged = merge_message_payloads("wire", &missing_payload, &string_payload).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&merged).unwrap();
        assert_eq!(value.get("timestamp"), Some(&serde_json::Value::Null));
        // 两侧都缺失：不引入 timestamp 键。
        let merged = merge_message_payloads("wire", &missing_payload, &missing_payload).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&merged).unwrap();
        assert!(value.get("timestamp").is_none());
    }

    #[test]
    fn message_session_refs_accumulate_across_separate_batches() {
        // The same message arriving in a later batch must add its session
        // without dropping the ones already recorded.
        let store = SqliteStore::open_in_memory().unwrap();
        let msg = sid(IdKind::Message, b"batched-msg");
        let first = SourceBatch {
            source_path: "first.jsonl".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(shared.clone(), b"p".to_vec(), "shared text".into())],
            },
            SourceBatch {
                source_path: "two".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(shared.clone(), b"p".to_vec(), "shared text".into())],
            },
        ];
        assert!(store.commit_source_batches_if_changed(&first).unwrap());
        let second = [
            SourceBatch {
                source_path: "one".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: Vec::new(),
            },
            SourceBatch {
                source_path: "two".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
            entries: vec![(moved.clone(), b"p".to_vec(), "moved text".into())],
        };
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&original))
            .unwrap();

        let moved_batches = [
            SourceBatch {
                source_path: "source-a".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: Vec::new(),
            },
            SourceBatch {
                source_path: "source-b".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
                    placements: Vec::new(),
                    edges: Vec::new(),
                    relation_complete: true,
                    len_bytes: None,
                    fingerprint: None,
                    entries: vec![
                        (moved.clone(), b"m".to_vec(), "moved text".into()),
                        (removed.clone(), b"r".to_vec(), "removed text".into()),
                    ],
                },
                SourceBatch {
                    source_path: "source-c".into(),
                    placements: Vec::new(),
                    edges: Vec::new(),
                    relation_complete: true,
                    len_bytes: None,
                    fingerprint: None,
                    entries: vec![(kept.clone(), b"k".to_vec(), "kept text".into())],
                },
            ];
            store.commit_source_batches_if_changed(&initial).unwrap();

            let mut replacement = [
                Some(SourceBatch {
                    source_path: "source-a".into(),
                    placements: Vec::new(),
                    edges: Vec::new(),
                    relation_complete: true,
                    len_bytes: None,
                    fingerprint: None,
                    entries: Vec::new(),
                }),
                Some(SourceBatch {
                    source_path: "source-b".into(),
                    placements: Vec::new(),
                    edges: Vec::new(),
                    relation_complete: true,
                    len_bytes: None,
                    fingerprint: None,
                    entries: vec![(moved, b"m".to_vec(), "moved text".into())],
                }),
                Some(SourceBatch {
                    source_path: "source-c".into(),
                    placements: Vec::new(),
                    edges: Vec::new(),
                    relation_complete: true,
                    len_bytes: None,
                    fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
            entries: vec![(id.clone(), b"p".to_vec(), "will disappear".into())],
        };
        store
            .commit_source_batches_if_changed(std::slice::from_ref(&populated))
            .unwrap();
        let empty = SourceBatch {
            source_path: "empty-later".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: Vec::new(),
            },
            SourceBatch {
                source_path: "same".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
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
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
                entries: vec![(reconstructed, b"p".to_vec(), "same text".into())],
            },
            SourceBatch {
                source_path: "two".into(),
                placements: Vec::new(),
                edges: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
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
    fn separate_batches_reject_conflicting_identity_metadata_without_state_change() {
        let store = SqliteStore::open_in_memory().unwrap();
        let reconstructed = sid(IdKind::Message, b"stored-wire-identity");
        let wire = reconstructed.as_str().to_string();
        let unstable = StableId::from_wire(&wire).unwrap();
        let first = SourceBatch {
            source_path: "first-source".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
            entries: vec![(reconstructed, b"p".to_vec(), "same text".into())],
        };
        assert!(
            store
                .commit_source_batches_if_changed(std::slice::from_ref(&first))
                .unwrap()
        );
        let generation = store.active_generation().unwrap();

        let second = SourceBatch {
            source_path: "second-source".into(),
            placements: Vec::new(),
            edges: Vec::new(),
            relation_complete: true,
            len_bytes: None,
            fingerprint: None,
            entries: vec![(unstable, b"p".to_vec(), "same text".into())],
        };
        let err = store
            .commit_source_batches_if_changed(std::slice::from_ref(&second))
            .unwrap_err();
        let PortError::Backend(message) = err else {
            panic!("expected backend error");
        };
        assert!(message.contains("conflicting identity metadata"));
        assert!(!message.contains(&wire));
        assert_eq!(store.active_generation().unwrap(), generation);
        assert!(
            store
                .source_message_ids("second-source")
                .unwrap()
                .is_empty()
        );
        let hits = store.query("same", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.stability(), Stability::Reconstructed);
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
    fn commit_rejects_tampered_durable_relation_manifest() {
        let store = SqliteStore::open_in_memory().unwrap();
        let session = sid(IdKind::Session, b"manifest-session");
        let document = sid(IdKind::Document, b"manifest-document");
        let message = sid(IdKind::Message, b"manifest-message");
        let message_placement = placement(&session, &document, &message, 0, false, Some((0, 4)));
        let relations = RelationManifests {
            relation_upserts: vec![RelationUpsertManifest::Placement(message_placement)],
            ..RelationManifests::default()
        };
        let pending = store
            .begin_index_batch_with_relations(&[], &[], &relations)
            .unwrap();
        store
            .conn
            .borrow()
            .execute(
                "UPDATE index_batches SET relation_upserts_json = '[]'
                 WHERE operation_id = ?1",
                [&pending.operation_id],
            )
            .unwrap();

        let err = store
            .commit_index_batch_with_relations(&pending, &[], &[], &relations)
            .unwrap_err();
        assert!(
            matches!(err, PortError::Backend(message) if message.contains("does not match durable intent"))
        );
        assert_eq!(store.active_generation().unwrap(), 0);
        assert_eq!(table_count(&store, "message_placements"), 0);
        let batch = store.index_batch(&pending.operation_id).unwrap().unwrap();
        assert_eq!(batch.state, "building");
        assert_eq!(batch.durable_point, "intent");
    }

    #[test]
    fn commit_rejects_tampered_durable_source_replacement_manifest() {
        let store = SqliteStore::open_in_memory().unwrap();
        let relations = RelationManifests {
            source_replacements: vec![SourceReplacementManifest {
                source_path: "manifest-source.jsonl".into(),
                entity_memberships: Vec::new(),
                placement_ids: Vec::new(),
                relation_complete: true,
                len_bytes: None,
                fingerprint: None,
            }],
            ..RelationManifests::default()
        };
        let pending = store
            .begin_index_batch_with_relations(&[], &[], &relations)
            .unwrap();
        store
            .conn
            .borrow()
            .execute(
                "UPDATE index_batches SET source_replacements_json = '[]'
                 WHERE operation_id = ?1",
                [&pending.operation_id],
            )
            .unwrap();

        let err = store
            .commit_index_batch_with_relations(&pending, &[], &[], &relations)
            .unwrap_err();
        assert!(
            matches!(err, PortError::Backend(message) if message.contains("does not match durable intent"))
        );
        assert_eq!(store.active_generation().unwrap(), 0);
        assert_eq!(table_count(&store, "source_scans"), 0);
        assert_eq!(table_count(&store, "source_relation_scans"), 0);
        let batch = store.index_batch(&pending.operation_id).unwrap().unwrap();
        assert_eq!(batch.state, "building");
        assert_eq!(batch.durable_point, "intent");
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
    fn rebuild_indexes_json_payload_text_not_structural_tokens() {
        // H1 regression: ingest/sync writes full JSON payloads. rebuild must
        // index only the `text` field — indexing raw JSON would let
        // structural tokens (`user`, `null`, `sessions`) match every message.
        let store = SqliteStore::open_in_memory().unwrap();
        let a = sid(IdKind::Message, b"json-message-a");
        let payload = serde_json::json!({
            "role": "user",
            "text": "the real searchable body",
            "parent": null,
            "session": "ses-1",
            "sessions": ["ses-1"],
            "timestamp": null,
        })
        .to_string()
        .into_bytes();
        store
            .commit_batch(&[(a.clone(), payload, "one".into())])
            .unwrap();
        // commit 实时路径用调用方传入的 text（此处为 "one"）。
        assert_eq!(store.query("one", 10).unwrap().len(), 1);
        // rebuild 从 catalog 重投影：只索引 JSON 的 text 字段，结构 token 不命中。
        store.rebuild_index().unwrap();
        assert_eq!(store.query("searchable", 10).unwrap().len(), 1);
        assert!(store.query("null", 10).unwrap().is_empty());
        assert!(store.query("sessions", 10).unwrap().is_empty());
        // `ses-1` 里的 `-` 会被 FTS5 当成 NOT 运算符（查询报错而非匹配），
        // 因此用单 token `ses` 断言会话值不被索引。
        assert!(store.query("ses", 10).unwrap().is_empty());
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
