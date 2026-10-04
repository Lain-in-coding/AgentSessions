//! Golden 契约测试：把 `tests/golden/basic.jsonl` 的解析输出逐字段钉死在
//! `tests/golden/basic.expected.json`——任何 parser 行为变化都必须显式更新
//! expected 才能通过，防止 canonical 输出无声漂移。
//!
//! 字节精确性：fixture 的**磁盘字节**是权威输入，expected 里钉了它的 BLAKE3
//! 指纹。若 git 换行转换或编辑器改写了字节，先在指纹断言处响亮失败，
//! 而不是留到后面变成难懂的 span 错位。

use agent_session_grep_ports::{CanonicalEventSink, Confidence, MessageEvent, ProviderAdapter};
use agent_session_grep_provider_codex::CodexAdapter;
use agent_session_grep_testkit::assert_read_only;
use serde_json::{Value, json};
use std::path::PathBuf;

/// 收集 emit 的消息事件（各 provider crate 测试各自持有收集器的既有先例）。
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
    fn emit_message(
        &mut self,
        event: MessageEvent<'_>,
    ) -> agent_session_grep_ports::PortResult<()> {
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

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

fn read_fixture_bytes() -> Vec<u8> {
    std::fs::read(golden_dir().join("basic.jsonl")).expect("read tests/golden/basic.jsonl")
}

/// 解析 fixture 并构造与 basic.expected.json 同形的 canonical JSON。
fn parse_to_canonical_json(bytes: &[u8]) -> Value {
    let mut sink = CollectingSink::default();
    let report = CodexAdapter::new()
        .parse(bytes, &mut sink)
        .expect("golden fixture must parse");
    let messages: Vec<Value> = sink
        .messages
        .iter()
        .map(|m| {
            let (start, end) = m
                .span
                .expect("codex adapter must attribute a span per message");
            json!({
                "seq": m.seq,
                "native_id": m.native_id,
                "parent_native_id": m.parent_native_id,
                "role": m.role,
                "text": m.text,
                "timestamp": m.timestamp,
                "is_sidechain": m.is_sidechain,
                "span": {"start": start, "end": end},
            })
        })
        .collect();
    json!({
        "fixture_blake3": blake3::hash(bytes).to_hex().as_str(),
        "session_native_id": report.session_native_id,
        "committed": report.committed,
        "skipped": report.skipped,
        "messages": messages,
    })
}

#[test]
fn probe_never_mutates_source_bytes() {
    let bytes = read_fixture_bytes();

    // RFC-0002 §7 的只读契约必须有可执行守护，不能只依赖代码审查。
    assert_read_only(&bytes, |source| CodexAdapter::new().probe(source))
        .expect("golden fixture probe must succeed");
}

#[test]
fn parse_never_mutates_source_bytes() {
    let bytes = read_fixture_bytes();
    let mut sink = CollectingSink::default();

    // parse 是实际产出路径；运行时指纹断言守护 RFC-0002 §7 的源只读契约。
    let report = assert_read_only(&bytes, |source| {
        CodexAdapter::new().parse(source, &mut sink)
    })
    .expect("golden fixture must parse");
    assert!(report.committed > 0, "fixture must exercise message output");
    assert!(!sink.messages.is_empty(), "fixture must emit messages");
}

#[test]
fn golden_provenance_revision_matches_manifest() {
    assert_eq!(CodexAdapter::new().manifest().fixture_revision, Some(2));
}

#[test]
fn golden_basic_matches_pinned_canonical_output() {
    let bytes = read_fixture_bytes();
    let expected: Value = serde_json::from_slice(
        &std::fs::read(golden_dir().join("basic.expected.json"))
            .expect("read tests/golden/basic.expected.json"),
    )
    .expect("basic.expected.json must be valid JSON");

    // 先验字节指纹：最常见的漂移来源是 git eol 转换（autocrlf），
    // 必须在这里失败并给出可行动的修复指向。
    let actual_hash = blake3::hash(&bytes).to_hex().to_string();
    let pinned_hash = expected["fixture_blake3"].as_str().unwrap_or_default();
    assert_eq!(
        actual_hash, pinned_hash,
        "fixture bytes drifted — check .gitattributes -text rules \
         (on-disk blake3 = {actual_hash}, pinned = {pinned_hash})"
    );

    let actual = parse_to_canonical_json(&bytes);
    assert_eq!(
        actual,
        expected,
        "canonical output drifted from basic.expected.json; actual =\n{}",
        serde_json::to_string_pretty(&actual).unwrap()
    );
}

#[test]
fn golden_probe_tolerates_intentional_broken_line() {
    // PRD R2.3：fixture 内置一条故意破损行——probe 必须容忍（≤3），不得
    // 整源拒绝；置信度降一档（无破损时为 Confirmed → High），保持 adapter 认领。
    let bytes = read_fixture_bytes();
    let r = CodexAdapter::new()
        .probe(&bytes)
        .expect("golden fixture probe must tolerate the broken line");
    assert_eq!(r.confidence, Confidence::High);
}

#[test]
fn golden_basic_spans_slice_back_to_source_envelope_lines() {
    let bytes = read_fixture_bytes();
    let mut sink = CollectingSink::default();
    CodexAdapter::new()
        .parse(&bytes, &mut sink)
        .expect("golden fixture must parse");
    assert!(!sink.messages.is_empty(), "fixture must emit messages");

    // 独立于 parser 重新计算每行的字节区间（与 parse 相同的行语义：
    // split_inclusive + 去掉行尾 \n / \r\n），span 必须精确落在某一行上。
    let text = std::str::from_utf8(&bytes).expect("fixture is UTF-8");
    let mut line_spans: Vec<(u64, u64, &str)> = Vec::new();
    let mut offset = 0u64;
    for raw_line in text.split_inclusive('\n') {
        let start = offset;
        offset += raw_line.len() as u64;
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        line_spans.push((start, start + line.len() as u64, line));
    }

    for m in &sink.messages {
        let (start, end) = m.span.expect("span required");
        let (_, line_end, line) = line_spans
            .iter()
            .find(|(s, _, _)| *s == start)
            .unwrap_or_else(|| {
                panic!(
                    "span.start {start} does not land on any line start (native_id={})",
                    m.native_id
                )
            });
        assert_eq!(
            end, *line_end,
            "span.end must equal the line end (native_id={})",
            m.native_id
        );
        // span 切片必须等于来源封套行的原始字节。
        assert_eq!(
            &bytes[start as usize..end as usize],
            line.as_bytes(),
            "span slice must equal the source envelope line (native_id={})",
            m.native_id
        );
        // 且来源行必须是权威 response_item/message 封套——绝不是 event_msg 镜像。
        let v: Value = serde_json::from_str(line).expect("span target line must be valid JSON");
        assert_eq!(
            v["type"], "response_item",
            "span must point at an authoritative envelope (native_id={})",
            m.native_id
        );
        assert_eq!(
            v["payload"]["type"], "message",
            "span must point at a message payload (native_id={})",
            m.native_id
        );
        assert_eq!(
            v["payload"]["id"].as_str(),
            Some(m.native_id.as_str()),
            "span target line must carry the same native id"
        );
    }
}

/// 手动再生辅助：fixture 合法变更（PROVENANCE.md 的 fixture_revision 递增）后，
/// 运行下面命令打印新的 canonical JSON，人工审阅后粘贴回 basic.expected.json：
///
/// ```text
/// cargo test -p agent-session-grep-provider-codex --test golden -- --ignored --nocapture
/// ```
#[test]
#[ignore = "manual regeneration helper — prints canonical JSON for basic.expected.json"]
fn print_actual_canonical_output_for_regeneration() {
    for name in ["basic.jsonl", "session-metadata.jsonl"] {
        let bytes = std::fs::read(golden_dir().join(name)).expect("read synthetic fixture");
        println!(
            "{name}:\n{}",
            serde_json::to_string_pretty(&parse_to_canonical_json(&bytes)).unwrap()
        );
    }
}

#[test]
fn session_metadata_ids_are_type_scoped_and_keep_messages_native() {
    use agent_session_grep_ports::MetadataResolution;
    use agent_session_grep_testkit::golden;
    let cases = [
        (
            json!({"id":" sid-a ","cwd":" /synthetic/paired "}),
            Some("sid-a"),
            Some("/synthetic/paired"),
        ),
        (
            json!({"session_id":"sid-a","cwd":"/synthetic/paired"}),
            Some("sid-a"),
            Some("/synthetic/paired"),
        ),
        (
            json!({"id":"sid-a","session_id":" sid-a ","cwd":"/synthetic/paired"}),
            Some("sid-a"),
            Some("/synthetic/paired"),
        ),
        (
            json!({"id":" sid-a ","session_id":"root-session","cwd":"/synthetic/paired"}),
            Some("sid-a"),
            Some("/synthetic/paired"),
        ),
        (
            json!({"id":"sid-a","session_id":" \t","cwd":"/synthetic/paired"}),
            Some("sid-a"),
            Some("/synthetic/paired"),
        ),
        (
            json!({"id":" \t","session_id":"sid-a","cwd":"/synthetic/paired"}),
            Some("sid-a"),
            Some("/synthetic/paired"),
        ),
        (
            json!({"id":"sid-a","session_id":null,"cwd":"/synthetic/paired"}),
            Some("sid-a"),
            Some("/synthetic/paired"),
        ),
        (json!({"id":"sid-a"}), Some("sid-a"), None),
        (json!({"cwd":"/synthetic/unpaired"}), None, None),
        (
            json!({"id":" ","session_id":"\t","cwd":"/synthetic/unpaired"}),
            None,
            None,
        ),
    ];
    for (payload, expected_sid, expected_cwd) in cases {
        let records = [
            json!({"type":"session_meta","payload":payload}),
            json!({"type":"turn_context","payload":{"id":"other-turn","session_id":"not-a-session","cwd":"/synthetic/ignored"}}),
            json!({"type":"response_item","payload":{"type":"message","id":"message-u","session_id":"not-a-session","role":"user","content":[{"type":"input_text","text":"user body"}]}}),
            json!({"type":"event_msg","payload":{"type":"user_message","id":"mirror-id","message":"user body"}}),
            json!({"type":"response_item","payload":{"type":"message","id":"message-a","role":"assistant","content":[{"type":"output_text","text":"assistant body"}]}}),
        ];
        let bytes = records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>();
        let (report, sink) = golden::parse_golden(&CodexAdapter::new(), bytes.as_bytes());
        assert_eq!(report.session_native_id.as_deref(), expected_sid);
        assert_eq!(
            report.session_observation.provider_session_id,
            expected_sid
                .map(|id| MetadataResolution::Resolved(id.into()))
                .unwrap_or_default()
        );
        assert_eq!(
            report.session_observation.original_working_directory,
            expected_cwd
                .map(|cwd| MetadataResolution::Resolved(cwd.into()))
                .unwrap_or_default()
        );
        assert_eq!(
            report.session_observation.pair_observed,
            expected_cwd.is_some()
        );
        assert!(!report.session_observation.multi_session);
        assert!(report.diagnostics.is_empty());
        assert_eq!((report.committed, report.skipped), (2, 0));
        for (seq, (message, (id, text))) in sink
            .messages
            .iter()
            .zip([("message-u", "user body"), ("message-a", "assistant body")])
            .enumerate()
        {
            assert_eq!(message.seq, seq as u32);
            assert_eq!(message.native_id, id);
            assert_eq!(message.text, text);
            assert_eq!(message.timestamp, None);
            assert_eq!(message.parent_native_id, None);
        }
    }
}

#[test]
fn root_and_thread_metadata_are_distinct_in_both_entry_points() {
    use agent_session_grep_ports::{MetadataResolution, SliceSource};
    use agent_session_grep_testkit::golden::CapturingSink;
    let metadata = json!({"type":"session_meta","payload":{"id":"current-thread","session_id":"root-thread","cwd":"/synthetic/child"}});
    let message = json!({"type":"response_item","payload":{"type":"message","id":"message-after","role":"assistant","content":[{"type":"output_text","text":"synthetic answer"}]}});
    for prefix in [
        "",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"message-before\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"synthetic prompt\"}]}}\n",
    ] {
        let bytes = format!("{prefix}{metadata}\n{message}\n").into_bytes();
        for source_entry in [false, true] {
            let mut sink = CapturingSink::default();
            let report = assert_read_only(&bytes, |bytes| {
                if source_entry {
                    CodexAdapter::new().parse_source(&SliceSource::new(bytes), &mut sink)
                } else {
                    CodexAdapter::new().parse(bytes, &mut sink)
                }
            })
            .expect("root and current thread IDs are not conflicting aliases");
            assert_eq!(report.session_native_id.as_deref(), Some("current-thread"));
            assert_eq!(
                report.session_observation.provider_session_id,
                MetadataResolution::Resolved("current-thread".into())
            );
            assert_eq!(
                report.session_observation.original_working_directory,
                MetadataResolution::Resolved("/synthetic/child".into())
            );
            assert!(report.session_observation.pair_observed);
            assert!(!report.session_observation.multi_session);
            assert!(report.diagnostics.is_empty());
            assert_eq!(report.skipped, 0);
            let expected_ids = if prefix.is_empty() {
                vec!["message-after"]
            } else {
                vec!["message-before", "message-after"]
            };
            assert_eq!(report.committed, expected_ids.len());
            assert_eq!(
                sink.messages
                    .iter()
                    .map(|m| m.native_id.as_str())
                    .collect::<Vec<_>>(),
                expected_ids
            );
        }
    }
}

#[test]
fn multiple_thread_headers_remain_multi_session_and_unpaired_cwd_stays_missing() {
    use agent_session_grep_ports::MetadataResolution;
    use agent_session_grep_testkit::golden;
    let records = [
        json!({"type":"session_meta","payload":{"id":"sid-a","session_id":"shared-root"}}),
        json!({"type":"session_meta","payload":{"cwd":"/unpaired"}}),
        json!({"type":"session_meta","payload":{"id":"sid-b","session_id":"shared-root","cwd":"/other-session"}}),
    ];
    let bytes = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    let (report, _) = golden::parse_golden(&CodexAdapter::new(), bytes.as_bytes());
    assert_eq!(report.session_native_id.as_deref(), Some("sid-a"));
    assert!(report.session_observation.multi_session);
    assert_eq!(
        report.session_observation.provider_session_id,
        MetadataResolution::Ambiguous
    );
    assert_eq!(
        report.session_observation.original_working_directory,
        MetadataResolution::Missing
    );
    assert!(!report.session_observation.pair_observed);
    assert_eq!((report.committed, report.skipped), (0, 0));
    assert_eq!(report.diagnostics.len(), 1);
}

#[test]
fn malformed_metadata_id_types_do_not_gain_fallback_authority() {
    use agent_session_grep_testkit::golden;
    for payload in [
        json!({"id":null,"session_id":"sid-a","cwd":"/not-authorized"}),
        json!({"id":"sid-a","session_id":17,"cwd":"/not-authorized"}),
    ] {
        let bytes = format!("{}\n", json!({"type":"session_meta","payload":payload}));
        let (report, sink) = golden::parse_golden(&CodexAdapter::new(), bytes.as_bytes());
        assert_eq!(report.skipped, 1);
        assert_eq!(report.session_native_id, None);
        assert_eq!(report.session_observation, Default::default());
        assert!(sink.messages.is_empty());
    }
}

#[test]
fn session_metadata_golden_and_bounded_source_preserve_evidence() {
    use agent_session_grep_ports::{MetadataResolution, SliceSource};
    use agent_session_grep_testkit::golden;
    let path = golden_dir().join("session-metadata.jsonl");
    let expected_path = golden_dir().join("session-metadata.expected.json");
    let expected = golden::read_expected(expected_path.to_str().unwrap());
    let bytes = golden::read_fixture_verified(path.to_str().unwrap(), &expected);
    let (report, sink) = golden::parse_golden(&CodexAdapter::new(), &bytes);
    let hash = blake3::hash(&bytes).to_hex().to_string();
    assert_eq!(
        golden::canonical_json(&hash, &report, &sink.messages),
        expected
    );
    assert_eq!(
        report.session_observation.provider_session_id,
        MetadataResolution::Resolved("current-thread".into())
    );
    assert_eq!(
        report.session_observation.original_working_directory,
        MetadataResolution::Resolved("/synthetic/paired".into())
    );
    assert!(report.session_observation.pair_observed);
    assert!(!report.session_observation.multi_session);
    assert!(report.diagnostics.is_empty());
    let bom_crlf = [
        b"\xef\xbb\xbf".as_slice(),
        String::from_utf8(bytes.clone())
            .unwrap()
            .replace('\n', "\r\n")
            .as_bytes(),
    ]
    .concat();
    for variant in [&bytes, &bom_crlf] {
        let (expected_report, expected_sink) = golden::parse_golden(&CodexAdapter::new(), variant);
        let mut actual_sink = golden::CapturingSink::default();
        let actual_report = assert_read_only(variant, |input| {
            CodexAdapter::new().parse_source(&SliceSource::new(input), &mut actual_sink)
        })
        .unwrap();
        assert_eq!(
            actual_report.session_observation,
            expected_report.session_observation
        );
        assert_eq!(actual_report.diagnostics, expected_report.diagnostics);
        assert_eq!(
            golden::canonical_json("same-input", &actual_report, &actual_sink.messages),
            golden::canonical_json("same-input", &expected_report, &expected_sink.messages)
        );
    }
}
