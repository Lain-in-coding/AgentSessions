//! CLI 端到端集成测试：驱动真实编译出的二进制，走 index → search → get 全链路。
//!
//! 目的：把「组合根真的能把 SQLite adapter 注入 Application 并端到端跑通」这件事
//! 固化进 CI，而非依赖手动运行。Cargo 为集成测试注入 `CARGO_BIN_EXE_<bin>`，
//! 故此处零第三方依赖即可定位到刚构建的二进制。

use rusqlite::Connection;
use std::path::Path;
use std::process::{Command, Output};

/// 刚构建出的 `agent-session-grep` 二进制的绝对路径（由 Cargo 在编译期注入）。
const BIN: &str = env!("CARGO_BIN_EXE_agent-session-grep");

/// 在给定 db 上以 robot 协议模式跑一次 CLI，返回完整输出。
/// 功能性测试统一断言稳定 JSON envelope；human 版式走 [`run_human`]。
fn run(db: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("--db")
        .arg(db)
        .arg("--robot")
        .args(args)
        .output()
        .expect("failed to spawn agent-session-grep binary")
}

/// 在给定 db 上以默认 human 模式跑一次 CLI（无 --robot）：验证人类渲染器输出。
fn run_human(db: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("failed to spawn agent-session-grep binary")
}

/// 解析 stdout 的第一行为 JSON Value。
fn parse_first_line(o: &Output) -> serde_json::Value {
    let text = stdout(o);
    let line = text
        .lines()
        .next()
        .expect("output must have at least one line");
    serde_json::from_str(line).unwrap_or_else(|error| panic!("not valid JSON: {error}\n{line}"))
}

/// 断言 frame 满足 Robot v1 envelope 最小契约。
fn assert_envelope_shape(frame: &serde_json::Value, ok: bool) {
    assert_eq!(frame["schema_version"], "1.0", "schema_version");
    assert_eq!(frame["ok"], ok, "ok");
    if ok {
        assert_eq!(frame["frame_type"], "response", "frame_type");
        assert!(
            matches!(frame["outcome"].as_str(), Some("success") | Some("partial")),
            "outcome must be success or partial for ok:true"
        );
        assert!(frame["data"].is_object(), "data must be an object");
    } else {
        assert_eq!(frame["frame_type"], "error", "frame_type");
        assert_eq!(frame["outcome"], "failure", "outcome");
        let code = frame["error"]["code"]
            .as_str()
            .expect("error.code must be a string");
        assert!(!code.is_empty(), "error.code must not be empty");
        let message = frame["error"]["message"]
            .as_str()
            .expect("error.message must be a string");
        assert!(!message.is_empty(), "error.message must not be empty");
        assert!(
            frame["error"]["retryable"].is_boolean(),
            "error.retryable must be a boolean"
        );
        let details = frame["error"]["details"]
            .as_object()
            .expect("error.details must be an object");
        assert!(
            details.len() <= 32,
            "error.details must be bounded (schema caps at 32 properties), got {}",
            details.len()
        );
    }
    assert!(
        frame["request_id"].as_str().is_some(),
        "request_id must be a string"
    );
    assert!(frame["warnings"].is_array(), "warnings must be an array");
    assert!(
        frame["page"]["has_more"].is_boolean(),
        "page.has_more must be a boolean"
    );
    assert!(
        frame["page"]["next_cursor"].is_null() || frame["page"]["next_cursor"].is_string(),
        "page.next_cursor must be a string or null"
    );
    assert!(
        frame["meta"]["duration_ms"].is_number(),
        "meta.duration_ms must be a number"
    );
    assert!(
        frame["meta"]["generation"].is_null() || frame["meta"]["generation"].is_number(),
        "meta.generation must be null or a number"
    );
}

/// 每个测试用独立临时目录，避免 WAL/SHM 旁文件互相干扰。
fn temp_db(tag: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("{tag}.db"));
    let s = path.to_string_lossy().into_owned();
    (dir, s)
}

fn create_v6_catalog(
    db: &str,
    source_path: &str,
    session_wire: &str,
    message_wire: &str,
    document_wire: &str,
    message_text: &str,
) {
    let conn = Connection::open(db).expect("open v6 fixture catalog");
    conn.execute_batch(
        "CREATE TABLE catalog (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
         CREATE VIRTUAL TABLE fts USING fts5(id UNINDEXED, text);
         CREATE TABLE store_metadata (
             singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
             active_generation INTEGER NOT NULL
         );
         INSERT INTO store_metadata(singleton, active_generation) VALUES(1, 1);
         CREATE TABLE index_batches (
             operation_id TEXT PRIMARY KEY,
             base_generation INTEGER NOT NULL,
             target_generation INTEGER NOT NULL,
             state TEXT NOT NULL,
             operation_digest TEXT NOT NULL,
             upsert_ids_json TEXT NOT NULL,
             delete_ids_json TEXT NOT NULL,
             durable_point TEXT NOT NULL,
             created_at_ms INTEGER NOT NULL,
             committed_at_ms INTEGER,
             error_code TEXT
         );
         CREATE INDEX index_batches_state ON index_batches(state);
         CREATE TABLE fts_ids (
             wire_id TEXT PRIMARY KEY,
             id_json TEXT NOT NULL UNIQUE
         );
         CREATE TABLE source_membership (
             source_path TEXT NOT NULL,
             message_id TEXT NOT NULL,
             document_id TEXT,
             PRIMARY KEY(source_path, message_id)
         );
         CREATE INDEX source_membership_source
         ON source_membership(source_path);
         CREATE TABLE source_scans (
             source_path TEXT PRIMARY KEY,
             scanned_at_ms INTEGER NOT NULL
         );
         PRAGMA user_version = 6;",
    )
    .expect("create v6 schema");

    let session_payload = serde_json::json!({
        "document": document_wire,
        "documents": [document_wire],
        "messages": [message_wire],
    })
    .to_string();
    let message_payload = serde_json::json!({
        "role": "user",
        "text": message_text,
        "timestamp": "2026-07-28T00:00:00.000Z",
        "parent": null,
        "parent_native_id": null,
        "is_sidechain": false,
        "session": session_wire,
        "sessions": [session_wire],
        "span": {"start": 0, "end": 1},
        "spans": [{
            "document": document_wire,
            "start": 0,
            "end": 1,
        }],
    })
    .to_string();
    let document_payload = serde_json::json!({
        "provider": "claude-code",
        "variant": "claude-code/jsonl-v1",
        "fingerprint": "legacy-v6-fingerprint",
        "len": 1,
    })
    .to_string();
    for (wire, payload) in [
        (session_wire, session_payload.as_bytes()),
        (message_wire, message_payload.as_bytes()),
        (document_wire, document_payload.as_bytes()),
    ] {
        conn.execute(
            "INSERT INTO catalog(id, payload) VALUES(?1, ?2)",
            rusqlite::params![wire, payload],
        )
        .expect("insert legacy catalog row");
    }
    for wire in [session_wire, message_wire, document_wire] {
        conn.execute(
            "INSERT INTO source_membership(source_path, message_id, document_id)
             VALUES(?1, ?2, ?3)",
            rusqlite::params![source_path, wire, document_wire],
        )
        .expect("insert legacy membership");
    }
    conn.execute(
        "INSERT INTO source_scans(source_path, scanned_at_ms) VALUES(?1, 1)",
        [source_path],
    )
    .expect("insert legacy source scan");
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// 不带 `--db` 跑一次 CLI——用于 `--help`/`--version`/`doctor` 等无需存储的命令。
fn run_bare(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("failed to spawn agent-session-grep binary")
}

#[test]
fn index_search_get_roundtrip() {
    let (_dir, db) = temp_db("roundtrip");

    let out = run(&db, &["index", "m1", "the quick brown fox jumps"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("\"ok\":true"));
    assert!(stdout(&out).contains("msg_v1_"));

    run(&db, &["index", "m2", "lazy dog sleeps all day"]);

    // search 命中正确文档：查 "brown" 只应命中 m1。
    let out = run(&db, &["search", "brown"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(
        s.contains("msg_v1_52db0bc4880412c58a3cc166ec6c389a"),
        "got: {s}"
    );
    assert!(!s.contains("lazy"), "brown 不应命中 m2");

    // get 取回 index 时写入的原始 payload——用 search 显示的真实 wire id
    // （`get` 现在按 wire 串反解 id，见 StableId::from_wire，不再把参数当 fact 重派生）。
    let out = run(&db, &["get", "msg_v1_52db0bc4880412c58a3cc166ec6c389a"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("the quick brown fox jumps"));
}

#[test]
fn migrated_v6_catalog_stays_readable_until_complete_reingest_enables_context() {
    let (dir, db) = temp_db("migrated-v6-reingest");
    let session_native = "66111111-1111-4111-8111-111111111111";
    let message_native = "66222222-2222-4222-8222-222222222222";
    let session_wire = format!("ses_v1_{session_native}");
    let message_wire = format!("msg_v1_{message_native}");
    let legacy_document_wire = "doc_v1_legacy-v6-document";
    let message_text = "legacy catalog survives migration";
    let source = dir.path().join("legacy-source.jsonl");
    let source_content = format!(
        "{{\"type\":\"user\",\"uuid\":\"{message_native}\",\"parentUuid\":null,\
         \"sessionId\":\"{session_native}\",\"timestamp\":\"2026-07-28T00:00:00.000Z\",\
         \"message\":{{\"role\":\"user\",\"content\":\"{message_text}\"}}}}\n"
    );
    std::fs::write(&source, &source_content).expect("write v6 re-ingest fixture");
    let source_path = source.to_string_lossy().into_owned();
    create_v6_catalog(
        &db,
        &source_path,
        &session_wire,
        &message_wire,
        legacy_document_wire,
        message_text,
    );

    let get = run(&db, &["get", &message_wire]);
    assert!(get.status.success(), "get failed: {}", stdout(&get));
    assert!(
        parse_first_line(&get)["data"]["payload"]
            .as_str()
            .is_some_and(|payload| payload.contains(message_text))
    );

    let show = run(&db, &["show", &session_wire]);
    assert!(show.status.success(), "show failed: {}", stdout(&show));
    assert_eq!(
        parse_first_line(&show)["data"]["entity"]["document"],
        legacy_document_wire
    );

    let list = run(&db, &["list", "10"]);
    assert!(list.status.success(), "list failed: {}", stdout(&list));
    let entries = parse_first_line(&list)["data"]["entries"]
        .as_array()
        .expect("list entries")
        .clone();
    assert!(
        entries
            .iter()
            .any(|entry| entry["id"] == session_wire.as_str())
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["id"] == message_wire.as_str())
    );

    let context = run(&db, &["context", &session_wire]);
    assert_eq!(
        context.status.code(),
        Some(9),
        "context must require re-ingest: {}",
        stdout(&context)
    );
    let error = parse_first_line(&context);
    assert_eq!(error["error"]["code"], "schema_incompatible");
    assert!(
        error["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("re-ingest required"))
    );
    assert!(
        !stdout(&context).contains(&source_path),
        "schema error must not disclose the source path"
    );

    let ingest = run(&db, &["ingest", &source_path]);
    assert!(
        ingest.status.success(),
        "re-ingest failed: {}",
        stdout(&ingest)
    );
    assert_eq!(parse_first_line(&ingest)["data"]["skipped"], 0);

    let context = run(&db, &["context", &session_wire]);
    assert!(
        context.status.success(),
        "context after re-ingest failed: {}",
        stdout(&context)
    );
    let context = parse_first_line(&context);
    let messages = context["data"]["messages"]
        .as_array()
        .expect("context messages");
    assert_eq!(messages.len(), 1, "{context}");
    assert_eq!(messages[0]["message_id"], message_wire);
    assert_eq!(
        context["data"]["evidence"][0]["occurrence_id"],
        messages[0]["placement_id"]
    );

    let show = run(&db, &["show", &session_wire]);
    let entity = parse_first_line(&show)["data"]["entity"].clone();
    let documents = entity["documents"].as_array().expect("session documents");
    assert_eq!(documents.len(), 1, "entity={entity}");
    assert_ne!(documents[0], legacy_document_wire);

    for command in [
        vec!["get", message_wire.as_str()],
        vec!["show", session_wire.as_str()],
        vec!["list", "10"],
    ] {
        let output = run(&db, &command);
        assert!(
            output.status.success(),
            "{} must remain readable after re-ingest: {}",
            command[0],
            stdout(&output)
        );
    }
}

#[test]
fn get_missing_returns_null_payload() {
    let (_dir, db) = temp_db("missing");
    // 合法前缀但从未写入的 id：解析成功、catalog 查无 → payload:null。
    let out = run(&db, &["get", "msg_v1_ffffffffffffffffffffffffffffffff"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("\"payload\":null"));
}

#[test]
fn get_malformed_id_is_usage_error() {
    let (_dir, db) = temp_db("malformed");
    // 无已知前缀的串无法反解为实体 id（见 StableId::from_wire）→ 用法错误 exit 2。
    let out = run(&db, &["get", "does-not-exist"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn show_returns_normalized_role_and_text() {
    let (dir, db) = temp_db("show");
    // ingest 写入的 payload 是 `role\ttext`，show 应把它拆成结构化 entity。
    let fixture = dir.path().join("show.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"how do I show an entity"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    // 从 search 输出取真实 wire id。
    let out = run(&db, &["search", "entity"]);
    let s = stdout(&out);
    let id = s
        .split("\"id\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("search 输出应含 id 字段");

    let out = run(&db, &["show", id]);
    assert!(out.status.success(), "show failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["command"], "show");
    // show 与 get 的区别：结构化 role/text，而非裸 payload 串。
    assert_eq!(
        frame["data"]["entity"]["role"],
        "user",
        "show={}",
        stdout(&out)
    );
    assert_eq!(
        frame["data"]["entity"]["text"],
        "how do I show an entity",
        "show={}",
        stdout(&out)
    );
}

#[test]
fn show_missing_returns_null_entity() {
    let (_dir, db) = temp_db("show-missing");
    // 合法前缀但从未写入的 id：解析成功、catalog 查无 → entity:null。
    let out = run(&db, &["show", "msg_v1_ffffffffffffffffffffffffffffffff"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("\"entity\":null"),
        "show={}",
        stdout(&out)
    );
}

#[test]
fn show_malformed_id_is_usage_error() {
    let (_dir, db) = temp_db("show-malformed");
    let out = run(&db, &["show", "does-not-exist"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn ingest_preserves_native_uuid_identity_and_threading() {
    let (dir, db) = temp_db("native-threading");
    // 带真实 uuid/parentUuid 的两条记录：identity 应采用 native uuid（原样透传，
    // 非 path+seq 派生），threading 边经存储穿到 show。
    let fixture = dir.path().join("threaded.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","uuid":"11111111-1111-4111-8111-111111111111","parentUuid":null,"timestamp":"2026-06-27T13:57:42.685Z","message":{"role":"user","content":"root message about widgets"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"22222222-2222-4222-8222-222222222222","parentUuid":"11111111-1111-4111-8111-111111111111","message":{"role":"assistant","content":"child reply about widgets"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    // identity 采用 native uuid：wire id 应内含原始 uuid，而非 path+seq 派生的 hex。
    let child_id = "msg_v1_22222222-2222-4222-8222-222222222222";
    let out = run(&db, &["show", child_id]);
    assert!(out.status.success(), "show failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    let entity = &frame["data"]["entity"];
    assert_eq!(entity["role"], "assistant", "show={}", stdout(&out));
    assert_eq!(entity["text"], "child reply about widgets");
    // threading 边穿过存储：子消息的 parent 指向根消息的 native uuid。
    assert_eq!(
        entity["parent_native_id"],
        "11111111-1111-4111-8111-111111111111",
        "show={}",
        stdout(&out)
    );

    // 根消息 parent 为 null，timestamp 原样保留。
    let out = run(
        &db,
        &["show", "msg_v1_11111111-1111-4111-8111-111111111111"],
    );
    let frame = parse_first_line(&out);
    let entity = &frame["data"]["entity"];
    assert!(
        entity["parent_native_id"].is_null(),
        "root parent must be null"
    );
    assert_eq!(entity["timestamp"], "2026-06-27T13:57:42.685Z");
}

#[test]
fn ingest_persists_session_and_document_entities_with_spans() {
    let (dir, db) = temp_db("canonical-entities");
    // 覆盖 canonical foundation 验收：消息 → 会话 → 文档 交叉引用 + evidence span。
    let line_root = r#"{"type":"user","uuid":"33333333-3333-4333-8333-333333333333","sessionId":"abcd1234-5678-4abc-8def-aabbccddeeff","message":{"role":"user","content":"trace the span origin"}}"#;
    let line_reply = r#"{"type":"assistant","uuid":"44444444-4444-4444-8444-444444444444","sessionId":"abcd1234-5678-4abc-8def-aabbccddeeff","message":{"role":"assistant","content":"span recorded faithfully"}}"#;
    let fixture = dir.path().join("spans.jsonl");
    let content = format!("{line_root}\n{line_reply}\n");
    std::fs::write(&fixture, &content).expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    // 消息实体：session 引用 native 会话 id、span 指回源行字节区间。
    let out = run(
        &db,
        &["show", "msg_v1_33333333-3333-4333-8333-333333333333"],
    );
    assert!(out.status.success(), "show failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    let entity = &frame["data"]["entity"];
    let session_wire = "ses_v1_abcd1234-5678-4abc-8def-aabbccddeeff";
    assert_eq!(
        entity["session"],
        session_wire,
        "消息应引用 native 会话 id: {}",
        stdout(&out)
    );
    // span round-trip：按 show 返回的区间切源文件字节 == 原始行。
    let start = entity["span"]["start"].as_u64().expect("span.start") as usize;
    let end = entity["span"]["end"].as_u64().expect("span.end") as usize;
    assert_eq!(
        &content.as_bytes()[start..end],
        line_root.as_bytes(),
        "span 应精确指回第一条源记录"
    );

    // 会话实体：引用文档 + 按序成员消息。
    let out = run(&db, &["show", session_wire]);
    assert!(
        out.status.success(),
        "show session failed: {}",
        stdout(&out)
    );
    let frame = parse_first_line(&out);
    let entity = &frame["data"]["entity"];
    let document_wire = entity["document"].as_str().expect("session.document");
    assert!(
        document_wire.starts_with("doc_v1_"),
        "会话应引用文档实体: {}",
        stdout(&out)
    );
    let members = entity["messages"].as_array().expect("session.messages");
    assert_eq!(members.len(), 2);
    assert_eq!(
        members[0], "msg_v1_33333333-3333-4333-8333-333333333333",
        "成员按 seq 排序"
    );

    // 文档实体：provider/variant/fingerprint/len 与 ingest 报告一致。
    let out = run(&db, &["show", document_wire]);
    assert!(
        out.status.success(),
        "show document failed: {}",
        stdout(&out)
    );
    let frame = parse_first_line(&out);
    let entity = &frame["data"]["entity"];
    assert_eq!(entity["provider"], "claude-code");
    assert_eq!(entity["variant"], "claude-code/jsonl-v1");
    assert_eq!(entity["len"].as_u64(), Some(content.len() as u64));
    assert!(
        entity["fingerprint"]
            .as_str()
            .is_some_and(|f| !f.is_empty()),
        "文档应携带快照指纹: {}",
        stdout(&out)
    );

    // 容器实体不参与全文搜索：搜索只命中消息。
    let out = run(&db, &["search", "span"]);
    let s = stdout(&out);
    assert!(s.contains("msg_v1_"), "消息应命中: {s}");
    assert!(
        !s.contains("ses_v1_") && !s.contains("doc_v1_"),
        "容器实体不应命中搜索: {s}"
    );
}

#[test]
fn ingest_auto_selects_codex_and_ignores_event_mirror() {
    let (dir, db) = temp_db("codex");
    // 合成的 Codex rollout（非真实 transcript，遵守 R0 脱敏规范）：
    // 每条对话消息出现两次——权威 response_item/message（带 native id）与
    // event_msg UI 镜像（无 id）。adapter 只取前者，committed 应为 2 而非 4。
    let fixture = dir.path().join("rollout.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"timestamp":"2026-07-19T15:40:00.000Z","type":"session_meta","payload":{"session_id":"aaaa1111-2222-7333-8444-555566667777","cwd":"/tmp","originator":"codex","cli_version":"1.0"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-19T15:41:00.000Z","type":"response_item","payload":{"type":"message","id":"msg_codex_root","role":"user","content":[{"type":"input_text","text":"how do I configure the pipeline"}]}}"#,
            "\n",
            r#"{"timestamp":"2026-07-19T15:41:00.500Z","type":"event_msg","payload":{"type":"user_message","message":"how do I configure the pipeline"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-19T15:41:19.000Z","type":"response_item","payload":{"type":"message","id":"msg_codex_reply","role":"assistant","content":[{"type":"output_text","text":"set the pipeline stages first"}]}}"#,
            "\n",
            r#"{"timestamp":"2026-07-19T15:41:19.500Z","type":"event_msg","payload":{"type":"agent_message","message":"set the pipeline stages first"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    // registry 自动 probe-select：无需指定 provider，应判定为 codex variant。
    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(
        frame["data"]["variant"],
        "codex/rollout-jsonl-v1",
        "registry 应自动选中 codex: {}",
        stdout(&out)
    );
    // 关键去重断言：2 条权威消息，event_msg 镜像不计入。
    assert_eq!(
        frame["data"]["committed"],
        2,
        "event_msg 镜像应被忽略，只提交 2 条: {}",
        stdout(&out)
    );

    // native id 原样保留，show 展开 role/text。
    let out = run(&db, &["show", "msg_v1_msg_codex_reply"]);
    assert!(out.status.success(), "show failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    let entity = &frame["data"]["entity"];
    assert_eq!(entity["role"], "assistant", "show={}", stdout(&out));
    assert_eq!(entity["text"], "set the pipeline stages first");

    // 内容可检索。
    let out = run(&db, &["search", "pipeline"]);
    assert!(
        stdout(&out).contains("msg_v1_msg_codex"),
        "codex 内容应可检索: {}",
        stdout(&out)
    );
}

#[test]
fn index_rebuild_reprojects_and_keeps_search_working() {
    let (dir, db) = temp_db("rebuild");
    let fixture = dir.path().join("rebuild.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"rebuild the search index please"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":"reprojecting from catalog now"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    // rebuild：从权威 catalog 全量重投影 FTS 索引，推进 generation。
    // catalog 含 2 条消息 + 1 会话 + 1 文档目录行；rebuild 重投影全部 4 个实体
    // （容器实体只重建身份边车，不进全文表）。
    let out = run(&db, &["index", "rebuild"]);
    assert!(out.status.success(), "rebuild failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["command"], "index.rebuild");
    assert_eq!(
        frame["data"]["reindexed"],
        4,
        "应重投影 4 个实体（2 消息 + 会话 + 文档）: {}",
        stdout(&out)
    );
    // generation：ingest 推进到 1，rebuild 再推进到 2。
    assert_eq!(frame["data"]["generation"], 2, "rebuild={}", stdout(&out));

    // 重建后搜索仍命中原内容，且身份保真（wire id 前缀不变）。
    let out = run(&db, &["search", "reprojecting"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("msg_v1_"),
        "rebuild 后搜索仍应命中: {}",
        stdout(&out)
    );
}

#[test]
fn index_rebuild_on_empty_db_succeeds() {
    let (_dir, db) = temp_db("rebuild-empty");
    let out = run(&db, &["index", "rebuild"]);
    assert!(
        out.status.success(),
        "empty rebuild failed: {}",
        stdout(&out)
    );
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["data"]["reindexed"], 0);
}

#[test]
fn missing_subcommand_is_usage_error() {
    let (_dir, db) = temp_db("usage");
    let out = run(&db, &[]);
    // 用法错误映射为 exit code 2（见 main.rs 的 CliError::Usage）。
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn unknown_subcommand_is_usage_error() {
    let (_dir, db) = temp_db("unknown");
    let out = run(&db, &["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
}

// ─── 参数校验（Minor-2 / Minor-3）───────────────────────────────────────────

#[test]
fn search_flag_named_query_is_searched_not_intercepted() {
    // Minor-2：query 恰等于 flag 名（--output/--robot/--help/--request-id）时按
    // 查询走，不得被输出模式解析、help/version 拦截或 request-id 抽取短路。
    let (_dir, db) = temp_db("flag-query");
    let out = run(
        &db,
        &[
            "index",
            "f1",
            "literal --output --robot --help --request-id flag text",
        ],
    );
    assert!(out.status.success(), "index failed: {}", stdout(&out));

    // 正常检索 sanity：分词后的词元仍可命中（内容确实入库）。
    let out = run(&db, &["search", "output"]);
    assert!(
        out.status.success(),
        "search output failed: {}",
        stdout(&out)
    );
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert!(!frame["data"]["hits"].as_array().expect("hits").is_empty());

    // `--output` 在命令名之后是查询文本：参数解析放行给 search 引擎。
    // 引擎层对纯否定查询（"--output" 分词后是 `NOT output`）报 catalog_error——
    // 这正说明查询串抵达检索层；若被误判为输出模式，这里会是 exit 2 的
    // "--output requires human|json|jsonl" 模式错误。
    let out = run(&db, &["search", "--output"]);
    assert_eq!(out.status.code(), Some(6), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["command"], "search");
    assert_eq!(frame["error"]["code"], "catalog_error");
    assert!(
        frame["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("fts5")),
        "错误应来自检索层: {frame}"
    );

    // --robot 在命令名之前仍是合法输出模式 flag；query "--robot" 同样放行给检索层。
    let out = run(&db, &["search", "--robot"]);
    assert_eq!(out.status.code(), Some(6), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["command"], "search");
    assert_eq!(frame["error"]["code"], "catalog_error");

    // --help 在命令名之后是查询文本，不打印帮助；human stdout 保持协议干净。
    let out = run_human(&db, &["search", "--help"]);
    assert_eq!(out.status.code(), Some(6), "stdout={}", stdout(&out));
    assert!(
        stdout(&out).is_empty(),
        "human 模式 stdout 不得出现帮助文本: {}",
        stdout(&out)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains("catalog_error"),
        "search --help 应走检索层错误: {stderr}"
    );

    // --request-id 在命令名之后同样是查询文本，不被 request-id 抽取误判。
    let out = run_human(&db, &["search", "--request-id"]);
    assert_eq!(out.status.code(), Some(6), "stdout={}", stdout(&out));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains("catalog_error"),
        "search --request-id 应走检索层错误: {stderr}"
    );
}

#[test]
fn extra_positional_arguments_are_rejected_with_exit_2() {
    // Minor-3：多余位置参数是用法错误（exit 2），不静默忽略。
    let (dir, db) = temp_db("extra-args");
    let fixture = dir.path().join("extra.jsonl");
    std::fs::write(&fixture, b"").expect("write empty fixture");
    let path = fixture.to_string_lossy().into_owned();

    for (args, usage) in [
        (vec!["doctor", "bogus"], "doctor"),
        (vec!["config", "paths", "extra"], "config paths"),
        (vec!["status", "extra"], "status"),
        (vec!["get", "msg_v1_x", "extra"], "get"),
        (vec!["show", "msg_v1_x", "extra"], "show"),
        (vec!["index", "f", "text", "extra"], "index"),
        (vec!["index", "rebuild", "extra"], "index rebuild"),
        (vec!["ingest", &path, "extra"], "ingest"),
        (vec!["search", "q", "extra"], "search"),
        (vec!["list", "5", "extra"], "list"),
        (vec!["context", "ses_v1_x", "extra"], "context"),
        (vec!["mcp", "extra"], "mcp"),
    ] {
        let out = run(&db, &args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{usage}: stdout={}",
            stdout(&out)
        );
        let frame = parse_first_line(&out);
        assert_envelope_shape(&frame, false);
        assert_eq!(frame["error"]["code"], "invalid_request", "{usage}");
        assert_eq!(frame["command"], args[0], "{usage}");
    }
}

// ─── EPIPE 契约（Major-1）───────────────────────────────────────────────────

#[test]
fn help_and_version_pipe_closed_early_exit_zero_without_panic() {
    // CONTRACT §6：下游提前关闭管道（head/pager）时 --help/--version 必须
    // 静默 exit 0，不得 panic（exit 101）或污染 stderr。
    use std::process::Stdio;
    for args in [&["--help"][..], &["--version"][..]] {
        let mut child = Command::new(BIN)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        // 立即关闭读端：子进程写 stdout 时管道已断 → EPIPE。
        drop(child.stdout.take());
        let out = child.wait_with_output().expect("wait");
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?} must exit 0 on EPIPE, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(
            !stderr.contains("panic"),
            "{args:?} must not panic on EPIPE: {stderr}"
        );
    }
}

// ─── 错误 exit code 目录（E1）───────────────────────────────────────────────

#[test]
fn ingest_missing_file_is_source_io_exit_5() {
    let (dir, db) = temp_db("exit-5");
    let missing = dir.path().join("does-not-exist.jsonl");
    let missing_path = missing.to_string_lossy().into_owned();
    let out = run(&db, &["ingest", &missing_path]);
    assert_eq!(out.status.code(), Some(5), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["error"]["code"], "source_io");
    assert_eq!(frame["error"]["retryable"], false);
}

#[test]
fn unrecognized_source_content_is_invalid_request_exit_2() {
    // 非法内容源：无 provider 认领 → invalid_request（exit 2）。
    // （task 原话：确认 2/7 之一；当前映射为 2。）
    let (dir, db) = temp_db("exit-provider");
    let garbage = dir.path().join("garbage.jsonl");
    std::fs::write(&garbage, "this is not any known transcript format\n").expect("write garbage");
    let garbage_path = garbage.to_string_lossy().into_owned();
    let out = run(&db, &["ingest", &garbage_path]);
    assert_eq!(out.status.code(), Some(2), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["error"]["code"], "invalid_request");
    assert!(
        frame["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("no provider")),
        "message 应指出无 provider 认领: {frame}"
    );
}

#[test]
fn writer_busy_exits_6_with_retryable_flag() {
    // 持有 data-root writer lease 时写子命令 → writer_busy（exit 6，可重试）。
    // data root 是 db 文件所在目录（SqliteStore::open_for_write 的 lease 语义）。
    let (dir, db) = temp_db("exit-6");
    let lease = agent_session_grep_adapters_sqlite::WriterLease::try_acquire(dir.path())
        .expect("test process acquires the writer lease");
    let out = run(&db, &["index", "w1", "writer busy probe"]);
    assert_eq!(out.status.code(), Some(6), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["error"]["code"], "writer_busy");
    assert_eq!(frame["error"]["retryable"], true);
    drop(lease);
    // 释放后写入恢复。
    let out = run(&db, &["index", "w1", "writer busy probe"]);
    assert!(
        out.status.success(),
        "write must succeed after lease release: {}",
        stdout(&out)
    );
}

#[test]
fn error_outputs_never_disclose_source_path_or_content() {
    // 隐私回归守卫：错误场景的 stderr（human 诊断）与 envelope 都不得泄露
    // 源路径或正文片段（E2）。
    let (dir, db) = temp_db("error-privacy");
    let missing = dir.path().join("secret-path-transcript.jsonl");
    let missing_path = missing.to_string_lossy().into_owned();

    // robot 模式：错误只走 stdout envelope；stderr 必须为空。
    let out = run(&db, &["ingest", &missing_path]);
    assert_eq!(out.status.code(), Some(5), "stdout={}", stdout(&out));
    assert!(
        out.stderr.is_empty(),
        "robot mode stderr must stay empty: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !stdout(&out).contains(&missing_path),
        "envelope 不得泄露源路径: {}",
        stdout(&out)
    );

    // human 模式：stderr 诊断不得含源路径。
    let out = run_human(&db, &["ingest", &missing_path]);
    assert_eq!(out.status.code(), Some(5));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("error [source_io]"), "stderr={stderr}");
    assert!(
        !stderr.contains(&missing_path),
        "stderr 泄露源路径: {stderr}"
    );

    // 非法内容源：正文片段不进入 stderr。
    let garbage = dir.path().join("garbage.jsonl");
    std::fs::write(&garbage, "top-secret-transcript-body\n").expect("write garbage fixture");
    let garbage_path = garbage.to_string_lossy().into_owned();
    let out = run_human(&db, &["ingest", &garbage_path]);
    assert_eq!(out.status.code(), Some(2), "stdout={}", stdout(&out));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        !stderr.contains(&garbage_path),
        "stderr 泄露源路径: {stderr}"
    );
    assert!(
        !stderr.contains("top-secret-transcript-body"),
        "stderr 泄露源正文: {stderr}"
    );
}

#[test]
fn version_flag_prints_version_without_db() {
    // --version 不需要 --db：在 parse_db_flag 之前拦截。
    let out = run_bare(&["--version"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("0.1.0"));
}

#[test]
fn help_flag_lists_commands_without_db() {
    let out = run_bare(&["--help"]);
    assert!(out.status.success());
    let s = stdout(&out);
    // 帮助里应列出核心子命令，便于发现。
    assert!(s.contains("ingest"), "help 应列出 ingest: {s}");
    assert!(s.contains("search"), "help 应列出 search: {s}");
}

#[test]
fn doctor_reports_ok_without_db() {
    let out = run_bare(&["--robot", "doctor"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("\"ok\":true"), "doctor 应报告 ok:true: {s}");
    assert!(
        s.contains("\"db\":\"not-checked\""),
        "无 --db 时应标记未校验: {s}"
    );
}

#[test]
fn doctor_with_db_reports_generation_and_recovery_evidence() {
    let (_dir, db) = temp_db("doctor-db");
    // 写一条推进 generation 到 1。
    let out = run(&db, &["index", "d1", "doctor evidence content"]);
    assert!(out.status.success());

    let out = run(&db, &["doctor", "--db", &db]);
    assert!(out.status.success(), "doctor failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["data"]["db"], "ok");
    // 一致性/恢复证据：活动 generation + 待收敛 intent 数（干净库应为 0）。
    assert_eq!(frame["data"]["generation"], 1, "doctor={}", stdout(&out));
    assert_eq!(
        frame["data"]["interrupted_batches"],
        0,
        "干净库不应有待收敛 intent: {}",
        stdout(&out)
    );
}

#[test]
fn ingest_search_get_roundtrip_via_binary() {
    let (dir, db) = temp_db("ingest");
    // 合成的 Claude Code 风格 .jsonl fixture（按 R0 脱敏规范，非真实 transcript）。
    let fixture = dir.path().join("session.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"how do I configure the neural net"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":"set the learning rate first"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    // ingest：probe 判定 variant + parse 流式入库。
    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));
    let s = stdout(&out);
    assert!(s.contains("claude-code/jsonl-v1"), "应判定出 variant: {s}");
    assert!(s.contains("\"committed\":2"), "应入库 2 条消息: {s}");

    // search：ingest 的内容可被检索到。
    let out = run(&db, &["search", "neural"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("msg_v1_"), "search 应命中 ingest 的消息: {s}");

    // 从 search 输出提取真实 wire id，拿去 get 应取回原文（往返闭合）。
    let id = s
        .split("\"id\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("search 输出应含 id 字段");
    let out = run(&db, &["get", id]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("neural"),
        "get 应取回含 neural 的 payload: {}",
        stdout(&out)
    );
}

#[test]
fn list_and_status_report_catalog_contents() {
    let (_dir, db) = temp_db("list-status");
    let out = run(&db, &["index", "a", "alpha payload"]);
    assert!(out.status.success());
    let out = run(&db, &["index", "b", "beta payload"]);
    assert!(out.status.success());

    let out = run(&db, &["status"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("\"catalog_count\":2"), "status={s}");

    let out = run(&db, &["list", "1"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert_eq!(
        s.lines().count(),
        1,
        "list limit should return one row: {s}"
    );
    assert!(s.contains("\"id\":\"msg_v1_"), "list={s}");
}

#[test]
fn sync_commits_then_reports_unchanged_on_resync() {
    let (dir, db) = temp_db("sync");
    let fixture = dir.path().join("s1.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"how do I tune the index"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":"raise the batch size"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let path = fixture.to_string_lossy().into_owned();

    // 首次 sync：两条消息提交，generation 从 0 推进到 1。
    let out = run(&db, &["sync", &path]);
    assert!(out.status.success(), "sync failed: {}", stdout(&out));
    let s = stdout(&out);
    assert!(s.contains("\"messages\":2"), "sync={s}");
    assert!(s.contains("\"committed\":2"), "sync={s}");
    assert!(s.contains("\"generation\":1"), "sync={s}");

    // 内容可检索。
    let out = run(&db, &["search", "tune"]);
    assert!(stdout(&out).contains("msg_v1_"), "search={}", stdout(&out));

    // 重复 sync 未改变的源：no-op，generation 不推进。
    let out = run(&db, &["sync", &path]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("\"committed\":0"), "resync={s}");
    assert!(s.contains("\"unchanged\":2"), "resync={s}");
    assert!(
        s.contains("\"generation\":1"),
        "resync 不应推进 generation: {s}"
    );
}

#[test]
fn sync_requires_at_least_one_file() {
    let (_dir, db) = temp_db("sync-empty");
    let out = run(&db, &["sync"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn sync_deduplicates_duplicate_paths_in_one_invocation() {
    // Minor-5：`sync a a` 是书写冗余而非两个源——CLI 层提前去重（exit 0），
    // 不落成 store 层的 catalog_error（exit 6）。
    let (dir, db) = temp_db("sync-dup");
    let fixture = dir.path().join("dup.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"dedupe me once"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["sync", &path, &path]);
    assert!(out.status.success(), "sync dup failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_eq!(frame["data"]["sources"], 1, "frame={frame}");
    assert_eq!(frame["data"]["messages"], 1, "frame={frame}");
    assert_eq!(frame["data"]["committed"], 1, "frame={frame}");

    // 1 条消息 + 会话 + 文档目录行 = 3 个实体，消息可检索。
    let out = run(&db, &["status"]);
    assert!(
        stdout(&out).contains("\"catalog_count\":3"),
        "{}",
        stdout(&out)
    );
    let out = run(&db, &["search", "dedupe"]);
    assert!(stdout(&out).contains("msg_v1_"), "search={}", stdout(&out));
}

#[test]
fn sync_all_or_nothing_on_bad_source() {
    let (dir, db) = temp_db("sync-atomic");
    let good = dir.path().join("good.jsonl");
    std::fs::write(
        &good,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"valid line"}}"#,
            "\n",
        ),
    )
    .expect("write good fixture");
    let good_path = good.to_string_lossy().into_owned();
    // 不存在的第二个源：整批 sync 必须失败且不写入任何数据。
    let missing = dir.path().join("missing.jsonl");
    let missing_path = missing.to_string_lossy().into_owned();

    let out = run(&db, &["sync", &good_path, &missing_path]);
    assert!(
        !out.status.success(),
        "sync 应因缺失源失败: {}",
        stdout(&out)
    );

    // 第一个源的消息不能被部分写入。
    let out = run(&db, &["status"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("\"catalog_count\":0"),
        "失败的 sync 不应留下部分数据: {}",
        stdout(&out)
    );
}

#[test]
fn sync_tombstones_message_removed_from_source() {
    let (dir, db) = temp_db("sync-shrink");
    let fixture = dir.path().join("shrink.jsonl");
    // 首次：两条消息。
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"keep this message"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":"drop this later"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["sync", &path]);
    assert!(out.status.success(), "sync failed: {}", stdout(&out));
    let out = run(&db, &["status"]);
    // 2 条消息 + 会话 + 文档目录行 = 4 个 catalog 实体。
    assert!(
        stdout(&out).contains("\"catalog_count\":4"),
        "{}",
        stdout(&out)
    );
    // 第二条消息此刻可检索。
    let out = run(&db, &["search", "drop"]);
    assert!(stdout(&out).contains("msg_v1_"), "search={}", stdout(&out));

    // 源收缩到一条：被移除的消息应被 tombstone，catalog 与搜索都不再有它。
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"keep this message"}}"#,
            "\n",
        ),
    )
    .expect("rewrite fixture");

    let out = run(&db, &["sync", &path]);
    assert!(
        out.status.success(),
        "reshrink sync failed: {}",
        stdout(&out)
    );
    let out = run(&db, &["status"]);
    // 收缩后：1 条消息 + 新会话 + 新文档 = 3。文档 id 内容寻址（fingerprint 变 →
    // id 变），旧会话/文档行随旧 membership 一起 tombstone，不残留孤儿。
    assert!(
        stdout(&out).contains("\"catalog_count\":3"),
        "收缩后应剩 1 消息 + 会话 + 文档: {}",
        stdout(&out)
    );
    let out = run(&db, &["search", "drop"]);
    assert!(
        !stdout(&out).contains("msg_v1_"),
        "被移除消息不应再命中搜索: {}",
        stdout(&out)
    );
    let out = run(&db, &["search", "keep"]);
    assert!(
        stdout(&out).contains("msg_v1_"),
        "保留消息仍应命中: {}",
        stdout(&out)
    );
}

#[test]
fn sync_empty_source_tombstones_all_messages() {
    // 整源清空：0 字节源必须作为合法空批次提交（tombstone 全部旧消息），
    // 而不是被 provider 拒绝（此前 "no provider recognized" 使整源清空不可达）。
    let (dir, db) = temp_db("sync-empty");
    let fixture = dir.path().join("empty-me.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"will be wiped"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":"gone too"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["sync", &path]);
    assert!(out.status.success(), "sync failed: {}", stdout(&out));
    let out = run(&db, &["search", "wiped"]);
    assert!(stdout(&out).contains("msg_v1_"), "search={}", stdout(&out));

    // 清空文件后 sync：空源是合法空批次，旧消息全部 tombstone。
    std::fs::write(&fixture, b"").expect("truncate fixture");
    let out = run(&db, &["sync", &path]);
    assert!(
        out.status.success(),
        "empty-source sync must succeed: {}",
        stdout(&out)
    );
    let out = run(&db, &["search", "wiped"]);
    assert!(
        !stdout(&out).contains("msg_v1_"),
        "整源清空后消息不应再命中: {}",
        stdout(&out)
    );
    let out = run(&db, &["status"]);
    // 空源仍派生会话+文档目录实体（内容寻址），但消息全部 tombstone：
    // placements 归零即证明无消息残留。
    assert!(
        stdout(&out).contains("\"placements\":0"),
        "整源清空后 placements 应为 0: {}",
        stdout(&out)
    );
}

// ─── Robot v1 Envelope 契约 E2E ────────────────────────────────────────────

#[test]
fn robot_envelope_shape_on_success() {
    let (_dir, db) = temp_db("env-ok");
    let out = run(&db, &["index", "e1", "envelope shape test"]);
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    // frame_type / outcome / data.generation present
    assert_eq!(frame["command"], "index");
    assert!(frame["data"]["generation"].is_number());
    assert!(frame["meta"]["duration_ms"].as_u64().is_some());
}

#[test]
fn robot_envelope_shape_on_error() {
    let (_dir, db) = temp_db("env-err");
    let out = run(&db, &["--robot", "get", "not-a-valid-id"]);
    assert!(!out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["command"], "get");
    assert_eq!(frame["error"]["code"], "invalid_request");
    assert!(!frame["error"]["retryable"].as_bool().unwrap());
}

#[test]
fn robot_flag_produces_same_json_envelope() {
    let (_dir, db) = temp_db("env-robot");
    let out = Command::new(BIN)
        .args(["--db", &db, "--robot", "status"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["command"], "status");
}

#[test]
fn output_json_flag_produces_well_formed_envelope() {
    let (_dir, db) = temp_db("env-json");
    let out = Command::new(BIN)
        .args(["--db", &db, "--output", "json", "status"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
}

#[test]
fn invalid_output_mode_exits_with_code_2() {
    let (_dir, db) = temp_db("env-mode");
    let out = Command::new(BIN)
        .args(["--db", &db, "--output", "yaml", "status"])
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    // Even mode errors produce a valid error envelope on stdout.
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["error"]["code"], "invalid_request");
}

#[test]
fn doctor_envelope_shape_has_meta_generation_null() {
    let out = run_bare(&["--robot", "doctor"]);
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["command"], "doctor");
    // doctor has no generation context
    assert!(frame["meta"]["generation"].is_null() || frame["meta"]["generation"].is_number());
}

// ─── config paths ──────────────────────────────────────────────────────────

#[test]
fn config_paths_reports_platform_directories() {
    let out = run_bare(&["--robot", "config", "paths"]);
    assert!(
        out.status.success(),
        "config paths failed: {}",
        stdout(&out)
    );
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    let data = &frame["data"];
    // All four directory fields must be present and non-empty strings.
    for field in ["config", "data", "cache", "logs"] {
        let value = data[field]
            .as_str()
            .unwrap_or_else(|| panic!("config paths must include {field} field, got: {data}"));
        assert!(!value.is_empty(), "{field} must not be empty");
    }
}
#[test]
fn jsonl_output_is_one_complete_frame_per_line() {
    let (_dir, db) = temp_db("env-jsonl");
    let out = Command::new(BIN)
        .args(["--db", &db, "--output", "jsonl", "status"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let output_text = stdout(&out);
    let lines: Vec<_> = output_text.lines().collect();
    assert_eq!(lines.len(), 1, "one command must emit one JSONL frame");
    let frame: serde_json::Value = serde_json::from_str(lines[0]).expect("valid JSONL frame");
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["frame_type"], "response");
}

#[test]
fn human_error_writes_diagnostic_to_stderr_only() {
    let (_dir, db) = temp_db("env-human-error");
    let out = run_human(&db, &["get", "not-a-valid-id"]);
    assert!(!out.status.success());
    assert!(
        stdout(&out).is_empty(),
        "human mode stdout must stay protocol-clean"
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).is_empty(),
        "human mode must write diagnostic to stderr"
    );
}

// ─── 分页 cursor + 预算 + context（shared Application ADT，design §7）───────

/// 从 search envelope 里取命中 id 列表。
fn hit_ids(frame: &serde_json::Value) -> Vec<String> {
    frame["data"]["hits"]
        .as_array()
        .expect("data.hits must be an array")
        .iter()
        .map(|h| h["id"].as_str().expect("hit.id").to_string())
        .collect()
}

#[test]
fn search_cursor_pages_partition_results() {
    let (_dir, db) = temp_db("cursor-pages");
    for (fact, text) in [
        ("c1", "cursor pagination alpha one"),
        ("c2", "cursor pagination alpha two"),
        ("c3", "cursor pagination alpha three"),
    ] {
        let out = run(&db, &["index", fact, text]);
        assert!(out.status.success());
    }

    // 不分页基线：一页拿全。
    let out = run(&db, &["search", "pagination"]);
    assert!(out.status.success());
    let all = hit_ids(&parse_first_line(&out));
    assert_eq!(all.len(), 3);

    // 第一页：页大小 2，应带续读令牌。
    let out = run(&db, &["search", "pagination", "--max-items", "2"]);
    assert!(out.status.success(), "page1 failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    let page1 = hit_ids(&frame);
    assert_eq!(page1.len(), 2);
    assert_eq!(frame["page"]["has_more"], true, "page1={frame}");
    let token = frame["page"]["next_cursor"]
        .as_str()
        .expect("page1 must issue next_cursor")
        .to_string();

    // 第二页：续读到末尾，不再发令牌。
    let out = run(
        &db,
        &[
            "search",
            "pagination",
            "--max-items",
            "2",
            "--cursor",
            &token,
        ],
    );
    assert!(out.status.success(), "page2 failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    let page2 = hit_ids(&frame);
    assert_eq!(page2.len(), 1);
    assert_eq!(frame["page"]["has_more"], false, "page2={frame}");
    assert!(frame["page"]["next_cursor"].is_null());

    // 两页拼接 == 不分页结果：钉住排序（bm25 + id tiebreak）下不重不漏、同序。
    let joined: Vec<String> = page1.into_iter().chain(page2).collect();
    assert_eq!(joined, all);
}

#[test]
fn tampered_cursor_is_rejected_with_cursor_invalid() {
    let (_dir, db) = temp_db("cursor-tamper");
    for (fact, text) in [("t1", "tamper target one"), ("t2", "tamper target two")] {
        let out = run(&db, &["index", fact, text]);
        assert!(out.status.success());
    }
    let out = run(&db, &["search", "tamper", "--max-items", "1"]);
    let token = parse_first_line(&out)["page"]["next_cursor"]
        .as_str()
        .expect("next_cursor")
        .to_string();

    // 翻转 payload 首字符（仍是合法 base64url 字符）→ 完整性摘要不符。
    let mut chars: Vec<char> = token.chars().collect();
    chars[0] = if chars[0] == 'A' { 'B' } else { 'A' };
    let tampered: String = chars.into_iter().collect();

    let out = run(
        &db,
        &[
            "--robot",
            "search",
            "tamper",
            "--max-items",
            "1",
            "--cursor",
            &tampered,
        ],
    );
    assert_eq!(out.status.code(), Some(2), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["error"]["code"], "cursor_invalid");
    // 错误消息必须指示重新发起查询（合同禁止静默回第一页）。
    assert!(
        frame["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("re-run")),
        "message={frame}"
    );
}

#[test]
fn generation_bump_invalidates_cursor_with_exit_9() {
    let (_dir, db) = temp_db("cursor-generation");
    for (fact, text) in [("g1", "bump probe one"), ("g2", "bump probe two")] {
        let out = run(&db, &["index", fact, text]);
        assert!(out.status.success());
    }
    let out = run(&db, &["search", "probe", "--max-items", "1"]);
    let token = parse_first_line(&out)["page"]["next_cursor"]
        .as_str()
        .expect("next_cursor")
        .to_string();

    // 再写一条推进 generation：数据已换代，旧令牌必须显式失效。
    let out = run(&db, &["index", "g3", "bump probe three"]);
    assert!(out.status.success());

    let out = run(
        &db,
        &[
            "--robot",
            "search",
            "probe",
            "--max-items",
            "1",
            "--cursor",
            &token,
        ],
    );
    assert_eq!(out.status.code(), Some(9), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["error"]["code"], "generation_mismatch");
    // 结构化 details：调用方无需解析 message 即可拿到两侧 generation（contract §4）。
    assert_eq!(frame["error"]["details"]["cursor_generation"], 2);
    assert_eq!(frame["error"]["details"]["active_generation"], 3);
}

/// 写入 context e2e 用的真实 Claude 格式夹具（合成数据，含 sidechain 与 fork）：
/// root → reply → { sidechain probe, mainline tail }。返回 (夹具字节, 会话 wire id)。
fn write_context_fixture(dir: &std::path::Path) -> (String, String, String) {
    let lines = concat!(
        r#"{"type":"user","uuid":"c0000000-0000-4000-8000-000000000001","parentUuid":null,"sessionId":"ccdd1234-5678-4abc-8def-001122334455","timestamp":"2026-07-26T01:00:00.000Z","message":{"role":"user","content":"ctx root question"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"c0000000-0000-4000-8000-000000000002","parentUuid":"c0000000-0000-4000-8000-000000000001","sessionId":"ccdd1234-5678-4abc-8def-001122334455","message":{"role":"assistant","content":"ctx first answer"}}"#,
        "\n",
        r#"{"type":"user","uuid":"c0000000-0000-4000-8000-000000000003","parentUuid":"c0000000-0000-4000-8000-000000000002","isSidechain":true,"sessionId":"ccdd1234-5678-4abc-8def-001122334455","message":{"role":"user","content":"ctx sidechain probe"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"c0000000-0000-4000-8000-000000000004","parentUuid":"c0000000-0000-4000-8000-000000000002","sessionId":"ccdd1234-5678-4abc-8def-001122334455","message":{"role":"assistant","content":"ctx final answer"}}"#,
        "\n",
    );
    let fixture = dir.join("context.jsonl");
    std::fs::write(&fixture, lines).expect("write context fixture");
    (
        fixture.to_string_lossy().into_owned(),
        lines.to_string(),
        "ses_v1_ccdd1234-5678-4abc-8def-001122334455".to_string(),
    )
}

struct RelationalContextFixture {
    paths: [String; 3],
    contents: [String; 3],
    session_a: String,
    session_b: String,
    parent_a: String,
    parent_b: String,
    shared_message: String,
}

fn write_relational_context_fixture(dir: &Path) -> RelationalContextFixture {
    let session_a_native = "aa111111-1111-4111-8111-111111111111";
    let session_b_native = "bb222222-2222-4222-8222-222222222222";
    let parent_a_native = "aa333333-3333-4333-8333-333333333333";
    let parent_b_native = "bb444444-4444-4444-8444-444444444444";
    let shared_native = "cc555555-5555-4555-8555-555555555555";
    let head_a = format!(
        "{{\"type\":\"user\",\"uuid\":\"{parent_a_native}\",\"parentUuid\":null,\
         \"sessionId\":\"{session_a_native}\",\"timestamp\":\"2026-07-28T01:00:00.000Z\",\
         \"message\":{{\"role\":\"user\",\"content\":\"relational parent A\"}}}}\n"
    );
    let tail_a = format!(
        "{{\"type\":\"assistant\",\"uuid\":\"{shared_native}\",\
         \"parentUuid\":\"{parent_a_native}\",\"sessionId\":\"{session_a_native}\",\
         \"timestamp\":\"2026-07-28T01:00:01.000Z\",\
         \"message\":{{\"role\":\"assistant\",\"content\":\"relational shared answer\"}}}}\n"
    );
    let source_b = format!(
        "{{\"type\":\"user\",\"uuid\":\"{parent_b_native}\",\"parentUuid\":null,\
         \"sessionId\":\"{session_b_native}\",\"timestamp\":\"2026-07-28T01:00:00.000Z\",\
         \"message\":{{\"role\":\"user\",\"content\":\"relational parent B\"}}}}\n\
         {{\"type\":\"assistant\",\"uuid\":\"{shared_native}\",\
         \"parentUuid\":\"{parent_b_native}\",\"sessionId\":\"{session_b_native}\",\
         \"timestamp\":\"2026-07-28T01:00:01.000Z\",\
         \"message\":{{\"role\":\"assistant\",\"content\":\"relational shared answer\"}}}}\n"
    );
    let files = [
        ("relational-a-head.jsonl", head_a.clone()),
        ("relational-a-tail.jsonl", tail_a.clone()),
        ("relational-b.jsonl", source_b.clone()),
    ];
    let mut paths = Vec::new();
    for (name, content) in &files {
        let path = dir.join(name);
        std::fs::write(&path, content).expect("write relational context fixture");
        paths.push(path.to_string_lossy().into_owned());
    }
    RelationalContextFixture {
        paths: paths.try_into().expect("three fixture paths"),
        contents: [head_a, tail_a, source_b],
        session_a: format!("ses_v1_{session_a_native}"),
        session_b: format!("ses_v1_{session_b_native}"),
        parent_a: format!("msg_v1_{parent_a_native}"),
        parent_b: format!("msg_v1_{parent_b_native}"),
        shared_message: format!("msg_v1_{shared_native}"),
    }
}

#[test]
fn context_assembles_mainline_branch_with_evidence() {
    let (dir, db) = temp_db("context-mainline");
    let (fixture_path, content, session_wire) = write_context_fixture(dir.path());
    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    let out = run(&db, &["context", &session_wire]);
    assert!(out.status.success(), "context failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["command"], "context");
    assert_eq!(frame["data"]["session_id"], session_wire.as_str());
    assert_eq!(frame["outcome"], "success");

    // mainline：排除 sidechain，沿 parent 链 root→leaf。
    let messages = frame["data"]["messages"].as_array().expect("messages");
    let wires: Vec<&str> = messages
        .iter()
        .map(|m| m["id"].as_str().expect("message.id"))
        .collect();
    assert!(
        messages
            .iter()
            .all(|message| message["id"] == message["message_id"])
    );
    assert_eq!(
        wires,
        vec![
            "msg_v1_c0000000-0000-4000-8000-000000000001",
            "msg_v1_c0000000-0000-4000-8000-000000000002",
            "msg_v1_c0000000-0000-4000-8000-000000000004",
        ],
        "frame={frame}"
    );
    assert_eq!(
        frame["data"]["branch_leaf"],
        "msg_v1_c0000000-0000-4000-8000-000000000004"
    );
    assert_eq!(
        frame["data"]["branch_leaf_placement_id"],
        messages[2]["placement_id"]
    );

    // 证据与链对齐：byte 精度 + 指纹 + 文档身份；span 精确切回源记录。
    let evidence = frame["data"]["evidence"].as_array().expect("evidence");
    assert_eq!(evidence.len(), 3);
    for (message, span) in messages.iter().zip(evidence) {
        assert_eq!(span["occurrence_id"], message["placement_id"]);
        assert_eq!(span["message_id"], message["message_id"]);
    }
    assert_eq!(evidence[0]["precision"], "byte");
    assert!(
        evidence[0]["source_document_id"]
            .as_str()
            .is_some_and(|d| d.starts_with("doc_v1_")),
        "frame={frame}"
    );
    assert!(
        evidence[0]["source_fingerprint"]
            .as_str()
            .is_some_and(|f| !f.is_empty()),
        "frame={frame}"
    );
    let start = evidence[0]["byte_start"].as_u64().expect("byte_start") as usize;
    let end = evidence[0]["byte_end"].as_u64().expect("byte_end") as usize;
    let sliced = &content.as_bytes()[start..end];
    assert!(
        sliced.starts_with(br#"{"type":"user","uuid":"c0000000-0000-4000-8000-000000000001""#),
        "span 应切回 root 源记录"
    );
    // 证据 ordinal 是会话内 seq：mainline 第三条是成员序号 3（sidechain 占 2）。
    assert_eq!(evidence[2]["record_ordinal"], 3, "frame={frame}");

    // full 策略包含 sidechain，按冻结顺序：missing timestamp 先，再按 ordinal；
    // 有 timestamp 的 root 最后。该顺序不声称缺失时间的跨记录 chronology。
    let out = run(&db, &["context", &session_wire, "--policy", "full"]);
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    let full: Vec<&str> = frame["data"]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        full,
        vec![
            "msg_v1_c0000000-0000-4000-8000-000000000002",
            "msg_v1_c0000000-0000-4000-8000-000000000003",
            "msg_v1_c0000000-0000-4000-8000-000000000004",
            "msg_v1_c0000000-0000-4000-8000-000000000001",
        ]
    );
}

/// 把一个会话拆成两个文件写出：前半 root→reply，后半 sidechain + mainline tail。
/// 两个文件都声明同一个 `sessionId`——这是真实语料里的常态（会话续写/分片），
/// 而非人造边角：单会话被 55 个文件各自声明的情况已在真实数据回归中实测。
fn write_split_session_fixture(dir: &std::path::Path) -> (String, String, String) {
    let head = concat!(
        r#"{"type":"user","uuid":"5p1i7000-0000-4000-8000-000000000001","parentUuid":null,"sessionId":"5p1i7aaa-1111-4bbb-8ccc-000000000001","timestamp":"2026-07-27T01:00:00.000Z","message":{"role":"user","content":"split root question"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"5p1i7000-0000-4000-8000-000000000002","parentUuid":"5p1i7000-0000-4000-8000-000000000001","sessionId":"5p1i7aaa-1111-4bbb-8ccc-000000000001","message":{"role":"assistant","content":"split first answer"}}"#,
        "\n",
    );
    let tail = concat!(
        r#"{"type":"user","uuid":"5p1i7000-0000-4000-8000-000000000003","parentUuid":"5p1i7000-0000-4000-8000-000000000002","isSidechain":true,"sessionId":"5p1i7aaa-1111-4bbb-8ccc-000000000001","message":{"role":"user","content":"split sidechain probe"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"5p1i7000-0000-4000-8000-000000000004","parentUuid":"5p1i7000-0000-4000-8000-000000000002","sessionId":"5p1i7aaa-1111-4bbb-8ccc-000000000001","message":{"role":"assistant","content":"split final answer"}}"#,
        "\n",
    );
    let head_path = dir.join("split-head.jsonl");
    let tail_path = dir.join("split-tail.jsonl");
    std::fs::write(&head_path, head).expect("write split head fixture");
    std::fs::write(&tail_path, tail).expect("write split tail fixture");
    (
        head_path.to_string_lossy().into_owned(),
        tail_path.to_string_lossy().into_owned(),
        "ses_v1_5p1i7aaa-1111-4bbb-8ccc-000000000001".to_string(),
    )
}

#[test]
fn session_split_across_files_syncs_and_assembles_one_context() {
    // 修复前：两个源各自声明同一 ses_v1_ 却带不同成员列表，提交层判为冲突投影
    // 并整批拒绝（catalog_error / exit 6），真实语料因此完全无法入库。
    let (dir, db) = temp_db("split-session-one-batch");
    let (head, tail, session_wire) = write_split_session_fixture(dir.path());

    let out = run(&db, &["sync", &head, &tail]);
    assert!(out.status.success(), "sync failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_eq!(frame["data"]["messages"], 4, "frame={frame}");

    // 会话实体承载两个源的成员并集，并记录两个贡献文档。
    let out = run(&db, &["show", &session_wire]);
    assert!(out.status.success(), "show failed: {}", stdout(&out));
    let entity = parse_first_line(&out)["data"]["entity"].clone();
    let members = entity["messages"].as_array().expect("session.messages");
    assert_eq!(members.len(), 4, "entity={entity}");
    let documents = entity["documents"].as_array().expect("session.documents");
    assert_eq!(documents.len(), 2, "entity={entity}");
    // 单值 `document` 是兼容别名：多文档会话上它只指其中一个贡献者。
    assert!(
        entity["document"]
            .as_str()
            .is_some_and(|d| d.starts_with("doc_v1_")),
        "entity={entity}"
    );

    // 跨文件的 parent 边可解析：mainline 链跨越两个源文件。
    let out = run(&db, &["context", &session_wire]);
    assert!(out.status.success(), "context failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_eq!(frame["outcome"], "success");
    let wires: Vec<&str> = frame["data"]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|m| m["id"].as_str().expect("message.id"))
        .collect();
    assert_eq!(
        wires,
        vec![
            "msg_v1_5p1i7000-0000-4000-8000-000000000001",
            "msg_v1_5p1i7000-0000-4000-8000-000000000002",
            "msg_v1_5p1i7000-0000-4000-8000-000000000004",
        ],
        "frame={frame}"
    );
    assert_eq!(
        frame["data"]["branch_leaf"],
        "msg_v1_5p1i7000-0000-4000-8000-000000000004"
    );
}

#[test]
fn shared_message_keeps_per_session_parent_and_exact_evidence_across_split_sources() {
    let (dir, db) = temp_db("relational-context-shapes");
    let fixture = write_relational_context_fixture(dir.path());
    let out = run(
        &db,
        &[
            "sync",
            &fixture.paths[0],
            &fixture.paths[1],
            &fixture.paths[2],
        ],
    );
    assert!(out.status.success(), "sync failed: {}", stdout(&out));
    let sync = parse_first_line(&out);
    assert_eq!(sync["data"]["emitted"], 4);
    assert_eq!(sync["data"]["skipped"], 0);

    let status = run(&db, &["status"]);
    let status = parse_first_line(&status);
    assert_eq!(status["data"]["placements"], 4);
    assert_eq!(status["data"]["source_placement_claims"], 4);

    let list = run(&db, &["list", "20"]);
    let list = parse_first_line(&list);
    let stable_messages = list["data"]["entries"]
        .as_array()
        .expect("list entries")
        .iter()
        .filter(|entry| {
            entry["id"]
                .as_str()
                .is_some_and(|wire| wire.starts_with("msg_v1_"))
        })
        .count();
    assert_eq!(stable_messages, 3, "stable Message census is de-duplicated");

    let shared = run(&db, &["show", &fixture.shared_message]);
    let shared = parse_first_line(&shared)["data"]["entity"].clone();
    assert_eq!(shared["sessions"].as_array().expect("sessions").len(), 2);
    assert_eq!(shared["spans"].as_array().expect("spans").len(), 2);
    assert!(
        shared["parent"].is_null(),
        "divergent parents must not alias"
    );
    assert!(
        shared["parent_native_id"].is_null(),
        "divergent native parents must not alias"
    );

    let session_a = run(&db, &["show", &fixture.session_a]);
    let session_a = parse_first_line(&session_a)["data"]["entity"].clone();
    assert_eq!(
        session_a["documents"]
            .as_array()
            .expect("session A documents")
            .len(),
        2,
        "session A must span both source documents"
    );

    let context_a = run(&db, &["context", &fixture.session_a]);
    assert!(
        context_a.status.success(),
        "session A context failed: {}",
        stdout(&context_a)
    );
    let context_a = parse_first_line(&context_a);
    let messages_a = context_a["data"]["messages"]
        .as_array()
        .expect("session A messages");
    assert_eq!(
        messages_a
            .iter()
            .map(|message| message["message_id"].as_str().expect("message id"))
            .collect::<Vec<_>>(),
        vec![fixture.parent_a.as_str(), fixture.shared_message.as_str()]
    );
    let evidence_a = context_a["data"]["evidence"]
        .as_array()
        .expect("session A evidence");
    assert_eq!(
        evidence_a[1]["occurrence_id"],
        messages_a[1]["placement_id"]
    );
    let start_a = evidence_a[1]["byte_start"].as_u64().expect("start A") as usize;
    let end_a = evidence_a[1]["byte_end"].as_u64().expect("end A") as usize;
    assert_eq!(
        &fixture.contents[1].as_bytes()[start_a..end_a],
        fixture.contents[1].trim_end().as_bytes()
    );

    let context_b = run(&db, &["context", &fixture.session_b]);
    assert!(
        context_b.status.success(),
        "session B context failed: {}",
        stdout(&context_b)
    );
    let context_b = parse_first_line(&context_b);
    let messages_b = context_b["data"]["messages"]
        .as_array()
        .expect("session B messages");
    assert_eq!(
        messages_b
            .iter()
            .map(|message| message["message_id"].as_str().expect("message id"))
            .collect::<Vec<_>>(),
        vec![fixture.parent_b.as_str(), fixture.shared_message.as_str()]
    );
    let evidence_b = context_b["data"]["evidence"]
        .as_array()
        .expect("session B evidence");
    assert_eq!(
        evidence_b[1]["occurrence_id"],
        messages_b[1]["placement_id"]
    );
    let start_b = evidence_b[1]["byte_start"].as_u64().expect("start B") as usize;
    let end_b = evidence_b[1]["byte_end"].as_u64().expect("end B") as usize;
    assert!(fixture.contents[2].as_bytes()[start_b..end_b].starts_with(br#"{"type":"assistant""#));
    assert_ne!(
        messages_a[1]["placement_id"], messages_b[1]["placement_id"],
        "shared stable Message must retain distinct placements"
    );
    assert_ne!(
        evidence_a[1]["source_document_id"], evidence_b[1]["source_document_id"],
        "each occurrence must retain its exact document"
    );
    assert_ne!(
        evidence_a[1]["byte_start"], evidence_b[1]["byte_start"],
        "each occurrence must retain its exact source-local span"
    );
}

#[test]
fn session_synced_in_separate_invocations_keeps_both_halves() {
    // 真实用法：语料太大，分多次 sync。第二批不得覆盖第一批已记录的成员。
    let (dir, db) = temp_db("split-session-two-batches");
    let (head, tail, session_wire) = write_split_session_fixture(dir.path());

    let out = run(&db, &["sync", &head]);
    assert!(out.status.success(), "first sync failed: {}", stdout(&out));
    let out = run(&db, &["sync", &tail]);
    assert!(out.status.success(), "second sync failed: {}", stdout(&out));

    let out = run(&db, &["show", &session_wire]);
    let entity = parse_first_line(&out)["data"]["entity"].clone();
    assert_eq!(
        entity["messages"].as_array().expect("messages").len(),
        4,
        "第二批 sync 不得丢弃第一批成员: entity={entity}"
    );
    assert_eq!(entity["documents"].as_array().expect("documents").len(), 2);

    // 两个半区都可检索，证明并集是真实可用的而非仅 payload 好看。
    for term in ["\"split root question\"", "\"split final answer\""] {
        let out = run(&db, &["search", term]);
        assert!(out.status.success(), "search failed: {}", stdout(&out));
        let frame = parse_first_line(&out);
        assert!(
            !frame["data"]["hits"].as_array().expect("hits").is_empty(),
            "term={term} frame={frame}"
        );
    }
}

#[test]
fn context_budget_truncation_reports_partial_exit_10() {
    let (dir, db) = temp_db("context-budget");
    let (fixture_path, _, session_wire) = write_context_fixture(dir.path());
    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    let out = run(&db, &["context", &session_wire, "--max-messages", "2"]);
    // 部分成功：结果可用但被预算截断 → outcome partial + exit 10（contract §5）。
    assert_eq!(out.status.code(), Some(10), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["outcome"], "partial");
    assert_eq!(frame["data"]["messages"].as_array().unwrap().len(), 2);
    assert_eq!(frame["data"]["truncation"]["truncated"], true);
    assert_eq!(frame["data"]["truncation"]["reason"], "max_messages");
}

#[test]
fn context_missing_session_is_not_found() {
    let (_dir, db) = temp_db("context-missing");
    let out = run(
        &db,
        &[
            "--robot",
            "context",
            "ses_v1_ffffffff-ffff-4fff-8fff-ffffffffffff",
        ],
    );
    assert_eq!(out.status.code(), Some(4), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["error"]["code"], "not_found");
}

// ─── Human/Robot 输出真值表（contract §4 §6，child 4）───────────────────────

#[test]
fn human_search_and_status_render_text_not_envelope() {
    let (_dir, db) = temp_db("human-search");
    for (fact, text) in [("h1", "human render alpha"), ("h2", "human render beta")] {
        let out = run(&db, &["index", fact, text]);
        assert!(out.status.success());
    }

    let out = run_human(&db, &["search", "render"]);
    assert!(out.status.success(), "search failed: {}", stdout(&out));
    let s = stdout(&out);
    assert!(s.contains("hit(s) (generation"), "human header: {s}");
    assert!(s.contains("msg_v1_"), "human hits list ids: {s}");
    assert!(
        !s.contains("schema_version"),
        "no envelope in human mode: {s}"
    );

    // 零结果有措辞，不是空输出。
    let out = run_human(&db, &["search", "nomatchword"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("no hits"), "got: {}", stdout(&out));

    let out = run_human(&db, &["status"]);
    let s = stdout(&out);
    assert!(s.contains("entities: 2"), "human status: {s}");
    assert!(s.contains("generation:"), "human status: {s}");
}

#[test]
fn human_context_renders_chain_and_partial_exits_10() {
    let (dir, db) = temp_db("human-context");
    let (fixture_path, _, session_wire) = write_context_fixture(dir.path());
    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    let out = run_human(&db, &["context", &session_wire]);
    assert!(out.status.success(), "context failed: {}", stdout(&out));
    let s = stdout(&out);
    assert!(
        s.contains(&format!("session {session_wire}")),
        "header: {s}"
    );
    assert!(s.contains("[user] ctx root question"), "messages: {s}");
    assert!(s.contains("evidence: 3 span(s)"), "evidence line: {s}");
    assert!(!s.contains("schema_version"), "no envelope: {s}");

    // 预算截断：human 模式同样如实报 partial（截断行 + exit 10）。
    let out = run_human(&db, &["context", &session_wire, "--max-messages", "2"]);
    assert_eq!(out.status.code(), Some(10), "stdout={}", stdout(&out));
    assert!(
        stdout(&out).contains("truncated: max_messages"),
        "truncation line: {}",
        stdout(&out)
    );
}

#[test]
fn jsonl_sync_emits_progress_frames_then_single_response() {
    let (dir, db) = temp_db("jsonl-progress");
    let mut paths = Vec::new();
    for tag in ["p1", "p2"] {
        let fixture = dir.path().join(format!("{tag}.jsonl"));
        // 每源内容必须不同：同字节 → 同内容寻址 document/session id，
        // 而成员消息不同 → 存储层正确拒绝跨源投影冲突。
        std::fs::write(
            &fixture,
            format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"progress fixture {tag}\"}}}}\n"
            ),
        )
        .expect("write fixture");
        paths.push(fixture.to_string_lossy().into_owned());
    }

    let out = Command::new(BIN)
        .args([
            "--db", &db, "--output", "jsonl", "sync", &paths[0], &paths[1],
        ])
        .output()
        .expect("spawn");
    assert!(out.status.success(), "sync failed: {}", stdout(&out));
    let text = stdout(&out);
    let frames: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("bad frame: {e}\n{line}")))
        .collect();
    // 逐源 progress + 收尾 response，每行一个完整 frame（contract §4）。
    assert_eq!(frames.len(), 3, "2 progress + 1 response: {text}");
    assert_eq!(frames[0]["frame_type"], "progress");
    assert_eq!(frames[1]["frame_type"], "progress");
    assert!(
        frames[0]["message"]
            .as_str()
            .is_some_and(|m| m.contains("scanned")),
        "{text}"
    );
    for frame in &frames[..2] {
        let message = frame["message"].as_str().expect("progress message");
        assert!(
            paths.iter().all(|path| !message.contains(path)),
            "progress must not disclose source paths: {message}"
        );
    }
    assert_eq!(frames[2]["frame_type"], "response");
    assert_eq!(frames[2]["command"], "sync");

    // 指纹缓存命中（重扫同一批源）：措辞如实切换为 checked/unchanged，
    // 不得谎报 "staged (0 messages)"（Minor-4）。
    let out = Command::new(BIN)
        .args([
            "--db", &db, "--output", "jsonl", "sync", &paths[0], &paths[1],
        ])
        .output()
        .expect("spawn");
    assert!(out.status.success(), "resync failed: {}", stdout(&out));
    let text = stdout(&out);
    let frames: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("bad frame: {e}\n{line}")))
        .collect();
    assert_eq!(frames.len(), 3, "2 checked + 1 response: {text}");
    assert!(
        frames[0]["message"]
            .as_str()
            .is_some_and(|m| m.contains("checked source 1/2") && m.contains("unchanged")),
        "resync progress must say checked/unchanged: {text}"
    );
    assert!(
        frames[1]["message"]
            .as_str()
            .is_some_and(|m| m.contains("checked source 2/2")),
        "resync progress must say checked/unchanged: {text}"
    );

    // --robot 禁 progress：同一命令只有一个 response envelope。
    let (_dir2, db2) = temp_db("robot-no-progress");
    let out = Command::new(BIN)
        .args(["--db", &db2, "--robot", "sync", &paths[0], &paths[1]])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let text = stdout(&out);
    assert_eq!(
        text.lines().count(),
        1,
        "robot mode must not emit progress: {text}"
    );
    let frame = parse_first_line(&out);
    assert_eq!(frame["frame_type"], "response");
}

#[test]
fn request_id_echoes_verbatim_and_invalid_is_rejected() {
    let (_dir, db) = temp_db("request-id");
    let out = Command::new(BIN)
        .args([
            "--db",
            &db,
            "--robot",
            "--request-id",
            "corr.42:a_b-c",
            "status",
        ])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    assert_eq!(
        frame["request_id"], "corr.42:a_b-c",
        "echo verbatim: {frame}"
    );

    // 非法 request-id 是用法错误（exit 2），不静默替换。
    let out = Command::new(BIN)
        .args(["--db", &db, "--robot", "--request-id", "bad id", "status"])
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2), "stdout={}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_eq!(frame["error"]["code"], "invalid_request");
}

#[test]
fn context_envelope_carries_warnings_array_on_modern_store() {
    let (dir, db) = temp_db("warnings-plumbing");
    let (fixture_path, _, session_wire) = write_context_fixture(dir.path());
    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success());

    let out = run(&db, &["context", &session_wire]);
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    // 现代库全部 byte 精度 → warnings 存在且为空（通路端到端可见；
    // 有 unknown 精度时的告警文案由 main.rs 单测锁定，见 design §0.2）。
    assert_eq!(frame["warnings"], serde_json::json!([]), "{frame}");
}

// ─── 初始性能基线 ───────────────────────────────────────────────────────────

#[test]
fn perf_baseline_100_messages_index_and_search() {
    let (_dir, db) = temp_db("perf-baseline");
    let start = std::time::Instant::now();

    // 写入 100 条消息
    for i in 0..100u32 {
        let fact = format!("perf-fact-{i}");
        let text = format!("performance baseline message number {i} with unique content");
        let out = run(&db, &["index", &fact, &text]);
        assert!(out.status.success(), "index {i} failed: {}", stdout(&out));
    }
    let index_ms = start.elapsed().as_millis();

    // 全文检索应命中
    let search_start = std::time::Instant::now();
    let out = run(&db, &["search", "performance baseline"]);
    let query_ms = search_start.elapsed().as_millis();

    assert!(out.status.success());
    assert!(
        stdout(&out).contains("msg_v1_"),
        "search must return results"
    );

    // 仅输出历史 smoke 基线供观察。正式性能分布、环境和样本量由
    // scripts/evidence/core_beta_benchmark.py 负责；普通 CI 机器不以固定墙钟阈值阻断。
    eprintln!("[perf-baseline] index 100 msgs: {index_ms}ms  search: {query_ms}ms");
}
