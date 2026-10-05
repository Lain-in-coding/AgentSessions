//! Golden 契约测试：固定 fixture 字节 → pinned canonical 输出。
//!
//! 任何 parser 行为漂移（角色映射、文本抽取、span 计算、计数口径）或
//! fixture 字节漂移（git 行尾转换、误编辑）都必须在此响亮失败，作为
//! Beta 认证的可复核证据。fixture 为纯合成数据，来源与覆盖点见
//! `tests/golden/PROVENANCE.md`。
//!
//! 全字段捕获 sink、fixture 读取与 BLAKE3 校验、canonical JSON 投影复用
//! `agent_session_grep_testkit::golden`，本文件只保留 kimi 特有的 span↔record
//! 断言与一个手动再生辅助。

use agent_session_grep_ports::{Confidence, ProviderAdapter, SliceSource};
use agent_session_grep_provider_kimi::KimiCodeAdapter;
use agent_session_grep_testkit::assert_read_only;
use agent_session_grep_testkit::golden::{self, CapturingSink};
use serde_json::Value;

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/basic.jsonl");
const EXPECTED_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/basic.expected.json"
);

const TURN_FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/turn-inputs.jsonl"
);
const TURN_EXPECTED_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/turn-inputs.expected.json"
);

/// 解析 fixture：经共享 sink 全字段捕获，返回报告与 sink。
fn parse_fixture(bytes: &[u8]) -> (agent_session_grep_ports::ParseReport, CapturingSink) {
    golden::parse_golden(&KimiCodeAdapter::new(), bytes)
}

#[test]
fn probe_never_mutates_source_bytes() {
    let expected = golden::read_expected(EXPECTED_PATH);
    let bytes = golden::read_fixture_verified(FIXTURE_PATH, &expected);
    assert_read_only(&bytes, |source| KimiCodeAdapter::new().probe(source))
        .expect("golden fixture probe must succeed");
}

#[test]
fn parse_never_mutates_source_bytes() {
    let expected = golden::read_expected(EXPECTED_PATH);
    let bytes = golden::read_fixture_verified(FIXTURE_PATH, &expected);
    let mut sink = CapturingSink::default();
    let report = assert_read_only(&bytes, |source| {
        KimiCodeAdapter::new().parse(source, &mut sink)
    })
    .expect("golden fixture parse must succeed");
    assert!(report.committed > 0, "fixture must exercise message output");
    assert!(!sink.messages.is_empty(), "fixture must emit messages");
}

#[test]
fn golden_provenance_revision_matches_manifest() {
    assert_eq!(KimiCodeAdapter::new().manifest().fixture_revision, Some(2));
}

#[test]
fn golden_canonical_output_is_pinned() {
    let expected = golden::read_expected(EXPECTED_PATH);
    let bytes = golden::read_fixture_verified(FIXTURE_PATH, &expected);
    let (report, sink) = parse_fixture(&bytes);
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let actual = golden::canonical_json(&hash, &report, &sink.messages);
    let actual_pretty = serde_json::to_string_pretty(&actual).expect("serialize actual");
    assert_eq!(
        actual, expected,
        "canonical 输出与 pinned 期望不一致——parser 行为漂移或 fixture 未经评审变更。actual =\n{actual_pretty}"
    );
}

#[test]
fn golden_probe_tolerates_intentional_broken_line() {
    let expected = golden::read_expected(EXPECTED_PATH);
    let bytes = golden::read_fixture_verified(FIXTURE_PATH, &expected);
    let r = KimiCodeAdapter::new()
        .probe(&bytes)
        .expect("golden fixture probe must tolerate the broken line");
    assert_eq!(r.confidence, Confidence::Confirmed);
}

#[test]
fn golden_spans_slice_back_to_exact_source_lines() {
    let expected = golden::read_expected(EXPECTED_PATH);
    let bytes = golden::read_fixture_verified(FIXTURE_PATH, &expected);
    let (_, sink) = parse_fixture(&bytes);
    assert_source_line_spans(&bytes, &sink);
}

fn assert_source_line_spans(bytes: &[u8], sink: &CapturingSink) {
    let messages = &sink.messages;
    assert!(!messages.is_empty(), "golden fixture must emit messages");

    let text = std::str::from_utf8(bytes).expect("fixture is UTF-8");
    let mut line_by_start = std::collections::HashMap::new();
    let mut offset = 0u64;
    for raw in text.split_inclusive('\n') {
        let start = offset;
        offset += raw.len() as u64;
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line = line.strip_suffix('\r').unwrap_or(line);
        line_by_start.insert(start, line);
    }

    for m in messages {
        let (start, end) = m.span.expect("golden message must carry a span");
        let line = line_by_start
            .get(&start)
            .unwrap_or_else(|| panic!("span.start={start} is not the start of any source line"));
        assert_eq!(
            end - start,
            line.len() as u64,
            "seq={}: span 长度必须等于源记录行（去行尾）字节长",
            m.seq
        );
        let slice = &bytes[start as usize..end as usize];
        assert_eq!(
            slice,
            line.as_bytes(),
            "seq={}: span 切片必须与源记录行逐字节一致",
            m.seq
        );
        // span 指向的必须是"它自己"的记录：type + message.role 与消息角色一致。
        let record: Value =
            serde_json::from_slice(slice).expect("span slice must be a complete JSON record");
        match record["type"].as_str() {
            Some("context.append_message") => assert_eq!(
                record["message"]["role"].as_str().unwrap_or(""),
                m.role,
                "seq={}: span must point to the message's own role",
                m.seq
            ),
            Some("turn.prompt" | "turn.steer") => assert_eq!(m.role, "user"),
            other => panic!(
                "seq={}: span points to an unsupported record {other:?}",
                m.seq
            ),
        }
    }
}

#[test]
fn golden_turn_inputs_probe_without_sampled_append_messages() {
    let expected = golden::read_expected(TURN_EXPECTED_PATH);
    let bytes = golden::read_fixture_verified(TURN_FIXTURE_PATH, &expected);
    // The first append_message is deliberately outside the eight-line probe window.
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .take(8)
            .all(|line| !line.contains("context.append_message"))
    );
    let adapter = KimiCodeAdapter::new();
    let probe = assert_read_only(&bytes, |bytes| adapter.probe(bytes)).unwrap();
    assert_eq!(probe.variant_id, "kimi-code/wire-jsonl-v1");
    assert_eq!(probe.confidence, Confidence::Confirmed);
    assert_eq!(probe.unmatched_evidence, ["line 7: not valid JSON"]);
    assert_eq!(
        probe,
        assert_read_only(&bytes, |bytes| adapter
            .probe_source(&SliceSource::new(bytes)))
        .unwrap()
    );
    assert_eq!(std::fs::read(TURN_FIXTURE_PATH).unwrap(), bytes);
}

#[test]
fn golden_turn_inputs_pin_parse_and_source_equivalence() {
    for (fixture_path, expected_path) in [
        (FIXTURE_PATH, EXPECTED_PATH),
        (TURN_FIXTURE_PATH, TURN_EXPECTED_PATH),
    ] {
        let expected = golden::read_expected(expected_path);
        let bytes = golden::read_fixture_verified(fixture_path, &expected);
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let adapter = KimiCodeAdapter::new();
        let mut byte_sink = CapturingSink::default();
        let report =
            assert_read_only(&bytes, |bytes| adapter.parse(bytes, &mut byte_sink)).unwrap();
        let mut source_sink = CapturingSink::default();
        let source_report = assert_read_only(&bytes, |bytes| {
            adapter.parse_source(&SliceSource::new(bytes), &mut source_sink)
        })
        .unwrap();
        assert_eq!(report, source_report);
        assert_eq!(
            golden::canonical_json(&hash, &report, &byte_sink.messages),
            expected
        );
        assert_eq!(
            golden::canonical_json(&hash, &source_report, &source_sink.messages),
            expected
        );
        assert!(byte_sink.activities.is_empty() && source_sink.activities.is_empty());
        assert!(byte_sink.usages.is_empty() && source_sink.usages.is_empty());
        assert_source_line_spans(&bytes, &byte_sink);
        assert_source_line_spans(&bytes, &source_sink);
        assert_eq!(std::fs::read(fixture_path).unwrap(), bytes);
    }
}

#[test]
fn turn_input_source_equivalence_preserves_bom_crlf_and_final_line() {
    let bytes = concat!(
        "\u{feff}{\"type\":\"turn.prompt\",\"input\":\"  first 会话🚀  \"}\r\n",
        " \t\r\n",
        "{\"type\":\"turn.steer\",\"input\":[{\"text\":\"next\"},{\"text\":\" \\ufeff last\\n\"}]}"
    )
    .as_bytes();
    let adapter = KimiCodeAdapter::new();
    let hash = blake3::hash(bytes).to_hex().to_string();
    let (report, sink) = parse_fixture(bytes);
    let mut source_sink = CapturingSink::default();
    let source_report = assert_read_only(bytes, |bytes| {
        adapter.parse_source(&SliceSource::new(bytes), &mut source_sink)
    })
    .unwrap();
    assert_eq!(report, source_report);
    assert_eq!(report.committed, 2);
    assert_eq!(report.skipped, 0);
    assert_eq!(sink.messages[0].text, "  first 会话🚀  ");
    assert_eq!(sink.messages[1].text, "next\n \u{feff} last\n");
    assert_eq!(
        golden::canonical_json(&hash, &report, &sink.messages),
        golden::canonical_json(&hash, &source_report, &source_sink.messages)
    );
    assert_eq!(
        adapter.probe(bytes).unwrap(),
        adapter.probe_source(&SliceSource::new(bytes)).unwrap()
    );
    let first_end = bytes.iter().position(|byte| *byte == b'\r').unwrap();
    let second_start = bytes.windows(2).rposition(|pair| pair == b"\r\n").unwrap() + 2;
    assert_eq!(sink.messages[0].span, Some((0, first_end as u64)));
    assert_eq!(
        sink.messages[1].span,
        Some((second_start as u64, bytes.len() as u64))
    );
    assert_eq!(&bytes[..3], &[0xef, 0xbb, 0xbf]);
}

#[test]
fn tool_activity_stays_unsupported_because_activities_cannot_anchor() {
    // 双向钉住 tool_activity=Unsupported 的诚实性：
    // 1) 语料确含文档化的 loop 事件记录（`context.append_loop_event`，其
    //    step/tool 事件含 tool.call/tool.result，本切片 deferred）——格式有
    //    工具事件通道，不是"格式无记录"；
    // 2) 所有消息以空 native id 上报（append_message 记录无 per-message id），
    //    且 adapter 零 activity 输出——per RFC-0002 R5.3 + staging fail-closed，
    //    活动根本无法锚定，Unsupported 是唯一诚实声明。
    // 未来若格式获得 per-message id 或 loop 事件被解析，本测试的断言会失败，
    // 强制重评 capability。
    let expected = golden::read_expected(EXPECTED_PATH);
    let bytes = golden::read_fixture_verified(FIXTURE_PATH, &expected);
    let (report, sink) = parse_fixture(&bytes);
    assert!(report.committed > 0, "fixture must exercise message output");
    assert!(
        sink.activities.is_empty(),
        "adapter 不得发出无法锚定的 activity（空 native id 会被 staging fail-closed 丢弃）"
    );
    assert!(
        !sink.messages.is_empty() && sink.messages.iter().all(|m| m.native_id.trim().is_empty()),
        "golden 消息全部以空 native id 上报——若未来带上 per-message id，\
         capability.rs 的 tool_activity 声明必须重新评估"
    );
    let text = std::str::from_utf8(&bytes).expect("fixture is UTF-8");
    assert!(
        text.contains("\"context.append_loop_event\""),
        "golden 语料必须保留文档化的 loop 事件记录形状（如实承认格式有工具事件通道）"
    );
}

/// 手动再生辅助：
/// ```text
/// cargo test -p agent-session-grep-provider-kimi --test golden -- --ignored --nocapture
/// ```
#[test]
#[ignore = "manual regeneration helper — prints canonical JSON for both expected files"]
fn print_actual_canonical_output_for_regeneration() {
    for fixture_path in [FIXTURE_PATH, TURN_FIXTURE_PATH] {
        let bytes = std::fs::read(fixture_path).expect("read golden fixture");
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let (report, sink) = parse_fixture(&bytes);
        println!(
            "{fixture_path}:\n{}",
            serde_json::to_string_pretty(&golden::canonical_json(&hash, &report, &sink.messages))
                .unwrap()
        );
    }
}
