//! Testkit：跨 crate 复用的测试构件，包括 fixture builder、只读断言和 fake Provider。
//!
//! 本 crate 只被其他 crate 的 `[dev-dependencies]` 依赖，绝不进入生产依赖图。
//! 提供四类构件：
//! 1. [`SessionBuilder`] —— 合成合法的 Canonical 会话，省去每个测试手搓样板；
//! 2. [`InMemoryStore`] —— 同时实现 `CatalogStore` + `SearchIndex` 的内存后端；
//! 3. [`FakeProvider`] —— 可配置的 `ProviderAdapter`，用于脱离真实格式测 ingestion；
//! 4. [`assert_read_only`] —— 只读断言：包裹一段操作，断言目标字节未被改写。
//!
//! 所有合成数据一律用非真实内容，符合 R0 fixture 脱敏规范。

use std::cell::RefCell;
use std::collections::HashMap;

use agent_session_grep_domain::{
    DomainResult, IdKind, Message, MessageEdge, MessagePlacement, MessageRelation, Role,
    SessionContextGraph, SourceDocument, Stability, StableId,
};
use agent_session_grep_ports::{
    CanonicalEventSink, CatalogEntry, CatalogStore, Confidence, ContextGraphStore, ContextStats,
    MessageContextCandidate, MessageEvent, ParseReport, PortError, PortResult, ProbeResult,
    ProviderAdapter, ProviderError, SearchHit, SearchIndex,
};

/// 构造合法 Canonical 会话的 builder（fixture builder）。
///
/// 默认产出一个带若干 `User`/`Assistant` 交替消息的会话，序号从 0 连续，
/// 通过 [`Session::validate`]。用 `fact` 决定派生 id 的种子，确保可复现。
pub struct SessionBuilder {
    fact: String,
    texts: Vec<String>,
}

impl SessionBuilder {
    /// 以一个稳定 fact 起手（同 fact 产出同 id，便于断言）。
    pub fn new(fact: &str) -> Self {
        Self {
            fact: fact.to_string(),
            texts: Vec::new(),
        }
    }

    /// 追加一条消息正文（角色按 User/Assistant 交替，从 User 起）。
    pub fn message(mut self, text: &str) -> Self {
        self.texts.push(text.to_string());
        self
    }

    /// 追加多条消息正文。
    pub fn messages<'a>(mut self, texts: impl IntoIterator<Item = &'a str>) -> Self {
        for t in texts {
            self.texts.push(t.to_string());
        }
        self
    }

    /// 产出一个线性、placement-aware 的会话图。保证通过领域不变量校验。
    pub fn build(self) -> SessionContextGraph {
        let document_id = StableId::derive(
            IdKind::Document,
            Stability::Reconstructed,
            &[self.fact.as_bytes()],
        );
        let id = StableId::derive(
            IdKind::Session,
            Stability::Reconstructed,
            &[self.fact.as_bytes()],
        );
        let mid = |seq: u32| {
            StableId::derive(
                IdKind::Message,
                Stability::Reconstructed,
                &[self.fact.as_bytes(), &seq.to_le_bytes()],
            )
        };
        let messages: Vec<Message> = self
            .texts
            .iter()
            .enumerate()
            .map(|(i, text)| {
                let role = if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                };
                Message {
                    id: mid(i as u32),
                    role,
                    text: text.clone(),
                    timestamp: None,
                }
            })
            .collect();
        let placements: Vec<MessagePlacement> = messages
            .iter()
            .enumerate()
            .map(|(i, message)| {
                MessagePlacement::new(
                    id.clone(),
                    document_id.clone(),
                    message.id.clone(),
                    i as u32,
                    false,
                    None,
                )
            })
            .collect();
        let edges = placements
            .iter()
            .skip(1)
            .zip(messages.iter())
            .map(|(child, parent)| MessageEdge {
                child_placement_id: child.id.clone(),
                parent_message_id: parent.id.clone(),
                parent_native_id: None,
                relation: MessageRelation::Reply,
            })
            .collect();
        let graph = SessionContextGraph {
            session_id: id,
            messages,
            source_documents: vec![SourceDocument {
                id: document_id,
                provider_id: "test-provider".into(),
                variant_id: "test-provider/synthetic-v1".into(),
                // fingerprint 与 len 描述同一字节串（fact 即合成快照）——自洽。
                fingerprint: blake3_hex(self.fact.as_bytes()),
                len: self.fact.len() as u64,
            }],
            placements,
            edges,
        };
        graph
            .validate()
            .expect("SessionBuilder must produce a valid graph");
        graph
    }
}

/// 内存后端：同时实现 `CatalogStore` 与 `SearchIndex`，供 application 层快速测试。
///
/// 检索是最朴素的子串匹配——不追求相关性排序保真，只用于验证用例接线正确。
/// 真正的相关性由 SQLite/FTS5 adapter 的测试覆盖。
#[derive(Default)]
pub struct InMemoryStore {
    catalog: RefCell<HashMap<String, Vec<u8>>>,
    index: RefCell<Vec<(StableId, String)>>,
    graphs: RefCell<HashMap<String, SessionContextGraph>>,
    /// 已提交批次数：每次 `insert_graph`（一个 committed 批次）推进，
    /// 与 SqliteStore 的 `active_generation` 语义对齐（0 = 尚未激活任何批次）。
    generation: RefCell<u64>,
    /// 独立的 source-placement-membership 计数（对应 SQLite
    /// `source_placement_membership` 表）：`insert_graph` 时按该批 placements
    /// 数写入，绝不从 placements 现算。
    claims: RefCell<u64>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_graph(&self, graph: SessionContextGraph) -> DomainResult<()> {
        graph.validate()?;
        let claims = graph.placements.len() as u64;
        self.graphs
            .borrow_mut()
            .insert(graph.session_id.as_str().to_string(), graph);
        // 一个批次提交：推进对外可见的 generation，并按该批 placements 数
        // 写入独立的 membership claims（与 SqliteStore 的批次提交语义一致）。
        *self.generation.borrow_mut() += 1;
        *self.claims.borrow_mut() += claims;
        Ok(())
    }
}

impl CatalogStore for InMemoryStore {
    fn get(&self, id: &StableId) -> PortResult<Option<Vec<u8>>> {
        Ok(self.catalog.borrow().get(id.as_str()).cloned())
    }

    fn put(&self, id: &StableId, payload: &[u8]) -> PortResult<()> {
        self.catalog
            .borrow_mut()
            .insert(id.as_str().to_string(), payload.to_vec());
        Ok(())
    }

    fn list(&self, limit: usize) -> PortResult<Vec<CatalogEntry>> {
        let catalog = self.catalog.borrow();
        let mut rows: Vec<_> = catalog.iter().collect();
        rows.sort_by_key(|(id, _)| *id);
        rows.into_iter()
            .take(limit)
            .map(|(wire, payload)| {
                let id = StableId::from_wire(wire).ok_or_else(|| {
                    PortError::Backend(format!("invalid StableId in fake catalog: {wire}"))
                })?;
                Ok(CatalogEntry {
                    id,
                    payload: payload.clone(),
                })
            })
            .collect()
    }

    fn count(&self) -> PortResult<u64> {
        Ok(self.catalog.borrow().len() as u64)
    }

    fn active_generation(&self) -> PortResult<u64> {
        Ok(*self.generation.borrow())
    }
}

impl SearchIndex for InMemoryStore {
    fn index(&self, id: &StableId, text: &str) -> PortResult<()> {
        let mut idx = self.index.borrow_mut();
        // 幂等：先移除同 id 旧条目，与真实 adapter 的重索引语义一致。
        idx.retain(|(existing, _)| existing.as_str() != id.as_str());
        idx.push((id.clone(), text.to_string()));
        Ok(())
    }

    fn query(&self, query: &str, limit: usize) -> PortResult<Vec<SearchHit>> {
        // 空查询返回零命中：SQLite 端空 MATCH 是错误，这里以空结果近似，
        // 绝不返回全部（内存实现无 SQL 语法层，无法复刻报错）。
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let idx = self.index.borrow();
        let mut hits: Vec<SearchHit> = idx
            .iter()
            .filter(|(_, text)| text.contains(query))
            .map(|(id, _)| SearchHit {
                id: id.clone(),
                score: 1.0,
            })
            .collect();
        // 与 SQLite 的全序一致：score 降序，同分按 id 升序（全 1.0 时退化为
        // 纯 id 升序），保证跨次运行与分页稳定。
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

impl ContextGraphStore for InMemoryStore {
    fn load_session_graph(&self, session_id: &StableId) -> PortResult<SessionContextGraph> {
        // 与 SqliteStore 一致：kind 校验 + catalog 存在性校验先行。
        if session_id.kind() != IdKind::Session {
            return Err(PortError::NotFound("session context not found".into()));
        }
        if !self.catalog.borrow().contains_key(session_id.as_str()) {
            return Err(PortError::NotFound("session context not found".into()));
        }
        self.graphs
            .borrow()
            .get(session_id.as_str())
            .cloned()
            .ok_or_else(|| PortError::NotFound("session graph not found".into()))
    }

    fn message_contexts(&self, message_id: &StableId) -> PortResult<Vec<MessageContextCandidate>> {
        // 与 SqliteStore 一致：先做 kind 校验与 catalog 存在性校验（端口契约：
        // catalog 中不存在的消息是 lookup miss → NotFound，绝非空成功）。
        if message_id.kind() != IdKind::Message {
            return Err(PortError::NotFound("message context not found".into()));
        }
        if !self.catalog.borrow().contains_key(message_id.as_str()) {
            return Err(PortError::NotFound("message context not found".into()));
        }
        let graphs = self.graphs.borrow();
        let mut candidates: Vec<MessageContextCandidate> = graphs
            .values()
            .filter_map(|graph| {
                let mut placement_ids = graph
                    .placements
                    .iter()
                    .filter(|placement| placement.message_id.as_str() == message_id.as_str())
                    .map(|placement| placement.id.clone())
                    .collect::<Vec<_>>();
                if placement_ids.is_empty() {
                    return None;
                }
                placement_ids.sort();
                Some(MessageContextCandidate {
                    session_id: graph.session_id.clone(),
                    placement_ids,
                })
            })
            .collect();
        candidates.sort_by(|left, right| left.session_id.as_str().cmp(right.session_id.as_str()));
        Ok(candidates)
    }

    fn context_stats(&self) -> PortResult<ContextStats> {
        let placements = self
            .graphs
            .borrow()
            .values()
            .map(|graph| graph.placements.len() as u64)
            .sum();
        Ok(ContextStats {
            placements,
            // 独立计数：与 SqliteStore 从 source_placement_membership 表取值一致。
            source_placement_claims: *self.claims.borrow(),
        })
    }
}

/// 可配置的 fake Provider（RFC-0002 合同的测试替身）。
///
/// 不解析任何真实格式——按构造时给定的脚本行为响应 probe/parse，
/// 用于在不依赖具体 provider 的前提下测 ingestion 编排、错误矩阵与拒绝路径。
pub struct FakeProvider {
    provider_id: String,
    probe_confidence: Confidence,
    variant_id: String,
    /// parse 时逐条 emit 的 (role, text)；为空则产出空报告。
    messages: Vec<(String, String)>,
    /// 若 Some，parse 直接返回该错误（测试回滚/拒绝路径）。
    parse_error: Option<ProviderError>,
}

impl FakeProvider {
    /// 一个"表现良好"的 provider：confirmed 置信度，按给定消息成功解析。
    pub fn good(provider_id: &str, messages: &[(&str, &str)]) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            probe_confidence: Confidence::Confirmed,
            variant_id: format!("{provider_id}/fake-v1"),
            messages: messages
                .iter()
                .map(|(r, t)| (r.to_string(), t.to_string()))
                .collect(),
            parse_error: None,
        }
    }

    /// 一个 probe 判定为 ambiguous 的 provider（测拒绝解析路径）。
    pub fn ambiguous(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            probe_confidence: Confidence::Ambiguous,
            variant_id: format!("{provider_id}/unknown"),
            messages: Vec::new(),
            parse_error: None,
        }
    }

    /// 一个 parse 时结构性失败的 provider（测 source 回滚路径）。
    ///
    /// 先 emit 给定 messages，再返回 StructuralFatal——用于验证 staging
    /// "部分产出后失败 → 整批丢弃"的原子语义（RFC-0002 §5）。
    pub fn structural_fatal(provider_id: &str, reason: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            probe_confidence: Confidence::Confirmed,
            variant_id: format!("{provider_id}/fake-v1"),
            messages: Vec::new(),
            parse_error: Some(ProviderError::StructuralFatal(reason.to_string())),
        }
    }

    /// parse 先 emit 给定消息，再返回 StructuralFatal（staging 原子性硬测）。
    pub fn failing_after(provider_id: &str, messages: &[(&str, &str)], reason: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            probe_confidence: Confidence::Confirmed,
            variant_id: format!("{provider_id}/fake-v1"),
            messages: messages
                .iter()
                .map(|(r, t)| (r.to_string(), t.to_string()))
                .collect(),
            parse_error: Some(ProviderError::StructuralFatal(reason.to_string())),
        }
    }
}

impl ProviderAdapter for FakeProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn probe(&self, _bytes: &[u8]) -> Result<ProbeResult, ProviderError> {
        Ok(ProbeResult {
            variant_id: self.variant_id.clone(),
            confidence: self.probe_confidence,
            matched_evidence: vec!["fake probe".to_string()],
            unmatched_evidence: Vec::new(),
        })
    }

    fn parse(
        &self,
        _bytes: &[u8],
        sink: &mut dyn CanonicalEventSink,
    ) -> Result<ParseReport, ProviderError> {
        // 先 emit 已配置的消息；若同时设了 parse_error，则在 emit 之后再失败——
        // 这是 staging 原子性测试的关键路径：部分产出后 fatal，缓冲应整批丢弃。
        let mut report = ParseReport::default();
        for (seq, (role, text)) in self.messages.iter().enumerate() {
            sink.emit_message(MessageEvent {
                seq: seq as u32,
                native_id: "",
                parent_native_id: None,
                role,
                text,
                timestamp: None,
                is_sidechain: false,
                span: None,
            })
            // 与真实 adapter 一致：sink 写入失败属于结构性错误 → StructuralFatal。
            .map_err(|e| ProviderError::StructuralFatal(e.to_string()))?;
            report.committed += 1;
        }
        if let Some(err) = &self.parse_error {
            return Err(err.clone());
        }
        Ok(report)
    }
}

/// 只读断言：捕获 `bytes` 的长度与内容指纹，运行 `op`，再断言字节未被改写。
///
/// 落实 RFC-0002 §7 "adapter 绝不修改源" 的测试侧守护——把只读契约变成可执行断言，
/// 而非仅靠代码审查。`op` 拿到的是只读切片，任何试图改写的 adapter 都无法通过类型系统，
/// 本函数额外在运行时确认调用前后指纹一致（防御 `op` 内经旁路改动传入缓冲）。
pub fn assert_read_only<R>(bytes: &[u8], op: impl FnOnce(&[u8]) -> R) -> R {
    let before_len = bytes.len();
    let before_fp = blake3_hex(bytes);
    let result = op(bytes);
    assert_eq!(bytes.len(), before_len, "只读契约破坏：字节长度改变");
    assert_eq!(blake3_hex(bytes), before_fp, "只读契约破坏：内容指纹改变");
    result
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// 便捷断言：一个 `PortResult` 应为 `PortError::NotFound`。
pub fn assert_not_found<T: std::fmt::Debug>(result: PortResult<T>) {
    match result {
        Err(PortError::NotFound(_)) => {}
        other => panic!("expected PortError::NotFound, got {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_builder_produces_valid_session() {
        let graph = SessionBuilder::new("t1")
            .messages(["hello", "world", "again"])
            .build();
        assert_eq!(graph.messages.len(), 3);
        assert!(graph.validate().is_ok(), "builder 应产出通过不变量的会话图");
        // 角色交替：User, Assistant, User。
        assert_eq!(graph.messages[0].role, Role::User);
        assert_eq!(graph.messages[1].role, Role::Assistant);
        assert_eq!(graph.messages[2].role, Role::User);
        assert_eq!(graph.placements.len(), 3);
        assert_eq!(graph.edges.len(), 2);
    }

    #[test]
    fn session_builder_is_deterministic() {
        let a = SessionBuilder::new("same").message("x").build();
        let b = SessionBuilder::new("same").message("x").build();
        assert_eq!(a.session_id, b.session_id);
        assert_eq!(a.messages[0].id, b.messages[0].id);
        assert_eq!(a.placements[0].id, b.placements[0].id);
    }

    #[test]
    fn in_memory_context_store_groups_by_session() {
        let store = InMemoryStore::new();
        let graph = SessionBuilder::new("ctx").messages(["a", "b"]).build();
        let session_id = graph.session_id.clone();
        let message_id = graph.messages[0].id.clone();
        // catalog 是"实体是否存在"的唯一事实源（端口契约）：查询前先提交。
        store.put(&session_id, b"session").unwrap();
        store.put(&message_id, b"message").unwrap();
        store.insert_graph(graph).unwrap();

        assert_eq!(
            store.load_session_graph(&session_id).unwrap().session_id,
            session_id
        );
        let candidates = store.message_contexts(&message_id).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].placement_ids.len(), 1);
        let stats = store.context_stats().unwrap();
        assert_eq!(stats.placements, 2);
        // claims 来自独立计数（insert_graph 时写入），本用例与 placements 同值。
        assert_eq!(stats.source_placement_claims, 2);
        // 提交一个批次 → generation 推进为 1（与 SqliteStore 批次语义一致）。
        assert_eq!(store.active_generation().unwrap(), 1);
    }

    #[test]
    fn in_memory_message_contexts_missing_message_is_not_found() {
        let store = InMemoryStore::new();
        let graph = SessionBuilder::new("ctx-miss").messages(["a"]).build();
        let session_id = graph.session_id.clone();
        store.put(&session_id, b"session").unwrap();
        store.insert_graph(graph).unwrap();
        // 从未提交到 catalog 的消息 → lookup miss → NotFound（端口契约）。
        let ghost = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"ghost"]);
        assert_not_found(store.message_contexts(&ghost));
        // 非 Message kind 的 id 同样 NotFound。
        assert_not_found(store.message_contexts(&session_id));
    }

    #[test]
    fn in_memory_load_session_graph_checks_kind_and_catalog() {
        let store = InMemoryStore::new();
        let graph = SessionBuilder::new("ctx-kind").messages(["a"]).build();
        let message_id = graph.messages[0].id.clone();
        store.put(&message_id, b"message").unwrap();
        store.insert_graph(graph.clone()).unwrap();
        // kind 不符 → NotFound（与 SqliteStore 一致）。
        assert_not_found(store.load_session_graph(&message_id));
        // catalog 中不存在 → NotFound。
        let ghost = StableId::derive(IdKind::Session, Stability::Reconstructed, &[b"ghost-sess"]);
        assert_not_found(store.load_session_graph(&ghost));
        // catalog + graph 都提交过 → 正常加载。
        store.put(&graph.session_id, b"session").unwrap();
        assert_eq!(
            store
                .load_session_graph(&graph.session_id)
                .unwrap()
                .session_id,
            graph.session_id
        );
    }

    #[test]
    fn in_memory_generation_advances_per_committed_batch() {
        let store = InMemoryStore::new();
        assert_eq!(store.active_generation().unwrap(), 0);
        store
            .insert_graph(SessionBuilder::new("g1").messages(["a"]).build())
            .unwrap();
        store
            .insert_graph(SessionBuilder::new("g2").messages(["a", "b"]).build())
            .unwrap();
        assert_eq!(store.active_generation().unwrap(), 2);
        let stats = store.context_stats().unwrap();
        assert_eq!(stats.placements, 3);
        assert_eq!(stats.source_placement_claims, 3);
    }

    #[test]
    fn in_memory_store_roundtrips_and_searches() {
        let store = InMemoryStore::new();
        let id = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"m"]);
        store.put(&id, b"payload").unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap(), b"payload");

        store.index(&id, "the quick brown fox").unwrap();
        let hits = store.query("brown", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);
    }

    #[test]
    fn in_memory_reindex_is_idempotent() {
        let store = InMemoryStore::new();
        let id = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"m"]);
        store.index(&id, "alpha").unwrap();
        store.index(&id, "beta").unwrap();
        assert!(store.query("alpha", 10).unwrap().is_empty());
        assert_eq!(store.query("beta", 10).unwrap().len(), 1);
    }

    #[test]
    fn in_memory_query_orders_by_score_then_id_and_rejects_empty_query() {
        let store = InMemoryStore::new();
        // 乱序索引，期望结果按 wire id 升序（score 全 1.0 时即纯 id 全序）。
        let z = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"zebra"]);
        let a = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"alpha"]);
        let m = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"middle"]);
        store.index(&z, "shared token zoo").unwrap();
        store.index(&a, "shared token alpha").unwrap();
        store.index(&m, "shared token middle").unwrap();
        let hits = store.query("shared token", 10).unwrap();
        let mut expected = vec![a.as_str(), m.as_str(), z.as_str()];
        expected.sort();
        let actual: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(actual, expected);
        // 空查询返回零命中（SQLite 空 MATCH 是错误，此处以空近似），绝不返回全部。
        assert!(store.query("", 10).unwrap().is_empty());
        // limit 截断生效。
        assert_eq!(store.query("shared token", 2).unwrap().len(), 2);
    }

    #[test]
    fn session_builder_source_document_metadata_is_self_consistent() {
        let graph = SessionBuilder::new("meta").message("x").build();
        let doc = &graph.source_documents[0];
        // fingerprint 与 len 必须描述同一字节串（fact 即合成快照），不得互相矛盾。
        assert_eq!(doc.fingerprint, blake3_hex(b"meta"));
        assert_eq!(doc.len, "meta".len() as u64);
    }

    #[test]
    fn fake_provider_good_parses_all_messages() {
        let provider = FakeProvider::good("demo", &[("user", "hi"), ("assistant", "yo")]);
        let probe = provider.probe(b"anything").unwrap();
        assert_eq!(probe.confidence, Confidence::Confirmed);

        // 用 InMemoryStore 无法直接当 sink——sink 需要 emit_message；用一个计数 sink。
        struct CountSink(usize);
        impl CanonicalEventSink for CountSink {
            fn emit_message(&mut self, _event: MessageEvent<'_>) -> PortResult<()> {
                self.0 += 1;
                Ok(())
            }
        }
        let mut sink = CountSink(0);
        let report = provider.parse(b"anything", &mut sink).unwrap();
        assert_eq!(report.committed, 2);
        assert_eq!(sink.0, 2);
    }

    #[test]
    fn fake_provider_ambiguous_signals_ambiguous() {
        let provider = FakeProvider::ambiguous("demo");
        assert_eq!(
            provider.probe(b"x").unwrap().confidence,
            Confidence::Ambiguous
        );
    }

    #[test]
    fn fake_provider_structural_fatal_errors_on_parse() {
        let provider = FakeProvider::structural_fatal("demo", "corrupt header");
        struct NullSink;
        impl CanonicalEventSink for NullSink {
            fn emit_message(&mut self, _event: MessageEvent<'_>) -> PortResult<()> {
                Ok(())
            }
        }
        let err = provider.parse(b"x", &mut NullSink).unwrap_err();
        assert!(matches!(err, ProviderError::StructuralFatal(_)));
    }

    #[test]
    fn fake_provider_maps_sink_failure_to_structural_fatal() {
        let provider = FakeProvider::good("demo", &[("user", "hi")]);
        struct FailingSink;
        impl CanonicalEventSink for FailingSink {
            fn emit_message(&mut self, _event: MessageEvent<'_>) -> PortResult<()> {
                Err(PortError::Backend("synthetic sink failure".into()))
            }
        }
        let err = provider.parse(b"x", &mut FailingSink).unwrap_err();
        assert!(
            matches!(err, ProviderError::StructuralFatal(_)),
            "sink 错误应映射为 StructuralFatal（与真实 adapter 一致），got {err:?}"
        );
    }

    #[test]
    fn assert_read_only_passes_for_pure_read() {
        let data = b"immutable source bytes";
        let count = assert_read_only(data, |b| b.iter().filter(|&&c| c == b' ').count());
        assert_eq!(count, 2);
    }
}
