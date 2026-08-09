//! Codex provider adapter：把 Codex CLI 的 rollout JSONL 隔离解析为 Canonical
//! 事件流（落实 RFC-0002 的 probe + parse 职责）。
//!
//! 格式（variant `codex/rollout-jsonl-v1`）：每行一个独立 JSON 对象，统一封套
//! `{"timestamp":..,"type":..,"payload":{..}}`。`type` 取值有 `session_meta` /
//! `event_msg` / `response_item` / `turn_context` / `world_state` 等。
//!
//! **权威对话记录**是 `type:"response_item"` 且 `payload.type:"message"`——它带
//! provider-native `payload.id`、`payload.role`（developer/user/assistant）与
//! `payload.content[]`（block 数组，每块 `{type,text}`）。`event_msg` 的
//! `user_message`/`agent_message` 是同一消息的 UI 镜像（文本重复、无 id），
//! **必须忽略以免重复计数**（真实样本实测：每条对话在两处各出现一次）。
//!
//! 与 Claude Code 的差异：对话字段在 `payload` 封套内（非顶层）；无 `parentUuid`
//! ——Codex rollout 是线性序列，不提供显式 threading 边，故 parent 一律 `None`
//! （诚实：不编造上层可推断的线性链）。外层时间戳是 source occurrence 元数据，
//! 同一 native message 的复制记录可能不同，因此不进入稳定 Message 投影。
//!
//! adapter 只做格式隔离，绝不接触存储 / 检索 / UI（RFC-0002 §7）。

use agentsessions_ports::{
    CanonicalEventSink, Confidence, MessageEvent, ParseReport, ProbeResult, ProviderAdapter,
    ProviderError,
};
use serde::Deserialize;

/// 本 adapter 认证的 variant 标识。
const VARIANT_ID: &str = "codex/rollout-jsonl-v1";

/// Codex rollout JSONL adapter。无状态——所有解析所需信息都来自输入字节。
#[derive(Debug, Default, Clone, Copy)]
pub struct CodexAdapter;

impl CodexAdapter {
    pub fn new() -> Self {
        CodexAdapter
    }
}

/// 一行 rollout 的最小反序列化视图（统一封套）。
///
/// 只声明判定与抽取所需字段；additive 未知字段被 serde 默认忽略
/// （RFC-0002 §3：additive unknown fields 默认忽略但保留诊断）。
#[derive(Debug, Deserialize)]
struct RawLine {
    /// 外层封套时间戳（ISO-8601 UTC）；仅用于识别 rollout 封套形态。
    ///
    /// 该值属于 source occurrence，同一 native message 的复制记录可能不同，
    /// 因此不能作为稳定 Message 字段。
    #[serde(default)]
    timestamp: Option<String>,
    /// 顶层记录类型（`session_meta` / `event_msg` / `response_item` / …）。
    #[serde(default)]
    r#type: String,
    /// 类型相关的载荷；对话记录才含 role/content。
    #[serde(default)]
    payload: Option<RawPayload>,
}

/// `payload` 的最小视图。因各 `type` 的 payload 结构不同，只声明对话消息
/// （`response_item` + 内层 `message`）所需字段；其它类型缺失这些字段无妨。
#[derive(Debug, Deserialize)]
struct RawPayload {
    /// 内层记录类型（`message` / `reasoning` / `custom_tool_call` / …）。
    #[serde(default)]
    r#type: String,
    /// provider-native 消息 id（`response_item/message` 才有）。
    #[serde(default)]
    id: String,
    /// 角色（`developer` / `user` / `assistant`）。
    #[serde(default)]
    role: String,
    /// Message 的 content-block 数组。非 message 的 response_item（例如
    /// reasoning）可能显式写入 null，必须先按 payload type 分类再解释。
    #[serde(default)]
    content: Option<Vec<RawBlock>>,
    /// durable 会话 id（仅 `session_meta` 的 payload 携带）。
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawBlock {
    /// 仅抽取带 `text` 的 block（如 `input_text` / `output_text`）；
    /// 工具调用等无 text 的 block 被忽略。
    #[serde(default)]
    text: Option<String>,
}

impl RawPayload {
    /// 抽取可检索纯文本；按顺序拼接各 block 的 text。
    fn to_plain_text(&self) -> Option<String> {
        self.content.as_ref().map(|content| {
            content
                .iter()
                .filter_map(|b| b.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
}

/// 判定一个角色是否是我们承认的对话角色。
///
/// `developer` 是 Codex 注入的系统提示层（≈ system）；一并保留，让上层按 role
/// 过滤，adapter 不做语义裁剪（RFC-0002 §7：provider 只做格式隔离）。
fn is_conversational_role(role: &str) -> bool {
    matches!(role, "user" | "assistant" | "developer" | "system")
}

/// Codex 已知的顶层封套类型——probe 判定用。
fn is_known_envelope_type(kind: &str) -> bool {
    matches!(
        kind,
        "session_meta"
            | "event_msg"
            | "response_item"
            | "turn_context"
            | "world_state"
            | "compacted"
    )
}

impl ProviderAdapter for CodexAdapter {
    fn provider_id(&self) -> &str {
        "codex"
    }

    fn probe(&self, bytes: &[u8]) -> Result<ProbeResult, ProviderError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| ProviderError::StructuralFatal(format!("not valid UTF-8: {e}")))?;

        let mut matched = Vec::new();
        let mut unmatched = Vec::new();

        // 取前若干非空行做判定，避免整体加载（RFC-0002 §7 bounded）。
        let sample: Vec<&str> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(16)
            .collect();

        if sample.is_empty() {
            return Err(ProviderError::AmbiguousVariant(
                "empty input: no non-blank lines to probe".into(),
            ));
        }

        let mut json_lines = 0usize;
        let mut enveloped = 0usize;
        let mut timestamped = 0usize;
        let mut has_session_meta = false;
        let mut has_message = false;
        for line in &sample {
            match serde_json::from_str::<RawLine>(line) {
                Ok(rec) => {
                    json_lines += 1;
                    // Codex 封套的正信号：payload 存在 + type 属于已知集合。
                    if rec.payload.is_some() && is_known_envelope_type(&rec.r#type) {
                        enveloped += 1;
                        if rec.timestamp.is_some() {
                            timestamped += 1;
                        }
                    }
                    if rec.r#type == "session_meta" {
                        has_session_meta = true;
                    }
                    if rec.r#type == "response_item"
                        && rec.payload.as_ref().map(|p| p.r#type.as_str()) == Some("message")
                    {
                        has_message = true;
                    }
                }
                Err(_) => {
                    unmatched.push("found a non-JSON line".into());
                }
            }
        }

        // 每行都是 JSON 才可能是 JSONL；否则拒绝。
        if json_lines != sample.len() {
            return Err(ProviderError::AmbiguousVariant(format!(
                "not line-delimited JSON: {}/{} sampled lines parsed",
                json_lines,
                sample.len()
            )));
        }
        matched.push(format!("{json_lines} sampled lines are valid JSON objects"));

        // 无任何 Codex 封套结构 → 不是本 variant（可能是别的 JSONL，如 Claude Code）。
        if enveloped == 0 {
            return Err(ProviderError::AmbiguousVariant(
                "no Codex envelope records ({timestamp,type,payload}) found".into(),
            ));
        }
        matched.push(format!("{enveloped} lines carry a Codex envelope"));
        if timestamped > 0 {
            matched.push(format!(
                "{timestamped} Codex envelopes carry an outer occurrence timestamp"
            ));
        }

        // session_meta（会话头）或 response_item/message（权威对话）任一出现即高置信。
        let confidence = if has_session_meta || has_message {
            if has_session_meta {
                matched.push("session_meta header present".into());
            }
            if has_message {
                matched.push("response_item/message records present".into());
            }
            Confidence::Confirmed
        } else {
            unmatched.push("no session_meta or response_item/message in sample".into());
            Confidence::High
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
        let text = std::str::from_utf8(bytes)
            .map_err(|e| ProviderError::StructuralFatal(format!("not valid UTF-8: {e}")))?;

        let mut report = ParseReport::default();
        // seq 是会话内单调序号，只对成功 emit 的对话消息递增，
        // 从而满足 domain Session 的 seq 从 0 连续的不变量。
        let mut seq: u32 = 0;
        // 手动累计行首偏移：span 以快照字节为坐标系，end 排他且不含换行符。
        let mut offset: u64 = 0;

        for (line_no, raw_line) in text.split_inclusive('\n').enumerate() {
            let start = offset;
            offset += raw_line.len() as u64;
            // 去掉行尾 `\n` / `\r\n`——与 `str::lines` 的行语义一致。
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let end = start + line.len() as u64;
            if line.trim().is_empty() {
                continue;
            }
            let rec: RawLine = match serde_json::from_str(line) {
                Ok(r) => r,
                Err(e) => {
                    // 单行 JSON 破损属于 record_recoverable：跳过并标记，不整体失败。
                    report.skipped += 1;
                    report
                        .diagnostics
                        .push(format!("line {}: invalid JSON, skipped ({e})", line_no + 1));
                    continue;
                }
            };

            // session_meta 头携带 durable 会话 id——上报后该行不产生 Canonical 消息。
            if rec.r#type == "session_meta" {
                if report.session_native_id.is_none()
                    && let Some(sid) = rec.payload.as_ref().and_then(|p| p.session_id.as_deref())
                    && !sid.trim().is_empty()
                {
                    report.session_native_id = Some(sid.trim().to_string());
                }
                continue;
            }

            // 只认权威对话记录：response_item + 内层 message。event_msg 的对话镜像
            // （user_message/agent_message）在此被静默略过，避免同一消息重复计数。
            if rec.r#type != "response_item" {
                continue;
            }
            let Some(payload) = rec.payload else {
                continue;
            };
            if payload.r#type != "message" {
                continue;
            }
            if !is_conversational_role(&payload.role) {
                // An authoritative conversation occurrence with an unknown or
                // empty role cannot be emitted; count it as a recoverable skip
                // like the null-content path below, never silently.
                report.skipped += 1;
                report.diagnostics.push(format!(
                    "line {}: response message with unknown role {:?}, skipped",
                    line_no + 1,
                    payload.role
                ));
                continue;
            }

            let Some(body) = payload.to_plain_text() else {
                report.skipped += 1;
                report.diagnostics.push(format!(
                    "line {}: response message without content array, skipped",
                    line_no + 1
                ));
                continue;
            };

            sink.emit_message(MessageEvent {
                seq,
                native_id: &payload.id,
                // Codex rollout 不提供显式父指针，线性序列的 threading 由上层推断。
                parent_native_id: None,
                role: &payload.role,
                text: &body,
                // The outer envelope timestamp is occurrence-local. Real
                // cross-source copies retain one native id and stable content
                // while carrying different envelope timestamps, so no stable
                // provider timestamp exists for this Message entity.
                timestamp: None,
                is_sidechain: false,
                // 该消息来源封套行在快照字节中的区间（end 排他，不含换行）。
                span: Some((start, end)),
            })
            .map_err(|e| ProviderError::StructuralFatal(e.to_string()))?;
            seq += 1;
            report.committed += 1;
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 收集 emit 的消息事件，供断言解析结果（含 native 身份/时间）。
    #[derive(Default)]
    struct CollectingSink {
        messages: Vec<Captured>,
    }
    struct Captured {
        seq: u32,
        native_id: String,
        parent_native_id: Option<String>,
        role: String,
        text: String,
        timestamp: Option<String>,
        is_sidechain: bool,
        span: Option<(u64, u64)>,
    }
    impl CanonicalEventSink for CollectingSink {
        fn emit_message(&mut self, event: MessageEvent<'_>) -> agentsessions_ports::PortResult<()> {
            self.messages.push(Captured {
                seq: event.seq,
                native_id: event.native_id.to_string(),
                parent_native_id: event.parent_native_id.map(str::to_string),
                role: event.role.to_string(),
                text: event.text.to_string(),
                timestamp: event.timestamp.map(str::to_string),
                is_sidechain: event.is_sidechain,
                span: event.span,
            });
            Ok(())
        }
    }

    // 合成的 Codex rollout 片段（按 R0 脱敏规范，非真实 transcript）：
    // session_meta 头 + 一条 event_msg 对话镜像 + 两条权威 response_item/message。
    // 关键：user 消息在 event_msg 与 response_item 各出现一次，adapter 只应计一次。
    const SAMPLE: &str = r#"{"timestamp":"2026-07-19T23:40:00.000Z","type":"session_meta","payload":{"session_id":"019f7b08","cwd":"/tmp"}}
{"timestamp":"2026-07-19T23:40:01.000Z","type":"event_msg","payload":{"type":"user_message","message":"how do I configure the sandbox"}}
{"timestamp":"2026-07-19T23:40:01.000Z","type":"response_item","payload":{"type":"message","id":"msg_u1","role":"user","content":[{"type":"input_text","text":"how do I configure the sandbox"}]}}
{"timestamp":"2026-07-19T23:40:02.500Z","type":"response_item","payload":{"type":"message","id":"msg_a1","role":"assistant","content":[{"type":"output_text","text":"set the policy"},{"type":"output_text","text":"in config.toml"}]}}
{"timestamp":"2026-07-19T23:40:02.000Z","type":"response_item","payload":{"type":"reasoning","id":"rs_1","summary":[]}}"#;

    #[test]
    fn probe_confirms_codex_rollout() {
        let r = CodexAdapter::new().probe(SAMPLE.as_bytes()).unwrap();
        assert_eq!(r.variant_id, VARIANT_ID);
        assert_eq!(r.confidence, Confidence::Confirmed);
    }

    #[test]
    fn probe_rejects_non_jsonl() {
        let err = CodexAdapter::new()
            .probe(b"this is not json\nnor is this")
            .unwrap_err();
        assert!(matches!(err, ProviderError::AmbiguousVariant(_)));
    }

    #[test]
    fn probe_rejects_empty() {
        let err = CodexAdapter::new().probe(b"   \n  \n").unwrap_err();
        assert!(matches!(err, ProviderError::AmbiguousVariant(_)));
    }

    #[test]
    fn probe_rejects_claude_code_jsonl() {
        // Claude Code 行无 payload 封套——本 adapter 必须拒绝，不越界解析。
        let claude = r#"{"type":"user","uuid":"u-1","message":{"role":"user","content":"hi"}}
{"type":"assistant","uuid":"a-1","message":{"role":"assistant","content":"yo"}}"#;
        let err = CodexAdapter::new().probe(claude.as_bytes()).unwrap_err();
        assert!(matches!(err, ProviderError::AmbiguousVariant(_)));
    }

    #[test]
    fn parse_takes_authoritative_message_ignores_event_mirror() {
        let mut sink = CollectingSink::default();
        let report = CodexAdapter::new()
            .parse(SAMPLE.as_bytes(), &mut sink)
            .unwrap();
        // 只有两条 response_item/message 被提交：event_msg 镜像与 reasoning 被略过。
        assert_eq!(report.committed, 2);
        assert_eq!(sink.messages.len(), 2);
        // seq 从 0 连续。
        assert_eq!(sink.messages[0].seq, 0);
        assert_eq!(sink.messages[1].seq, 1);
        // native id 原样透传。
        assert_eq!(sink.messages[0].native_id, "msg_u1");
        assert_eq!(sink.messages[1].native_id, "msg_a1");
        // role 原样透传。
        assert_eq!(sink.messages[0].role, "user");
        assert_eq!(sink.messages[1].role, "assistant");
        // content block 数组按序拼接。
        assert_eq!(sink.messages[0].text, "how do I configure the sandbox");
        assert_eq!(sink.messages[1].text, "set the policy\nin config.toml");
        // 外层封套时间戳属于 source occurrence，不进入稳定 Message。
        assert_eq!(sink.messages[0].timestamp, None);
        // Codex 无显式父指针 / sidechain。
        assert_eq!(sink.messages[0].parent_native_id, None);
        assert!(!sink.messages[0].is_sidechain);
    }

    #[test]
    fn parse_ignores_reasoning_with_null_content_without_skip() {
        let input = br#"{"timestamp":"2026-07-19T23:40:02.000Z","type":"response_item","payload":{"type":"reasoning","id":"rs-null","content":null}}
{"timestamp":"2026-07-19T23:40:03.000Z","type":"response_item","payload":{"type":"message","id":"msg-kept","role":"assistant","content":[{"type":"output_text","text":"kept"}]}}"#;
        let mut sink = CollectingSink::default();
        let report = CodexAdapter::new()
            .parse(input, &mut sink)
            .expect("parse synthetic rollout");

        assert_eq!(report.committed, 1);
        assert_eq!(report.skipped, 0);
        assert!(report.diagnostics.is_empty());
        assert_eq!(sink.messages.len(), 1);
        assert_eq!(sink.messages[0].text, "kept");
    }

    #[test]
    fn parse_skips_message_with_null_content_recoverably() {
        let input = br#"{"timestamp":"2026-07-19T23:40:02.000Z","type":"response_item","payload":{"type":"message","id":"msg-null","role":"assistant","content":null}}"#;
        let mut sink = CollectingSink::default();
        let report = CodexAdapter::new()
            .parse(input, &mut sink)
            .expect("parse synthetic rollout");

        assert_eq!(report.committed, 0);
        assert_eq!(report.skipped, 1);
        assert_eq!(report.diagnostics.len(), 1);
        assert!(sink.messages.is_empty());
    }

    fn parse_single_message(timestamp: &str, role: &str, text: &str) -> Captured {
        let input = serde_json::to_vec(&serde_json::json!({
            "timestamp": timestamp,
            "type": "response_item",
            "payload": {
                "type": "message",
                "id": "shared-synthetic-id",
                "role": role,
                "content": [{"type": "input_text", "text": text}],
            },
        }))
        .expect("serialize synthetic rollout");
        let mut sink = CollectingSink::default();
        CodexAdapter::new()
            .parse(&input, &mut sink)
            .expect("parse synthetic rollout");
        assert_eq!(sink.messages.len(), 1);
        sink.messages.remove(0)
    }

    #[test]
    fn parse_keeps_stable_projection_equal_across_occurrence_timestamps() {
        let first = parse_single_message(
            "2026-07-19T23:40:01.000Z",
            "user",
            "stable synthetic content",
        );
        let copied = parse_single_message(
            "2026-07-20T10:15:30.000Z",
            "user",
            "stable synthetic content",
        );

        assert_eq!(first.native_id, copied.native_id);
        assert_eq!(first.role, copied.role);
        assert_eq!(first.text, copied.text);
        assert_eq!(first.timestamp, None);
        assert_eq!(copied.timestamp, None);
    }

    #[test]
    fn parse_preserves_genuine_role_and_text_differences() {
        let first = parse_single_message(
            "2026-07-19T23:40:01.000Z",
            "user",
            "first synthetic content",
        );
        let changed = parse_single_message(
            "2026-07-20T10:15:30.000Z",
            "assistant",
            "changed synthetic content",
        );

        assert_eq!(first.native_id, changed.native_id);
        assert_ne!((first.role, first.text), (changed.role, changed.text));
        assert_eq!(first.timestamp, None);
        assert_eq!(changed.timestamp, None);
    }

    #[test]
    fn parse_skips_broken_line_recoverably() {
        let input = "{\"timestamp\":\"t\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"m1\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"ok\"}]}}\n{not valid json}";
        let mut sink = CollectingSink::default();
        let report = CodexAdapter::new()
            .parse(input.as_bytes(), &mut sink)
            .unwrap();
        assert_eq!(report.committed, 1);
        assert_eq!(report.skipped, 1);
        assert_eq!(report.diagnostics.len(), 1);
    }

    #[test]
    fn parse_reports_span_roundtripping_to_source_line() {
        let bytes = SAMPLE.as_bytes();
        let mut sink = CollectingSink::default();
        CodexAdapter::new().parse(bytes, &mut sink).unwrap();
        // 每条消息的 span 切回快照字节，必须精确等于其来源封套行。
        let lines: Vec<&str> = SAMPLE.lines().collect();
        // 两条权威消息分别来自第 3、4 行（0 基：2、3）。
        for (captured, expected_line) in sink.messages.iter().zip([lines[2], lines[3]]) {
            let (start, end) = captured.span.expect("provider must report a span");
            assert_eq!(
                &bytes[start as usize..end as usize],
                expected_line.as_bytes()
            );
        }
    }

    #[test]
    fn parse_reports_span_roundtripping_on_crlf_lines() {
        // Windows CRLF rollout：span 不含 `\r`/`\n`，切回快照字节等于记录正文。
        let line = r#"{"timestamp":"2026-07-19T15:41:00.000Z","type":"response_item","payload":{"type":"message","id":"crlf-1","role":"user","content":[{"type":"input_text","text":"crlf span"}]}}"#;
        let bytes = format!("{line}\r\n").into_bytes();
        let mut sink = CollectingSink::default();
        CodexAdapter::new().parse(&bytes, &mut sink).unwrap();
        assert_eq!(sink.messages.len(), 1);
        let (start, end) = sink.messages[0].span.expect("span required");
        assert_eq!(&bytes[start as usize..end as usize], line.as_bytes());
        // end 排他：下一字节是 `\r`（CRLF 的 CR），不在 span 内。
        assert_eq!(bytes[end as usize], b'\r');
    }

    #[test]
    fn parse_surfaces_session_native_id_from_session_meta() {
        let mut sink = CollectingSink::default();
        let report = CodexAdapter::new()
            .parse(SAMPLE.as_bytes(), &mut sink)
            .unwrap();
        assert_eq!(report.session_native_id.as_deref(), Some("019f7b08"));
        // session_meta 行本身不产生消息，镜像忽略行为不变。
        assert_eq!(report.committed, 2);
    }

    #[test]
    fn parse_without_session_meta_reports_none() {
        let input = "{\"timestamp\":\"t\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"m1\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"ok\"}]}}";
        let mut sink = CollectingSink::default();
        let report = CodexAdapter::new()
            .parse(input.as_bytes(), &mut sink)
            .unwrap();
        // 无 session_meta → None，显式缺失不臆造。
        assert_eq!(report.session_native_id, None);
    }
}
