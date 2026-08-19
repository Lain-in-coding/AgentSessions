//! Cline provider adapter.
//!
//! Parses Cline's `api_conversation_history.json` format: a JSON array of
//! message objects with `role` (user/assistant) and `content`. Unlike JSONL
//! providers, Cline uses a single JSON array file per task.
//!
//! Format evidence: ctx (Apache-2.0) provider-support-matrix.json confirms
//! `cline_task_directory_json` source_format with
//! `api_conversation_history.json`, `ui_messages.json`, `context_history.json`,
//! `task_metadata.json` in task directories under CLINE_DATA_DIR/tasks/*/
//! and ~/.cline/data/tasks/*/.

use agent_session_grep_ports::{
    AdapterManifest, CanonicalEventSink, Confidence, MessageEvent, ParseReport, ProbeResult,
    ProviderAdapter, ProviderError, manifest_for, rfc3339_utc_from_epoch_millis,
};

/// Variant id surfaced in probe results.
const VARIANT_ID: &str = "cline/api-conversation-history-v1";

/// Cline adapter: parses `api_conversation_history.json` (JSON array of messages).
pub struct ClineAdapter;

impl ClineAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClineAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimal deserialization target for a Cline message record.
#[derive(serde::Deserialize)]
struct ClineMessage {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    timestamp: Option<serde_json::Value>,
}

impl ProviderAdapter for ClineAdapter {
    fn provider_id(&self) -> &str {
        "cline"
    }

    fn manifest(&self) -> AdapterManifest {
        manifest_for(
            self.provider_id(),
            Some(1),
            &[
                "no session id in the JSON array file; session_native_id is left unset",
                "no byte spans (whole-file JSON array); the format carries no per-message native id, so message identity is reconstructed document-scoped by the ingestion layer (Unstable)",
                "per-message timestamps come from the optional `timestamp` field (RFC3339 string or epoch milliseconds); records without one, or whose value is non-positive or out of range, carry no timestamp and therefore fall outside every `--since`/`--until` window",
            ],
        )
    }

    fn probe(&self, bytes: &[u8]) -> Result<ProbeResult, ProviderError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| ProviderError::StructuralFatal(format!("not valid UTF-8: {e}")))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);

        let mut matched = Vec::new();
        let unmatched = Vec::new();

        // Cline is a JSON array (not JSONL). Try to parse as a JSON array.
        let value: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                return Err(ProviderError::AmbiguousVariant(format!(
                    "not valid JSON array: {e}"
                )));
            }
        };

        let arr = match value.as_array() {
            Some(a) => a,
            None => {
                return Err(ProviderError::AmbiguousVariant(
                    "expected a JSON array, got a different shape".into(),
                ));
            }
        };

        if arr.is_empty() {
            return Err(ProviderError::AmbiguousVariant(
                "empty array: no messages to probe".into(),
            ));
        }

        // Sample up to 8 records to check for conversational messages.
        let sample = arr.iter().take(8);
        let mut json_records = 0usize;
        let mut conversational = 0usize;
        let mut has_role_field = 0usize;

        for record in sample {
            json_records += 1;
            if let Some(role) = record.get("role").and_then(serde_json::Value::as_str) {
                has_role_field += 1;
                if matches!(role, "user" | "assistant") {
                    conversational += 1;
                }
            }
        }

        matched.push(format!("{json_records} sampled records are valid JSON"));

        // Cline is distinct: JSON array with role/content. Refuse if no
        // role field present (could be a different JSON array format).
        if has_role_field == 0 {
            return Err(ProviderError::AmbiguousVariant(
                "no records with a `role` field found in sampled array".into(),
            ));
        }

        let confidence = if conversational > 0 {
            matched.push(format!(
                "{conversational} conversational messages (user/assistant)"
            ));
            Confidence::Confirmed
        } else {
            matched.push(format!("{has_role_field} records with role field"));
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

        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| ProviderError::StructuralFatal(format!("not valid JSON: {e}")))?;

        let arr = value
            .as_array()
            .ok_or_else(|| ProviderError::StructuralFatal("expected JSON array".into()))?;

        let mut report = ParseReport::default();
        let mut seq: u32 = 0;

        for (idx, record) in arr.iter().enumerate() {
            let rec: ClineMessage = match serde_json::from_value(record.clone()) {
                Ok(r) => r,
                Err(e) => {
                    report.skipped += 1;
                    report
                        .diagnostics
                        .push(format!("record {idx}: invalid shape, skipped ({e})"));
                    continue;
                }
            };

            let role = rec.role.as_str();
            if !matches!(role, "user" | "assistant") {
                continue;
            }

            let content = rec.content.as_ref().unwrap_or(&serde_json::Value::Null);
            let text = cline_content_text(content);
            if text.trim().is_empty() {
                continue;
            }

            // Cline records the message time either as an RFC3339 string or as
            // an epoch-millisecond integer. The integer must be *rendered*: the
            // search time filter only accepts a timezone-qualified RFC3339
            // string, and time predicates push down to SQL where NULL fails
            // every comparison — so a message with no usable timestamp is
            // silently excluded from every `--since` / `--until` query.
            let timestamp = cline_timestamp(rec.timestamp.as_ref());

            sink.emit_message(MessageEvent {
                seq,
                // The Cline format carries no per-message native id. Emitting a
                // synthetic `cline-msg-{seq}` would collide across documents,
                // because seq restarts at 0 in every task file: the first message
                // of every task would share one id and the storage merge would
                // silently drop all but one payload. Emit an empty native_id so
                // the ingestion layer derives a document-scoped id from
                // [provider_id, variant, document_id, seq].
                native_id: "",
                parent_native_id: None,
                role,
                text: &text,
                timestamp: timestamp.as_deref(),
                is_sidechain: false,
                // Cline is a single JSON array, not a line-delimited format: the
                // adapter cannot attribute a byte range to one message without
                // byte-level JSON parsing. Emitting array-index pseudo-spans
                // would violate the span contract (byte offsets into the source),
                // so span is left None (capability.rs `source_span: unsupported`).
                span: None,
            })
            .map_err(|e| ProviderError::StructuralFatal(e.to_string()))?;
            seq += 1;
            report.committed += 1;
        }

        Ok(report)
    }
}

/// Extract text from Cline message content.
///
/// Content can be a string or an array of content parts.
fn cline_content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => {
            let mut buf = String::new();
            for block in blocks {
                if let Some(t) = block.get("text").and_then(serde_json::Value::as_str) {
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

/// Normalize a Cline `timestamp` field into a search-filterable instant.
///
/// A string is passed through verbatim (Cline writes RFC3339); an integer is
/// rendered as RFC3339 UTC. A blank string, a non-string/non-integer shape, and
/// an out-of-range instant all yield `None` — an absent timestamp is honest,
/// whereas an empty string looks like data and a fabricated 1970 instant is a
/// lie. This previously mapped an epoch integer to `""`, discarding the one
/// value the time filter needed.
fn cline_timestamp(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        serde_json::Value::Number(number) => {
            number.as_i64().and_then(rfc3339_utc_from_epoch_millis)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_matches_provider_matrix() {
        let adapter = ClineAdapter::new();
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
        let adapter = ClineAdapter::new();
        assert!(adapter.probe(b"").is_err());
    }

    #[test]
    fn probe_rejects_non_json() {
        let adapter = ClineAdapter::new();
        assert!(adapter.probe(b"not json at all").is_err());
    }

    #[test]
    fn probe_rejects_json_object_not_array() {
        let adapter = ClineAdapter::new();
        assert!(adapter.probe(b"{\"role\":\"user\"}").is_err());
    }

    #[test]
    fn probe_rejects_empty_array() {
        let adapter = ClineAdapter::new();
        assert!(adapter.probe(b"[]").is_err());
    }

    #[test]
    fn probe_confirms_cline_json_array() {
        let adapter = ClineAdapter::new();
        let fixture = r#"[{"role":"user","content":"hello"},{"role":"assistant","content":"hi"}]"#;
        let result = adapter.probe(fixture.as_bytes()).unwrap();
        assert_eq!(result.variant_id, VARIANT_ID);
        assert_eq!(result.confidence, Confidence::Confirmed);
    }

    #[test]
    fn probe_rejects_array_without_role() {
        let adapter = ClineAdapter::new();
        let fixture = r#"[{"foo":"bar"},{"baz":"qux"}]"#;
        assert!(adapter.probe(fixture.as_bytes()).is_err());
    }

    #[test]
    fn parse_extracts_messages() {
        let adapter = ClineAdapter::new();
        let fixture = r#"[{"role":"user","content":"hello world"},{"role":"assistant","content":"hi there"}]"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(sink.count, 2);
        assert_eq!(report.committed, 2);
    }

    #[test]
    fn parse_skips_non_conversational_roles() {
        let adapter = ClineAdapter::new();
        let fixture =
            r#"[{"role":"system","content":"system msg"},{"role":"user","content":"real msg"}]"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
    }

    #[test]
    fn parse_handles_array_content() {
        let adapter = ClineAdapter::new();
        let fixture = r#"[{"role":"assistant","content":[{"type":"text","text":"part1"},{"type":"text","text":"part2"}]}]"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
    }

    #[test]
    fn parse_skips_empty_content() {
        let adapter = ClineAdapter::new();
        let fixture = r#"[{"role":"user","content":""},{"role":"assistant","content":"real"}]"#;
        let mut sink = CountSink { count: 0 };
        let report = adapter.parse(fixture.as_bytes(), &mut sink).unwrap();
        assert_eq!(report.committed, 1);
    }

    /// Captures the emitted timestamps so the epoch-millis path is observable.
    #[derive(Default)]
    struct TimestampSink {
        timestamps: Vec<Option<String>>,
    }
    impl CanonicalEventSink for TimestampSink {
        fn emit_message(
            &mut self,
            event: MessageEvent<'_>,
        ) -> agent_session_grep_ports::PortResult<()> {
            self.timestamps.push(event.timestamp.map(str::to_string));
            Ok(())
        }
    }

    fn parsed_timestamps(fixture: &str) -> Vec<Option<String>> {
        let mut sink = TimestampSink::default();
        ClineAdapter::new()
            .parse(fixture.as_bytes(), &mut sink)
            .unwrap();
        sink.timestamps
    }

    #[test]
    fn epoch_millis_timestamp_is_rendered_not_discarded() {
        // Regression: an epoch-millis integer used to map to `""` — the value was
        // read, thrown away, and replaced with a string that looks like data.
        let fixture = r#"[{"role":"user","content":"m","timestamp":1767225660000}]"#;
        assert_eq!(
            parsed_timestamps(fixture),
            vec![Some("2026-01-01T00:01:00.000Z".to_string())]
        );
    }

    #[test]
    fn string_timestamp_passes_through_verbatim() {
        let fixture = r#"[{"role":"user","content":"m","timestamp":"2026-01-01T00:01:00Z"}]"#;
        assert_eq!(
            parsed_timestamps(fixture),
            vec![Some("2026-01-01T00:01:00Z".to_string())]
        );
    }

    #[test]
    fn unusable_timestamp_stays_none_and_is_never_an_empty_string() {
        // Missing, blank, non-positive, absurd, and wrong-shaped values must all
        // be absent rather than an empty string: `""` cannot be parsed by the
        // search filter yet still reads as if the provider supplied a time.
        let cases = [
            r#"[{"role":"user","content":"m"}]"#,
            r#"[{"role":"user","content":"m","timestamp":""}]"#,
            r#"[{"role":"user","content":"m","timestamp":"   "}]"#,
            r#"[{"role":"user","content":"m","timestamp":0}]"#,
            r#"[{"role":"user","content":"m","timestamp":-1}]"#,
            r#"[{"role":"user","content":"m","timestamp":253402300800000}]"#,
            r#"[{"role":"user","content":"m","timestamp":true}]"#,
            r#"[{"role":"user","content":"m","timestamp":{"at":1767225660000}}]"#,
        ];
        for fixture in cases {
            assert_eq!(parsed_timestamps(fixture), vec![None], "case: {fixture}");
        }
    }

    #[test]
    fn epoch_millis_render_as_rfc3339_utc() {
        // The conversion itself is covered in ports (`time.rs`); this pins the
        // instant the cline tests depend on.
        assert_eq!(
            rfc3339_utc_from_epoch_millis(1_767_225_660_000).as_deref(),
            Some("2026-01-01T00:01:00.000Z")
        );
        // Better no timestamp than a fabricated one.
        assert!(rfc3339_utc_from_epoch_millis(0).is_none());
        assert!(rfc3339_utc_from_epoch_millis(-1).is_none());
        assert!(rfc3339_utc_from_epoch_millis(i64::MAX).is_none());
    }
}
