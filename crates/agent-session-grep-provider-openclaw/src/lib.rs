//! OpenClaw coding agent provider adapter.
//!
//! Parses OpenClaw's v3 session JSONL format: each line carries a `type`
//! discriminator. `type: "session"` is the header (carries `id`, `cwd`,
//! `timestamp`); `type: "message"` wraps `message.role` (user/assistant) and
//! `message.content`.
//!
//! Source root: `~/.openclaw/agents/<agent>/sessions/*.jsonl`
//! (legacy `~/.clawdbot`; `OPENCLAW_STATE_DIR` may relocate the root).
//! Identity: the header `id` field. Resume is intentionally unsupported
//! (gateway-managed) — the adapter never emits a resume command.
//!
//! Format evidence: OpenClaw v3 JSONL (header
//! `{type:session,id,timestamp,cwd}` + `{type:message,...}`). The type-based
//! dispatch and content extraction follow the same v3 JSONL shape as the Pi
//! adapter (`agent-session-grep-provider-pi`), which is adapted from
//! fast-resume (MIT).

use agent_session_grep_ports::MetadataResolution;
use agent_session_grep_ports::{
    AdapterManifest, CanonicalEventSink, Confidence, MessageEvent, ParseReport, ProbeResult,
    ProviderAdapter, ProviderError, manifest_for,
};

/// Variant id surfaced in probe results.
const VARIANT_ID: &str = "openclaw/session-jsonl-v3";

/// Upper bound on non-blank lines scanned during probe (RFC-0002 §7: the probe
/// is bounded and never reads the whole file).
///
/// The scan stops as soon as a session header and a conversational message have
/// both been seen, so a typical transcript costs only a handful of lines; this
/// cap only binds when the first message sits unusually deep.
///
/// An OpenClaw v3 session opens with the `session` header followed by a run of
/// `model_change` / `thinking_level_change` / `custom` records, so the first
/// `message` can sit past the first several lines. The previous limit of 8 lines
/// closed the window before that evidence was in hand, leaving the probe at
/// `High` instead of `Confirmed` while the synthetic fixture (message
/// immediately after the header) passed.
const SCAN_LINE_LIMIT: usize = 64;

/// OpenClaw coding agent adapter: parses v3 session JSONL with `type`-discriminated records.
pub struct OpenClawAdapter;

impl OpenClawAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenClawAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimal deserialization target for an OpenClaw JSONL record.
///
/// Only fields needed for probe/parse are modeled; unknown fields are silently
/// ignored (forward-compatible).
#[derive(serde::Deserialize)]
struct OpenClawRecord {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    message: Option<OpenClawMessage>,
}

#[derive(serde::Deserialize)]
struct OpenClawMessage {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    timestamp: Option<serde_json::Value>,
}

impl ProviderAdapter for OpenClawAdapter {
    fn provider_id(&self) -> &str {
        "openclaw"
    }

    fn manifest(&self) -> AdapterManifest {
        manifest_for(
            self.provider_id(),
            Some(1),
            &[
                "resume is intentionally unsupported (gateway-managed)",
                "the format carries no per-message native id, so message identity is reconstructed document-scoped by the ingestion layer (Unstable)",
            ],
        )
    }

    fn probe(&self, bytes: &[u8]) -> Result<ProbeResult, ProviderError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| ProviderError::StructuralFatal(format!("not valid UTF-8: {e}")))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);

        let mut matched = Vec::new();
        let mut unmatched = Vec::new();

        let mut scanned = 0usize;
        let mut json_lines = 0usize;
        let mut session_headers = 0usize;
        let mut message_records = 0usize;
        let mut conversational = 0usize;

        for (idx, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if scanned == SCAN_LINE_LIMIT {
                break;
            }
            scanned += 1;
            let line_no = idx + 1;
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(v) => {
                    json_lines += 1;
                    let t = v
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    if t == "session" {
                        session_headers += 1;
                    }
                    if t == "message" {
                        message_records += 1;
                        let role = v
                            .pointer("/message/role")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("");
                        if matches!(role, "user" | "assistant") {
                            conversational += 1;
                        }
                    }
                }
                Err(_) => {
                    unmatched.push(format!("line {line_no}: not valid JSON"));
                }
            }
            // The `Confirmed` condition below is satisfied; further lines cannot
            // change the verdict, so stop rather than scan to the cap.
            if conversational > 0 && session_headers > 0 {
                break;
            }
        }

        if scanned == 0 {
            return Err(ProviderError::AmbiguousVariant(
                "empty input: no non-blank lines to probe".into(),
            ));
        }

        if json_lines == 0 {
            return Err(ProviderError::AmbiguousVariant(format!(
                "no JSON lines parsed in {scanned} sampled lines"
            )));
        }

        matched.push(format!("{json_lines} sampled lines are valid JSON"));

        // OpenClaw v3 format is distinct: type=session header + type=message
        // records with nested message.role. Refuse if no OpenClaw-specific type
        // markers present — a Low confidence would compete with Pi (same shape)
        // and other type-discriminated JSONL adapters, causing ambiguous
        // selection. The variant id disambiguates at parse time.
        if session_headers == 0 && message_records == 0 {
            return Err(ProviderError::AmbiguousVariant(
                "no OpenClaw session/message type records found in sampled lines".into(),
            ));
        }

        let confidence = if conversational > 0 && session_headers > 0 {
            matched.push(format!(
                "{session_headers} session headers, {conversational} conversational messages"
            ));
            Confidence::Confirmed
        } else if conversational > 0 || message_records > 0 {
            matched.push(format!("{message_records} message records found"));
            Confidence::High
        } else {
            matched.push(format!("{session_headers} session headers found"));
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
        // 字节兼容路径：把整段字节包成只读切片源，与生产流式路径共用同一实现。
        let source = agent_session_grep_ports::SliceSource::new(bytes);
        self.parse_source(&source, sink)
    }

    fn parse_source(
        &self,
        source: &dyn agent_session_grep_ports::ReadOnlySource,
        sink: &mut dyn CanonicalEventSink,
    ) -> Result<ParseReport, ProviderError> {
        // 流式逐行读取：内存上界是单条记录（manifest max_record_size），
        // 不是文件大小（RFC-0002 §7）。
        let mut lines = agent_session_grep_ports::BoundedLineReader::new(
            source,
            agent_session_grep_ports::STREAM_RECORD_MAX_BYTES,
        )
        .map_err(agent_session_grep_ports::provider_error_from_port)?;

        let mut report = ParseReport::default();
        let mut seq: u32 = 0;
        let mut session_ids: Vec<String> = Vec::new();

        while let Some(line) = lines.next_record()? {
            // 行负载已由 BoundedLineReader 剥离 \n/\r 与首行 BOM，span 仍以
            // 快照字节为坐标系（start/end 与整段 parse 逐字节一致）。
            let parse_line = std::str::from_utf8(line.bytes)
                .map_err(|e| ProviderError::StructuralFatal(format!("not valid UTF-8: {e}")))?;
            let line_no = line.number - 1;
            let start = line.start;
            let end = line.end;
            if parse_line.trim().is_empty() {
                continue;
            }

            let rec: OpenClawRecord = match serde_json::from_str(parse_line) {
                Ok(r) => r,
                Err(e) => {
                    report.skipped += 1;
                    report
                        .diagnostics
                        .push(format!("line {}: invalid JSON, skipped ({e})", line_no + 1));
                    continue;
                }
            };

            let span = Some((start, end));

            match rec.r#type.as_str() {
                "session" => {
                    if let Some(id) = rec.id.as_deref()
                        && !id.trim().is_empty()
                    {
                        let id = id.trim();
                        if report.session_native_id.is_none() {
                            report.session_native_id = Some(id.to_string());
                            report.session_observation.provider_session_id =
                                MetadataResolution::Resolved(id.to_string());
                        }
                        if !session_ids.iter().any(|s| s == id) {
                            session_ids.push(id.to_string());
                        }
                        // cwd observed from the same session record (pair preserved).
                        if !report.session_observation.pair_observed
                            && let Some(cwd) = rec.cwd.as_deref()
                            && !cwd.trim().is_empty()
                        {
                            report.session_observation.original_working_directory =
                                MetadataResolution::Resolved(cwd.trim().to_string());
                            report.session_observation.pair_observed = true;
                        }
                    }
                }
                "message" => {
                    let Some(msg) = &rec.message else {
                        report.skipped += 1;
                        report.diagnostics.push(format!(
                            "line {}: message record without `message` body, skipped",
                            line_no + 1
                        ));
                        continue;
                    };
                    let role = msg.role.as_str();
                    if !matches!(role, "user" | "assistant") {
                        continue;
                    }
                    let content = msg.content.as_ref().unwrap_or(&serde_json::Value::Null);
                    let text = openclaw_content_text(content);
                    if text.trim().is_empty() {
                        continue;
                    }
                    let timestamp = msg
                        .timestamp
                        .as_ref()
                        .and_then(|v| v.as_str())
                        .or(rec.timestamp.as_deref());
                    sink.emit_message(MessageEvent {
                        seq,
                        // The format carries no per-message native id. A
                        // synthetic `openclaw-msg-{seq}` would collide across
                        // documents because seq restarts at 0 in every file, so
                        // the first message of every session would share one id
                        // and the storage merge would silently drop all but one
                        // payload. Emit an empty native_id so the ingestion layer
                        // derives a document-scoped id from
                        // [provider_id, variant, document_id, seq].
                        native_id: "",
                        parent_native_id: None,
                        role,
                        text: &text,
                        timestamp,
                        is_sidechain: false,
                        span,
                    })
                    .map_err(|e| ProviderError::StructuralFatal(e.to_string()))?;
                    seq += 1;
                    report.committed += 1;
                }
                _ => {
                    // Unknown record types are non-conversational → skipped.
                }
            }
        }

        // Multi-session diagnostic.
        if session_ids.len() > 1 {
            report.session_observation.multi_session = true;
            report.session_observation.provider_session_id = MetadataResolution::Ambiguous;
            report.diagnostics.push(format!(
                "文件包含 {} 个不同 id——单文件=单会话，全部消息归属首个会话 {}",
                session_ids.len(),
                session_ids[0]
            ));
        }

        Ok(report)
    }
}

/// Extract plain text from an OpenClaw content value.
///
/// OpenClaw content can be a string or an array of `{type:"text", text:"..."}`
/// blocks (similar to Claude's content blocks).
fn openclaw_content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => {
            let mut buf = String::new();
            for block in blocks {
                if block.get("type").and_then(serde_json::Value::as_str) == Some("text")
                    && let Some(t) = block.get("text").and_then(serde_json::Value::as_str)
                {
                    if !buf.is_empty() {
                        buf.push('\n');
                    }
                    buf.push_str(t);
                }
            }
            buf
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_matches_provider_matrix() {
        let adapter = OpenClawAdapter::new();
        let manifest = adapter.manifest();
        assert_eq!(manifest.provider_id, adapter.provider_id());
        assert_eq!(manifest.supported_variants, vec![VARIANT_ID.to_string()]);
        assert_eq!(manifest.capabilities.provider_id, adapter.provider_id());
        assert_eq!(manifest.capabilities.variant_id, VARIANT_ID);
        assert!(manifest.last_certified_targets.is_empty());
        assert_eq!(manifest.fixture_revision, Some(1));
    }

    struct CountSink {
        count: usize,
    }
    impl CanonicalEventSink for CountSink {
        fn emit_message(
            &mut self,
            _event: MessageEvent<'_>,
        ) -> agent_session_grep_ports::PortResult<()> {
            self.count += 1;
            Ok(())
        }
    }

    #[test]
    fn probe_rejects_empty_input() {
        let adapter = OpenClawAdapter::new();
        assert!(adapter.probe(b"").is_err());
    }

    #[test]
    fn probe_confirms_openclaw_session_jsonl() {
        let adapter = OpenClawAdapter::new();
        let fixture = r#"{"type":"session","id":"sess-1","cwd":"/work","timestamp":"2026-01-01T00:00:00Z"}
{"type":"message","message":{"role":"user","content":"hello"}}
{"type":"message","message":{"role":"assistant","content":"hi there"}}
"#;
        let result = adapter.probe(fixture.as_bytes()).unwrap();
        assert_eq!(result.variant_id, VARIANT_ID);
        assert_eq!(result.confidence, Confidence::Confirmed);
    }

    #[test]
    fn probe_rejects_non_openclaw_jsonl() {
        let adapter = OpenClawAdapter::new();
        let fixture = "{\"foo\":1}\n{\"bar\":2}\n";
        assert!(adapter.probe(fixture.as_bytes()).is_err());
    }

    /// Regression (M1-14): a real OpenClaw v3 transcript does not put a message
    /// right after the header — it opens with `session`, then a run of
    /// `model_change` / `thinking_level_change` / `custom` records. The old
    /// 8-line probe window could close before any message was seen, leaving the
    /// probe at `High` on exactly the files it is meant to confirm. Record order
    /// here follows the openclaw stage0 fixtures in jazzyalex/agent-sessions
    /// (MIT), corroborated by kenn-io/agentsview's openclaw parser tests (MIT).
    /// Synthetic content, observed order.
    #[test]
    fn probe_confirms_when_meta_records_precede_first_message() {
        let adapter = OpenClawAdapter::new();
        let mut fixture = String::from(
            r#"{"type":"session","id":"sess-1","version":3,"timestamp":"2026-01-01T00:00:00Z","cwd":"/work"}
{"type":"thinking_level_change","id":"t1","thinkingLevel":"high"}
{"type":"custom","id":"c1","customType":"ui_state","data":{"phase":"analysis"}}
"#,
        );
        for i in 0..12 {
            fixture.push_str(&format!(
                "{{\"type\":\"model_change\",\"id\":\"m{i}\",\"modelId\":\"model-{i}\"}}\n"
            ));
        }
        fixture.push_str(
            r#"{"type":"message","message":{"role":"user","content":"hello"}}
{"type":"message","message":{"role":"assistant","content":"hi there"}}
"#,
        );
        let result = adapter.probe(fixture.as_bytes()).unwrap();
        assert_eq!(result.variant_id, VARIANT_ID);
        assert_eq!(
            result.confidence,
            Confidence::Confirmed,
            "a real-order openclaw transcript must reach Confirmed, not tie at High"
        );
    }

    /// The probe stays bounded (RFC-0002 §7): beyond the scan cap the verdict
    /// stops improving rather than the read growing with the file.
    #[test]
    fn probe_stays_bounded_when_first_message_is_beyond_scan_limit() {
        let adapter = OpenClawAdapter::new();
        let mut fixture = String::from(
            r#"{"type":"session","id":"sess-1","version":3,"timestamp":"2026-01-01T00:00:00Z","cwd":"/work"}
"#,
        );
        for i in 0..SCAN_LINE_LIMIT {
            fixture.push_str(&format!(
                "{{\"type\":\"model_change\",\"id\":\"m{i}\",\"modelId\":\"model-{i}\"}}\n"
            ));
        }
        fixture.push_str(r#"{"type":"message","message":{"role":"user","content":"hello"}}"#);
        let result = adapter.probe(fixture.as_bytes()).unwrap();
        assert_eq!(result.confidence, Confidence::High);
    }

    #[test]
    fn parse_extracts_session_and_messages() {
        let adapter = OpenClawAdapter::new();
        let fixture = r#"{"type":"session","id":"sess-1","cwd":"/home/user/proj","timestamp":"2026-01-01T00:00:00Z"}
{"type":"message","message":{"role":"user","content":"hello world"}}
{"type":"message","message":{"role":"assistant","content":"hi there"}}
"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(sink.count, 2);
        assert_eq!(report.committed, 2);
        assert_eq!(report.session_native_id.as_deref(), Some("sess-1"));
        assert!(report.session_observation.pair_observed);
    }

    #[test]
    fn parse_handles_array_content_blocks() {
        let adapter = OpenClawAdapter::new();
        let fixture = r#"{"type":"session","id":"s1","cwd":"/p"}
{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"part1"},{"type":"text","text":"part2"}]}}
"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
    }

    #[test]
    fn parse_skips_non_conversational_types() {
        let adapter = OpenClawAdapter::new();
        let fixture = r#"{"type":"session","id":"s1","cwd":"/p"}
{"type":"session_info","name":"my session"}
{"type":"compaction","summary":"compacted data"}
{"type":"message","message":{"role":"user","content":"real msg"}}
"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
    }

    #[test]
    fn parse_skips_invalid_json_lines() {
        let adapter = OpenClawAdapter::new();
        let fixture =
            "not json\n{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"ok\"}}\n";
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
        assert_eq!(report.skipped, 1);
    }

    #[test]
    fn parse_multi_session_emits_diagnostic() {
        let adapter = OpenClawAdapter::new();
        let fixture = r#"{"type":"session","id":"s1","cwd":"/p"}
{"type":"session","id":"s2","cwd":"/q"}
{"type":"message","message":{"role":"user","content":"msg"}}
"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
        assert!(!report.diagnostics.is_empty());
        assert!(report.session_observation.multi_session);
    }
}
