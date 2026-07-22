//! AgentSessions 领域层：Canonical 模型与类型化稳定 ID 的唯一事实来源。
//!
//! 本 crate 不依赖任何 IO、存储或框架——纯领域类型与不变量。
//! 身份规则见 `docs/architecture/RFC-0001-canonical-model-and-stable-id.md`。

mod error;
mod ids;

pub use error::{DomainError, DomainResult};
pub use ids::{IdKind, Stability, StableId};

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

/// 一条 Canonical 消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: StableId,
    /// 父消息 ID：threading DAG 的边。根消息为 `None`。
    ///
    /// 由 provider 的 native 父指针（如 Claude Code 的 `parentUuid`）重建；
    /// 供 `show` / `get_session_context` 定位 Thread/Branch（见计划 §show 要求）。
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
}
