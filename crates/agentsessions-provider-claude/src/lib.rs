//! Claude Code provider adapter：把 Claude Code 的 JSONL transcript 隔离解析为
//! Canonical 事件流（落实 RFC-0002 的 probe + parse 职责）。
//!
//! 格式（variant `claude-code/jsonl-v1`）：每行一个独立 JSON 对象，形如
//! `{"type":"user"|"assistant"|..., "message":{"role":..,"content":..}}`。
//! adapter 只做格式隔离，绝不接触存储 / 检索 / UI（RFC-0002 §7）。

use agentsessions_ports::{
    CanonicalEventSink, Confidence, MessageEvent, ParseReport, ProbeResult, ProviderAdapter,
    ProviderError,
};
use serde::Deserialize;

/// 本 adapter 认证的 variant 标识。
const VARIANT_ID: &str = "claude-code/jsonl-v1";

/// Claude Code JSONL adapter。无状态——所有解析所需信息都来自输入字节。
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeCodeAdapter;

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        ClaudeCodeAdapter
    }
}

/// 一行 transcript 的最小反序列化视图。
///
/// 只声明判定与抽取所需字段；additive 未知字段被 serde 默认忽略
/// （RFC-0002 §3：additive unknown fields 默认忽略但保留诊断）。
#[derive(Debug, Deserialize)]
struct RawLine {
    /// 顶层记录类型（`user` / `assistant` / `system` / `summary` / 工具类等）。
    #[serde(default)]
    r#type: String,
    /// 该记录的 provider-native id（Claude Code 每条对话记录都带 `uuid`）。
    #[serde(default)]
    uuid: String,
    /// 父记录的 native id（threading 边）；根消息为 null/缺失/空串。
    #[serde(default, rename = "parentUuid")]
    parent_uuid: Option<String>,
    /// ISO-8601 UTC 时间串；缺失则为 None。
    #[serde(default)]
    timestamp: Option<String>,
    /// subagent / 分支标记；缺失视为 false（主线）。
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    /// Claude-generated meta prompts may be copied with enriched content and
    /// a copy-local timestamp while retaining one native message identity.
    #[serde(default, rename = "isMeta")]
    is_meta: bool,
    /// Present on the enriched copied form of Claude meta prompts.
    #[serde(default, rename = "sessionKind")]
    session_kind: Option<String>,
    /// 该 transcript 的 durable 会话 id（Claude Code 每条对话记录都携带）。
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    /// 嵌套的 message 体（对话类记录才有）。
    #[serde(default)]
    message: Option<RawMessage>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    #[serde(default)]
    role: String,
    /// content 可能是字符串，也可能是 content-block 数组——用 untagged 兼容。
    #[serde(default)]
    content: RawContent,
}

/// Claude 的 content 字段有两种形态：纯字符串或 block 数组。
#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
enum RawContent {
    /// 早期/简单形态：直接是字符串。
    Text(String),
    /// 结构化形态：block 数组，每个 block 可能携带 `text`。
    Blocks(Vec<RawBlock>),
    /// 缺失或无法识别——视为空内容。
    #[default]
    Empty,
}

#[derive(Debug, Deserialize)]
struct RawBlock {
    /// Block discriminator used only for narrowly-scoped provider
    /// normalization; unknown block kinds remain non-fatal.
    #[serde(default, rename = "type")]
    kind: String,
    /// 仅抽取带 `text` 的 block（如 `type:"text"`）；工具调用块无 text，忽略。
    #[serde(default)]
    text: Option<String>,
    /// `tool_result` block 的载荷在 `content`（字符串或 text block 数组），
    /// 真实工具输出（文件内容、命令输出）由此携带；缺失则为 None。
    #[serde(default)]
    content: Option<RawContent>,
}

impl RawBlock {
    /// 抽取本 block 的可检索纯文本：`text` 优先，`tool_result` 的 `content`
    /// 其次，否则为空。
    fn plain_text(&self) -> Option<String> {
        if let Some(text) = self.text.as_deref() {
            return Some(text.to_string());
        }
        match &self.content {
            Some(RawContent::Text(s)) => Some(s.clone()),
            Some(RawContent::Blocks(blocks)) => {
                let joined = blocks
                    .iter()
                    .filter_map(|b| b.plain_text())
                    .collect::<Vec<_>>()
                    .join("\n");
                if joined.is_empty() {
                    None
                } else {
                    Some(joined)
                }
            }
            Some(RawContent::Empty) | None => None,
        }
    }
}

impl RawContent {
    /// 抽取可检索纯文本；block 数组按顺序拼接各 text 块。
    fn to_plain_text(&self, is_meta: bool, has_session_kind: bool) -> String {
        match self {
            RawContent::Text(s) => s.clone(),
            RawContent::Blocks(blocks) => {
                if is_meta
                    && has_session_kind
                    && let Some(text) = canonical_enriched_meta_prompt(blocks)
                {
                    return text.to_string();
                }
                let text_blocks = blocks
                    .iter()
                    .filter_map(|b| b.plain_text())
                    .collect::<Vec<_>>();
                if let Some(command) = text_blocks
                    .first()
                    .and_then(|text| canonical_local_command_block(text))
                {
                    return command.to_string();
                }
                text_blocks.join("\n")
            }
            RawContent::Empty => String::new(),
        }
    }
}

/// Claude copies some generated meta prompts as four blocks: two generated
/// text blocks, an image block, then the original stable prompt text. The
/// explicit meta/session markers and exact block shape are required so ordinary
/// multimodal messages keep every text block.
fn canonical_enriched_meta_prompt(blocks: &[RawBlock]) -> Option<&str> {
    match blocks {
        [first, second, image, original]
            if first.kind == "text"
                && second.kind == "text"
                && image.kind == "image"
                && original.kind == "text" =>
        {
            original.text.as_deref()
        }
        _ => None,
    }
}

/// Claude Code may enrich a local-command envelope with generated blocks while
/// retaining the same native message identity. Canonicalize only that shape.
fn canonical_local_command_block(text: &str) -> Option<&str> {
    if !text.starts_with("<command-name>")
        || !text.contains("<command-message>")
        || !text.contains("<command-args>")
    {
        return None;
    }

    Some(text.strip_suffix('\n').unwrap_or(text))
}

/// 判定一行是否是我们承认的对话记录。
///
/// Claude Code also uses `type:"system"` for event records that carry no
/// message body. A message-bearing system row remains compatible with the
/// existing adapter contract, while event-only system rows are metadata.
fn is_conversational(rec: &RawLine) -> bool {
    matches!(rec.r#type.as_str(), "user" | "assistant")
        || (rec.r#type == "system" && rec.message.is_some())
}

impl ProviderAdapter for ClaudeCodeAdapter {
    fn provider_id(&self) -> &str {
        "claude-code"
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
        let mut typed_lines = 0usize;
        let mut conversational = 0usize;
        for line in &sample {
            match serde_json::from_str::<RawLine>(line) {
                Ok(rec) => {
                    json_lines += 1;
                    if !rec.r#type.is_empty() {
                        typed_lines += 1;
                    }
                    if is_conversational(&rec) {
                        conversational += 1;
                    }
                }
                Err(_) => {
                    // 有非 JSON 行 → 不是 JSONL，证据不利。
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

        // 判定置信度：有 type 字段且出现对话类型 → confirmed；
        // 全是 JSON 但无可识别的对话 type → low（可能是别的 JSONL）。
        let confidence = if typed_lines == sample.len() && conversational > 0 {
            matched.push(format!(
                "{typed_lines} lines carry a `type`, {conversational} are conversational"
            ));
            Confidence::Confirmed
        } else if conversational > 0 {
            matched.push(format!("{conversational} conversational lines present"));
            unmatched.push(format!(
                "{} lines lack a `type` field",
                sample.len() - typed_lines
            ));
            Confidence::High
        } else {
            unmatched.push("no conversational (user/assistant/system) records found".into());
            Confidence::Low
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

            // 首个携带 sessionId 的记录确定本 transcript 的 durable 会话 id。
            if report.session_native_id.is_none()
                && let Some(sid) = rec.session_id.as_deref()
                && !sid.trim().is_empty()
            {
                report.session_native_id = Some(sid.trim().to_string());
            }

            // 非对话记录（工具结果、summary 等）不产生 Canonical 消息，静默略过。
            if !is_conversational(&rec) {
                continue;
            }

            let Some(msg) = rec.message else {
                report.skipped += 1;
                report.diagnostics.push(format!(
                    "line {}: conversational record without `message`, skipped",
                    line_no + 1
                ));
                continue;
            };

            let role = if msg.role.is_empty() {
                &rec.r#type
            } else {
                &msg.role
            };
            let body = msg
                .content
                .to_plain_text(rec.is_meta, rec.session_kind.is_some());
            // `isMeta` records are generated/copied by Claude Code. Real corpus
            // copies retain one UUID but report different top-level timestamps,
            // so no stable provider timestamp exists for this entity.
            let timestamp = if rec.is_meta {
                None
            } else {
                rec.timestamp.as_deref()
            };

            sink.emit_message(MessageEvent {
                seq,
                native_id: &rec.uuid,
                // 空串 parentUuid 语义等价于 null（根消息）；透传空串会在
                // 上层派生悬空父边，故归一化为 None。
                parent_native_id: rec.parent_uuid.as_deref().filter(|s| !s.is_empty()),
                role,
                text: &body,
                timestamp,
                is_sidechain: rec.is_sidechain,
                // 该消息来源行在快照字节中的区间（end 排他，不含换行）。
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

    /// 收集 emit 的消息事件，供断言解析结果（含 native 身份/threading）。
    #[derive(Default)]
    struct CollectingSink {
        messages: Vec<Captured>,
    }
    /// 拍平的事件快照（`MessageEvent` 借用输入，测试侧需拥有所有权）。
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

    const SAMPLE: &str = r#"{"type":"user","uuid":"u-1","parentUuid":null,"sessionId":"sess-abc","timestamp":"2026-06-27T13:57:42.685Z","message":{"role":"user","content":"hello there"}}
{"type":"assistant","uuid":"a-2","parentUuid":"u-1","sessionId":"sess-abc","isSidechain":true,"message":{"role":"assistant","content":[{"type":"text","text":"hi"},{"type":"text","text":"friend"}]}}
{"type":"summary","summary":"ignored non-conversational"}"#;

    fn parse_single_content(content: serde_json::Value) -> String {
        let input = serde_json::to_vec(&serde_json::json!({
            "type": "user",
            "uuid": "shared-command-id",
            "sessionId": "synthetic-command-session",
            "message": {
                "role": "user",
                "content": content,
            },
        }))
        .expect("serialize synthetic transcript");
        let mut sink = CollectingSink::default();
        ClaudeCodeAdapter::new()
            .parse(&input, &mut sink)
            .expect("parse synthetic transcript");
        assert_eq!(sink.messages.len(), 1);
        sink.messages.remove(0).text
    }

    #[test]
    fn probe_confirms_claude_jsonl() {
        let r = ClaudeCodeAdapter::new().probe(SAMPLE.as_bytes()).unwrap();
        assert_eq!(r.variant_id, VARIANT_ID);
        assert_eq!(r.confidence, Confidence::Confirmed);
    }

    #[test]
    fn probe_rejects_non_jsonl() {
        let err = ClaudeCodeAdapter::new()
            .probe(b"this is not json\nnor is this")
            .unwrap_err();
        assert!(matches!(err, ProviderError::AmbiguousVariant(_)));
    }

    #[test]
    fn probe_rejects_empty() {
        let err = ClaudeCodeAdapter::new().probe(b"   \n  \n").unwrap_err();
        assert!(matches!(err, ProviderError::AmbiguousVariant(_)));
    }

    #[test]
    fn parse_extracts_conversational_messages() {
        let mut sink = CollectingSink::default();
        let report = ClaudeCodeAdapter::new()
            .parse(SAMPLE.as_bytes(), &mut sink)
            .unwrap();
        // 两条对话消息被提交，summary 行被略过（不计入 skipped，因为它不是错误）。
        assert_eq!(report.committed, 2);
        assert_eq!(sink.messages.len(), 2);
        // seq 从 0 连续。
        assert_eq!(sink.messages[0].seq, 0);
        assert_eq!(sink.messages[1].seq, 1);
        // 字符串 content 与 block 数组 content 都被正确抽取。
        assert_eq!(sink.messages[0].text, "hello there");
        assert_eq!(sink.messages[1].text, "hi\nfriend");
        // 角色标签透传。
        assert_eq!(sink.messages[0].role, "user");
        assert_eq!(sink.messages[1].role, "assistant");
        // native 身份与 threading：uuid 原样透传，parentUuid 形成链，根消息 parent 为 None。
        assert_eq!(sink.messages[0].native_id, "u-1");
        assert_eq!(sink.messages[0].parent_native_id, None);
        assert_eq!(sink.messages[1].native_id, "a-2");
        assert_eq!(sink.messages[1].parent_native_id.as_deref(), Some("u-1"));
        // timestamp 原样透传；缺失为 None。
        assert_eq!(
            sink.messages[0].timestamp.as_deref(),
            Some("2026-06-27T13:57:42.685Z")
        );
        assert_eq!(sink.messages[1].timestamp, None);
        // isSidechain：缺失视为主线 false，显式 true 被捕获。
        assert!(!sink.messages[0].is_sidechain);
        assert!(sink.messages[1].is_sidechain);
    }

    #[test]
    fn parse_canonicalizes_string_and_enriched_local_command_forms() {
        let envelope = "<command-name>synthetic-local</command-name>\n\
                        <command-message>run synthetic local command</command-message>\n\
                        <command-args>--flag value</command-args>";
        let string_form = parse_single_content(serde_json::json!(envelope));
        let enriched_form = parse_single_content(serde_json::json!([
            {"type": "text", "text": format!("{envelope}\n")},
            {"type": "text", "text": "synthetic stdout"},
            {"type": "text", "text": "synthetic generated output"}
        ]));

        assert_eq!(string_form, envelope);
        assert_eq!(enriched_form, string_form);
    }

    #[test]
    fn parse_preserves_ordinary_multiblock_text_and_whitespace() {
        let text = parse_single_content(serde_json::json!([
            {"type": "text", "text": " ordinary first block "},
            {"type": "tool_use", "name": "Synthetic", "input": {}},
            {"type": "text", "text": "ordinary second block\n"}
        ]));

        assert_eq!(text, " ordinary first block \nordinary second block\n");
    }

    #[test]
    fn parse_keeps_genuinely_different_local_command_text_distinct() {
        let first = "<command-name>synthetic-local</command-name>\n\
                     <command-message>run synthetic local command</command-message>\n\
                     <command-args>--flag first</command-args>";
        let second = "<command-name>synthetic-local</command-name>\n\
                      <command-message>run synthetic local command</command-message>\n\
                      <command-args>--flag second</command-args>";
        let string_form = parse_single_content(serde_json::json!(first));
        let enriched_form = parse_single_content(serde_json::json!([
            {"type": "text", "text": format!("{second}\n")},
            {"type": "text", "text": "synthetic stdout"}
        ]));

        assert_eq!(string_form, first);
        assert_eq!(enriched_form, second);
        assert_ne!(string_form, enriched_form);
    }

    #[test]
    fn parse_ignores_event_only_system_records() {
        let input = br#"{"type":"system","subtype":"synthetic-event","uuid":"event-1"}
{"type":"user","uuid":"user-1","message":{"role":"user","content":"kept"}}"#;
        let mut sink = CollectingSink::default();
        let report = ClaudeCodeAdapter::new()
            .parse(input, &mut sink)
            .expect("parse synthetic transcript");

        assert_eq!(report.committed, 1);
        assert_eq!(report.skipped, 0);
        assert_eq!(sink.messages.len(), 1);
        assert_eq!(sink.messages[0].text, "kept");
    }

    #[test]
    fn parse_keeps_message_bearing_system_records() {
        let input = br#"{"type":"system","uuid":"system-1","message":{"role":"system","content":"kept system message"}}"#;
        let mut sink = CollectingSink::default();
        let report = ClaudeCodeAdapter::new()
            .parse(input, &mut sink)
            .expect("parse synthetic transcript");

        assert_eq!(report.committed, 1);
        assert_eq!(report.skipped, 0);
        assert_eq!(sink.messages.len(), 1);
        assert_eq!(sink.messages[0].role, "system");
        assert_eq!(sink.messages[0].text, "kept system message");
    }

    #[test]
    fn parse_still_skips_user_records_without_messages() {
        let input = br#"{"type":"user","uuid":"user-without-message"}"#;
        let mut sink = CollectingSink::default();
        let report = ClaudeCodeAdapter::new()
            .parse(input, &mut sink)
            .expect("parse synthetic transcript");

        assert_eq!(report.committed, 0);
        assert_eq!(report.skipped, 1);
        assert!(sink.messages.is_empty());
    }

    #[test]
    fn parse_canonicalizes_original_and_enriched_meta_prompt_forms() {
        let original = serde_json::json!({
            "type": "user",
            "uuid": "shared-meta-id",
            "isMeta": true,
            "timestamp": "2026-01-01T00:04:00Z",
            "message": {
                "role": "user",
                "content": [{"type": "text", "text": "stable synthetic meta prompt"}],
            },
        });
        let enriched = serde_json::json!({
            "type": "user",
            "uuid": "shared-meta-id",
            "isMeta": true,
            "sessionKind": "synthetic-copy",
            "timestamp": "2026-01-01T00:00:00Z",
            "message": {
                "role": "user",
                "content": [
                    {"type": "text", "text": "synthetic generated prefix"},
                    {"type": "text", "text": "synthetic generated instructions"},
                    {"type": "image", "source": {"type": "base64", "data": "AA=="}},
                    {"type": "text", "text": "stable synthetic meta prompt"},
                ],
            },
        });

        let parse = |record: serde_json::Value| {
            let input = serde_json::to_vec(&record).expect("serialize synthetic transcript");
            let mut sink = CollectingSink::default();
            ClaudeCodeAdapter::new()
                .parse(&input, &mut sink)
                .expect("parse synthetic transcript");
            assert_eq!(sink.messages.len(), 1);
            sink.messages.remove(0)
        };
        let original = parse(original);
        let enriched = parse(enriched);

        assert_eq!(original.text, "stable synthetic meta prompt");
        assert_eq!(enriched.text, original.text);
        assert_eq!(original.timestamp, None);
        assert_eq!(enriched.timestamp, None);
    }

    #[test]
    fn parse_preserves_similar_multimodal_content_without_meta_markers() {
        let input = serde_json::to_vec(&serde_json::json!({
            "type": "user",
            "uuid": "ordinary-multimodal-id",
            "message": {
                "role": "user",
                "content": [
                    {"type": "text", "text": "ordinary first"},
                    {"type": "text", "text": "ordinary second"},
                    {"type": "image", "source": {"type": "base64", "data": "AA=="}},
                    {"type": "text", "text": "ordinary last"},
                ],
            },
        }))
        .expect("serialize synthetic transcript");
        let mut sink = CollectingSink::default();
        ClaudeCodeAdapter::new()
            .parse(&input, &mut sink)
            .expect("parse synthetic transcript");

        assert_eq!(sink.messages.len(), 1);
        assert_eq!(
            sink.messages[0].text,
            "ordinary first\nordinary second\nordinary last"
        );
    }

    #[test]
    fn parse_preserves_meta_content_when_enrichment_shape_is_not_exact() {
        let input = serde_json::to_vec(&serde_json::json!({
            "type": "user",
            "uuid": "different-meta-id",
            "isMeta": true,
            "sessionKind": "synthetic-copy",
            "message": {
                "role": "user",
                "content": [
                    {"type": "text", "text": "meaningful first"},
                    {"type": "image", "source": {"type": "base64", "data": "AA=="}},
                    {"type": "text", "text": "meaningful last"},
                ],
            },
        }))
        .expect("serialize synthetic transcript");
        let mut sink = CollectingSink::default();
        ClaudeCodeAdapter::new()
            .parse(&input, &mut sink)
            .expect("parse synthetic transcript");

        assert_eq!(sink.messages.len(), 1);
        assert_eq!(sink.messages[0].text, "meaningful first\nmeaningful last");
    }

    #[test]
    fn parse_skips_broken_line_recoverably() {
        let input = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"ok\"}}\n{not valid json}";
        let mut sink = CollectingSink::default();
        let report = ClaudeCodeAdapter::new()
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
        ClaudeCodeAdapter::new().parse(bytes, &mut sink).unwrap();
        // 每条消息的 span 切回快照字节，必须精确等于其来源行。
        let lines: Vec<&str> = SAMPLE.lines().collect();
        for (captured, expected_line) in sink.messages.iter().zip([lines[0], lines[1]]) {
            let (start, end) = captured.span.expect("provider must report a span");
            assert_eq!(
                &bytes[start as usize..end as usize],
                expected_line.as_bytes()
            );
        }
    }

    #[test]
    fn parse_reports_span_roundtripping_on_crlf_lines() {
        // Windows 真实 transcript 常见 CRLF；span 必须不含 `\r`/`\n`，
        // 切回快照字节应等于去掉行尾换行后的记录正文。
        let line = r#"{"type":"user","uuid":"crlf-1","sessionId":"sess-crlf","message":{"role":"user","content":"crlf span"}}"#;
        let bytes = format!("{line}\r\n").into_bytes();
        let mut sink = CollectingSink::default();
        ClaudeCodeAdapter::new().parse(&bytes, &mut sink).unwrap();
        assert_eq!(sink.messages.len(), 1);
        let (start, end) = sink.messages[0].span.expect("span required");
        assert_eq!(&bytes[start as usize..end as usize], line.as_bytes());
        // end 排他：下一字节是 `\r`（CRLF 的 CR），不在 span 内。
        assert_eq!(bytes[end as usize], b'\r');
    }

    #[test]
    fn parse_surfaces_session_native_id() {
        let mut sink = CollectingSink::default();
        let report = ClaudeCodeAdapter::new()
            .parse(SAMPLE.as_bytes(), &mut sink)
            .unwrap();
        assert_eq!(report.session_native_id.as_deref(), Some("sess-abc"));
    }

    #[test]
    fn parse_without_session_id_reports_none() {
        let input = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"ok\"}}";
        let mut sink = CollectingSink::default();
        let report = ClaudeCodeAdapter::new()
            .parse(input.as_bytes(), &mut sink)
            .unwrap();
        // provider 未提供 sessionId → None，显式缺失不臆造。
        assert_eq!(report.session_native_id, None);
    }
}
