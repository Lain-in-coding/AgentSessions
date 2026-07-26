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

use agentsessions_domain::{IdKind, Message, Role, Session, Stability, StableId};
use agentsessions_ports::{
    CanonicalEventSink, CatalogEntry, CatalogStore, Confidence, MessageEvent, ParseReport,
    PortError, PortResult, ProbeResult, ProviderAdapter, ProviderError, SearchHit, SearchIndex,
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

    /// 产出会话。保证通过领域不变量校验。
    pub fn build(self) -> Session {
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
        let messages = self
            .texts
            .iter()
            .enumerate()
            .map(|(i, text)| {
                let seq = i as u32;
                let role = if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                };
                // 每条消息的父指向上一条，形成一条合法的线性 threading 链。
                let parent = seq.checked_sub(1).map(mid);
                Message {
                    id: mid(seq),
                    role,
                    text: text.clone(),
                    seq,
                    parent,
                    timestamp: None,
                    is_sidechain: false,
                    span: None,
                }
            })
            .collect();
        Session {
            id,
            document_id,
            messages,
        }
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
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
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
        Ok(0)
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
        let idx = self.index.borrow();
        let hits = idx
            .iter()
            .filter(|(_, text)| text.contains(query))
            .take(limit)
            .map(|(id, _)| SearchHit {
                id: id.clone(),
                score: 1.0,
            })
            .collect();
        Ok(hits)
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
            .map_err(|e| ProviderError::Io(e.to_string()))?;
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
        let s = SessionBuilder::new("t1")
            .messages(["hello", "world", "again"])
            .build();
        assert_eq!(s.messages.len(), 3);
        assert!(s.validate().is_ok(), "builder 应产出通过不变量的会话");
        // 角色交替：User, Assistant, User。
        assert_eq!(s.messages[0].role, Role::User);
        assert_eq!(s.messages[1].role, Role::Assistant);
        assert_eq!(s.messages[2].role, Role::User);
    }

    #[test]
    fn session_builder_is_deterministic() {
        let a = SessionBuilder::new("same").message("x").build();
        let b = SessionBuilder::new("same").message("x").build();
        assert_eq!(a.id, b.id);
        assert_eq!(a.messages[0].id, b.messages[0].id);
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
    fn assert_read_only_passes_for_pure_read() {
        let data = b"immutable source bytes";
        let count = assert_read_only(data, |b| b.iter().filter(|&&c| c == b' ').count());
        assert_eq!(count, 2);
    }
}
