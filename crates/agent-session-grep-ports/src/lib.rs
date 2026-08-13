//! Ports：Application 与外部世界之间的契约边界（Hexagonal 架构的"端口"）。
//!
//! 这里只定义 trait 和相关 DTO，不含任何具体实现——实现属于 adapter crate。
//! Application 只依赖本 crate 的抽象，从而与 SQLite / 文件系统 / 具体 provider 解耦。
//!
//! 分层依赖不变量：domain ← ports ← application ← adapters。

use agent_session_grep_domain::{
    DomainError, DomainResult, PlacementId, SessionContextGraph, StableId,
};

/// 端口层错误：包裹底层 IO/存储故障，向上层暴露稳定分类。
///
/// 与 `DomainError` 区分：DomainError 是"业务语义错误"，PortError 是"基础设施错误"。
/// Application 层负责把 PortError 归一为对外协议错误。
#[derive(Debug, thiserror::Error)]
pub enum PortError {
    /// 底层存储/IO 故障。
    #[error("backend failure: {0}")]
    Backend(String),

    /// 源文件读取或快照元数据 I/O 故障。
    #[error("source I/O failure: {0}")]
    SourceIo(String),

    /// 当前二进制不支持该 catalog schema 版本。
    #[error("schema incompatible: {0}")]
    SchemaIncompatible(String),

    /// 请求的资源在后端不存在。
    #[error("not found: {0}")]
    NotFound(String),

    /// 源快照校验失败（长度/mtime/指纹不一致，见 ReadOnlySourceSnapshot 契约）。
    #[error("source snapshot changed: {0}")]
    SnapshotChanged(String),

    /// data-root writer lease 已被占用，未能取得（对应 error catalog `writer_busy`）。
    #[error("writer lease held: {0}")]
    WriterBusy(String),
}

pub type PortResult<T> = Result<T, PortError>;

/// 只读源快照：记录发现时刻源文件的验证元数据。
///
/// 落实 ReadOnlySourceSnapshot 契约——原始会话文件严格只读，
/// 通过 len + mtime + fingerprint 三元组检测读取期间是否被并发改写。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    /// 源在存储中的规范路径（相对 data-root 或绝对，由 adapter 定义）。
    pub path: String,
    /// 字节长度。
    pub len: u64,
    /// 修改时间（Unix 毫秒）。
    pub mtime_ms: i64,
    /// 内容指纹（BLAKE3 十六进制），用于识别等长改写。
    pub fingerprint: String,
}

/// 源发现端口：枚举某个 provider 根目录下的候选原始会话文件。
///
/// 严格只读——实现绝不修改被发现的文件。
pub trait SourceDiscovery {
    /// 列出当前可见的源快照。返回顺序不做保证。
    fn discover(&self) -> PortResult<Vec<SourceSnapshot>>;

    /// 读取指定源的完整字节，并校验其未在发现后被改写。
    ///
    /// 若 len/mtime/fingerprint 与传入快照不符，返回 [`PortError::SnapshotChanged`]。
    fn read_verified(&self, snapshot: &SourceSnapshot) -> PortResult<Vec<u8>>;
}

/// Catalog 列表项：稳定 ID + 已规范化 payload。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: StableId,
    pub payload: Vec<u8>,
}

/// 目录存储端口：规范化实体的持久化目录（对应 SQLite catalog）。
///
/// 只暴露按 StableId 存取及稳定排序列表；全文查询能力由 SearchIndex 承担。
pub trait CatalogStore {
    /// 按 StableId 取回已规范化实体的原始 JSON 负载。
    fn get(&self, id: &StableId) -> PortResult<Option<Vec<u8>>>;

    /// 写入（或覆盖）一个规范化实体。
    fn put(&self, id: &StableId, payload: &[u8]) -> PortResult<()>;

    /// 按 wire id 升序列出最多 `limit` 个实体；稳定排序便于后续接 cursor。
    fn list(&self, limit: usize) -> PortResult<Vec<CatalogEntry>>;

    /// Catalog 当前实体总数（status/doctor 使用）。
    fn count(&self) -> PortResult<u64>;

    /// 当前对外可见的不可变 generation。`0` 表示尚未激活任何写批次。
    fn active_generation(&self) -> PortResult<u64>;
}

/// One distinct Session that contains placements for a stable Message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageContextCandidate {
    pub session_id: StableId,
    pub placement_ids: Vec<PlacementId>,
}

/// Aggregate contextual-relation counts exposed without backend details.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextStats {
    pub placements: u64,
    pub source_placement_claims: u64,
}

/// Backend-independent read capability for contextual Message relations.
///
/// Implementations return Domain graph values and stable typed candidates;
/// SQLite rows, table names, and compatibility JSON never cross this boundary.
pub trait ContextGraphStore {
    fn load_session_graph(&self, session_id: &StableId) -> PortResult<SessionContextGraph>;

    /// Return candidates grouped by distinct Session, not by raw placement.
    ///
    /// A message that is not present in the catalog is a lookup miss and must
    /// fail with [`PortError::NotFound`], matching `load_session_graph`; it is
    /// never an empty success. Implementations must not invent candidates for
    /// messages they cannot see.
    fn message_contexts(&self, message_id: &StableId) -> PortResult<Vec<MessageContextCandidate>>;

    fn context_stats(&self) -> PortResult<ContextStats>;
}

/// 检索命中：一条搜索结果的最小信息。
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// 命中实体的稳定 ID。
    pub id: StableId,
    /// 相关性打分（后端相对值，跨后端不可比）。
    pub score: f32,
}

/// 全文检索端口：对应 FTS5 主存（见 ADR-0001）。
pub trait SearchIndex {
    /// 将实体文本纳入索引。`text` 为已抽取的可检索正文。
    fn index(&self, id: &StableId, text: &str) -> PortResult<()>;

    /// 执行查询，返回按相关性降序的命中，最多 `limit` 条。
    fn query(&self, query: &str, limit: usize) -> PortResult<Vec<SearchHit>>;
}

// 对 `&T` 的 blanket impl：端口方法均取 `&self`，故一个具体 store 可以
// 用共享引用同时填充 App<C,S> 的两个泛型槽（catalog 与 index 是同一实例）。
// 组合根据此复用单一 SqliteStore，无需两份连接或内部 Arc。
impl<T: CatalogStore + ?Sized> CatalogStore for &T {
    fn get(&self, id: &StableId) -> PortResult<Option<Vec<u8>>> {
        (**self).get(id)
    }
    fn put(&self, id: &StableId, payload: &[u8]) -> PortResult<()> {
        (**self).put(id, payload)
    }
    fn list(&self, limit: usize) -> PortResult<Vec<CatalogEntry>> {
        (**self).list(limit)
    }
    fn count(&self) -> PortResult<u64> {
        (**self).count()
    }
    fn active_generation(&self) -> PortResult<u64> {
        (**self).active_generation()
    }
}

impl<T: ContextGraphStore + ?Sized> ContextGraphStore for &T {
    fn load_session_graph(&self, session_id: &StableId) -> PortResult<SessionContextGraph> {
        (**self).load_session_graph(session_id)
    }

    fn message_contexts(&self, message_id: &StableId) -> PortResult<Vec<MessageContextCandidate>> {
        (**self).message_contexts(message_id)
    }

    fn context_stats(&self) -> PortResult<ContextStats> {
        (**self).context_stats()
    }
}

impl<T: SearchIndex + ?Sized> SearchIndex for &T {
    fn index(&self, id: &StableId, text: &str) -> PortResult<()> {
        (**self).index(id, text)
    }
    fn query(&self, query: &str, limit: usize) -> PortResult<Vec<SearchHit>> {
        (**self).query(query, limit)
    }
}

/// 读取并校验一个源快照，把端口错误归一为领域语义，并返回校验过的字节——
/// 调用方直接复用返回值，不必再读一次（单次 I/O）。
///
/// - 快照漂移（[`PortError::SnapshotChanged`]） → [`DomainError::InvalidRequest`]
///   （可重试的"请求已过期"信号，由 application 决定如何降级）；
/// - 源缺失（[`PortError::NotFound`]） → [`DomainError::NotFound`]（保留分类，
///   协议层映射 not_found，而不是 bug 信号）；
/// - 其余端口故障 → [`DomainError::InvariantViolation`]。
///
/// 错误消息一律不带后端细节（可能含路径）：只保留稳定分类，细节由端口层日志承担。
pub fn ensure_readable(
    discovery: &dyn SourceDiscovery,
    snapshot: &SourceSnapshot,
) -> DomainResult<Vec<u8>> {
    match discovery.read_verified(snapshot) {
        Ok(bytes) => Ok(bytes),
        Err(PortError::SnapshotChanged(_)) => {
            Err(DomainError::InvalidRequest("source snapshot stale".into()))
        }
        Err(PortError::NotFound(_)) => {
            Err(DomainError::NotFound("source snapshot not found".into()))
        }
        Err(other) => Err(DomainError::InvariantViolation(format!(
            "unexpected backend error: {}",
            port_error_kind(&other)
        ))),
    }
}

/// 端口错误的稳定分类名（不含载荷——载荷可能携带后端路径等内部细节）。
fn port_error_kind(error: &PortError) -> &'static str {
    match error {
        PortError::Backend(_) => "backend",
        PortError::SourceIo(_) => "source_io",
        PortError::SchemaIncompatible(_) => "schema_incompatible",
        PortError::NotFound(_) => "not_found",
        PortError::SnapshotChanged(_) => "snapshot_changed",
        PortError::WriterBusy(_) => "writer_busy",
    }
}

// ---------------------------------------------------------------------------
// Provider adapter 契约（落实 RFC-0002）。
//
// 每个 Provider 的格式差异被隔离在一个 adapter 内；adapter 之外只面对统一的
// Canonical 事件流。四阶段职责：discover / probe / fingerprint / parse。
// ---------------------------------------------------------------------------

/// Provider adapter 的错误分级（对应 RFC-0002 §5 错误矩阵）。
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// variant 无法确定或未知——默认拒绝解析，不做"尽量解析"。
    #[error("ambiguous or unknown variant: {0}")]
    AmbiguousVariant(String),

    /// 结构性致命错误——source 整体回滚，不污染旧数据。
    #[error("structural fatal: {0}")]
    StructuralFatal(String),

    /// 读取期间源被改写（见 ReadOnlySourceSnapshot 契约）——丢弃 staging。
    #[error("source changed during read: {0}")]
    SourceChangedDuringRead(String),

    /// 底层 IO 故障。
    #[error("io failure: {0}")]
    Io(String),
}

/// variant 探测置信度（RFC-0002 §3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// 证据充分、唯一匹配已知 variant。
    Confirmed,
    /// 高度可能，但存在少量未匹配证据。
    High,
    /// 弱匹配——单独不足以承诺解析。
    Low,
    /// 多个 variant 都可能或无法区分——默认拒绝解析。
    Ambiguous,
}

/// variant 探测结果（RFC-0002 §3 的最小落地子集）。
///
/// 首个 provider 切片只承载判定所需的核心字段；`compatibility_range`、
/// `required_capabilities` 等留待 provider 数量增长后补齐，避免过早抽象。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// 判定出的 variant 标识（如 `claude-code/jsonl-v1`）。
    pub variant_id: String,
    /// 探测置信度。
    pub confidence: Confidence,
    /// 支持判定的证据（人类可读，供诊断）。
    pub matched_evidence: Vec<String>,
    /// 与判定相悖或缺失的证据。
    pub unmatched_evidence: Vec<String>,
}

/// 解析报告（RFC-0002 §5）：区分 committed / skipped / failed，
/// 不把部分成功伪装成成功。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParseReport {
    /// 成功规范化并可提交的消息数。
    pub committed: usize,
    /// 可恢复地跳过的记录数（source 标记 incomplete，仍可提交）。
    pub skipped: usize,
    /// 诊断信息（跳过原因、未知字段计数等）。
    pub diagnostics: Vec<String>,
    /// provider 报告的 durable 会话 native id（如 Claude Code 的 `sessionId`、
    /// Codex `session_meta` 的 `session_id`）。`None` 表示 provider 未提供，
    /// 由上层回退 Reconstructed 派生——绝不臆造。
    pub session_native_id: Option<String>,
}

/// 一条规范化消息的事件载荷（RFC-0002 §2）：parse 流式产出的最小单元。
///
/// 用结构体而非长参数列表，使后续增删字段（如工具调用元数据）不必改动
/// 所有 sink 实现的方法签名。字段刻意用 provider-native 的字符串/原串承载，
/// 由 sink 侧决定如何映射到 domain 类型（未知角色、id 稳定性策略归 sink）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEvent<'a> {
    /// 会话内单调序号，从 0 起（只对成功产出的对话消息递增）。
    pub seq: u32,
    /// provider-native 消息 id（如 Claude Code 的 `uuid`）。空串表示 provider 未提供，
    /// 此时 sink 应回退到 reconstructed 派生（path+seq）。
    pub native_id: &'a str,
    /// 父消息的 native id（threading 边）。`None` 表示根消息或 provider 未提供。
    pub parent_native_id: Option<&'a str>,
    /// 角色标签（`user`/`assistant`/`system`/`tool` 等原串）。
    pub role: &'a str,
    /// 归一化可检索正文。
    pub text: &'a str,
    /// provider 原样时间串（Claude Code 为 ISO-8601 UTC）。`None` 表示缺失。
    pub timestamp: Option<&'a str>,
    /// 是否为 sidechain（subagent/分支）消息。
    pub is_sidechain: bool,
    /// 本消息源记录在**已验证快照字节**中的字节区间 `(start, end)`，end 排他。
    ///
    /// 坐标系是"快照字节"而非"文件"：对文件级来源即快照全文，对未来的行级
    /// 来源即提取出的行负载——同一契约无需改动即可覆盖两者（R4）。
    /// `None` 表示 provider 无法归因一段连续区间，绝不臆造。
    pub span: Option<(u64, u64)>,
}

/// Canonical 事件接收端（RFC-0002 §2）：parse 流式产出，绝不整体加载。
///
/// adapter 把每条规范化消息推入 sink；sink 的具体实现（staging / 直接入库）
/// 由上层决定，adapter 不关心。
pub trait CanonicalEventSink {
    /// 接收一条规范化消息事件。
    ///
    /// 载荷用 [`MessageEvent`] 承载 native 身份、父指针、时间与 sidechain 标记；
    /// 角色/ id 均以字符串传递，避免 ports 依赖 domain 的 `Role`/`StableId` 构造细节，
    /// 由 sink 侧负责映射到 Canonical 类型（未知角色、id 稳定性策略归 sink）。
    fn emit_message(&mut self, event: MessageEvent<'_>) -> PortResult<()>;
}

/// Provider adapter 最低合同（RFC-0002 §2）。
///
/// 四阶段中，discover/fingerprint 在首个切片从简（由 SourceDiscovery 端口承担
/// 发现与快照），本 trait 聚焦 probe + parse 这对最能体现格式隔离的职责。
pub trait ProviderAdapter: Send + Sync {
    /// 稳定的 provider 标识（如 `claude-code`）。
    fn provider_id(&self) -> &str;

    /// 判定字节流属于哪个 variant 及置信度。
    ///
    /// `ambiguous`/未知 variant 必须返回 [`Confidence::Ambiguous`] 或
    /// [`ProviderError::AmbiguousVariant`]，绝不静默降级为已知语义。
    fn probe(&self, bytes: &[u8]) -> Result<ProbeResult, ProviderError>;

    /// 在一致快照的字节上流式解析，把 Canonical 消息推入 `sink`。
    ///
    /// 只有整体成功才返回 `Ok(ParseReport)`；结构性错误返回
    /// [`ProviderError::StructuralFatal`]，由上层回滚 staging。
    fn parse(
        &self,
        bytes: &[u8],
        sink: &mut dyn CanonicalEventSink,
    ) -> Result<ParseReport, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_session_grep_domain::{IdKind, SessionContextGraph, Stability};

    struct FakeContextStore {
        graph: SessionContextGraph,
    }

    impl ContextGraphStore for FakeContextStore {
        fn load_session_graph(&self, session_id: &StableId) -> PortResult<SessionContextGraph> {
            if session_id.as_str() == self.graph.session_id.as_str() {
                Ok(self.graph.clone())
            } else {
                Err(PortError::NotFound("session graph".into()))
            }
        }

        fn message_contexts(
            &self,
            _message_id: &StableId,
        ) -> PortResult<Vec<MessageContextCandidate>> {
            Ok(Vec::new())
        }

        fn context_stats(&self) -> PortResult<ContextStats> {
            Ok(ContextStats {
                placements: 2,
                source_placement_claims: 3,
            })
        }
    }

    #[test]
    fn context_graph_store_reference_blanket_impl_forwards() {
        let session_id = StableId::derive(IdKind::Session, Stability::Reconstructed, &[b"session"]);
        let store = FakeContextStore {
            graph: SessionContextGraph {
                session_id: session_id.clone(),
                messages: Vec::new(),
                source_documents: Vec::new(),
                placements: Vec::new(),
                edges: Vec::new(),
            },
        };
        let store_ref = &store;

        assert_eq!(
            ContextGraphStore::load_session_graph(&store_ref, &session_id)
                .unwrap()
                .session_id,
            session_id
        );
        assert_eq!(
            ContextGraphStore::context_stats(&store_ref).unwrap(),
            ContextStats {
                placements: 2,
                source_placement_claims: 3,
            }
        );
    }

    /// 只响应一次 `read_verified` 的假发现端口（后续读取报 Backend，用于
    /// 断言调用方不会重复读取）。
    struct ReadOnceDiscovery {
        reads: std::cell::Cell<usize>,
        result: std::cell::RefCell<Option<Result<Vec<u8>, PortError>>>,
    }

    impl ReadOnceDiscovery {
        fn new(result: Result<Vec<u8>, PortError>) -> Self {
            Self {
                reads: std::cell::Cell::new(0),
                result: std::cell::RefCell::new(Some(result)),
            }
        }
    }

    impl SourceDiscovery for ReadOnceDiscovery {
        fn discover(&self) -> PortResult<Vec<SourceSnapshot>> {
            Ok(Vec::new())
        }

        fn read_verified(&self, _snapshot: &SourceSnapshot) -> PortResult<Vec<u8>> {
            self.reads.set(self.reads.get() + 1);
            self.result
                .borrow_mut()
                .take()
                .unwrap_or_else(|| Err(PortError::Backend("double read".into())))
        }
    }

    fn snapshot() -> SourceSnapshot {
        SourceSnapshot {
            path: "C:\\Users\\secret\\transcript.jsonl".into(),
            len: 42,
            mtime_ms: 1,
            fingerprint: "fp".into(),
        }
    }

    #[test]
    fn ensure_readable_returns_verified_bytes_with_single_read() {
        let discovery = ReadOnceDiscovery::new(Ok(b"verified bytes".to_vec()));
        let bytes = ensure_readable(&discovery, &snapshot()).unwrap();
        assert_eq!(bytes, b"verified bytes");
        assert_eq!(
            discovery.reads.get(),
            1,
            "校验用的字节必须直接复用，不得二次读取"
        );
    }

    #[test]
    fn ensure_readable_maps_snapshot_changed_without_backend_detail() {
        let discovery = ReadOnceDiscovery::new(Err(PortError::SnapshotChanged(
            "C:\\Users\\secret\\transcript.jsonl changed".into(),
        )));
        let err = ensure_readable(&discovery, &snapshot()).unwrap_err();
        assert_eq!(err.code(), "invalid_request");
        assert!(
            !err.to_string().contains("secret"),
            "错误消息不得泄漏后端路径: {err}"
        );
    }

    #[test]
    fn ensure_readable_preserves_not_found_classification() {
        let discovery = ReadOnceDiscovery::new(Err(PortError::NotFound(
            "C:\\Users\\secret\\missing.jsonl".into(),
        )));
        let err = ensure_readable(&discovery, &snapshot()).unwrap_err();
        assert_eq!(err.code(), "not_found");
        assert!(
            !err.to_string().contains("secret"),
            "错误消息不得泄漏后端路径: {err}"
        );
    }

    #[test]
    fn ensure_readable_maps_other_backend_errors_without_payload() {
        let discovery = ReadOnceDiscovery::new(Err(PortError::Backend(
            "sqlite error at C:\\Users\\secret\\catalog.db".into(),
        )));
        let err = ensure_readable(&discovery, &snapshot()).unwrap_err();
        assert_eq!(err.code(), "invariant_violation");
        let message = err.to_string();
        assert!(
            !message.contains("secret") && !message.contains("sqlite"),
            "错误消息只保留分类，不带后端载荷: {message}"
        );
    }
}
