//! EvidenceSpan DTO：版本化证据区间装配（CONTRACT-cli-robot-mcp-draft §2）。
//!
//! 从已存储的 canonical payload（消息 / 文档 JSON，由 CLI `staged_to_entries` 写入）
//! 装配 [`EvidenceSpanDto`]，供 Context/Show 类用例向前端返回可核验的证据定位。
//! 装配绝不臆造：输入缺失（无 span、非 JSON、无 fingerprint）一律 `None`，
//! 并用 [`Precision`] 显式声明实际达到的定位精度（contract §2 的显式降级）。

use serde::{Deserialize, Serialize};

/// 证据定位精度（contract §2：`byte|line|record|unknown`）。
///
/// 无法精确定位时不猜测，显式降级到更弱的档位；v1 只产出 `Byte` 与 `Unknown`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    Byte,
    Line,
    Record,
    Unknown,
}

/// EvidenceSpan DTO（contract §2，serde snake_case，随 contract major 版本化）。
///
/// 字节区间为半开 `[byte_start, byte_end)`，单位是"已验证快照字节"。
/// v1 只存字节 span，因此 `line_*` / `snippet_*` 恒为 `None`（design §0.2 记录的
/// 降级：降级的是 line 字段本身，precision 仍如实报告 `byte`）。不含绝对路径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EvidenceSpanDto {
    /// 确定性出现标识：blake3-16hex(`message_wire` ‖ ":" ‖ `ordinal`)，
    /// 同一消息在证据列表中的不同出现（ordinal）得到不同 id，跨进程可重现。
    pub occurrence_id: String,
    /// 消息稳定 wire id（如 `msg_v1_...`）。
    pub message_id: String,
    /// 源文档稳定 wire id；调用方经 session→document 解析后传入，未解析则 `None`。
    pub source_document_id: Option<String>,
    /// 装配时的活动 generation，供消费方与 cursor/查询结果做一致性核对。
    pub generation: u64,
    /// 源文档内容指纹（文档 payload 的 `fingerprint`）；无文档 payload 则 `None`。
    pub source_fingerprint: Option<String>,
    pub byte_start: Option<u64>,
    pub byte_end: Option<u64>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    /// 消息在所属会话中的记录序号（调用方按 seq 提供，非从 payload 推断）。
    pub record_ordinal: Option<u32>,
    pub snippet_char_start: Option<u32>,
    pub snippet_char_end: Option<u32>,
    pub precision: Precision,
}

/// 从已存储的 canonical payload 装配一条证据区间。
///
/// - `message_payload`：消息 canonical JSON（`span{start,end}` 等字段）。
///   `span` 存在且完整 → `byte_start`/`byte_end` + `precision: Byte`；
///   `span` 缺失、为 null、字段不完整或 payload 非 JSON（legacy 字节）→
///   全部位置字段 `None` + `precision: Unknown`。
/// - `document_payload`：源文档 canonical JSON，仅用于取 `fingerprint`；
///   `None` 或无该字段 → `source_fingerprint: None`。
/// - `source_document_id`：文档 wire id 由调用方解析传入（session→document
///   的归属关系存在会话 payload 里，不在消息 payload 里，装配本身不做检索）。
/// - `ordinal`：消息在会话内的记录序号，同时参与 `occurrence_id` 派生。
pub fn assemble(
    message_wire: &str,
    message_payload: &[u8],
    document_payload: Option<&[u8]>,
    source_document_id: Option<&str>,
    generation: u64,
    ordinal: u32,
) -> EvidenceSpanDto {
    let occurrence_id = {
        let digest = blake3::hash(format!("{message_wire}:{ordinal}").as_bytes());
        digest.to_hex()[..16].to_string()
    };
    let (byte_start, byte_end, precision) = match parse_span(message_payload) {
        Some((start, end)) => (Some(start), Some(end), Precision::Byte),
        None => (None, None, Precision::Unknown),
    };
    EvidenceSpanDto {
        occurrence_id,
        message_id: message_wire.to_string(),
        source_document_id: source_document_id.map(str::to_string),
        generation,
        source_fingerprint: document_payload.and_then(parse_fingerprint),
        byte_start,
        byte_end,
        line_start: None,
        line_end: None,
        record_ordinal: Some(ordinal),
        snippet_char_start: None,
        snippet_char_end: None,
        precision,
    }
}

/// 从消息 payload 提取字节 span；任何一环缺失或类型不符都返回 `None`（不臆造）。
fn parse_span(payload: &[u8]) -> Option<(u64, u64)> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let span = value.get("span")?;
    let start = span.get("start")?.as_u64()?;
    let end = span.get("end")?.as_u64()?;
    Some((start, end))
}

/// 从文档 payload 提取内容指纹；非 JSON 或无 `fingerprint` 字符串字段返回 `None`。
fn parse_fingerprint(payload: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    Some(value.get("fingerprint")?.as_str()?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CLI `staged_to_entries` 写入的 post-v6 消息 payload 形状。
    fn post_v6_payload(span: Option<(u64, u64)>) -> Vec<u8> {
        serde_json::json!({
            "role": "user",
            "text": "hello",
            "parent_native_id": null,
            "timestamp": "2026-07-26T00:00:00Z",
            "is_sidechain": false,
            "session": "ses_v1_demo",
            "span": span.map(|(start, end)| serde_json::json!({
                "start": start,
                "end": end,
            })),
        })
        .to_string()
        .into_bytes()
    }

    fn document_payload() -> Vec<u8> {
        serde_json::json!({
            "provider": "claude-code",
            "variant": "claude-code/jsonl-v1",
            "fingerprint": "b3-deadbeef",
            "len": 123,
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn post_v6_span_yields_byte_precision() {
        let dto = assemble(
            "msg_v1_a",
            &post_v6_payload(Some((10, 42))),
            None,
            None,
            7,
            0,
        );
        assert_eq!(dto.precision, Precision::Byte);
        assert_eq!(dto.byte_start, Some(10));
        assert_eq!(dto.byte_end, Some(42));
        assert_eq!(dto.message_id, "msg_v1_a");
        assert_eq!(dto.generation, 7);
        assert_eq!(dto.record_ordinal, Some(0));
        // v1 只存字节 span：line/snippet 显式缺失（design §0.2）。
        assert_eq!(dto.line_start, None);
        assert_eq!(dto.line_end, None);
        assert_eq!(dto.snippet_char_start, None);
        assert_eq!(dto.snippet_char_end, None);
    }

    #[test]
    fn legacy_json_without_span_is_unknown() {
        // CLI 对 span:None 写 null；键整体缺失的更老形状也一并覆盖。
        let dto = assemble("msg_v1_b", &post_v6_payload(None), None, None, 7, 1);
        assert_eq!(dto.precision, Precision::Unknown);
        assert_eq!(dto.byte_start, None);
        assert_eq!(dto.byte_end, None);

        let dto = assemble(
            "msg_v1_b",
            br#"{"role":"user","text":"hi"}"#,
            None,
            None,
            7,
            1,
        );
        assert_eq!(dto.precision, Precision::Unknown);
        assert_eq!(dto.byte_start, None);
        assert_eq!(dto.byte_end, None);
    }

    #[test]
    fn non_json_payload_is_unknown() {
        let dto = assemble("msg_v1_c", b"user\thello legacy bytes", None, None, 7, 2);
        assert_eq!(dto.precision, Precision::Unknown);
        assert_eq!(dto.byte_start, None);
        assert_eq!(dto.byte_end, None);
        // 非 JSON 只降级精度，不影响身份/序号字段。
        assert_eq!(dto.message_id, "msg_v1_c");
        assert_eq!(dto.record_ordinal, Some(2));
    }

    #[test]
    fn fingerprint_passes_through_from_document_payload() {
        let dto = assemble(
            "msg_v1_d",
            &post_v6_payload(Some((0, 5))),
            Some(&document_payload()),
            Some("doc_v1_x"),
            7,
            0,
        );
        assert_eq!(dto.source_fingerprint.as_deref(), Some("b3-deadbeef"));
        assert_eq!(dto.source_document_id.as_deref(), Some("doc_v1_x"));

        // 无文档 payload / payload 缺 fingerprint → 显式 None，不臆造。
        let dto = assemble("msg_v1_d", &post_v6_payload(None), None, None, 7, 0);
        assert_eq!(dto.source_fingerprint, None);
        assert_eq!(dto.source_document_id, None);
        let dto = assemble(
            "msg_v1_d",
            &post_v6_payload(None),
            Some(br#"{"provider":"p"}"#),
            None,
            7,
            0,
        );
        assert_eq!(dto.source_fingerprint, None);
    }

    #[test]
    fn occurrence_id_is_deterministic_and_ordinal_distinct() {
        let a1 = assemble("msg_v1_e", &post_v6_payload(None), None, None, 7, 0);
        let a2 = assemble("msg_v1_e", &post_v6_payload(None), None, None, 7, 0);
        let b = assemble("msg_v1_e", &post_v6_payload(None), None, None, 7, 1);
        assert_eq!(a1.occurrence_id, a2.occurrence_id);
        assert_ne!(a1.occurrence_id, b.occurrence_id);
        assert_eq!(a1.occurrence_id.len(), 16);
        assert!(a1.occurrence_id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn serializes_snake_case_wire_shape() {
        let dto = assemble(
            "msg_v1_f",
            &post_v6_payload(Some((3, 9))),
            Some(&document_payload()),
            Some("doc_v1_y"),
            5,
            4,
        );
        let wire = serde_json::to_value(&dto).unwrap();
        assert_eq!(wire["precision"], "byte");
        assert_eq!(wire["byte_start"], 3);
        assert_eq!(wire["byte_end"], 9);
        assert_eq!(wire["message_id"], "msg_v1_f");
        assert_eq!(wire["source_document_id"], "doc_v1_y");
        assert_eq!(wire["source_fingerprint"], "b3-deadbeef");
        assert_eq!(wire["record_ordinal"], 4);
        assert_eq!(wire["generation"], 5);
        // 缺失字段以显式 null 出现在 wire 上（contract §2 的完整字段集）。
        assert!(wire["line_start"].is_null());
        assert!(wire["snippet_char_start"].is_null());

        let unknown = assemble("msg_v1_f", b"not-json", None, None, 5, 4);
        let wire = serde_json::to_value(&unknown).unwrap();
        assert_eq!(wire["precision"], "unknown");
        assert!(wire["byte_start"].is_null());
    }
}
