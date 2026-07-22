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
    /// 父记录的 native id（threading 边）；根消息为 null/缺失。
    #[serde(default, rename = "parentUuid")]
    parent_uuid: Option<String>,
    /// ISO-8601 UTC 时间串；缺失则为 None。
    #[serde(default)]
    timestamp: Option<String>,
    /// subagent / 分支标记；缺失视为 false（主线）。
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
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
    /// 仅抽取带 `text` 的 block（如 `type:"text"`）；工具调用块无 text，忽略。
    #[serde(default)]
    text: Option<String>,
}

impl RawContent {
    /// 抽取可检索纯文本；block 数组按顺序拼接各 text 块。
    fn to_plain_text(&self) -> String {
        match self {
            RawContent::Text(s) => s.clone(),
            RawContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| b.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n"),
            RawContent::Empty => String::new(),
        }
    }
}

/// 判定一行是否是我们承认的对话记录类型。
fn is_conversational(kind: &str) -> bool {
    matches!(kind, "user" | "assistant" | "system")
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
                    if is_conversational(&rec.r#type) {
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

        for (line_no, line) in text.lines().enumerate() {
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

            // 非对话记录（工具结果、summary 等）不产生 Canonical 消息，静默略过。
            if !is_conversational(&rec.r#type) {
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
            let body = msg.content.to_plain_text();

            sink.emit_message(MessageEvent {
                seq,
                native_id: &rec.uuid,
                parent_native_id: rec.parent_uuid.as_deref(),
                role,
                text: &body,
                timestamp: rec.timestamp.as_deref(),
                is_sidechain: rec.is_sidechain,
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
            });
            Ok(())
        }
    }

    const SAMPLE: &str = r#"{"type":"user","uuid":"u-1","parentUuid":null,"timestamp":"2026-06-27T13:57:42.685Z","message":{"role":"user","content":"hello there"}}
{"type":"assistant","uuid":"a-2","parentUuid":"u-1","isSidechain":true,"message":{"role":"assistant","content":[{"type":"text","text":"hi"},{"type":"text","text":"friend"}]}}
{"type":"summary","summary":"ignored non-conversational"}"#;

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
}
