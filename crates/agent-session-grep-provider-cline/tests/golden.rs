//! Golden 契约测试：固定 fixture 字节 → pinned canonical 输出。
//!
//! 任何 parser 行为漂移（角色映射、文本抽取、计数口径）或 fixture 字节漂移
//! （git 行尾转换、误编辑）都必须在此响亮失败，作为 Beta 认证的可复核证据。
//! fixture 为纯合成数据，来源与覆盖点见 `tests/golden/PROVENANCE.md`。
//!
//! Cline 是单文件 JSON 数组（`api_conversation_history.json`）。适配器不产出
//! 字节 span（`span: None`）——span round-trip 标记 N/A；capability.rs 的
//! `source_span` 诚实声明为 `unsupported`。

use agent_session_grep_application::parse_search_instant;
use agent_session_grep_ports::{
    CanonicalEventSink, Confidence, MessageEvent, ParseReport, ProviderAdapter,
};
use agent_session_grep_provider_cline::ClineAdapter;
use agent_session_grep_testkit::assert_read_only;
use serde_json::{Value, json};

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/basic.json");
const EXPECTED_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/basic.expected.json"
);

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

fn read_expected() -> Value {
    let bytes = std::fs::read(EXPECTED_PATH).expect("read basic.expected.json");
    serde_json::from_slice(&bytes).expect("basic.expected.json must be valid JSON")
}

fn read_fixture_verified(expected: &Value) -> Vec<u8> {
    let bytes = std::fs::read(FIXTURE_PATH).expect("read basic.json fixture");
    let actual = blake3::hash(&bytes).to_hex().to_string();
    let pinned = expected["fixture_blake3"]
        .as_str()
        .expect("expected.json must pin fixture_blake3");
    assert_eq!(
        actual, pinned,
        "fixture bytes drifted — check .gitattributes -text rules (actual blake3 = {actual})"
    );
    bytes
}

fn parse_fixture(bytes: &[u8]) -> (ParseReport, Vec<Captured>) {
    let mut sink = CollectingSink::default();
    let report = ClineAdapter::new()
        .parse(bytes, &mut sink)
        .expect("golden fixture parse must succeed");
    (report, sink.messages)
}

fn canonical_json(fixture_blake3: &str, report: &ParseReport, messages: &[Captured]) -> Value {
    json!({
        "fixture_blake3": fixture_blake3,
        "session_native_id": report.session_native_id,
        "committed": report.committed,
        "skipped": report.skipped,
        "messages": messages
            .iter()
            .map(|m| {
                json!({
                    "seq": m.seq,
                    "native_id": m.native_id,
                    "parent_native_id": m.parent_native_id,
                    "role": m.role,
                    "text": m.text,
                    "timestamp": m.timestamp,
                    "is_sidechain": m.is_sidechain,
                    "span": m.span.map(|(start, end)| json!({"start": start, "end": end})),
                })
            })
            .collect::<Vec<_>>(),
    })
}

#[test]
fn probe_never_mutates_source_bytes() {
    let expected = read_expected();
    let bytes = read_fixture_verified(&expected);
    assert_read_only(&bytes, |source| ClineAdapter::new().probe(source))
        .expect("golden fixture probe must succeed");
}

#[test]
fn parse_never_mutates_source_bytes() {
    let expected = read_expected();
    let bytes = read_fixture_verified(&expected);
    let mut sink = CollectingSink::default();
    let report = assert_read_only(&bytes, |source| {
        ClineAdapter::new().parse(source, &mut sink)
    })
    .expect("golden fixture parse must succeed");
    assert!(report.committed > 0, "fixture must exercise message output");
    assert!(!sink.messages.is_empty(), "fixture must emit messages");
}

#[test]
fn golden_provenance_revision_matches_manifest() {
    assert_eq!(ClineAdapter::new().manifest().fixture_revision, Some(1));
}

#[test]
fn golden_canonical_output_is_pinned() {
    let expected = read_expected();
    let bytes = read_fixture_verified(&expected);
    let (report, messages) = parse_fixture(&bytes);
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let actual = canonical_json(&hash, &report, &messages);
    let actual_pretty = serde_json::to_string_pretty(&actual).expect("serialize actual");
    assert_eq!(
        actual, expected,
        "canonical 输出与 pinned 期望不一致——parser 行为漂移或 fixture 未经评审变更。actual =\n{actual_pretty}"
    );
}

#[test]
fn golden_probe_confirms_fixture() {
    // Cline 是单文档 JSON 数组：probe 直接解析整体，无"破损行"概念。fixture 必须
    // 被确认为 Cline（数组 + 含 role 字段的记录）。
    let expected = read_expected();
    let bytes = read_fixture_verified(&expected);
    let r = ClineAdapter::new()
        .probe(&bytes)
        .expect("golden fixture probe must succeed");
    assert_eq!(r.confidence, Confidence::Confirmed);
}

#[test]
fn golden_messages_carry_no_byte_span() {
    // JSON 数组无行式字节坐标：span round-trip 标记 N/A，全部消息 span 为 None。
    let expected = read_expected();
    let bytes = read_fixture_verified(&expected);
    let (_, messages) = parse_fixture(&bytes);
    assert!(!messages.is_empty(), "golden fixture must emit messages");
    assert!(
        messages.iter().all(|m| m.span.is_none()),
        "cline 不应归因字节 span（N/A，pseudo-span 已移除）"
    );
}

/// 回归：两个不同任务文件的 seq-0 消息不得共享 native id。
///
/// 历史缺陷：适配器合成 `cline-msg-{seq}`，而 seq 每个文件从 0 重启，因此任意两个
/// 任务文件的首条消息都会得到 `cline-msg-0`——storage merge 只保留一份 payload，
/// 另一条被静默丢弃（sync 仍报 "2 added"）。
///
/// 修复后适配器发出空 native_id；document-scoped 派生发生在 CLI 摄取层
/// （`[provider_id, variant, document_id, seq]` + `Stability::Unstable`），
/// 适配器层只能断言"未发明 id"。
#[test]
fn distinct_documents_do_not_collide_on_seq_zero() {
    let doc_a = br#"[{"role":"user","content":"task A first message"}]"#;
    let doc_b = br#"[{"role":"user","content":"task B first message"}]"#;

    let (report_a, messages_a) = parse_fixture(doc_a);
    let (report_b, messages_b) = parse_fixture(doc_b);

    assert_eq!(report_a.committed, 1);
    assert_eq!(report_b.committed, 1);
    assert_eq!(messages_a[0].seq, 0);
    assert_eq!(messages_b[0].seq, 0);

    // 关键断言：适配器不发明 native id，把身份交给 document-scoped 派生。
    assert!(
        messages_a[0].native_id.is_empty(),
        "cline 不应合成 native message id（会跨文档碰撞）"
    );
    assert!(
        messages_b[0].native_id.is_empty(),
        "cline 不应合成 native message id（会跨文档碰撞）"
    );

    // 文本仍然可区分，证明两条消息是不同 payload 而非同一条。
    assert_ne!(messages_a[0].text, messages_b[0].text);
}

/// 时间过滤回归：`--since`/`--until` 下推为 `sort_key >= ?`，而 NULL 比较恒假——
/// 没有 timestamp 的消息会被每一次时间窗查询静默排除。此测试用**生产**解析器
/// `parse_search_instant` 解析每条 emitted timestamp，并断言其落在合成窗口内
/// （即 `--since`/`--until` 会命中这些消息）。只断言"非 None"是不够的：一个
/// 过滤器无法解析的字符串（历史缺陷里的 `""`）也能通过那种断言。
#[test]
fn golden_timestamps_fall_inside_search_window() {
    let expected = read_expected();
    let bytes = read_fixture_verified(&expected);
    let (_, messages) = parse_fixture(&bytes);
    assert!(!messages.is_empty(), "golden fixture must emit messages");

    let since = parse_search_instant("2026-01-01T00:00:00Z").expect("window lower bound parses");
    let until = parse_search_instant("2026-01-01T01:00:00Z").expect("window upper bound parses");

    let mut timestamped = 0_usize;
    for message in &messages {
        let Some(raw) = message.timestamp.as_deref() else {
            continue;
        };
        // 空串比 None 更糟：它看起来像数据，却解析不出任何 instant。
        assert!(
            !raw.trim().is_empty(),
            "seq {} 的 timestamp 是空串——应当为 None",
            message.seq
        );
        let instant = parse_search_instant(raw).unwrap_or_else(|| {
            panic!("timestamp {raw} must parse with the production search filter parser")
        });
        assert!(
            instant.sort_key() >= since.sort_key() && instant.sort_key() <= until.sort_key(),
            "timestamp {raw} 落在时间窗之外——`--since`/`--until` 会漏掉该消息"
        );
        timestamped += 1;
    }
    // fixture 只有最后一条带 timestamp：钉住数量，避免"全部为 None"时本测试空转通过。
    assert_eq!(
        timestamped, 1,
        "fixture 应恰有 1 条带 timestamp 的消息（其余诚实地为 None）"
    );
}

/// epoch 毫秒整数必须被**渲染**成过滤器可解析的 RFC3339，而不是被丢弃。
///
/// 历史缺陷：整数被映射成 `""`——值读到了却扔掉，换成一个"看起来像数据"的空串。
/// fixture 字节不可变更（BLAKE3 已钉住），故 epoch 形态用内联合成文档覆盖。
#[test]
fn epoch_millis_timestamp_matches_the_same_window_as_its_string_form() {
    // 1767225660000 ms 与 fixture 里的 "2026-01-01T00:01:00Z" 是同一时刻。
    let epoch_doc = br#"[{"role":"user","content":"epoch form","timestamp":1767225660000}]"#;
    let string_doc =
        br#"[{"role":"user","content":"string form","timestamp":"2026-01-01T00:01:00Z"}]"#;

    let (_, epoch_messages) = parse_fixture(epoch_doc);
    let (_, string_messages) = parse_fixture(string_doc);

    let since = parse_search_instant("2026-01-01T00:00:00Z").expect("window lower bound parses");
    let until = parse_search_instant("2026-01-01T01:00:00Z").expect("window upper bound parses");

    let epoch_raw = epoch_messages[0]
        .timestamp
        .as_deref()
        .expect("epoch-millis timestamp must be rendered, not discarded");
    let string_raw = string_messages[0]
        .timestamp
        .as_deref()
        .expect("string timestamp must pass through");

    let epoch_instant = parse_search_instant(epoch_raw)
        .expect("rendered epoch must parse with the production parser");
    let string_instant = parse_search_instant(string_raw).expect("string form must parse");

    // 两种形态指向同一 instant：整数形态确实落在窗口里，而不是被静默排除。
    assert_eq!(epoch_instant.sort_key(), string_instant.sort_key());
    assert!(
        epoch_instant.sort_key() >= since.sort_key()
            && epoch_instant.sort_key() <= until.sort_key(),
        "渲染出的 {epoch_raw} 落在时间窗之外"
    );
}

/// 手动再生辅助：
/// ```text
/// cargo test -p agent-session-grep-provider-cline --test golden -- --ignored --nocapture
/// ```
#[test]
#[ignore = "manual regeneration helper — prints canonical JSON for basic.expected.json"]
fn print_actual_canonical_output_for_regeneration() {
    let bytes = std::fs::read(FIXTURE_PATH).expect("read basic.json fixture");
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let (report, messages) = parse_fixture(&bytes);
    println!(
        "{}",
        serde_json::to_string_pretty(&canonical_json(&hash, &report, &messages)).unwrap()
    );
}
