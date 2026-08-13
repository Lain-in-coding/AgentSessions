//! MCP stdio e2e：驱动真实二进制的 `mcp` 子命令，覆盖 design §4 的 9 个必测场景
//! （initialize 协商 → tools/list → tools/call 往返、错误分层、initialize gate）。
//!
//! stdout 纯净性断言固化在会话 helper 里：MCP 模式下 stdout 只允许合法 JSON-RPC
//! frame（contract §6/§8），任何解析失败的行当场 panic。响应按 JSON-RPC id 匹配，
//! 不按行位置——通知不产生响应帧。

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// 刚构建出的 `agent-session-grep` 二进制的绝对路径（由 Cargo 在编译期注入）。
const BIN: &str = env!("CARGO_BIN_EXE_agent-session-grep");

/// 每个测试用独立临时目录，避免 WAL/SHM 旁文件互相干扰。
fn temp_db(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("{tag}.db"));
    (dir, path)
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// 以 robot 模式跑一次 CLI：仅用于测试前置的数据准备（ingest 夹具），不涉 MCP。
fn run_cli(db: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("--db")
        .arg(db)
        .arg("--robot")
        .args(args)
        .output()
        .expect("failed to spawn agent-session-grep binary")
}

/// 跑一个完整 MCP stdio 会话：逐行写入 → 关 stdin（EOF）→ 收全 stdout。
///
/// 两条契约在此执行：EOF 后服务器必须干净停机 exit 0（design §0.8）；
/// stdout 每一行都必须是完整 JSON frame，解析失败当场 panic（诊断只准走 stderr）。
fn mcp_session_raw(db: &Path, lines: &[&str]) -> Vec<Value> {
    mcp_session_raw_stderr(db, lines).0
}

/// 同 [`mcp_session_raw`]，额外返回 stderr 全文（隐私回归守卫用）。
fn mcp_session_raw_stderr(db: &Path, lines: &[&str]) -> (Vec<Value>, String) {
    let mut child = Command::new(BIN)
        .arg("--db")
        .arg(db)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn agent-session-grep mcp");
    {
        let mut stdin = child.stdin.take().expect("child stdin must be piped");
        for line in lines {
            stdin.write_all(line.as_bytes()).expect("write stdin line");
            stdin.write_all(b"\n").expect("write stdin newline");
        }
        // 作用域结束丢弃 stdin → EOF，服务器应据此优雅停机。
    }
    let out = child.wait_with_output().expect("wait for mcp server");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "mcp server must exit 0 on EOF, got {:?}\nstderr: {stderr}",
        out.status.code()
    );
    let text = stdout(&out);
    let frames = text
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!("stdout not pure JSON-RPC: {error}\nline: {line}\nstderr: {stderr}")
            })
        })
        .collect();
    (frames, stderr)
}

/// design §4 冻结的 helper：以 JSON Value 逐条喂入一个 MCP 会话。
fn mcp_session(db: &Path, inputs: &[Value]) -> Vec<Value> {
    let lines: Vec<String> = inputs.iter().map(|input| input.to_string()).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    mcp_session_raw(db, &refs)
}

/// 按 JSON-RPC id 取响应帧：通知没有响应，按位置对齐不可靠。
fn frame_by_id(frames: &[Value], id: i64) -> &Value {
    frames
        .iter()
        .find(|frame| frame["id"] == json!(id))
        .unwrap_or_else(|| panic!("no response frame with id {id}, got: {frames:?}"))
}

/// initialize 请求（协商目标版本由用例指定）。
fn initialize_request(id: i64, version: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" }
        }
    })
}

/// initialized 通知：发出后 initialize gate 才放行其余请求（design §0.7）。
fn initialized_notification() -> Value {
    json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
}

fn tool_call(id: i64, name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    })
}

/// 写入 context 用的真实 Claude 格式夹具（合成数据，与 e2e.rs 同形状）：
/// root → reply → { sidechain probe, mainline tail }，全部消息含检索词 "ctx"。
/// 返回 (夹具路径, 会话 wire id)。
fn write_context_fixture(dir: &Path) -> (String, String) {
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
    let session_a_native = "da111111-1111-4111-8111-111111111111";
    let session_b_native = "db222222-2222-4222-8222-222222222222";
    let parent_a_native = "da333333-3333-4333-8333-333333333333";
    let parent_b_native = "db444444-4444-4444-8444-444444444444";
    let shared_native = "dc555555-5555-4555-8555-555555555555";
    let head_a = format!(
        "{{\"type\":\"user\",\"uuid\":\"{parent_a_native}\",\"parentUuid\":null,\
         \"sessionId\":\"{session_a_native}\",\"timestamp\":\"2026-07-28T02:00:00.000Z\",\
         \"message\":{{\"role\":\"user\",\"content\":\"mcp relational parent A\"}}}}\n"
    );
    let tail_a = format!(
        "{{\"type\":\"assistant\",\"uuid\":\"{shared_native}\",\
         \"parentUuid\":\"{parent_a_native}\",\"sessionId\":\"{session_a_native}\",\
         \"timestamp\":\"2026-07-28T02:00:01.000Z\",\
         \"message\":{{\"role\":\"assistant\",\"content\":\"mcp relational shared\"}}}}\n"
    );
    let source_b = format!(
        "{{\"type\":\"user\",\"uuid\":\"{parent_b_native}\",\"parentUuid\":null,\
         \"sessionId\":\"{session_b_native}\",\"timestamp\":\"2026-07-28T02:00:00.000Z\",\
         \"message\":{{\"role\":\"user\",\"content\":\"mcp relational parent B\"}}}}\n\
         {{\"type\":\"assistant\",\"uuid\":\"{shared_native}\",\
         \"parentUuid\":\"{parent_b_native}\",\"sessionId\":\"{session_b_native}\",\
         \"timestamp\":\"2026-07-28T02:00:01.000Z\",\
         \"message\":{{\"role\":\"assistant\",\"content\":\"mcp relational shared\"}}}}\n"
    );
    let files = [
        ("mcp-relational-a-head.jsonl", head_a.clone()),
        ("mcp-relational-a-tail.jsonl", tail_a.clone()),
        ("mcp-relational-b.jsonl", source_b.clone()),
    ];
    let mut paths = Vec::new();
    for (name, content) in &files {
        let path = dir.join(name);
        std::fs::write(&path, content).expect("write MCP relational fixture");
        paths.push(path.to_string_lossy().into_owned());
    }
    RelationalContextFixture {
        paths: paths.try_into().expect("three MCP fixture paths"),
        contents: [head_a, tail_a, source_b],
        session_a: format!("ses_v1_{session_a_native}"),
        session_b: format!("ses_v1_{session_b_native}"),
        parent_a: format!("msg_v1_{parent_a_native}"),
        parent_b: format!("msg_v1_{parent_b_native}"),
        shared_message: format!("msg_v1_{shared_native}"),
    }
}

// ─── design §4 场景 1：initialize 握手与版本协商 ────────────────────────────

#[test]
fn initialize_negotiates_versions_honestly_and_ping_answers() {
    let (_dir, db) = temp_db("mcp-init");
    // 客户端请求受支持的旧版本 → 原样回显（version negotiation 不撒谎）。
    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2024-11-05"),
            initialized_notification(),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
        ],
    );
    assert_eq!(frames.len(), 2, "initialize + ping 各一帧: {frames:?}");
    let init = frame_by_id(&frames, 1);
    assert_eq!(init["jsonrpc"], "2.0");
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(init["result"]["serverInfo"]["name"], "agent-session-grep");
    assert!(
        init["result"]["serverInfo"]["version"]
            .as_str()
            .is_some_and(|version| !version.is_empty()),
        "serverInfo.version 必须存在: {init}"
    );
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "capabilities 必须声明 tools: {init}"
    );
    // ping 在任何阶段都可答，result 为空对象。
    assert_eq!(frame_by_id(&frames, 2)["result"], json!({}));

    // 不支持的版本 → 回落到我们钉住的最新版，绝不假装支持对方版本。
    let (_dir2, db2) = temp_db("mcp-init-unsupported");
    let frames = mcp_session(&db2, &[initialize_request(1, "9999-01-01")]);
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(
        frame_by_id(&frames, 1)["result"]["protocolVersion"],
        "2025-06-18"
    );
}

// ─── design §4 场景 2：tools/list 固定工具集 ────────────────────────────────

#[test]
fn tools_list_exposes_exactly_six_contract_tools() {
    let (_dir, db) = temp_db("mcp-tools");
    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        ],
    );
    assert_eq!(frames.len(), 2, "{frames:?}");
    let tools = frame_by_id(&frames, 2)["result"]["tools"]
        .as_array()
        .expect("result.tools must be an array");
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool.name"))
        .collect();
    assert_eq!(
        names,
        vec![
            "search_sessions",
            "get_session_context",
            "list_sessions",
            "list_providers",
            "get_status",
            "doctor",
        ],
        "contract §8 固定的工具集与顺序"
    );
    for tool in tools {
        assert!(
            tool["inputSchema"].is_object(),
            "每个工具必须发布 inputSchema: {tool}"
        );
        assert!(
            tool["description"].as_str().is_some_and(|d| !d.is_empty()),
            "每个工具必须自带描述: {tool}"
        );
    }
}

// ─── design §4 场景 3：search_sessions 分页（复用 ADT cursor）───────────────

#[test]
fn search_sessions_cursor_pages_are_disjoint() {
    let (dir, db) = temp_db("mcp-search");
    let (fixture_path, _session_wire) = write_context_fixture(dir.path());
    let out = run_cli(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    // 第一页：夹具 4 条消息全含 "ctx"，页大小 1 → 必发续读令牌。
    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                2,
                "search_sessions",
                json!({ "query": "ctx", "max_items": 1 }),
            ),
        ],
    );
    assert_eq!(frames.len(), 2, "{frames:?}");
    let result = &frame_by_id(&frames, 2)["result"];
    assert_eq!(result["isError"], false, "search 应成功: {result}");
    let payload = &result["structuredContent"];
    assert_eq!(payload["outcome"], "success");
    let hits = payload["data"]["hits"].as_array().expect("data.hits");
    assert_eq!(hits.len(), 1, "页大小 1: {payload}");
    let first_id = hits[0]["id"].as_str().expect("hit.id").to_string();
    assert_eq!(payload["page"]["has_more"], true, "{payload}");
    let cursor = payload["page"]["next_cursor"]
        .as_str()
        .expect("第一页必须签发 next_cursor")
        .to_string();

    // 第二页：cursor 是无状态签名令牌，跨会话（新进程）依然有效；页间不重叠
    // （复用 ADT 分页的钉住排序，MCP 层不得另造分页规则）。
    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                2,
                "search_sessions",
                json!({ "query": "ctx", "max_items": 1, "cursor": cursor.as_str() }),
            ),
        ],
    );
    assert_eq!(frames.len(), 2, "{frames:?}");
    let payload = &frame_by_id(&frames, 2)["result"]["structuredContent"];
    let hits = payload["data"]["hits"].as_array().expect("data.hits");
    assert_eq!(hits.len(), 1, "{payload}");
    let second_id = hits[0]["id"].as_str().expect("hit.id");
    assert_ne!(first_id, second_id, "分页必须不重不漏: {payload}");
}

// ─── design §4 场景 4：get_session_context 真实夹具往返 ─────────────────────

#[test]
fn get_session_context_returns_mainline_messages_and_evidence() {
    let (dir, db) = temp_db("mcp-context");
    let (fixture_path, session_wire) = write_context_fixture(dir.path());
    let out = run_cli(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                2,
                "get_session_context",
                json!({ "session_id": session_wire.as_str() }),
            ),
        ],
    );
    assert_eq!(frames.len(), 2, "{frames:?}");
    let result = &frame_by_id(&frames, 2)["result"];
    assert_eq!(result["isError"], false, "context 应成功: {result}");
    let payload = &result["structuredContent"];
    assert_eq!(payload["outcome"], "success");
    assert_eq!(payload["data"]["session_id"], session_wire.as_str());
    // 默认 mainline 策略：排除 sidechain → root/reply/tail 共 3 条。
    let messages = payload["data"]["messages"]
        .as_array()
        .expect("data.messages");
    assert_eq!(messages.len(), 3, "{payload}");
    // 证据数组与消息链对齐（byte 精度夹具 → 每条消息一个 span）。
    let evidence = payload["data"]["evidence"]
        .as_array()
        .expect("data.evidence");
    assert_eq!(evidence.len(), 3, "{payload}");
}

#[test]
fn get_session_context_keeps_shared_message_parent_and_evidence_per_session() {
    let (dir, db) = temp_db("mcp-relational-context");
    let fixture = write_relational_context_fixture(dir.path());
    let out = run_cli(
        &db,
        &[
            "sync",
            &fixture.paths[0],
            &fixture.paths[1],
            &fixture.paths[2],
        ],
    );
    assert!(out.status.success(), "sync failed: {}", stdout(&out));

    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                2,
                "get_session_context",
                json!({ "session_id": fixture.session_a }),
            ),
            tool_call(
                3,
                "get_session_context",
                json!({ "session_id": fixture.session_b }),
            ),
        ],
    );
    assert_eq!(frames.len(), 3, "{frames:?}");

    let mut shared_placements = Vec::new();
    let mut shared_documents = Vec::new();
    let cases = [
        (2, fixture.parent_a.as_str(), 1usize),
        (3, fixture.parent_b.as_str(), 2usize),
    ];
    for (id, expected_parent, content_index) in cases {
        let result = &frame_by_id(&frames, id)["result"];
        assert_eq!(result["isError"], false, "{result}");
        let payload = &result["structuredContent"];
        let messages = payload["data"]["messages"]
            .as_array()
            .expect("context messages");
        assert_eq!(
            messages
                .iter()
                .map(|message| message["message_id"].as_str().expect("message id"))
                .collect::<Vec<_>>(),
            vec![expected_parent, fixture.shared_message.as_str()]
        );
        let evidence = payload["data"]["evidence"]
            .as_array()
            .expect("context evidence");
        assert_eq!(evidence[1]["occurrence_id"], messages[1]["placement_id"]);
        let start = evidence[1]["byte_start"].as_u64().expect("byte start") as usize;
        let end = evidence[1]["byte_end"].as_u64().expect("byte end") as usize;
        assert!(
            fixture.contents[content_index].as_bytes()[start..end]
                .starts_with(br#"{"type":"assistant""#)
        );
        shared_placements.push(
            messages[1]["placement_id"]
                .as_str()
                .expect("placement id")
                .to_string(),
        );
        shared_documents.push(
            evidence[1]["source_document_id"]
                .as_str()
                .expect("document id")
                .to_string(),
        );
    }
    assert_ne!(shared_placements[0], shared_placements[1]);
    assert_ne!(shared_documents[0], shared_documents[1]);
}

// ─── design §4 场景 5：坏 cursor 是业务错误（isError 结果帧）────────────────

#[test]
fn garbage_cursor_is_business_error_cursor_invalid() {
    let (_dir, db) = temp_db("mcp-bad-cursor");
    // 通过结构校验但业务层失败（App 拒绝 cursor）→ isError 结果帧，
    // 不是 JSON-RPC error（design §0.3 错误分层）。
    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                2,
                "search_sessions",
                json!({ "query": "x", "cursor": "garbage" }),
            ),
        ],
    );
    assert_eq!(frames.len(), 2, "{frames:?}");
    let frame = frame_by_id(&frames, 2);
    assert!(frame["error"].is_null(), "应为 result 帧: {frame}");
    let result = &frame["result"];
    assert_eq!(result["isError"], true, "{result}");
    let error = &result["structuredContent"]["error"];
    assert_eq!(error["canonical_code"], "cursor_invalid");
    assert_eq!(error["retryable"], false);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "content[0] 必须携带人读文本: {result}"
    );
}

// ─── design §4 场景 6：协议层错误的 JSON-RPC code 映射 ──────────────────────

#[test]
fn protocol_errors_map_to_json_rpc_codes() {
    // 未知工具 → -32602；未知方法 → -32601（同一已握手会话内验证）。
    let (_dir, db) = temp_db("mcp-protocol-errors");
    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(10, "does_not_exist", json!({})),
            json!({ "jsonrpc": "2.0", "id": 11, "method": "foo/bar" }),
        ],
    );
    assert_eq!(frames.len(), 3, "{frames:?}");
    let unknown_tool = frame_by_id(&frames, 10);
    assert!(unknown_tool["result"].is_null(), "{unknown_tool}");
    assert_eq!(unknown_tool["error"]["code"], -32602);
    assert_eq!(frame_by_id(&frames, 11)["error"]["code"], -32601);

    // 畸形 JSON 行 → -32700 且 id:null（parse error 无从关联请求 id）。
    let (_dir2, db2) = temp_db("mcp-malformed");
    let frames = mcp_session_raw(&db2, &["not json"]);
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["error"]["code"], -32700);
    assert!(frames[0]["id"].is_null(), "{:?}", frames[0]);

    // JSON-RPC batch 数组：2025-06-18 已移除 batching → 整体 -32600 拒绝。
    let (_dir3, db3) = temp_db("mcp-batch");
    let frames = mcp_session(
        &db3,
        &[json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping" }])],
    );
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["error"]["code"], -32600);
}

// ─── design §4 场景 7：initialize gate ──────────────────────────────────────

#[test]
fn requests_before_initialized_are_rejected() {
    let (_dir, db) = temp_db("mcp-gate");
    // 首条消息即 tools/list（未 initialize）→ -32600 server not initialized。
    let frames = mcp_session(
        &db,
        &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })],
    );
    assert_eq!(frames.len(), 1, "{frames:?}");
    let frame = frame_by_id(&frames, 1);
    assert!(frame["result"].is_null(), "{frame}");
    assert_eq!(frame["error"]["code"], -32600);
}

// ─── design §4 场景 8：status / doctor / list_providers 真实数据 ────────────

#[test]
fn status_doctor_and_providers_return_real_data() {
    let (dir, db) = temp_db("mcp-status");
    let (fixture_path, _session_wire) = write_context_fixture(dir.path());
    let out = run_cli(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(2, "get_status", json!({})),
            tool_call(3, "doctor", json!({})),
            tool_call(4, "list_providers", json!({})),
        ],
    );
    assert_eq!(frames.len(), 4, "{frames:?}");

    // get_status：入库后 catalog 非空（4 消息 + 会话 + 文档）。
    let status = &frame_by_id(&frames, 2)["result"]["structuredContent"];
    assert_eq!(status["outcome"], "success");
    let catalog_count = status["data"]["catalog_count"]
        .as_u64()
        .expect("data.catalog_count");
    assert!(catalog_count >= 1, "{status}");

    // doctor：真实打开的库 → db:ok，干净库无待收敛 intent。
    let doctor = &frame_by_id(&frames, 3)["result"]["structuredContent"];
    assert_eq!(doctor["data"]["db"], "ok", "{doctor}");
    assert!(doctor["data"]["generation"].is_number(), "{doctor}");
    assert_eq!(doctor["data"]["interrupted_batches"], 0, "{doctor}");

    // list_providers：真实枚举组合根注册表，两个已支持 provider 必在。
    let providers = &frame_by_id(&frames, 4)["result"]["structuredContent"];
    let ids: Vec<&str> = providers["data"]["providers"]
        .as_array()
        .expect("data.providers")
        .iter()
        .map(|provider| provider["id"].as_str().expect("provider.id"))
        .collect();
    assert!(ids.contains(&"claude-code"), "{ids:?}");
    assert!(ids.contains(&"codex"), "{ids:?}");
}

// ─── design §4 场景 9：参数值域校验失败 → -32602 + canonical data ───────────

#[test]
fn invalid_session_id_param_is_protocol_error() {
    let (_dir, db) = temp_db("mcp-invalid-params");
    // 参数结构合法但值域校验失败（非法 wire id）→ 协议层 -32602，
    // error.data 携带 canonical 映射（design §2 注记）。
    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                2,
                "get_session_context",
                json!({ "session_id": "not-a-wire-id" }),
            ),
        ],
    );
    assert_eq!(frames.len(), 2, "{frames:?}");
    let frame = frame_by_id(&frames, 2);
    assert!(frame["result"].is_null(), "{frame}");
    assert_eq!(frame["error"]["code"], -32602);
    assert_eq!(frame["error"]["data"]["canonical_code"], "invalid_request");
    assert_eq!(frame["error"]["data"]["retryable"], false);
}

// ─── 分层一致性（Minor-7，E5）：list_sessions limit:0 与 search 同层拒绝 ──────

#[test]
fn list_sessions_zero_limit_is_protocol_error_like_search() {
    // list_sessions 的 limit:0 必须在协议层回 -32602，与 search_sessions 一致；
    // 不得漏到 App 层变成 isError 业务帧（分层一致性）。
    let (_dir, db) = temp_db("mcp-list-limit");
    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(2, "list_sessions", json!({ "limit": 0 })),
            tool_call(3, "list_sessions", json!({ "max_items": 0 })),
        ],
    );
    assert_eq!(frames.len(), 3, "{frames:?}");
    for id in [2, 3] {
        let frame = frame_by_id(&frames, id);
        assert!(frame["result"].is_null(), "{frame}");
        assert_eq!(frame["error"]["code"], -32602, "{frame}");
        assert_eq!(frame["error"]["data"]["canonical_code"], "invalid_request");
    }
    // 对照：search_sessions 的 limit:0 同样 -32602（分层一致性的基准）。
    let frames = mcp_session(
        &db,
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(2, "search_sessions", json!({ "query": "x", "limit": 0 })),
        ],
    );
    assert_eq!(frame_by_id(&frames, 2)["error"]["code"], -32602);
}

// ─── stderr 隐私（E2）：协议错误不落 stderr ─────────────────────────────────

#[test]
fn mcp_stderr_stays_clean_on_protocol_errors() {
    // stderr 是进程级诊断通道，正常协议交互（含错误帧）下必须为空或至少
    // 不含 db 路径——隐私回归守卫。
    let (_dir, db) = temp_db("mcp-stderr-privacy");
    let db_str = db.to_string_lossy().into_owned();

    // 坏 JSON → -32700 帧；stderr 不得出现 db 路径。
    let (frames, stderr) = mcp_session_raw_stderr(&db, &["{ not json"]);
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["error"]["code"], -32700);
    assert!(!stderr.contains(&db_str), "stderr 泄露 db 路径: {stderr}");

    // 未握手请求 → -32600；stderr 保持为空。
    let uninitialized =
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }).to_string();
    let (_, stderr) = mcp_session_raw_stderr(&db, &[&uninitialized]);
    assert!(stderr.is_empty(), "协议错误不得写入 stderr: {stderr}");

    // 工具参数错误 → -32602；stderr 保持为空。
    let (_, stderr) = mcp_session_raw_stderr(
        &db,
        &[
            &initialize_request(1, "2025-06-18").to_string(),
            &initialized_notification().to_string(),
            &tool_call(2, "list_sessions", json!({ "limit": 0 })).to_string(),
        ],
    );
    assert!(stderr.is_empty(), "参数错误不得写入 stderr: {stderr}");
}
