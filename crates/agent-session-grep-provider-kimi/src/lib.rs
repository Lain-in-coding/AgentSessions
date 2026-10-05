//! Kimi Code provider adapter.
//!
//! Parses Kimi's wire.jsonl format: each line carries a `type` discriminator.
//! `context.append_message` records wrap `message.role` (user/assistant) and
//! `message.content`. `context.append_loop_event` records carry step/tool
//! events (step.begin/content.part/tool.call/tool.result/step.end) — this
//! adapter also reads user input from `turn.prompt` / `turn.steer`; loop events
//! are deferred.
//!
//! Format evidence: fast-resume (MIT) `src/adapters/kimi.rs`. The message
//! extraction is adapted from fast-resume under its MIT license.

use agent_session_grep_ports::MetadataResolution;
use agent_session_grep_ports::{
    AdapterManifest, CanonicalEventSink, Confidence, MessageEvent, ParseReport, ProbeResult,
    ProviderAdapter, ProviderError, manifest_for,
};

/// Variant id surfaced in probe results.
const VARIANT_ID: &str = "kimi-code/wire-jsonl-v1";

/// Number of non-blank lines to sample during probe (bounded, RFC-0002 §7).
const SAMPLE_LINE_LIMIT: usize = 8;

/// Kimi Code adapter: parses wire.jsonl with type-discriminated records.
pub struct KimiCodeAdapter;

impl KimiCodeAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for KimiCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimal deserialization target for a Kimi wire.jsonl record.
#[derive(serde::Deserialize)]
struct WireRecord {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    message: Option<WireMessage>,
    /// `turn.prompt` / `turn.steer` carry the user's own words here rather than
    /// in `message`: a string or array of content blocks in the same shape
    /// `message.content` uses.
    ///
    /// Evidence: ctx's `kimi_event_type`, `kimi_event_role` and `kimi_event_text`
    /// classify these exact types as user messages and read this field. Its
    /// shape fixture uses text blocks; its tests also construct string input.
    /// See `tests/golden/PROVENANCE.md` for immutable source links and the
    /// shape-only observations; no upstream fixture bytes are copied.
    #[serde(default)]
    input: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct WireMessage {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: Option<serde_json::Value>,
}

impl ProviderAdapter for KimiCodeAdapter {
    fn provider_id(&self) -> &str {
        "kimi-code"
    }

    fn manifest(&self) -> AdapterManifest {
        manifest_for(
            self.provider_id(),
            Some(2),
            &[
                "context.append_loop_event records (step/tool events) are not parsed: they are tool activity rather than messages, and wire.jsonl carries no per-message native id to anchor them to",
                "session id is rarely carried in wire.jsonl; session_native_id is usually left unset",
                "per-message timestamps are not extracted",
            ],
        )
    }

    fn probe(&self, bytes: &[u8]) -> Result<ProbeResult, ProviderError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| ProviderError::StructuralFatal(format!("not valid UTF-8: {e}")))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);

        let mut matched = Vec::new();
        let mut unmatched = Vec::new();

        let mut sample: Vec<(usize, &str)> = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if sample.len() == SAMPLE_LINE_LIMIT {
                break;
            }
            sample.push((idx + 1, line));
        }

        if sample.is_empty() {
            return Err(ProviderError::AmbiguousVariant(
                "empty input: no non-blank lines to probe".into(),
            ));
        }

        let mut json_lines = 0usize;
        let mut append_message_records = 0usize;
        let mut conversational = 0usize;
        let mut turn_input_records = 0usize;
        let mut conversational_turns = 0usize;

        for &(line_no, line) in &sample {
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(v) => {
                    json_lines += 1;
                    let t = v
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    if t == "context.append_message" {
                        append_message_records += 1;
                        let role = v
                            .pointer("/message/role")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("");
                        if matches!(role, "user" | "assistant") {
                            conversational += 1;
                        }
                    } else if matches!(t, "turn.prompt" | "turn.steer")
                        && let Some(input) = v
                            .get("input")
                            .filter(|input| input.is_string() || input.is_array())
                    {
                        turn_input_records += 1;
                        if !kimi_content_texts(input).trim().is_empty() {
                            conversational_turns += 1;
                        }
                    }
                }
                Err(_) => {
                    unmatched.push(format!("line {line_no}: not valid JSON"));
                }
            }
        }

        if json_lines == 0 {
            return Err(ProviderError::AmbiguousVariant(format!(
                "no JSON lines parsed in {0} sampled lines",
                sample.len()
            )));
        }

        matched.push(format!("{json_lines} sampled lines are valid JSON"));

        // Only supported message discriminators provide Kimi evidence. Turn records
        // additionally require the string/array input shape the parser handles.
        if append_message_records == 0 && turn_input_records == 0 {
            return Err(ProviderError::AmbiguousVariant(
                "no supported Kimi message or turn input records found in sampled lines".into(),
            ));
        }

        if append_message_records > 0 {
            if conversational > 0 {
                matched.push(format!(
                    "{append_message_records} append_message records, {conversational} conversational"
                ));
            } else {
                matched.push(format!(
                    "{append_message_records} append_message records found"
                ));
            }
        }
        if turn_input_records > 0 {
            matched.push(format!(
                "{turn_input_records} turn.prompt/turn.steer records with string/array input, {conversational_turns} with nonempty text"
            ));
        }
        let confidence = if conversational > 0 || conversational_turns > 0 {
            Confidence::Confirmed
        } else {
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
        .map_err(|e| ProviderError::Io(e.to_string()))?;

        let mut report = ParseReport::default();
        let mut seq: u32 = 0;

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

            let rec: WireRecord = match serde_json::from_str(parse_line) {
                Ok(r) => r,
                Err(e) => {
                    report.skipped += 1;
                    report
                        .diagnostics
                        .push(format!("line {}: invalid JSON, skipped ({e})", line_no + 1));
                    continue;
                }
            };

            // 三类承载对话文本的记录：
            // - `context.append_message`：role 在 `message.role`；
            // - `turn.prompt` / `turn.steer`：**用户自己的话**，文本在顶层
            //   `input` 的 content-block 数组里，role 恒为 user（证据见
            //   `WireRecord::input` 的文档注释：ctx 的 kimi 适配器把这两类
            //   归为 user-role message 事件）。此前只解析 append_message，
            //   于是 kimi 会话里最有检索价值的用户提问完全没进索引。
            // `context.append_loop_event`（step/tool 事件）仍不解析——它们是
            //   工具活动而非消息，且缺 per-message native id 无法锚定。
            let (role, text) = match rec.r#type.as_str() {
                "context.append_message" => {
                    let Some(msg) = &rec.message else {
                        report.skipped += 1;
                        continue;
                    };
                    let role = msg.role.as_str();
                    if !matches!(role, "user" | "assistant") {
                        continue;
                    }
                    let content = msg.content.as_ref().unwrap_or(&serde_json::Value::Null);
                    (role, kimi_content_texts(content))
                }
                "turn.prompt" | "turn.steer" => {
                    let input = rec.input.as_ref().unwrap_or(&serde_json::Value::Null);
                    ("user", kimi_content_texts(input))
                }
                _ => continue,
            };
            if text.trim().is_empty() {
                continue;
            }

            sink.emit_message(MessageEvent {
                session: None,
                seq,
                native_id: "",
                parent_native_id: None,
                role,
                text: &text,
                timestamp: None,
                is_sidechain: false,
                span: Some((start, end)),
            })
            .map_err(|e| ProviderError::StructuralFatal(e.to_string()))?;
            seq += 1;
            report.committed += 1;

            // Session identity: Kimi wire.jsonl rarely carries session id;
            // left None (discovery layer may supply path-derived id).
            if report.session_observation.provider_session_id == MetadataResolution::Missing {
                let _ = &report.session_observation;
            }
        }

        Ok(report)
    }
}

/// Extract text from Kimi message content.
///
/// Content can be a string or an array of content parts with `text` fields.
fn kimi_content_texts(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => {
            let mut buf = String::new();
            for part in parts {
                if let Some(t) = part.get("text").and_then(serde_json::Value::as_str) {
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
        let adapter = KimiCodeAdapter::new();
        let manifest = adapter.manifest();
        assert_eq!(manifest.provider_id, adapter.provider_id());
        assert_eq!(manifest.supported_variants, vec![VARIANT_ID.to_string()]);
        assert_eq!(manifest.capabilities.provider_id, adapter.provider_id());
        assert_eq!(manifest.capabilities.variant_id, VARIANT_ID);
        assert!(manifest.last_certified_targets.is_empty());
        assert_eq!(manifest.fixture_revision, Some(2));
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
        let adapter = KimiCodeAdapter::new();
        assert!(adapter.probe(b"").is_err());
    }

    #[test]
    fn probe_confirms_kimi_wire_jsonl() {
        let adapter = KimiCodeAdapter::new();
        let fixture = r#"{"type":"context.append_message","message":{"role":"user","content":"hello"}}
{"type":"context.append_message","message":{"role":"assistant","content":"hi there"}}
"#;
        let result = adapter.probe(fixture.as_bytes()).unwrap();
        assert_eq!(result.variant_id, VARIANT_ID);
        assert_eq!(result.confidence, Confidence::Confirmed);
    }

    #[test]
    fn probe_rejects_non_kimi_jsonl() {
        let adapter = KimiCodeAdapter::new();
        let fixture = "{\"foo\":1}\n{\"bar\":2}\n";
        assert!(adapter.probe(fixture.as_bytes()).is_err());
    }

    #[test]
    fn probe_confirms_exact_turn_records_with_supported_text() {
        let adapter = KimiCodeAdapter::new();
        let cases = [
            (
                serde_json::json!(" \tprompt 会话🚀 \n"),
                " \tprompt 会话🚀 \n",
            ),
            (
                serde_json::json!([
                    {"type": "text", "text": "first"},
                    {"type": "text", "text": " second "}
                ]),
                "first\n second ",
            ),
            (
                serde_json::json!([
                    {"text": ""}, {"type": "image"}, 42,
                    {"text": " part "}, null, {"text": 7}, {"text": "end"}
                ]),
                " part \nend",
            ),
            (serde_json::json!("body\u{feff}text"), "body\u{feff}text"),
        ];
        for kind in ["turn.prompt", "turn.steer"] {
            for (input, text) in &cases {
                let bytes = serde_json::to_vec(&serde_json::json!({
                    "type": kind, "input": input
                }))
                .unwrap();
                let result = adapter.probe(&bytes).unwrap();
                assert_eq!(result.variant_id, VARIANT_ID);
                assert_eq!(result.confidence, Confidence::Confirmed, "{kind}");
                assert!(result.unmatched_evidence.is_empty());
                let mut sink = RoleTextSink::default();
                let report = adapter.parse(&bytes, &mut sink).unwrap();
                assert_eq!(report.committed, 1);
                assert_eq!(report.skipped, 0);
                assert_eq!(sink.seen, vec![("user".into(), text.to_string())]);
            }
        }
    }

    #[test]
    fn probe_requires_supported_turn_input_shape() {
        let adapter = KimiCodeAdapter::new();
        for kind in ["turn.prompt", "turn.steer"] {
            for record in [
                serde_json::json!({"type": kind}),
                serde_json::json!({"type": kind, "input": null}),
                serde_json::json!({"type": kind, "input": false}),
                serde_json::json!({"type": kind, "input": 42}),
                serde_json::json!({"type": kind, "input": {"text": "not an input block array"}}),
                serde_json::json!({"type": kind, "message": {"role": "user", "content": "wrong location"}}),
                serde_json::json!({"type": kind, "data": {"input": [{"text": "wrong location"}]}}),
            ] {
                let bytes = serde_json::to_vec(&record).unwrap();
                assert!(
                    matches!(
                        adapter.probe(&bytes),
                        Err(ProviderError::AmbiguousVariant(_))
                    ),
                    "{record}"
                );
                let mut sink = RoleTextSink::default();
                let report = adapter.parse(&bytes, &mut sink).unwrap();
                assert_eq!(report.committed, 0);
                assert_eq!(report.skipped, 0);
                assert!(sink.seen.is_empty());
            }
        }
    }

    #[test]
    fn probe_keeps_textless_turn_input_at_high_confidence() {
        let adapter = KimiCodeAdapter::new();
        for kind in ["turn.prompt", "turn.steer"] {
            for input in [
                serde_json::json!(""),
                serde_json::json!(" \t\n"),
                serde_json::json!([]),
                serde_json::json!([{ "text": "" }, { "text": " \t" }]),
                serde_json::json!([null, 42, "not a text block", {"text": 7}, {"type": "image"}]),
            ] {
                let bytes = serde_json::to_vec(&serde_json::json!({
                    "type": kind, "input": input
                }))
                .unwrap();
                let probe = adapter.probe(&bytes).unwrap();
                assert_eq!(probe.confidence, Confidence::High, "{kind}: {input}");
                let mut sink = RoleTextSink::default();
                let report = adapter.parse(&bytes, &mut sink).unwrap();
                assert_eq!(report.committed, 0);
                assert_eq!(report.skipped, 0);
                assert!(sink.seen.is_empty());
            }
        }
    }

    #[test]
    fn probe_rejects_unknown_and_similar_turn_discriminators() {
        let adapter = KimiCodeAdapter::new();
        for kind in [
            "",
            "turn",
            "turn.",
            "turn.Prompt",
            "Turn.prompt",
            "turn.prompted",
            "turn.prompt.extra",
            "turn.prompt ",
            " turn.prompt",
            "turn.steering",
            "turn.steer.extra",
            "turn.steer ",
            "context.append_loop_event",
            "metadata",
        ] {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "type": kind, "input": [{"type": "text", "text": "not a supported turn"}]
            }))
            .unwrap();
            assert!(
                matches!(
                    adapter.probe(&bytes),
                    Err(ProviderError::AmbiguousVariant(_))
                ),
                "{kind}"
            );
        }
        for record in [
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!(true),
            serde_json::json!(42),
            serde_json::json!("turn.prompt"),
            serde_json::json!({"input": "no discriminator"}),
            serde_json::json!({"type": ["turn.prompt"], "input": "not a string discriminator"}),
        ] {
            assert!(matches!(
                adapter.probe(&serde_json::to_vec(&record).unwrap()),
                Err(ProviderError::AmbiguousVariant(_))
            ));
        }
    }

    #[test]
    fn probe_sampling_bound_counts_nonblank_records() {
        let adapter = KimiCodeAdapter::new();
        for kind in ["turn.prompt", "turn.steer"] {
            let candidate = format!(r#"{{"type":"{kind}","input":[{{"text":"sampled text"}}]}}"#);
            for noise in ["{\"type\":\"metadata\"}\n \t\r\n", "not json\n\n"] {
                let inside = format!("\n{}{}", noise.repeat(SAMPLE_LINE_LIMIT - 1), candidate);
                assert_eq!(
                    adapter.probe(inside.as_bytes()).unwrap().confidence,
                    Confidence::Confirmed
                );
                let outside = format!("\n{}{}", noise.repeat(SAMPLE_LINE_LIMIT), candidate);
                assert!(matches!(
                    adapter.probe(outside.as_bytes()),
                    Err(ProviderError::AmbiguousVariant(_))
                ));
            }
        }
    }

    #[test]
    fn probe_preserves_bom_utf8_and_broken_line_tolerance() {
        let adapter = KimiCodeAdapter::new();
        for kind in ["turn.prompt", "turn.steer"] {
            let record = format!(r#"{{"type":"{kind}","input":"text with \ufeff inside"}}"#);
            let with_bom = format!("\u{feff}{record}\r\n");
            assert_eq!(
                adapter.probe(with_bom.as_bytes()).unwrap().confidence,
                Confidence::Confirmed
            );
            let mixed = format!("\u{feff}not json\r\n \t\r\n{record}\r\n");
            let probe = adapter.probe(mixed.as_bytes()).unwrap();
            assert_eq!(probe.confidence, Confidence::Confirmed);
            assert_eq!(probe.unmatched_evidence, ["line 1: not valid JSON"]);
            for misplaced in [
                format!("\u{feff}\u{feff}{record}\n"),
                format!("{{\"type\":\"metadata\"}}\n\u{feff}{record}\n"),
            ] {
                assert!(matches!(
                    adapter.probe(misplaced.as_bytes()),
                    Err(ProviderError::AmbiguousVariant(_))
                ));
            }
            // Byte probe still validates UTF-8 even beyond its JSON line sample.
            let mut invalid =
                format!("{record}\n{}", "{}\n".repeat(SAMPLE_LINE_LIMIT)).into_bytes();
            invalid.push(0xff);
            assert!(matches!(
                adapter.probe(&invalid),
                Err(ProviderError::StructuralFatal(_))
            ));
        }
    }

    #[test]
    fn probe_preserves_append_message_confidence() {
        let adapter = KimiCodeAdapter::new();
        for (message, confidence, evidence) in [
            (
                serde_json::json!(null),
                Confidence::High,
                "1 append_message records found",
            ),
            (
                serde_json::json!({"role": "system", "content": "not conversational"}),
                Confidence::High,
                "1 append_message records found",
            ),
            (
                serde_json::json!({"role": "user"}),
                Confidence::Confirmed,
                "1 append_message records, 1 conversational",
            ),
            (
                serde_json::json!({"role": "assistant", "content": ""}),
                Confidence::Confirmed,
                "1 append_message records, 1 conversational",
            ),
        ] {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "type": "context.append_message", "message": message
            }))
            .unwrap();
            let probe = adapter.probe(&bytes).unwrap();
            assert_eq!(probe.confidence, confidence);
            assert_eq!(
                probe.matched_evidence,
                ["1 sampled lines are valid JSON", evidence]
            );
        }
    }

    #[test]
    fn probe_combines_turn_and_append_message_evidence() {
        let bytes = concat!(
            "{\"type\":\"context.append_message\",\"message\":{\"role\":\"system\"}}\n",
            "{\"type\":\"turn.prompt\",\"input\":[{\"text\":\"supported prompt\"}]}\n",
            "{\"type\":\"turn.steer\",\"input\":[]}\n",
        )
        .as_bytes();
        let probe = KimiCodeAdapter::new().probe(bytes).unwrap();
        assert_eq!(probe.confidence, Confidence::Confirmed);
        assert_eq!(
            probe.matched_evidence,
            [
                "3 sampled lines are valid JSON",
                "1 append_message records found",
                "2 turn.prompt/turn.steer records with string/array input, 1 with nonempty text",
            ]
        );
    }

    /// 记录 role/text 的最小 sink（本模块单测用；golden 集成测试各有自己的）。
    #[derive(Default)]
    struct RoleTextSink {
        seen: Vec<(String, String)>,
    }
    impl CanonicalEventSink for RoleTextSink {
        fn emit_message(
            &mut self,
            event: MessageEvent<'_>,
        ) -> agent_session_grep_ports::PortResult<()> {
            self.seen
                .push((event.role.to_string(), event.text.to_string()));
            Ok(())
        }
    }

    /// Kimi 把**用户自己的提问**写在 `turn.prompt`（含 `turn.steer` 续问）里，
    /// 而不是 `context.append_message`。此前适配器只解析后者，于是 kimi 会话
    /// 里检索价值最高的用户输入完全没进索引；golden fixture 不含该记录，所以
    /// 既有测试也抓不到。语料形状取自 ctx 的真实形态 fixture。
    #[test]
    fn parse_indexes_user_prompts_from_turn_records() {
        let adapter = KimiCodeAdapter::new();
        let fixture = concat!(
            r#"{"type":"metadata","protocol_version":"1.4"}"#,
            "\n",
            r#"{"type":"turn.prompt","time":1783170001000,"input":[{"type":"text","text":"why does sync tombstone"}],"origin":{"kind":"user"}}"#,
            "\n",
            r#"{"type":"context.append_message","message":{"role":"assistant","content":[{"type":"text","text":"because the scan was complete"}]}}"#,
            "\n",
            r#"{"type":"turn.steer","time":1783170003000,"input":[{"type":"text","text":"stop and explain"}]}"#,
            "\n",
            r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolName":"Write"}}"#,
            "\n",
        );
        let mut sink = RoleTextSink::default();
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();

        assert_eq!(
            sink.seen,
            vec![
                ("user".to_string(), "why does sync tombstone".to_string()),
                (
                    "assistant".to_string(),
                    "because the scan was complete".to_string()
                ),
                ("user".to_string(), "stop and explain".to_string()),
            ],
            "turn.prompt / turn.steer must index as user messages in file order"
        );
        assert_eq!(report.committed, 3);
        // metadata 与 loop event 都不是消息：既不提交也不计入 skipped。
        assert_eq!(report.skipped, 0);
    }

    #[test]
    fn parse_extracts_messages() {
        let adapter = KimiCodeAdapter::new();
        let fixture = r#"{"type":"context.append_message","message":{"role":"user","content":"hello world"}}
{"type":"context.append_message","message":{"role":"assistant","content":"hi there"}}
"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(sink.count, 2);
        assert_eq!(report.committed, 2);
    }

    #[test]
    fn parse_skips_loop_events() {
        let adapter = KimiCodeAdapter::new();
        let fixture = r#"{"type":"context.append_loop_event","event":{"type":"step.begin","uuid":"s1"}}
{"type":"context.append_message","message":{"role":"user","content":"real msg"}}
"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
    }

    #[test]
    fn parse_skips_invalid_json() {
        let adapter = KimiCodeAdapter::new();
        let fixture = "not json\n{\"type\":\"context.append_message\",\"message\":{\"role\":\"user\",\"content\":\"ok\"}}\n";
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
        assert_eq!(report.skipped, 1);
    }

    #[test]
    fn parse_array_content_parts() {
        let adapter = KimiCodeAdapter::new();
        let fixture = r#"{"type":"context.append_message","message":{"role":"assistant","content":[{"text":"part1"},{"text":"part2"}]}}
"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
    }

    #[test]
    fn parse_skips_non_conversational_roles() {
        let adapter = KimiCodeAdapter::new();
        let fixture = r#"{"type":"context.append_message","message":{"role":"system","content":"system msg"}}
{"type":"context.append_message","message":{"role":"user","content":"real msg"}}
"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
    }

    struct TextSink {
        texts: Vec<String>,
    }
    impl CanonicalEventSink for TextSink {
        fn emit_message(
            &mut self,
            event: MessageEvent<'_>,
        ) -> agent_session_grep_ports::PortResult<()> {
            self.texts.push(event.text.to_string());
            Ok(())
        }
    }

    #[test]
    fn parse_passes_noise_shaped_user_text_through_verbatim() {
        // 钉住测试：Kimi wire.jsonl 没有 system-reminder / AGENTS.md /
        // 环境上下文等注入概念（系统注入走 role:"system"，已被角色门跳过）。
        // 形似噪声的 user 文本必须逐字透传，防止将来把别家格式的过滤规则
        // 盲目搬来造成 silent drift。
        let adapter = KimiCodeAdapter::new();
        let fixture = r##"{"type":"context.append_message","message":{"role":"user","content":"<system-reminder>reminder text</system-reminder>"}}
{"type":"context.append_message","message":{"role":"user","content":"# AGENTS.md instructions"}}
"##;
        let mut sink = TextSink { texts: vec![] };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 2);
        assert_eq!(report.skipped, 0);
        assert_eq!(
            sink.texts,
            vec![
                "<system-reminder>reminder text</system-reminder>".to_string(),
                "# AGENTS.md instructions".to_string(),
            ]
        );
    }
}
