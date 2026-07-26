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

/// 刚构建出的 `agentsessions` 二进制的绝对路径（由 Cargo 在编译期注入）。
const BIN: &str = env!("CARGO_BIN_EXE_agentsessions");

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
        .expect("failed to spawn agentsessions binary")
}

/// 跑一个完整 MCP stdio 会话：逐行写入 → 关 stdin（EOF）→ 收全 stdout。
///
/// 两条契约在此执行：EOF 后服务器必须干净停机 exit 0（design §0.8）；
/// stdout 每一行都必须是完整 JSON frame，解析失败当场 panic（诊断只准走 stderr）。
fn mcp_session_raw(db: &Path, lines: &[&str]) -> Vec<Value> {
    let mut child = Command::new(BIN)
        .arg("--db")
        .arg(db)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn agentsessions mcp");
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
    text.lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!("stdout not pure JSON-RPC: {error}\nline: {line}\nstderr: {stderr}")
            })
        })
        .collect()
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
    assert_eq!(init["result"]["serverInfo"]["name"], "agentsessions");
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
