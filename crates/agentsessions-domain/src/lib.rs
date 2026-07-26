//! AgentSessions 领域层：Canonical 模型与类型化稳定 ID 的唯一事实来源。
//!
//! 本 crate 不依赖任何 IO、存储或框架——纯领域类型与不变量。
//! 身份规则见 `docs/architecture/RFC-0001-canonical-model-and-stable-id.md`。

mod error;
mod ids;
mod thread;

pub use error::{DomainError, DomainResult};
pub use ids::{IdKind, Stability, StableId};
pub use thread::{BranchSelection, ContextPolicy, select_full, select_mainline};

use serde::{Deserialize, Serialize};

/// 角色：Canonical 消息的发言方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

/// 指向已验证来源快照的字节区间；`end` 为排他边界。
///
/// 不变量：`start <= end`。区间以"已验证快照字节"为坐标系——对文件级来源
/// 即快照全文，对未来的行级来源即提取出的行负载。由 provider 在解析期上报，
/// 领域层绝不臆造。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSpan {
    pub start: u64,
    pub end: u64,
}

/// 一条 Canonical 消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: StableId,
    /// 父消息 ID：threading DAG 的边。根消息为 `None`。
    ///
    /// 由 provider 的 native 父指针（如 Claude Code 的 `parentUuid`）重建；
    /// 供 `show` / `get_session_context` 定位 Thread/Branch。
    #[serde(default)]
    pub parent: Option<StableId>,
    pub role: Role,
    /// 归一化后的纯文本内容（供全文索引）。
    pub text: String,
    /// 该消息在会话内的单调序号，从 0 起。
    pub seq: u32,
    /// 消息发生时刻，provider 原样保留的时间串（Claude Code 为 ISO-8601 UTC）。
    ///
    /// 领域层不解析为具体时间类型——保留 lossless 原串，避免引入日期库依赖，
    /// 也不臆造精度；需要比较/排序时由上层按 provider 语义解释。缺失为 `None`。
    #[serde(default)]
    pub timestamp: Option<String>,
    /// 是否为 sidechain（subagent/分支）消息。
    ///
    /// Claude Code 的 `isSidechain` 直接映射：主线程消息为 `false`，
    /// subagent transcript 里的消息为 `true`。供 Thread/Branch 区分主线与旁支。
    #[serde(default)]
    pub is_sidechain: bool,
    /// 该消息在来源快照中的字节区间证据。
    ///
    /// `None` 表示"未记录出处"（legacy 行或 provider 无法归因连续区间），
    /// 是显式的缺失，绝不臆造。
    #[serde(default)]
    pub span: Option<EvidenceSpan>,
}

/// 一个 Canonical 会话：来自某个来源文档的一段连续对话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: StableId,
    /// 归属的来源文档 ID。
    pub document_id: StableId,
    pub messages: Vec<Message>,
}

impl Session {
    /// 校验会话的领域不变量：ID 类别正确、消息序号从 0 连续单调递增。
    pub fn validate(&self) -> DomainResult<()> {
        if self.id.kind() != IdKind::Session {
            return Err(DomainError::InvariantViolation(format!(
                "session id has wrong kind: {:?}",
                self.id.kind()
            )));
        }
        if self.document_id.kind() != IdKind::Document {
            return Err(DomainError::InvariantViolation(format!(
                "document_id has wrong kind: {:?}",
                self.document_id.kind()
            )));
        }
        for (i, m) in self.messages.iter().enumerate() {
            if m.id.kind() != IdKind::Message {
                return Err(DomainError::InvariantViolation(format!(
                    "message[{i}] id has wrong kind: {:?}",
                    m.id.kind()
                )));
            }
            if m.seq as usize != i {
                return Err(DomainError::InvariantViolation(format!(
                    "message[{i}] seq is {}, expected {i}",
                    m.seq
                )));
            }
            // 父指针若存在，必须也命名一条 Message（threading 边只连消息）。
            // 不强制父消息在本会话内可见——sidechain 的父可能落在主 transcript，
            // 跨文档的完整 DAG 拼接归上层，领域层只保证边的类型正确。
            if let Some(parent) = &m.parent
                && parent.kind() != IdKind::Message
            {
                return Err(DomainError::InvariantViolation(format!(
                    "message[{i}] parent has wrong kind: {:?}",
                    parent.kind()
                )));
            }
            // span 若存在，必须满足 start <= end（end 为排他边界）。
            if let Some(span) = &m.span
                && span.start > span.end
            {
                return Err(DomainError::InvariantViolation(format!(
                    "message[{i}] span start {} > end {}",
                    span.start, span.end
                )));
            }
        }
        Ok(())
    }
}

/// 一个来源文档实体：某 provider 变体下、已验证快照的内容寻址描述。
///
/// 实体本身不存路径——身份与位置分离（RFC-0001）；路径→文档的关联归存储层。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocument {
    pub id: StableId,
    /// 来源 provider 标识，如 `"claude-code"`。
    pub provider_id: String,
    /// provider 格式变体标识，如 `"claude-code/jsonl-v1"`。
    pub variant_id: String,
    /// 已验证快照全文的 BLAKE3 十六进制指纹。
    pub fingerprint: String,
    /// 快照字节长度——消息 span 索引的坐标系上界。
    pub len: u64,
}

impl SourceDocument {
    /// 校验文档实体的领域不变量：ID 类别必须为 Document。
    pub fn validate(&self) -> DomainResult<()> {
        if self.id.kind() != IdKind::Document {
            return Err(DomainError::InvariantViolation(format!(
                "document id has wrong kind: {:?}",
                self.id.kind()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(seq: u32) -> Message {
        Message {
            id: StableId::derive(
                IdKind::Message,
                Stability::Reconstructed,
                &[&seq.to_le_bytes()],
            ),
            parent: None,
            role: Role::User,
            text: format!("m{seq}"),
            seq,
            timestamp: None,
            is_sidechain: false,
            span: None,
        }
    }

    fn session_with(messages: Vec<Message>) -> Session {
        Session {
            id: StableId::derive(IdKind::Session, Stability::Reconstructed, &[b"s"]),
            document_id: StableId::derive(IdKind::Document, Stability::Reconstructed, &[b"d"]),
            messages,
        }
    }

    #[test]
    fn valid_session_passes() {
        let s = session_with(vec![msg(0), msg(1), msg(2)]);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn non_monotonic_seq_rejected() {
        let s = session_with(vec![msg(0), msg(2)]);
        let err = s.validate().unwrap_err();
        assert_eq!(err.code(), "invariant_violation");
    }

    #[test]
    fn wrong_id_kind_rejected() {
        let mut s = session_with(vec![msg(0)]);
        s.id = StableId::derive(IdKind::Source, Stability::Reconstructed, &[b"x"]);
        assert_eq!(s.validate().unwrap_err().code(), "invariant_violation");
    }

    #[test]
    fn valid_parent_link_passes() {
        let mut child = msg(1);
        child.parent = Some(msg(0).id);
        let s = session_with(vec![msg(0), child]);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn parent_with_wrong_kind_rejected() {
        let mut child = msg(1);
        // 父指针指向一个 Session 而非 Message——threading 边类型非法。
        child.parent = Some(StableId::derive(
            IdKind::Session,
            Stability::Reconstructed,
            &[b"not-a-message"],
        ));
        let s = session_with(vec![msg(0), child]);
        assert_eq!(s.validate().unwrap_err().code(), "invariant_violation");
    }

    #[test]
    fn sidechain_and_timestamp_fields_carried() {
        let mut m = msg(0);
        m.is_sidechain = true;
        m.timestamp = Some("2026-06-27T13:57:42.685Z".into());
        // 新字段随消息一同承载并参与相等性判定。
        let clone = m.clone();
        assert_eq!(clone, m);
        assert!(clone.is_sidechain);
        assert_eq!(clone.timestamp.as_deref(), Some("2026-06-27T13:57:42.685Z"));
    }

    #[test]
    fn message_json_without_span_deserializes_to_none() {
        // legacy JSON（无 span 字段）必须反序列化为 None——显式缺失，不臆造。
        let m = msg(0);
        let mut value = serde_json::to_value(&m).unwrap();
        value.as_object_mut().unwrap().remove("span");
        let parsed: Message = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.span, None);
        assert_eq!(parsed, m);
    }

    #[test]
    fn valid_span_passes() {
        let mut m = msg(0);
        m.span = Some(EvidenceSpan { start: 10, end: 42 });
        let s = session_with(vec![m]);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn span_start_greater_than_end_rejected() {
        let mut m = msg(0);
        m.span = Some(EvidenceSpan { start: 43, end: 42 });
        let s = session_with(vec![m]);
        assert_eq!(s.validate().unwrap_err().code(), "invariant_violation");
    }

    fn doc(kind: IdKind) -> SourceDocument {
        SourceDocument {
            id: StableId::derive(kind, Stability::Reconstructed, &[b"d"]),
            provider_id: "claude-code".into(),
            variant_id: "claude-code/jsonl-v1".into(),
            fingerprint: "deadbeef".into(),
            len: 128,
        }
    }

    #[test]
    fn valid_source_document_passes() {
        assert!(doc(IdKind::Document).validate().is_ok());
    }

    #[test]
    fn source_document_wrong_id_kind_rejected() {
        let d = doc(IdKind::Session);
        assert_eq!(d.validate().unwrap_err().code(), "invariant_violation");
    }
}
